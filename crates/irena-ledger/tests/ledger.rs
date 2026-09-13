//! The company on the chain: founding, amendment, reconstruction at a height, and the
//! boundaries that keep the two engines beneath Irena ignorant of it.

use irena_core::{CompanyIdV1, NotarisationV1, NotaryIdV1, NotaryTimeV1, RecordKindV1};
use irena_ledger::{LedgerError, company_now, genesis_with_company, history, publish, reconstruct};
use prunella_core::{
    BlockHeight, GenesisSpec, Hash, Namespace, NetworkId, SchemaVersion, TransactionDraft, TxId,
};
use prunella_crypto::SigningKey;
use prunella_store::LocalChainStore;
use std::path::PathBuf;
use tempfile::TempDir;

const SHARES_V1: &str = r#"<share-structure>
  <holder id="alice" key="4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c" name="Alice Smith" shares="500"/>
  <holder id="bob" key="7b217b217b217b217b217b217b217b217b217b217b217b217b217b217b217b21" shares="300"/>
  <holder id="carol" shares="200"/>
</share-structure>"#;

/// Bob sells half his shares to Dave, who registers a key.
const SHARES_V2: &str = r#"<share-structure>
  <holder id="alice" key="4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c" name="Alice Smith" shares="500"/>
  <holder id="bob" key="7b217b217b217b217b217b217b217b217b217b217b217b217b217b217b217b21" shares="150"/>
  <holder id="carol" shares="200"/>
  <holder id="dave" key="d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0" shares="150"/>
</share-structure>"#;

const RULES_V1: &str = r#"<voting-rules version="1.0">
  <weight type="electorate"/>
  <exclusions enabled="true"/>
  <quorum type="none"/>
  <threshold type="simple-majority" basis="votes-cast"/>
  <abstentions treatment="exclude"/>
  <tie treatment="reject"/>
</voting-rules>"#;

/// The same rules, amended to a two-thirds supermajority.
const RULES_V2: &str = r#"<voting-rules version="1.0">
  <weight type="electorate"/>
  <exclusions enabled="true"/>
  <quorum type="none"/>
  <threshold type="fraction" numerator="2" denominator="3" basis="votes-cast"/>
  <abstentions treatment="exclude"/>
  <tie treatment="accept"/>
</voting-rules>"#;

const IDENTITY_V2: &str =
    r#"<identity name="Acme Industries plc" jurisdiction="gb" registered-number="01234567"/>"#;

/// The whole company, as founded.
fn genesis_xml() -> String {
    format!(
        r#"<company-genesis>
  <identity name="Acme Industries Ltd" jurisdiction="gb" registered-number="01234567"/>
  <incorporation document-digest="9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f"/>
  {SHARES_V1}
  <governance>
  {RULES_V1}
  </governance>
</company-genesis>"#
    )
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_seed([seed; 32])
}

fn acme() -> CompanyIdV1 {
    CompanyIdV1::new("acme").expect("company")
}

fn notary(at: &str) -> NotarisationV1 {
    NotarisationV1 {
        id: NotaryIdV1::new("notary-07").expect("id"),
        name: "Jane Roe".to_owned(),
        address: Some("12 High Street, London".to_owned()),
        at: NotaryTimeV1::parse(at).expect("time"),
        statement: Some("Filed at Companies House".to_owned()),
        source_digest: Some(Hash::from_bytes([0xc4; 32])),
    }
}

struct Chain {
    _dir: TempDir,
    path: PathBuf,
    store: LocalChainStore,
}

/// A chain whose genesis block founds Acme: identity, register and rules, all at 0.
fn founded() -> Chain {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("acme.chain");
    let spec = genesis_with_company(
        NetworkId::new("acme-net").expect("network"),
        &key(1),
        &acme(),
        &genesis_xml(),
        &notary("2026-01-10T09:00:00Z"),
        0,
    )
    .expect("genesis spec");
    let store = LocalChainStore::init_genesis(&path, spec).expect("create");
    Chain {
        _dir: dir,
        path,
        store,
    }
}

fn amend(
    chain: &Chain,
    kind: RecordKindV1,
    body: &str,
    at: &str,
    ts: u64,
) -> irena_ledger::RecordRefV1 {
    let current = company_now(&chain.store).expect("state").provider_of(kind);
    publish(
        &chain.store,
        &key(1),
        kind,
        body,
        Some(current),
        &notary(at),
        ts,
    )
    .expect("publish")
}

#[test]
fn the_genesis_founds_the_whole_company_at_height_zero() {
    let chain = founded();
    let state = reconstruct(&chain.store, BlockHeight::GENESIS).expect("state");
    assert_eq!(state.company, acme());
    assert_eq!(state.at, BlockHeight::GENESIS);
    assert_eq!(state.genesis_height, BlockHeight::GENESIS);
    assert_eq!(state.identity.value.name, "Acme Industries Ltd");
    assert_eq!(state.shares.value.total_shares(), 1000);
    assert_eq!(
        state.rules.value,
        bornite_xml::read_rules_document(RULES_V1).unwrap()
    );
    // Every part is provided by the genesis transaction, which supersedes nothing.
    for kind in RecordKindV1::ALL {
        assert_eq!(state.provider_of(kind), state.genesis_tx_id, "{kind}");
    }
    assert!(state.identity.supersedes.is_none());
    assert_eq!(state.identity.notarisation, notary("2026-01-10T09:00:00Z"));
    assert_eq!(state.applied.len(), 1);

    // The genesis block is a plain Prunella block: its one transaction is the record.
    let block = chain
        .store
        .get_block(BlockHeight::GENESIS)
        .unwrap()
        .unwrap();
    assert_eq!(block.transactions.len(), 1);
    assert_eq!(block.transactions[0].namespace.as_str(), "irena.company.v1");
    assert_eq!(company_now(&chain.store).unwrap(), state);
}

#[test]
fn the_same_founding_inputs_derive_the_same_genesis_hash() {
    let spec = |at: &str| {
        genesis_with_company(
            NetworkId::new("acme-net").expect("n"),
            &key(1),
            &acme(),
            &genesis_xml(),
            &notary(at),
            0,
        )
        .expect("spec")
        .build()
        .expect("g")
        .hash()
    };
    assert_eq!(spec("2026-01-10T09:00:00Z"), spec("2026-01-10T09:00:00Z"));
    // A different notary time is a different genesis: the notarisation is part of
    // what the chain commits to.
    assert_ne!(spec("2026-01-10T09:00:00Z"), spec("2026-01-10T09:00:01Z"));
}

#[test]
fn the_ledger_stores_bodies_byte_for_byte_and_the_export_shows_them_nested() {
    let chain = founded();
    amend(
        &chain,
        RecordKindV1::ShareStructure,
        SHARES_V2,
        "2026-02-01T10:00:00Z",
        1000,
    );
    for (height, body) in [(0, SHARES_V1), (0, RULES_V1), (1, SHARES_V2)] {
        let block = chain.store.get_block(BlockHeight(height)).unwrap().unwrap();
        let payload = core::str::from_utf8(&block.transactions[0].payload).expect("utf-8");
        assert!(payload.contains(body), "height {height}:\n{payload}");
        assert!(payload.starts_with("<irena-record "), "{payload}");
    }

    // A Prunella export carries every record as readable nested XML, not base64.
    let document =
        prunella_xml::export(&chain.store, &prunella_xml::ExportRequest::full()).expect("export");
    let xml = prunella_xml::write_document(&document).expect("render");
    assert_eq!(
        xml.matches("<payload encoding=\"xml\"><irena-record ")
            .count(),
        2,
        "{xml}"
    );
    assert!(!xml.contains("encoding=\"base64\""), "{xml}");
    assert!(xml.contains("<governance>"), "{xml}");

    // And it comes back from that export to the same company.
    let restored_path = chain.path.with_extension("restored");
    let (restored, _) = prunella_xml::restore(
        &restored_path,
        &prunella_xml::read_document(&xml).expect("parse"),
    )
    .expect("restore");
    assert_eq!(restored.head().unwrap(), chain.store.head().unwrap());
    assert_eq!(
        company_now(&restored).unwrap(),
        company_now(&chain.store).unwrap()
    );
}

#[test]
fn an_amendment_replaces_one_part_and_leaves_the_others() {
    let chain = founded();
    let genesis = company_now(&chain.store).unwrap().genesis_tx_id;
    let amended = amend(
        &chain,
        RecordKindV1::ShareStructure,
        SHARES_V2,
        "2026-02-01T10:00:00Z",
        1000,
    );
    assert_eq!(amended.height, BlockHeight(1));
    assert_eq!(amended.record.supersedes, Some(genesis));

    let state = company_now(&chain.store).unwrap();
    assert_eq!(state.shares.tx_id, amended.tx_id);
    assert_eq!(state.shares.supersedes, Some(genesis));
    assert_eq!(state.shares.value.len(), 4);
    assert_eq!(state.identity.tx_id, genesis, "identity untouched");
    assert_eq!(state.rules.tx_id, genesis, "rules untouched");
    assert_eq!(state.applied.len(), 2);

    // Identity and rules each amend on their own; the register's provider stays.
    let renamed = amend(
        &chain,
        RecordKindV1::Identity,
        IDENTITY_V2,
        "2026-03-01T10:00:00Z",
        2000,
    );
    let stricter = amend(
        &chain,
        RecordKindV1::VotingRules,
        RULES_V2,
        "2026-03-01T10:05:00Z",
        3000,
    );
    let state = company_now(&chain.store).unwrap();
    assert_eq!(state.identity.value.name, "Acme Industries plc");
    assert_eq!(state.identity.tx_id, renamed.tx_id);
    assert_eq!(state.rules.tx_id, stricter.tx_id);
    assert_eq!(state.shares.tx_id, amended.tx_id);
    assert_eq!(renamed.record.supersedes, Some(genesis));
    assert_eq!(stricter.record.supersedes, Some(genesis));
}

#[test]
fn reconstruction_at_a_past_height_is_the_company_of_that_time() {
    let chain = founded();
    let v2 = amend(
        &chain,
        RecordKindV1::ShareStructure,
        SHARES_V2,
        "2026-02-01T10:00:00Z",
        1000,
    );
    amend(
        &chain,
        RecordKindV1::VotingRules,
        RULES_V2,
        "2026-02-01T10:05:00Z",
        2000,
    );
    let then = reconstruct(&chain.store, BlockHeight::GENESIS).unwrap();
    assert_eq!(then.shares.value.len(), 3);
    assert_eq!(
        then.rules.value,
        bornite_xml::read_rules_document(RULES_V1).unwrap()
    );
    let mid = reconstruct(&chain.store, BlockHeight(1)).unwrap();
    assert_eq!(mid.shares.tx_id, v2.tx_id);
    assert_eq!(mid.rules.tx_id, mid.genesis_tx_id);
    let now = reconstruct(&chain.store, BlockHeight(2)).unwrap();
    assert_eq!(
        now.rules.value,
        bornite_xml::read_rules_document(RULES_V2).unwrap()
    );
    // Asking beyond the head reconstructs at the head, and says which height was asked.
    let beyond = reconstruct(&chain.store, BlockHeight(999)).unwrap();
    assert_eq!(beyond.shares, now.shares);
    assert_eq!(beyond.at, BlockHeight(999));
    // History of a part: the genesis, then its amendments.
    let shares = history(&chain.store, RecordKindV1::ShareStructure, BlockHeight(2)).unwrap();
    assert_eq!(
        shares.iter().map(|r| r.height.value()).collect::<Vec<_>>(),
        [0, 1]
    );
    let rules = history(&chain.store, RecordKindV1::VotingRules, BlockHeight(2)).unwrap();
    assert_eq!(
        rules.iter().map(|r| r.height.value()).collect::<Vec<_>>(),
        [0, 2]
    );
    let identity = history(&chain.store, RecordKindV1::Identity, BlockHeight(2)).unwrap();
    assert_eq!(identity.len(), 1);
    assert_eq!(
        history(&chain.store, RecordKindV1::ShareStructure, BlockHeight(0))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_stale_amendment_is_refused_and_the_ledger_is_untouched() {
    let chain = founded();
    let genesis = company_now(&chain.store).unwrap().genesis_tx_id;
    let v2 = amend(
        &chain,
        RecordKindV1::ShareStructure,
        SHARES_V2,
        "2026-02-01T10:00:00Z",
        1000,
    );
    let head = chain.store.head().unwrap();

    // Amending the genesis register again, after v2 exists, is the two-editors problem.
    let error = publish(
        &chain.store,
        &key(1),
        RecordKindV1::ShareStructure,
        SHARES_V1,
        Some(genesis),
        &notary("2026-02-02T10:00:00Z"),
        2000,
    )
    .expect_err("stale");
    assert!(
        matches!(
            &error,
            LedgerError::StaleAmendment {
                kind: RecordKindV1::ShareStructure,
                ..
            }
        ),
        "{error}"
    );
    assert!(error.to_string().contains(&v2.tx_id.to_string()), "{error}");
    assert_eq!(chain.store.head().unwrap(), head, "nothing written");

    // So is superseding nothing.
    let error = publish(
        &chain.store,
        &key(1),
        RecordKindV1::ShareStructure,
        SHARES_V1,
        None,
        &notary("2026-02-02T10:00:00Z"),
        2000,
    )
    .expect_err("stale");
    assert!(matches!(error, LedgerError::StaleAmendment { .. }));

    // And naming the provider of a different part.
    let error = publish(
        &chain.store,
        &key(1),
        RecordKindV1::VotingRules,
        RULES_V2,
        Some(v2.tx_id),
        &notary("2026-02-02T10:00:00Z"),
        2000,
    )
    .expect_err("wrong part");
    assert!(matches!(
        error,
        LedgerError::StaleAmendment {
            kind: RecordKindV1::VotingRules,
            ..
        }
    ));

    // A second genesis cannot be published at all.
    let error = publish(
        &chain.store,
        &key(1),
        RecordKindV1::CompanyGenesis,
        &genesis_xml(),
        None,
        &notary("2026-02-02T10:00:00Z"),
        2000,
    )
    .expect_err("second genesis");
    assert!(matches!(error, LedgerError::Malformed { .. }), "{error}");
    assert_eq!(chain.store.head().unwrap(), head);
}

fn append_raw(store: &LocalChainStore, namespace: &str, payload: Vec<u8>) -> TxId {
    let signer = key(9);
    let head = store.head().unwrap();
    let parent = store.get_block(head.height).unwrap().unwrap();
    let transaction = signer.sign_transaction(TransactionDraft {
        namespace: Namespace::new(namespace).expect("ns"),
        schema_version: SchemaVersion(1),
        payload,
        signer: signer.public_key(),
        nonce: head.height.value() + 1,
    });
    let id = transaction.id;
    let block = parent
        .header
        .child_draft(vec![transaction], 9_999_999)
        .unwrap()
        .build()
        .unwrap();
    store.append_block(block).unwrap();
    id
}

#[test]
fn a_chain_written_around_irena_is_reported_not_repaired() {
    let chain = founded();
    let genesis = company_now(&chain.store).unwrap().genesis_tx_id;

    // 1. A register record that supersedes something that never provided the register.
    let bogus = TxId::from_hash(Hash::from_bytes([0x55; 32]));
    let payload = irena_core::compose_record(
        RecordKindV1::ShareStructure,
        &acme(),
        Some(bogus),
        &notary("2026-02-01T10:00:00Z"),
        SHARES_V2,
    )
    .unwrap();
    let broken_at = append_raw(&chain.store, "irena.shares.v1", payload.into_bytes());
    let error = company_now(&chain.store).expect_err("broken");
    assert!(
        matches!(&error, LedgerError::BrokenAmendmentChain { kind: RecordKindV1::ShareStructure, height: BlockHeight(1), tx_id, .. } if *tx_id == broken_at),
        "{error}"
    );
    assert!(error.to_string().contains(&genesis.to_string()), "{error}");
    // Before the break, reconstruction still works; nothing is repaired.
    assert!(reconstruct(&chain.store, BlockHeight::GENESIS).is_ok());
    assert!(company_now(&chain.store).is_err());
    assert!(
        publish(
            &chain.store,
            &key(1),
            RecordKindV1::Identity,
            IDENTITY_V2,
            Some(genesis),
            &notary("2026-02-02T10:00:00Z"),
            2000
        )
        .is_err(),
        "nothing can be published on a broken chain"
    );
}

#[test]
fn a_second_genesis_a_foreign_company_and_an_unreadable_record_are_each_named() {
    let chain = founded();
    let genesis = company_now(&chain.store).unwrap().genesis_tx_id;

    let second = irena_core::compose_record(
        RecordKindV1::CompanyGenesis,
        &acme(),
        None,
        &notary("2026-02-01T10:00:00Z"),
        &genesis_xml(),
    )
    .unwrap();
    let second_tx = append_raw(&chain.store, "irena.company.v1", second.into_bytes());
    let error = company_now(&chain.store).expect_err("second genesis");
    assert!(
        matches!(&error, LedgerError::SecondGenesis { first, tx_id, .. } if *first == genesis && *tx_id == second_tx),
        "{error}"
    );

    let beta = founded();
    let foreign = irena_core::compose_record(
        RecordKindV1::Identity,
        &CompanyIdV1::new("beta").unwrap(),
        Some(
            beta.store
                .head()
                .unwrap()
                .hash
                .to_hex()
                .parse()
                .unwrap_or(genesis),
        ),
        &notary("2026-02-01T10:00:00Z"),
        IDENTITY_V2,
    )
    .unwrap();
    append_raw(&beta.store, "irena.company.v1", foreign.into_bytes());
    let error = company_now(&beta.store).expect_err("foreign");
    assert!(
        matches!(&error, LedgerError::ForeignCompany { found, .. } if found == "beta"),
        "{error}"
    );

    let gamma = founded();
    append_raw(&gamma.store, "irena.rules.v1", b"not a record".to_vec());
    let error = company_now(&gamma.store).expect_err("unreadable");
    assert!(
        matches!(
            &error,
            LedgerError::UnreadableRecord {
                height: BlockHeight(1),
                ..
            }
        ),
        "{error}"
    );

    // A readable record of the wrong kind for its namespace is just as wrong.
    let delta = founded();
    let misfiled = irena_core::compose_record(
        RecordKindV1::ShareStructure,
        &acme(),
        Some(genesis),
        &notary("2026-02-01T10:00:00Z"),
        SHARES_V2,
    )
    .unwrap();
    append_raw(&delta.store, "irena.company.v1", misfiled.into_bytes());
    assert!(matches!(
        company_now(&delta.store),
        Err(LedgerError::UnreadableRecord { .. })
    ));

    // Transactions in other namespaces are not Irena's business.
    let epsilon = founded();
    append_raw(&epsilon.store, "app.other", b"whatever".to_vec());
    assert!(company_now(&epsilon.store).is_ok());
}

#[test]
fn an_amendment_before_any_genesis_is_refused_and_a_late_founding_is_allowed() {
    let dir = TempDir::new().expect("dir");
    let store = LocalChainStore::init_genesis(
        dir.path().join("plain.chain"),
        GenesisSpec::new(NetworkId::new("plain").expect("n")),
    )
    .expect("create");
    assert!(matches!(
        company_now(&store),
        Err(LedgerError::NoCompany { .. })
    ));
    assert!(matches!(
        publish(
            &store,
            &key(1),
            RecordKindV1::Identity,
            IDENTITY_V2,
            None,
            &notary("2026-02-01T10:00:00Z"),
            1000
        ),
        Err(LedgerError::NoCompany { .. })
    ));

    // An amendment written before any genesis is not a company.
    let early = irena_core::compose_record(
        RecordKindV1::Identity,
        &acme(),
        None,
        &notary("2026-02-01T10:00:00Z"),
        IDENTITY_V2,
    )
    .unwrap();
    append_raw(&store, "irena.company.v1", early.into_bytes());
    assert!(matches!(
        company_now(&store),
        Err(LedgerError::NoGenesisFirst {
            height: BlockHeight(1),
            ..
        })
    ));

    // But a company may be founded later on an existing chain: the genesis need not be
    // in block 0, only first among Irena's records.
    let dir = TempDir::new().expect("dir");
    let store = LocalChainStore::init_genesis(
        dir.path().join("late.chain"),
        GenesisSpec::new(NetworkId::new("late").expect("n")),
    )
    .expect("create");
    append_raw(&store, "app.other", b"before the company".to_vec());
    let founding = irena_core::compose_record(
        RecordKindV1::CompanyGenesis,
        &acme(),
        None,
        &notary("2026-02-01T10:00:00Z"),
        &genesis_xml(),
    )
    .unwrap();
    let founding_tx = append_raw(&store, "irena.company.v1", founding.into_bytes());
    let state = company_now(&store).unwrap();
    assert_eq!(state.genesis_height, BlockHeight(2));
    assert_eq!(state.genesis_tx_id, founding_tx);
    assert!(matches!(
        reconstruct(&store, BlockHeight(1)),
        Err(LedgerError::NoCompany { at: BlockHeight(1) })
    ));
}

#[test]
fn reconstruction_is_identical_across_independently_built_chains() {
    let build = |dir: &TempDir| {
        let spec = genesis_with_company(
            NetworkId::new("acme-net").expect("n"),
            &key(1),
            &acme(),
            &genesis_xml(),
            &notary("2026-01-10T09:00:00Z"),
            0,
        )
        .expect("spec");
        let store =
            LocalChainStore::init_genesis(dir.path().join("acme.chain"), spec).expect("create");
        let genesis = company_now(&store).unwrap().genesis_tx_id;
        publish(
            &store,
            &key(1),
            RecordKindV1::ShareStructure,
            SHARES_V2,
            Some(genesis),
            &notary("2026-02-01T10:00:00Z"),
            1000,
        )
        .unwrap();
        store
    };
    let (a, b) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let (store_a, store_b) = (build(&a), build(&b));
    assert_eq!(store_a.head().unwrap(), store_b.head().unwrap());
    assert_eq!(
        company_now(&store_a).unwrap(),
        company_now(&store_b).unwrap()
    );
}

#[test]
fn founding_and_publishing_need_valid_bodies() {
    let error = genesis_with_company(
        NetworkId::new("acme-net").expect("n"),
        &key(1),
        &acme(),
        "<company-genesis/>",
        &notary("2026-01-10T09:00:00Z"),
        0,
    )
    .expect_err("empty genesis");
    assert!(matches!(error, LedgerError::Record(_)), "{error}");

    let chain = founded();
    let head = chain.store.head().unwrap();
    let genesis = company_now(&chain.store).unwrap().genesis_tx_id;
    let error = publish(
        &chain.store,
        &key(1),
        RecordKindV1::ShareStructure,
        "<share-structure><holder id=\"x\" shares=\"0\"/></share-structure>",
        Some(genesis),
        &notary("2026-02-01T10:00:00Z"),
        1000,
    )
    .expect_err("zero shares");
    assert!(matches!(error, LedgerError::Record(_)), "{error}");
    assert_eq!(chain.store.head().unwrap(), head);
}

// ---------------------------------------------------------------------------------
// Boundaries.
// ---------------------------------------------------------------------------------

/// Neither engine mentions the other, and neither mentions Irena: the company layer
/// imports both, and nothing points back up.
#[test]
fn the_engines_know_nothing_of_each_other_or_of_irena() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut leaks = Vec::new();
    for entry in std::fs::read_dir(&crates).expect("crates") {
        let path = entry.expect("entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_owned();
        let forbidden: &[&str] = if name.starts_with("bornite-") {
            &["prunella", "irena"]
        } else if name.starts_with("prunella-") {
            &["bornite", "irena"]
        } else {
            continue;
        };
        for file in walk(&path) {
            let text = std::fs::read_to_string(&file).expect("read").to_lowercase();
            for word in forbidden {
                if text.contains(word) {
                    leaks.push(format!("{} mentions {word}", file.display()));
                }
            }
        }
    }
    assert!(leaks.is_empty(), "{leaks:?}");
}

fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) != Some("target") {
                files.extend(walk(&path));
            }
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs" | "toml")
        ) {
            files.push(path);
        }
    }
    files
}
