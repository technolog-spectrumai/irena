//! Property tests for the core arithmetic.

use bornite_core::{
    BallotSetV1, BallotV1, ChoiceV1, ElectorateV1, FractionV1, VoterIdV1, VoterV1, WeightTotalV1,
    WeightV1,
};
use core::cmp::Ordering;
use proptest::prelude::*;

fn voter_id() -> impl Strategy<Value = VoterIdV1> {
    "[A-Za-z0-9._:+@-]{1,16}".prop_map(|text| VoterIdV1::new(text).expect("grammar"))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Cross-multiplication is total for every pair of u64 operands.
    #[test]
    fn share_comparison_never_panics(n in any::<u64>(), d in 1u64.., value in any::<u64>(), basis in any::<u64>()) {
        let fraction = FractionV1::new(n, d).expect("non-zero denominator");
        let _ = fraction.compare_share(value, basis);
    }

    /// The comparison agrees with exact rational arithmetic.
    #[test]
    fn share_comparison_is_exact(n in 0u64..1000, d in 1u64..1000, value in 0u64..100_000, basis in 0u64..100_000) {
        let fraction = FractionV1::new(n, d).expect("fraction");
        let lhs = u128::from(value) * u128::from(d);
        let rhs = u128::from(n) * u128::from(basis);
        prop_assert_eq!(fraction.compare_share(value, basis), lhs.cmp(&rhs));
    }

    /// Scaling both value and basis never changes the comparison.
    #[test]
    fn share_comparison_is_scale_invariant(n in 0u64..100, d in 1u64..100, value in 0u64..1000, basis in 0u64..1000, scale in 1u64..1000) {
        let fraction = FractionV1::new(n, d).expect("fraction");
        prop_assert_eq!(
            fraction.compare_share(value, basis),
            fraction.compare_share(value * scale, basis * scale)
        );
    }

    /// Equivalent fractions compare identically.
    #[test]
    fn equivalent_fractions_agree(n in 0u64..100, d in 1u64..100, k in 1u64..100, value in 0u64..1000, basis in 0u64..1000) {
        let plain = FractionV1::new(n, d).expect("fraction");
        let scaled = FractionV1::new(n * k, d * k).expect("fraction");
        prop_assert_eq!(plain.compare_share(value, basis), scaled.compare_share(value, basis));
    }

    /// A total either sums exactly or reports overflow; it never wraps.
    #[test]
    fn totals_are_exact_or_overflow(weights in prop::collection::vec(1u64.., 0..8)) {
        let expected: Option<u64> = weights.iter().try_fold(0u64, |acc, w| acc.checked_add(*w));
        let result = WeightTotalV1::sum(weights.iter().map(|w| WeightV1::new(*w).expect("non-zero")));
        match expected {
            Some(total) => prop_assert_eq!(result.expect("fits").value(), total),
            None => prop_assert!(result.is_err()),
        }
    }

    /// Any permutation of the same voters builds the same electorate.
    #[test]
    fn electorates_are_order_independent(ids in prop::collection::btree_set(voter_id(), 0..12), shuffle in any::<u64>()) {
        let mut voters: Vec<VoterV1> = ids
            .iter()
            .map(|id| VoterV1 { id: id.clone(), weight: WeightV1::ONE, excluded: false })
            .collect();
        let sorted = ElectorateV1::new(voters.clone()).expect("electorate");
        // A cheap deterministic shuffle: rotate by the seed.
        if !voters.is_empty() {
            let by = (shuffle % voters.len() as u64) as usize;
            voters.rotate_left(by);
        }
        voters.reverse();
        prop_assert_eq!(ElectorateV1::new(voters).expect("electorate"), sorted);
    }

    /// Any permutation of the same ballots builds the same ballot set.
    #[test]
    fn ballot_sets_are_order_independent(ids in prop::collection::btree_set(voter_id(), 0..12)) {
        let ballots: Vec<BallotV1> = ids
            .iter()
            .map(|id| BallotV1 { voter: id.clone(), choice: ChoiceV1::Yes })
            .collect();
        let sorted = BallotSetV1::new(ballots.clone()).expect("ballots");
        let mut reversed = ballots;
        reversed.reverse();
        prop_assert_eq!(BallotSetV1::new(reversed).expect("ballots"), sorted);
    }

    /// Comparing against a fraction of a basis is monotone in the value.
    #[test]
    fn share_comparison_is_monotone(n in 0u64..100, d in 1u64..100, basis in 0u64..1000, value in 0u64..999) {
        let fraction = FractionV1::new(n, d).expect("fraction");
        let lower = fraction.compare_share(value, basis);
        let higher = fraction.compare_share(value + 1, basis);
        prop_assert!(lower != Ordering::Greater || higher == Ordering::Greater);
    }
}
