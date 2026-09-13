//! Why a meeting operation refused.

use crate::meeting::MeetingStatusV1;
use prunella_core::{BlockHeight, TxId};

/// Failure modes of a meeting.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MeetingError {
    /// The operation is not allowed in the meeting's current status.
    #[error("cannot {to} a meeting that is {from}")]
    InvalidTransition {
        /// The status the meeting is in.
        from: MeetingStatusV1,
        /// The operation attempted.
        to: &'static str,
    },
    /// The agenda is not a valid agenda.
    #[error("invalid agenda: {detail}")]
    InvalidAgenda {
        /// What was wrong.
        detail: String,
    },
    /// The meeting has no item with that number.
    #[error("the agenda has no item {number}")]
    NoSuchItem {
        /// The number asked for.
        number: u32,
    },
    /// The item is informational and has no vote.
    #[error("item {number} is informational; there is nothing to vote on")]
    NotAVoteItem {
        /// The item.
        number: u32,
    },
    /// A vote refused.
    #[error("item {number}: {source}")]
    Vote {
        /// Which item's vote.
        number: u32,
        /// What it said.
        #[source]
        source: irena_vote::VoteError,
    },
    /// The company could not be reconstructed.
    #[error(transparent)]
    Ledger(#[from] irena_ledger::LedgerError),
    /// A meeting document is not valid.
    #[error(transparent)]
    Record(#[from] irena_core::IrenaError),
    /// The chain refused the record or could not be read.
    #[error(transparent)]
    Store(#[from] prunella_store::StoreError),
    /// A core ledger value could not be built.
    #[error(transparent)]
    Core(#[from] prunella_core::CoreError),
    /// The transaction is not on the chain.
    #[error("no transaction {tx_id} is on the chain")]
    NoSuchTransaction {
        /// The id asked for.
        tx_id: TxId,
    },
    /// A transaction does not hold a meeting record.
    #[error("transaction {tx_id} at height {height} does not hold a meeting record: {detail}")]
    NotAMeetingRecord {
        /// The transaction.
        tx_id: TxId,
        /// Its height.
        height: BlockHeight,
        /// What was wrong.
        detail: String,
    },
    /// The chain is in a state the meeting cannot work with.
    #[error("malformed chain: {detail}")]
    Chain {
        /// What was wrong.
        detail: String,
    },
}
