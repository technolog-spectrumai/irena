//! Construction rules, exact arithmetic, and the determinism guards.

use bornite_core::{
    BallotSetV1, BallotV1, ChoiceV1, CoreError, ElectorateV1, FractionV1, VoterIdV1, VoterV1,
    WeightTotalV1, WeightV1,
};
use core::cmp::Ordering;

fn id(text: &str) -> VoterIdV1 {
    VoterIdV1::new(text).expect("valid id")
}

fn voter(text: &str, weight: u64) -> VoterV1 {
    VoterV1 {
        id: id(text),
        weight: WeightV1::new(weight).expect("weight"),
        excluded: false,
    }
}

#[test]
fn voter_ids_accept_the_documented_grammar() {
    for text in [
        "a",
        "alice",
        "drone-01",
        "share:0042",
        "user+tag@example",
        "A.B_C",
        &"x".repeat(128),
    ] {
        assert!(VoterIdV1::new(text).is_ok(), "{text} should be accepted");
    }
}

#[test]
fn voter_ids_reject_what_the_grammar_excludes() {
    for text in ["", " ", "a b", "é", "a/b", "a\n", &"x".repeat(129)] {
        assert!(
            matches!(VoterIdV1::new(text), Err(CoreError::InvalidVoterId { .. })),
            "{text:?} should be rejected"
        );
    }
}

#[test]
fn voter_ids_are_case_sensitive_and_never_folded() {
    // Two people, not one. Folding would depend on locale rules and could merge them.
    let upper = id("Alice");
    let lower = id("alice");
    assert_ne!(upper, lower);
    let electorate =
        ElectorateV1::new(vec![voter("Alice", 1), voter("alice", 1)]).expect("two voters");
    assert_eq!(electorate.len(), 2);
}

#[test]
fn weights_are_at_least_one() {
    assert_eq!(WeightV1::new(0), Err(CoreError::ZeroWeight));
    assert_eq!(WeightV1::new(1), Ok(WeightV1::ONE));
    assert_eq!(WeightV1::new(u64::MAX).expect("max").value(), u64::MAX);
}

#[test]
fn totals_are_checked_and_never_wrap() {
    let big = WeightV1::new(u64::MAX - 1).expect("weight");
    let total = WeightTotalV1::ZERO.checked_add(big).expect("fits");
    assert_eq!(
        total
            .checked_add(WeightV1::ONE)
            .expect("exactly max")
            .value(),
        u64::MAX
    );
    assert_eq!(
        WeightTotalV1::sum([big, WeightV1::ONE, WeightV1::ONE]),
        Err(CoreError::WeightOverflow)
    );
    assert_eq!(WeightTotalV1::sum([]).expect("empty"), WeightTotalV1::ZERO);
}

#[test]
fn fractions_refuse_zero_denominators_and_improper_proportions() {
    assert_eq!(FractionV1::new(1, 0), Err(CoreError::ZeroDenominator));
    assert!(
        FractionV1::new(5, 3).is_ok(),
        "a general fraction may exceed one"
    );
    assert_eq!(
        FractionV1::proportion(5, 3),
        Err(CoreError::ImproperFraction {
            numerator: 5,
            denominator: 3
        })
    );
    assert!(FractionV1::proportion(3, 3).is_ok());
    assert!(FractionV1::proportion(0, 3).is_ok());
}

#[test]
fn share_comparison_is_exact_at_the_boundary() {
    let half = FractionV1::HALF;
    assert_eq!(half.compare_share(2, 4), Ordering::Equal);
    assert_eq!(half.compare_share(3, 4), Ordering::Greater);
    assert_eq!(half.compare_share(1, 4), Ordering::Less);
    // Half of seven is not an integer; nothing is rounded, the comparison is still exact.
    assert_eq!(half.compare_share(3, 7), Ordering::Less);
    assert_eq!(half.compare_share(4, 7), Ordering::Greater);

    let two_thirds = FractionV1::proportion(2, 3).expect("2/3");
    assert_eq!(two_thirds.compare_share(2, 3), Ordering::Equal);
    assert_eq!(two_thirds.compare_share(4, 6), Ordering::Equal);
    assert_eq!(two_thirds.compare_share(6, 9), Ordering::Equal);
    assert_eq!(two_thirds.compare_share(5, 9), Ordering::Less);
}

#[test]
fn share_comparison_cannot_overflow_at_the_extremes() {
    // The largest products the comparison can ever form. If this were ever to
    // overflow, the engine would have a hidden failure mode; it does not, because
    // (2^64 - 1)^2 fits in u128 with room to spare.
    let extreme = FractionV1::new(u64::MAX, u64::MAX).expect("fraction");
    assert_eq!(extreme.compare_share(u64::MAX, u64::MAX), Ordering::Equal);
    assert_eq!(
        extreme.compare_share(u64::MAX - 1, u64::MAX),
        Ordering::Less
    );
    assert_eq!(
        extreme.compare_share(u64::MAX, u64::MAX - 1),
        Ordering::Greater
    );
    let bound = u128::from(u64::MAX) * u128::from(u64::MAX);
    assert!(bound < u128::MAX);
}

#[test]
fn a_zero_basis_compares_exactly() {
    // A share of nothing: any positive value is above it, zero is exactly it.
    let half = FractionV1::HALF;
    assert_eq!(half.compare_share(0, 0), Ordering::Equal);
    assert_eq!(half.compare_share(1, 0), Ordering::Greater);
}

#[test]
fn electorates_sort_and_refuse_duplicates() {
    let electorate = ElectorateV1::new(vec![voter("carol", 1), voter("alice", 2), voter("bob", 3)])
        .expect("electorate");
    let ids: Vec<&str> = electorate.voters().iter().map(|v| v.id.as_str()).collect();
    assert_eq!(ids, ["alice", "bob", "carol"]);
    assert_eq!(electorate.get(&id("bob")).expect("bob").weight.value(), 3);
    assert!(electorate.get(&id("dave")).is_none());

    let error = ElectorateV1::new(vec![voter("bob", 1), voter("alice", 1), voter("bob", 2)])
        .expect_err("duplicate");
    assert_eq!(error, CoreError::DuplicateVoter { id: id("bob") });
}

#[test]
fn electorate_order_never_depends_on_input_order() {
    let forward = ElectorateV1::new(vec![voter("a", 1), voter("b", 2), voter("c", 3)]).expect("e");
    let backward = ElectorateV1::new(vec![voter("c", 3), voter("b", 2), voter("a", 1)]).expect("e");
    assert_eq!(forward, backward);
}

#[test]
fn ballot_sets_sort_and_refuse_a_second_ballot_from_one_voter() {
    let ballots = BallotSetV1::new(vec![
        BallotV1 {
            voter: id("zed"),
            choice: ChoiceV1::No,
        },
        BallotV1 {
            voter: id("amy"),
            choice: ChoiceV1::Yes,
        },
    ])
    .expect("ballots");
    let voters: Vec<&str> = ballots.ballots().iter().map(|b| b.voter.as_str()).collect();
    assert_eq!(voters, ["amy", "zed"]);

    let error = BallotSetV1::new(vec![
        BallotV1 {
            voter: id("amy"),
            choice: ChoiceV1::Yes,
        },
        BallotV1 {
            voter: id("amy"),
            choice: ChoiceV1::No,
        },
    ])
    .expect_err("two ballots from amy");
    assert_eq!(error, CoreError::DuplicateBallot { voter: id("amy") });
    assert!(BallotSetV1::empty().is_empty());
}

#[test]
fn choices_parse_only_their_exact_lowercase_form() {
    assert_eq!(ChoiceV1::parse("yes"), Some(ChoiceV1::Yes));
    assert_eq!(ChoiceV1::parse("abstain"), Some(ChoiceV1::Abstain));
    assert_eq!(ChoiceV1::parse("Yes"), None);
    assert_eq!(ChoiceV1::parse("YES"), None);
    assert_eq!(ChoiceV1::parse(""), None);
    assert_eq!(ChoiceV1::Abstain.as_str(), "abstain");
}

/// No Bornite crate may use a hash map or hash set: their iteration order is not
/// specified, and a result that depends on it is not reproducible.
#[test]
fn no_bornite_source_uses_hash_collections() {
    let crates_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir(&crates_dir).expect("crates dir") {
        let path = entry.expect("entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.starts_with("bornite-") && !name.starts_with("governance-") {
            continue;
        }
        let src = path.join("src");
        if !src.exists() {
            continue;
        }
        for file in walk(&src) {
            let text = std::fs::read_to_string(&file).expect("read source");
            if text.contains("HashMap") || text.contains("HashSet") {
                offenders.push(file.display().to_string());
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "hash collections found in {offenders:?}"
    );
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            files.push(path);
        }
    }
    files
}
