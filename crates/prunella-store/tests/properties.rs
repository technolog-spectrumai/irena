//! Property tests and atomicity stress for persistent storage.
//!
//! The contract under stress: after any sequence of appends, some of which fail, a
//! reopened chain holds either the previous valid head or the complete appended block —
//! never a partial block, a dangling index entry, or a head that points at nothing.

use proptest::prelude::*;
use prunella_canonical::Canonical;
use prunella_core::{
    Block, BlockHeight, GenesisSpec, Hash, Namespace, NetworkId, SchemaVersion, Signature,
    Transaction, TransactionDraft, TxId,
};
use prunella_crypto::SigningKey;
use prunella_store::{AppendStatus, ChainStorage, LocalChainStore};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use std::collections::BTreeSet;
use std::path::Path;
use tempfile::TempDir;

const BLOCKS: TableDefinition<'static, u64, &[u8]> = TableDefinition::new("prunella_blocks");
const HEIGHT_BY_HASH: TableDefinition<'static, &[u8], u64> =
    TableDefinition::new("prunella_height_by_hash");
const TX_LOCATION: TableDefinition<'static, &[u8], &[u8]> =
    TableDefinition::new("prunella_tx_location");
const META: TableDefinition<'static, &str, &[u8]> = TableDefinition::new("prunella_meta");

fn network() -> NetworkId {
    NetworkId::new("testnet").expect("valid network id")
}

fn transaction(seed: u8, payload: &str, nonce: u64) -> Transaction {
    let key = SigningKey::from_seed([seed; 32]);
    key.sign_transaction(TransactionDraft {
        namespace: Namespace::new("app.demo").expect("valid namespace"),
        schema_version: SchemaVersion(1),
        payload: payload.as_bytes().to_vec(),
        signer: key.public_key(),
        nonce,
    })
}

fn next_block(store: &LocalChainStore, transactions: Vec<Transaction>) -> Block {
    let head = store.head().expect("head");
    let parent = store
        .get_block(head.height)
        .expect("read")
        .expect("head block");
    parent
        .header
        .child_draft(transactions, parent.header.timestamp_millis + 1)
        .expect("draft")
        .build()
        .expect("build")
}

/// Every index in the store, read directly, so a dangling entry is visible.
struct RawState {
    blocks: Vec<(u64, Vec<u8>)>,
    hashes: BTreeSet<(Vec<u8>, u64)>,
    transactions: BTreeSet<(Vec<u8>, Vec<u8>)>,
    head_height: u64,
    head_hash: Vec<u8>,
    transaction_count: u64,
}

fn raw_state(path: &Path) -> RawState {
    let database = Database::open(path).expect("open raw");
    let read = database.begin_read().expect("read txn");
    let blocks = read
        .open_table(BLOCKS)
        .expect("blocks")
        .iter()
        .expect("iter")
        .map(|e| {
            let (h, b) = e.expect("entry");
            (h.value(), b.value().to_vec())
        })
        .collect();
    let hashes = read
        .open_table(HEIGHT_BY_HASH)
        .expect("hash index")
        .iter()
        .expect("iter")
        .map(|e| {
            let (k, v) = e.expect("entry");
            (k.value().to_vec(), v.value())
        })
        .collect();
    let transactions = read
        .open_table(TX_LOCATION)
        .expect("tx index")
        .iter()
        .expect("iter")
        .map(|e| {
            let (k, v) = e.expect("entry");
            (k.value().to_vec(), v.value().to_vec())
        })
        .collect();
    let meta = read.open_table(META).expect("meta");
    let get = |key: &str| {
        meta.get(key)
            .expect("get")
            .expect("present")
            .value()
            .to_vec()
    };
    RawState {
        blocks,
        hashes,
        transactions,
        head_height: u64::from_le_bytes(get("head_height").try_into().expect("u64")),
        head_hash: get("head_hash"),
        transaction_count: u64::from_le_bytes(get("transaction_count").try_into().expect("u64")),
    }
}

/// Checks that no index entry refers to anything that is not there.
///
/// This is what "never partial indexes/state" means concretely: the head names a block
/// that exists and hashes to it; every hash-index entry names a block that hashes to
/// that key; every transaction-index entry names a transaction that is really at that
/// position; and there are exactly as many entries as the committed blocks imply.
fn assert_fully_consistent(path: &Path) {
    let state = raw_state(path);
    let decoded: Vec<(u64, Block)> = state
        .blocks
        .iter()
        .map(|(height, bytes)| {
            (
                *height,
                Block::from_canonical_bytes(bytes).expect("stored block decodes"),
            )
        })
        .collect();

    let heights: Vec<u64> = decoded.iter().map(|(h, _)| *h).collect();
    assert_eq!(
        heights,
        (0..heights.len() as u64).collect::<Vec<u64>>(),
        "block heights must be contiguous from zero"
    );
    assert_eq!(
        state.head_height,
        heights.len() as u64 - 1,
        "head height must name the highest stored block"
    );

    let head_block = &decoded[state.head_height as usize].1;
    assert_eq!(
        head_block.hash().as_bytes(),
        state.head_hash,
        "head hash must match its block"
    );

    assert_eq!(
        state.hashes.len(),
        decoded.len(),
        "one hash index entry per block"
    );
    for (height, block) in &decoded {
        assert!(
            state
                .hashes
                .contains(&(block.hash().as_bytes().to_vec(), *height)),
            "hash index is missing height {height}"
        );
    }

    let mut expected_transactions = BTreeSet::new();
    let mut count = 0u64;
    for (height, block) in &decoded {
        for (index, transaction) in block.transactions.iter().enumerate() {
            let mut location = height.to_le_bytes().to_vec();
            location.extend_from_slice(&u32::try_from(index).expect("small").to_le_bytes());
            expected_transactions.insert((transaction.id.as_bytes().to_vec(), location));
            count += 1;
        }
    }
    assert_eq!(
        state.transactions, expected_transactions,
        "the transaction index must match the committed blocks exactly"
    );
    assert_eq!(
        state.transaction_count, count,
        "the transaction count must match"
    );

    // And the chain must verify end to end through the public API.
    let store = LocalChainStore::open(path).expect("reopen");
    let report = store.verify_from(BlockHeight::GENESIS);
    assert!(report.is_valid(), "{report}");
}

/// One step in an append sequence.
#[derive(Debug, Clone, Copy)]
enum Step {
    /// A block that should be accepted.
    Valid,
    /// A block whose payload was rewritten after signing.
    TamperedPayload,
    /// A block whose parent link is wrong.
    WrongParent,
    /// A block at a height the chain is not waiting for.
    WrongHeight,
    /// A block carrying a transaction already committed.
    ReplayedTransaction,
    /// The block already at the head, offered again unchanged.
    RepeatHead,
}

fn steps() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(
        prop_oneof![
            3 => Just(Step::Valid),
            1 => Just(Step::TamperedPayload),
            1 => Just(Step::WrongParent),
            1 => Just(Step::WrongHeight),
            1 => Just(Step::ReplayedTransaction),
            1 => Just(Step::RepeatHead),
        ],
        1..14,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// After any mixture of succeeding and failing appends, the chain is whole.
    ///
    /// Every failed append must leave the previous valid head; every accepted one must
    /// leave the complete block with all of its indexes. Nothing in between is allowed
    /// to survive a reopen.
    #[test]
    fn a_chain_is_whole_after_any_mixture_of_appends(steps in steps()) {
        let directory = TempDir::new().expect("temp dir");
        let path = directory.path().join("stress.prunella");
        let store = LocalChainStore::init_genesis(&path, GenesisSpec::new(network()))
            .expect("create");

        let mut committed = 0u64;
        let mut nonce = 0u64;
        let mut first_transaction: Option<Transaction> = None;

        for step in steps {
            let head_before = store.head().expect("head");
            nonce += 1;
            let payload = format!("payload{nonce}");

            let result = match step {
                Step::Valid => {
                    let transaction = transaction(1, &payload, nonce);
                    if first_transaction.is_none() {
                        first_transaction = Some(transaction.clone());
                    }
                    store.append_block(next_block(&store, vec![transaction]))
                }
                Step::TamperedPayload => {
                    let mut block = next_block(&store, vec![transaction(1, &payload, nonce)]);
                    block.transactions[0].payload = b"rewritten".to_vec();
                    store.append_block(block)
                }
                Step::WrongParent => {
                    let mut block = next_block(&store, vec![transaction(1, &payload, nonce)]);
                    block.header.previous_hash = Hash::from_bytes([0x33; 32]);
                    store.append_block(block)
                }
                Step::WrongHeight => {
                    let mut block = next_block(&store, vec![transaction(1, &payload, nonce)]);
                    block.header.height = BlockHeight(head_before.height.value() + 7);
                    store.append_block(block)
                }
                Step::ReplayedTransaction => match &first_transaction {
                    Some(earlier) => store.append_block(next_block(&store, vec![earlier.clone()])),
                    None => continue,
                },
                Step::RepeatHead => {
                    let head_block = store
                        .get_block(head_before.height)
                        .expect("read")
                        .expect("head block");
                    store.append_block(head_block)
                }
            };

            match result {
                Ok(outcome) => {
                    match outcome.status {
                        AppendStatus::Committed => {
                            committed += 1;
                            prop_assert_eq!(outcome.head.height.value(), committed);
                        }
                        AppendStatus::AlreadyPresent => {
                            prop_assert_eq!(outcome.head, head_before);
                        }
                    }
                }
                Err(_) => {
                    // A failed append leaves the previous valid head, exactly.
                    prop_assert_eq!(store.head().expect("head"), head_before);
                }
            }
        }

        prop_assert_eq!(store.head().expect("head").height.value(), committed);
        drop(store);
        assert_fully_consistent(&path);
    }

    /// A failed append is invisible: the whole raw state is byte-identical afterwards.
    #[test]
    fn a_failed_append_leaves_the_raw_state_untouched(
        existing in 0u64..4,
        which in 0usize..4,
        noise in any::<[u8; 32]>(),
    ) {
        let directory = TempDir::new().expect("temp dir");
        let path = directory.path().join("atomic.prunella");
        let store = LocalChainStore::init_genesis(&path, GenesisSpec::new(network()))
            .expect("create");
        for n in 1..=existing {
            store
                .append_block(next_block(&store, vec![transaction(1, &format!("p{n}"), n)]))
                .expect("append");
        }

        let mut block = next_block(&store, vec![transaction(1, "candidate", 99)]);
        match which {
            0 => block.transactions[0].payload = b"rewritten".to_vec(),
            1 => block.header.previous_hash = Hash::from_bytes(noise),
            2 => block.header.tx_root = Hash::from_bytes(noise),
            _ => block.transactions[0].signature = Signature::from_bytes([0; 64]),
        }
        let candidate_hash = block.hash();
        let candidate_tx: TxId = block.transactions[0].id;

        drop(store);
        let before = raw_state(&path);
        let store = LocalChainStore::open(&path).expect("reopen");

        prop_assert!(store.append_block(block).is_err());

        // Nothing reachable through the API, and nothing in the raw tables either.
        prop_assert_eq!(store.get_block_by_hash(&candidate_hash).expect("read"), None);
        prop_assert_eq!(store.get_transaction(&candidate_tx).expect("read"), None);
        drop(store);

        let after = raw_state(&path);
        prop_assert_eq!(after.blocks, before.blocks);
        prop_assert_eq!(after.hashes, before.hashes);
        prop_assert_eq!(after.transactions, before.transactions);
        prop_assert_eq!(after.head_height, before.head_height);
        prop_assert_eq!(after.head_hash, before.head_hash);
        prop_assert_eq!(after.transaction_count, before.transaction_count);
        assert_fully_consistent(&path);
    }

    /// A batch that fails anywhere commits nothing, however long it is.
    #[test]
    fn a_failing_batch_commits_nothing(good in 0usize..6, tail in 0usize..4) {
        let directory = TempDir::new().expect("temp dir");
        let path = directory.path().join("batch.prunella");
        let store = LocalChainStore::init_genesis(&path, GenesisSpec::new(network()))
            .expect("create");
        let head_before = store.head().expect("head");

        let mut blocks = Vec::new();
        let mut parent = store.get_block(BlockHeight::GENESIS).expect("read").expect("genesis");
        for n in 0..(good + 1 + tail) {
            let n = n as u64;
            let block = parent
                .header
                .child_draft(vec![transaction(1, &format!("b{n}"), n + 1)], n + 1)
                .expect("draft")
                .build()
                .expect("build");
            parent = block.clone();
            blocks.push(block);
        }
        blocks[good].transactions[0].payload = b"tampered".to_vec();

        prop_assert!(store.append_blocks(blocks).is_err());
        prop_assert_eq!(store.head().expect("head"), head_before);
        prop_assert_eq!(store.get_block(BlockHeight(1)).expect("read"), None);
        drop(store);
        assert_fully_consistent(&path);
    }

    /// Reads never panic, whatever is asked for.
    #[test]
    fn reads_are_total(
        height in any::<u64>(),
        hash in any::<[u8; 32]>(),
        id in any::<[u8; 32]>(),
        start in any::<u64>(),
        end in any::<u64>(),
    ) {
        let directory = TempDir::new().expect("temp dir");
        let path = directory.path().join("reads.prunella");
        let store = LocalChainStore::init_genesis(&path, GenesisSpec::new(network()))
            .expect("create");
        store
            .append_block(next_block(&store, vec![transaction(1, "one", 1)]))
            .expect("append");

        let _ = store.get_block(BlockHeight(height));
        let _ = store.get_block_by_hash(&Hash::from_bytes(hash));
        let _ = store.get_transaction(&TxId::from_hash(Hash::from_bytes(id)));
        if let Ok(range) = store.iter_blocks(BlockHeight(start), BlockHeight(end)) {
            // Bound the walk: an enormous range must still be safe to start, and this
            // only consumes what the chain actually holds.
            for block in range.take(8) {
                let _ = block;
            }
        }
        let _ = store.verify_from(BlockHeight(height));
    }

    /// Append order does not change the chain a sequence produces.
    #[test]
    fn the_same_blocks_always_produce_the_same_chain(count in 1u64..8) {
        let left = TempDir::new().expect("temp dir");
        let right = TempDir::new().expect("temp dir");
        let mut heads = Vec::new();
        for directory in [&left, &right] {
            let path = directory.path().join("c.prunella");
            let store = LocalChainStore::init_genesis(&path, GenesisSpec::new(network()))
                .expect("create");
            for n in 1..=count {
                store
                    .append_block(next_block(&store, vec![transaction(1, &format!("p{n}"), n)]))
                    .expect("append");
            }
            heads.push(store.head().expect("head"));
            drop(store);
            assert_fully_consistent(&path);
        }
        prop_assert_eq!(heads[0], heads[1]);
    }
}

#[test]
fn a_store_error_is_never_a_panic_for_a_truncated_file() {
    // A file cut short mid-database is hostile input like any other: it must produce a
    // typed error, not a crash.
    let directory = TempDir::new().expect("temp dir");
    let path = directory.path().join("cut.prunella");
    {
        let store =
            LocalChainStore::init_genesis(&path, GenesisSpec::new(network())).expect("create");
        store
            .append_block(next_block(&store, vec![transaction(1, "one", 1)]))
            .expect("append");
    }
    let whole = std::fs::read(&path).expect("read");
    for fraction in [1usize, 2, 4, 8, 16] {
        let cut = directory.path().join(format!("cut{fraction}.prunella"));
        std::fs::write(&cut, &whole[..whole.len() / fraction]).expect("write");
        // Either it opens and reads, or it reports; never a panic.
        if let Ok(store) = LocalChainStore::open(&cut) {
            let _ = store.head();
            let _ = store.get_block(BlockHeight::GENESIS);
            let _ = store.verify_from(BlockHeight::GENESIS);
        }
    }
}
