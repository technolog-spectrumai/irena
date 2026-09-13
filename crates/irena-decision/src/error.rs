//! Why a channel could not be resolved, or a decision refused.

use crate::decision::DecisionStatusV1;
use irena_core::ChannelIdV1;
use prunella_core::{BlockHeight, TxId};

/// Failure modes of channel resolution and individual decisions.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DecisionError {
    /// The company has no channel of that id at that height.
    #[error("the company has no decision channel {channel} at height {height}")]
    NoSuchChannel {
        /// The id asked for.
        channel: ChannelIdV1,
        /// The height it was looked for at.
        height: BlockHeight,
    },
    /// An individual channel's source resolved to other than one actor.
    #[error(
        "channel {channel} is individual but its actors resolve to {found} people; exactly one is required"
    )]
    NotSingleActor {
        /// The channel.
        channel: ChannelIdV1,
        /// How many actors resolved.
        found: usize,
    },
    /// An operation for individual channels was asked of a collective one.
    #[error("channel {channel} is collective; it decides by vote, not by one signature")]
    NotIndividual {
        /// The channel.
        channel: ChannelIdV1,
    },
    /// An operation for collective channels was asked of an individual one.
    #[error("channel {channel} is individual; it decides by one signature, not by vote")]
    NotCollective {
        /// The channel.
        channel: ChannelIdV1,
    },
    /// The sole actor of an individual channel registered no signing key.
    #[error("{actor}, the sole actor of channel {channel}, has no registered signing key")]
    NoKey {
        /// The channel.
        channel: ChannelIdV1,
        /// The actor.
        actor: String,
    },
    /// The key offered is not the frozen actor's.
    #[error("the signing key is not the registered key of {actor}, the frozen actor")]
    WrongKey {
        /// The actor.
        actor: String,
    },
    /// The signature does not verify against the frozen key.
    #[error("the signature does not verify: {detail}")]
    BadSignature {
        /// What the verifier said.
        detail: String,
    },
    /// The operation is not allowed in the decision's current status.
    #[error("cannot {to} a decision that is {from}")]
    InvalidTransition {
        /// The status the decision is in.
        from: DecisionStatusV1,
        /// The operation attempted.
        to: &'static str,
    },
    /// The company could not be resolved at the requested height.
    #[error(transparent)]
    Ledger(#[from] irena_ledger::LedgerError),
    /// Bornite refused the actors as an electorate.
    #[error("cannot derive an electorate: {0}")]
    Derivation(bornite_core::CoreError),
    /// The chain refused the record or could not be read.
    #[error(transparent)]
    Store(#[from] prunella_store::StoreError),
    /// A core ledger value could not be built.
    #[error(transparent)]
    Core(#[from] prunella_core::CoreError),
    /// A record's bytes are not a final decision record.
    #[error(
        "transaction {tx_id} at height {height} does not hold a final decision record: {detail}"
    )]
    NotADecisionRecord {
        /// The transaction.
        tx_id: TxId,
        /// Its height.
        height: BlockHeight,
        /// What was wrong.
        detail: String,
    },
    /// The chain is in a state the decision cannot work with.
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
    /// A label in a decoded snapshot is not valid.
    #[error(transparent)]
    Label(#[from] irena_core::IrenaError),
}
