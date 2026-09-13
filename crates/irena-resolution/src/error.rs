//! Why a resolution operation refused.

use crate::resolution::{AmendmentTargetV1, ResolutionStatusV1};
use prunella_core::{BlockHeight, Hash, TxId};

/// Failure modes of a resolution.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ResolutionError {
    /// The operation is not allowed in the resolution's current status.
    #[error("cannot {to} a resolution that is {from}")]
    InvalidTransition {
        /// The status the resolution is in.
        from: ResolutionStatusV1,
        /// The operation attempted.
        to: &'static str,
    },
    /// A declarative resolution changes nothing, so there is nothing to execute.
    #[error("a declarative resolution records a decision; there is nothing to execute")]
    NothingToExecute,
    /// The meeting the resolution rests on does not verify.
    #[error("the meeting {meeting_tx} does not verify: {detail}")]
    MeetingUnverified {
        /// The meeting's final record.
        meeting_tx: TxId,
        /// Which checks failed.
        detail: String,
    },
    /// The meeting has no such agenda item, or it is not a vote item.
    #[error("meeting {meeting_tx} has no vote on agenda item {item_number}")]
    NoSuchVoteItem {
        /// The meeting.
        meeting_tx: TxId,
        /// The item asked for.
        item_number: u32,
    },
    /// The vote named is not the vote that answered that agenda item.
    #[error(
        "item {item_number} of meeting {meeting_tx} was answered by vote {expected}, but the resolution names {found}"
    )]
    WrongVote {
        /// The meeting.
        meeting_tx: TxId,
        /// The item.
        item_number: u32,
        /// The vote that answered it.
        expected: TxId,
        /// The vote the resolution names.
        found: TxId,
    },
    /// The vote does not verify from the chain.
    #[error("the vote {vote_tx} does not verify: {detail}")]
    VoteUnverified {
        /// The vote.
        vote_tx: TxId,
        /// Which checks failed.
        detail: String,
    },
    /// The vote did not pass.
    ///
    /// A resolution rests on a decision, and a rejected motion is a decision not to
    /// act. There is nothing to resolve.
    #[error("the vote {vote_tx} was rejected ({reason}); a rejected motion authorises nothing")]
    VoteRejected {
        /// The vote.
        vote_tx: TxId,
        /// Bornite's reason code.
        reason: String,
    },
    /// What the resolution carries is not what the shareholders approved.
    #[error(
        "the resolution does not match the approved proposal: the agenda item's proposal digest is {expected}, what the resolution carries digests to {found}"
    )]
    ProposalMismatch {
        /// The digest the vote committed to.
        expected: Hash,
        /// The digest of what the resolution carries.
        found: Hash,
    },
    /// The resolution is for a different company than the chain holds.
    #[error("the vote is for company {found}, but this chain holds {expected}")]
    CompanyMismatch {
        /// The chain's company.
        expected: String,
        /// The vote's.
        found: String,
    },
    /// The company moved under the resolution between the vote and the execution.
    ///
    /// The shareholders approved replacing one exact record; if that record no longer
    /// provides its part, executing would replace something they never saw. The
    /// resolution must go back to a meeting.
    #[error(
        "the {target} approved for replacement was {approved}, but {current} provides it now; the resolution was passed against a company that has since changed"
    )]
    StaleBase {
        /// Which part.
        target: AmendmentTargetV1,
        /// What the voters saw.
        approved: TxId,
        /// What provides it now.
        current: TxId,
    },
    /// The resolution has already been executed.
    #[error("resolution {resolution_tx} was already executed at height {height} by {execution_tx}")]
    AlreadyExecuted {
        /// The resolution.
        resolution_tx: TxId,
        /// The execution record.
        execution_tx: TxId,
        /// Where it is.
        height: BlockHeight,
    },
    /// A meeting could not be read.
    ///
    /// The layered errors below are boxed: a resolution sits on top of a meeting, a
    /// vote, a ledger and a document reader, and carrying all of them inline would
    /// make every `Result` in this crate as large as the deepest one.
    #[error("{0}")]
    Meeting(#[source] Box<irena_meeting::MeetingError>),
    /// A vote could not be read.
    #[error("{0}")]
    Vote(#[source] Box<irena_vote::VoteError>),
    /// The company could not be reconstructed, or the amendment was refused.
    #[error("{0}")]
    Ledger(#[source] Box<irena_ledger::LedgerError>),
    /// A document is not valid.
    #[error("{0}")]
    Record(#[source] Box<irena_core::IrenaError>),
    /// The chain refused the record or could not be read.
    #[error("{0}")]
    Store(#[source] Box<prunella_store::StoreError>),
    /// A core ledger value could not be built.
    #[error("{0}")]
    Core(#[source] Box<prunella_core::CoreError>),
    /// The transaction is not on the chain.
    #[error("no transaction {tx_id} is on the chain")]
    NoSuchTransaction {
        /// The id asked for.
        tx_id: TxId,
    },
    /// The chain is in a state the resolution cannot work with.
    #[error("malformed chain: {detail}")]
    Chain {
        /// What was wrong.
        detail: String,
    },
}

macro_rules! boxed_from {
    ($source:path, $variant:ident) => {
        impl From<$source> for ResolutionError {
            fn from(error: $source) -> Self {
                Self::$variant(Box::new(error))
            }
        }
    };
}

boxed_from!(irena_meeting::MeetingError, Meeting);
boxed_from!(irena_vote::VoteError, Vote);
boxed_from!(irena_ledger::LedgerError, Ledger);
boxed_from!(irena_core::IrenaError, Record);
boxed_from!(prunella_store::StoreError, Store);
boxed_from!(prunella_core::CoreError, Core);
