//! Rules on the ledger: genesis, amendment, resolution at a height, and the boundary
//! that keeps the rules organisation-blind.

use bornite_core::{BallotSetV1, BallotV1, ChoiceV1, VoterIdV1};
use bornite_eval::OutcomeV1;
use bornite_rules::{ThresholdBasisV1, ThresholdRuleV1, TieTreatmentV1};
use governance_bridge::{
    BridgeError, NotarisationV1, RecordKindV1, SubjectV1, compose_record, evaluate_at,
    genesis_with_rules, history, publish_roll, publish_rules, read_record, rules_in_force,
};
use prunella_core::{BlockHeight, GenesisSpec, Hash, NetworkId};
use prunella_crypto::SigningKey;
use prunella_store::LocalChainStore;
use std::path::PathBuf;
use tempfile::TempDir;

const RULES_V1: &str = r#"<voting-rules version="1.0">
  <weight type="electorate"/>
  <exclusions enabled="true"/>
  <quorum type="none"/>
  <threshold type="simple-majority" basis="votes-cast"/>
  <abstentions treatment="exclude"/>
  <tie treatment="reject"/>
</voting-rules>"#;

/// The same rules, amended to a two-thirds supermajority with ties accepted.
const RULES_V2: &str = r#"<voting-rules version="1.0">
  <weight type="electorate"/>
  <exclusions enabled="true"/>
  <quorum type="none"/>
  <threshold type="fraction" numerator="2" denominator="3" basis="votes-cast"/>
  <abstentions treatment="exclude"/>
  <tie treatment="accept"/>
</voting-rules>"#;

const ROLL: &str = r#"<electorate>
  <voter id="drone-01" weight="2"/>
  <voter id="drone-02" weight="1"/>
  <voter id="drone-03" weight="1"/>
</electorate>"#;

fn key(seed: u8) -> SigningKey {
    SigningKey::from_seed([seed; 32])
}

fn subject() -> SubjectV1 {
    SubjectV1::new("swarm-alpha").expect("subject")
}

fn notary() -> NotarisationV1 {
    NotarisationV1 {
        notary: "notary-07".to_owned(),
        statement: Some("Minuted at the founding assembly".to_owned()),
        source_digest: Some(Hash::from_bytes([0xab; 32])),
    }
}

fn ballots(entries: &[(&str, ChoiceV1)]) -> BallotSetV1 {
    BallotSetV1::new(
        entries
            .iter()
            .map(|(v, c)| BallotV1 {
                voter: VoterIdV1::new(*v).expect("id"),
                choice: *c,
            })
            .collect(),
    )
    .expect("ballots")
}

struct Chain {
    _dir: TempDir,
    path: PathBuf,
    store: LocalChainStore,
}

/// A chain whose genesis carries RULES_V1 for the subject, and a roll at height 1.
fn founded() -> Chain {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("gov.chain");
    let spec = genesis_with_rules(
        NetworkId::new("swarm").expect("network"),
        &key(1),
        &subject(),
        RULES_V1,
        Some(&notary()),
        0,
    )
    .expect("genesis spec");
    let store = LocalChainStore::init_genesis(&path, spec).expect("create");
    publish_roll(&store, &key(1), &subject(), ROLL, None, None, 1000).expect("roll");
    Chain {
        _dir: dir,
        path,
        store,
    }
}

#[test]
fn rules_in_genesis_are_in_force_at_height_zero() {
    let chain = founded();
    let in_force =
        rules_in_force(&chain.store, &subject(), BlockHeight::GENESIS).expect("in force");
    assert_eq!(in_force.height, BlockHeight::GENESIS);
    assert!(in_force.supersedes.is_none());
    assert_eq!(in_force.value.tie, TieTreatmentV1::Reject);
    assert_eq!(in_force.notarisation, Some(notary()));

    // The genesis block is a plain Prunella block: its one transaction is the record.
    let genesis = chain
        .store
        .get_block(BlockHeight::GENESIS)
        .expect("read")
        .expect("genesis");
    assert_eq!(genesis.transactions.len(), 1);
    assert_eq!(
        genesis.transactions[0].namespace.as_str(),
        "governance.rules"
    );
}

#[test]
fn the_same_founding_inputs_derive_the_same_genesis_hash() {
    let spec = || {
        genesis_with_rules(
            NetworkId::new("swarm").expect("n"),
            &key(1),
            &subject(),
            RULES_V1,
            Some(&notary()),
            0,
        )
        .expect("spec")
    };
    assert_eq!(
        spec().build().expect("g").hash(),
        spec().build().expect("g").hash()
    );
}

#[test]
fn the_ledger_stores_the_rules_element_byte_for_byte() {
    let chain = founded();
    let genesis = chain
        .store
        .get_block(BlockHeight::GENESIS)
        .expect("read")
        .expect("genesis");
    let payload = core::str::from_utf8(&genesis.transactions[0].payload).expect("utf-8");
    assert!(
        payload.contains(RULES_V1),
        "the standalone rules text must appear verbatim:\n{payload}"
    );
    // And it reads back through Bornite's own parser to exactly the standalone rules.
    let standalone = bornite_xml::read_rules_document(RULES_V1).expect("standalone");
    assert_eq!(
        rules_in_force(&chain.store, &subject(), BlockHeight::GENESIS)
            .expect("f")
            .value,
        standalone
    );
}

#[test]
fn an_amendment_supersedes_the_rules_in_force() {
    let chain = founded();
    let before = rules_in_force(&chain.store, &subject(), BlockHeight(1)).expect("v1");
    let amended = publish_rules(
        &chain.store,
        &key(1),
        &subject(),
        RULES_V2,
        Some(before.tx_id),
        None,
        2000,
    )
    .expect("amend");
    assert_eq!(amended.height, BlockHeight(2));
    assert_eq!(amended.record.supersedes, Some(before.tx_id));

    let now = rules_in_force(&chain.store, &subject(), BlockHeight(2)).expect("v2");
    assert_eq!(now.tx_id, amended.tx_id);
    assert_eq!(now.value.tie, TieTreatmentV1::Accept);
    assert!(matches!(
        now.value.threshold,
        ThresholdRuleV1::Fraction {
            basis: ThresholdBasisV1::VotesCast,
            ..
        }
    ));
}

#[test]
fn resolution_at_a_past_height_returns_the_rules_of_that_time() {
    let chain = founded();
    let v1 = rules_in_force(&chain.store, &subject(), BlockHeight(1)).expect("v1");
    publish_rules(
        &chain.store,
        &key(1),
        &subject(),
        RULES_V2,
        Some(v1.tx_id),
        None,
        2000,
    )
    .expect("amend");

    // Height 1 still sees v1; the head sees v2. History is not rewritten by an amendment.
    assert_eq!(
        rules_in_force(&chain.store, &subject(), BlockHeight(1))
            .expect("f")
            .tx_id,
        v1.tx_id
    );
    assert_eq!(
        rules_in_force(&chain.store, &subject(), BlockHeight(0))
            .expect("f")
            .tx_id,
        v1.tx_id
    );
    assert_ne!(
        rules_in_force(&chain.store, &subject(), BlockHeight(2))
            .expect("f")
            .tx_id,
        v1.tx_id
    );
}

#[test]
fn a_stale_amendment_is_refused_and_the_ledger_is_untouched() {
    let chain = founded();
    let v1 = rules_in_force(&chain.store, &subject(), BlockHeight(1)).expect("v1");
    publish_rules(
        &chain.store,
        &key(1),
        &subject(),
        RULES_V2,
        Some(v1.tx_id),
        None,
        2000,
    )
    .expect("amend");
    let head = chain.store.head().expect("head");

    // Someone who has not seen v2 tries to amend v1 again.
    let error = publish_rules(
        &chain.store,
        &key(2),
        &subject(),
        RULES_V1,
        Some(v1.tx_id),
        None,
        3000,
    )
    .expect_err("stale");
    assert!(
        matches!(error, BridgeError::StaleAmendment { .. }),
        "{error}"
    );

    // Or forgets to supersede at all.
    let error = publish_rules(
        &chain.store,
        &key(2),
        &subject(),
        RULES_V1,
        None,
        None,
        3000,
    )
    .expect_err("stale");
    assert!(
        matches!(error, BridgeError::StaleAmendment { .. }),
        "{error}"
    );

    assert_eq!(
        chain.store.head().expect("head"),
        head,
        "nothing was written"
    );
}

#[test]
fn a_first_record_may_not_supersede_anything() {
    let chain = founded();
    let other = SubjectV1::new("membership").expect("subject");
    let v1 = rules_in_force(&chain.store, &subject(), BlockHeight(1)).expect("v1");
    let error = publish_rules(
        &chain.store,
        &key(1),
        &other,
        RULES_V1,
        Some(v1.tx_id),
        None,
        2000,
    )
    .expect_err("nothing to supersede");
    assert!(
        matches!(error, BridgeError::StaleAmendment { .. }),
        "{error}"
    );
}

#[test]
fn rules_and_rolls_have_separate_amendment_chains() {
    let chain = founded();
    let rules = rules_in_force(&chain.store, &subject(), BlockHeight(1)).expect("rules");
    // A roll cannot supersede a rules record.
    let error = publish_roll(
        &chain.store,
        &key(1),
        &subject(),
        ROLL,
        Some(rules.tx_id),
        None,
        2000,
    )
    .expect_err("cross-kind");
    assert!(
        matches!(
            error,
            BridgeError::StaleAmendment {
                kind: RecordKindV1::Roll,
                ..
            }
        ),
        "{error}"
    );
}

#[test]
fn history_lists_every_version_in_ledger_order() {
    let chain = founded();
    let v1 = rules_in_force(&chain.store, &subject(), BlockHeight(1)).expect("v1");
    let v2 = publish_rules(
        &chain.store,
        &key(1),
        &subject(),
        RULES_V2,
        Some(v1.tx_id),
        None,
        2000,
    )
    .expect("v2");
    let v3 = publish_rules(
        &chain.store,
        &key(2),
        &subject(),
        RULES_V1,
        Some(v2.tx_id),
        Some(&notary()),
        3000,
    )
    .expect("v3");

    let versions = history(
        &chain.store,
        &subject(),
        RecordKindV1::VotingRules,
        BlockHeight(99),
    )
    .expect("history");
    let ids: Vec<_> = versions.iter().map(|r| r.tx_id).collect();
    assert_eq!(ids, vec![v1.tx_id, v2.tx_id, v3.tx_id]);
    assert_eq!(versions[2].signer, key(2).public_key());
    assert_eq!(versions[2].record.notarisation, Some(notary()));
    assert_eq!(
        versions
            .iter()
            .map(|r| r.height.value())
            .collect::<Vec<_>>(),
        vec![0, 2, 3]
    );

    // History up to height 2 does not know about v3.
    assert_eq!(
        history(
            &chain.store,
            &subject(),
            RecordKindV1::VotingRules,
            BlockHeight(2)
        )
        .expect("h")
        .len(),
        2
    );
}

#[test]
fn subjects_are_independent() {
    let chain = founded();
    let other = SubjectV1::new("membership").expect("subject");
    assert!(matches!(
        rules_in_force(&chain.store, &other, BlockHeight(1)),
        Err(BridgeError::NothingInForce { .. })
    ));
    publish_rules(&chain.store, &key(1), &other, RULES_V2, None, None, 2000)
        .expect("other subject");
    assert_eq!(
        rules_in_force(&chain.store, &subject(), BlockHeight(2))
            .expect("f")
            .value
            .tie,
        TieTreatmentV1::Reject
    );
    assert_eq!(
        rules_in_force(&chain.store, &other, BlockHeight(2))
            .expect("f")
            .value
            .tie,
        TieTreatmentV1::Accept
    );
}

#[test]
fn evaluating_against_the_ledger_matches_evaluating_standalone() {
    let chain = founded();
    let cast = ballots(&[
        ("drone-01", ChoiceV1::Yes),
        ("drone-02", ChoiceV1::No),
        ("drone-03", ChoiceV1::No),
    ]);

    let standalone = bornite_eval::evaluate(
        &bornite_xml::read_rules_document(RULES_V1).expect("rules"),
        &governance_bridge::read_electorate_document(ROLL).expect("roll"),
        &cast,
    )
    .expect("standalone");
    let ledger = evaluate_at(&chain.store, &subject(), BlockHeight(1), &cast).expect("ledger");

    assert_eq!(
        ledger.evaluation, standalone,
        "living on a ledger changes nothing about the rules"
    );
    // 2 yes against 2 no: an exact tie, rejected under v1.
    assert_eq!(ledger.evaluation.outcome, OutcomeV1::Rejected);
    assert_eq!(ledger.rules.height, BlockHeight::GENESIS);
    assert_eq!(ledger.roll.height, BlockHeight(1));
}

#[test]
fn the_outcome_at_a_height_follows_the_rules_in_force_then() {
    let chain = founded();
    let cast = ballots(&[
        ("drone-01", ChoiceV1::Yes),
        ("drone-02", ChoiceV1::No),
        ("drone-03", ChoiceV1::No),
    ]);
    let v1 = rules_in_force(&chain.store, &subject(), BlockHeight(1)).expect("v1");
    publish_rules(
        &chain.store,
        &key(1),
        &subject(),
        RULES_V2,
        Some(v1.tx_id),
        None,
        2000,
    )
    .expect("amend");

    // Under v1 (tie rejected) the 2-2 tie fails; under v2 (2/3, tie accepted) 2 of 4 is below.
    let then = evaluate_at(&chain.store, &subject(), BlockHeight(1), &cast).expect("then");
    let now = evaluate_at(&chain.store, &subject(), BlockHeight(2), &cast).expect("now");
    assert_eq!(
        then.evaluation.reason,
        bornite_eval::ReasonCodeV1::TieRejected
    );
    assert_eq!(
        now.evaluation.reason,
        bornite_eval::ReasonCodeV1::ThresholdNotMet
    );
    assert_ne!(then.rules.tx_id, now.rules.tx_id);
}

#[test]
fn a_chain_written_around_the_bridge_is_reported_not_repaired() {
    // Publish a second rules record for the subject directly through Prunella, with a
    // supersedes that names nothing. The bridge must refuse to resolve anything for
    // the subject rather than quietly pick a winner.
    let chain = founded();
    let rogue = compose_record(RecordKindV1::VotingRules, &subject(), None, None, RULES_V2)
        .expect("compose");
    let transaction = key(9).sign_transaction(prunella_core::TransactionDraft {
        namespace: prunella_core::Namespace::new("governance.rules").expect("ns"),
        schema_version: prunella_core::SchemaVersion(1),
        payload: rogue.into_bytes(),
        signer: key(9).public_key(),
        nonce: 77,
    });
    let head = chain.store.head().expect("head");
    let parent = chain
        .store
        .get_block(head.height)
        .expect("read")
        .expect("block");
    let block = parent
        .header
        .child_draft(vec![transaction], 5000)
        .expect("draft")
        .build()
        .expect("build");
    chain
        .store
        .append_block(block)
        .expect("prunella accepts a valid block");

    let error = rules_in_force(&chain.store, &subject(), BlockHeight(9)).expect_err("broken chain");
    assert!(
        matches!(error, BridgeError::BrokenAmendmentChain { height, .. } if height == BlockHeight(2)),
        "{error}"
    );
    // Resolution before the rogue record still works.
    assert!(rules_in_force(&chain.store, &subject(), BlockHeight(1)).is_ok());
}

#[test]
fn resolution_is_identical_across_independently_built_chains() {
    let left = founded();
    let right = founded();
    for chain in [&left, &right] {
        let v1 = rules_in_force(&chain.store, &subject(), BlockHeight(1)).expect("v1");
        publish_rules(
            &chain.store,
            &key(1),
            &subject(),
            RULES_V2,
            Some(v1.tx_id),
            None,
            2000,
        )
        .expect("amend");
    }
    let l = rules_in_force(&left.store, &subject(), BlockHeight(2)).expect("l");
    let r = rules_in_force(&right.store, &subject(), BlockHeight(2)).expect("r");
    assert_eq!(l, r);
    assert_eq!(
        left.store.head().expect("h"),
        right.store.head().expect("h")
    );
    let _ = (&left.path, &right.path);
}

#[test]
fn a_record_composes_and_reads_back_with_the_element_verbatim() {
    let v1 = Some(prunella_core::TxId::from_hash(Hash::from_bytes([0x11; 32])));
    let xml = compose_record(
        RecordKindV1::VotingRules,
        &subject(),
        v1,
        Some(&notary()),
        RULES_V1,
    )
    .expect("compose");
    assert!(xml.contains(RULES_V1));
    assert!(xml.contains(r#"kind="voting-rules""#));
    assert!(xml.contains(r#"subject="swarm-alpha""#));
    assert!(xml.contains(&format!(r#"supersedes="{}""#, "11".repeat(32))));
    assert!(xml.contains(r#"notary="notary-07""#));

    let record = read_record(&xml).expect("read");
    assert_eq!(record.kind(), RecordKindV1::VotingRules);
    assert_eq!(record.supersedes, v1);
    assert_eq!(record.notarisation, Some(notary()));

    // A leading declaration on the inner document is removed; the element is kept.
    let with_decl = format!("<?xml version=\"1.0\"?>\n{RULES_V1}");
    let xml2 = compose_record(RecordKindV1::VotingRules, &subject(), v1, None, &with_decl)
        .expect("compose");
    assert!(xml2.contains(RULES_V1));
    assert!(!xml2.contains("<?xml"));
}

#[test]
fn a_notarisation_with_special_characters_survives() {
    let awkward = NotarisationV1 {
        notary: "Smith & Co <notaries>".to_owned(),
        statement: Some("Said \"approved\"".to_owned()),
        source_digest: None,
    };
    let xml = compose_record(RecordKindV1::Roll, &subject(), None, Some(&awkward), ROLL)
        .expect("compose");
    assert_eq!(read_record(&xml).expect("read").notarisation, Some(awkward));
}

#[test]
fn malformed_records_are_refused() {
    let good = compose_record(RecordKindV1::VotingRules, &subject(), None, None, RULES_V1)
        .expect("compose");
    let cases = [
        (
            "kind mismatch",
            good.replace(r#"kind="voting-rules""#, r#"kind="roll""#),
        ),
        (
            "unknown kind",
            good.replace(r#"kind="voting-rules""#, r#"kind="bylaws""#),
        ),
        (
            "bad subject",
            good.replace(r#"subject="swarm-alpha""#, r#"subject="Swarm Alpha""#),
        ),
        (
            "bad supersedes",
            good.replace(
                r#"subject="swarm-alpha""#,
                r#"subject="swarm-alpha" supersedes="zz""#,
            ),
        ),
        (
            "unknown attribute",
            good.replace(
                r#"subject="swarm-alpha""#,
                r#"subject="swarm-alpha" company="acme""#,
            ),
        ),
        (
            "unknown element",
            good.replace("</governance-record>", "<shares/></governance-record>"),
        ),
        (
            "wrong version",
            good.replace(r#"version="1.0" kind"#, r#"version="2.0" kind"#),
        ),
        (
            "no body",
            "<governance-record version=\"1.0\" kind=\"roll\" subject=\"s\"/>".to_owned(),
        ),
        (
            "inner rules invalid",
            good.replace(r#"treatment="reject""#, r#"treatment="flip""#),
        ),
    ];
    for (label, xml) in cases {
        assert!(read_record(&xml).is_err(), "{label} should be refused");
    }
    assert!(
        compose_record(RecordKindV1::Roll, &subject(), None, None, RULES_V1).is_err(),
        "kind must match inner"
    );
}

#[test]
fn composed_records_validate_against_the_published_schema() {
    if std::process::Command::new("xmllint")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping: xmllint is not installed");
        return;
    }
    let schema = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/governance-record-v1.xsd")
        .canonicalize()
        .expect("schema");
    let dir = TempDir::new().expect("temp dir");
    let v1 = Some(prunella_core::TxId::from_hash(Hash::from_bytes([0x11; 32])));
    for (name, xml) in [
        (
            "rules",
            compose_record(
                RecordKindV1::VotingRules,
                &subject(),
                v1,
                Some(&notary()),
                RULES_V1,
            )
            .expect("c"),
        ),
        (
            "roll",
            compose_record(RecordKindV1::Roll, &subject(), None, None, ROLL).expect("c"),
        ),
    ] {
        let file = dir.path().join(format!("{name}.xml"));
        std::fs::write(&file, &xml).expect("write");
        let output = std::process::Command::new("xmllint")
            .args(["--noout", "--schema"])
            .arg(&schema)
            .arg(&file)
            .output()
            .expect("xmllint");
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// The boundary, checked mechanically: no Bornite source mentions Prunella, no
/// Prunella source mentions Bornite, and no crate but this one depends on both.
#[test]
fn neither_engine_knows_the_other_exists() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut leaks = Vec::new();
    for entry in std::fs::read_dir(&crates).expect("crates") {
        let path = entry.expect("entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_owned();
        let forbidden = if name.starts_with("bornite-") {
            "prunella"
        } else if name.starts_with("prunella-") {
            "bornite"
        } else {
            continue;
        };
        for file in walk(&path) {
            let text = std::fs::read_to_string(&file).expect("read").to_lowercase();
            if text.contains(forbidden) {
                leaks.push(format!("{} mentions {forbidden}", file.display()));
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

#[test]
fn a_founding_needs_valid_rules() {
    let error = genesis_with_rules(
        NetworkId::new("swarm").expect("n"),
        &key(1),
        &subject(),
        "<voting-rules/>",
        None,
        0,
    )
    .expect_err("invalid rules");
    assert!(
        matches!(
            error,
            BridgeError::Record(_) | BridgeError::Malformed { .. }
        ),
        "{error}"
    );
    let _ = GenesisSpec::new(NetworkId::new("unused").expect("n"));
}
