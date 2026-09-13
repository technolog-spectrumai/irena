//! Why the bridge refused.

use crate::record::RecordKindV1;
use prunella_core::{BlockHeight, TxId};

/// Failure modes of the bridge.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BridgeError {
    /// The record document is not a valid governance record.
    #[error("invalid governance record: {0}")]
    Record(#[from] bornite_xml::XmlError),
    /// A structural problem in the record envelope.
    #[error("malformed governance record: {detail}")]
    Malformed {
        /// What was wrong.
        detail: String,
    },
    /// The record's `kind` does not match the element it carries.
    #[error("record declares kind {declared} but carries a {carried} element")]
    KindMismatch {
        /// The `kind` attribute.
        declared: RecordKindV1,
        /// What the body actually is.
        carried: RecordKindV1,
    },
    /// The subject is not a valid label.
    #[error("invalid subject {value:?}: {reason}")]
    InvalidSubject {
        /// The rejected value.
        value: String,
        /// Why.
        reason: &'static str,
    },
    /// The record would amend something other than what is in force.
    ///
    /// Amending a version you have not seen is how two editors clobber each other, so
    /// it is refused: `supersedes` must name exactly the record currently in force for
    /// the subject, or be absent when there is none.
    #[error(
        "stale amendment for subject {subject}: {kind} in force is {expected}, but the record supersedes {found}"
    )]
    StaleAmendment {
        /// The subject.
        subject: String,
        /// The record kind.
        kind: RecordKindV1,
        /// What is in force, rendered (`none` when nothing is).
        expected: String,
        /// What the record claims to supersede, rendered.
        found: String,
    },
    /// The ledger holds records whose amendment chain does not link.
    ///
    /// This can only happen if a record was written around the bridge. It is reported,
    /// never repaired, and until it is resolved nothing is in force for the subject.
    #[error(
        "broken amendment chain for subject {subject} at height {height}, transaction {tx_id}: expected supersedes {expected}, found {found}"
    )]
    BrokenAmendmentChain {
        /// The subject.
        subject: String,
        /// Where the break is.
        height: BlockHeight,
        /// The offending record.
        tx_id: TxId,
        /// What it should have superseded, rendered.
        expected: String,
        /// What it claims, rendered.
        found: String,
    },
    /// Nothing is in force for the subject.
    #[error("no {kind} record is in force for subject {subject} at height {at}")]
    NothingInForce {
        /// The subject.
        subject: String,
        /// The record kind.
        kind: RecordKindV1,
        /// The height asked about.
        at: BlockHeight,
    },
    /// The ledger refused the block or could not be read.
    #[error(transparent)]
    Store(#[from] prunella_store::StoreError),
    /// A core ledger value could not be built.
    #[error(transparent)]
    Ledger(#[from] prunella_core::CoreError),
    /// The vote could not be evaluated.
    #[error(transparent)]
    Evaluation(#[from] bornite_eval::EvaluationErrorV1),
}
