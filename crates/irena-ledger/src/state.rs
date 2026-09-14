//! Deterministic reconstruction of the company from the chain.
//!
//! The company at height `h` is its genesis record plus every amendment up to `h`,
//! applied in chain order. Block order is total and transaction order within a block
//! is fixed, so two instances holding the same chain reconstruct the same state at
//! every height, and the state at a past height never changes because of anything
//! appended since. Nothing is cached, indexed or repaired: the chain is read, and what
//! it says is what the company is.

use crate::authority::{authorised_signer, lockout_after};
use crate::error::LedgerError;
use irena_core::{
    AuthorisationV1, CompanyIdV1, DecisionChannelsV1, IdentitiesV1, IdentityV1, IrenaRecordV1,
    NotarisationV1, RecordBodyV1, RecordFamilyV1, RecordKindV1, ShareStructureV1, read_record,
};
use prunella_core::{BlockHeight, PublicKey, TxId};
use prunella_store::LocalChainStore;

/// A record as found on the ledger.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RecordRefV1 {
    /// The block it was committed in.
    pub height: BlockHeight,
    /// Its position in that block.
    pub index: u32,
    /// The transaction that carries it.
    pub tx_id: TxId,
    /// Who signed the transaction.
    ///
    /// The signer put the record on the chain; the notary inside the record is who
    /// vouched for it. They may be the same key holder and need not be.
    pub signer: PublicKey,
    /// The record itself.
    pub record: IrenaRecordV1,
}

/// A part of the company together with the record that currently provides it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct InForceV1<T> {
    /// The value in force.
    pub value: T,
    /// The transaction it came from: the genesis, or the last amendment of this part.
    pub tx_id: TxId,
    /// The block it was committed in.
    pub height: BlockHeight,
    /// Who attested to it, and when.
    pub notarisation: NotarisationV1,
    /// The record it amended, if any.
    pub supersedes: Option<TxId>,
}

impl<T> InForceV1<T> {
    fn from_record<U>(found: &RecordRefV1, value: T, _: &U) -> Self {
        Self {
            value,
            tx_id: found.tx_id,
            height: found.height,
            notarisation: found.record.notarisation.clone(),
            supersedes: found.record.supersedes,
        }
    }
}

/// The company as reconstructed at one height.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CompanyStateV1 {
    /// Which company. One per chain.
    pub company: CompanyIdV1,
    /// The height everything was reconstructed at.
    pub at: BlockHeight,
    /// The transaction that founded the company.
    pub genesis_tx_id: TxId,
    /// The block the company was founded in.
    pub genesis_height: BlockHeight,
    /// Who the company is.
    pub identity: InForceV1<IdentityV1>,
    /// The share register.
    pub shares: InForceV1<ShareStructureV1>,
    /// The active governance configuration: who decides, and how.
    pub channels: InForceV1<DecisionChannelsV1>,
    /// The persons and the key each currently signs with.
    pub identities: InForceV1<IdentitiesV1>,
    /// Who may sign which family of record transaction.
    pub authorisation: InForceV1<AuthorisationV1>,
    /// Every Irena record applied, in chain order, the genesis first.
    pub applied: Vec<RecordRefV1>,
}

impl CompanyStateV1 {
    /// The transaction currently providing a part.
    #[must_use]
    pub fn provider_of(&self, kind: RecordKindV1) -> TxId {
        match kind {
            RecordKindV1::CompanyGenesis => self.genesis_tx_id,
            RecordKindV1::Identity => self.identity.tx_id,
            RecordKindV1::ShareStructure => self.shares.tx_id,
            RecordKindV1::DecisionChannels => self.channels.tx_id,
            RecordKindV1::Identities => self.identities.tx_id,
            RecordKindV1::Authorisation => self.authorisation.tx_id,
        }
    }

    /// The records that have provided a part, in chain order: the genesis, then every
    /// amendment of that part.
    #[must_use]
    pub fn history_of(&self, kind: RecordKindV1) -> Vec<&RecordRefV1> {
        self.applied
            .iter()
            .filter(|found| {
                found.record.kind() == RecordKindV1::CompanyGenesis || found.record.kind() == kind
            })
            .collect()
    }
}

/// Reconstructs the company at height `at`.
///
/// Walks every block from genesis to `at` (or the head, if lower). The first Irena
/// record must be a `company-genesis`; every later Irena record must name the same
/// company, be an amendment, be signed by a `company` signer's key under the company
/// as it was before the record, supersede exactly the transaction currently providing
/// the part it amends, and leave at least one `company` signer holding a key. Anything
/// else can only have been written around this crate, and stops reconstruction with an
/// error naming the exact transaction. Nothing is skipped and nothing is repaired.
///
/// Transactions in other namespaces are not Irena's and are ignored.
///
/// # Errors
///
/// [`LedgerError::NoCompany`] if no genesis is found by `at`;
/// [`LedgerError::UnreadableRecord`], [`LedgerError::SecondGenesis`],
/// [`LedgerError::ForeignCompany`], [`LedgerError::UnauthorisedRecord`],
/// [`LedgerError::BrokenAmendmentChain`], [`LedgerError::Lockout`] as described; or a
/// ledger error.
pub fn reconstruct(
    store: &LocalChainStore,
    at: BlockHeight,
) -> Result<CompanyStateV1, LedgerError> {
    let head = store.head()?;
    let end = at.min(head.height);
    let mut state: Option<CompanyStateV1> = None;

    for block in store.iter_blocks(BlockHeight::GENESIS, end)? {
        let block = block?;
        let height = block.header.height;
        for (index, transaction) in block.transactions.iter().enumerate() {
            let Some(kind) = kind_of_namespace(transaction.namespace.as_str()) else {
                continue;
            };
            let unreadable = |detail: String| LedgerError::UnreadableRecord {
                height,
                tx_id: transaction.id,
                namespace: transaction.namespace.to_string(),
                detail,
            };
            let text = core::str::from_utf8(&transaction.payload)
                .map_err(|error| unreadable(format!("payload is not UTF-8: {error}")))?;
            let record = read_record(text).map_err(|error| unreadable(error.to_string()))?;
            if record.kind().namespace() != kind {
                return Err(unreadable(format!(
                    "a {} record does not belong in the {} namespace",
                    record.kind(),
                    transaction.namespace
                )));
            }
            let found = RecordRefV1 {
                height,
                index: u32::try_from(index).map_err(|_| LedgerError::Malformed {
                    detail: format!("block {height} has more transactions than fit in u32"),
                })?,
                tx_id: transaction.id,
                signer: transaction.signer,
                record,
            };
            state = Some(match state.take() {
                None => found_company(found)?,
                Some(current) => apply(current, found)?,
            });
        }
    }

    let mut state = state.ok_or(LedgerError::NoCompany { at: end })?;
    state.at = at;
    Ok(state)
}

/// The Irena namespace a transaction is in, if any.
fn kind_of_namespace(namespace: &str) -> Option<&'static str> {
    RecordKindV1::ALL
        .into_iter()
        .map(RecordKindV1::namespace)
        .find(|candidate| *candidate == namespace)
}

/// The first Irena record on the chain must found the company.
fn found_company(found: RecordRefV1) -> Result<CompanyStateV1, LedgerError> {
    let RecordBodyV1::CompanyGenesis(genesis) = &found.record.body else {
        return Err(LedgerError::NoGenesisFirst {
            height: found.height,
            tx_id: found.tx_id,
            kind: found.record.kind(),
        });
    };
    if let Some(supersedes) = found.record.supersedes {
        return Err(LedgerError::BrokenAmendmentChain {
            company: found.record.company.to_string(),
            kind: RecordKindV1::CompanyGenesis,
            height: found.height,
            tx_id: found.tx_id,
            expected: "none".to_owned(),
            found: supersedes.to_string(),
        });
    }
    // The genesis signer is whoever founded the chain; the authorisation inside the
    // genesis applies from the next record on. What the genesis must not do is found a
    // company nobody can amend.
    if let Some(detail) = lockout_after(&genesis.identities, &genesis.authorisation) {
        return Err(LedgerError::Lockout {
            height: found.height,
            tx_id: found.tx_id,
            detail,
        });
    }
    let genesis = genesis.clone();
    Ok(CompanyStateV1 {
        company: found.record.company.clone(),
        at: found.height,
        genesis_tx_id: found.tx_id,
        genesis_height: found.height,
        identity: InForceV1::from_record(&found, genesis.identity, &()),
        shares: InForceV1::from_record(&found, genesis.shares, &()),
        channels: InForceV1::from_record(&found, genesis.channels, &()),
        identities: InForceV1::from_record(&found, genesis.identities, &()),
        authorisation: InForceV1::from_record(&found, genesis.authorisation, &()),
        applied: vec![found],
    })
}

/// Applies one amendment to the company.
fn apply(mut state: CompanyStateV1, found: RecordRefV1) -> Result<CompanyStateV1, LedgerError> {
    if found.record.company != state.company {
        return Err(LedgerError::ForeignCompany {
            expected: state.company.to_string(),
            found: found.record.company.to_string(),
            height: found.height,
            tx_id: found.tx_id,
        });
    }
    let kind = found.record.kind();
    if kind == RecordKindV1::CompanyGenesis {
        return Err(LedgerError::SecondGenesis {
            first: state.genesis_tx_id,
            height: found.height,
            tx_id: found.tx_id,
        });
    }
    // The company as it was before this record says who may write it.
    if let Err(LedgerError::UnauthorisedSigner { detail, .. }) =
        authorised_signer(&state, RecordFamilyV1::Company, &found.signer)
    {
        return Err(LedgerError::UnauthorisedRecord {
            height: found.height,
            tx_id: found.tx_id,
            kind,
            signer: found.signer,
            detail,
        });
    }
    let expected = state.provider_of(kind);
    if found.record.supersedes != Some(expected) {
        return Err(LedgerError::BrokenAmendmentChain {
            company: state.company.to_string(),
            kind,
            height: found.height,
            tx_id: found.tx_id,
            expected: expected.to_string(),
            found: found
                .record
                .supersedes
                .map_or_else(|| "none".to_owned(), |id| id.to_string()),
        });
    }
    match &found.record.body {
        RecordBodyV1::Identity(value) => {
            state.identity = InForceV1::from_record(&found, value.clone(), &());
        }
        RecordBodyV1::ShareStructure(value) => {
            state.shares = InForceV1::from_record(&found, value.clone(), &());
        }
        RecordBodyV1::DecisionChannels(value) => {
            state.channels = InForceV1::from_record(&found, value.clone(), &());
        }
        RecordBodyV1::Identities(value) => {
            state.identities = InForceV1::from_record(&found, value.clone(), &());
        }
        RecordBodyV1::Authorisation(value) => {
            state.authorisation = InForceV1::from_record(&found, value.clone(), &());
        }
        RecordBodyV1::CompanyGenesis(_) => unreachable!("refused above"),
    }
    if let Some(detail) = lockout_after(&state.identities.value, &state.authorisation.value) {
        return Err(LedgerError::Lockout {
            height: found.height,
            tx_id: found.tx_id,
            detail,
        });
    }
    state.applied.push(found);
    Ok(state)
}
