//! The V1 freeze.
//!
//! Every committed vector is held against the implementation and against the
//! independent oracle, field by field, byte for byte. A dependency upgrade that changed
//! Borsh's layout, BLAKE3's output or Ed25519's signatures would fail here, naming the
//! vector and the field that moved.

use prunella_conformance::oracle;
use prunella_conformance::vectors::{self, VectorFile};
use prunella_core::{InclusionProof, Side};
use std::collections::BTreeMap;

const REQUIRED: &[&str] = &[
    "genesis",
    "empty-block",
    "one-transaction",
    "three-transactions",
    "five-transactions",
    "boundary-values",
    "arbitrary-payload-bytes",
];

fn committed() -> BTreeMap<String, (String, VectorFile)> {
    let dir = prunella_conformance::vector_dir();
    let mut files = BTreeMap::new();
    for entry in
        std::fs::read_dir(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
    {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read vector");
        let vector: VectorFile = serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("{} is not a vector file: {error}", path.display()));
        files.insert(vector.name.clone(), (text, vector));
    }
    files
}

fn unhex(text: &str) -> Vec<u8> {
    hex::decode(text).expect("hex")
}

fn unhex32(text: &str) -> [u8; 32] {
    unhex(text).try_into().expect("32 bytes")
}

fn unhex64(text: &str) -> [u8; 64] {
    unhex(text).try_into().expect("64 bytes")
}

#[test]
fn every_required_vector_is_committed() {
    let files = committed();
    for name in REQUIRED {
        assert!(files.contains_key(*name), "missing vector {name}");
    }
    for (name, (_, vector)) in &files {
        assert_eq!(vector.protocol, vectors::PROTOCOL, "{name}");
        assert_eq!(&vector.name, name);
    }
}

#[test]
fn the_implementation_reproduces_every_committed_vector_exactly() {
    for (name, (_, vector)) in committed() {
        let block = vectors::build_block(&vector.keys, &vector.block.input);
        let derived = vectors::derive_expected(&block);
        let expected = &vector.block.expected;

        assert_eq!(
            derived.header_hex, expected.header_hex,
            "{name}: header bytes"
        );
        assert_eq!(
            derived.block_hash_hex, expected.block_hash_hex,
            "{name}: block hash"
        );
        assert_eq!(derived.tx_root_hex, expected.tx_root_hex, "{name}: tx root");
        assert_eq!(derived.tx_count, expected.tx_count, "{name}: tx count");
        assert_eq!(
            derived.canonical_block_hex, expected.canonical_block_hex,
            "{name}: canonical block bytes"
        );
        assert_eq!(
            derived.leaf_hashes_hex, expected.leaf_hashes_hex,
            "{name}: leaf hashes"
        );
        assert_eq!(
            derived.transactions.len(),
            expected.transactions.len(),
            "{name}"
        );
        for (index, (got, want)) in derived
            .transactions
            .iter()
            .zip(&expected.transactions)
            .enumerate()
        {
            assert_eq!(
                got.signing_preimage_hex, want.signing_preimage_hex,
                "{name} tx {index}: signing preimage"
            );
            assert_eq!(
                got.signing_message_hex, want.signing_message_hex,
                "{name} tx {index}: signing message"
            );
            assert_eq!(
                got.signature_hex, want.signature_hex,
                "{name} tx {index}: signature"
            );
            assert_eq!(
                got.id_preimage_hex, want.id_preimage_hex,
                "{name} tx {index}: id preimage"
            );
            assert_eq!(got.id_hex, want.id_hex, "{name} tx {index}: id");
            assert_eq!(
                got.canonical_transaction_hex, want.canonical_transaction_hex,
                "{name} tx {index}: canonical bytes"
            );
        }
        assert_eq!(
            derived.inclusion_proofs, expected.inclusion_proofs,
            "{name}: inclusion proofs"
        );
        assert_eq!(derived, *expected, "{name}: whole expected block");
    }
}

#[test]
fn the_independent_oracle_reproduces_every_committed_vector_exactly() {
    for (name, (_, vector)) in committed() {
        let input = &vector.block.input;
        let expected = &vector.block.expected;

        let mut ids: Vec<[u8; 32]> = Vec::new();
        let mut encoded_transactions: Vec<Vec<u8>> = Vec::new();
        for (index, tx_input) in input.transactions.iter().enumerate() {
            let want = &expected.transactions[index];
            let key = &vector.keys[tx_input.signer_key_index];
            let seed = unhex32(&key.seed_hex);
            let public = oracle::public_key(seed);
            assert_eq!(
                hex::encode(public),
                key.public_key_hex,
                "{name}: key {}",
                tx_input.signer_key_index
            );

            let payload = unhex(&tx_input.payload_hex);
            let tx = oracle::TxInput {
                namespace: &tx_input.namespace,
                schema_version: tx_input.schema_version,
                payload: &payload,
                signer: public,
                nonce: tx_input.nonce,
            };
            let preimage = oracle::signing_preimage(&tx);
            assert_eq!(
                hex::encode(&preimage),
                want.signing_preimage_hex,
                "{name} tx {index}: signing preimage"
            );
            let message = oracle::signing_message(&tx);
            assert_eq!(
                hex::encode(message),
                want.signing_message_hex,
                "{name} tx {index}: signing message"
            );
            let signature = oracle::sign(seed, &message);
            assert_eq!(
                hex::encode(signature),
                want.signature_hex,
                "{name} tx {index}: signature"
            );
            assert_eq!(
                hex::encode(oracle::id_preimage(&tx, &signature)),
                want.id_preimage_hex,
                "{name} tx {index}: id preimage"
            );
            let id = oracle::tx_id(&tx, &signature);
            assert_eq!(hex::encode(id), want.id_hex, "{name} tx {index}: id");
            let encoded = oracle::transaction(&id, &tx, &signature);
            assert_eq!(
                hex::encode(&encoded),
                want.canonical_transaction_hex,
                "{name} tx {index}: canonical bytes"
            );
            assert_eq!(
                hex::encode(oracle::leaf(&id)),
                expected.leaf_hashes_hex[index],
                "{name} tx {index}: leaf"
            );
            ids.push(id);
            encoded_transactions.push(encoded);
        }

        let root = oracle::merkle_root(&ids);
        assert_eq!(hex::encode(root), expected.tx_root_hex, "{name}: tx root");

        for (index, want) in expected.inclusion_proofs.iter().enumerate() {
            let path = oracle::audit_path(&ids, index);
            assert_eq!(want.index as usize, index, "{name}: proof index");
            assert_eq!(
                path.len(),
                want.steps.len(),
                "{name} proof {index}: step count"
            );
            for (step, want_step) in path.iter().zip(&want.steps) {
                assert_eq!(
                    if step.0 { "left" } else { "right" },
                    want_step.side,
                    "{name} proof {index}: side"
                );
                assert_eq!(
                    hex::encode(step.1),
                    want_step.hash_hex,
                    "{name} proof {index}: hash"
                );
            }
            assert_eq!(
                hex::encode(oracle::proof(index as u32, &path)),
                want.canonical_proof_hex,
                "{name} proof {index}: canonical bytes"
            );
        }

        let header = oracle::header(&oracle::HeaderInput {
            network_id: &input.network_id,
            height: input.height,
            previous_hash: unhex32(&input.previous_hash_hex),
            tx_root: root,
            tx_count: u32::try_from(ids.len()).expect("small"),
            timestamp_millis: input.timestamp_millis,
        });
        assert_eq!(
            hex::encode(&header),
            expected.header_hex,
            "{name}: header bytes"
        );
        assert_eq!(
            hex::encode(oracle::block_hash(&header)),
            expected.block_hash_hex,
            "{name}: block hash"
        );
        assert_eq!(
            hex::encode(oracle::block(&header, &encoded_transactions)),
            expected.canonical_block_hex,
            "{name}: canonical block"
        );
    }
}

#[test]
fn committed_proofs_verify_against_committed_headers_and_reject_tampering() {
    for (name, (_, vector)) in committed() {
        let block = vectors::build_block(&vector.keys, &vector.block.input);
        for (index, want) in vector.block.expected.inclusion_proofs.iter().enumerate() {
            let proof = InclusionProof {
                index: want.index,
                steps: want
                    .steps
                    .iter()
                    .map(|step| prunella_core::ProofStep {
                        side: if step.side == "left" {
                            Side::Left
                        } else {
                            Side::Right
                        },
                        hash: prunella_core::Hash::from_hex(&step.hash_hex).expect("hash"),
                    })
                    .collect(),
            };
            let id = block.transactions[index].id;
            proof
                .verify_against(&id, &block.header)
                .unwrap_or_else(|error| {
                    panic!("{name} proof {index}: {error}");
                });

            // The same proof must not verify for any other transaction in the block.
            for (other_index, other) in block.transactions.iter().enumerate() {
                if other_index != index {
                    assert!(
                        proof.verify_against(&other.id, &block.header).is_err(),
                        "{name} proof {index} accepted tx {other_index}"
                    );
                }
            }
        }
    }
}

#[test]
fn regenerating_reproduces_the_committed_files_byte_for_byte() {
    // This is the freeze. If the implementation changes in any way that touches a V1
    // derivation, the generator's output moves and this fails; the fix is to revert the
    // change or introduce V2, never to regenerate the files.
    let files = committed();
    let generated = vectors::build_all();
    assert_eq!(generated.len(), files.len(), "vector count");
    for vector in generated {
        let (text, _) = files
            .get(&vector.name)
            .unwrap_or_else(|| panic!("no committed file for {}", vector.name));
        assert_eq!(
            &vectors::render(&vector),
            text,
            "{}: committed file differs from regeneration",
            vector.name
        );
    }
}

#[test]
fn the_signature_of_a_committed_vector_is_the_only_one_accepted() {
    // Ed25519 is deterministic and verification is strict, so the recorded signature is
    // the single acceptable signature for that key and message. Re-signing yields the
    // same bytes; any other bytes are refused.
    for (name, (_, vector)) in committed() {
        for (index, tx_input) in vector.block.input.transactions.iter().enumerate() {
            let tx = vectors::build_transaction(&vector.keys, tx_input);
            let want = unhex64(&vector.block.expected.transactions[index].signature_hex);
            assert_eq!(tx.signature.to_bytes(), want, "{name} tx {index}");
            prunella_crypto::verify_transaction(&tx)
                .unwrap_or_else(|error| panic!("{name} tx {index}: {error}"));

            let mut altered = tx.clone();
            let mut bytes = altered.signature.to_bytes();
            bytes[5] ^= 0x01;
            altered.signature = prunella_core::Signature::from_bytes(bytes);
            assert!(
                prunella_crypto::verify_transaction(&altered).is_err(),
                "{name} tx {index}: altered signature accepted"
            );
        }
    }
}
