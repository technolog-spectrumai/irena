//! Every rule, asserted through both its finding kind and its exact location.

mod support;

use prunella_core::{BlockHeight, Hash, NetworkId, Signature, TxId};
use prunella_verify::{
    BlockContext, Finding, FindingKind, VerifyOptions, check_block, verify_chain,
};
use support::{MemoryChain, rebuild, signed_transaction};

fn kinds(findings: &[Finding]) -> Vec<FindingKind> {
    findings.iter().map(|finding| finding.kind).collect()
}

fn only(findings: &[Finding], kind: FindingKind) -> &Finding {
    let matching: Vec<&Finding> = findings.iter().filter(|f| f.kind == kind).collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one {kind}, got {:?}",
        kinds(findings)
    );
    matching[0]
}

#[test]
fn a_well_formed_chain_verifies() {
    let chain = MemoryChain::with_blocks(5);
    let report = verify_chain(&chain, VerifyOptions::default());
    assert!(report.is_valid(), "{report}");
    assert_eq!(report.blocks_checked, 6);
    assert_eq!(report.transactions_checked, 5);
    assert_eq!(report.range_start, Some(BlockHeight(0)));
    assert_eq!(report.range_end, Some(BlockHeight(5)));
}

#[test]
fn an_empty_chain_of_only_genesis_verifies() {
    let chain = MemoryChain::with_blocks(0);
    let report = verify_chain(&chain, VerifyOptions::default());
    assert!(report.is_valid(), "{report}");
    assert_eq!(report.blocks_checked, 1);
}

#[test]
fn a_wrong_previous_hash_is_reported_at_its_height() {
    let mut chain = MemoryChain::with_blocks(2);
    let tampered = rebuild(chain.blocks[2].as_ref().expect("block"), |draft| {
        draft.previous_hash = Hash::from_bytes([0x42; 32]);
    });
    chain.replace(BlockHeight(2), tampered);

    let report = verify_chain(&chain, VerifyOptions::default());
    assert!(!report.is_valid());
    let finding = only(&report.findings, FindingKind::PreviousHashMismatch);
    assert_eq!(finding.location.height, Some(BlockHeight(2)));
    assert!(
        finding.detail.contains("parent at height 1"),
        "{}",
        finding.detail
    );
}

#[test]
fn tampering_mid_chain_is_reported_at_the_tampered_block_and_at_its_child() {
    // Rewriting a block changes its hash, so its child no longer links to it. Both
    // findings are real and each names its own height: verification reports the whole
    // break rather than only its first symptom.
    let mut chain = MemoryChain::with_blocks(3);
    let tampered = rebuild(chain.blocks[2].as_ref().expect("block"), |draft| {
        draft.previous_hash = Hash::from_bytes([0x42; 32]);
    });
    chain.replace(BlockHeight(2), tampered);

    let report = verify_chain(&chain, VerifyOptions::default());
    let heights: Vec<Option<BlockHeight>> = report
        .findings
        .iter()
        .filter(|finding| finding.kind == FindingKind::PreviousHashMismatch)
        .map(|finding| finding.location.height)
        .collect();
    assert_eq!(heights, vec![Some(BlockHeight(2)), Some(BlockHeight(3))]);
}

#[test]
fn a_block_at_the_wrong_height_is_reported() {
    let mut chain = MemoryChain::with_blocks(2);
    let tampered = rebuild(chain.blocks[2].as_ref().expect("block"), |draft| {
        draft.height = BlockHeight(7);
    });
    chain.replace(BlockHeight(2), tampered);

    let report = verify_chain(&chain, VerifyOptions::default());
    let finding = only(&report.findings, FindingKind::HeightOutOfOrder);
    assert!(
        finding.detail.contains("expected height 2"),
        "{}",
        finding.detail
    );
}

#[test]
fn a_genesis_with_a_non_zero_previous_hash_is_reported() {
    let mut chain = MemoryChain::with_blocks(0);
    let tampered = rebuild(chain.blocks[0].as_ref().expect("genesis"), |draft| {
        draft.previous_hash = Hash::from_bytes([0x01; 32]);
    });
    chain.replace(BlockHeight(0), tampered);

    let report = verify_chain(&chain, VerifyOptions::default());
    let finding = only(&report.findings, FindingKind::PreviousHashMismatch);
    assert_eq!(finding.location.height, Some(BlockHeight::GENESIS));
    // Rewriting genesis also breaks the recorded genesis identity.
    assert!(kinds(&report.findings).contains(&FindingKind::GenesisMismatch));
}

#[test]
fn a_replaced_genesis_block_is_detected_even_when_self_consistent() {
    // The replacement is internally valid; only the independently recorded genesis
    // hash reveals that the chain's root was swapped.
    let mut chain = MemoryChain::with_blocks(0);
    let tampered = rebuild(chain.blocks[0].as_ref().expect("genesis"), |draft| {
        draft.timestamp_millis = 999;
    });
    chain.replace(BlockHeight(0), tampered);

    let report = verify_chain(&chain, VerifyOptions::default());
    let finding = only(&report.findings, FindingKind::GenesisMismatch);
    assert_eq!(finding.location.height, Some(BlockHeight::GENESIS));
}

#[test]
fn a_block_from_another_network_is_reported() {
    let mut chain = MemoryChain::with_blocks(2);
    let tampered = rebuild(chain.blocks[2].as_ref().expect("block"), |draft| {
        draft.network_id = NetworkId::new("othernet").expect("valid");
    });
    chain.replace(BlockHeight(2), tampered);

    let report = verify_chain(&chain, VerifyOptions::default());
    let finding = only(&report.findings, FindingKind::NetworkMismatch);
    assert!(finding.detail.contains("othernet"), "{}", finding.detail);
}

#[test]
fn a_tampered_transaction_count_is_reported() {
    let chain = MemoryChain::with_blocks(1);
    let mut block = chain.blocks[1].clone().expect("block");
    block.header.tx_count = 9;

    let findings = check_block(
        &BlockContext::child_of(
            &chain.network_id,
            &chain.blocks[0].as_ref().expect("g").header,
        ),
        &block,
    );
    let finding = only(&findings, FindingKind::TxCountMismatch);
    assert!(finding.detail.contains("declares 9"), "{}", finding.detail);
}

#[test]
fn a_tampered_transaction_root_is_reported() {
    let chain = MemoryChain::with_blocks(1);
    let mut block = chain.blocks[1].clone().expect("block");
    block.header.tx_root = Hash::from_bytes([0x11; 32]);

    let findings = check_block(
        &BlockContext::child_of(
            &chain.network_id,
            &chain.blocks[0].as_ref().expect("g").header,
        ),
        &block,
    );
    only(&findings, FindingKind::TxRootMismatch);
}

#[test]
fn removing_a_transaction_breaks_both_the_count_and_the_root() {
    let chain = MemoryChain::with_blocks(1);
    let mut block = chain.blocks[1].clone().expect("block");
    block.transactions.clear();

    let findings = check_block(
        &BlockContext::child_of(
            &chain.network_id,
            &chain.blocks[0].as_ref().expect("g").header,
        ),
        &block,
    );
    let found = kinds(&findings);
    assert!(found.contains(&FindingKind::TxCountMismatch), "{found:?}");
    assert!(found.contains(&FindingKind::TxRootMismatch), "{found:?}");
}

#[test]
fn a_transaction_with_a_wrong_id_is_reported_with_its_index() {
    let mut chain = MemoryChain::with_blocks(1);
    let mut block = chain.blocks[1].clone().expect("block");
    block.transactions[0].id = TxId::from_hash(Hash::from_bytes([0x55; 32]));
    // Rebuild the header so the root still commits to the (now wrong) declared id,
    // isolating the id rule from the root rule.
    let block = rebuild(&block, |draft| {
        draft.transactions = block.transactions.clone();
    });
    chain.replace(BlockHeight(1), block);

    let report = verify_chain(&chain, VerifyOptions::default());
    let finding = only(&report.findings, FindingKind::TxIdMismatch);
    assert_eq!(finding.location.height, Some(BlockHeight(1)));
    assert_eq!(finding.location.tx_index, Some(0));
    assert_eq!(
        finding.location.tx_id,
        Some(TxId::from_hash(Hash::from_bytes([0x55; 32])))
    );
}

#[test]
fn a_transaction_with_an_invalid_signature_is_reported_with_its_index() {
    let mut chain = MemoryChain::with_blocks(1);
    let mut block = chain.blocks[1].clone().expect("block");
    block.transactions[0].signature = Signature::from_bytes([0u8; 64]);
    block.transactions[0].id = block.transactions[0].compute_id();
    let block = rebuild(&block, |draft| {
        draft.transactions = block.transactions.clone();
    });
    chain.replace(BlockHeight(1), block);

    let report = verify_chain(&chain, VerifyOptions::default());
    let finding = only(&report.findings, FindingKind::SignatureInvalid);
    assert_eq!(finding.location.height, Some(BlockHeight(1)));
    assert_eq!(finding.location.tx_index, Some(0));
}

#[test]
fn a_tampered_payload_breaks_the_id_and_the_signature_together() {
    let mut chain = MemoryChain::with_blocks(1);
    let mut block = chain.blocks[1].clone().expect("block");
    block.transactions[0].payload = b"rewritten".to_vec();
    chain.replace(BlockHeight(1), block);

    let report = verify_chain(&chain, VerifyOptions::default());
    let found = kinds(&report.findings);
    assert!(found.contains(&FindingKind::TxIdMismatch), "{found:?}");
    assert!(found.contains(&FindingKind::SignatureInvalid), "{found:?}");
    // The transaction root commits to declared ids, not to payloads, so it still
    // matches. The payload is caught by the id derivation and by the signature, which
    // is exactly why both of those rules exist alongside the root.
    assert!(!found.contains(&FindingKind::TxRootMismatch), "{found:?}");
}

#[test]
fn a_transaction_repeated_inside_one_block_is_reported() {
    let chain = MemoryChain::with_blocks(0);
    let transaction = signed_transaction(1, "duplicated", 1);
    let block = chain.next_block(vec![transaction.clone(), transaction]);

    let findings = check_block(
        &BlockContext::child_of(
            &chain.network_id,
            &chain.blocks[0].as_ref().expect("g").header,
        ),
        &block,
    );
    let finding = only(&findings, FindingKind::DuplicateTxIdInBlock);
    assert_eq!(finding.location.tx_index, Some(1));
}

#[test]
fn a_transaction_replayed_in_a_later_block_is_reported() {
    let mut chain = MemoryChain::with_blocks(0);
    let transaction = signed_transaction(1, "replayed", 1);
    let first = chain.next_block(vec![transaction.clone()]);
    chain.push(first);
    let second = chain.next_block(vec![transaction]);
    chain.push(second);

    let report = verify_chain(&chain, VerifyOptions::default());
    let finding = only(&report.findings, FindingKind::DuplicateTxIdInChain);
    assert_eq!(finding.location.height, Some(BlockHeight(2)));
}

#[test]
fn duplicate_detection_can_be_turned_off() {
    let mut chain = MemoryChain::with_blocks(0);
    let transaction = signed_transaction(1, "replayed", 1);
    let first = chain.next_block(vec![transaction.clone()]);
    chain.push(first);
    let second = chain.next_block(vec![transaction]);
    chain.push(second);

    let options = VerifyOptions {
        detect_duplicate_transactions: false,
        ..Default::default()
    };
    assert!(verify_chain(&chain, options).is_valid());
}

#[test]
fn a_timestamp_going_backwards_is_reported() {
    let mut chain = MemoryChain::with_blocks(2);
    let tampered = rebuild(chain.blocks[2].as_ref().expect("block"), |draft| {
        draft.timestamp_millis = 0;
    });
    chain.replace(BlockHeight(2), tampered);

    let report = verify_chain(&chain, VerifyOptions::default());
    let finding = only(&report.findings, FindingKind::TimestampRegression);
    assert_eq!(finding.location.height, Some(BlockHeight(2)));
}

#[test]
fn an_equal_timestamp_is_allowed() {
    let mut chain = MemoryChain::with_blocks(1);
    let parent_timestamp = chain.blocks[0].as_ref().expect("g").header.timestamp_millis;
    let tampered = rebuild(chain.blocks[1].as_ref().expect("block"), |draft| {
        draft.timestamp_millis = parent_timestamp;
    });
    chain.replace(BlockHeight(1), tampered);

    assert!(verify_chain(&chain, VerifyOptions::default()).is_valid());
}

#[test]
fn an_unsupported_header_version_is_reported() {
    let chain = MemoryChain::with_blocks(1);
    let mut block = chain.blocks[1].clone().expect("block");
    block.header.version = 99;

    let findings = check_block(
        &BlockContext::child_of(
            &chain.network_id,
            &chain.blocks[0].as_ref().expect("g").header,
        ),
        &block,
    );
    only(&findings, FindingKind::UnsupportedHeaderVersion);
}

#[test]
fn a_block_that_does_not_match_the_hash_it_was_found_under_is_reported() {
    let chain = MemoryChain::with_blocks(1);
    let block = chain.blocks[1].clone().expect("block");
    let context = BlockContext::child_of(
        &chain.network_id,
        &chain.blocks[0].as_ref().expect("g").header,
    )
    .expecting_hash(Hash::from_bytes([0x77; 32]));

    let findings = check_block(&context, &block);
    let finding = only(&findings, FindingKind::BlockHashMismatch);
    assert!(
        finding.detail.contains("looked up as"),
        "{}",
        finding.detail
    );
}

#[test]
fn a_missing_block_ends_the_walk_at_its_height() {
    let mut chain = MemoryChain::with_blocks(4);
    chain.remove(BlockHeight(2));

    let report = verify_chain(&chain, VerifyOptions::default());
    let finding = only(&report.findings, FindingKind::MissingBlock);
    assert_eq!(finding.location.height, Some(BlockHeight(2)));
    // Heights 0 and 1 were still checked; nothing past the gap is claimed.
    assert_eq!(report.blocks_checked, 2);
    assert_eq!(report.range_end, Some(BlockHeight(1)));
}

#[test]
fn an_unreadable_block_is_reported_as_a_decode_error() {
    let mut chain = MemoryChain::with_blocks(3);
    chain.read_failure_at = Some(BlockHeight(2));

    let report = verify_chain(&chain, VerifyOptions::default());
    let finding = only(&report.findings, FindingKind::DecodeError);
    assert_eq!(finding.location.height, Some(BlockHeight(2)));
    assert!(
        finding.detail.contains("simulated storage corruption"),
        "{}",
        finding.detail
    );
}

#[test]
fn a_range_verification_loads_the_parent_it_needs() {
    let chain = MemoryChain::with_blocks(5);
    let report = verify_chain(&chain, VerifyOptions::range(BlockHeight(3), BlockHeight(4)));
    assert!(report.is_valid(), "{report}");
    assert_eq!(report.blocks_checked, 2);
    assert_eq!(report.range_start, Some(BlockHeight(3)));
    assert_eq!(report.range_end, Some(BlockHeight(4)));
}

#[test]
fn a_range_verification_still_catches_a_break_inside_it() {
    let mut chain = MemoryChain::with_blocks(5);
    let tampered = rebuild(chain.blocks[4].as_ref().expect("block"), |draft| {
        draft.previous_hash = Hash::ZERO;
    });
    chain.replace(BlockHeight(4), tampered);

    let report = verify_chain(&chain, VerifyOptions::range(BlockHeight(3), BlockHeight(4)));
    only(&report.findings, FindingKind::PreviousHashMismatch);
}

#[test]
fn a_range_beyond_the_head_is_clamped_to_the_head() {
    let chain = MemoryChain::with_blocks(2);
    let report = verify_chain(
        &chain,
        VerifyOptions::range(BlockHeight(1), BlockHeight(99)),
    );
    assert!(report.is_valid(), "{report}");
    assert_eq!(report.range_end, Some(BlockHeight(2)));
}

#[test]
fn a_report_renders_findings_with_their_locations() {
    let mut chain = MemoryChain::with_blocks(2);
    let tampered = rebuild(chain.blocks[2].as_ref().expect("block"), |draft| {
        draft.previous_hash = Hash::ZERO;
    });
    chain.replace(BlockHeight(2), tampered);

    let report = verify_chain(&chain, VerifyOptions::default());
    let rendered = report.to_string();
    assert!(rendered.contains("INVALID"), "{rendered}");
    assert!(rendered.contains("previous_hash_mismatch"), "{rendered}");
    assert!(rendered.contains("height 2"), "{rendered}");

    let json = serde_json::to_string(&report).expect("report serializes");
    assert!(json.contains("\"previous_hash_mismatch\""), "{json}");
}
