//! Persistence, immutability, atomicity and corruption-detection tests.

use prunella_core::{
    Block, BlockHeight, GenesisSpec, Hash, Namespace, NetworkId, SchemaVersion, Transaction,
    TransactionDraft,
};
use prunella_crypto::SigningKey;
use prunella_store::{
    ChainStore, ExistingBlockPolicy, LocalDeterministicPolicy, STORE_FORMAT_VERSION, StoreError,
};
use prunella_verify::{VerifyOptions, verify_chain};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Mirrors the documented on-disk layout so tests can tamper with stored bytes.
const BLOCKS: TableDefinition<'static, u64, &[u8]> = TableDefinition::new("prunella_blocks");

fn network() -> NetworkId {
    NetworkId::new("testnet").expect("valid network id")
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_seed([seed; 32])
}

fn transaction(seed: u8, payload: &str, nonce: u64) -> Transaction {
    let signing_key = key(seed);
    signing_key.sign_transaction(TransactionDraft {
        namespace: Namespace::new("app.demo").expect("valid namespace"),
        schema_version: SchemaVersion(1),
        payload: payload.as_bytes().to_vec(),
        signer: signing_key.public_key(),
        nonce,
    })
}

struct Fixture {
    _directory: TempDir,
    path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = TempDir::new().expect("temp dir");
        let path = directory.path().join("chain.prunella");
        Self {
            _directory: directory,
            path,
        }
    }

    fn create(&self) -> ChainStore {
        ChainStore::create(&self.path, GenesisSpec::new(network())).expect("create chain")
    }

    fn open(&self) -> Result<ChainStore, StoreError> {
        ChainStore::open(&self.path)
    }
}

/// Builds the block that would follow the store's current head.
fn next_block(store: &ChainStore, transactions: Vec<Transaction>) -> Block {
    let head = store.head().expect("head");
    let parent = store
        .block_at(head.height)
        .expect("read")
        .expect("head block");
    parent
        .header
        .child_draft(transactions, parent.header.timestamp_millis + 1)
        .expect("child draft")
        .build()
        .expect("build")
}

fn append(store: &ChainStore, payload: &str, nonce: u64) -> Block {
    let block = next_block(store, vec![transaction(1, payload, nonce)]);
    store.append_block(block.clone()).expect("append");
    block
}

/// Every stored block, read straight out of the table, exactly as it sits on disk.
///
/// Comparing raw database file bytes would be the wrong assertion: redb rewrites its
/// own bookkeeping pages when a file is opened or checkpointed, so byte-identity of the
/// file is not a property it offers. What must not change is the stored chain content,
/// and this reads precisely that.
fn stored_blocks(path: &Path) -> Vec<(u64, Vec<u8>)> {
    let database = Database::open(path).expect("open raw");
    let read = database.begin_read().expect("read txn");
    let blocks = read.open_table(BLOCKS).expect("blocks table");
    blocks
        .iter()
        .expect("iterate")
        .map(|entry| {
            let (height, bytes) = entry.expect("entry");
            (height.value(), bytes.value().to_vec())
        })
        .collect()
}

#[test]
fn a_new_chain_starts_at_its_genesis() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let head = store.head().expect("head");

    assert_eq!(head.height, BlockHeight::GENESIS);
    assert_eq!(head.hash, store.genesis_hash());
    assert_eq!(store.network_id(), &network());
    assert_eq!(store.acceptance_policy(), "local-deterministic");

    let status = store.status().expect("status");
    assert_eq!(status.block_count, 1);
    assert_eq!(status.transaction_count, 0);
    assert_eq!(status.format_version, STORE_FORMAT_VERSION);
}

#[test]
fn creating_a_chain_where_one_exists_is_refused() {
    let fixture = Fixture::new();
    let _store = fixture.create();
    let second = ChainStore::create(&fixture.path, GenesisSpec::new(network()));
    assert!(matches!(second, Err(StoreError::AlreadyExists { .. })));
}

#[test]
fn appended_blocks_survive_a_reopen() {
    let fixture = Fixture::new();
    let expected: Vec<Block>;
    let head_before;
    {
        let store = fixture.create();
        expected = (1..=4)
            .map(|n| append(&store, &format!("payload{n}"), n))
            .collect();
        head_before = store.head().expect("head");
    }

    let store = fixture.open().expect("reopen");
    assert_eq!(store.head().expect("head"), head_before);
    assert_eq!(store.network_id(), &network());
    for block in &expected {
        assert_eq!(
            store.block_at(block.height()).expect("read"),
            Some(block.clone())
        );
        assert_eq!(
            store.block_by_hash(&block.hash()).expect("read"),
            Some(block.clone())
        );
    }
    assert_eq!(store.status().expect("status").transaction_count, 4);
}

#[test]
fn transactions_are_findable_by_id_after_a_reopen() {
    let fixture = Fixture::new();
    let block;
    {
        let store = fixture.create();
        append(&store, "first", 1);
        block = append(&store, "second", 2);
    }

    let store = fixture.open().expect("reopen");
    let wanted = &block.transactions[0];
    let located = store
        .transaction(&wanted.id)
        .expect("lookup")
        .expect("found");
    assert_eq!(located.height, BlockHeight(2));
    assert_eq!(located.index, 0);
    assert_eq!(&located.transaction, wanted);

    let absent = transaction(9, "never committed", 1);
    assert_eq!(store.transaction(&absent.id).expect("lookup"), None);
}

#[test]
fn a_range_iterates_exactly_the_requested_heights() {
    let fixture = Fixture::new();
    let store = fixture.create();
    for n in 1..=5 {
        append(&store, &format!("payload{n}"), n);
    }

    let heights: Vec<u64> = store
        .blocks_in_range(BlockHeight(1), BlockHeight(3))
        .expect("range")
        .map(|block| block.expect("block").height().value())
        .collect();
    assert_eq!(heights, vec![1, 2, 3]);

    let single: Vec<u64> = store
        .blocks_in_range(BlockHeight(2), BlockHeight(2))
        .expect("range")
        .map(|block| block.expect("block").height().value())
        .collect();
    assert_eq!(single, vec![2]);

    let empty = store
        .blocks_in_range(BlockHeight(3), BlockHeight(1))
        .expect("range")
        .count();
    assert_eq!(empty, 0);
}

#[test]
fn a_block_already_committed_cannot_be_appended_again() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let block = append(&store, "payload", 1);

    let error = store
        .append_block(block.clone())
        .expect_err("duplicate block");
    assert!(
        matches!(&error, StoreError::HeightOccupied { height, existing }
            if *height == BlockHeight(1) && *existing == block.hash()),
        "got {error}"
    );
    assert_eq!(store.head().expect("head").height, BlockHeight(1));
}

#[test]
fn a_block_that_skips_a_height_is_refused() {
    let fixture = Fixture::new();
    let store = fixture.create();
    append(&store, "one", 1);
    let skipped = next_block(&store, vec![transaction(1, "two", 2)]);
    let mut far = skipped.clone();
    far.header.height = BlockHeight(7);

    let error = store.append_block(far).expect_err("gap");
    assert!(
        matches!(&error, StoreError::NonContiguous { expected, found }
            if *expected == BlockHeight(2) && *found == BlockHeight(7)),
        "got {error}"
    );
}

#[test]
fn a_block_that_fails_validation_is_refused_and_changes_nothing() {
    let fixture = Fixture::new();
    let store = fixture.create();
    append(&store, "one", 1);
    let before = store.head().expect("head");

    let mut tampered = next_block(&store, vec![transaction(1, "two", 2)]);
    tampered.transactions[0].payload = b"rewritten after signing".to_vec();

    let error = store.append_block(tampered).expect_err("invalid block");
    assert!(matches!(error, StoreError::NotAccepted(_)), "got {error}");
    assert_eq!(store.head().expect("head"), before);
    assert_eq!(store.status().expect("status").transaction_count, 1);
}

#[test]
fn a_block_whose_parent_link_is_wrong_is_refused() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let mut block = next_block(&store, vec![transaction(1, "one", 1)]);
    block.header.previous_hash = Hash::from_bytes([0x33; 32]);

    assert!(matches!(
        store.append_block(block),
        Err(StoreError::NotAccepted(_))
    ));
    assert_eq!(store.head().expect("head").height, BlockHeight::GENESIS);
}

#[test]
fn a_transaction_already_in_the_chain_cannot_be_committed_again() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let replayed = transaction(1, "replayed", 1);
    store
        .append_block(next_block(&store, vec![replayed.clone()]))
        .expect("first append");

    let error = store
        .append_block(next_block(&store, vec![replayed]))
        .expect_err("replayed transaction");
    assert!(matches!(error, StoreError::NotAccepted(_)), "got {error}");
    assert_eq!(store.head().expect("head").height, BlockHeight(1));
}

#[test]
fn a_batch_append_is_all_or_nothing() {
    let fixture = Fixture::new();
    let store = fixture.create();
    append(&store, "committed", 1);
    let head_before = store.head().expect("head");
    let blocks_before = {
        drop(store);
        stored_blocks(&fixture.path)
    };
    let store = fixture.open().expect("reopen");

    // Build five valid blocks, then poison the fourth.
    let mut blocks = Vec::new();
    let mut parent = store
        .block_at(head_before.height)
        .expect("read")
        .expect("block");
    for n in 2..=6u64 {
        let block = parent
            .header
            .child_draft(vec![transaction(1, &format!("batch{n}"), n)], n)
            .expect("draft")
            .build()
            .expect("build");
        parent = block.clone();
        blocks.push(block);
    }
    blocks[3].transactions[0].payload = b"tampered".to_vec();

    let error = store
        .append_blocks(blocks, ExistingBlockPolicy::Reject)
        .expect_err("batch must fail");
    assert!(matches!(error, StoreError::NotAccepted(_)), "got {error}");

    assert_eq!(store.head().expect("head"), head_before);
    assert_eq!(store.block_at(BlockHeight(2)).expect("read"), None);
    assert_eq!(store.status().expect("status").transaction_count, 1);
    drop(store);
    assert_eq!(
        stored_blocks(&fixture.path),
        blocks_before,
        "no block may have been written"
    );
}

#[test]
fn a_batch_append_commits_every_block_when_all_are_valid() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let mut blocks = Vec::new();
    let mut parent = store
        .block_at(BlockHeight::GENESIS)
        .expect("read")
        .expect("genesis");
    for n in 1..=3u64 {
        let block = parent
            .header
            .child_draft(vec![transaction(1, &format!("batch{n}"), n)], n)
            .expect("draft")
            .build()
            .expect("build");
        parent = block.clone();
        blocks.push(block);
    }

    let outcome = store
        .append_blocks(blocks, ExistingBlockPolicy::Reject)
        .expect("append");
    assert_eq!(outcome.appended, 3);
    assert_eq!(outcome.skipped, 0);
    assert_eq!(outcome.head.height, BlockHeight(3));
    assert_eq!(store.status().expect("status").transaction_count, 3);
}

#[test]
fn re_offering_identical_blocks_is_a_no_op_under_the_skip_policy() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let first = append(&store, "one", 1);
    let second = append(&store, "two", 2);
    let head_before = store.head().expect("head");

    let outcome = store
        .append_blocks(vec![first, second], ExistingBlockPolicy::SkipIfIdentical)
        .expect("idempotent reapply");
    assert_eq!(outcome.appended, 0);
    assert_eq!(outcome.skipped, 2);
    assert_eq!(outcome.head, head_before);
    assert_eq!(store.status().expect("status").transaction_count, 2);
}

#[test]
fn a_different_block_at_a_committed_height_is_refused_as_a_fork() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let committed = append(&store, "original", 1);

    let genesis = store
        .block_at(BlockHeight::GENESIS)
        .expect("read")
        .expect("genesis");
    let rival = genesis
        .header
        .child_draft(vec![transaction(1, "rival", 1)], 5)
        .expect("draft")
        .build()
        .expect("build");

    let error = store
        .append_blocks(vec![rival.clone()], ExistingBlockPolicy::SkipIfIdentical)
        .expect_err("fork");
    assert!(
        matches!(&error, StoreError::ForkedHistory { height, existing, offered }
            if *height == BlockHeight(1) && *existing == committed.hash() && *offered == rival.hash()),
        "got {error}"
    );
}

#[test]
fn opening_something_that_is_not_a_chain_is_refused() {
    let directory = TempDir::new().expect("temp dir");
    let missing = directory.path().join("absent.prunella");
    assert!(matches!(
        ChainStore::open(&missing),
        Err(StoreError::NotAChain { .. })
    ));

    let junk = directory.path().join("junk.prunella");
    std::fs::write(&junk, b"this is not a database").expect("write junk");
    assert!(matches!(
        ChainStore::open(&junk),
        Err(StoreError::NotAChain { .. })
    ));
}

#[test]
fn a_corrupted_block_is_detected_and_never_repaired() {
    let fixture = Fixture::new();
    {
        let store = fixture.create();
        append(&store, "one", 1);
        append(&store, "two", 2);
    }

    // Overwrite the stored bytes of block 1 with something undecodable.
    {
        let database = Database::open(&fixture.path).expect("open raw");
        let write = database.begin_write().expect("write txn");
        {
            let mut blocks = write.open_table(BLOCKS).expect("blocks table");
            blocks
                .insert(1u64, b"not a block".as_slice())
                .expect("insert");
        }
        write.commit().expect("commit");
    }
    let corrupted_blocks = stored_blocks(&fixture.path);

    // Opening still succeeds: genesis and head are intact, and the store does not go
    // looking for trouble it was not asked about.
    let store = fixture.open().expect("reopen");
    let error = store.block_at(BlockHeight(1)).expect_err("corrupt block");
    assert!(
        matches!(&error, StoreError::CorruptBlock { height, .. } if *height == BlockHeight(1)),
        "got {error}"
    );

    // Full verification names the damage exactly.
    let report = verify_chain(&store, VerifyOptions::default());
    assert!(!report.is_valid());
    assert_eq!(report.findings[0].location.height, Some(BlockHeight(1)));

    // And the damaged bytes are still exactly as they were: reading and verifying a
    // broken chain never quietly repairs it.
    drop(store);
    assert_eq!(stored_blocks(&fixture.path), corrupted_blocks);
}

#[test]
fn a_head_pointing_at_a_missing_block_refuses_to_open() {
    let fixture = Fixture::new();
    {
        let store = fixture.create();
        append(&store, "one", 1);
    }
    {
        let database = Database::open(&fixture.path).expect("open raw");
        let write = database.begin_write().expect("write txn");
        {
            let mut blocks = write.open_table(BLOCKS).expect("blocks table");
            blocks.remove(1u64).expect("remove");
        }
        write.commit().expect("commit");
    }

    let error = fixture.open().expect_err("inconsistent chain");
    assert!(
        matches!(error, StoreError::Inconsistent { .. }),
        "got {error}"
    );
}

#[test]
fn a_replaced_genesis_block_refuses_to_open() {
    let fixture = Fixture::new();
    {
        let store = fixture.create();
        append(&store, "one", 1);
    }
    {
        let replacement = GenesisSpec {
            network_id: network(),
            timestamp_millis: 12_345,
            transactions: Vec::new(),
        }
        .build()
        .expect("genesis");
        let database = Database::open(&fixture.path).expect("open raw");
        let write = database.begin_write().expect("write txn");
        {
            let mut blocks = write.open_table(BLOCKS).expect("blocks table");
            blocks
                .insert(
                    0u64,
                    prunella_canonical::Canonical::canonical_bytes(&replacement).as_slice(),
                )
                .expect("insert");
        }
        write.commit().expect("commit");
    }

    let error = fixture.open().expect_err("replaced genesis");
    assert!(
        matches!(&error, StoreError::Inconsistent { detail } if detail.contains("genesis")),
        "got {error}"
    );
}

#[test]
fn a_stored_chain_verifies_through_the_block_source_interface() {
    let fixture = Fixture::new();
    let store = fixture.create();
    for n in 1..=6 {
        append(&store, &format!("payload{n}"), n);
    }

    let report = verify_chain(&store, VerifyOptions::default());
    assert!(report.is_valid(), "{report}");
    assert_eq!(report.blocks_checked, 7);
    assert_eq!(report.transactions_checked, 6);
}

#[test]
fn a_custom_acceptance_policy_is_the_only_gate_on_commits() {
    // A policy that refuses everything must be able to stop a block that local
    // validation would happily accept, which is the property a consensus engine needs.
    struct RefuseEverything;
    impl prunella_store::BlockAcceptancePolicy for RefuseEverything {
        fn name(&self) -> &'static str {
            "refuse-everything"
        }

        fn evaluate<'a>(
            &self,
            _context: &prunella_store::AcceptanceContext<'a>,
            _block: &Block,
        ) -> Result<prunella_store::Accepted<'a>, prunella_store::AcceptanceError> {
            Err(prunella_store::AcceptanceError::Declined {
                reason: "this policy accepts nothing".to_owned(),
            })
        }
    }

    let fixture = Fixture::new();
    {
        let store = fixture.create();
        append(&store, "one", 1);
    }
    let store = ChainStore::open_with_policy(&fixture.path, Box::new(RefuseEverything))
        .expect("reopen with policy");
    assert_eq!(store.acceptance_policy(), "refuse-everything");

    let block = next_block(&store, vec![transaction(1, "two", 2)]);
    let error = store.append_block(block).expect_err("policy refuses");
    assert!(
        matches!(&error, StoreError::NotAccepted(inner) if inner.to_string().contains("accepts nothing")),
        "got {error}"
    );
    assert_eq!(store.head().expect("head").height, BlockHeight(1));
}

#[test]
fn the_default_policy_is_reported_as_local_deterministic() {
    let fixture = Fixture::new();
    let store = ChainStore::create_with_policy(
        &fixture.path,
        GenesisSpec::new(network()),
        Box::new(LocalDeterministicPolicy),
    )
    .expect("create");
    assert_eq!(
        store.status().expect("status").acceptance_policy,
        "local-deterministic"
    );
}

#[test]
fn two_instances_given_the_same_genesis_and_blocks_derive_the_same_chain() {
    // The core invariant, end to end through storage.
    let left = Fixture::new();
    let right = Fixture::new();
    let transactions: Vec<Transaction> = (1..=5)
        .map(|n| transaction(1, &format!("payload{n}"), n))
        .collect();

    let mut heads = Vec::new();
    for fixture in [&left, &right] {
        let store = fixture.create();
        for (index, tx) in transactions.iter().enumerate() {
            let head = store.head().expect("head");
            let parent = store.block_at(head.height).expect("read").expect("block");
            let block = parent
                .header
                .child_draft(vec![tx.clone()], index as u64 + 1)
                .expect("draft")
                .build()
                .expect("build");
            store.append_block(block).expect("append");
        }
        heads.push(store.head().expect("head"));
    }

    assert_eq!(heads[0], heads[1]);
    assert_eq!(heads[0].height, BlockHeight(5));
}
