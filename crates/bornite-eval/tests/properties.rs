//! Property tests: determinism under permutation, totality, and monotonicity.

mod support;

use bornite_core::{BallotSetV1, BallotV1, ChoiceV1, ElectorateV1, VoterIdV1, VoterV1, WeightV1};
use bornite_eval::{OutcomeV1, evaluate};
use bornite_rules::{
    AbstentionTreatmentV1, QuorumBasisV1, QuorumRuleV1, ThresholdBasisV1, ThresholdRuleV1,
    TieTreatmentV1, VotingRulesV1, WeightRuleV1,
};
use proptest::prelude::*;
use support::fraction;

fn choice() -> impl Strategy<Value = ChoiceV1> {
    prop_oneof![
        Just(ChoiceV1::Yes),
        Just(ChoiceV1::No),
        Just(ChoiceV1::Abstain)
    ]
}

fn threshold_basis() -> impl Strategy<Value = ThresholdBasisV1> {
    prop_oneof![
        Just(ThresholdBasisV1::VotesCast),
        Just(ThresholdBasisV1::EffectiveElectorate),
        Just(ThresholdBasisV1::TotalElectorate),
    ]
}

fn quorum_basis() -> impl Strategy<Value = QuorumBasisV1> {
    prop_oneof![
        Just(QuorumBasisV1::TotalElectorate),
        Just(QuorumBasisV1::EffectiveElectorate)
    ]
}

/// Arbitrary rules that are internally valid.
fn arbitrary_rules() -> impl Strategy<Value = VotingRulesV1> {
    (
        prop_oneof![Just(WeightRuleV1::Equal), Just(WeightRuleV1::Electorate)],
        any::<bool>(),
        prop_oneof![
            Just(QuorumRuleV1::None),
            (1u64..20).prop_map(|w| QuorumRuleV1::Absolute {
                weight: WeightV1::new(w).expect("w")
            }),
            (0u64..10, 1u64..10, quorum_basis()).prop_map(|(n, d, basis)| QuorumRuleV1::Fraction {
                fraction: fraction(n.min(d), d),
                basis,
            }),
        ],
        prop_oneof![
            threshold_basis().prop_map(|basis| ThresholdRuleV1::SimpleMajority { basis }),
            (0u64..10, 1u64..10, threshold_basis()).prop_map(|(n, d, basis)| {
                ThresholdRuleV1::Fraction {
                    fraction: fraction(n.min(d), d),
                    basis,
                }
            }),
        ],
        prop_oneof![
            Just(AbstentionTreatmentV1::Exclude),
            Just(AbstentionTreatmentV1::Include)
        ],
        prop_oneof![Just(TieTreatmentV1::Reject), Just(TieTreatmentV1::Accept)],
    )
        .prop_map(
            |(weight, exclusions_enabled, quorum, threshold, abstentions, tie)| VotingRulesV1 {
                weight,
                exclusions_enabled,
                quorum,
                threshold,
                abstentions,
                tie,
            },
        )
}

/// An electorate that is consistent with `rules`, plus ballots from some of its
/// effective voters.
fn consistent_vote(
    rules: VotingRulesV1,
) -> impl Strategy<Value = (VotingRulesV1, ElectorateV1, Vec<BallotV1>)> {
    let weight = if rules.weight == WeightRuleV1::Equal {
        1u64..2
    } else {
        1u64..8
    };
    let excluded = if rules.exclusions_enabled {
        any::<bool>().boxed()
    } else {
        Just(false).boxed()
    };
    prop::collection::btree_map(
        "[a-z]{1,4}",
        (weight, excluded, prop::option::of(choice())),
        0..10,
    )
    .prop_map(move |voters| {
        let electorate = ElectorateV1::new(
            voters
                .iter()
                .map(|(id, (w, x, _))| VoterV1 {
                    id: VoterIdV1::new(id.clone()).expect("id"),
                    weight: WeightV1::new(*w).expect("w"),
                    excluded: *x,
                })
                .collect(),
        )
        .expect("unique by map");
        let ballots = voters
            .iter()
            .filter(|(_, (_, x, c))| !(*x && rules.exclusions_enabled) && c.is_some())
            .map(|(id, (_, _, c))| BallotV1 {
                voter: VoterIdV1::new(id.clone()).expect("id"),
                choice: c.expect("some"),
            })
            .collect();
        (rules.clone(), electorate, ballots)
    })
}

fn vote() -> impl Strategy<Value = (VotingRulesV1, ElectorateV1, Vec<BallotV1>)> {
    arbitrary_rules().prop_flat_map(consistent_vote)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(384))]

    /// The same vote in any ballot order produces the identical evaluation.
    #[test]
    fn permuting_ballots_never_changes_the_result((rules, electorate, mut ballots) in vote(), seed in any::<u64>()) {
        let forward = BallotSetV1::new(ballots.clone()).expect("unique");
        if !ballots.is_empty() {
            let by = (seed % ballots.len() as u64) as usize;
            ballots.rotate_left(by);
        }
        ballots.reverse();
        let permuted = BallotSetV1::new(ballots).expect("unique");
        let left = evaluate(&rules, &electorate, &forward);
        let right = evaluate(&rules, &electorate, &permuted);
        prop_assert_eq!(left, right);
    }

    /// Evaluation is total: a consistent vote never fails, and an inconsistent one
    /// returns a typed error rather than panicking.
    #[test]
    fn evaluation_is_total((rules, electorate, ballots) in vote()) {
        let ballots = BallotSetV1::new(ballots).expect("unique");
        let outcome = evaluate(&rules, &electorate, &ballots);
        // An absolute quorum may exceed this electorate; that is a validation error, not a panic.
        if let Err(error) = &outcome {
            prop_assert!(matches!(error, bornite_eval::EvaluationErrorV1::Validation(_)), "{error}");
        }
    }

    /// The report's arithmetic is internally consistent.
    #[test]
    fn the_report_adds_up((rules, electorate, ballots) in vote()) {
        let ballots = BallotSetV1::new(ballots).expect("unique");
        let Ok(result) = evaluate(&rules, &electorate, &ballots) else { return Ok(()) };
        let t = &result.tally;
        prop_assert_eq!(t.yes_count + t.no_count + t.abstain_count, result.participation.ballot_count);
        prop_assert_eq!(
            t.yes_weight.value() + t.no_weight.value() + t.abstain_weight.value(),
            result.participation.weight.value()
        );
        prop_assert_eq!(
            result.participation.weight.value() + result.participation.non_participant_weight.value(),
            result.electorate.effective_weight.value()
        );
        prop_assert_eq!(
            result.electorate.effective_voter_count + result.electorate.excluded_count,
            result.electorate.voter_count
        );
        prop_assert!(result.electorate.effective_weight.value() <= result.electorate.total_weight.value());
        prop_assert_eq!(result.threshold.yes_weight, t.yes_weight);
        prop_assert_eq!(result.accepted(), result.outcome == OutcomeV1::Accepted);
        if result.accepted() {
            prop_assert!(result.quorum.met && result.threshold.met);
        }
    }

    /// Adding a YES ballot from a silent effective voter never turns Accepted into Rejected.
    #[test]
    fn a_new_yes_never_hurts((rules, electorate, ballots) in vote()) {
        let cast = BallotSetV1::new(ballots.clone()).expect("unique");
        let Ok(before) = evaluate(&rules, &electorate, &cast) else { return Ok(()) };
        let voted: std::collections::BTreeSet<&VoterIdV1> = ballots.iter().map(|b| &b.voter).collect();
        let Some(silent) = electorate
            .voters()
            .iter()
            .filter(|v| !voted.contains(&v.id))
            .find(|v| !(rules.exclusions_enabled && v.excluded))
        else { return Ok(()) };

        let mut more = ballots.clone();
        more.push(BallotV1 { voter: silent.id.clone(), choice: ChoiceV1::Yes });
        let after = evaluate(&rules, &electorate, &BallotSetV1::new(more).expect("unique")).expect("still consistent");
        if before.accepted() {
            prop_assert!(after.accepted(), "a YES turned acceptance into rejection\nbefore: {before:?}\nafter: {after:?}");
        }
    }
}
