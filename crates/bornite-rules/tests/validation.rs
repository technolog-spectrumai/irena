//! Contradiction detection, with every issue reported together.

use bornite_core::{ElectorateV1, FractionV1, VoterIdV1, VoterV1, WeightV1};
use bornite_rules::{
    AbstentionTreatmentV1, QuorumBasisV1, QuorumRuleV1, ThresholdBasisV1, ThresholdRuleV1,
    TieTreatmentV1, ValidationIssueV1, VotingRulesV1, WeightRuleV1, validate,
};

fn voter(id: &str, weight: u64, excluded: bool) -> VoterV1 {
    VoterV1 {
        id: VoterIdV1::new(id).expect("id"),
        weight: WeightV1::new(weight).expect("weight"),
        excluded,
    }
}

fn rules() -> VotingRulesV1 {
    VotingRulesV1 {
        weight: WeightRuleV1::Electorate,
        exclusions_enabled: true,
        quorum: QuorumRuleV1::None,
        threshold: ThresholdRuleV1::SimpleMajority {
            basis: ThresholdBasisV1::VotesCast,
        },
        abstentions: AbstentionTreatmentV1::Exclude,
        tie: TieTreatmentV1::Reject,
    }
}

#[test]
fn a_consistent_pairing_validates() {
    let electorate = ElectorateV1::new(vec![voter("a", 3, false), voter("b", 1, true)]).expect("e");
    validate(&rules(), &electorate).expect("valid");
}

#[test]
fn equal_weights_contradict_declared_weights() {
    let electorate = ElectorateV1::new(vec![
        voter("a", 3, false),
        voter("b", 1, false),
        voter("c", 2, false),
    ])
    .expect("e");
    let rules = VotingRulesV1 {
        weight: WeightRuleV1::Equal,
        ..rules()
    };
    let error = validate(&rules, &electorate).expect_err("contradiction");
    assert_eq!(
        error.issues,
        vec![
            ValidationIssueV1::ContradictoryWeight {
                voter: VoterIdV1::new("a").expect("id"),
                weight: WeightV1::new(3).expect("w")
            },
            ValidationIssueV1::ContradictoryWeight {
                voter: VoterIdV1::new("c").expect("id"),
                weight: WeightV1::new(2).expect("w")
            },
        ]
    );
}

#[test]
fn equal_weights_over_uniform_ones_are_not_a_contradiction() {
    let electorate =
        ElectorateV1::new(vec![voter("a", 1, false), voter("b", 1, false)]).expect("e");
    validate(
        &VotingRulesV1 {
            weight: WeightRuleV1::Equal,
            ..rules()
        },
        &electorate,
    )
    .expect("valid");
}

#[test]
fn disabled_exclusions_contradict_excluded_voters() {
    let electorate = ElectorateV1::new(vec![voter("a", 1, true), voter("b", 1, false)]).expect("e");
    let rules = VotingRulesV1 {
        exclusions_enabled: false,
        ..rules()
    };
    let error = validate(&rules, &electorate).expect_err("contradiction");
    assert_eq!(
        error.issues,
        vec![ValidationIssueV1::ContradictoryExclusion {
            voter: VoterIdV1::new("a").expect("id")
        }]
    );
}

#[test]
fn an_absolute_quorum_beyond_the_electorate_is_unachievable() {
    let electorate =
        ElectorateV1::new(vec![voter("a", 2, false), voter("b", 3, false)]).expect("e");
    let too_much = VotingRulesV1 {
        quorum: QuorumRuleV1::Absolute {
            weight: WeightV1::new(6).expect("w"),
        },
        ..rules()
    };
    let error = validate(&too_much, &electorate).expect_err("unachievable");
    assert!(matches!(
        error.issues[0],
        ValidationIssueV1::UnachievableQuorum { required, total }
            if required.value() == 6 && total.value() == 5
    ));

    // Exactly the whole electorate is achievable.
    let exactly = VotingRulesV1 {
        quorum: QuorumRuleV1::Absolute {
            weight: WeightV1::new(5).expect("w"),
        },
        ..rules()
    };
    validate(&exactly, &electorate).expect("achievable");
}

#[test]
fn achievability_uses_the_weight_rule_in_force() {
    // Under equal weights this electorate weighs 2, not 5.
    let electorate =
        ElectorateV1::new(vec![voter("a", 1, false), voter("b", 1, false)]).expect("e");
    let rules = VotingRulesV1 {
        weight: WeightRuleV1::Equal,
        quorum: QuorumRuleV1::Absolute {
            weight: WeightV1::new(3).expect("w"),
        },
        ..rules()
    };
    assert!(validate(&rules, &electorate).is_err());
}

#[test]
fn improper_rule_fractions_are_reported() {
    let electorate = ElectorateV1::new(vec![voter("a", 1, false)]).expect("e");
    let rules = VotingRulesV1 {
        quorum: QuorumRuleV1::Fraction {
            fraction: FractionV1::new(3, 2).expect("f"),
            basis: QuorumBasisV1::TotalElectorate,
        },
        threshold: ThresholdRuleV1::Fraction {
            fraction: FractionV1::new(5, 4).expect("f"),
            basis: ThresholdBasisV1::VotesCast,
        },
        ..rules()
    };
    let error = validate(&rules, &electorate).expect_err("improper");
    let rules_named: Vec<&str> = error
        .issues
        .iter()
        .filter_map(|issue| match issue {
            ValidationIssueV1::ImproperFraction { rule, .. } => Some(*rule),
            _ => None,
        })
        .collect();
    assert_eq!(rules_named, ["quorum", "threshold"]);
}

#[test]
fn every_issue_is_reported_together_in_a_fixed_order() {
    let electorate = ElectorateV1::new(vec![voter("z", 5, true), voter("a", 1, false)]).expect("e");
    let rules = VotingRulesV1 {
        weight: WeightRuleV1::Equal,
        exclusions_enabled: false,
        quorum: QuorumRuleV1::Absolute {
            weight: WeightV1::new(9).expect("w"),
        },
        ..rules()
    };
    let error = validate(&rules, &electorate).expect_err("many issues");
    assert_eq!(error.issues.len(), 3, "{error}");
    // Sorted by variant order, then by fields; identical whatever the input order was.
    let reversed = ElectorateV1::new(vec![voter("a", 1, false), voter("z", 5, true)]).expect("e");
    assert_eq!(validate(&rules, &reversed).expect_err("same issues"), error);
}

#[test]
fn an_overflowing_electorate_is_reported_not_wrapped() {
    let electorate =
        ElectorateV1::new(vec![voter("a", u64::MAX, false), voter("b", 1, false)]).expect("e");
    let rules = VotingRulesV1 {
        quorum: QuorumRuleV1::Absolute {
            weight: WeightV1::new(1).expect("w"),
        },
        ..rules()
    };
    let error = validate(&rules, &electorate).expect_err("overflow");
    assert_eq!(error.issues, vec![ValidationIssueV1::WeightOverflow]);
}

#[test]
fn the_abstention_rule_is_known_to_be_inert_under_electorate_denominators() {
    let votes_cast = rules();
    assert!(votes_cast.abstentions_affect_denominator());
    let electorate_based = VotingRulesV1 {
        threshold: ThresholdRuleV1::SimpleMajority {
            basis: ThresholdBasisV1::EffectiveElectorate,
        },
        ..rules()
    };
    assert!(!electorate_based.abstentions_affect_denominator());
}

#[test]
fn simple_majority_is_exactly_one_half() {
    let threshold = ThresholdRuleV1::SimpleMajority {
        basis: ThresholdBasisV1::VotesCast,
    };
    assert_eq!(threshold.fraction(), FractionV1::HALF);
    assert_eq!(threshold.basis(), ThresholdBasisV1::VotesCast);
}
