//! Versioned voting types with exact integer arithmetic.
//!
//! Bornite's job is to take a frozen electorate, a set of rules and a set of ballots and
//! return the same auditable result on every machine. This crate holds the values those
//! results are made of, built so that determinism is a property of the types rather
//! than a discipline the caller has to remember:
//!
//! * weights and totals are integers, and every sum is checked;
//! * fractions are exact numerator/denominator pairs, compared by cross-multiplication
//!   in `u128`, which cannot overflow for `u64` operands and so cannot fail;
//! * electorates and ballot sets sort themselves at construction and refuse duplicates,
//!   so nothing downstream can observe input order;
//! * voter ids are compared byte for byte, with no case folding.
//!
//! There is no floating point anywhere in Bornite. The workspace denies
//! `clippy::float_arithmetic`, so none can be introduced by accident.
//!
//! This crate knows nothing about organisations, ledgers, networks, or what a vote is
//! for. A voter id is opaque; a weight is a number; where the number came from is not
//! its concern.

mod ballot;
mod error;
mod fraction;
mod voter;

pub use ballot::{BallotSetV1, BallotV1, ChoiceV1};
pub use error::CoreError;
pub use fraction::FractionV1;
pub use voter::{ElectorateV1, MAX_VOTER_ID_LEN, VoterIdV1, VoterV1, WeightTotalV1, WeightV1};

/// The Bornite type version every `*V1` type belongs to.
pub const VERSION: u32 = 1;
