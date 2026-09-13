//! Typed voting rules and rule-versus-electorate validation.
//!
//! [`VotingRulesV1`] is the typed form of a `<voting-rules>` document. It carries
//! exactly what the document carries — weights, exclusions, quorum, threshold,
//! abstentions, tie — and nothing about who is voting or why.
//!
//! [`validate`] checks a rule set against the electorate it will be applied to and
//! reports every contradiction at once. It never resolves a contradiction by picking a
//! side: a rule set that says "equal weights" over an electorate that declares weights
//! is a mistake by whoever paired them, and the engine's job is to say so.

mod rules;
mod validation;

pub use rules::{
    AbstentionTreatmentV1, QuorumBasisV1, QuorumRuleV1, ThresholdBasisV1, ThresholdRuleV1,
    TieTreatmentV1, VotingRulesV1, WeightRuleV1,
};
pub use validation::{ValidationErrorV1, ValidationIssueV1, validate};
