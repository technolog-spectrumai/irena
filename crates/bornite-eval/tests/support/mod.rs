//! Builders shared by the evaluation tests.
#![allow(dead_code)]

use bornite_core::{
    BallotSetV1, BallotV1, ChoiceV1, ElectorateV1, FractionV1, VoterIdV1, VoterV1, WeightV1,
};
use bornite_rules::{
    AbstentionTreatmentV1, QuorumBasisV1, QuorumRuleV1, ThresholdBasisV1, ThresholdRuleV1,
    TieTreatmentV1, VotingRulesV1, WeightRuleV1,
};

pub fn id(text: &str) -> VoterIdV1 {
    VoterIdV1::new(text).expect("id")
}

pub fn voter(text: &str, weight: u64) -> VoterV1 {
    VoterV1 {
        id: id(text),
        weight: WeightV1::new(weight).expect("weight"),
        excluded: false,
    }
}

pub fn excluded(text: &str, weight: u64) -> VoterV1 {
    VoterV1 {
        excluded: true,
        ..voter(text, weight)
    }
}

pub fn electorate(voters: Vec<VoterV1>) -> ElectorateV1 {
    ElectorateV1::new(voters).expect("electorate")
}

pub fn ballots(entries: &[(&str, ChoiceV1)]) -> BallotSetV1 {
    BallotSetV1::new(
        entries
            .iter()
            .map(|(v, c)| BallotV1 {
                voter: id(v),
                choice: *c,
            })
            .collect(),
    )
    .expect("ballots")
}

pub fn yes(v: &str) -> (&str, ChoiceV1) {
    (v, ChoiceV1::Yes)
}

pub fn no(v: &str) -> (&str, ChoiceV1) {
    (v, ChoiceV1::No)
}

pub fn abstain(v: &str) -> (&str, ChoiceV1) {
    (v, ChoiceV1::Abstain)
}

/// Weighted, exclusions on, no quorum, simple majority of votes cast, abstentions
/// excluded, ties rejected. Each test overrides what it exercises.
pub fn rules() -> VotingRulesV1 {
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

pub fn fraction(n: u64, d: u64) -> FractionV1 {
    FractionV1::proportion(n, d).expect("proportion")
}

pub fn threshold(n: u64, d: u64, basis: ThresholdBasisV1) -> ThresholdRuleV1 {
    ThresholdRuleV1::Fraction {
        fraction: fraction(n, d),
        basis,
    }
}

pub fn quorum_fraction(n: u64, d: u64, basis: QuorumBasisV1) -> QuorumRuleV1 {
    QuorumRuleV1::Fraction {
        fraction: fraction(n, d),
        basis,
    }
}

pub fn quorum_absolute(weight: u64) -> QuorumRuleV1 {
    QuorumRuleV1::Absolute {
        weight: WeightV1::new(weight).expect("weight"),
    }
}
