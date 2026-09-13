//! Why an evaluation could not be produced.
//!
//! These are refusals, not outcomes. A vote that fails is a [`VoteEvaluationV1`]
//! with `outcome: Rejected`; an evaluation error means the inputs were not a vote the
//! engine could evaluate at all.
//!
//! [`VoteEvaluationV1`]: crate::VoteEvaluationV1

use bornite_core::VoterIdV1;
use bornite_rules::ValidationErrorV1;

/// One thing wrong with a ballot.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(tag = "issue", rename_all = "snake_case")]
#[non_exhaustive]
pub enum BallotIssueV1 {
    /// The ballot names a voter who is not in the electorate.
    #[error("ballot from {voter}, who is not in the electorate")]
    UnknownVoter {
        /// The voter named.
        voter: VoterIdV1,
    },
    /// The ballot comes from a voter the exclusion rule removed.
    #[error("ballot from {voter}, who is excluded")]
    ExcludedVoter {
        /// The voter.
        voter: VoterIdV1,
    },
}

/// Why no evaluation was produced.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "error", rename_all = "snake_case")]
#[non_exhaustive]
pub enum EvaluationErrorV1 {
    /// The rules and the electorate contradict each other. Step 1.
    #[error(transparent)]
    Validation(ValidationErrorV1),
    /// One or more ballots could not be accepted. Step 4. Every issue is reported.
    #[error("{} ballot issue(s): {}", .issues.len(), render(.issues))]
    Ballots {
        /// The issues, sorted by voter.
        issues: Vec<BallotIssueV1>,
    },
    /// A weight sum exceeded `u64::MAX`.
    #[error("a weight total exceeds the maximum this engine can represent")]
    WeightOverflow,
}

fn render(issues: &[BallotIssueV1]) -> String {
    issues
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}
