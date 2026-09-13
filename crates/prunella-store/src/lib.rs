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

mod acceptance;
mod error;
mod store;
mod tables;

pub use acceptance::{
    AcceptanceContext, AcceptanceError, Accepted, BlockAcceptancePolicy, LocalDeterministicPolicy,
};
pub use error::StoreError;
pub use store::{
    AppendOutcome, BlockRange, ChainStatus, ChainStore, ExistingBlockPolicy, LocatedTransaction,
};
pub use tables::STORE_FORMAT_VERSION;
