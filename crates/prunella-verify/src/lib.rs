//! Deterministic block rules and structured chain verification for Prunella.
//!
//! Verification answers one question: is this chain exactly what its genesis and its
//! blocks say it is? It never repairs, never guesses and never skips. A chain that
//! cannot be fully justified is reported as invalid, with the exact height,
//! transaction index and identifier of every defect.
//!
//! # One set of rules
//!
//! [`check_block`] is the only implementation of block validity in Prunella. The
//! block-acceptance policy in `prunella-store` calls it before committing, full-chain
//! verification calls it for every block, and XML import calls it for every candidate
//! block. Nothing reimplements a rule, so nothing can drift from the others — which is
//! what makes two independent instances agree on the same chain.
//!
//! # Reading a report
//!
//! [`verify_chain`] always returns a [`VerificationReport`]. [`VerificationReport::is_valid`]
//! is true only when no finding was produced at all; there is no warning level, because
//! a ledger that is almost consistent is inconsistent.

mod chain;
mod finding;
mod rules;
mod source;

pub use chain::{VerificationReport, VerifyOptions, verify_chain};
pub use finding::{Finding, FindingKind, Location};
pub use rules::{BlockContext, TxIdLookup, check_block};
pub use source::{BlockSource, SourceError};
