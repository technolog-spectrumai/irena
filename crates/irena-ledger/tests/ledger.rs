//! The company on the chain: founding, amendment, resolution at a height, and the
//! boundaries that keep the two engines beneath Irena ignorant of it.

use irena_core::{
    CompanyIdV1, NotarisationV1, NotaryIdV1, NotaryTimeV1, RecordBodyV1, RecordKindV1,
};
use irena_ledger::{
    LedgerError, company_at, genesis_in_force, genesis_with_company, history, in_force, publish,
    rules_in_force, shares_in_force,
};
use prunella_core::{
    BlockHeight, GenesisSpec, Hash, Namespace, NetworkId, SchemaVersion, TransactionDraft,
};
use prunella_crypto::SigningKey;
use prunella_store::LocalChainStore;
use std::path::PathBuf;
use tempfile::TempDir;

const GENESIS: &str = r#"<company-genesis>
  <identity name="Acme Industries Ltd" jurisdiction="gb" registered-number="01234567"/>
  <incorporation document-digest="9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f"/>
</company-genesis>"#;

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

/// A chain whose genesis carries Acme's founding record, with shares at height 1 and
/// rules at height 2.
fn founded() -> Chain {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("acme.chain");
    let spec = genesis_with_company(
        NetworkId::new("acme-net").expect("network"),
        &key(1),
        &acme(),
        GENESIS,
        &notary("2026-01-10T09:00:00Z"),
        0,
    )
    .expect("genesis spec");
    let store = LocalChainStore::init_genesis(&path, spec).expect("create");
    publish(
        &store,
        &key(1),
        &acme(),
        RecordKindV1::ShareStructure,
        SHARES_V1,
        None,
        &notary("2026-01-10T09:05:00Z"),
        1000,
    )
    .expect("shares");
    publish(
        &store,
        &key(1),
        &acme(),
        RecordKindV1::VotingRules,
        RULES_V1,
        None,
        &notary("2026-01-10T09:10:00Z"),
        2000,
    )
    .expect("rules");
    Chain {
        _dir: dir,
        path,
        store,
    }
}

#[test]
fn the_founding_record_is_in_force_at_height_zero() {
    let chain = founded();
    let genesis = genesis_in_force(&chain.store, &acme(), BlockHeight::GENESIS).expect("in force");
    assert_eq!(genesis.height, BlockHeight::GENESIS);
    assert!(genesis.supersedes.is_none());
    assert_eq!(genesis.value.identity.name, "Acme Industries Ltd");
    assert_eq!(genesis.notarisation, notary("2026-01-10T09:00:00Z"));

    // The genesis block is a plain Prunella block: its one transaction is the record.
    let block = chain
        .store
        .get_block(BlockHeight::GENESIS)
        .expect("read")
        .expect("genesis");
    assert_eq!(block.transactions.len(), 1);
    assert_eq!(block.transactions[0].namespace.as_str(), "irena.company.v1");
}

#[test]
fn the_same_founding_inputs_derive_the_same_genesis_hash() {
    let spec = || {
        genesis_with_company(
            NetworkId::new("acme-net").expect("n"),
            &key(1),
            &acme(),
            GENESIS,
            &notary("2026-01-10T09:00:00Z"),
            0,
        )
        .expect("spec")
    };
    assert_eq!(
        spec().build().expect("g").hash(),
        spec().build().expect("g").hash()
    );
    // And a different notary time is a different genesis: the notarisation is part of
    // what the chain commits to.
    let other = genesis_with_company(
        NetworkId::new("acme-net").expect("n"),
        &key(1),
        &acme(),
        GENESIS,
        &notary("2026-01-10T09:00:01Z"),
        0,
    )
    .expect("spec");
    assert_ne!(
        spec().build().expect("g").hash(),
        other.build().expect("g").hash()
    );
}

#[test]
fn the_ledger_stores_each_body_byte_for_byte_and_the_export_shows_it_nested() {
    let chain = founded();
    for (height, body) in [(0, GENESIS), (1, SHARES_V1), (2, RULES_V1)] {
        let block = chain
            .store
            .get_block(BlockHeight(height))
            .expect("read")
            .expect("block");
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
        3,
        "{xml}"
    );
    assert!(!xml.contains("encoding=\"base64\""), "{xml}");
    assert!(xml.contains(SHARES_V1), "{xml}");

    // And it comes back from that export to the same chain.
    let restored_path = chain.path.with_extension("restored");
    let (restored, _) = prunella_xml::restore(
        &restored_path,
        &prunella_xml::read_document(&xml).expect("parse"),
    )
    .expect("restore");
    assert_eq!(
        restored.head().expect("head"),
        chain.store.head().expect("head")
    );
    assert_eq!(
        company_at(&restored, &acme(), BlockHeight(2)).expect("state"),
        company_at(&chain.store, &acme(), BlockHeight(2)).expect("state")
    );
}

#[test]
fn the_rules_read_back_through_bornites_own_parser() {
    let chain = founded();
    let standalone = bornite_xml::read_rules_document(RULES_V1).expect("standalone");
    assert_eq!(
        rules_in_force(&chain.store, &acme(), BlockHeight(2))
            .expect("in force")
            .value,
        standalone
    );
}

#[test]
fn company_at_needs_all_three_records() {
    let chain = founded();
    let missing_rules =
        company_at(&chain.store, &acme(), BlockHeight(1)).expect_err("no rules yet");
    assert!(
        matches!(
            &missing_rules,
            LedgerError::NothingInForce {
                kind: RecordKindV1::VotingRules,
                at: BlockHeight(1),
                ..
            }
        ),
        "{missing_rules}"
    );
    let missing_shares =
        company_at(&chain.store, &acme(), BlockHeight::GENESIS).expect_err("no shares yet");
    assert!(
        matches!(
            &missing_shares,
            LedgerError::NothingInForce {
                kind: RecordKindV1::ShareStructure,
                ..
            }
        ),
        "{missing_shares}"
    );

    let state = company_at(&chain.store, &acme(), BlockHeight(2)).expect("complete");
    assert_eq!(state.at, BlockHeight(2));
    assert_eq!(state.genesis.height, BlockHeight(0));
    assert_eq!(state.shares.height, BlockHeight(1));
    assert_eq!(state.rules.height, BlockHeight(2));
    assert_eq!(state.shares.value.total_shares(), 1000);
    assert_eq!(
        state.shares.notarisation.at.as_str(),
        "2026-01-10T09:05:00Z"
    );
}

#[test]
fn an_amendment_supersedes_the_record_in_force() {
    let chain = founded();
    let before = shares_in_force(&chain.store, &acme(), BlockHeight(2)).expect("v1");
    let amended = publish(
        &chain.store,
        &key(2),
        &acme(),
        RecordKindV1::ShareStructure,
        SHARES_V2,
        Some(before.tx_id),
        &notary("2026-02-01T10:00:00Z"),
        3000,
    )
    .expect("amend");
    assert_eq!(amended.height, BlockHeight(3));
    assert_eq!(amended.record.supersedes, Some(before.tx_id));
    assert_eq!(amended.signer, key(2).public_key());

    let after = shares_in_force(&chain.store, &acme(), BlockHeight(3)).expect("v2");
    assert_eq!(after.tx_id, amended.tx_id);
    assert_eq!(after.supersedes, Some(before.tx_id));
    assert_eq!(after.value.len(), 4);
    assert_eq!(after.value.total_shares(), 1000);
}

#[test]
fn resolution_at_a_past_height_returns_the_register_of_that_time() {
    let chain = founded();
    let v1 = shares_in_force(&chain.store, &acme(), BlockHeight(2)).expect("v1");
    publish(
        &chain.store,
        &key(1),
        &acme(),
        RecordKindV1::ShareStructure,
        SHARES_V2,
        Some(v1.tx_id),
        &notary("2026-02-01T10:00:00Z"),
        3000,
    )
    .expect("amend");
    let rules_v1 = rules_in_force(&chain.store, &acme(), BlockHeight(3)).expect("rules");
    publish(
        &chain.store,
        &key(1),
        &acme(),
        RecordKindV1::VotingRules,
        RULES_V2,
        Some(rules_v1.tx_id),
        &notary("2026-02-01T10:05:00Z"),
        4000,
    )
    .expect("amend rules");

    for height in 1..=2 {
        let state_then = company_at(&chain.store, &acme(), BlockHeight(height));
        let shares_then =
            shares_in_force(&chain.store, &acme(), BlockHeight(height)).expect("then");
        assert_eq!(shares_then.tx_id, v1.tx_id, "height {height}");
        assert_eq!(shares_then.value.len(), 3);
        if height == 2 {
            assert_eq!(state_then.expect("complete").rules.tx_id, rules_v1.tx_id);
        }
    }
    let now = company_at(&chain.store, &acme(), BlockHeight(4)).expect("now");
    assert_eq!(now.shares.value.len(), 4);
    assert_ne!(now.rules.tx_id, rules_v1.tx_id);
    // Asking beyond the head resolves at the head.
    assert_eq!(
        company_at(&chain.store, &acme(), BlockHeight(999))
            .expect("head")
            .shares,
        now.shares
    );
}

#[test]
fn a_stale_amendment_is_refused_and_the_ledger_is_untouched() {
    let chain = founded();
    let v1 = shares_in_force(&chain.store, &acme(), BlockHeight(2)).expect("v1");
    let v2 = publish(
        &chain.store,
        &key(1),
        &acme(),
        RecordKindV1::ShareStructure,
        SHARES_V2,
        Some(v1.tx_id),
        &notary("2026-02-01T10:00:00Z"),
        3000,
    )
    .expect("amend");
    let head = chain.store.head().expect("head");

    // Amending v1 again, after v2 exists, is the two-editors problem.
    let error = publish(
        &chain.store,
        &key(1),
        &acme(),
        RecordKindV1::ShareStructure,
        SHARES_V1,
        Some(v1.tx_id),
        &notary("2026-02-02T10:00:00Z"),
        4000,
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
    assert_eq!(chain.store.head().expect("head"), head, "nothing written");

    // So is superseding nothing when something is in force.
    let error = publish(
        &chain.store,
        &key(1),
        &acme(),
        RecordKindV1::ShareStructure,
        SHARES_V1,
        None,
        &notary("2026-02-02T10:00:00Z"),
        4000,
    )
    .expect_err("stale");
    assert!(matches!(error, LedgerError::StaleAmendment { .. }));
    assert_eq!(chain.store.head().expect("head"), head);

    // And so is a first record that claims to supersede something.
    let bogus = prunella_core::TxId::from_hash(Hash::from_bytes([0x77; 32]));
    let error = publish(
        &chain.store,
        &key(1),
        &CompanyIdV1::new("other").expect("id"),
        RecordKindV1::ShareStructure,
        SHARES_V1,
        Some(bogus),
        &notary("2026-02-02T10:00:00Z"),
        4000,
    )
    .expect_err("stale");
    assert!(matches!(error, LedgerError::StaleAmendment { .. }));
    assert_eq!(chain.store.head().expect("head"), head);
}

#[test]
fn the_three_kinds_have_separate_amendment_chains() {
    let chain = founded();
    let shares_v1 = shares_in_force(&chain.store, &acme(), BlockHeight(2)).expect("shares");
    let rules_v1 = rules_in_force(&chain.store, &acme(), BlockHeight(2)).expect("rules");
    let genesis_v1 = genesis_in_force(&chain.store, &acme(), BlockHeight(2)).expect("genesis");
    assert!(shares_v1.supersedes.is_none());
    assert!(rules_v1.supersedes.is_none());
    assert!(genesis_v1.supersedes.is_none());

    // Amending the rules leaves the register's chain alone, and vice versa.
    publish(
        &chain.store,
        &key(1),
        &acme(),
        RecordKindV1::VotingRules,
        RULES_V2,
        Some(rules_v1.tx_id),
        &notary("2026-02-01T10:00:00Z"),
        3000,
    )
    .expect("amend rules");
    assert_eq!(
        shares_in_force(&chain.store, &acme(), BlockHeight(3))
            .expect("shares")
            .tx_id,
        shares_v1.tx_id
    );
    // The identity can be amended too (a name change), on its own chain.
    let renamed = GENESIS.replace("Acme Industries Ltd", "Acme Industries plc");
    publish(
        &chain.store,
        &key(1),
        &acme(),
        RecordKindV1::CompanyGenesis,
        &renamed,
        Some(genesis_v1.tx_id),
        &notary("2026-03-01T10:00:00Z"),
        4000,
    )
    .expect("rename");
    let state = company_at(&chain.store, &acme(), BlockHeight(4)).expect("state");
    assert_eq!(state.genesis.value.identity.name, "Acme Industries plc");
    assert_eq!(state.shares.tx_id, shares_v1.tx_id);
    assert_ne!(state.rules.tx_id, rules_v1.tx_id);
    for kind in RecordKindV1::ALL {
        let versions = history(&chain.store, &acme(), kind, BlockHeight(4)).expect("history");
        assert_eq!(
            versions.len(),
            if kind == RecordKindV1::ShareStructure {
                1
            } else {
                2
            },
            "{kind}"
        );
    }
}

#[test]
fn history_lists_every_version_in_ledger_order_with_links() {
    let chain = founded();
    let v1 = shares_in_force(&chain.store, &acme(), BlockHeight(2)).expect("v1");
    let v2 = publish(
        &chain.store,
        &key(1),
        &acme(),
        RecordKindV1::ShareStructure,
        SHARES_V2,
        Some(v1.tx_id),
        &notary("2026-02-01T10:00:00Z"),
        3000,
    )
    .expect("v2");
    let v3 = publish(
        &chain.store,
        &key(1),
        &acme(),
        RecordKindV1::ShareStructure,
        SHARES_V1,
        Some(v2.tx_id),
        &notary("2026-03-01T10:00:00Z"),
        4000,
    )
    .expect("v3");
    let versions = history(
        &chain.store,
        &acme(),
        RecordKindV1::ShareStructure,
        BlockHeight(4),
    )
    .expect("history");
    let ids: Vec<_> = versions.iter().map(|r| r.tx_id).collect();
    assert_eq!(ids, [v1.tx_id, v2.tx_id, v3.tx_id]);
    let links: Vec<_> = versions.iter().map(|r| r.record.supersedes).collect();
    assert_eq!(links, [None, Some(v1.tx_id), Some(v2.tx_id)]);
    let heights: Vec<_> = versions.iter().map(|r| r.height.value()).collect();
    assert_eq!(heights, [1, 3, 4]);
    assert_eq!(
        history(
            &chain.store,
            &acme(),
            RecordKindV1::ShareStructure,
            BlockHeight(3)
        )
        .expect("h")
        .len(),
        2
    );
}

#[test]
fn companies_on_one_chain_are_independent() {
    let chain = founded();
    let beta = CompanyIdV1::new("beta").expect("id");
    let beta_shares = publish(
        &chain.store,
        &key(3),
        &beta,
        RecordKindV1::ShareStructure,
        SHARES_V2,
        None,
        &notary("2026-02-01T10:00:00Z"),
        3000,
    )
    .expect("beta shares");
    assert_eq!(
        shares_in_force(&chain.store, &acme(), BlockHeight(3))
            .expect("acme")
            .value
            .len(),
        3
    );
    assert_eq!(
        shares_in_force(&chain.store, &beta, BlockHeight(3))
            .expect("beta")
            .tx_id,
        beta_shares.tx_id
    );
    assert!(matches!(
        rules_in_force(&chain.store, &beta, BlockHeight(3)),
        Err(LedgerError::NothingInForce { .. })
    ));
}

#[test]
fn a_chain_written_around_irena_is_reported_not_repaired() {
    let chain = founded();
    let v1 = shares_in_force(&chain.store, &acme(), BlockHeight(2)).expect("v1");

    // Append, through Prunella directly, a register record that supersedes nothing.
    let bogus = prunella_core::TxId::from_hash(Hash::from_bytes([0x55; 32]));
    let payload = irena_core::compose_record(
        RecordKindV1::ShareStructure,
        &acme(),
        Some(bogus),
        &notary("2026-02-01T10:00:00Z"),
        SHARES_V2,
    )
    .expect("compose");
    append_raw(&chain.store, "irena.shares.v1", payload.into_bytes());

    let error = shares_in_force(&chain.store, &acme(), BlockHeight(3)).expect_err("broken");
    assert!(
        matches!(
            &error,
            LedgerError::BrokenAmendmentChain {
                kind: RecordKindV1::ShareStructure,
                height: BlockHeight(3),
                ..
            }
        ),
        "{error}"
    );
    assert!(error.to_string().contains(&v1.tx_id.to_string()), "{error}");
    // Before the break, resolution still works.
    assert_eq!(
        shares_in_force(&chain.store, &acme(), BlockHeight(2))
            .expect("before")
            .tx_id,
        v1.tx_id
    );
    // Other kinds and other companies are unaffected.
    assert!(rules_in_force(&chain.store, &acme(), BlockHeight(3)).is_ok());
    // Nothing is repaired: the break is still there on the next read.
    assert!(shares_in_force(&chain.store, &acme(), BlockHeight(3)).is_err());
}

#[test]
fn an_unreadable_payload_in_an_irena_namespace_is_reported_not_skipped() {
    let chain = founded();
    append_raw(&chain.store, "irena.rules.v1", b"not a record".to_vec());
    let error = rules_in_force(&chain.store, &acme(), BlockHeight(3)).expect_err("unreadable");
    assert!(
        matches!(
            &error,
            LedgerError::UnreadableRecord {
                kind: RecordKindV1::VotingRules,
                height: BlockHeight(3),
                ..
            }
        ),
        "{error}"
    );

    // A readable record of the wrong kind for its namespace is just as wrong.
    let misfiled = irena_core::compose_record(
        RecordKindV1::ShareStructure,
        &acme(),
        None,
        &notary("2026-02-01T10:00:00Z"),
        SHARES_V2,
    )
    .expect("compose");
    append_raw(&chain.store, "irena.company.v1", misfiled.into_bytes());
    let error = genesis_in_force(&chain.store, &acme(), BlockHeight(4)).expect_err("misfiled");
    assert!(
        matches!(&error, LedgerError::UnreadableRecord { .. }),
        "{error}"
    );

    // Transactions in other namespaces are not Irena's business.
    append_raw(&chain.store, "app.other", b"whatever".to_vec());
    assert!(shares_in_force(&chain.store, &acme(), BlockHeight(5)).is_ok());
}

fn append_raw(store: &LocalChainStore, namespace: &str, payload: Vec<u8>) {
    let signer = key(9);
    let head = store.head().expect("head");
    let parent = store.get_block(head.height).expect("read").expect("block");
    let transaction = signer.sign_transaction(TransactionDraft {
        namespace: Namespace::new(namespace).expect("ns"),
        schema_version: SchemaVersion(1),
        payload,
        signer: signer.public_key(),
        nonce: head.height.value() + 1,
    });
    let block = parent
        .header
        .child_draft(vec![transaction], 9999)
        .expect("draft")
        .build()
        .expect("build");
    store.append_block(block).expect("append");
}

#[test]
fn resolution_is_identical_across_independently_built_chains() {
    let build = |dir: &TempDir| {
        let path = dir.path().join("acme.chain");
        let spec = genesis_with_company(
            NetworkId::new("acme-net").expect("n"),
            &key(1),
            &acme(),
            GENESIS,
            &notary("2026-01-10T09:00:00Z"),
            0,
        )
        .expect("spec");
        let store = LocalChainStore::init_genesis(&path, spec).expect("create");
        for (kind, body, at, ts) in [
            (
                RecordKindV1::ShareStructure,
                SHARES_V1,
                "2026-01-10T09:05:00Z",
                1000,
            ),
            (
                RecordKindV1::VotingRules,
                RULES_V1,
                "2026-01-10T09:10:00Z",
                2000,
            ),
        ] {
            publish(&store, &key(1), &acme(), kind, body, None, &notary(at), ts).expect("publish");
        }
        store
    };
    let a = TempDir::new().expect("a");
    let b = TempDir::new().expect("b");
    let (store_a, store_b) = (build(&a), build(&b));
    assert_eq!(store_a.head().expect("h"), store_b.head().expect("h"));
    assert_eq!(
        company_at(&store_a, &acme(), BlockHeight(2)).expect("a"),
        company_at(&store_b, &acme(), BlockHeight(2)).expect("b")
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
    let head = chain.store.head().expect("head");
    let error = publish(
        &chain.store,
        &key(1),
        &acme(),
        RecordKindV1::ShareStructure,
        "<share-structure><holder id=\"x\" shares=\"0\"/></share-structure>",
        Some(
            shares_in_force(&chain.store, &acme(), head.height)
                .expect("v1")
                .tx_id,
        ),
        &notary("2026-02-01T10:00:00Z"),
        3000,
    )
    .expect_err("zero shares");
    assert!(matches!(error, LedgerError::Record(_)), "{error}");
    assert_eq!(chain.store.head().expect("head"), head);

    // The generic accessor hands back whichever body is there.
    let body = in_force(
        &chain.store,
        &acme(),
        RecordKindV1::ShareStructure,
        head.height,
    )
    .expect("body");
    assert!(matches!(body.value, RecordBodyV1::ShareStructure(_)));
}

#[test]
fn a_fresh_prunella_chain_has_no_company() {
    let dir = TempDir::new().expect("dir");
    let store = LocalChainStore::init_genesis(
        dir.path().join("plain.chain"),
        GenesisSpec::new(NetworkId::new("plain").expect("n")),
    )
    .expect("create");
    assert!(matches!(
        company_at(&store, &acme(), BlockHeight::GENESIS),
        Err(LedgerError::NothingInForce {
            kind: RecordKindV1::CompanyGenesis,
            ..
        })
    ));
    assert!(
        history(
            &store,
            &acme(),
            RecordKindV1::VotingRules,
            BlockHeight::GENESIS
        )
        .expect("empty")
        .is_empty()
    );
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
