//! The storage abstraction.
//!
//! [`ChainStorage`] is the contract every chain store satisfies, regardless of how it
//! keeps bytes. One implementation ships today: [`LocalChainStore`](crate::LocalChainStore),
//! backed by a local redb file.
//!
//! # Storage is not consensus
//!
//! A successful [`ChainStorage::append_block`] means **locally accepted after full
//! deterministic validation**. It does not mean finalized, agreed, or irreversible in a
//! distributed sense — there is no distributed sense here yet. The seam where a
//! consensus engine would attach is [`BlockAcceptancePolicy`](crate::BlockAcceptancePolicy),
//! documented in `docs/consensus-boundary.md`, and it is deliberately empty.
//!
//! # Reading the head
//!
//! `head()` comes from the [`BlockSource`] supertrait rather than being declared here a
//! second time. Declaring it twice would make `store.head()` ambiguous in code generic
//! over `ChainStorage`, so there is exactly one.

use crate::error::StoreError;
use prunella_core::{Block, BlockHeight, GenesisSpec, Hash, TxId};
use prunella_verify::{BlockSource, VerificationReport, VerifyOptions, verify_chain};

/// What an append did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppendStatus {
    /// The block passed validation and was committed. The head advanced.
    Committed,
    /// A byte-identical block was already committed at this height.
    ///
    /// Nothing was written and nothing changed. Re-offering a block the chain already
    /// holds is how a retried or replayed transfer behaves, and it is not an error —
    /// but a *different* block at a committed height is
    /// [`StoreError::ForkedHistory`], because that would rewrite history.
    AlreadyPresent,
}

impl AppendStatus {
    /// Whether this append advanced the chain.
    #[must_use]
    pub const fn committed(self) -> bool {
        matches!(self, Self::Committed)
    }
}

impl core::fmt::Display for AppendStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Committed => "committed",
            Self::AlreadyPresent => "already present",
        })
    }
}

/// The result of appending one block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppendOutcome {
    /// Whether the block was committed or was already present.
    pub status: AppendStatus,
    /// The head after the call, which is unchanged when the block was already present.
    pub head: prunella_core::ChainHead,
}

/// The result of appending a run of blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchOutcome {
    /// Blocks newly committed.
    pub appended: u64,
    /// Blocks already present, identically, and therefore not committed again.
    pub already_present: u64,
    /// The head after the call.
    pub head: prunella_core::ChainHead,
}

/// A persistent, append-only chain.
///
/// # Invariants every implementation must uphold
///
/// * **Genesis exists exactly once.** It is written by [`ChainStorage::init_genesis`]
///   and never again; opening an existing chain never creates one.
/// * **Heights are contiguous.** A block is accepted only at `head.height + 1`.
/// * **Blocks link.** `block.previous_hash` equals the current head's hash.
/// * **Network ids match.** A block from another chain is never stored.
/// * **Transaction ids are unique** across the whole chain, not merely within a block.
/// * **Every signature verifies** against its signer over the recomputed signing message.
/// * **`tx_root` matches the block's contents**, recomputed rather than trusted.
/// * **Block hashes are recomputed**, never taken from anything the caller supplied.
/// * **Committed blocks are immutable.** No method rewrites or removes one.
/// * **Appends are atomic.** Either the block and every index entry are stored, or
///   nothing changes.
/// * **History is never silently repaired.** A chain that disagrees with itself is
///   reported, not rewritten.
///
/// Each of those is enforced by `prunella_verify::check_block`, which is the single
/// implementation of block validity in Prunella. An implementation of this trait must
/// not write its own version of any of these rules.
///
/// # Object safety
///
/// This trait is not object-safe: `init_genesis` returns `Self` and `iter_blocks`
/// returns an associated iterator type. Use [`BlockSource`] where a `dyn` read-only
/// view is needed.
pub trait ChainStorage: BlockSource + Sized {
    /// How this implementation is addressed — a filesystem path, a directory, a handle.
    type Location;

    /// Errors this implementation produces.
    type Error;

    /// The iterator returned by [`ChainStorage::iter_blocks`].
    type Blocks<'a>: Iterator<Item = Result<Block, Self::Error>>
    where
        Self: 'a;

    /// Creates a chain and writes its genesis block.
    ///
    /// The genesis block is derived from `genesis`, so two instances given the same
    /// specification produce the same genesis hash without communicating. Fails if a
    /// chain already exists at `location`: genesis is written exactly once, and
    /// overwriting it would silently replace a chain's root.
    ///
    /// # Errors
    ///
    /// Implementation-defined; at minimum when the location is occupied or unwritable.
    fn init_genesis(location: Self::Location, genesis: GenesisSpec) -> Result<Self, Self::Error>;

    /// Reads the block at a height, or `None` if the chain does not reach it.
    ///
    /// # Errors
    ///
    /// Returns an error if the block exists but cannot be read or decoded. A damaged
    /// block is reported, never skipped or reconstructed.
    fn get_block(&self, height: BlockHeight) -> Result<Option<Block>, Self::Error>;

    /// Reads the block with a given hash, or `None` if the chain does not hold it.
    ///
    /// # Errors
    ///
    /// Returns an error if the block cannot be read, or if the hash index disagrees
    /// with the blocks it points at.
    fn get_block_by_hash(&self, hash: &Hash) -> Result<Option<Block>, Self::Error>;

    /// Reads a transaction by id, with the position it was committed at.
    ///
    /// # Errors
    ///
    /// Returns an error if the index points somewhere the transaction is not.
    fn get_transaction(&self, id: &TxId) -> Result<Option<crate::LocatedTransaction>, Self::Error>;

    /// Iterates blocks over the inclusive height range `start..=end`.
    ///
    /// The range is read from one consistent snapshot, so a concurrent append cannot
    /// make it internally inconsistent. An empty range yields nothing.
    ///
    /// # Errors
    ///
    /// Returns an error if the snapshot cannot be opened.
    fn iter_blocks(
        &self,
        start: BlockHeight,
        end: BlockHeight,
    ) -> Result<Self::Blocks<'_>, Self::Error>;

    /// Validates a block and, if it is acceptable, commits it atomically.
    ///
    /// Returns [`AppendStatus::AlreadyPresent`] when a byte-identical block is already
    /// committed at that height; a different block there is rejected rather than
    /// stored. A block that fails validation leaves the chain exactly as it was.
    ///
    /// A successful append means locally accepted, not distributed finality.
    ///
    /// # Errors
    ///
    /// Returns an error when the block is not acceptable, or when the write fails.
    fn append_block(&self, block: Block) -> Result<AppendOutcome, Self::Error>;

    /// Verifies the chain from `start_height` to the head.
    ///
    /// Re-derives every hash, transaction id, transaction root and signature, and
    /// reports every defect with its exact location. Genesis identity is checked even
    /// when `start_height` is past genesis, because a range of blocks means nothing
    /// without knowing which chain they belong to.
    ///
    /// The default implementation defers entirely to `prunella_verify`. Overriding it
    /// would mean two answers to whether a chain is valid, so do not.
    #[must_use]
    fn verify_from(&self, start_height: BlockHeight) -> VerificationReport {
        verify_chain(
            self,
            VerifyOptions {
                from: Some(start_height),
                ..VerifyOptions::default()
            },
        )
    }
}

/// Converts a storage error into the form verification reports.
pub(crate) fn source_error(error: &StoreError) -> prunella_verify::SourceError {
    prunella_verify::SourceError::new(error.to_string())
}
