//! The typed rules.
//!
//! Every enumeration here mirrors one attribute of the `<voting-rules>` document, and
//! carries nothing the document does not: no organisation, no purpose, no context. The
//! same rules organise a company meeting, a membership vote, or a fleet of drones
//! deciding a peaceful deployment.

use bornite_core::{FractionV1, WeightV1};

/// How each voter's weight is resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WeightRuleV1 {
    /// Every voter counts one.
    Equal,
    /// Each voter counts the weight the electorate declares.
    Electorate,
}

/// What a fractional quorum is a share of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuorumBasisV1 {
    /// Every voter, before exclusions.
    TotalElectorate,
    /// The voters left after exclusions.
    EffectiveElectorate,
}

/// The participation a vote needs before it can decide anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum QuorumRuleV1 {
    /// No participation requirement.
    None,
    /// At least this much weight must participate.
    Absolute {
        /// The required participating weight.
        weight: WeightV1,
    },
    /// At least this share of the basis must participate.
    Fraction {
        /// The required share.
        fraction: FractionV1,
        /// What it is a share of.
        basis: QuorumBasisV1,
    },
}

/// What the YES weight is measured against: the threshold's denominator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThresholdBasisV1 {
    /// The weight that cast a ballot, with abstentions per the abstention rule.
    VotesCast,
    /// The voters left after exclusions, whether or not they voted.
    EffectiveElectorate,
    /// Every voter, whether or not they voted.
    TotalElectorate,
}

/// The share of the basis the YES weight must reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ThresholdRuleV1 {
    /// One half. Evaluated exactly as `Fraction { 1/2 }`; kept distinct so the result
    /// echoes what the rule said.
    SimpleMajority {
        /// The denominator.
        basis: ThresholdBasisV1,
    },
    /// An explicit share.
    Fraction {
        /// The required share, at most one.
        fraction: FractionV1,
        /// The denominator.
        basis: ThresholdBasisV1,
    },
}

impl ThresholdRuleV1 {
    /// The share the YES weight must reach.
    #[must_use]
    pub const fn fraction(self) -> FractionV1 {
        match self {
            Self::SimpleMajority { .. } => FractionV1::HALF,
            Self::Fraction { fraction, .. } => fraction,
        }
    }

    /// The denominator.
    #[must_use]
    pub const fn basis(self) -> ThresholdBasisV1 {
        match self {
            Self::SimpleMajority { basis } | Self::Fraction { basis, .. } => basis,
        }
    }
}

/// How abstentions figure in a votes-cast denominator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AbstentionTreatmentV1 {
    /// Abstentions are left out of the denominator: YES is measured against YES + NO.
    Exclude,
    /// Abstentions stay in the denominator: YES is measured against everyone who voted,
    /// so an abstention counts against a YES threshold.
    Include,
}

/// What happens when the threshold is met exactly.
///
/// This is the only boundary rule. The threshold comparison is always strict, and a
/// result that lands exactly on it is a tie: `Reject` makes the rule "more than",
/// `Accept` makes it "at least".
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TieTreatmentV1 {
    /// A tie fails.
    Reject,
    /// A tie passes.
    Accept,
}

/// A complete set of voting rules, version 1.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct VotingRulesV1 {
    /// How weights are resolved.
    pub weight: WeightRuleV1,
    /// Whether voters marked excluded are removed from the effective electorate.
    pub exclusions_enabled: bool,
    /// The participation requirement.
    pub quorum: QuorumRuleV1,
    /// The passing requirement.
    pub threshold: ThresholdRuleV1,
    /// How abstentions figure in a votes-cast denominator.
    pub abstentions: AbstentionTreatmentV1,
    /// What happens at exact equality.
    pub tie: TieTreatmentV1,
}

impl VotingRulesV1 {
    /// Whether the abstention rule can change this rule set's threshold denominator.
    ///
    /// Only a votes-cast denominator is affected. With an electorate denominator the
    /// abstention rule is still valid and still recorded, but it changes nothing; the
    /// evaluation says so explicitly rather than leaving it to be inferred.
    #[must_use]
    pub const fn abstentions_affect_denominator(&self) -> bool {
        matches!(self.threshold.basis(), ThresholdBasisV1::VotesCast)
    }
}
