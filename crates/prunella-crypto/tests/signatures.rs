//! Signing, verification and tamper-rejection tests.

use prunella_core::{Namespace, PublicKey, SchemaVersion, Signature, TransactionDraft};
use prunella_crypto::{CryptoError, SigningKey, verify, verify_transaction};

fn key(seed: u8) -> SigningKey {
    SigningKey::from_seed([seed; 32])
}

fn draft(payload: &[u8]) -> TransactionDraft {
    TransactionDraft {
        namespace: Namespace::new("app.demo").expect("valid namespace"),
        schema_version: SchemaVersion(1),
        payload: payload.to_vec(),
        signer: PublicKey::from_bytes([0u8; 32]),
        nonce: 1,
    }
}

#[test]
fn a_signed_transaction_verifies() {
    let signing_key = key(1);
    let transaction = signing_key.sign_transaction(draft(b"payload"));
    assert_eq!(transaction.signer, signing_key.public_key());
    assert!(transaction.has_consistent_id());
    verify_transaction(&transaction).expect("signature should verify");
}

#[test]
fn signing_overwrites_a_claimed_signer() {
    // A draft that claims someone else's key must not be able to produce a transaction
    // attributed to them.
    let signing_key = key(1);
    let mut claimed = draft(b"payload");
    claimed.signer = key(2).public_key();
    let transaction = signing_key.sign_transaction(claimed);
    assert_eq!(transaction.signer, signing_key.public_key());
    verify_transaction(&transaction).expect("signature should verify");
}

#[test]
fn signing_is_deterministic_for_the_same_key_and_message() {
    let signing_key = key(1);
    let first = signing_key.sign_transaction(draft(b"payload"));
    let second = signing_key.sign_transaction(draft(b"payload"));
    assert_eq!(first.signature, second.signature);
    assert_eq!(first.id, second.id);
}

#[test]
fn the_same_seed_always_produces_the_same_key() {
    assert_eq!(key(5).public_key(), key(5).public_key());
    assert_ne!(key(5).public_key(), key(6).public_key());
    assert_eq!(key(5).to_seed(), [5u8; 32]);
}

#[test]
fn a_generated_key_is_usable_and_not_constant() {
    let first = SigningKey::generate().expect("random source");
    let second = SigningKey::generate().expect("random source");
    assert_ne!(first.public_key(), second.public_key());
    let transaction = first.sign_transaction(draft(b"payload"));
    verify_transaction(&transaction).expect("signature should verify");
}

#[test]
fn a_wrong_signer_is_rejected() {
    let mut transaction = key(1).sign_transaction(draft(b"payload"));
    transaction.signer = key(2).public_key();
    assert!(matches!(
        verify_transaction(&transaction),
        Err(CryptoError::SignatureMismatch { .. })
    ));
}

#[test]
fn a_tampered_payload_is_rejected() {
    let mut transaction = key(1).sign_transaction(draft(b"payload"));
    transaction.payload = b"payloae".to_vec();
    assert!(matches!(
        verify_transaction(&transaction),
        Err(CryptoError::SignatureMismatch { .. })
    ));
}

#[test]
fn a_tampered_namespace_nonce_or_schema_version_is_rejected() {
    let original = key(1).sign_transaction(draft(b"payload"));

    let mut namespace = original.clone();
    namespace.namespace = Namespace::new("app.other").expect("valid namespace");
    let mut nonce = original.clone();
    nonce.nonce = 99;
    let mut schema = original.clone();
    schema.schema_version = SchemaVersion(2);

    for (label, transaction) in [
        ("namespace", namespace),
        ("nonce", nonce),
        ("schema_version", schema),
    ] {
        assert!(
            verify_transaction(&transaction).is_err(),
            "tampering with {label} was not detected"
        );
    }
}

#[test]
fn a_tampered_signature_is_rejected() {
    let mut transaction = key(1).sign_transaction(draft(b"payload"));
    let mut bytes = transaction.signature.to_bytes();
    bytes[0] ^= 0x01;
    transaction.signature = Signature::from_bytes(bytes);
    assert!(verify_transaction(&transaction).is_err());
}

#[test]
fn an_all_zero_signature_is_rejected() {
    let mut transaction = key(1).sign_transaction(draft(b"payload"));
    transaction.signature = Signature::from_bytes([0u8; 64]);
    assert!(verify_transaction(&transaction).is_err());
}

#[test]
fn a_non_canonical_scalar_in_the_signature_is_rejected() {
    // The upper half of an ed25519 signature is a scalar that must be reduced. An
    // unreduced scalar is a second encoding of the same signature, and strict
    // verification refuses it: the transaction id commits to the signature bytes, so
    // two accepted encodings would mean two ids for one signed message.
    let mut transaction = key(1).sign_transaction(draft(b"payload"));
    let mut bytes = transaction.signature.to_bytes();
    bytes[63] |= 0xe0;
    transaction.signature = Signature::from_bytes(bytes);
    assert!(verify_transaction(&transaction).is_err());
}

#[test]
fn a_malformed_public_key_is_reported_as_malformed() {
    // A compressed Edwards point whose y-coordinate has no corresponding x on the
    // curve. It is reported separately from a wrong-but-well-formed key so operators
    // can tell corrupt key material apart from a genuine signature mismatch.
    let mut bytes = [0u8; 32];
    bytes[0] = 0xff;
    bytes[31] = 0x01;
    let key = PublicKey::from_bytes(bytes);
    let result = verify(&key, b"message", &Signature::from_bytes([0u8; 64]));
    assert!(
        matches!(result, Err(CryptoError::MalformedPublicKey { .. })),
        "got {result:?}"
    );
}

#[test]
fn a_well_formed_but_unrelated_key_is_reported_as_a_mismatch() {
    let transaction = key(1).sign_transaction(draft(b"payload"));
    let result = verify(
        &key(2).public_key(),
        &transaction.signing_message(),
        &transaction.signature,
    );
    assert!(
        matches!(result, Err(CryptoError::SignatureMismatch { .. })),
        "got {result:?}"
    );
}

#[test]
fn a_small_order_public_key_is_rejected() {
    // The identity point: signatures under it verify for any message under permissive
    // verification, which is exactly what strict verification exists to prevent.
    let mut identity = [0u8; 32];
    identity[0] = 1;
    let key = PublicKey::from_bytes(identity);
    let mut signature = [0u8; 64];
    signature[0] = 1;
    assert!(verify(&key, b"message", &Signature::from_bytes(signature)).is_err());
}

#[test]
fn a_signature_over_a_different_message_does_not_verify() {
    let signing_key = key(1);
    let signature = signing_key.sign(b"message one");
    assert!(verify(&signing_key.public_key(), b"message one", &signature).is_ok());
    assert!(verify(&signing_key.public_key(), b"message two", &signature).is_err());
}

#[test]
fn the_debug_rendering_does_not_expose_the_seed() {
    let signing_key = key(0xab);
    let rendered = format!("{signing_key:?}");
    assert!(
        !rendered.contains(&"ab".repeat(32)),
        "seed leaked: {rendered}"
    );
    assert!(rendered.contains(&signing_key.public_key().to_hex()));
}

#[test]
fn an_empty_payload_signs_and_verifies() {
    let transaction = key(1).sign_transaction(draft(b""));
    verify_transaction(&transaction).expect("empty payloads are ordinary payloads");
}

#[test]
fn a_non_utf8_payload_signs_and_verifies() {
    let transaction = key(1).sign_transaction(draft(&[0xff, 0xfe, 0x00, 0x80]));
    verify_transaction(&transaction).expect("payloads are opaque bytes");
}
