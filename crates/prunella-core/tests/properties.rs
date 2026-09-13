//! Property tests for the core derivations and the Merkle tree.

use proptest::prelude::*;
use prunella_canonical::Canonical;
use prunella_core::{
    Hash, InclusionProof, Namespace, ProofStep, PublicKey, SchemaVersion, Side, Signature,
    Transaction, TransactionDraft, TxId, merkle,
};

fn tx_id() -> impl Strategy<Value = TxId> {
    any::<[u8; 32]>().prop_map(|bytes| TxId::from_hash(Hash::from_bytes(bytes)))
}

fn ids(max: usize) -> impl Strategy<Value = Vec<TxId>> {
    prop::collection::vec(tx_id(), 1..max)
}

fn transaction() -> impl Strategy<Value = Transaction> {
    (
        prop::sample::select(vec![
            "a".to_owned(),
            "app.demo".to_owned(),
            "z9._-".to_owned(),
            "m".repeat(64),
        ]),
        any::<u32>(),
        prop::collection::vec(any::<u8>(), 0..96),
        any::<[u8; 32]>(),
        any::<u64>(),
        any::<[u8; 64]>(),
    )
        .prop_map(
            |(namespace, schema_version, payload, signer, nonce, signature)| {
                TransactionDraft {
                    namespace: Namespace::new(namespace).expect("valid label"),
                    schema_version: SchemaVersion(schema_version),
                    payload,
                    signer: PublicKey::from_bytes(signer),
                    nonce,
                }
                .into_transaction(Signature::from_bytes(signature))
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// A built transaction always carries the id its contents derive.
    #[test]
    fn a_built_transaction_is_self_consistent(transaction in transaction()) {
        prop_assert!(transaction.has_consistent_id());
        prop_assert_eq!(transaction.id, transaction.compute_id());
    }

    /// Changing any signed field changes the signing message.
    #[test]
    fn the_signing_message_covers_every_signed_field(
        transaction in transaction(),
        other_payload in prop::collection::vec(any::<u8>(), 0..96),
        other_nonce in any::<u64>(),
    ) {
        let mut changed = transaction.clone();
        changed.payload = other_payload;
        changed.nonce = other_nonce;
        if changed.payload != transaction.payload || changed.nonce != transaction.nonce {
            prop_assert_ne!(changed.signing_message(), transaction.signing_message());
        }
    }

    /// Transactions round trip through canonical bytes with their payloads intact.
    #[test]
    fn transactions_round_trip(transaction in transaction()) {
        let decoded = Transaction::from_canonical_bytes(&transaction.canonical_bytes())
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(decoded, transaction);
    }

    /// Every leaf in every tree proves and verifies against the root.
    #[test]
    fn every_proof_verifies(ids in ids(40)) {
        let root = merkle::merkle_root(&ids);
        let count = u32::try_from(ids.len()).expect("small");
        for (index, id) in ids.iter().enumerate() {
            let proof = InclusionProof::generate(&ids, index).expect("in range");
            prop_assert!(proof.verify(id, count, &root).is_ok());
        }
    }

    /// A proof never verifies for a transaction that is not at its index.
    #[test]
    fn a_proof_is_specific_to_its_leaf(ids in ids(24), pick in any::<prop::sample::Index>()) {
        prop_assume!(ids.len() >= 2);
        let root = merkle::merkle_root(&ids);
        let count = u32::try_from(ids.len()).expect("small");
        let index = pick.index(ids.len());
        let proof = InclusionProof::generate(&ids, index).expect("in range");
        for (other, id) in ids.iter().enumerate() {
            if other != index && id != &ids[index] {
                prop_assert!(proof.verify(id, count, &root).is_err());
            }
        }
    }

    /// Altering any step of a proof is detected.
    #[test]
    fn altering_a_step_is_detected(
        ids in ids(24),
        pick in any::<prop::sample::Index>(),
        step_pick in any::<prop::sample::Index>(),
        noise in any::<[u8; 32]>(),
    ) {
        prop_assume!(ids.len() >= 2);
        let root = merkle::merkle_root(&ids);
        let count = u32::try_from(ids.len()).expect("small");
        let index = pick.index(ids.len());
        let proof = InclusionProof::generate(&ids, index).expect("in range");
        prop_assume!(!proof.steps.is_empty());
        let target = step_pick.index(proof.steps.len());

        let mut altered = proof.clone();
        altered.steps[target].hash = Hash::from_bytes(noise);
        if altered.steps[target].hash != proof.steps[target].hash {
            prop_assert!(altered.verify(&ids[index], count, &root).is_err());
        }

        let mut flipped = proof.clone();
        flipped.steps[target].side = match flipped.steps[target].side {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        };
        prop_assert!(flipped.verify(&ids[index], count, &root).is_err());

        let mut appended = proof;
        appended.steps.push(ProofStep { side: Side::Left, hash: root });
        prop_assert!(appended.verify(&ids[index], count, &root).is_err());
    }

    /// A proof never verifies against a different block's root.
    ///
    /// This is the binding that matters. The `tx_count` check rejects an out-of-range
    /// index and a path whose shape is impossible for the claimed position, but it is
    /// not an independent binding to tree size: some sizes share a shape for a given
    /// index (3 and 4 both give index 0 a two-step path, for instance). What separates
    /// one block from another is the root, and a header carries the root and the count
    /// together.
    #[test]
    fn a_proof_never_verifies_against_a_different_root(ids in ids(24), other in ids(24)) {
        prop_assume!(ids != other);
        let root = merkle::merkle_root(&ids);
        let other_root = merkle::merkle_root(&other);
        prop_assume!(root != other_root);
        let count = u32::try_from(ids.len()).expect("small");
        let proof = InclusionProof::generate(&ids, 0).expect("in range");
        prop_assert!(proof.verify(&ids[0], count, &other_root).is_err());
    }

    /// An index at or beyond the declared count is always refused.
    #[test]
    fn an_index_outside_the_declared_count_is_refused(ids in ids(24), beyond in 0u32..8) {
        let root = merkle::merkle_root(&ids);
        let count = u32::try_from(ids.len()).expect("small");
        let mut proof = InclusionProof::generate(&ids, 0).expect("in range");
        proof.index = count.saturating_add(beyond);
        prop_assert!(proof.verify(&ids[0], count, &root).is_err());
    }

    /// The root depends on order.
    #[test]
    fn the_root_depends_on_order(ids in ids(24), swap in any::<prop::sample::Index>()) {
        prop_assume!(ids.len() >= 2);
        let i = swap.index(ids.len() - 1);
        let mut swapped = ids.clone();
        swapped.swap(i, i + 1);
        if swapped != ids {
            prop_assert_ne!(merkle::merkle_root(&swapped), merkle::merkle_root(&ids));
        }
    }

    /// Appending a leaf always changes the root, including duplicating the last one.
    #[test]
    fn appending_a_leaf_changes_the_root(ids in ids(24)) {
        let before = merkle::merkle_root(&ids);
        let mut extended = ids.clone();
        extended.push(*ids.last().expect("non-empty"));
        prop_assert_ne!(merkle::merkle_root(&extended), before);
    }

    /// Arbitrary bytes never panic a proof decoder.
    #[test]
    fn decoding_arbitrary_proof_bytes_never_panics(
        bytes in prop::collection::vec(any::<u8>(), 0..256),
    ) {
        let _ = InclusionProof::from_canonical_bytes(&bytes);
    }

    /// Hex parsing never panics and never accepts a non-canonical form.
    #[test]
    fn hex_parsing_is_total(text in ".{0,80}") {
        let _ = Hash::from_hex(&text);
        let _ = TxId::from_hex(&text);
        let _ = PublicKey::from_hex(&text);
        let _ = Signature::from_hex(&text);
        if let Ok(hash) = Hash::from_hex(&text) {
            prop_assert_eq!(hash.to_hex(), text);
        }
    }

    /// Label validation never panics and always round trips what it accepts.
    #[test]
    fn label_validation_is_total(text in ".{0,80}") {
        if let Ok(namespace) = Namespace::new(text.clone()) {
            prop_assert_eq!(namespace.as_str(), text);
        }
    }
}
