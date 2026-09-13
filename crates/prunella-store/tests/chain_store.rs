//! Persistence, immutability, atomicity and corruption-detection tests.

use prunella_core::{
    Block, BlockHeight, GenesisSpec, Hash, Namespace, NetworkId, SchemaVersion, Signature,
    Transaction, TransactionDraft,
};
use prunella_crypto::SigningKey;
use prunella_store::{
    AppendStatus, ChainStorage, LocalChainStore, LocalDeterministicPolicy, STORE_FORMAT_VERSION,
    StoreError,
};
use prunella_verify::{FindingKind, VerifyOptions, verify_chain};
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

    fn create(&self) -> LocalChainStore {
        LocalChainStore::init_genesis(&self.path, GenesisSpec::new(network()))
            .expect("create chain")
    }

    fn open(&self) -> Result<LocalChainStore, StoreError> {
        LocalChainStore::open(&self.path)
    }
}

/// Builds the block that would follow the store's current head.
fn next_block(store: &LocalChainStore, transactions: Vec<Transaction>) -> Block {
    let head = store.head().expect("head");
    let parent = store
        .get_block(head.height)
        .expect("read")
        .expect("head block");
    parent
        .header
        .child_draft(transactions, parent.header.timestamp_millis + 1)
        .expect("child draft")
        .build()
        .expect("build")
}

fn append(store: &LocalChainStore, payload: &str, nonce: u64) -> Block {
    let block = next_block(store, vec![transaction(1, payload, nonce)]);
    let outcome = store.append_block(block.clone()).expect("append");
    assert_eq!(outcome.status, AppendStatus::Committed);
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

/// Overwrites the stored bytes at a height, leaving every index entry pointing at it.
fn overwrite_stored_block(path: &Path, height: u64, bytes: &[u8]) {
    let database = Database::open(path).expect("open raw");
    let write = database.begin_write().expect("write txn");
    {
        let mut blocks = write.open_table(BLOCKS).expect("blocks table");
        blocks.insert(height, bytes).expect("insert");
    }
    write.commit().expect("commit");
}

// ---------------------------------------------------------------- genesis creation

#[test]
fn genesis_is_created_once_and_is_the_head() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let head = store.head().expect("head");

    assert_eq!(head.height, BlockHeight::GENESIS);
    assert_eq!(head.hash, store.genesis_hash());
    assert_eq!(store.network_id(), &network());
    assert_eq!(store.acceptance_policy(), "local-deterministic");

    let genesis = store
        .get_block(BlockHeight::GENESIS)
        .expect("read")
        .expect("genesis");
    assert!(genesis.header.previous_hash.is_zero());
    assert_eq!(genesis.hash(), store.genesis_hash());

    let status = store.status().expect("status");
    assert_eq!(status.block_count, 1);
    assert_eq!(status.transaction_count, 0);
    assert_eq!(status.format_version, STORE_FORMAT_VERSION);
}

#[test]
fn genesis_is_never_written_twice() {
    let fixture = Fixture::new();
    let first = fixture.create();
    let genesis = first.genesis_hash();

    // A second init on the same path must not replace the chain's root.
    let second = LocalChainStore::init_genesis(&fixture.path, GenesisSpec::new(network()));
    assert!(matches!(second, Err(StoreError::AlreadyExists { .. })));

    // Nor does reopening create one.
    drop(first);
    let reopened = fixture.open().expect("reopen");
    assert_eq!(reopened.genesis_hash(), genesis);
    assert_eq!(reopened.head().expect("head").height, BlockHeight::GENESIS);
}

#[test]
fn the_same_genesis_specification_always_derives_the_same_chain_root() {
    let left = Fixture::new();
    let right = Fixture::new();
    assert_eq!(left.create().genesis_hash(), right.create().genesis_hash());
}

// ------------------------------------------------------------- append and reopen

#[test]
fn appended_blocks_survive_a_clean_close_and_reopen() {
    let fixture = Fixture::new();
    let expected: Vec<Block>;
    let head_before;
    {
        let store = fixture.create();
        expected = (1..=4)
            .map(|n| append(&store, &format!("payload{n}"), n))
            .collect();
        head_before = store.head().expect("head");
        store.close().expect("clean close");
    }

    let store = fixture.open().expect("reopen");
    assert_eq!(store.head().expect("head"), head_before);
    assert_eq!(store.network_id(), &network());
    for block in &expected {
        assert_eq!(
            store.get_block(block.height()).expect("read"),
            Some(block.clone())
        );
        assert_eq!(
            store.get_block_by_hash(&block.hash()).expect("read"),
            Some(block.clone())
        );
    }
    assert_eq!(store.status().expect("status").transaction_count, 4);
    assert!(store.verify_from(BlockHeight::GENESIS).is_valid());
}

#[test]
fn a_chain_reopens_correctly_when_it_was_dropped_rather_than_closed() {
    // Every commit is durable by the time it returns, so an abrupt drop loses nothing
    // that was already reported as appended.
    let fixture = Fixture::new();
    let head_before;
    {
        let store = fixture.create();
        append(&store, "one", 1);
        append(&store, "two", 2);
        head_before = store.head().expect("head");
        // No close(): just let it fall out of scope.
    }

    let store = fixture.open().expect("reopen");
    assert_eq!(store.head().expect("head"), head_before);
    assert!(store.verify_from(BlockHeight::GENESIS).is_valid());
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
        .get_transaction(&wanted.id)
        .expect("lookup")
        .expect("found");
    assert_eq!(located.height, BlockHeight(2));
    assert_eq!(located.index, 0);
    assert_eq!(&located.transaction, wanted);

    let absent = transaction(9, "never committed", 1);
    assert_eq!(store.get_transaction(&absent.id).expect("lookup"), None);
}

#[test]
fn the_hash_index_finds_every_block() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let mut hashes = vec![store.genesis_hash()];
    for n in 1..=3 {
        hashes.push(append(&store, &format!("payload{n}"), n).hash());
    }

    for (height, hash) in hashes.iter().enumerate() {
        let found = store.get_block_by_hash(hash).expect("read").expect("block");
        assert_eq!(found.height(), BlockHeight(height as u64));
    }
    assert_eq!(
        store
            .get_block_by_hash(&Hash::from_bytes([0x99; 32]))
            .expect("read"),
        None
    );
}

// ------------------------------------------------------------------- iteration

#[test]
fn iteration_runs_from_genesis_to_head() {
    let fixture = Fixture::new();
    let store = fixture.create();
    for n in 1..=5 {
        append(&store, &format!("payload{n}"), n);
    }
    let head = store.head().expect("head");

    let walked: Vec<Block> = store
        .iter_blocks(BlockHeight::GENESIS, head.height)
        .expect("range")
        .map(|block| block.expect("block"))
        .collect();

    assert_eq!(walked.len(), 6);
    assert_eq!(walked[0].hash(), store.genesis_hash());
    assert_eq!(walked[5].hash(), head.hash);
    // Every block links to the one before it, in order.
    for pair in walked.windows(2) {
        assert_eq!(pair[1].header.previous_hash, pair[0].hash());
        assert_eq!(pair[1].height().value(), pair[0].height().value() + 1);
    }
}

#[test]
fn iteration_covers_sub_ranges_and_empty_ranges() {
    let fixture = Fixture::new();
    let store = fixture.create();
    for n in 1..=5 {
        append(&store, &format!("payload{n}"), n);
    }

    let heights = |from, to| -> Vec<u64> {
        store
            .iter_blocks(BlockHeight(from), BlockHeight(to))
            .expect("range")
            .map(|block| block.expect("block").height().value())
            .collect()
    };

    assert_eq!(heights(1, 3), vec![1, 2, 3]);
    assert_eq!(heights(2, 2), vec![2]);
    assert_eq!(heights(3, 1), Vec::<u64>::new());
}

// -------------------------------------------------------------- rejection rules

#[test]
fn a_block_with_a_wrong_previous_hash_is_rejected() {
    let fixture = Fixture::new();
    let store = fixture.create();
    append(&store, "one", 1);
    let before = store.head().expect("head");

    let mut block = next_block(&store, vec![transaction(1, "two", 2)]);
    block.header.previous_hash = Hash::from_bytes([0x33; 32]);

    let error = store.append_block(block).expect_err("wrong parent");
    assert_not_accepted(&error, FindingKind::PreviousHashMismatch);
    assert_eq!(store.head().expect("head"), before);
}

#[test]
fn a_block_at_a_wrong_height_is_rejected() {
    let fixture = Fixture::new();
    let store = fixture.create();
    append(&store, "one", 1);

    let mut far = next_block(&store, vec![transaction(1, "two", 2)]);
    far.header.height = BlockHeight(7);

    let error = store.append_block(far).expect_err("gap");
    assert!(
        matches!(&error, StoreError::NonContiguous { expected, found }
            if *expected == BlockHeight(2) && *found == BlockHeight(7)),
        "got {error}"
    );
    assert_eq!(store.head().expect("head").height, BlockHeight(1));
}

#[test]
fn a_block_from_another_network_is_rejected() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let mut block = next_block(&store, vec![transaction(1, "one", 1)]);
    block.header.network_id = NetworkId::new("othernet").expect("valid");

    let error = store.append_block(block).expect_err("wrong network");
    assert_not_accepted(&error, FindingKind::NetworkMismatch);
    assert_eq!(store.head().expect("head").height, BlockHeight::GENESIS);
}

#[test]
fn a_block_with_an_invalid_signature_is_rejected() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let mut block = next_block(&store, vec![transaction(1, "one", 1)]);

    // Replace the signature and re-derive the id and root so that only the signature
    // rule can fail. Nothing else about the block is wrong.
    block.transactions[0].signature = Signature::from_bytes([0u8; 64]);
    block.transactions[0].id = block.transactions[0].compute_id();
    block.header.tx_root = Transaction::compute_root(&block.transactions).expect("root");

    let error = store.append_block(block).expect_err("bad signature");
    assert_not_accepted(&error, FindingKind::SignatureInvalid);
    assert_eq!(store.head().expect("head").height, BlockHeight::GENESIS);
}

#[test]
fn a_block_with_an_incorrect_transaction_root_is_rejected() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let mut block = next_block(&store, vec![transaction(1, "one", 1)]);
    block.header.tx_root = Hash::from_bytes([0x11; 32]);

    let error = store.append_block(block).expect_err("bad root");
    assert_not_accepted(&error, FindingKind::TxRootMismatch);
    assert_eq!(store.head().expect("head").height, BlockHeight::GENESIS);
}

#[test]
fn a_block_whose_transaction_count_disagrees_with_its_contents_is_rejected() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let mut block = next_block(&store, vec![transaction(1, "one", 1)]);
    block.header.tx_count = 9;

    let error = store.append_block(block).expect_err("bad count");
    assert_not_accepted(&error, FindingKind::TxCountMismatch);
}

#[test]
fn a_block_with_a_tampered_payload_is_rejected() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let mut block = next_block(&store, vec![transaction(1, "one", 1)]);
    block.transactions[0].payload = b"rewritten after signing".to_vec();

    let error = store.append_block(block).expect_err("tampered payload");
    assert_not_accepted(&error, FindingKind::SignatureInvalid);
    assert_not_accepted(&error, FindingKind::TxIdMismatch);
}

#[test]
fn a_block_carrying_the_same_transaction_twice_is_rejected() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let repeated = transaction(1, "repeated", 1);
    let block = next_block(&store, vec![repeated.clone(), repeated]);

    let error = store
        .append_block(block)
        .expect_err("duplicate inside a block");
    assert_not_accepted(&error, FindingKind::DuplicateTxIdInBlock);
    assert_eq!(store.head().expect("head").height, BlockHeight::GENESIS);
}

#[test]
fn a_transaction_already_committed_cannot_be_committed_again() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let replayed = transaction(1, "replayed", 1);
    store
        .append_block(next_block(&store, vec![replayed.clone()]))
        .expect("first append");

    let error = store
        .append_block(next_block(&store, vec![replayed]))
        .expect_err("replayed transaction");
    assert_not_accepted(&error, FindingKind::DuplicateTxIdInChain);
    assert_eq!(store.head().expect("head").height, BlockHeight(1));
}

#[test]
fn an_unsupported_header_version_is_rejected() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let mut block = next_block(&store, vec![transaction(1, "one", 1)]);
    block.header.version = 99;

    let error = store
        .append_block(block)
        .expect_err("unknown header version");
    assert_not_accepted(&error, FindingKind::UnsupportedHeaderVersion);
}

/// Asserts that the store refused the block, and that the named rule is why.
fn assert_not_accepted(error: &StoreError, expected: FindingKind) {
    let StoreError::NotAccepted(prunella_store::AcceptanceError::Invalid { findings }) = error
    else {
        panic!("expected a validation refusal, got {error}");
    };
    assert!(
        findings.iter().any(|finding| finding.kind == expected),
        "expected {expected}, got {:?}",
        findings
            .iter()
            .map(|finding| finding.kind)
            .collect::<Vec<_>>()
    );
}

// -------------------------------------------------- repeated and conflicting appends

#[test]
fn appending_an_identical_block_again_reports_already_present() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let block = append(&store, "payload", 1);
    let head_after_first = store.head().expect("head");
    let blocks_after_first = {
        drop(store);
        stored_blocks(&fixture.path)
    };
    let store = fixture.open().expect("reopen");

    let outcome = store
        .append_block(block.clone())
        .expect("identical re-append");
    assert_eq!(outcome.status, AppendStatus::AlreadyPresent);
    assert!(!outcome.status.committed());
    assert_eq!(outcome.head, head_after_first);

    // Repeating it any number of times stays a no-op.
    for _ in 0..3 {
        assert_eq!(
            store.append_block(block.clone()).expect("re-append").status,
            AppendStatus::AlreadyPresent
        );
    }

    assert_eq!(store.head().expect("head"), head_after_first);
    assert_eq!(store.status().expect("status").transaction_count, 1);
    drop(store);
    assert_eq!(
        stored_blocks(&fixture.path),
        blocks_after_first,
        "nothing may be rewritten"
    );
}

#[test]
fn a_conflicting_block_at_a_committed_height_is_rejected() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let committed = append(&store, "original", 1);

    let genesis = store
        .get_block(BlockHeight::GENESIS)
        .expect("read")
        .expect("genesis");
    let rival = genesis
        .header
        .child_draft(vec![transaction(1, "rival", 1)], 5)
        .expect("draft")
        .build()
        .expect("build");
    assert_ne!(rival.hash(), committed.hash());

    let error = store.append_block(rival.clone()).expect_err("fork");
    assert!(
        matches!(&error, StoreError::ForkedHistory { height, existing, offered }
            if *height == BlockHeight(1)
                && *existing == committed.hash()
                && *offered == rival.hash()),
        "got {error}"
    );

    // The committed block is untouched, and still the one the chain holds.
    assert_eq!(
        store.get_block(BlockHeight(1)).expect("read"),
        Some(committed)
    );
}

// ------------------------------------------------------- atomicity and durability

#[test]
fn a_failed_append_leaves_the_old_head_and_every_index_intact() {
    let fixture = Fixture::new();
    let store = fixture.create();
    append(&store, "committed", 1);
    let head_before = store.head().expect("head");
    let good = next_block(&store, vec![transaction(1, "two", 2)]);

    let mut poisoned = good.clone();
    poisoned.transactions[0].payload = b"tampered".to_vec();

    let error = store.append_block(poisoned).expect_err("invalid block");
    assert!(matches!(error, StoreError::NotAccepted(_)), "got {error}");

    // Head, blocks and both indexes are exactly as they were.
    assert_eq!(store.head().expect("head"), head_before);
    assert_eq!(store.get_block(BlockHeight(2)).expect("read"), None);
    assert_eq!(store.get_block_by_hash(&good.hash()).expect("read"), None);
    assert_eq!(
        store
            .get_transaction(&good.transactions[0].id)
            .expect("read"),
        None
    );
    assert_eq!(store.status().expect("status").transaction_count, 1);

    // And the chain still accepts the block that was always valid.
    assert_eq!(
        store.append_block(good).expect("append").status,
        AppendStatus::Committed
    );
}

#[test]
fn an_interrupted_batch_writes_no_block_at_all() {
    let fixture = Fixture::new();
    let store = fixture.create();
    append(&store, "committed", 1);
    let head_before = store.head().expect("head");
    let blocks_before = {
        drop(store);
        stored_blocks(&fixture.path)
    };
    let store = fixture.open().expect("reopen");

    // Five valid blocks, with the fourth poisoned.
    let mut blocks = Vec::new();
    let mut parent = store
        .get_block(head_before.height)
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

    let error = store.append_blocks(blocks).expect_err("batch must fail");
    assert!(matches!(error, StoreError::NotAccepted(_)), "got {error}");

    // Not even the three blocks that preceded the bad one were written.
    assert_eq!(store.head().expect("head"), head_before);
    assert_eq!(store.get_block(BlockHeight(2)).expect("read"), None);
    assert_eq!(store.status().expect("status").transaction_count, 1);
    drop(store);
    assert_eq!(
        stored_blocks(&fixture.path),
        blocks_before,
        "no block may have been written"
    );
}

#[test]
fn a_batch_commits_every_block_when_all_are_valid() {
    let fixture = Fixture::new();
    let store = fixture.create();
    let mut blocks = Vec::new();
    let mut parent = store
        .get_block(BlockHeight::GENESIS)
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

    let outcome = store.append_blocks(blocks.clone()).expect("append");
    assert_eq!(outcome.appended, 3);
    assert_eq!(outcome.already_present, 0);
    assert_eq!(outcome.head.height, BlockHeight(3));

    // Re-applying the same run is a no-op, which is what makes a retried transfer safe.
    let again = store.append_blocks(blocks).expect("re-apply");
    assert_eq!(again.appended, 0);
    assert_eq!(again.already_present, 3);
    assert_eq!(again.head, outcome.head);
}

// --------------------------------------------------- corruption, never repaired

#[test]
fn a_block_that_no_longer_matches_its_indexed_hash_is_detected() {
    // The hash index still points at height 1, but the bytes there now hash to
    // something else. Nothing is rebuilt to make the two agree again.
    let fixture = Fixture::new();
    let original;
    {
        let store = fixture.create();
        original = append(&store, "one", 1);
        append(&store, "two", 2);
    }

    let substitute = {
        let store = fixture.open().expect("reopen");
        let genesis = store
            .get_block(BlockHeight::GENESIS)
            .expect("read")
            .expect("genesis");
        genesis
            .header
            .child_draft(vec![transaction(1, "substituted", 1)], 1)
            .expect("draft")
            .build()
            .expect("build")
    };
    assert_ne!(substitute.hash(), original.hash());
    overwrite_stored_block(
        &fixture.path,
        1,
        &prunella_canonical::Canonical::canonical_bytes(&substitute),
    );

    let store = fixture.open().expect("reopen");
    let report = store.verify_from(BlockHeight::GENESIS);
    assert!(!report.is_valid());
    let kinds: Vec<_> = report.findings.iter().map(|finding| finding.kind).collect();
    assert!(
        kinds.contains(&FindingKind::PreviousHashMismatch),
        "{kinds:?}"
    );

    // Looking the original hash up still routes to height 1, and the block there is
    // reported as it is rather than as it was indexed.
    let found = store
        .get_block_by_hash(&original.hash())
        .expect("read")
        .expect("block");
    assert_eq!(found, substitute);
}

#[test]
fn a_corrupted_block_is_detected_and_never_repaired() {
    let fixture = Fixture::new();
    {
        let store = fixture.create();
        append(&store, "one", 1);
        append(&store, "two", 2);
    }
    overwrite_stored_block(&fixture.path, 1, b"not a block");
    let corrupted_blocks = stored_blocks(&fixture.path);

    // Opening still succeeds: genesis and head are intact, and the store does not go
    // looking for trouble it was not asked about.
    let store = fixture.open().expect("reopen");
    let error = store.get_block(BlockHeight(1)).expect_err("corrupt block");
    assert!(
        matches!(&error, StoreError::CorruptBlock { height, .. } if *height == BlockHeight(1)),
        "got {error}"
    );

    let report = store.verify_from(BlockHeight::GENESIS);
    assert!(!report.is_valid());
    assert_eq!(report.findings[0].location.height, Some(BlockHeight(1)));

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
    let replacement = GenesisSpec {
        network_id: network(),
        timestamp_millis: 12_345,
        transactions: Vec::new(),
    }
    .build()
    .expect("genesis");
    overwrite_stored_block(
        &fixture.path,
        0,
        &prunella_canonical::Canonical::canonical_bytes(&replacement),
    );

    let error = fixture.open().expect_err("replaced genesis");
    assert!(
        matches!(&error, StoreError::Inconsistent { detail } if detail.contains("genesis")),
        "got {error}"
    );
}

#[test]
fn opening_something_that_is_not_a_chain_is_refused() {
    let directory = TempDir::new().expect("temp dir");
    let missing = directory.path().join("absent.prunella");
    assert!(matches!(
        LocalChainStore::open(&missing),
        Err(StoreError::NotAChain { .. })
    ));

    let junk = directory.path().join("junk.prunella");
    std::fs::write(&junk, b"this is not a database").expect("write junk");
    assert!(matches!(
        LocalChainStore::open(&junk),
        Err(StoreError::NotAChain { .. })
    ));
}

// ------------------------------------------------------------------ verification

#[test]
fn verify_from_walks_a_sound_chain_clean() {
    let fixture = Fixture::new();
    let store = fixture.create();
    for n in 1..=6 {
        append(&store, &format!("payload{n}"), n);
    }

    let report = store.verify_from(BlockHeight::GENESIS);
    assert!(report.is_valid(), "{report}");
    assert_eq!(report.blocks_checked, 7);
    assert_eq!(report.transactions_checked, 6);

    // Starting part-way through still checks genesis identity and loads the parent it
    // needs, so a partial verification is never a verification of nothing.
    let partial = store.verify_from(BlockHeight(4));
    assert!(partial.is_valid(), "{partial}");
    assert_eq!(partial.blocks_checked, 3);
    assert_eq!(partial.range_start, Some(BlockHeight(4)));
}

#[test]
fn verify_from_reports_damage_with_its_exact_location() {
    let fixture = Fixture::new();
    {
        let store = fixture.create();
        for n in 1..=3 {
            append(&store, &format!("payload{n}"), n);
        }
    }
    overwrite_stored_block(&fixture.path, 2, b"not a block");

    let store = fixture.open().expect("reopen");
    let report = store.verify_from(BlockHeight::GENESIS);
    assert!(!report.is_valid());
    assert_eq!(report.findings[0].kind, FindingKind::DecodeError);
    assert_eq!(report.findings[0].location.height, Some(BlockHeight(2)));
}

#[test]
fn a_stored_chain_verifies_through_the_block_source_interface() {
    let fixture = Fixture::new();
    let store = fixture.create();
    for n in 1..=3 {
        append(&store, &format!("payload{n}"), n);
    }
    assert!(verify_chain(&store, VerifyOptions::default()).is_valid());
}

// ------------------------------------------------------- the storage abstraction

/// Exercises a store only through [`ChainStorage`], with no knowledge of redb.
fn drive_through_the_trait<S: ChainStorage>(store: &S, expected_blocks: u64)
where
    S::Error: core::fmt::Display,
{
    let head = store.head().expect("head");
    assert_eq!(head.height.value() + 1, expected_blocks);

    let genesis = store
        .get_block(BlockHeight::GENESIS)
        .unwrap_or_else(|error| panic!("read: {error}"))
        .expect("genesis");
    assert_eq!(genesis.hash(), store.genesis_hash());

    let by_hash = store
        .get_block_by_hash(&head.hash)
        .unwrap_or_else(|error| panic!("read: {error}"))
        .expect("head block");
    assert_eq!(by_hash.hash(), head.hash);

    let walked = store
        .iter_blocks(BlockHeight::GENESIS, head.height)
        .unwrap_or_else(|error| panic!("range: {error}"))
        .count();
    assert_eq!(walked as u64, expected_blocks);

    assert!(store.verify_from(BlockHeight::GENESIS).is_valid());
}

#[test]
fn the_trait_is_enough_to_use_a_chain() {
    let fixture = Fixture::new();
    let store = fixture.create();
    for n in 1..=3 {
        append(&store, &format!("payload{n}"), n);
    }
    drive_through_the_trait(&store, 4);
}

#[test]
fn the_trait_can_create_and_extend_a_chain() {
    let directory = TempDir::new().expect("temp dir");
    let path = directory.path().join("via-trait.prunella");

    let store = <LocalChainStore as ChainStorage>::init_genesis(path, GenesisSpec::new(network()))
        .expect("init genesis");

    let block = next_block(&store, vec![transaction(1, "through the trait", 1)]);
    let outcome = ChainStorage::append_block(&store, block.clone()).expect("append");
    assert_eq!(outcome.status, AppendStatus::Committed);

    let repeat = ChainStorage::append_block(&store, block).expect("re-append");
    assert_eq!(repeat.status, AppendStatus::AlreadyPresent);

    drive_through_the_trait(&store, 2);
}

// ------------------------------------------------------------ acceptance boundary

#[test]
fn the_acceptance_policy_is_the_only_gate_on_commits() {
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
    let store = LocalChainStore::open_with_policy(&fixture.path, Box::new(RefuseEverything))
        .expect("reopen with policy");
    assert_eq!(store.acceptance_policy(), "refuse-everything");

    let block = next_block(&store, vec![transaction(1, "two", 2)]);
    let error = store.append_block(block).expect_err("policy refuses");
    assert!(
        matches!(&error, StoreError::NotAccepted(inner)
            if inner.to_string().contains("accepts nothing")),
        "got {error}"
    );
    assert_eq!(store.head().expect("head").height, BlockHeight(1));
}

#[test]
fn the_default_policy_is_reported_as_local_deterministic() {
    let fixture = Fixture::new();
    let store = LocalChainStore::init_genesis_with_policy(
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

// --------------------------------------------------------------- core invariant

#[test]
fn two_instances_given_the_same_genesis_and_blocks_derive_the_same_chain() {
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
            let parent = store.get_block(head.height).expect("read").expect("block");
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
