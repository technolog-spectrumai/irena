//! Storage failures.
//!
//! Every variant names something the store refused to do or could not justify. There
//! is deliberately no variant for "repaired": the store never rewrites a chain to make
//! it consistent, because silently repairing a ledger destroys the evidence that
//! something went wrong with it.

use crate::acceptance::AcceptanceError;
use prunella_core::{BlockHeight, Hash};

/// Failure modes of the chain store.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The database file could not be opened, created or written.
    #[error("chain database error: {0}")]
    Database(String),
    /// A chain already exists at this path.
    #[error("a chain already exists at {path}")]
    AlreadyExists {
        /// The path that is already occupied.
        path: String,
    },
    /// The file exists but does not hold a Prunella chain.
    #[error("{path} does not contain a prunella chain: {detail}")]
    NotAChain {
        /// The path that was opened.
        path: String,
        /// What was missing or unreadable.
        detail: String,
    },
    /// The store format is from a different version of Prunella.
    #[error("chain uses store format version {found}, this build supports {supported}")]
    UnsupportedFormatVersion {
        /// The version recorded in the file.
        found: u32,
        /// The version this build writes and reads.
        supported: u32,
    },
    /// The store's own bookkeeping disagrees with its contents.
    ///
    /// Reported rather than corrected. Recovery is an operator decision, made with a
    /// backup, not something the store performs behind their back.
    #[error("chain is internally inconsistent and has not been modified: {detail}")]
    Inconsistent {
        /// Exactly what disagrees with what.
        detail: String,
    },
    /// Stored bytes could not be decoded into a block.
    #[error("block at height {height} could not be decoded: {detail}")]
    CorruptBlock {
        /// Where the undecodable bytes are.
        height: BlockHeight,
        /// The decoder's complaint.
        detail: String,
    },
    /// The acceptance policy refused the block.
    #[error(transparent)]
    NotAccepted(#[from] AcceptanceError),
    /// A block was offered that does not continue the chain.
    #[error("expected the block at height {expected}, but was offered height {found}")]
    NonContiguous {
        /// The height the chain is waiting for.
        expected: BlockHeight,
        /// The height that was offered.
        found: BlockHeight,
    },
    /// A block was offered at a committed height with different contents.
    #[error("height {height} holds block {existing}, but a different block {offered} was offered")]
    ForkedHistory {
        /// The height where the histories diverge.
        height: BlockHeight,
        /// What is committed.
        existing: Hash,
        /// What was offered.
        offered: Hash,
    },
    /// A core value could not be constructed from stored bytes.
    #[error("stored value is not a valid core type: {0}")]
    Core(#[from] prunella_core::CoreError),
}

impl StoreError {
    pub(crate) fn database(error: impl core::fmt::Display) -> Self {
        Self::Database(error.to_string())
    }
}
