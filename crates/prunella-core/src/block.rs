//! Block headers, blocks, and the drafts used to build them.
//!
//! A committed block is immutable: nothing in this crate, or anywhere else in
//! Prunella, mutates a block once it has been built. New blocks are produced from
//! [`BlockDraft`]s, which derive the header fields that must not be supplied by hand.

use crate::error::CoreError;
use crate::hash::Hash;
use crate::labels::{BlockHeight, NetworkId};
use crate::transaction::Transaction;
use borsh::{BorshDeserialize, BorshSerialize};
use prunella_canonical::{Canonical, domain, hash_canonical};

/// The only block header format version this build understands.
pub const HEADER_VERSION: u16 = 1;

/// The immutable, hashed summary of a block.
///
/// The block hash is the hash of this header, so the header commits to the chain
/// position, the parent, and (through `tx_root`) every transaction in the block.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct BlockHeader {
    /// Header format version. See [`HEADER_VERSION`].
    pub version: u16,
    /// The chain this block belongs to.
    pub network_id: NetworkId,
    /// Position in the chain; zero for genesis.
    pub height: BlockHeight,
    /// Block hash of the parent, or [`Hash::ZERO`] for genesis.
    pub previous_hash: Hash,
    /// Linear digest over the ordered transaction ids.
    pub tx_root: Hash,
    /// Number of transactions in the block.
    pub tx_count: u32,
    /// Wall-clock time declared by whoever produced the block.
    ///
    /// Prunella attaches no trust to this value. The only rule applied to it is that
    /// it may not go backwards relative to the parent, which is deterministic and
    /// locally checkable. Anything stronger is a consensus concern.
    pub timestamp_millis: u64,
}

impl Canonical for BlockHeader {}

impl BlockHeader {
    /// Computes the block hash by hashing this header canonically.
    #[must_use]
    pub fn block_hash(&self) -> Hash {
        Hash::from_bytes(hash_canonical(domain::BLOCK_HEADER, self))
    }

    /// Starts a draft for the block that would follow this one.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::HeightOverflow`] if this header sits at [`u64::MAX`].
    pub fn child_draft(
        &self,
        transactions: Vec<Transaction>,
        timestamp_millis: u64,
    ) -> Result<BlockDraft, CoreError> {
        Ok(BlockDraft {
            network_id: self.network_id.clone(),
            height: self.height.next()?,
            previous_hash: self.block_hash(),
            timestamp_millis,
            transactions,
        })
    }
}

/// A block: an immutable header plus the ordered transactions it commits to.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Block {
    /// The hashed header.
    pub header: BlockHeader,
    /// Transactions in the order the header's `tx_root` commits to.
    pub transactions: Vec<Transaction>,
}

impl Canonical for Block {}

impl Block {
    /// Returns this block's hash, which is the hash of its header.
    #[must_use]
    pub fn hash(&self) -> Hash {
        self.header.block_hash()
    }

    /// Returns this block's height.
    #[must_use]
    pub fn height(&self) -> BlockHeight {
        self.header.height
    }
}

/// An unbuilt block.
///
/// Holds only the fields a producer chooses. `tx_root` and `tx_count` are derived by
/// [`BlockDraft::build`], so they cannot disagree with the transaction list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockDraft {
    /// The chain this block will belong to.
    pub network_id: NetworkId,
    /// Position in the chain.
    pub height: BlockHeight,
    /// Block hash of the parent, or [`Hash::ZERO`] for genesis.
    pub previous_hash: Hash,
    /// Declared wall-clock time.
    pub timestamp_millis: u64,
    /// Transactions, in the order they will be committed.
    pub transactions: Vec<Transaction>,
}

impl BlockDraft {
    /// Derives the header and produces an immutable block.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::TooManyTransactions`] if the transaction list is longer
    /// than [`u32::MAX`].
    pub fn build(self) -> Result<Block, CoreError> {
        let tx_count =
            u32::try_from(self.transactions.len()).map_err(|_| CoreError::TooManyTransactions {
                count: self.transactions.len(),
                max: u32::MAX,
            })?;
        let tx_root = Transaction::compute_root(&self.transactions)?;
        Ok(Block {
            header: BlockHeader {
                version: HEADER_VERSION,
                network_id: self.network_id,
                height: self.height,
                previous_hash: self.previous_hash,
                tx_root,
                tx_count,
                timestamp_millis: self.timestamp_millis,
            },
            transactions: self.transactions,
        })
    }
}

/// Everything needed to derive a genesis block.
///
/// This is the root of the core invariant: two instances given the same
/// [`GenesisSpec`] derive byte-identical genesis blocks, and therefore identical
/// genesis hashes, without communicating.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenesisSpec {
    /// The chain identifier.
    pub network_id: NetworkId,
    /// Declared creation time. Defaults to zero in tooling so that a chain created
    /// from the same identifier is reproducible on any machine at any time.
    pub timestamp_millis: u64,
    /// Transactions to commit in the genesis block. Usually empty.
    pub transactions: Vec<Transaction>,
}

impl GenesisSpec {
    /// Creates a spec for an empty genesis block at timestamp zero.
    #[must_use]
    pub fn new(network_id: NetworkId) -> Self {
        Self {
            network_id,
            timestamp_millis: 0,
            transactions: Vec::new(),
        }
    }

    /// Derives the genesis block.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::TooManyTransactions`] if the transaction list is longer
    /// than [`u32::MAX`].
    pub fn build(self) -> Result<Block, CoreError> {
        BlockDraft {
            network_id: self.network_id,
            height: BlockHeight::GENESIS,
            previous_hash: Hash::ZERO,
            timestamp_millis: self.timestamp_millis,
            transactions: self.transactions,
        }
        .build()
    }
}
