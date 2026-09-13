//! Publishing records to a ledger and resolving what is in force.
//!
//! Every operation is deterministic over the chain's contents: block order is total and
//! transaction order within a block is fixed, so two instances holding the same chain
//! resolve the same record for any subject at any height.

use crate::error::BridgeError;
use crate::record::{GovernanceRecordV1, NotarisationV1, RecordBodyV1, RecordKindV1, SubjectV1};
use crate::xml::{compose_record, read_record};
use bornite_core::{BallotSetV1, ElectorateV1};
use bornite_eval::VoteEvaluationV1;
use bornite_rules::VotingRulesV1;
use prunella_core::{
    BlockHeight, GenesisSpec, Namespace, NetworkId, PublicKey, SchemaVersion, Transaction,
    TransactionDraft, TxId,
};
use prunella_crypto::SigningKey;
use prunella_store::LocalChainStore;

/// The schema version every governance transaction declares.
pub const RECORD_SCHEMA_VERSION: u32 = 1;

/// A record as found on the ledger.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RecordRefV1 {
    /// The block it was committed in.
    pub height: BlockHeight,
    /// Its position in that block.
    pub index: u32,
    /// The transaction that carries it.
    pub tx_id: TxId,
    /// Who signed it.
    pub signer: PublicKey,
    /// The record itself.
    pub record: GovernanceRecordV1,
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
    /// Who attested to it.
    pub notarisation: Option<NotarisationV1>,
    /// The record it amended, if any.
    pub supersedes: Option<TxId>,
}

/// An evaluation together with the ledger records it used.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct EvaluationAtV1 {
    /// The height the rules and roll were resolved at.
    pub at: BlockHeight,
    /// The rules used.
    pub rules: InForceV1<VotingRulesV1>,
    /// The roll used.
    pub roll: InForceV1<ElectorateV1>,
    /// The result.
    pub evaluation: VoteEvaluationV1,
}

fn transaction_for(
    key: &SigningKey,
    kind: RecordKindV1,
    payload: String,
    nonce: u64,
) -> Result<Transaction, BridgeError> {
    Ok(key.sign_transaction(TransactionDraft {
        namespace: Namespace::new(kind.namespace())?,
        schema_version: SchemaVersion(RECORD_SCHEMA_VERSION),
        payload: payload.into_bytes(),
        signer: key.public_key(),
        nonce,
    }))
}

/// Builds a genesis specification whose genesis block carries a rules record.
///
/// The rules are present from height 0, and two parties given the same inputs derive
/// the same genesis hash.
///
/// # Errors
///
/// Returns [`BridgeError`] if the rules document is not valid.
pub fn genesis_with_rules(
    network: NetworkId,
    key: &SigningKey,
    subject: &SubjectV1,
    rules_xml: &str,
    notarisation: Option<&NotarisationV1>,
    timestamp_millis: u64,
) -> Result<GenesisSpec, BridgeError> {
    let payload = compose_record(
        RecordKindV1::VotingRules,
        subject,
        None,
        notarisation,
        rules_xml,
    )?;
    let transaction = transaction_for(key, RecordKindV1::VotingRules, payload, 0)?;
    Ok(GenesisSpec {
        network_id: network,
        timestamp_millis,
        transactions: vec![transaction],
    })
}

/// Publishes a rules record.
///
/// `supersedes` must name the rules record currently in force for `subject`, or be
/// `None` when there is none. Anything else is a stale amendment and is refused
/// before the ledger is touched.
///
/// # Errors
///
/// Returns [`BridgeError::StaleAmendment`], a document error, or a ledger error.
pub fn publish_rules(
    store: &LocalChainStore,
    key: &SigningKey,
    subject: &SubjectV1,
    rules_xml: &str,
    supersedes: Option<TxId>,
    notarisation: Option<&NotarisationV1>,
    timestamp_millis: u64,
) -> Result<RecordRefV1, BridgeError> {
    publish(
        store,
        key,
        RecordKindV1::VotingRules,
        subject,
        rules_xml,
        supersedes,
        notarisation,
        timestamp_millis,
    )
}

/// Publishes a roll record.
///
/// # Errors
///
/// As [`publish_rules`].
pub fn publish_roll(
    store: &LocalChainStore,
    key: &SigningKey,
    subject: &SubjectV1,
    electorate_xml: &str,
    supersedes: Option<TxId>,
    notarisation: Option<&NotarisationV1>,
    timestamp_millis: u64,
) -> Result<RecordRefV1, BridgeError> {
    publish(
        store,
        key,
        RecordKindV1::Roll,
        subject,
        electorate_xml,
        supersedes,
        notarisation,
        timestamp_millis,
    )
}

#[allow(clippy::too_many_arguments)]
fn publish(
    store: &LocalChainStore,
    key: &SigningKey,
    kind: RecordKindV1,
    subject: &SubjectV1,
    inner_xml: &str,
    supersedes: Option<TxId>,
    notarisation: Option<&NotarisationV1>,
    timestamp_millis: u64,
) -> Result<RecordRefV1, BridgeError> {
    let head = store.head()?;
    let current = in_force_ref(store, subject, kind, head.height)?;
    let expected = current.as_ref().map(|r| r.tx_id);
    if expected != supersedes {
        return Err(BridgeError::StaleAmendment {
            subject: subject.to_string(),
            kind,
            expected: render(expected),
            found: render(supersedes),
        });
    }

    let payload = compose_record(kind, subject, supersedes, notarisation, inner_xml)?;
    let parent = store
        .get_block(head.height)?
        .ok_or_else(|| BridgeError::Malformed {
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

    let record = read_record(
        core::str::from_utf8(
            &store
                .get_transaction(&tx_id)?
                .expect("just committed")
                .transaction
                .payload,
        )
        .expect("composed as UTF-8"),
    )?;
    Ok(RecordRefV1 {
        height,
        index: 0,
        tx_id,
        signer: key.public_key(),
        record,
    })
}

fn render(id: Option<TxId>) -> String {
    id.map_or_else(|| "none".to_owned(), |id| id.to_string())
}

/// Every record of `kind` for `subject`, in ledger order, up to and including `at`.
///
/// The amendment chain is checked as it is walked: each record must supersede the one
/// before it, and the first must supersede nothing. A break is an error naming the
/// exact record, never something to skip past.
///
/// # Errors
///
/// Returns [`BridgeError::BrokenAmendmentChain`] or a ledger error.
pub fn history(
    store: &LocalChainStore,
    subject: &SubjectV1,
    kind: RecordKindV1,
    at: BlockHeight,
) -> Result<Vec<RecordRefV1>, BridgeError> {
    let head = store.head()?;
    let end = at.min(head.height);
    let mut records: Vec<RecordRefV1> = Vec::new();
    for block in store.iter_blocks(BlockHeight::GENESIS, end)? {
        let block = block?;
        for (index, transaction) in block.transactions.iter().enumerate() {
            if transaction.namespace.as_str() != kind.namespace() {
                continue;
            }
            let Ok(text) = core::str::from_utf8(&transaction.payload) else {
                continue;
            };
            let Ok(record) = read_record(text) else {
                continue;
            };
            if &record.subject != subject || record.kind() != kind {
                continue;
            }
            let expected = records.last().map(|r| r.tx_id);
            if record.supersedes != expected {
                return Err(BridgeError::BrokenAmendmentChain {
                    subject: subject.to_string(),
                    height: block.header.height,
                    tx_id: transaction.id,
                    expected: render(expected),
                    found: render(record.supersedes),
                });
            }
            records.push(RecordRefV1 {
                height: block.header.height,
                index: u32::try_from(index).expect("block indexes fit"),
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
    subject: &SubjectV1,
    kind: RecordKindV1,
    at: BlockHeight,
) -> Result<Option<RecordRefV1>, BridgeError> {
    Ok(history(store, subject, kind, at)?.pop())
}

/// The rules in force for `subject` at height `at`.
///
/// # Errors
///
/// Returns [`BridgeError::NothingInForce`] if no rules record exists by that height,
/// or the errors of [`history`].
pub fn rules_in_force(
    store: &LocalChainStore,
    subject: &SubjectV1,
    at: BlockHeight,
) -> Result<InForceV1<VotingRulesV1>, BridgeError> {
    let found = in_force_ref(store, subject, RecordKindV1::VotingRules, at)?.ok_or_else(|| {
        BridgeError::NothingInForce {
            subject: subject.to_string(),
            kind: RecordKindV1::VotingRules,
            at,
        }
    })?;
    let RecordBodyV1::VotingRules(value) = found.record.body else {
        unreachable!("filtered by kind")
    };
    Ok(InForceV1 {
        value,
        tx_id: found.tx_id,
        height: found.height,
        notarisation: found.record.notarisation,
        supersedes: found.record.supersedes,
    })
}

/// The roll in force for `subject` at height `at`.
///
/// # Errors
///
/// As [`rules_in_force`].
pub fn roll_in_force(
    store: &LocalChainStore,
    subject: &SubjectV1,
    at: BlockHeight,
) -> Result<InForceV1<ElectorateV1>, BridgeError> {
    let found = in_force_ref(store, subject, RecordKindV1::Roll, at)?.ok_or_else(|| {
        BridgeError::NothingInForce {
            subject: subject.to_string(),
            kind: RecordKindV1::Roll,
            at,
        }
    })?;
    let RecordBodyV1::Roll(value) = found.record.body else {
        unreachable!("filtered by kind")
    };
    Ok(InForceV1 {
        value,
        tx_id: found.tx_id,
        height: found.height,
        notarisation: found.record.notarisation,
        supersedes: found.record.supersedes,
    })
}

/// Evaluates ballots against the rules and roll in force at `at`.
///
/// This is the bridge's whole purpose in one call: ledger truth in, Bornite result out.
/// The result carries the records it used, so it can be traced back to the chain.
///
/// # Errors
///
/// As [`rules_in_force`] and [`roll_in_force`], plus any evaluation error.
pub fn evaluate_at(
    store: &LocalChainStore,
    subject: &SubjectV1,
    at: BlockHeight,
    ballots: &BallotSetV1,
) -> Result<EvaluationAtV1, BridgeError> {
    let rules = rules_in_force(store, subject, at)?;
    let roll = roll_in_force(store, subject, at)?;
    let evaluation = bornite_eval::evaluate(&rules.value, &roll.value, ballots)?;
    Ok(EvaluationAtV1 {
        at,
        rules,
        roll,
        evaluation,
    })
}
