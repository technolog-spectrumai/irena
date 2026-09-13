//! Why a vote operation refused.

use crate::lifecycle::VoteStatusV1;
use prunella_core::{BlockHeight, TxId};

/// Why a ballot was not accepted. Checked in this order; the first failure is reported.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "rejection", rename_all = "snake_case")]
#[non_exhaustive]
pub enum BallotRejectionV1 {
    /// The ballot is for a different vote.
    #[error("the ballot is for vote {found}, not this vote {expected}")]
    WrongVote {
        /// This vote's id.
        expected: String,
        /// The id the ballot names.
        found: String,
    },
    /// The voter id is not well-formed.
    #[error("the voter id is not valid: {reason}")]
    InvalidVoter {
        /// Why.
        reason: String,
    },
    /// The voter is not in the frozen electorate.
    #[error("{voter} is not in the frozen electorate")]
    NotInElectorate {
        /// The voter.
        voter: String,
    },
    /// The voter is excluded by the frozen electorate.
    #[error("{voter} is excluded from this vote")]
    Excluded {
        /// The voter.
        voter: String,
    },
    /// The voter registered no signing key in the frozen register.
    #[error("{voter} has no signing key in the frozen share register")]
    NoKey {
        /// The voter.
        voter: String,
    },
    /// The signature does not verify against the registered key.
    #[error("the signature from {voter} does not verify: {detail}")]
    BadSignature {
        /// The voter.
        voter: String,
        /// What the verifier said.
        detail: String,
    },
    /// The voter already cast a ballot.
    #[error("{voter} already cast a ballot")]
    AlreadyVoted {
        /// The voter.
        voter: String,
    },
}

/// Failure modes of a vote.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum VoteError {
    /// The operation is not allowed in the vote's current status.
    #[error("cannot {to} a vote that is {from}")]
    InvalidTransition {
        /// The status the vote is in.
        from: VoteStatusV1,
        /// The operation attempted.
        to: &'static str,
    },
    /// A ballot was refused.
    #[error("ballot rejected: {0}")]
    Ballot(BallotRejectionV1),
    /// The company could not be resolved at the requested height.
    #[error(transparent)]
    Ledger(#[from] irena_ledger::LedgerError),
    /// The frozen electorate could not be rebuilt for Bornite.
    #[error("cannot derive an electorate: {0}")]
    Derivation(bornite_core::CoreError),
    /// The channel could not be resolved.
    #[error(transparent)]
    Decision(#[from] irena_decision::DecisionError),
    /// A vote was asked of a channel that decides by one signature.
    #[error("channel {channel} is individual; it decides by one signature, not by vote")]
    NotCollective {
        /// The channel.
        channel: String,
    },
    /// Bornite could not evaluate the vote.
    #[error(transparent)]
    Evaluation(#[from] bornite_eval::EvaluationErrorV1),
    /// The chain refused the record or could not be read.
    #[error(transparent)]
    Store(#[from] prunella_store::StoreError),
    /// A core ledger value could not be built.
    #[error(transparent)]
    Core(#[from] prunella_core::CoreError),
    /// A record's bytes are not a final vote record.
    #[error("transaction {tx_id} at height {height} does not hold a final vote record: {detail}")]
    NotAVoteRecord {
        /// The transaction.
        tx_id: TxId,
        /// Its height.
        height: BlockHeight,
        /// What was wrong.
        detail: String,
    },
    /// The chain is in a state the vote cannot work with.
    #[error("malformed chain: {detail}")]
    Chain {
        /// What was wrong.
        detail: String,
    },
    /// The transaction is not on the chain.
    #[error("no transaction {tx_id} is on the chain")]
    NoSuchTransaction {
        /// The id asked for.
        tx_id: TxId,
    },
    /// The frozen channel set no longer resolves to what the snapshot pinned.
    #[error(
        "the channel set in force at height {height} is {found}, but the snapshot pinned {expected}"
    )]
    ChannelsMoved {
        /// The snapshot height.
        height: BlockHeight,
        /// What the snapshot pinned.
        expected: TxId,
        /// What resolves now.
        found: TxId,
    },
    /// The snapshot's company label is not valid.
    #[error(transparent)]
    Company(#[from] irena_core::IrenaError),
    /// The stored evaluation does not match a rerun.
    #[error("the stored result does not match a fresh evaluation")]
    ResultMismatch,
}
