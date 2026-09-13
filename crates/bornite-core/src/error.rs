//! Failures of core type construction.

use crate::VoterIdV1;

/// Why a core value could not be built.
///
/// Every variant is a refusal to build something invalid. None describe an evaluation
/// outcome: a vote that fails is a result, not an error.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum CoreError {
    /// A voter id did not satisfy the documented grammar.
    #[error("invalid voter id {value:?}: {reason}")]
    InvalidVoterId {
        /// The rejected value.
        value: String,
        /// Why it was rejected.
        reason: &'static str,
    },
    /// A weight of zero was supplied. Weights are at least one.
    #[error("a voting weight must be at least 1")]
    ZeroWeight,
    /// A fraction with a zero denominator was supplied.
    #[error("a fraction's denominator must be at least 1")]
    ZeroDenominator,
    /// A proportion above one was supplied where a share of something was expected.
    #[error("{numerator}/{denominator} is not a proportion: the numerator exceeds the denominator")]
    ImproperFraction {
        /// The numerator.
        numerator: u64,
        /// The denominator.
        denominator: u64,
    },
    /// Summing weights exceeded `u64::MAX`.
    #[error("the total weight exceeds the maximum this engine can represent")]
    WeightOverflow,
    /// The same voter appeared twice in an electorate.
    #[error("voter {id} appears more than once in the electorate")]
    DuplicateVoter {
        /// The repeated id.
        id: VoterIdV1,
    },
    /// The same voter cast more than one ballot.
    #[error("voter {voter} cast more than one ballot")]
    DuplicateBallot {
        /// The repeated voter.
        voter: VoterIdV1,
    },
}
