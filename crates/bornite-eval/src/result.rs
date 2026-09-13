//! The evaluation result and everything it is made of.
//!
//! A result is complete on purpose. It carries the rules it was produced under, every
//! intermediate quantity, both requirements with what they were measured against, and
//! a typed reason — so an auditor can check every step from the result alone, without
//! re-running anything.

use bornite_core::{FractionV1, WeightTotalV1, WeightV1};
use bornite_rules::{QuorumBasisV1, ThresholdBasisV1, VotingRulesV1};

/// Counts and weights of the electorate, before and after exclusions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ElectorateSummaryV1 {
    /// Every voter, including excluded ones.
    pub voter_count: u64,
    /// Voters removed by the exclusion rule.
    pub excluded_count: u64,
    /// Voters left after exclusions.
    pub effective_voter_count: u64,
    /// Weight of every voter, under the weight rule in force.
    pub total_weight: WeightTotalV1,
    /// Weight of the effective voters.
    pub effective_weight: WeightTotalV1,
}

/// Who took part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ParticipationV1 {
    /// Ballots cast, of any choice.
    pub ballot_count: u64,
    /// Weight of the voters who cast a ballot.
    pub weight: WeightTotalV1,
    /// Effective voters who cast nothing.
    pub non_participant_count: u64,
    /// Their weight.
    pub non_participant_weight: WeightTotalV1,
}

/// The three-way tally.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct TallyV1 {
    /// Weight voting YES.
    pub yes_weight: WeightTotalV1,
    /// Weight voting NO.
    pub no_weight: WeightTotalV1,
    /// Weight abstaining.
    pub abstain_weight: WeightTotalV1,
    /// Ballots voting YES.
    pub yes_count: u64,
    /// Ballots voting NO.
    pub no_count: u64,
    /// Ballots abstaining.
    pub abstain_count: u64,
}

/// The quorum rule as it applied to this electorate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum QuorumRequirementV1 {
    /// No requirement.
    None,
    /// A fixed participating weight.
    Absolute {
        /// The required weight.
        weight: WeightV1,
    },
    /// A share of a basis, with the basis weight it was measured against.
    Fraction {
        /// The required share.
        fraction: FractionV1,
        /// What it is a share of.
        basis: QuorumBasisV1,
        /// The basis weight, so the requirement can be checked by hand.
        basis_weight: WeightTotalV1,
    },
}

/// How the quorum came out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct QuorumOutcomeV1 {
    /// The requirement as applied.
    pub requirement: QuorumRequirementV1,
    /// The participating weight measured against it.
    pub actual_weight: WeightTotalV1,
    /// Whether it was met. Quorum is inclusive: exactly the requirement meets it.
    pub met: bool,
}

/// Where the YES weight landed relative to the required share.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonV1 {
    /// Strictly more than the required share.
    Above,
    /// Exactly the required share: a tie.
    Exactly,
    /// Strictly less than the required share.
    Below,
}

/// How the threshold came out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ThresholdOutcomeV1 {
    /// The denominator basis the rule named.
    pub basis: ThresholdBasisV1,
    /// The denominator weight, after the abstention rule where it applies.
    pub denominator_weight: WeightTotalV1,
    /// The share required.
    pub required_fraction: FractionV1,
    /// Whether the abstention rule was able to change the denominator at all.
    ///
    /// False under an electorate denominator: the abstention rule was valid and is
    /// recorded, but it could not have mattered, and this says so explicitly.
    pub abstentions_affected_denominator: bool,
    /// The YES weight measured.
    pub yes_weight: WeightTotalV1,
    /// Where it landed.
    pub comparison: ComparisonV1,
    /// Whether the threshold was met, after the tie rule.
    pub met: bool,
}

/// The decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeV1 {
    /// The motion passed.
    Accepted,
    /// The motion failed.
    Rejected,
}

/// Why the decision came out as it did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReasonCodeV1 {
    /// YES was strictly above the required share.
    ThresholdMet,
    /// YES was strictly below the required share.
    ThresholdNotMet,
    /// YES was exactly the required share and the tie rule accepts.
    TieAccepted,
    /// YES was exactly the required share and the tie rule rejects.
    TieRejected,
    /// Participation fell short of the quorum. The threshold was still computed and is
    /// reported, but it did not decide anything.
    QuorumNotMet,
    /// The threshold denominator weighed nothing, so there was no share to reach.
    EmptyThresholdBasis,
}

impl ReasonCodeV1 {
    /// A stable identifier for machine consumption.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ThresholdMet => "threshold_met",
            Self::ThresholdNotMet => "threshold_not_met",
            Self::TieAccepted => "tie_accepted",
            Self::TieRejected => "tie_rejected",
            Self::QuorumNotMet => "quorum_not_met",
            Self::EmptyThresholdBasis => "empty_threshold_basis",
        }
    }
}

impl core::fmt::Display for ReasonCodeV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The complete, auditable result of one vote.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct VoteEvaluationV1 {
    /// Always 1.
    pub version: u32,
    /// The rules this result was produced under, echoed so the result is self-describing.
    pub rules: VotingRulesV1,
    /// The electorate before and after exclusions.
    pub electorate: ElectorateSummaryV1,
    /// Who took part.
    pub participation: ParticipationV1,
    /// The three-way tally.
    pub tally: TallyV1,
    /// The quorum as applied.
    pub quorum: QuorumOutcomeV1,
    /// The threshold as applied.
    pub threshold: ThresholdOutcomeV1,
    /// The decision.
    pub outcome: OutcomeV1,
    /// Why.
    pub reason: ReasonCodeV1,
}

impl VoteEvaluationV1 {
    /// Whether the motion passed.
    #[must_use]
    pub const fn accepted(&self) -> bool {
        matches!(self.outcome, OutcomeV1::Accepted)
    }
}
