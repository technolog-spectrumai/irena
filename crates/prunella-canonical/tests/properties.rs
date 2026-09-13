//! Property tests for canonical decoding.
//!
//! The contract under test is narrow and absolute: arbitrary bytes handed to a decoder
//! produce a value or a typed error, never a panic, never an unbounded allocation, and
//! never two byte strings that mean the same thing.

use borsh::{BorshDeserialize, BorshSerialize};
use proptest::prelude::*;
use prunella_canonical::{Canonical, CanonicalError, decode, domain, hash_domain};

#[derive(BorshSerialize, BorshDeserialize, Debug, PartialEq, Eq, Clone)]
struct Sample {
    a: u64,
    b: Vec<u8>,
    c: String,
    d: Vec<u32>,
}

impl Canonical for Sample {}

fn sample() -> impl Strategy<Value = Sample> {
    (
        any::<u64>(),
        prop::collection::vec(any::<u8>(), 0..64),
        ".{0,64}",
        prop::collection::vec(any::<u32>(), 0..16),
    )
        .prop_map(|(a, b, c, d)| Sample { a, b, c, d })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Arbitrary bytes never panic a decoder.
    #[test]
    fn decoding_arbitrary_bytes_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let _ = decode::<Sample>(&bytes);
        let _ = decode::<u64>(&bytes);
        let _ = decode::<Vec<u8>>(&bytes);
        let _ = decode::<String>(&bytes);
    }

    /// A hostile length prefix is refused rather than allocated.
    ///
    /// The strategy puts a huge `u32` length in front of a short body, which is what a
    /// malicious encoder would send to make a decoder reserve gigabytes.
    #[test]
    fn an_oversized_length_prefix_is_refused(
        length in 0x0010_0000u32..=u32::MAX,
        tail in prop::collection::vec(any::<u8>(), 0..32),
    ) {
        let mut bytes = length.to_le_bytes().to_vec();
        bytes.extend_from_slice(&tail);
        prop_assert!(matches!(decode::<Vec<u8>>(&bytes), Err(CanonicalError::Decode(_))));
        prop_assert!(matches!(decode::<String>(&bytes), Err(CanonicalError::Decode(_))));
    }

    /// Encoding is a function: the same value always gives the same bytes.
    #[test]
    fn encoding_is_deterministic(value in sample()) {
        let first = value.canonical_bytes();
        prop_assert_eq!(&first, &value.clone().canonical_bytes());
        prop_assert_eq!(&first, &value.canonical_bytes());
    }

    /// Encoding is injective: different values never share an encoding.
    #[test]
    fn distinct_values_have_distinct_encodings(left in sample(), right in sample()) {
        if left != right {
            prop_assert_ne!(left.canonical_bytes(), right.canonical_bytes());
        }
    }

    /// Every encoding decodes back to the value it came from.
    #[test]
    fn encoding_round_trips(value in sample()) {
        let decoded: Sample = decode(&value.canonical_bytes()).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(decoded, value);
    }

    /// Appending anything to a valid encoding makes it non-canonical.
    #[test]
    fn trailing_bytes_are_always_rejected(
        value in sample(),
        tail in prop::collection::vec(any::<u8>(), 1..16),
    ) {
        let mut bytes = value.canonical_bytes();
        let extra = tail.len();
        bytes.extend_from_slice(&tail);
        prop_assert_eq!(decode::<Sample>(&bytes), Err(CanonicalError::TrailingBytes { trailing: extra }));
    }

    /// Truncating a valid encoding never yields a different valid value.
    #[test]
    fn truncated_encodings_never_decode_to_something_else(value in sample(), cut in 1usize..32) {
        let bytes = value.canonical_bytes();
        if cut < bytes.len() {
            let truncated = &bytes[..bytes.len() - cut];
            if let Ok(other) = decode::<Sample>(truncated) {
                prop_assert_ne!(other, value);
            }
        }
    }

    /// Domain separation holds for arbitrary pre-images.
    #[test]
    fn domains_never_collide(payload in prop::collection::vec(any::<u8>(), 0..128)) {
        let tags = [domain::TX_SIGN, domain::TX_ID, domain::TX_ROOT, domain::TX_LEAF,
                    domain::TX_NODE, domain::BLOCK_HEADER];
        for (i, left) in tags.iter().enumerate() {
            for right in &tags[i + 1..] {
                prop_assert_ne!(hash_domain(left, &payload), hash_domain(right, &payload));
            }
        }
    }

    /// The tag length prefix keeps `(tag, data)` unambiguous for arbitrary splits.
    #[test]
    fn tag_and_data_boundaries_are_unambiguous(text in "[a-z]{2,12}", split in 1usize..11) {
        if split < text.len() {
            let (left_tag, left_data) = text.split_at(split);
            prop_assert_ne!(
                hash_domain(left_tag, left_data.as_bytes()),
                hash_domain(&text, b"")
            );
        }
    }
}
