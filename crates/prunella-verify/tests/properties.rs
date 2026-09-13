//! Property tests for block and transaction verification.
//!
//! The contract: arbitrary structurally-valid-but-wrong input produces findings, never
//! a panic, and a block that passes has no finding to give.

mod support;

use proptest::prelude::*;
use prunella_core::{
    Block, BlockHeight, Hash, NetworkId, PublicKey, SchemaVersion, Signature, Transaction,
};
use prunella_crypto::verify_transaction;
use prunella_verify::{BlockContext, FindingKind, VerifyOptions, check_block, verify_chain};
use support::{MemoryChain, signed_transaction};

/// A block built honestly, then corrupted in one arbitrary way.
fn corruption() -> impl Strategy<Value = usize> {
    0usize..10
}

fn corrupt(block: &mut Block, which: usize, noise: [u8; 32], number: u64) {
    match which {
        0 => block.header.version = block.header.version.wrapping_add(1).max(2),
        1 => block.header.network_id = NetworkId::new("othernet").expect("valid"),
        2 => block.header.height = BlockHeight(number),
        3 => block.header.previous_hash = Hash::from_bytes(noise),
        4 => block.header.tx_root = Hash::from_bytes(noise),
        5 => block.header.tx_count = block.header.tx_count.wrapping_add(1),
        6 => block.header.timestamp_millis = 0,
        7 => {
            if let Some(tx) = block.transactions.first_mut() {
                tx.payload.push(0xff);
            }
        }
        8 => {
            if let Some(tx) = block.transactions.first_mut() {
                let mut bytes = tx.signature.to_bytes();
                bytes[0] ^= 0x01;
                tx.signature = Signature::from_bytes(bytes);
            }
        }
        _ => {
            if let Some(tx) = block.transactions.first_mut() {
                tx.signer = PublicKey::from_bytes(noise);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(192))]

    /// An honestly built chain always verifies, whatever its shape.
    #[test]
    fn an_honest_chain_always_verifies(blocks in 0u64..12) {
        let chain = MemoryChain::with_blocks(blocks);
        let report = verify_chain(&chain, VerifyOptions::default());
        prop_assert!(report.is_valid(), "{report}");
        prop_assert_eq!(report.blocks_checked, blocks + 1);
    }

    /// Verification from any starting height never panics and never claims more blocks
    /// than the chain holds.
    #[test]
    fn verification_from_any_height_is_total(blocks in 0u64..8, from in 0u64..32, to in 0u64..32) {
        let chain = MemoryChain::with_blocks(blocks);
        let report = verify_chain(
            &chain,
            VerifyOptions {
                from: Some(BlockHeight(from)),
                to: Some(BlockHeight(to)),
                detect_duplicate_transactions: true,
            },
        );
        prop_assert!(report.blocks_checked <= blocks + 1);
    }

    /// Any single corruption of a valid block produces at least one finding.
    #[test]
    fn every_corruption_is_caught(
        which in corruption(),
        noise in any::<[u8; 32]>(),
        number in any::<u64>(),
    ) {
        let chain = MemoryChain::with_blocks(1);
        let parent = &chain.blocks[0].as_ref().expect("genesis").header;
        let mut block = chain.blocks[1].clone().expect("block");
        let before = check_block(&BlockContext::child_of(&chain.network_id, parent), &block);
        prop_assert!(before.is_empty(), "the starting block must be valid: {before:?}");

        corrupt(&mut block, which, noise, number);
        let after = check_block(&BlockContext::child_of(&chain.network_id, parent), &block);

        // Corruption 2 and 6 can be no-ops when the arbitrary value happens to equal
        // the honest one; everything else always changes something.
        let unchanged = (which == 2 && number == 1) || (which == 6 && parent.timestamp_millis == 0);
        if !unchanged {
            prop_assert!(!after.is_empty(), "corruption {which} went undetected");
        }
    }

    /// Checking a block never panics, whatever the header holds.
    #[test]
    fn checking_never_panics(
        version in any::<u16>(),
        height in any::<u64>(),
        previous in any::<[u8; 32]>(),
        root in any::<[u8; 32]>(),
        count in any::<u32>(),
        timestamp in any::<u64>(),
    ) {
        let chain = MemoryChain::with_blocks(1);
        let parent = &chain.blocks[0].as_ref().expect("genesis").header;
        let mut block = chain.blocks[1].clone().expect("block");
        block.header.version = version;
        block.header.height = BlockHeight(height);
        block.header.previous_hash = Hash::from_bytes(previous);
        block.header.tx_root = Hash::from_bytes(root);
        block.header.tx_count = count;
        block.header.timestamp_millis = timestamp;

        let _ = check_block(&BlockContext::child_of(&chain.network_id, parent), &block);
        let _ = check_block(&BlockContext::genesis(&chain.network_id), &block);
    }

    /// Transaction verification never panics on arbitrary key and signature bytes.
    #[test]
    fn transaction_verification_is_total(
        signer in any::<[u8; 32]>(),
        signature in any::<[u8; 64]>(),
        payload in prop::collection::vec(any::<u8>(), 0..64),
        nonce in any::<u64>(),
        schema in any::<u32>(),
    ) {
        let mut transaction: Transaction = signed_transaction(1, "payload", 1);
        transaction.signer = PublicKey::from_bytes(signer);
        transaction.signature = Signature::from_bytes(signature);
        transaction.payload = payload;
        transaction.nonce = nonce;
        transaction.schema_version = SchemaVersion(schema);
        transaction.id = transaction.compute_id();

        // Whatever the bytes, verification answers rather than crashing. Random bytes
        // are essentially never a valid signature.
        prop_assert!(verify_transaction(&transaction).is_err());
        prop_assert!(transaction.has_consistent_id());
    }

    /// A duplicated transaction inside a block is always caught.
    #[test]
    fn a_duplicated_transaction_is_always_caught(nonce in any::<u64>()) {
        let chain = MemoryChain::with_blocks(0);
        let parent = &chain.blocks[0].as_ref().expect("genesis").header;
        let transaction = signed_transaction(1, "duplicated", nonce);
        let block = chain.next_block(vec![transaction.clone(), transaction]);
        let findings = check_block(&BlockContext::child_of(&chain.network_id, parent), &block);
        prop_assert!(
            findings.iter().any(|f| f.kind == FindingKind::DuplicateTxIdInBlock),
            "{findings:?}"
        );
    }
}
