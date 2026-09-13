//! Hash derivation, determinism and construction tests for the core ledger types.

use prunella_canonical::Canonical;
use prunella_core::{
    Block, BlockDraft, BlockHeight, CoreError, GenesisSpec, HEADER_VERSION, Hash, Namespace,
    NetworkId, PublicKey, SchemaVersion, Signature, Transaction, TransactionDraft, TxId,
};
use std::str::FromStr;

fn network() -> NetworkId {
    NetworkId::new("testnet").expect("valid network id")
}

fn signer(seed: u8) -> PublicKey {
    PublicKey::from_bytes([seed; 32])
}

fn signature(seed: u8) -> Signature {
    Signature::from_bytes([seed; 64])
}

fn draft(payload: &[u8], nonce: u64) -> TransactionDraft {
    TransactionDraft {
        namespace: Namespace::new("app.demo").expect("valid namespace"),
        schema_version: SchemaVersion(1),
        payload: payload.to_vec(),
        signer: signer(7),
        nonce,
    }
}

fn transaction(payload: &[u8], nonce: u64) -> Transaction {
    draft(payload, nonce).into_transaction(signature(9))
}

#[test]
fn transaction_id_is_derived_and_self_consistent() {
    let transaction = transaction(b"payload", 1);
    assert_eq!(transaction.id, transaction.compute_id());
    assert!(transaction.has_consistent_id());
}

#[test]
fn every_transaction_field_changes_the_id() {
    let base = transaction(b"payload", 1);

    let mut namespace_changed = base.clone();
    namespace_changed.namespace = Namespace::new("app.other").expect("valid namespace");

    let mut schema_changed = base.clone();
    schema_changed.schema_version = SchemaVersion(2);

    let mut payload_changed = base.clone();
    payload_changed.payload = b"payloae".to_vec();

    let mut signer_changed = base.clone();
    signer_changed.signer = signer(8);

    let mut nonce_changed = base.clone();
    nonce_changed.nonce = 2;

    let mut signature_changed = base.clone();
    signature_changed.signature = signature(10);

    for (label, variant) in [
        ("namespace", namespace_changed),
        ("schema_version", schema_changed),
        ("payload", payload_changed),
        ("signer", signer_changed),
        ("nonce", nonce_changed),
        ("signature", signature_changed),
    ] {
        assert_ne!(
            base.compute_id(),
            variant.compute_id(),
            "{label} did not affect the id"
        );
    }
}

#[test]
fn the_id_commits_to_the_signature_so_swapping_it_is_detectable() {
    let mut tampered = transaction(b"payload", 1);
    tampered.signature = signature(11);
    assert!(!tampered.has_consistent_id());
}

#[test]
fn the_signing_message_excludes_the_signature() {
    let base = transaction(b"payload", 1);
    let mut resigned = base.clone();
    resigned.signature = signature(12);
    assert_eq!(base.signing_message(), resigned.signing_message());
}

#[test]
fn the_signing_message_covers_every_signed_field() {
    let base = draft(b"payload", 1);
    let mut payload_changed = base.clone();
    payload_changed.payload = b"other".to_vec();
    let mut nonce_changed = base.clone();
    nonce_changed.nonce = 2;
    let mut signer_changed = base.clone();
    signer_changed.signer = signer(8);

    for (label, variant) in [
        ("payload", payload_changed),
        ("nonce", nonce_changed),
        ("signer", signer_changed),
    ] {
        assert_ne!(
            base.signing_message(),
            variant.signing_message(),
            "{label} did not affect the signing message"
        );
    }
}

#[test]
fn the_draft_signing_message_matches_the_built_transaction() {
    let draft = draft(b"payload", 1);
    let expected = draft.signing_message();
    let transaction = draft.into_transaction(signature(9));
    assert_eq!(transaction.signing_message(), expected);
}

#[test]
fn the_transaction_root_depends_on_order() {
    let first = transaction(b"a", 1);
    let second = transaction(b"b", 2);
    let forward = Transaction::compute_root(&[first.clone(), second.clone()]).expect("root");
    let reversed = Transaction::compute_root(&[second, first]).expect("root");
    assert_ne!(forward, reversed);
}

#[test]
fn the_transaction_root_commits_to_the_count() {
    let empty = Transaction::compute_root(&[]).expect("root");
    let one = Transaction::compute_root(&[transaction(b"a", 1)]).expect("root");
    assert_ne!(empty, one);
    // A single-transaction root is not simply that transaction's id: the count prefix
    // and the domain tag keep the two pre-images apart.
    assert_ne!(one.to_bytes(), transaction(b"a", 1).id.to_bytes());
}

#[test]
fn building_a_block_derives_the_count_and_root() {
    let transactions = vec![transaction(b"a", 1), transaction(b"b", 2)];
    let expected_root = Transaction::compute_root(&transactions).expect("root");
    let block = BlockDraft {
        network_id: network(),
        height: BlockHeight(3),
        previous_hash: Hash::from_bytes([1u8; 32]),
        timestamp_millis: 42,
        transactions,
    }
    .build()
    .expect("build");

    assert_eq!(block.header.version, HEADER_VERSION);
    assert_eq!(block.header.tx_count, 2);
    assert_eq!(block.header.tx_root, expected_root);
    assert_eq!(block.height(), BlockHeight(3));
}

#[test]
fn every_header_field_changes_the_block_hash() {
    let block = BlockDraft {
        network_id: network(),
        height: BlockHeight(3),
        previous_hash: Hash::from_bytes([1u8; 32]),
        timestamp_millis: 42,
        transactions: vec![transaction(b"a", 1)],
    }
    .build()
    .expect("build");
    let base = block.hash();

    let mut version = block.header.clone();
    version.version = 2;
    let mut network_id = block.header.clone();
    network_id.network_id = NetworkId::new("othernet").expect("valid");
    let mut height = block.header.clone();
    height.height = BlockHeight(4);
    let mut previous = block.header.clone();
    previous.previous_hash = Hash::from_bytes([2u8; 32]);
    let mut root = block.header.clone();
    root.tx_root = Hash::from_bytes([3u8; 32]);
    let mut count = block.header.clone();
    count.tx_count = 2;
    let mut timestamp = block.header.clone();
    timestamp.timestamp_millis = 43;

    for (label, header) in [
        ("version", version),
        ("network_id", network_id),
        ("height", height),
        ("previous_hash", previous),
        ("tx_root", root),
        ("tx_count", count),
        ("timestamp_millis", timestamp),
    ] {
        assert_ne!(
            base,
            header.block_hash(),
            "{label} did not affect the block hash"
        );
    }
}

#[test]
fn a_child_draft_links_to_its_parent_hash() {
    let parent = GenesisSpec::new(network()).build().expect("genesis");
    let child = parent
        .header
        .child_draft(vec![transaction(b"a", 1)], 100)
        .expect("child draft")
        .build()
        .expect("build");

    assert_eq!(child.header.previous_hash, parent.hash());
    assert_eq!(child.header.height, BlockHeight(1));
    assert_eq!(child.header.network_id, parent.header.network_id);
}

#[test]
fn genesis_is_at_height_zero_with_a_zero_previous_hash() {
    let genesis = GenesisSpec::new(network()).build().expect("genesis");
    assert_eq!(genesis.header.height, BlockHeight::GENESIS);
    assert!(genesis.header.previous_hash.is_zero());
    assert!(genesis.header.height.is_genesis());
    assert_eq!(genesis.header.tx_count, 0);
}

#[test]
fn the_same_genesis_spec_always_derives_the_same_hash() {
    // The core invariant, at its root: two instances that never communicate derive the
    // same genesis from the same specification.
    let left = GenesisSpec::new(network()).build().expect("genesis");
    let right = GenesisSpec::new(network()).build().expect("genesis");
    assert_eq!(left.hash(), right.hash());
    assert_eq!(left.canonical_bytes(), right.canonical_bytes());
}

#[test]
fn different_networks_derive_different_genesis_hashes() {
    let left = GenesisSpec::new(network()).build().expect("genesis");
    let right = GenesisSpec::new(NetworkId::new("othernet").expect("valid"))
        .build()
        .expect("genesis");
    assert_ne!(left.hash(), right.hash());
}

#[test]
fn blocks_round_trip_through_canonical_bytes() {
    let block = BlockDraft {
        network_id: network(),
        height: BlockHeight(1),
        previous_hash: Hash::from_bytes([9u8; 32]),
        timestamp_millis: 7,
        transactions: vec![transaction(b"", 0), transaction(&[0xff, 0x00, 0xfe], 1)],
    }
    .build()
    .expect("build");

    let bytes = block.canonical_bytes();
    let decoded = Block::from_canonical_bytes(&bytes).expect("decode");
    assert_eq!(decoded, block);
    assert_eq!(decoded.hash(), block.hash());
    assert_eq!(decoded.transactions[1].payload, vec![0xff, 0x00, 0xfe]);
}

#[test]
fn hex_text_round_trips_and_rejects_non_canonical_forms() {
    let hash = Hash::from_bytes([0xab; 32]);
    assert_eq!(hash.to_string(), "ab".repeat(32));
    assert_eq!(Hash::from_str(&hash.to_string()).expect("parse"), hash);

    // Uppercase is a different textual form of the same bytes, so it is rejected to
    // keep exactly one rendering per value.
    assert!(matches!(
        Hash::from_hex(&"AB".repeat(32)),
        Err(CoreError::HexDigits { .. })
    ));
    assert!(matches!(
        Hash::from_hex("abcd"),
        Err(CoreError::HexLength { .. })
    ));
    assert!(matches!(
        Hash::from_hex(&"zz".repeat(32)),
        Err(CoreError::HexDigits { .. })
    ));
}

#[test]
fn transaction_ids_and_hashes_are_separate_types_with_the_same_text_form() {
    let id = TxId::from_hash(Hash::from_bytes([0x01; 32]));
    assert_eq!(id.to_string(), "01".repeat(32));
    assert_eq!(TxId::from_str(&id.to_string()).expect("parse"), id);
    assert_eq!(id.hash(), Hash::from_bytes([0x01; 32]));
}

#[test]
fn keys_and_signatures_round_trip_through_hex() {
    let key = signer(0x0a);
    assert_eq!(PublicKey::from_hex(&key.to_hex()).expect("parse"), key);
    let sig = signature(0x0b);
    assert_eq!(Signature::from_hex(&sig.to_hex()).expect("parse"), sig);
    assert_eq!(sig.to_hex().len(), 128);
}

#[test]
fn heights_increment_and_refuse_to_overflow() {
    assert_eq!(BlockHeight(0).next().expect("next"), BlockHeight(1));
    assert!(matches!(
        BlockHeight(u64::MAX).next(),
        Err(CoreError::HeightOverflow { .. })
    ));
}

/// Locked regression vector for the genesis derivation.
///
/// Pinned deliberately: if the canonical encoding, the field order, the domain tags or
/// the header layout ever change, this fails instead of silently invalidating every
/// chain that was ever created with this network id.
#[test]
fn locked_genesis_hash_vector() {
    let genesis = GenesisSpec::new(NetworkId::new("prunella.example").expect("valid"))
        .build()
        .expect("genesis");
    assert_eq!(
        genesis.hash().to_string(),
        "4cfcf0687ebd6e97e1ae8aab69ed46793088cfb705c63c91475e1fd75fed507c"
    );
}
