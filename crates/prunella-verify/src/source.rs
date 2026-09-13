//! The read-only view of a chain that verification walks.

use prunella_core::{Block, BlockHeight, ChainHead, Hash, NetworkId};

/// A chain could not be read.
///
/// Verification does not care why a source failed — a corrupt file, a closed database,
/// a truncated import document. It records the failure with its location and keeps the
/// report honest rather than guessing at missing data.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[error("chain source failure: {detail}")]
pub struct SourceError {
    /// What went wrong, in the source's own words.
    pub detail: String,
}

impl SourceError {
    /// Wraps a description of a read failure.
    #[must_use]
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

/// Anything verification can walk from genesis to head.
///
/// Implemented by `prunella-store::ChainStore` for a persisted chain and by the XML
/// importer for a candidate document, so both are verified by exactly the same code.
pub trait BlockSource {
    /// The chain this source claims to hold.
    fn network_id(&self) -> &NetworkId;

    /// The genesis hash this chain was created with.
    ///
    /// Recorded independently of the blocks so that a replaced genesis block is
    /// detectable rather than self-consistent.
    fn genesis_hash(&self) -> Hash;

    /// The highest committed block.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError`] if the head could not be read.
    fn head(&self) -> Result<ChainHead, SourceError>;

    /// Reads the block at a height, or `None` if there is none.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError`] if the block exists but could not be read or decoded.
    fn block_at(&self, height: BlockHeight) -> Result<Option<Block>, SourceError>;
}
