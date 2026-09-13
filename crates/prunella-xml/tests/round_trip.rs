//! Export, import, round-trip fidelity, rejection and atomicity tests.

use prunella_core::{
    Block, BlockHeight, GenesisSpec, Namespace, NetworkId, SchemaVersion, Transaction,
    TransactionDraft,
};
use prunella_crypto::SigningKey;
use prunella_store::{ChainStore, StoreError};
use prunella_xml::{
    ChainDocument, DocumentKind, ExportRequest, XmlError, export, import, plan_import,
    read_document, read_document_with_limit, restore, write_document,
};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const BLOCKS: TableDefinition<'static, u64, &[u8]> = TableDefinition::new("prunella_blocks");

fn network() -> NetworkId {
    NetworkId::new("testnet").expect("valid network id")
}

fn transaction(seed: u8, namespace: &str, payload: &[u8], nonce: u64) -> Transaction {
    let key = SigningKey::from_seed([seed; 32]);
    key.sign_transaction(TransactionDraft {
        namespace: Namespace::new(namespace).expect("valid namespace"),
        schema_version: SchemaVersion(1),
        payload: payload.to_vec(),
        signer: key.public_key(),
        nonce,
    })
}

struct Workspace {
    directory: TempDir,
    counter: std::cell::Cell<u32>,
}

impl Workspace {
    fn new() -> Self {
        Self {
            directory: TempDir::new().expect("temp dir"),
            counter: std::cell::Cell::new(0),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }

    fn fresh(&self) -> PathBuf {
        let n = self.counter.get();
        self.counter.set(n + 1);
        self.path(&format!("chain{n}.prunella"))
    }

    /// A chain with genesis plus `count` blocks, one transaction each.
    fn chain(&self, count: u64) -> (PathBuf, ChainStore) {
        let path = self.fresh();
        let store = ChainStore::create(&path, GenesisSpec::new(network())).expect("create");
        for n in 1..=count {
            append(
                &store,
                vec![transaction(
                    1,
                    "app.demo",
                    format!("payload{n}").as_bytes(),
                    n,
                )],
            );
        }
        (path, store)
    }
}

fn append(store: &ChainStore, transactions: Vec<Transaction>) -> Block {
    let head = store.head().expect("head");
    let parent = store.block_at(head.height).expect("read").expect("block");
    let block = parent
        .header
        .child_draft(transactions, parent.header.timestamp_millis + 1)
        .expect("draft")
        .build()
        .expect("build");
    store.append_block(block.clone()).expect("append");
    block
}

fn export_xml(store: &ChainStore, request: &ExportRequest) -> String {
    write_document(&export(store, request).expect("export")).expect("render")
}

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
fn a_full_export_reimports_into_an_identical_chain() {
    let workspace = Workspace::new();
    let (_source_path, source) = workspace.chain(5);
    let xml = export_xml(
        &source,
        &ExportRequest::full().exported_at(1_700_000_000_000),
    );

    let document = read_document(&xml).expect("parse");
    assert_eq!(document.kind, DocumentKind::Full);
    assert_eq!(document.block_count(), 6);
    assert_eq!(document.network_id, network());
    assert_eq!(document.genesis_hash, source.genesis_hash());
    assert_eq!(document.exported_at_millis, 1_700_000_000_000);

    let restored_path = workspace.path("restored.prunella");
    let (restored, outcome) = restore(&restored_path, &document).expect("restore");
    assert_eq!(outcome.appended, 5);
    assert_eq!(
        outcome.skipped, 1,
        "genesis is created, then recognised as already present"
    );

    assert_eq!(restored.genesis_hash(), source.genesis_hash());
    assert_eq!(restored.head().expect("head"), source.head().expect("head"));
    for height in 0..=5 {
        let height = BlockHeight(height);
        assert_eq!(
            restored.block_at(height).expect("read"),
            source.block_at(height).expect("read"),
            "block {height} differs"
        );
    }
}

#[test]
fn payload_bytes_survive_a_round_trip_exactly() {
    let workspace = Workspace::new();
    let path = workspace.fresh();
    let store = ChainStore::create(&path, GenesisSpec::new(network())).expect("create");

    let payloads: Vec<Vec<u8>> = vec![
        Vec::new(),
        vec![0x00],
        vec![0xff, 0xfe, 0x00, 0x80, 0x7f],
        (0..=255u8).collect(),
        b"<payload>&amp; \"quotes\" \n\t\r</payload>".to_vec(),
        vec![0xab; 100_000],
    ];
    for (index, payload) in payloads.iter().enumerate() {
        append(
            &store,
            vec![transaction(1, "app.demo", payload, index as u64 + 1)],
        );
    }

    let xml = export_xml(&store, &ExportRequest::full());
    let document = read_document(&xml).expect("parse");
    let restored_path = workspace.path("payloads.prunella");
    let (restored, _) = restore(&restored_path, &document).expect("restore");

    for (index, payload) in payloads.iter().enumerate() {
        let height = BlockHeight(index as u64 + 1);
        let block = restored.block_at(height).expect("read").expect("block");
        assert_eq!(
            &block.transactions[0].payload, payload,
            "payload at height {height}"
        );
    }
}

#[test]
fn transaction_order_is_preserved() {
    let workspace = Workspace::new();
    let path = workspace.fresh();
    let store = ChainStore::create(&path, GenesisSpec::new(network())).expect("create");
    let ordered: Vec<Transaction> = (1..=6)
        .map(|n| transaction(1, "app.demo", format!("tx{n}").as_bytes(), n))
        .collect();
    append(&store, ordered.clone());

    let document = read_document(&export_xml(&store, &ExportRequest::full())).expect("parse");
    let restored_path = workspace.path("ordered.prunella");
    let (restored, _) = restore(&restored_path, &document).expect("restore");
    let block = restored
        .block_at(BlockHeight(1))
        .expect("read")
        .expect("block");

    assert_eq!(block.transactions, ordered);
    assert_eq!(
        block.hash(),
        store
            .block_at(BlockHeight(1))
            .expect("read")
            .expect("b")
            .hash()
    );
}

#[test]
fn a_document_round_trips_through_text_unchanged() {
    let workspace = Workspace::new();
    let (_, store) = workspace.chain(3);
    let document = export(&store, &ExportRequest::full().exported_at(42)).expect("export");
    let xml = write_document(&document).expect("render");
    assert_eq!(read_document(&xml).expect("parse"), document);
    assert_eq!(
        write_document(&read_document(&xml).expect("parse")).expect("render"),
        xml
    );
}

#[test]
fn a_range_export_imports_incrementally() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(5);

    let full_head = source.head().expect("head");
    let first = read_document(&export_xml(
        &source,
        &ExportRequest::range(BlockHeight(0), BlockHeight(2)),
    ))
    .expect("parse");
    assert_eq!(
        first.kind,
        DocumentKind::Range,
        "a partial range is not a backup"
    );
    let second = read_document(&export_xml(
        &source,
        &ExportRequest::range(BlockHeight(3), BlockHeight(5)),
    ))
    .expect("parse");

    let target_path = workspace.fresh();
    let target = ChainStore::create(&target_path, GenesisSpec::new(network())).expect("create");
    let outcome = import(&target, &first).expect("first range");
    assert_eq!(outcome.appended, 2);
    assert_eq!(outcome.skipped, 1);

    let outcome = import(&target, &second).expect("second range");
    assert_eq!(outcome.appended, 3);
    assert_eq!(target.head().expect("head"), full_head);
}

#[test]
fn importing_ranges_out_of_order_is_refused_as_a_gap() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(5);
    let later = read_document(&export_xml(
        &source,
        &ExportRequest::range(BlockHeight(3), BlockHeight(5)),
    ))
    .expect("parse");

    let target_path = workspace.fresh();
    let target = ChainStore::create(&target_path, GenesisSpec::new(network())).expect("create");

    let error = import(&target, &later).expect_err("gap");
    assert!(
        matches!(&error, XmlError::Gap { head, expected, found }
            if head.value() == 0 && expected.value() == 1 && found.value() == 3),
        "got {error}"
    );
    assert_eq!(target.head().expect("head").height, BlockHeight::GENESIS);
}

#[test]
fn importing_the_same_document_twice_is_a_no_op() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(4);
    let document = read_document(&export_xml(&source, &ExportRequest::full())).expect("parse");

    let target_path = workspace.fresh();
    let target = ChainStore::create(&target_path, GenesisSpec::new(network())).expect("create");

    let first = import(&target, &document).expect("first import");
    assert_eq!(first.appended, 4);
    let head_after_first = target.head().expect("head");

    let second = import(&target, &document).expect("second import");
    assert_eq!(second.appended, 0);
    assert_eq!(second.skipped, 5);
    assert_eq!(target.head().expect("head"), head_after_first);
}

#[test]
fn a_dry_run_reports_the_same_outcome_and_writes_nothing() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(4);
    let document = read_document(&export_xml(&source, &ExportRequest::full())).expect("parse");

    let target_path = workspace.fresh();
    let target = ChainStore::create(&target_path, GenesisSpec::new(network())).expect("create");
    let before = {
        drop(target);
        stored_blocks(&target_path)
    };
    let target = ChainStore::open(&target_path).expect("reopen");

    let plan = plan_import(&target, &document).expect("dry run");
    assert_eq!(plan.blocks_in_document, 5);
    assert_eq!(plan.blocks_already_present, 1);
    assert_eq!(plan.blocks_to_append, 4);
    assert_eq!(plan.resulting_head, source.head().expect("head"));

    assert_eq!(target.head().expect("head").height, BlockHeight::GENESIS);
    drop(target);
    assert_eq!(
        stored_blocks(&target_path),
        before,
        "a dry run must not write"
    );
}

#[test]
fn a_dry_run_refuses_exactly_what_a_real_import_refuses() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(3);
    let mut xml = export_xml(&source, &ExportRequest::full());
    xml = tamper_first_payload(&xml);
    let document = read_document(&xml).expect("parse");

    let target_path = workspace.fresh();
    let target = ChainStore::create(&target_path, GenesisSpec::new(network())).expect("create");

    let dry = plan_import(&target, &document).expect_err("dry run rejects");
    let real = import(&target, &document).expect_err("import rejects");
    assert!(matches!(dry, XmlError::InvalidBlocks { .. }), "got {dry}");
    assert!(matches!(real, XmlError::InvalidBlocks { .. }), "got {real}");
}

/// Replaces the first payload element's content, leaving every declared hash intact.
fn tamper_first_payload(xml: &str) -> String {
    let start = xml.find("<payload>").expect("a payload element");
    let end = xml[start..].find("</payload>").expect("its close") + start;
    let mut tampered = String::with_capacity(xml.len());
    tampered.push_str(&xml[..start]);
    tampered.push_str("<payload>dGFtcGVyZWQ=</payload>");
    tampered.push_str(&xml[end + "</payload>".len()..]);
    tampered
}

#[test]
fn a_tampered_payload_is_caught_even_though_the_block_hash_still_matches() {
    // Rewriting a payload does not change the header, so the declared block hash still
    // agrees. The transaction id derivation and the signature are what catch it, which
    // is why the importer applies the full rule set rather than trusting hashes alone.
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(2);
    let document = read_document(&tamper_first_payload(&export_xml(
        &source,
        &ExportRequest::full(),
    )))
    .expect("parse");

    let target_path = workspace.fresh();
    let target = ChainStore::create(&target_path, GenesisSpec::new(network())).expect("create");
    let error = import(&target, &document).expect_err("tampering");
    let XmlError::InvalidBlocks { findings } = error else {
        panic!("expected invalid blocks, got {error}");
    };
    let kinds: Vec<_> = findings.iter().map(|finding| finding.kind).collect();
    assert!(
        kinds.contains(&prunella_verify::FindingKind::TxIdMismatch),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&prunella_verify::FindingKind::SignatureInvalid),
        "{kinds:?}"
    );
}

#[test]
fn a_tampered_header_is_caught_as_a_declared_hash_mismatch() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(2);
    let xml = export_xml(&source, &ExportRequest::full())
        .replace("timestamp-millis=\"2\"", "timestamp-millis=\"9999\"");
    let document = read_document(&xml).expect("parse");

    let target_path = workspace.fresh();
    let target = ChainStore::create(&target_path, GenesisSpec::new(network())).expect("create");
    let error = import(&target, &document).expect_err("tampering");
    assert!(
        matches!(error, XmlError::DeclaredHashMismatch { .. }),
        "got {error}"
    );
}

#[test]
fn a_document_from_another_network_is_refused() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(2);
    let document = read_document(&export_xml(&source, &ExportRequest::full())).expect("parse");

    let other_path = workspace.fresh();
    let other = ChainStore::create(
        &other_path,
        GenesisSpec::new(NetworkId::new("othernet").expect("valid")),
    )
    .expect("create");

    let error = import(&other, &document).expect_err("wrong network");
    assert!(
        matches!(error, XmlError::NetworkMismatch { .. }),
        "got {error}"
    );
}

#[test]
fn a_document_from_a_chain_with_a_different_genesis_is_refused() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(2);
    let document = read_document(&export_xml(&source, &ExportRequest::full())).expect("parse");

    let other_path = workspace.fresh();
    let other = ChainStore::create(
        &other_path,
        GenesisSpec {
            network_id: network(),
            timestamp_millis: 777,
            transactions: Vec::new(),
        },
    )
    .expect("create");

    let error = import(&other, &document).expect_err("wrong genesis");
    assert!(
        matches!(error, XmlError::GenesisMismatch { .. }),
        "got {error}"
    );
}

#[test]
fn a_document_that_contradicts_committed_history_is_refused_as_a_fork() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(2);
    let document = read_document(&export_xml(&source, &ExportRequest::full())).expect("parse");

    let rival_path = workspace.fresh();
    let rival = ChainStore::create(&rival_path, GenesisSpec::new(network())).expect("create");
    append(
        &rival,
        vec![transaction(1, "app.demo", b"a different history", 1)],
    );

    let error = import(&rival, &document).expect_err("fork");
    assert!(
        matches!(error, XmlError::Fork { height, .. } if height == BlockHeight(1)),
        "got {error}"
    );
    assert_eq!(rival.head().expect("head").height, BlockHeight(1));
}

#[test]
fn a_failed_import_leaves_the_chain_untouched() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(5);
    let xml = export_xml(
        &source,
        &ExportRequest::range(BlockHeight(1), BlockHeight(5)),
    );
    // Poison the fourth block in the document.
    let marker = "<payload>cGF5bG9hZDQ=</payload>";
    assert!(
        xml.contains(marker),
        "expected the fourth payload in the export"
    );
    let xml = xml.replace(marker, "<payload>dGFtcGVyZWQ=</payload>");
    let document = read_document(&xml).expect("parse");

    let target_path = workspace.fresh();
    let target = ChainStore::create(&target_path, GenesisSpec::new(network())).expect("create");
    let before = {
        drop(target);
        stored_blocks(&target_path)
    };
    let target = ChainStore::open(&target_path).expect("reopen");

    let error = import(&target, &document).expect_err("poisoned document");
    assert!(
        matches!(error, XmlError::InvalidBlocks { .. }),
        "got {error}"
    );

    assert_eq!(target.head().expect("head").height, BlockHeight::GENESIS);
    assert_eq!(target.block_at(BlockHeight(1)).expect("read"), None);
    drop(target);
    assert_eq!(
        stored_blocks(&target_path),
        before,
        "no block may have been written"
    );
}

#[test]
fn a_namespace_export_is_marked_as_a_projection_and_cannot_be_imported() {
    let workspace = Workspace::new();
    let path = workspace.fresh();
    let store = ChainStore::create(&path, GenesisSpec::new(network())).expect("create");
    append(
        &store,
        vec![
            transaction(1, "app.alpha", b"alpha one", 1),
            transaction(1, "app.beta", b"beta one", 2),
            transaction(1, "app.alpha", b"alpha two", 3),
        ],
    );
    append(&store, vec![transaction(1, "app.beta", b"beta two", 4)]);

    let alpha = Namespace::new("app.alpha").expect("valid namespace");
    let xml = export_xml(&store, &ExportRequest::full().filtered_to(alpha.clone()));
    assert!(xml.contains("kind=\"projection\""), "{xml}");
    assert!(xml.contains("filter-namespace=\"app.alpha\""), "{xml}");

    let document = read_document(&xml).expect("parse");
    assert_eq!(document.kind, DocumentKind::Projection);
    assert!(!document.is_importable());
    assert_eq!(
        document
            .projection
            .as_ref()
            .expect("projection")
            .filter_namespace,
        alpha
    );

    // Heights stay contiguous: a block with nothing in the namespace is still present,
    // so a reader never mistakes a filtered-out block for a missing one.
    assert_eq!(document.block_count(), 3);
    assert_eq!(document.blocks[1].block.transactions.len(), 2);
    assert!(document.blocks[2].block.transactions.is_empty());
    // The header still records the block's true transaction count.
    assert_eq!(document.blocks[1].block.header.tx_count, 3);

    let target_path = workspace.fresh();
    let target = ChainStore::create(&target_path, GenesisSpec::new(network())).expect("create");
    let error = import(&target, &document).expect_err("projections are not backups");
    assert!(
        matches!(error, XmlError::NotImportable { .. }),
        "got {error}"
    );

    let error = restore(workspace.path("from-projection.prunella"), &document)
        .expect_err("projections cannot found a chain");
    assert!(
        matches!(error, XmlError::NotRestorable { .. }),
        "got {error}"
    );
}

#[test]
fn only_a_full_document_can_create_a_chain() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(4);
    let document = read_document(&export_xml(
        &source,
        &ExportRequest::range(BlockHeight(1), BlockHeight(3)),
    ))
    .expect("parse");

    let error = restore(workspace.path("from-range.prunella"), &document)
        .expect_err("range is not a backup");
    assert!(
        matches!(error, XmlError::NotRestorable { .. }),
        "got {error}"
    );
}

#[test]
fn restoring_over_an_existing_chain_is_refused() {
    let workspace = Workspace::new();
    let (path, source) = workspace.chain(2);
    let document = read_document(&export_xml(&source, &ExportRequest::full())).expect("parse");

    let error = restore(&path, &document).expect_err("path is occupied");
    assert!(
        matches!(error, XmlError::Store(StoreError::AlreadyExists { .. })),
        "got {error}"
    );
}

#[test]
fn malformed_documents_are_refused_with_a_reason() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(2);
    let good = export_xml(&source, &ExportRequest::full());

    let cases: Vec<(&str, String)> = vec![
        ("not xml at all", "<<<".to_owned()),
        ("truncated", good[..good.len() / 2].to_owned()),
        (
            "wrong namespace",
            good.replace("urn:prunella:chain:1", "urn:something:else"),
        ),
        (
            "unknown root attribute",
            good.replace("kind=\"full\"", "kind=\"full\" surprise=\"1\""),
        ),
        (
            "unknown element",
            good.replace("</prunella-chain>", "  <mystery/>\n</prunella-chain>"),
        ),
        ("missing attribute", good.replace(" tx-count=\"0\"", "")),
        (
            "uppercase hash",
            good.replacen("genesis-hash=\"", "genesis-hash=\"AB", 1),
        ),
        (
            "bad base64",
            good.replace("<payload>", "<payload>!!!not base64!!!"),
        ),
        (
            "non-numeric height",
            good.replace("height=\"1\"", "height=\"one\""),
        ),
        (
            "transaction index out of order",
            good.replace("<transaction index=\"0\"", "<transaction index=\"3\""),
        ),
    ];

    for (label, xml) in cases {
        let result = read_document(&xml);
        assert!(result.is_err(), "{label} should have been refused");
    }
}

#[test]
fn a_document_with_a_height_gap_is_refused_at_parse_time() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(3);
    let good = export_xml(&source, &ExportRequest::full());
    // Renumber block 2 as block 7, leaving a hole.
    let broken = good.replacen("<block height=\"2\"", "<block height=\"7\"", 1);

    let error = read_document(&broken).expect_err("gap inside the document");
    assert!(
        matches!(error, XmlError::NonContiguousDocument { .. }),
        "got {error}"
    );
}

#[test]
fn an_unknown_format_version_is_refused() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(1);
    let xml = export_xml(&source, &ExportRequest::full())
        .replace("format-version=\"1\"", "format-version=\"99\"");

    let error = read_document(&xml).expect_err("unknown version");
    assert!(
        matches!(
            error,
            XmlError::UnsupportedFormatVersion {
                found: 99,
                supported: 1
            }
        ),
        "got {error}"
    );
}

#[test]
fn a_document_over_the_size_limit_is_refused_before_parsing() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(1);
    let xml = export_xml(&source, &ExportRequest::full());

    let error = read_document_with_limit(&xml, 16).expect_err("over the limit");
    assert!(
        matches!(error, XmlError::TooLarge { limit: 16, .. }),
        "got {error}"
    );
}

#[test]
fn exporting_an_empty_range_is_refused() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(1);
    let error = export(
        &source,
        &ExportRequest::range(BlockHeight(5), BlockHeight(9)),
    )
    .expect_err("nothing to export");
    // The end is clamped to the head before the range is judged, so the error reports
    // the range that was actually asked of the chain.
    assert!(
        matches!(error, XmlError::EmptyRange { from, to }
            if from == BlockHeight(5) && to == BlockHeight(1)),
        "got {error}"
    );
}

#[test]
fn exports_validate_against_the_published_schema() {
    // Skips rather than fails where xmllint is unavailable: the schema is the contract
    // for other tooling, and the Rust parser enforces the same structure in code.
    if std::process::Command::new("xmllint")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping: xmllint is not installed");
        return;
    }

    let workspace = Workspace::new();
    let path = workspace.fresh();
    let store = ChainStore::create(&path, GenesisSpec::new(network())).expect("create");
    append(&store, vec![transaction(1, "app.alpha", b"", 1)]);
    append(
        &store,
        vec![
            transaction(1, "app.alpha", &[0xff, 0x00], 2),
            transaction(2, "app.beta", b"another", 3),
        ],
    );

    let schema = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/prunella-chain-v1.xsd")
        .canonicalize()
        .expect("schema path");

    let documents = [
        ("full", export_xml(&store, &ExportRequest::full())),
        (
            "range",
            export_xml(
                &store,
                &ExportRequest::range(BlockHeight(1), BlockHeight(2)),
            ),
        ),
        (
            "projection",
            export_xml(
                &store,
                &ExportRequest::full()
                    .filtered_to(Namespace::new("app.alpha").expect("valid namespace")),
            ),
        ),
    ];

    for (label, xml) in documents {
        let file = workspace.path(&format!("{label}.xml"));
        std::fs::write(&file, &xml).expect("write document");
        let output = std::process::Command::new("xmllint")
            .arg("--noout")
            .arg("--schema")
            .arg(&schema)
            .arg(&file)
            .output()
            .expect("run xmllint");
        assert!(
            output.status.success(),
            "{label} export failed schema validation:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn the_declared_document_range_must_match_the_blocks_present() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(3);
    let xml =
        export_xml(&source, &ExportRequest::full()).replace("range-end=\"3\"", "range-end=\"9\"");

    let error = read_document(&xml).expect_err("range does not match contents");
    assert!(matches!(error, XmlError::Malformed { .. }), "got {error}");
}

#[test]
fn a_projection_element_may_not_appear_on_a_full_document() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(1);
    let xml = export_xml(&source, &ExportRequest::full()).replace(
        "  <block",
        "  <projection filter-namespace=\"app.demo\"/>\n  <block",
    );

    let error = read_document(&xml).expect_err("projection element on a full document");
    assert!(matches!(error, XmlError::Malformed { .. }), "got {error}");
}

#[test]
fn a_projection_document_must_name_its_filter() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(1);
    let xml =
        export_xml(&source, &ExportRequest::full()).replace("kind=\"full\"", "kind=\"projection\"");

    let error = read_document(&xml).expect_err("projection without a filter");
    assert!(matches!(error, XmlError::Malformed { .. }), "got {error}");
}

#[test]
fn the_document_is_a_faithful_rendering_a_human_can_read() {
    let workspace = Workspace::new();
    let (_, source) = workspace.chain(1);
    let document = ChainDocument {
        ..export(&source, &ExportRequest::full()).expect("export")
    };
    let xml = write_document(&document).expect("render");

    assert!(
        xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
        "{xml}"
    );
    assert!(xml.contains("xmlns=\"urn:prunella:chain:1\""), "{xml}");
    assert!(xml.contains("network-id=\"testnet\""), "{xml}");
    assert!(
        xml.contains(&format!("genesis-hash=\"{}\"", source.genesis_hash())),
        "{xml}"
    );
    assert!(xml.contains("<header version=\"1\""), "{xml}");
}
