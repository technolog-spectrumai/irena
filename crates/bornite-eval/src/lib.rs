//! The fixed-order deterministic vote evaluation.
//!
//! [`evaluate`] takes a rule set, a frozen electorate and a set of ballots and returns
//! a [`VoteEvaluationV1`] — the same one on every machine, in every input order, at any
//! time. The steps run in a fixed order and none is skipped when an earlier one decides
//! the outcome, so the result is complete enough to audit by hand.
//!
//! This crate knows nothing about what is being voted on, who the voters are, or where
//! the result goes.

mod error;
mod evaluate;
mod result;

pub use error::{BallotIssueV1, EvaluationErrorV1};
pub use evaluate::evaluate;
pub use result::{
    ComparisonV1, ElectorateSummaryV1, OutcomeV1, ParticipationV1, QuorumOutcomeV1,
    QuorumRequirementV1, ReasonCodeV1, TallyV1, ThresholdOutcomeV1, VoteEvaluationV1,
};
