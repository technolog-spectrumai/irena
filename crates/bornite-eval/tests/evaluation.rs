//! Boundary, ordering, weight, abstention, quorum, threshold, tie, invalid-input and
//! overflow behaviour of the evaluation.

mod support;

use bornite_core::{BallotSetV1, ChoiceV1, ElectorateV1, WeightV1};
use bornite_eval::{
    BallotIssueV1, ComparisonV1, EvaluationErrorV1, OutcomeV1, QuorumRequirementV1, ReasonCodeV1,
    evaluate,
};
use bornite_rules::{
    AbstentionTreatmentV1, QuorumBasisV1, ThresholdBasisV1, ThresholdRuleV1, TieTreatmentV1,
    VotingRulesV1, WeightRuleV1,
};
use support::*;

// ------------------------------------------------------------------ boundaries

#[test]
fn a_clear_majority_is_accepted_with_the_full_report() {
    let e = electorate(vec![voter("a", 1), voter("b", 1), voter("c", 1)]);
    let result = evaluate(&rules(), &e, &ballots(&[yes("a"), yes("b"), no("c")])).expect("ok");

    assert_eq!(result.outcome, OutcomeV1::Accepted);
    assert_eq!(result.reason, ReasonCodeV1::ThresholdMet);
    assert_eq!(result.version, 1);
    assert_eq!(result.electorate.voter_count, 3);
    assert_eq!(result.electorate.total_weight.value(), 3);
    assert_eq!(result.participation.ballot_count, 3);
    assert_eq!(result.tally.yes_weight.value(), 2);
    assert_eq!(result.tally.no_weight.value(), 1);
    assert_eq!(result.threshold.denominator_weight.value(), 3);
    assert_eq!(result.threshold.comparison, ComparisonV1::Above);
    assert!(result.threshold.met);
    assert!(result.quorum.met);
    assert_eq!(result.quorum.requirement, QuorumRequirementV1::None);
}

#[test]
fn a_single_voter_decides_alone() {
    let e = electorate(vec![voter("solo", 1)]);
    assert!(
        evaluate(&rules(), &e, &ballots(&[yes("solo")]))
            .expect("ok")
            .accepted()
    );
    assert!(
        !evaluate(&rules(), &e, &ballots(&[no("solo")]))
            .expect("ok")
            .accepted()
    );
}

#[test]
fn an_empty_electorate_has_an_empty_threshold_basis() {
    let result = evaluate(&rules(), &electorate(vec![]), &BallotSetV1::empty()).expect("ok");
    assert_eq!(result.outcome, OutcomeV1::Rejected);
    assert_eq!(result.reason, ReasonCodeV1::EmptyThresholdBasis);
    assert_eq!(result.threshold.denominator_weight.value(), 0);
    assert!(!result.threshold.met);
}

#[test]
fn no_ballots_at_all_is_an_empty_basis_not_a_tie() {
    let e = electorate(vec![voter("a", 1), voter("b", 1)]);
    let result = evaluate(&rules(), &e, &BallotSetV1::empty()).expect("ok");
    assert_eq!(result.reason, ReasonCodeV1::EmptyThresholdBasis);
    assert_eq!(result.participation.non_participant_count, 2);
    assert_eq!(result.participation.non_participant_weight.value(), 2);
}

#[test]
fn everyone_abstaining_with_abstentions_excluded_is_an_empty_basis() {
    let e = electorate(vec![voter("a", 1), voter("b", 1)]);
    let result = evaluate(&rules(), &e, &ballots(&[abstain("a"), abstain("b")])).expect("ok");
    assert_eq!(result.reason, ReasonCodeV1::EmptyThresholdBasis);
    assert_eq!(
        result.participation.weight.value(),
        2,
        "abstentions still participate"
    );
    assert_eq!(result.tally.abstain_count, 2);
}

#[test]
fn everyone_abstaining_with_abstentions_included_fails_the_threshold() {
    let e = electorate(vec![voter("a", 1), voter("b", 1)]);
    let r = VotingRulesV1 {
        abstentions: AbstentionTreatmentV1::Include,
        ..rules()
    };
    let result = evaluate(&r, &e, &ballots(&[abstain("a"), abstain("b")])).expect("ok");
    assert_eq!(result.reason, ReasonCodeV1::ThresholdNotMet);
    assert_eq!(result.threshold.denominator_weight.value(), 2);
    assert_eq!(result.threshold.yes_weight.value(), 0);
}

#[test]
fn zero_yes_against_some_no_is_below() {
    let e = electorate(vec![voter("a", 1), voter("b", 1)]);
    let result = evaluate(&rules(), &e, &ballots(&[no("a"), no("b")])).expect("ok");
    assert_eq!(result.reason, ReasonCodeV1::ThresholdNotMet);
    assert_eq!(result.threshold.comparison, ComparisonV1::Below);
}

#[test]
fn everyone_excluded_leaves_an_empty_effective_electorate() {
    let e = electorate(vec![excluded("a", 1), excluded("b", 1)]);
    let result = evaluate(&rules(), &e, &BallotSetV1::empty()).expect("ok");
    assert_eq!(result.electorate.effective_voter_count, 0);
    assert_eq!(result.electorate.effective_weight.value(), 0);
    assert_eq!(result.electorate.excluded_count, 2);
    assert_eq!(result.reason, ReasonCodeV1::EmptyThresholdBasis);
}

// ------------------------------------------------------------------- ordering

#[test]
fn the_result_does_not_depend_on_input_order() {
    let forward = electorate(vec![voter("a", 3), voter("b", 2), voter("c", 5)]);
    let backward = electorate(vec![voter("c", 5), voter("b", 2), voter("a", 3)]);
    let one = ballots(&[yes("a"), no("b"), yes("c")]);
    let other = ballots(&[yes("c"), yes("a"), no("b")]);

    let left = evaluate(&rules(), &forward, &one).expect("ok");
    let right = evaluate(&rules(), &backward, &other).expect("ok");
    assert_eq!(left, right);
    assert_eq!(
        serde_json::to_string(&left).expect("json"),
        serde_json::to_string(&right).expect("json")
    );
}

// ------------------------------------------------------------------- weights

#[test]
fn electorate_weights_change_what_equal_weights_would_decide() {
    let e = electorate(vec![
        voter("whale", 10),
        voter("a", 1),
        voter("b", 1),
        voter("c", 1),
    ]);
    let cast = ballots(&[yes("whale"), no("a"), no("b"), no("c")]);

    let weighted = evaluate(&rules(), &e, &cast).expect("ok");
    assert_eq!(weighted.outcome, OutcomeV1::Accepted, "10 against 3");
    assert_eq!(weighted.tally.yes_weight.value(), 10);
    assert_eq!(weighted.tally.yes_count, 1);

    let equal_e = electorate(vec![
        voter("whale", 1),
        voter("a", 1),
        voter("b", 1),
        voter("c", 1),
    ]);
    let equal = evaluate(
        &VotingRulesV1 {
            weight: WeightRuleV1::Equal,
            ..rules()
        },
        &equal_e,
        &cast,
    )
    .expect("ok");
    assert_eq!(equal.outcome, OutcomeV1::Rejected, "1 against 3");
}

#[test]
fn equal_weights_ignore_nothing_because_they_are_validated_first() {
    let e = electorate(vec![voter("a", 7), voter("b", 1)]);
    let error = evaluate(
        &VotingRulesV1 {
            weight: WeightRuleV1::Equal,
            ..rules()
        },
        &e,
        &BallotSetV1::empty(),
    )
    .expect_err("contradiction");
    assert!(matches!(error, EvaluationErrorV1::Validation(_)), "{error}");
}

// ---------------------------------------------------------------- abstentions

#[test]
fn the_abstention_rule_flips_the_outcome() {
    // 2 yes, 1 no, 2 abstain. Excluded: 2/3 > 1/2 passes. Included: 2/5 < 1/2 fails.
    let e = electorate(vec![
        voter("a", 1),
        voter("b", 1),
        voter("c", 1),
        voter("d", 1),
        voter("e", 1),
    ]);
    let cast = ballots(&[yes("a"), yes("b"), no("c"), abstain("d"), abstain("e")]);

    let excluded = evaluate(&rules(), &e, &cast).expect("ok");
    assert_eq!(excluded.outcome, OutcomeV1::Accepted);
    assert_eq!(excluded.threshold.denominator_weight.value(), 3);
    assert!(excluded.threshold.abstentions_affected_denominator);

    let included = evaluate(
        &VotingRulesV1 {
            abstentions: AbstentionTreatmentV1::Include,
            ..rules()
        },
        &e,
        &cast,
    )
    .expect("ok");
    assert_eq!(included.outcome, OutcomeV1::Rejected);
    assert_eq!(included.threshold.denominator_weight.value(), 5);
}

#[test]
fn the_abstention_rule_is_reported_inert_under_an_electorate_denominator() {
    let e = electorate(vec![voter("a", 1), voter("b", 1), voter("c", 1)]);
    let r = VotingRulesV1 {
        threshold: ThresholdRuleV1::SimpleMajority {
            basis: ThresholdBasisV1::EffectiveElectorate,
        },
        ..rules()
    };
    let result = evaluate(&r, &e, &ballots(&[yes("a"), yes("b"), abstain("c")])).expect("ok");
    assert!(!result.threshold.abstentions_affected_denominator);
    assert_eq!(result.threshold.denominator_weight.value(), 3);
    assert_eq!(result.outcome, OutcomeV1::Accepted);
}

// -------------------------------------------------------------------- quorum

#[test]
fn a_missed_quorum_rejects_but_still_reports_everything() {
    let e = electorate(vec![
        voter("a", 1),
        voter("b", 1),
        voter("c", 1),
        voter("d", 1),
    ]);
    let r = VotingRulesV1 {
        quorum: quorum_fraction(1, 2, QuorumBasisV1::EffectiveElectorate),
        ..rules()
    };
    let result = evaluate(&r, &e, &ballots(&[yes("a")])).expect("ok");

    assert_eq!(result.outcome, OutcomeV1::Rejected);
    assert_eq!(result.reason, ReasonCodeV1::QuorumNotMet);
    assert!(!result.quorum.met);
    // The threshold was still evaluated: 1 yes of 1 cast is above one half.
    assert_eq!(result.threshold.comparison, ComparisonV1::Above);
    assert!(result.threshold.met);
    assert_eq!(result.tally.yes_weight.value(), 1);
}

#[test]
fn quorum_is_inclusive_at_the_boundary() {
    let e = electorate(vec![
        voter("a", 1),
        voter("b", 1),
        voter("c", 1),
        voter("d", 1),
    ]);
    let r = VotingRulesV1 {
        quorum: quorum_fraction(1, 2, QuorumBasisV1::EffectiveElectorate),
        ..rules()
    };
    // Exactly half participate.
    let result = evaluate(&r, &e, &ballots(&[yes("a"), yes("b")])).expect("ok");
    assert!(result.quorum.met, "exactly the requirement meets a quorum");
    assert_eq!(result.outcome, OutcomeV1::Accepted);
}

#[test]
fn quorum_counts_abstentions_as_participation() {
    let e = electorate(vec![
        voter("a", 1),
        voter("b", 1),
        voter("c", 1),
        voter("d", 1),
    ]);
    let r = VotingRulesV1 {
        quorum: quorum_fraction(3, 4, QuorumBasisV1::EffectiveElectorate),
        ..rules()
    };
    let result = evaluate(&r, &e, &ballots(&[yes("a"), abstain("b"), abstain("c")])).expect("ok");
    assert!(result.quorum.met);
    assert_eq!(result.quorum.actual_weight.value(), 3);
}

#[test]
fn an_absolute_quorum_is_a_fixed_weight() {
    let e = electorate(vec![voter("a", 5), voter("b", 5)]);
    let r = VotingRulesV1 {
        quorum: quorum_absolute(6),
        ..rules()
    };
    assert!(
        !evaluate(&r, &e, &ballots(&[yes("a")]))
            .expect("ok")
            .quorum
            .met
    );
    assert!(
        evaluate(&r, &e, &ballots(&[yes("a"), no("b")]))
            .expect("ok")
            .quorum
            .met
    );
}

#[test]
fn the_quorum_basis_decides_whether_excluded_voters_count() {
    // 2 effective (a, b) plus an excluded c weighing 6. Half of total = 4; half of effective = 1.
    let e = electorate(vec![voter("a", 1), voter("b", 1), excluded("c", 6)]);
    let cast = ballots(&[yes("a")]);

    let of_total = VotingRulesV1 {
        quorum: quorum_fraction(1, 2, QuorumBasisV1::TotalElectorate),
        ..rules()
    };
    let r1 = evaluate(&of_total, &e, &cast).expect("ok");
    assert!(!r1.quorum.met);
    assert!(
        matches!(r1.quorum.requirement, QuorumRequirementV1::Fraction { basis_weight, .. } if basis_weight.value() == 8)
    );

    let of_effective = VotingRulesV1 {
        quorum: quorum_fraction(1, 2, QuorumBasisV1::EffectiveElectorate),
        ..rules()
    };
    let r2 = evaluate(&of_effective, &e, &cast).expect("ok");
    assert!(r2.quorum.met);
    assert!(
        matches!(r2.quorum.requirement, QuorumRequirementV1::Fraction { basis_weight, .. } if basis_weight.value() == 2)
    );
}

#[test]
fn a_quorum_of_a_share_of_nothing_is_met_by_nothing() {
    let e = electorate(vec![excluded("a", 1)]);
    let r = VotingRulesV1 {
        quorum: quorum_fraction(1, 2, QuorumBasisV1::EffectiveElectorate),
        ..rules()
    };
    let result = evaluate(&r, &e, &BallotSetV1::empty()).expect("ok");
    assert!(result.quorum.met, "0 >= 1/2 of 0");
    assert_eq!(result.reason, ReasonCodeV1::EmptyThresholdBasis);
}

// ----------------------------------------------------------------- threshold

#[test]
fn fractional_thresholds_across_every_denominator() {
    // 6 voters weighing 1. 4 yes, 1 no, 1 silent. Two thirds required.
    let e = electorate(vec![
        voter("a", 1),
        voter("b", 1),
        voter("c", 1),
        voter("d", 1),
        voter("e", 1),
        voter("f", 1),
    ]);
    let cast = ballots(&[yes("a"), yes("b"), yes("c"), yes("d"), no("e")]);

    // Of votes cast: 4/5 > 2/3.
    let of_cast = VotingRulesV1 {
        threshold: threshold(2, 3, ThresholdBasisV1::VotesCast),
        ..rules()
    };
    assert!(evaluate(&of_cast, &e, &cast).expect("ok").accepted());

    // Of the effective electorate: 4/6 == 2/3, a tie, rejected by default.
    let of_effective = VotingRulesV1 {
        threshold: threshold(2, 3, ThresholdBasisV1::EffectiveElectorate),
        ..rules()
    };
    let r = evaluate(&of_effective, &e, &cast).expect("ok");
    assert_eq!(r.threshold.comparison, ComparisonV1::Exactly);
    assert_eq!(r.reason, ReasonCodeV1::TieRejected);

    // Of the total electorate with one more, excluded, voter: 4/7 < 2/3.
    let e7 = electorate(vec![
        voter("a", 1),
        voter("b", 1),
        voter("c", 1),
        voter("d", 1),
        voter("e", 1),
        voter("f", 1),
        excluded("g", 1),
    ]);
    let of_total = VotingRulesV1 {
        threshold: threshold(2, 3, ThresholdBasisV1::TotalElectorate),
        ..rules()
    };
    let r = evaluate(&of_total, &e7, &cast).expect("ok");
    assert_eq!(r.threshold.denominator_weight.value(), 7);
    assert_eq!(r.reason, ReasonCodeV1::ThresholdNotMet);
}

#[test]
fn simple_majority_evaluates_as_exactly_one_half() {
    let e = electorate(vec![
        voter("a", 1),
        voter("b", 1),
        voter("c", 1),
        voter("d", 1),
    ]);
    let majority = evaluate(
        &rules(),
        &e,
        &ballots(&[yes("a"), yes("b"), no("c"), no("d")]),
    )
    .expect("ok");
    let half = evaluate(
        &VotingRulesV1 {
            threshold: threshold(1, 2, ThresholdBasisV1::VotesCast),
            ..rules()
        },
        &e,
        &ballots(&[yes("a"), yes("b"), no("c"), no("d")]),
    )
    .expect("ok");
    assert_eq!(majority.outcome, half.outcome);
    assert_eq!(majority.reason, half.reason);
    assert_eq!(majority.threshold.comparison, ComparisonV1::Exactly);
}

#[test]
fn a_zero_threshold_needs_at_least_one_yes() {
    let e = electorate(vec![voter("a", 1), voter("b", 1)]);
    let r = VotingRulesV1 {
        threshold: threshold(0, 1, ThresholdBasisV1::VotesCast),
        ..rules()
    };
    // 0 yes of 1 cast is exactly 0/1: a tie, rejected.
    assert_eq!(
        evaluate(&r, &e, &ballots(&[no("a")])).expect("ok").reason,
        ReasonCodeV1::TieRejected
    );
    // 1 yes of 2 cast is above 0.
    assert!(
        evaluate(&r, &e, &ballots(&[yes("a"), no("b")]))
            .expect("ok")
            .accepted()
    );
}

#[test]
fn a_unanimity_threshold_is_met_exactly_by_everyone() {
    let e = electorate(vec![voter("a", 1), voter("b", 1)]);
    let r = VotingRulesV1 {
        threshold: threshold(1, 1, ThresholdBasisV1::VotesCast),
        tie: TieTreatmentV1::Accept,
        ..rules()
    };
    assert_eq!(
        evaluate(&r, &e, &ballots(&[yes("a"), yes("b")]))
            .expect("ok")
            .reason,
        ReasonCodeV1::TieAccepted
    );
    assert_eq!(
        evaluate(&r, &e, &ballots(&[yes("a"), no("b")]))
            .expect("ok")
            .reason,
        ReasonCodeV1::ThresholdNotMet
    );
}

// ----------------------------------------------------------------------- ties

#[test]
fn the_tie_rule_decides_exact_equality_at_one_half() {
    let e = electorate(vec![voter("a", 1), voter("b", 1)]);
    let cast = ballots(&[yes("a"), no("b")]);

    let rejected = evaluate(&rules(), &e, &cast).expect("ok");
    assert_eq!(rejected.threshold.comparison, ComparisonV1::Exactly);
    assert_eq!(rejected.reason, ReasonCodeV1::TieRejected);
    assert_eq!(rejected.outcome, OutcomeV1::Rejected);

    let accepted = evaluate(
        &VotingRulesV1 {
            tie: TieTreatmentV1::Accept,
            ..rules()
        },
        &e,
        &cast,
    )
    .expect("ok");
    assert_eq!(accepted.reason, ReasonCodeV1::TieAccepted);
    assert_eq!(accepted.outcome, OutcomeV1::Accepted);
}

#[test]
fn the_tie_rule_decides_exact_equality_at_two_thirds() {
    let e = electorate(vec![voter("a", 1), voter("b", 1), voter("c", 1)]);
    let cast = ballots(&[yes("a"), yes("b"), no("c")]);
    let two_thirds = threshold(2, 3, ThresholdBasisV1::VotesCast);

    let r = evaluate(
        &VotingRulesV1 {
            threshold: two_thirds,
            ..rules()
        },
        &e,
        &cast,
    )
    .expect("ok");
    assert_eq!(r.reason, ReasonCodeV1::TieRejected);

    let r = evaluate(
        &VotingRulesV1 {
            threshold: two_thirds,
            tie: TieTreatmentV1::Accept,
            ..rules()
        },
        &e,
        &cast,
    )
    .expect("ok");
    assert_eq!(r.reason, ReasonCodeV1::TieAccepted);
}

#[test]
fn a_weighted_tie_is_still_exact() {
    let e = electorate(vec![voter("a", 3), voter("b", 2), voter("c", 1)]);
    let result = evaluate(&rules(), &e, &ballots(&[yes("a"), no("b"), no("c")])).expect("ok");
    assert_eq!(
        result.threshold.comparison,
        ComparisonV1::Exactly,
        "3 against 3"
    );
    assert_eq!(result.reason, ReasonCodeV1::TieRejected);
}

// -------------------------------------------------------------- invalid input

#[test]
fn a_ballot_from_an_unknown_voter_is_refused() {
    let e = electorate(vec![voter("a", 1)]);
    let error = evaluate(&rules(), &e, &ballots(&[yes("a"), yes("ghost")])).expect_err("unknown");
    assert_eq!(
        error,
        EvaluationErrorV1::Ballots {
            issues: vec![BallotIssueV1::UnknownVoter { voter: id("ghost") }]
        }
    );
}

#[test]
fn a_ballot_from_an_excluded_voter_is_refused() {
    let e = electorate(vec![voter("a", 1), excluded("b", 1)]);
    let error = evaluate(&rules(), &e, &ballots(&[yes("a"), yes("b")])).expect_err("excluded");
    assert_eq!(
        error,
        EvaluationErrorV1::Ballots {
            issues: vec![BallotIssueV1::ExcludedVoter { voter: id("b") }]
        }
    );
}

#[test]
fn an_excluded_voter_may_vote_when_exclusions_are_disabled_and_none_are_marked() {
    // With exclusions disabled, the electorate must not mark anyone; with it enabled
    // and nobody marked, everyone votes. Both are consistent pairings.
    let e = electorate(vec![voter("a", 1), voter("b", 1)]);
    let r = VotingRulesV1 {
        exclusions_enabled: false,
        ..rules()
    };
    assert!(
        evaluate(&r, &e, &ballots(&[yes("a"), yes("b")]))
            .expect("ok")
            .accepted()
    );
}

#[test]
fn every_ballot_issue_is_reported_together_sorted_by_voter() {
    let e = electorate(vec![voter("a", 1), excluded("m", 1)]);
    let error =
        evaluate(&rules(), &e, &ballots(&[yes("z"), yes("m"), yes("b")])).expect_err("issues");
    let EvaluationErrorV1::Ballots { issues } = error else {
        panic!("expected ballot issues")
    };
    assert_eq!(
        issues,
        vec![
            BallotIssueV1::UnknownVoter { voter: id("b") },
            BallotIssueV1::UnknownVoter { voter: id("z") },
            BallotIssueV1::ExcludedVoter { voter: id("m") },
        ]
    );
}

#[test]
fn contradictory_rules_are_refused_before_anything_is_counted() {
    let e = electorate(vec![excluded("a", 1)]);
    let r = VotingRulesV1 {
        exclusions_enabled: false,
        ..rules()
    };
    assert!(matches!(
        evaluate(&r, &e, &ballots(&[yes("a")])),
        Err(EvaluationErrorV1::Validation(_))
    ));
}

#[test]
fn duplicate_ballots_and_voters_cannot_reach_the_evaluator() {
    use bornite_core::{BallotV1, CoreError, VoterV1};
    assert!(matches!(
        BallotSetV1::new(vec![
            BallotV1 {
                voter: id("a"),
                choice: ChoiceV1::Yes
            },
            BallotV1 {
                voter: id("a"),
                choice: ChoiceV1::No
            },
        ]),
        Err(CoreError::DuplicateBallot { .. })
    ));
    assert!(matches!(
        ElectorateV1::new(vec![
            VoterV1 {
                id: id("a"),
                weight: WeightV1::ONE,
                excluded: false
            },
            VoterV1 {
                id: id("a"),
                weight: WeightV1::ONE,
                excluded: true
            },
        ]),
        Err(CoreError::DuplicateVoter { .. })
    ));
}

// ------------------------------------------------------------------ overflow

#[test]
fn a_total_past_u64_max_is_refused_not_wrapped() {
    let e = electorate(vec![voter("a", u64::MAX), voter("b", 1)]);
    let error = evaluate(&rules(), &e, &BallotSetV1::empty()).expect_err("overflow");
    assert_eq!(error, EvaluationErrorV1::WeightOverflow);
}

#[test]
fn weights_at_u64_max_evaluate_exactly() {
    let e = electorate(vec![voter("a", u64::MAX)]);
    let result = evaluate(&rules(), &e, &ballots(&[yes("a")])).expect("ok");
    assert_eq!(result.tally.yes_weight.value(), u64::MAX);
    assert_eq!(result.threshold.denominator_weight.value(), u64::MAX);
    assert_eq!(result.threshold.comparison, ComparisonV1::Above);
    assert!(result.accepted());
}

#[test]
fn the_result_serialises_with_stable_names() {
    let e = electorate(vec![voter("a", 2), voter("b", 1)]);
    let result = evaluate(&rules(), &e, &ballots(&[yes("a"), no("b")])).expect("ok");
    let json = serde_json::to_value(&result).expect("json");
    assert_eq!(json["outcome"], "accepted");
    assert_eq!(json["reason"], "threshold_met");
    assert_eq!(json["threshold"]["comparison"], "above");
    assert_eq!(json["rules"]["threshold"]["type"], "simple-majority");
    assert_eq!(json["rules"]["threshold"]["basis"], "votes-cast");
    assert_eq!(json["quorum"]["requirement"]["type"], "none");
    assert_eq!(json["tally"]["yes_weight"], 2);
}
