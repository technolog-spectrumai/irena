//! Determinism, uniqueness-of-representation and domain-separation tests.

use borsh::{BorshDeserialize, BorshSerialize};
use prunella_canonical::{
    Canonical, CanonicalError, DomainHasher, decode, domain, encode, hash_canonical, hash_domain,
};

#[derive(BorshSerialize, BorshDeserialize, Debug, PartialEq, Eq, Clone)]
struct Sample {
    a: u64,
    b: Vec<u8>,
    c: String,
}

impl Canonical for Sample {}

fn sample() -> Sample {
    Sample {
        a: 1,
        b: vec![0xde, 0xad],
        c: "hi".to_owned(),
    }
}

#[test]
fn encoding_matches_hand_derived_layout() {
    // u64 little-endian, then u32 little-endian length + bytes, then the same for the
    // UTF-8 string. Derived by hand from the documented canonical subset, not captured
    // from an implementation run.
    let expected = concat!(
        "0100000000000000", // a = 1u64
        "02000000",         // b.len() = 2u32
        "dead",             // b
        "02000000",         // c.len() = 2u32
        "6869",             // c = "hi"
    );
    assert_eq!(hex_of(&sample().canonical_bytes()), expected);
}

#[test]
fn encoding_is_stable_across_repeated_calls() {
    let value = sample();
    let first = value.canonical_bytes();
    for _ in 0..64 {
        assert_eq!(value.canonical_bytes(), first);
    }
}

#[test]
fn distinct_values_encode_distinctly() {
    let mut other = sample();
    other.a = 2;
    assert_ne!(sample().canonical_bytes(), other.canonical_bytes());
}

#[test]
fn field_contents_cannot_be_shifted_between_fields() {
    // Length prefixes make `b`/`c` boundaries unambiguous: moving a byte across the
    // boundary must change the encoding rather than produce the same bytes.
    let left = Sample {
        a: 0,
        b: vec![0x61, 0x62],
        c: String::new(),
    };
    let right = Sample {
        a: 0,
        b: vec![0x61],
        c: "b".to_owned(),
    };
    assert_ne!(left.canonical_bytes(), right.canonical_bytes());
}

#[test]
fn decode_round_trips() {
    let value = sample();
    let decoded: Sample = decode(&value.canonical_bytes()).expect("round trip");
    assert_eq!(decoded, value);
}

#[test]
fn decode_rejects_trailing_bytes() {
    let mut bytes = sample().canonical_bytes();
    bytes.push(0x00);
    let error = decode::<Sample>(&bytes).expect_err("trailing byte must be rejected");
    assert_eq!(error, CanonicalError::TrailingBytes { trailing: 1 });
}

#[test]
fn decode_rejects_truncated_input() {
    let bytes = sample().canonical_bytes();
    let error = decode::<Sample>(&bytes[..bytes.len() - 1]).expect_err("truncation");
    assert!(matches!(error, CanonicalError::Decode(_)), "got {error:?}");
}

#[test]
fn decode_rejects_a_length_prefix_longer_than_the_input() {
    // A hostile encoder claiming a 4 GiB byte string must fail, not allocate.
    let mut bytes = vec![0u8; 8];
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    let error = decode::<Sample>(&bytes).expect_err("oversized length prefix");
    assert!(matches!(error, CanonicalError::Decode(_)), "got {error:?}");
}

#[test]
fn domains_separate_identical_pre_images() {
    let payload = b"same bytes, different purpose";
    let tags = [
        domain::TX_SIGN,
        domain::TX_ID,
        domain::TX_ROOT,
        domain::BLOCK_HEADER,
    ];
    for (i, left) in tags.iter().enumerate() {
        for right in &tags[i + 1..] {
            assert_ne!(
                hash_domain(left, payload),
                hash_domain(right, payload),
                "tags {left} and {right} collided"
            );
        }
    }
}

#[test]
fn tag_length_prefix_prevents_boundary_ambiguity() {
    // Without the length prefix, tag "ab" over payload "c" and tag "a" over payload
    // "bc" would share a pre-image.
    assert_ne!(hash_domain("ab", b"c"), hash_domain("a", b"bc"));
}

#[test]
fn incremental_hashing_matches_one_shot_hashing() {
    let mut hasher = DomainHasher::new(domain::TX_ROOT);
    hasher.update(b"one").update(b"two").update(b"three");
    assert_eq!(
        hasher.finalize(),
        hash_domain(domain::TX_ROOT, b"onetwothree")
    );
}

#[test]
fn hash_canonical_hashes_the_canonical_encoding() {
    let value = sample();
    assert_eq!(
        hash_canonical(domain::TX_ID, &value),
        hash_domain(domain::TX_ID, &value.canonical_bytes())
    );
}

#[test]
fn hash_is_not_derived_from_debug_output() {
    // Guards the rule that human renderings are never hash pre-images: if someone ever
    // swapped `canonical_bytes` for a `Debug` string, this would start passing.
    let value = sample();
    let debug_hash = hash_domain(domain::TX_ID, format!("{value:?}").as_bytes());
    assert_ne!(hash_canonical(domain::TX_ID, &value), debug_hash);
}

#[test]
fn encode_helper_agrees_with_trait_method() {
    let value = sample();
    assert_eq!(encode(&value).expect("encode"), value.canonical_bytes());
}

/// Locked regression vectors.
///
/// These digests were produced once by this implementation and are pinned here on
/// purpose. Their job is not to prove BLAKE3 correct but to fail loudly if the
/// canonical encoding, the domain tags or the hash construction ever change, because
/// any such change silently rewrites every hash in every existing chain.
#[test]
fn locked_digest_vectors() {
    assert_eq!(
        hex_of(&hash_domain(domain::TX_ID, b"")),
        "d38ecf7be26d4543036032cf25b446cb324681be61f7b08b4b79c43890898ee2"
    );
    assert_eq!(
        hex_of(&hash_canonical(domain::BLOCK_HEADER, &sample())),
        "f4ccf38b0b1b4c897e028e3e2756366b01eda44b07ab8339e868e454458d6ecb"
    );
}

fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
