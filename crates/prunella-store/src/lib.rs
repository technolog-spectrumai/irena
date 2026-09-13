//! Persistent, append-only chain storage for Prunella.
//!
//! One chain lives in one file. Blocks are stored as their `prunella-canonical`
//! encodings, so the bytes on disk are the bytes that were hashed — a stored block
//! cannot mean one thing to storage and something else to verification.
//!
//! # Immutability
//!
//! There is no API here that rewrites or deletes a committed block, and no code path
//! that repairs one. A chain whose bookkeeping disagrees with its contents is reported
//! through [`StoreError::Inconsistent`] and left exactly as it was found, because a
//! ledger that quietly fixes itself has destroyed the evidence an operator needs.
//!
//! # Atomicity
//!
//! [`ChainStore::append_blocks`] validates and commits a whole run of blocks in a
//! single database transaction. A rejection anywhere leaves the chain byte-identical,
//! which is what lets an import be all-or-nothing.
//!
//! # Acceptance
//!
//! Storage does not decide what is acceptable. It asks a [`BlockAcceptancePolicy`] and
//! commits only against the token that policy returns. Today that is
//! [`LocalDeterministicPolicy`], which defers entirely to `prunella-verify`. A
//! consensus engine would be a second implementation of the same trait and would
//! require no change to this module. See `docs/consensus-boundary.md`.
//!
//! **A successful append means locally accepted after full deterministic validation.**
//! It does not mean distributed finality. There is no consensus here, and storage is
//! written so that adding one later does not change anything in this module.
//!
//! # The abstraction and its implementation
//!
//! [`ChainStorage`] is the contract; [`LocalChainStore`] is the one implementation,
//! backed by a local redb file. The trait carries the invariants every chain store must
//! uphold, and its `verify_from` defers to `prunella-verify` so that no implementation
//! can grow its own opinion about what a valid chain is.
//!
//! ```no_run
//! use prunella_core::{BlockHeight, GenesisSpec, NetworkId};
//! use prunella_store::{ChainStorage, LocalChainStore};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let spec = GenesisSpec::new(NetworkId::new("demo")?);
//! let store = LocalChainStore::init_genesis("demo.chain", spec)?;
//!
//! let head = store.head()?;
//! let genesis = store.get_block(BlockHeight::GENESIS)?.expect("genesis exists");
//! assert_eq!(head.hash, genesis.hash());
//!
//! let report = store.verify_from(BlockHeight::GENESIS);
//! assert!(report.is_valid());
//! # Ok(())
//! # }
//! ```

mod acceptance;
mod error;
mod storage;
mod store;
mod tables;

pub use acceptance::{
    AcceptanceContext, AcceptanceError, Accepted, BlockAcceptancePolicy, LocalDeterministicPolicy,
};
pub use error::StoreError;
pub use storage::{AppendOutcome, AppendStatus, BatchOutcome, ChainStorage};
pub use store::{BlockRange, ChainStatus, LocalChainStore, LocatedTransaction};
pub use tables::STORE_FORMAT_VERSION;
