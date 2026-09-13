//! The algorithm, in its fixed order.
//!
//! ```text
//! 1. validate rules and electorate; detect contradictions
//! 2. apply exclusions            → the effective voter set
//! 3. resolve weights             → per-voter weight; total and effective weight
//! 4. validate ballots            → unknown voter, excluded voter
//! 5. participation and quorum    → participating weight; quorum met (inclusive)
//! 6. tally                       → yes / no / abstain, weight and count
//! 7. threshold denominator       → votes-cast (abstentions per rule) | effective | total
//! 8. threshold and tie           → cross-multiplied comparison; tie rule at equality
//! 9. produce the evaluation
//! ```
//!
//! Nothing is skipped when an earlier step decides the outcome. A vote that fails
//! quorum still has its tally and its threshold comparison computed and reported,
//! because an auditor needs to see why as well as what.

use crate::error::{BallotIssueV1, EvaluationErrorV1};
use crate::result::{
    ComparisonV1, ElectorateSummaryV1, OutcomeV1, ParticipationV1, QuorumOutcomeV1,
    QuorumRequirementV1, ReasonCodeV1, TallyV1, ThresholdOutcomeV1, VoteEvaluationV1,
};
use bornite_core::{
    BallotSetV1, ChoiceV1, CoreError, ElectorateV1, VoterV1, WeightTotalV1, WeightV1,
};
use bornite_rules::{
    AbstentionTreatmentV1, QuorumBasisV1, QuorumRuleV1, ThresholdBasisV1, TieTreatmentV1,
    VotingRulesV1, WeightRuleV1,
};
use core::cmp::Ordering;

/// Evaluates one vote.
///
/// Deterministic: the same rules, electorate and ballots produce the same result on
/// every machine, in every input order, at any time. No floating point, no hashing,
/// no clock, no randomness, no locale.
///
/// # Errors
///
/// Returns [`EvaluationErrorV1`] when the inputs are not a vote the engine can
/// evaluate: contradictory rules, an unknown or excluded voter on a ballot, or a weight
/// total past `u64::MAX`. A vote that merely fails is a successful evaluation with
/// `outcome: Rejected`.
pub fn evaluate(
    rules: &VotingRulesV1,
    electorate: &ElectorateV1,
    ballots: &BallotSetV1,
) -> Result<VoteEvaluationV1, EvaluationErrorV1> {
    // 1. Validate rules and electorate.
    bornite_rules::validate(rules, electorate).map_err(EvaluationErrorV1::Validation)?;

    // 2. Apply exclusions.
    let is_effective = |voter: &VoterV1| !(rules.exclusions_enabled && voter.excluded);

    // 3. Resolve weights.
    let weight_of = |voter: &VoterV1| match rules.weight {
        WeightRuleV1::Equal => WeightV1::ONE,
        WeightRuleV1::Electorate => voter.weight,
    };
    let total_weight = sum(electorate.voters().iter().map(weight_of))?;
    let effective_weight = sum(electorate
        .voters()
        .iter()
        .filter(|v| is_effective(v))
        .map(weight_of))?;
    let effective_voter_count = count(electorate.voters().iter().filter(|v| is_effective(v)));
    let voter_count = count(electorate.voters().iter());

    // 4. Validate ballots. Duplicates cannot reach here: the ballot set refuses them.
    let mut issues = Vec::new();
    for ballot in ballots.ballots() {
        match electorate.get(&ballot.voter) {
            None => issues.push(BallotIssueV1::UnknownVoter {
                voter: ballot.voter.clone(),
            }),
            Some(voter) if !is_effective(voter) => {
                issues.push(BallotIssueV1::ExcludedVoter {
                    voter: ballot.voter.clone(),
                });
            }
            Some(_) => {}
        }
    }
    if !issues.is_empty() {
        issues.sort();
        return Err(EvaluationErrorV1::Ballots { issues });
    }

    // 5. Participation and quorum.
    let ballot_weights = || {
        ballots.ballots().iter().map(|ballot| {
            let voter = electorate.get(&ballot.voter).expect("validated in step 4");
            (ballot.choice, weight_of(voter))
        })
    };
    let participating_weight = sum(ballot_weights().map(|(_, w)| w))?;
    let ballot_count = count(ballots.ballots().iter());
    let non_participant_count = effective_voter_count - ballot_count;
    let non_participant_weight = effective_weight
        .checked_sub_total(participating_weight)
        .expect("participants are a subset of the effective electorate, validated in step 4");

    let requirement = match rules.quorum {
        QuorumRuleV1::None => QuorumRequirementV1::None,
        QuorumRuleV1::Absolute { weight } => QuorumRequirementV1::Absolute { weight },
        QuorumRuleV1::Fraction { fraction, basis } => QuorumRequirementV1::Fraction {
            fraction,
            basis,
            basis_weight: match basis {
                QuorumBasisV1::TotalElectorate => total_weight,
                QuorumBasisV1::EffectiveElectorate => effective_weight,
            },
        },
    };
    let quorum_met = match requirement {
        QuorumRequirementV1::None => true,
        QuorumRequirementV1::Absolute { weight } => participating_weight.value() >= weight.value(),
        QuorumRequirementV1::Fraction {
            fraction,
            basis_weight,
            ..
        } => {
            fraction.compare_share(participating_weight.value(), basis_weight.value())
                != Ordering::Less
        }
    };

    // 6. Tally.
    let mut tally = TallyV1 {
        yes_weight: WeightTotalV1::ZERO,
        no_weight: WeightTotalV1::ZERO,
        abstain_weight: WeightTotalV1::ZERO,
        yes_count: 0,
        no_count: 0,
        abstain_count: 0,
    };
    for (choice, weight) in ballot_weights() {
        let (bucket, counter) = match choice {
            ChoiceV1::Yes => (&mut tally.yes_weight, &mut tally.yes_count),
            ChoiceV1::No => (&mut tally.no_weight, &mut tally.no_count),
            ChoiceV1::Abstain => (&mut tally.abstain_weight, &mut tally.abstain_count),
        };
        *bucket = bucket.checked_add(weight).map_err(overflow)?;
        *counter += 1;
    }

    // 7. Threshold denominator.
    let basis = rules.threshold.basis();
    let denominator_weight = match basis {
        ThresholdBasisV1::VotesCast => match rules.abstentions {
            AbstentionTreatmentV1::Exclude => tally
                .yes_weight
                .checked_add_total(tally.no_weight)
                .map_err(overflow)?,
            AbstentionTreatmentV1::Include => participating_weight,
        },
        ThresholdBasisV1::EffectiveElectorate => effective_weight,
        ThresholdBasisV1::TotalElectorate => total_weight,
    };

    // 8. Threshold and tie.
    let required_fraction = rules.threshold.fraction();
    let comparison = match required_fraction
        .compare_share(tally.yes_weight.value(), denominator_weight.value())
    {
        Ordering::Greater => ComparisonV1::Above,
        Ordering::Equal => ComparisonV1::Exactly,
        Ordering::Less => ComparisonV1::Below,
    };
    let threshold_met = match comparison {
        ComparisonV1::Above => true,
        ComparisonV1::Below => false,
        ComparisonV1::Exactly => matches!(rules.tie, TieTreatmentV1::Accept),
    } && !denominator_weight.is_zero();

    let (outcome, reason) = if !quorum_met {
        (OutcomeV1::Rejected, ReasonCodeV1::QuorumNotMet)
    } else if denominator_weight.is_zero() {
        (OutcomeV1::Rejected, ReasonCodeV1::EmptyThresholdBasis)
    } else {
        match (comparison, rules.tie) {
            (ComparisonV1::Above, _) => (OutcomeV1::Accepted, ReasonCodeV1::ThresholdMet),
            (ComparisonV1::Below, _) => (OutcomeV1::Rejected, ReasonCodeV1::ThresholdNotMet),
            (ComparisonV1::Exactly, TieTreatmentV1::Accept) => {
                (OutcomeV1::Accepted, ReasonCodeV1::TieAccepted)
            }
            (ComparisonV1::Exactly, TieTreatmentV1::Reject) => {
                (OutcomeV1::Rejected, ReasonCodeV1::TieRejected)
            }
        }
    };

    // 9. Produce the evaluation.
    Ok(VoteEvaluationV1 {
        version: bornite_core::VERSION,
        rules: rules.clone(),
        electorate: ElectorateSummaryV1 {
            voter_count,
            excluded_count: voter_count - effective_voter_count,
            effective_voter_count,
            total_weight,
            effective_weight,
        },
        participation: ParticipationV1 {
            ballot_count,
            weight: participating_weight,
            non_participant_count,
            non_participant_weight,
        },
        tally,
        quorum: QuorumOutcomeV1 {
            requirement,
            actual_weight: participating_weight,
            met: quorum_met,
        },
        threshold: ThresholdOutcomeV1 {
            basis,
            denominator_weight,
            required_fraction,
            abstentions_affected_denominator: rules.abstentions_affect_denominator(),
            yes_weight: tally.yes_weight,
            comparison,
            met: threshold_met,
        },
        outcome,
        reason,
    })
}

fn sum(weights: impl IntoIterator<Item = WeightV1>) -> Result<WeightTotalV1, EvaluationErrorV1> {
    WeightTotalV1::sum(weights).map_err(overflow)
}

fn overflow(_: CoreError) -> EvaluationErrorV1 {
    EvaluationErrorV1::WeightOverflow
}

fn count<T>(items: impl Iterator<Item = T>) -> u64 {
    u64::try_from(items.count()).expect("counts fit in u64")
}
