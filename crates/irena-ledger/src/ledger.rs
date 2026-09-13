//! Publishing records to a ledger and resolving what is in force.

use crate::error::LedgerError;
use bornite_rules::VotingRulesV1;
use irena_core::{
    CompanyGenesisV1, CompanyIdV1, IrenaRecordV1, NotarisationV1, RECORD_SCHEMA_VERSION,
    RecordBodyV1, RecordKindV1, ShareStructureV1, compose_record, read_record,
};
use prunella_core::{
    BlockHeight, GenesisSpec, Namespace, NetworkId, PublicKey, SchemaVersion, Transaction,
    TransactionDraft, TxId,
};
use prunella_crypto::SigningKey;
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

/// A value together with the ledger record it came from.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct InForceV1<T> {
    /// The value in force.
    pub value: T,
    /// The transaction it came from.
    pub tx_id: TxId,
    /// The block it was committed in.
    pub height: BlockHeight,
    /// Who attested to it, and when.
    pub notarisation: NotarisationV1,
    /// The record it amended, if any.
    pub supersedes: Option<TxId>,
}

/// Everything the ledger says a company is at one height.
///
/// All three records resolved together at the same height, each with where it came
/// from. A vote freezes one of these; a reader shows one of these.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CompanyStateV1 {
    /// Which company.
    pub company: CompanyIdV1,
    /// The height everything was resolved at.
    pub at: BlockHeight,
    /// The founding record in force.
    pub genesis: InForceV1<CompanyGenesisV1>,
    /// The share register in force.
    pub shares: InForceV1<ShareStructureV1>,
    /// The voting rules in force.
    pub rules: InForceV1<VotingRulesV1>,
}

fn transaction_for(
    key: &SigningKey,
    kind: RecordKindV1,
    payload: String,
    nonce: u64,
) -> Result<Transaction, LedgerError> {
    Ok(key.sign_transaction(TransactionDraft {
        namespace: Namespace::new(kind.namespace())?,
        schema_version: SchemaVersion(RECORD_SCHEMA_VERSION),
        payload: payload.into_bytes(),
        signer: key.public_key(),
        nonce,
    }))
}

/// Builds a genesis specification whose genesis block carries the company's founding
/// record.
///
/// The company exists from height 0, and two parties given the same inputs derive the
/// same genesis hash.
///
/// # Errors
///
/// Returns [`LedgerError::Record`] if the genesis body or notarisation is not valid.
pub fn genesis_with_company(
    network: NetworkId,
    key: &SigningKey,
    company: &CompanyIdV1,
    genesis_xml: &str,
    notarisation: &NotarisationV1,
    timestamp_millis: u64,
) -> Result<GenesisSpec, LedgerError> {
    let payload = compose_record(
        RecordKindV1::CompanyGenesis,
        company,
        None,
        notarisation,
        genesis_xml,
    )?;
    let transaction = transaction_for(key, RecordKindV1::CompanyGenesis, payload, 0)?;
    Ok(GenesisSpec {
        network_id: network,
        timestamp_millis,
        transactions: vec![transaction],
    })
}

/// Publishes a record in its own block.
///
/// `supersedes` must name the record of this kind currently in force for `company`,
/// or be `None` when there is none. Anything else is a stale amendment and is refused
/// before the ledger is touched. The body is validated and embedded verbatim
/// (`irena_core::compose_record`).
///
/// # Errors
///
/// Returns [`LedgerError::StaleAmendment`], [`LedgerError::Record`] for an invalid
/// body or notarisation, or a ledger error.
#[allow(clippy::too_many_arguments)]
pub fn publish(
    store: &LocalChainStore,
    key: &SigningKey,
    company: &CompanyIdV1,
    kind: RecordKindV1,
    body_xml: &str,
    supersedes: Option<TxId>,
    notarisation: &NotarisationV1,
    timestamp_millis: u64,
) -> Result<RecordRefV1, LedgerError> {
    let head = store.head()?;
    let current = in_force_ref(store, company, kind, head.height)?;
    let expected = current.as_ref().map(|r| r.tx_id);
    if expected != supersedes {
        return Err(LedgerError::StaleAmendment {
            company: company.to_string(),
            kind,
            expected: render(expected),
            found: render(supersedes),
        });
    }

    let payload = compose_record(kind, company, supersedes, notarisation, body_xml)?;
    let parent = store
        .get_block(head.height)?
        .ok_or_else(|| LedgerError::Malformed {
            detail: format!("the chain head is {head} but no block is stored there"),
        })?;
    let height = head.height.next()?;
    let transaction = transaction_for(key, kind, payload, height.value())?;
    let tx_id = transaction.id;
    let block = parent
        .header
        .child_draft(vec![transaction], timestamp_millis)?
        .build()?;
    store.append_block(block)?;

    let committed = store
        .get_transaction(&tx_id)?
        .ok_or_else(|| LedgerError::Malformed {
            detail: format!("transaction {tx_id} was appended but cannot be read back"),
        })?;
    let record = read_record(
        core::str::from_utf8(&committed.transaction.payload).map_err(|error| {
            LedgerError::Malformed {
                detail: format!("composed record is not UTF-8: {error}"),
            }
        })?,
    )?;
    Ok(RecordRefV1 {
        height,
        index: committed.index,
        tx_id,
        signer: key.public_key(),
        record,
    })
}

fn render(id: Option<TxId>) -> String {
    id.map_or_else(|| "none".to_owned(), |id| id.to_string())
}

/// Every record of `kind` for `company`, in ledger order, up to and including `at`.
///
/// The amendment chain is checked as it is walked: each record must supersede the one
/// before it, and the first must supersede nothing. A break is an error naming the
/// exact record, never something to skip past. So is a transaction in the kind's
/// namespace that does not hold a readable record of that kind.
///
/// # Errors
///
/// Returns [`LedgerError::BrokenAmendmentChain`], [`LedgerError::UnreadableRecord`] or
/// a ledger error.
pub fn history(
    store: &LocalChainStore,
    company: &CompanyIdV1,
    kind: RecordKindV1,
    at: BlockHeight,
) -> Result<Vec<RecordRefV1>, LedgerError> {
    let head = store.head()?;
    let end = at.min(head.height);
    let mut records: Vec<RecordRefV1> = Vec::new();
    for block in store.iter_blocks(BlockHeight::GENESIS, end)? {
        let block = block?;
        let height = block.header.height;
        for (index, transaction) in block.transactions.iter().enumerate() {
            if transaction.namespace.as_str() != kind.namespace() {
                continue;
            }
            let unreadable = |detail: String| LedgerError::UnreadableRecord {
                height,
                tx_id: transaction.id,
                namespace: transaction.namespace.to_string(),
                kind,
                detail,
            };
            let text = core::str::from_utf8(&transaction.payload)
                .map_err(|error| unreadable(format!("payload is not UTF-8: {error}")))?;
            let record = read_record(text).map_err(|error| unreadable(error.to_string()))?;
            if record.kind() != kind {
                return Err(unreadable(format!(
                    "the record is a {} record",
                    record.kind()
                )));
            }
            if &record.company != company {
                continue;
            }
            let expected = records.last().map(|r| r.tx_id);
            if record.supersedes != expected {
                return Err(LedgerError::BrokenAmendmentChain {
                    company: company.to_string(),
                    kind,
                    height,
                    tx_id: transaction.id,
                    expected: render(expected),
                    found: render(record.supersedes),
                });
            }
            records.push(RecordRefV1 {
                height,
                index: u32::try_from(index).map_err(|_| LedgerError::Malformed {
                    detail: format!("block {height} has more transactions than fit in u32"),
                })?,
                tx_id: transaction.id,
                signer: transaction.signer,
                record,
            });
        }
    }
    Ok(records)
}

fn in_force_ref(
    store: &LocalChainStore,
    company: &CompanyIdV1,
    kind: RecordKindV1,
    at: BlockHeight,
) -> Result<Option<RecordRefV1>, LedgerError> {
    Ok(history(store, company, kind, at)?.pop())
}

/// The record of `kind` in force for `company` at height `at`.
///
/// # Errors
///
/// Returns [`LedgerError::NothingInForce`] if no such record exists by that height, or
/// the errors of [`history`].
pub fn in_force(
    store: &LocalChainStore,
    company: &CompanyIdV1,
    kind: RecordKindV1,
    at: BlockHeight,
) -> Result<InForceV1<RecordBodyV1>, LedgerError> {
    let found =
        in_force_ref(store, company, kind, at)?.ok_or_else(|| LedgerError::NothingInForce {
            company: company.to_string(),
            kind,
            at,
        })?;
    let IrenaRecordV1 {
        supersedes,
        notarisation,
        body,
        ..
    } = found.record;
    Ok(InForceV1 {
        value: body,
        tx_id: found.tx_id,
        height: found.height,
        notarisation,
        supersedes,
    })
}

/// The founding record in force for `company` at `at`.
///
/// # Errors
///
/// As [`in_force`].
pub fn genesis_in_force(
    store: &LocalChainStore,
    company: &CompanyIdV1,
    at: BlockHeight,
) -> Result<InForceV1<CompanyGenesisV1>, LedgerError> {
    let found = in_force(store, company, RecordKindV1::CompanyGenesis, at)?;
    Ok(map_in_force(found, |body| match body {
        RecordBodyV1::CompanyGenesis(value) => Some(value),
        _ => None,
    }))
}

/// The share register in force for `company` at `at`.
///
/// # Errors
///
/// As [`in_force`].
pub fn shares_in_force(
    store: &LocalChainStore,
    company: &CompanyIdV1,
    at: BlockHeight,
) -> Result<InForceV1<ShareStructureV1>, LedgerError> {
    let found = in_force(store, company, RecordKindV1::ShareStructure, at)?;
    Ok(map_in_force(found, |body| match body {
        RecordBodyV1::ShareStructure(value) => Some(value),
        _ => None,
    }))
}

/// The voting rules in force for `company` at `at`.
///
/// # Errors
///
/// As [`in_force`].
pub fn rules_in_force(
    store: &LocalChainStore,
    company: &CompanyIdV1,
    at: BlockHeight,
) -> Result<InForceV1<VotingRulesV1>, LedgerError> {
    let found = in_force(store, company, RecordKindV1::VotingRules, at)?;
    Ok(map_in_force(found, |body| match body {
        RecordBodyV1::VotingRules(value) => Some(value),
        _ => None,
    }))
}

fn map_in_force<T>(
    found: InForceV1<RecordBodyV1>,
    extract: impl FnOnce(RecordBodyV1) -> Option<T>,
) -> InForceV1<T> {
    InForceV1 {
        value: extract(found.value).expect("filtered by kind"),
        tx_id: found.tx_id,
        height: found.height,
        notarisation: found.notarisation,
        supersedes: found.supersedes,
    }
}

/// What `company` is at height `at`: genesis, share register and voting rules, all in
/// force at that same height.
///
/// A company with any of the three missing is not yet a company that can vote, and
/// this says which record is missing rather than returning a partial state.
///
/// # Errors
///
/// [`LedgerError::NothingInForce`] naming the first missing kind, or the errors of
/// [`history`].
pub fn company_at(
    store: &LocalChainStore,
    company: &CompanyIdV1,
    at: BlockHeight,
) -> Result<CompanyStateV1, LedgerError> {
    Ok(CompanyStateV1 {
        company: company.clone(),
        at,
        genesis: genesis_in_force(store, company, at)?,
        shares: shares_in_force(store, company, at)?,
        rules: rules_in_force(store, company, at)?,
    })
}
