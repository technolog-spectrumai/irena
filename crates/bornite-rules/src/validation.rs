//! Rule-versus-electorate validation.
//!
//! The typed rules are internally consistent by construction. What can still be wrong
//! is the pairing of rules with a particular frozen electorate: a rule that says every
//! vote counts one while the electorate declares weights, or a quorum no electorate of
//! this size could ever reach. Those are contradictions between what the caller
//! believes and what they supplied, and they are refused loudly rather than resolved
//! quietly one way or the other.
//!
//! Every issue is collected and reported together, in a deterministic order.

use crate::rules::{QuorumRuleV1, ThresholdRuleV1, VotingRulesV1, WeightRuleV1};
use bornite_core::{ElectorateV1, FractionV1, VoterIdV1, WeightTotalV1, WeightV1};

/// One thing wrong with a rules-and-electorate pairing.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(tag = "issue", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ValidationIssueV1 {
    /// The rules say every vote counts one, but this voter declares another weight.
    #[error("weights are equal by rule, but voter {voter} declares weight {weight}")]
    ContradictoryWeight {
        /// The voter.
        voter: VoterIdV1,
        /// The weight they declare.
        weight: WeightV1,
    },
    /// The rules disable exclusions, but this voter is marked excluded.
    #[error("exclusions are disabled by rule, but voter {voter} is marked excluded")]
    ContradictoryExclusion {
        /// The voter.
        voter: VoterIdV1,
    },
    /// An absolute quorum exceeds the whole electorate's weight.
    #[error("an absolute quorum of {required} can never be met by an electorate weighing {total}")]
    UnachievableQuorum {
        /// The required participating weight.
        required: WeightV1,
        /// The electorate's total weight.
        total: WeightTotalV1,
    },
    /// A rule fraction is above one.
    #[error("{rule} fraction {fraction} is not a proportion")]
    ImproperFraction {
        /// Which rule: `"quorum"` or `"threshold"`.
        rule: &'static str,
        /// The offending fraction.
        fraction: FractionV1,
    },
    /// The electorate's weights sum past `u64::MAX`.
    #[error("the electorate's total weight exceeds the maximum this engine can represent")]
    WeightOverflow,
}

/// Every issue found, sorted.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq, serde::Serialize)]
#[error("{} validation issue(s): {}", .issues.len(), render(.issues))]
pub struct ValidationErrorV1 {
    /// The issues, in a deterministic order. Never empty.
    pub issues: Vec<ValidationIssueV1>,
}

fn render(issues: &[ValidationIssueV1]) -> String {
    issues
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// Checks a rule set against the electorate it will be applied to.
///
/// # Errors
///
/// Returns every [`ValidationIssueV1`] found, together and sorted.
pub fn validate(rules: &VotingRulesV1, electorate: &ElectorateV1) -> Result<(), ValidationErrorV1> {
    let mut issues = Vec::new();

    if let QuorumRuleV1::Fraction { fraction, .. } = rules.quorum
        && !fraction.is_proportion()
    {
        issues.push(ValidationIssueV1::ImproperFraction {
            rule: "quorum",
            fraction,
        });
    }
    if let ThresholdRuleV1::Fraction { fraction, .. } = rules.threshold
        && !fraction.is_proportion()
    {
        issues.push(ValidationIssueV1::ImproperFraction {
            rule: "threshold",
            fraction,
        });
    }

    for voter in electorate.voters() {
        if rules.weight == WeightRuleV1::Equal && voter.weight != WeightV1::ONE {
            issues.push(ValidationIssueV1::ContradictoryWeight {
                voter: voter.id.clone(),
                weight: voter.weight,
            });
        }
        if !rules.exclusions_enabled && voter.excluded {
            issues.push(ValidationIssueV1::ContradictoryExclusion {
                voter: voter.id.clone(),
            });
        }
    }

    if let QuorumRuleV1::Absolute { weight: required } = rules.quorum {
        let weights = electorate.voters().iter().map(|voter| match rules.weight {
            WeightRuleV1::Equal => WeightV1::ONE,
            WeightRuleV1::Electorate => voter.weight,
        });
        match WeightTotalV1::sum(weights) {
            Ok(total) if required.value() > total.value() => {
                issues.push(ValidationIssueV1::UnachievableQuorum { required, total });
            }
            Ok(_) => {}
            Err(_) => issues.push(ValidationIssueV1::WeightOverflow),
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        issues.sort();
        issues.dedup();
        Err(ValidationErrorV1 { issues })
    }
}
