//! The observable tip of a chain.

use crate::hash::Hash;
use crate::labels::BlockHeight;

/// The highest block currently committed to a chain.
///
/// Height and hash travel together deliberately: a height alone does not identify a
/// chain state, because two divergent chains can share a height.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ChainHead {
    /// Height of the highest committed block.
    pub height: BlockHeight,
    /// Hash of that block.
    pub hash: Hash,
}

impl ChainHead {
    /// Builds a head from its parts.
    #[must_use]
    pub const fn new(height: BlockHeight, hash: Hash) -> Self {
        Self { height, hash }
    }

    /// Returns the number of blocks in a chain with this head.
    #[must_use]
    pub const fn block_count(&self) -> u64 {
        self.height.value() + 1
    }
}

impl core::fmt::Display for ChainHead {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "height {} ({})", self.height, self.hash)
    }
}
