//! Why the ledger layer refused.

use irena_core::RecordKindV1;
use prunella_core::{BlockHeight, TxId};

/// Failure modes of the ledger layer.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LedgerError {
    /// The record document is not a valid Irena record.
    #[error("invalid irena record: {0}")]
    Record(#[from] irena_core::IrenaError),
    /// The record would amend something other than what is in force.
    ///
    /// Amending a version you have not seen is how two editors clobber each other, so
    /// it is refused: `supersedes` must name exactly the record currently in force for
    /// the (company, kind), or be absent when there is none.
    #[error(
        "stale amendment for company {company}: {kind} in force is {expected}, but the record supersedes {found}"
    )]
    StaleAmendment {
        /// The company.
        company: String,
        /// The record kind.
        kind: RecordKindV1,
        /// What is in force, rendered (`none` when nothing is).
        expected: String,
        /// What the record claims to supersede, rendered.
        found: String,
    },
    /// The ledger holds records whose amendment chain does not link.
    ///
    /// This can only happen if a record was written around this crate. It is reported,
    /// never repaired, and until it is resolved nothing is in force for the (company,
    /// kind).
    #[error(
        "broken amendment chain for company {company} ({kind}) at height {height}, transaction {tx_id}: expected supersedes {expected}, found {found}"
    )]
    BrokenAmendmentChain {
        /// The company.
        company: String,
        /// The record kind.
        kind: RecordKindV1,
        /// Where the break is.
        height: BlockHeight,
        /// The offending record.
        tx_id: TxId,
        /// What it should have superseded, rendered.
        expected: String,
        /// What it claims, rendered.
        found: String,
    },
    /// A transaction in an Irena namespace does not hold a readable record of that
    /// kind.
    ///
    /// Prunella accepts any payload in any namespace, so this can only come from
    /// something writing around this crate. It is reported, not skipped: a reader that
    /// stepped over it could not know whether it was meant to be an amendment.
    #[error(
        "transaction {tx_id} at height {height} is in the {namespace} namespace but does not hold a {kind} record: {detail}"
    )]
    UnreadableRecord {
        /// Where it is.
        height: BlockHeight,
        /// Which transaction.
        tx_id: TxId,
        /// The namespace it was published under.
        namespace: String,
        /// The kind that namespace carries.
        kind: RecordKindV1,
        /// What was wrong.
        detail: String,
    },
    /// Nothing is in force for the company.
    #[error("no {kind} record is in force for company {company} at height {at}")]
    NothingInForce {
        /// The company.
        company: String,
        /// The record kind.
        kind: RecordKindV1,
        /// The height asked about.
        at: BlockHeight,
    },
    /// A structural problem with the chain itself.
    #[error("malformed chain: {detail}")]
    Malformed {
        /// What was wrong.
        detail: String,
    },
    /// The ledger refused the block or could not be read.
    #[error(transparent)]
    Store(#[from] prunella_store::StoreError),
    /// A core ledger value could not be built.
    #[error(transparent)]
    Ledger(#[from] prunella_core::CoreError),
}
