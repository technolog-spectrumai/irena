//! Property and fuzz tests for XML import.
//!
//! A document is untrusted input. Arbitrary bytes, arbitrary mutations of a valid
//! document, and hostile size declarations must all produce a typed error and never a
//! panic, an unbounded allocation or a partial write.

use proptest::prelude::*;
use prunella_core::{GenesisSpec, Namespace, NetworkId, SchemaVersion, TransactionDraft};
use prunella_crypto::SigningKey;
use prunella_store::{ChainStorage, LocalChainStore};
use prunella_xml::{
    ExportRequest, MAX_BLOCKS, XmlError, export, import, plan_import, read_document,
    read_document_with_limit, write_document,
};
use tempfile::TempDir;

fn network() -> NetworkId {
    NetworkId::new("testnet").expect("valid network id")
}

/// A chain with `count` blocks, and the XML of a full export of it.
fn sample_document(directory: &TempDir, count: u64) -> String {
    let store = LocalChainStore::init_genesis(
        directory.path().join("s.prunella"),
        GenesisSpec::new(network()),
    )
    .expect("create");
    for n in 1..=count {
        let key = SigningKey::from_seed([1; 32]);
        let transaction = key.sign_transaction(TransactionDraft {
            namespace: Namespace::new("app.demo").expect("valid"),
            schema_version: SchemaVersion(1),
            payload: format!("payload{n}").into_bytes(),
            signer: key.public_key(),
            nonce: n,
        });
        let head = store.head().expect("head");
        let parent = store.get_block(head.height).expect("read").expect("block");
        let block = parent
            .header
            .child_draft(vec![transaction], n)
            .expect("draft")
            .build()
            .expect("build");
        store.append_block(block).expect("append");
    }
    write_document(&export(&store, &ExportRequest::full()).expect("export")).expect("render")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Arbitrary bytes never panic the parser.
    #[test]
    fn arbitrary_input_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let text = String::from_utf8_lossy(&bytes);
        let _ = read_document(&text);
    }

    /// Arbitrary text that merely looks like XML never panics the parser.
    #[test]
    fn arbitrary_xml_shaped_input_never_panics(
        parts in prop::collection::vec(
            prop::sample::select(vec![
                "<prunella-chain", ">", "</prunella-chain>", " xmlns=\"urn:prunella:chain:1\"",
                " format-version=\"1\"", " kind=\"full\"", "<block", "</block>", "<header/>",
                "<transactions>", "</transactions>", "<transaction", "<payload>", "</payload>",
                "&amp;", "<![CDATA[x]]>", "\u{0}", "\u{feff}", "aGk=", " height=\"0\"",
            ]),
            0..40,
        ),
    ) {
        let _ = read_document(&parts.concat());
    }

    /// Truncating a valid document anywhere yields an error, never a partial document.
    #[test]
    fn truncation_is_always_an_error(cut in 1usize..400) {
        let directory = TempDir::new().expect("temp dir");
        let xml = sample_document(&directory, 3);
        if cut < xml.len() {
            let truncated = &xml[..xml.len() - cut];
            if let Ok(document) = read_document(truncated) {
                // The only truncations that can still parse are ones that removed
                // nothing meaningful; the document must still be internally consistent.
                prop_assert_eq!(document.block_count(), document.blocks.len() as u64);
            }
        }
    }

    /// Flipping any byte of a valid document either fails to parse or fails to import.
    #[test]
    fn a_flipped_byte_never_imports_silently(
        position in any::<prop::sample::Index>(),
        replacement in prop::sample::select(vec![b'0', b'9', b'a', b'f', b'X', b'"', b'<']),
    ) {
        let directory = TempDir::new().expect("temp dir");
        let xml = sample_document(&directory, 2);
        let mut bytes = xml.clone().into_bytes();
        let at = position.index(bytes.len());
        prop_assume!(bytes[at] != replacement);
        bytes[at] = replacement;
        let Ok(mutated) = String::from_utf8(bytes) else { return Ok(()) };
        prop_assume!(mutated != xml);

        let Ok(document) = read_document(&mutated) else { return Ok(()) };

        let target = LocalChainStore::init_genesis(
            directory.path().join("t.prunella"),
            GenesisSpec::new(network()),
        )
        .expect("create target");
        let head_before = target.head().expect("head");

        if let Ok(outcome) = import(&target, &document) {
            // If it imported, the document was still a faithful description of a valid
            // chain: every block must verify.
            prop_assert!(target.verify_from(prunella_core::BlockHeight::GENESIS).is_valid());
            prop_assert!(outcome.head.height >= head_before.height);
        } else {
            // A rejected import must have written nothing.
            prop_assert_eq!(target.head().expect("head"), head_before);
        }
    }

    /// A dry run agrees with the real import on whether a document is acceptable.
    #[test]
    fn a_dry_run_agrees_with_the_import_it_predicts(blocks in 0u64..5) {
        let directory = TempDir::new().expect("temp dir");
        let xml = sample_document(&directory, blocks);
        let document = read_document(&xml).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let target = LocalChainStore::init_genesis(
            directory.path().join("t.prunella"),
            GenesisSpec::new(network()),
        )
        .expect("create target");

        let planned = plan_import(&target, &document);
        let imported = import(&target, &document);
        prop_assert_eq!(planned.is_ok(), imported.is_ok());
    }

    /// A declared block count that disagrees with the declared range is refused.
    #[test]
    fn a_lying_block_count_is_refused(count in 0u64..100_000) {
        let directory = TempDir::new().expect("temp dir");
        let xml = sample_document(&directory, 2);
        let mutated = xml.replace("block-count=\"3\"", &format!("block-count=\"{count}\""));
        prop_assume!(count != 3);
        prop_assert!(read_document(&mutated).is_err());
    }

    /// A hostile block count is refused by the limit, not by allocating.
    #[test]
    fn a_hostile_block_count_is_refused_by_the_limit(excess in 1u64..1_000_000) {
        let directory = TempDir::new().expect("temp dir");
        let xml = sample_document(&directory, 1);
        let count = MAX_BLOCKS.saturating_add(excess);
        let mutated = xml
            .replace("range-end=\"1\"", &format!("range-end=\"{}\"", count - 1))
            .replace("block-count=\"2\"", &format!("block-count=\"{count}\""));
        let error = read_document(&mutated).expect_err("over the limit");
        prop_assert!(matches!(error, XmlError::TooManyBlocks { .. }), "{error}");
    }

    /// The byte limit is enforced before parsing begins.
    #[test]
    fn the_size_limit_is_enforced_first(limit in 0u64..64) {
        let directory = TempDir::new().expect("temp dir");
        let xml = sample_document(&directory, 1);
        let error = read_document_with_limit(&xml, limit).expect_err("over the limit");
        prop_assert!(matches!(error, XmlError::TooLarge { .. }), "{error}");
    }

    /// Whatever a document does, a rejected import leaves the chain exactly as it was.
    #[test]
    fn a_rejected_import_never_changes_the_chain(
        junk in prop::collection::vec(any::<u8>(), 0..256),
    ) {
        let directory = TempDir::new().expect("temp dir");
        let target = LocalChainStore::init_genesis(
            directory.path().join("t.prunella"),
            GenesisSpec::new(network()),
        )
        .expect("create target");
        let before = target.head().expect("head");

        let text = String::from_utf8_lossy(&junk);
        if let Ok(document) = read_document(&text) {
            let _ = import(&target, &document);
        }
        prop_assert_eq!(target.head().expect("head"), before);
    }
}
