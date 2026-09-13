//! End-to-end: found a company, publish its register and rules, amend, show, verify.

use std::process::{Command, Output};
use tempfile::TempDir;

const GENESIS: &str = r#"<company-genesis>
  <identity name="Acme Industries Ltd" jurisdiction="gb" registered-number="01234567"/>
</company-genesis>
"#;
const SHARES_V1: &str = r#"<share-structure>
  <holder id="alice" key="4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c" name="Alice Smith" shares="500"/>
  <holder id="bob" key="7b217b217b217b217b217b217b217b217b217b217b217b217b217b217b217b21" shares="300"/>
  <holder id="carol" shares="200"/>
</share-structure>
"#;
const SHARES_V2: &str = r#"<share-structure>
  <holder id="alice" key="4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c" name="Alice Smith" shares="500"/>
  <holder id="bob" key="7b217b217b217b217b217b217b217b217b217b217b217b217b217b217b217b21" shares="150"/>
  <holder id="carol" shares="200"/>
  <holder id="dave" key="d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0" shares="150"/>
</share-structure>
"#;
const RULES: &str = r#"<voting-rules version="1.0">
  <weight type="electorate"/>
  <exclusions enabled="true"/>
  <quorum type="none"/>
  <threshold type="simple-majority" basis="votes-cast"/>
  <abstentions treatment="exclude"/>
  <tie treatment="reject"/>
</voting-rules>
"#;

struct Cli {
    dir: TempDir,
}

struct Run(Output);
impl Run {
    fn out(&self) -> String {
        String::from_utf8_lossy(&self.0.stdout).into_owned()
    }
    fn err(&self) -> String {
        String::from_utf8_lossy(&self.0.stderr).into_owned()
    }
    fn code(&self) -> i32 {
        self.0.status.code().unwrap_or(-1)
    }
    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.out())
            .unwrap_or_else(|e| panic!("not json ({e}):\n{}", self.out()))
    }
    fn ok(self) -> Self {
        assert_eq!(
            self.code(),
            0,
            "stdout:\n{}\nstderr:\n{}",
            self.out(),
            self.err()
        );
        self
    }
}

const NOTARY: &[&str] = &[
    "--notary-id",
    "notary-07",
    "--notary-name",
    "Jane Roe",
    "--notary-address",
    "12 High Street, London",
    "--notary-at",
    "2026-03-01T09:30:00Z",
];

impl Cli {
    fn new() -> Self {
        let dir = TempDir::new().expect("temp dir");
        for (name, text) in [
            ("genesis.xml", GENESIS),
            ("shares1.xml", SHARES_V1),
            ("shares2.xml", SHARES_V2),
            ("rules.xml", RULES),
        ] {
            std::fs::write(dir.path().join(name), text).expect("write");
        }
        std::fs::write(dir.path().join("k.key"), "01".repeat(32)).expect("key");
        Self { dir }
    }
    fn run(&self, args: &[&str]) -> Run {
        let output = Command::new(env!("CARGO_BIN_EXE_irena"))
            .current_dir(self.dir.path())
            .args(["--chain", "acme.chain"])
            .args(args)
            .output()
            .expect("run irena");
        Run(output)
    }
    fn with_notary(&self, args: &[&str]) -> Run {
        let mut full: Vec<&str> = args.to_vec();
        full.extend_from_slice(NOTARY);
        self.run(&full)
    }
    /// A founded company with its register at height 1 and rules at height 2.
    fn founded(&self) -> Run {
        let init = self
            .with_notary(&[
                "init",
                "--network",
                "acme-net",
                "--company",
                "acme",
                "--genesis",
                "genesis.xml",
                "--signing-key",
                "k.key",
                "--json",
            ])
            .ok();
        self.with_notary(&[
            "publish-shares",
            "--company",
            "acme",
            "--file",
            "shares1.xml",
            "--signing-key",
            "k.key",
            "--timestamp",
            "1000",
        ])
        .ok();
        self.with_notary(&[
            "publish-rules",
            "--company",
            "acme",
            "--file",
            "rules.xml",
            "--signing-key",
            "k.key",
            "--timestamp",
            "2000",
        ])
        .ok();
        init
    }
}

#[test]
fn init_founds_the_company_in_genesis() {
    let cli = Cli::new();
    let init = cli.founded();
    assert_eq!(init.json()["company"], "acme");
    let show = cli
        .run(&["show", "--company", "acme", "--at", "2", "--json"])
        .ok();
    let state = show.json();
    assert_eq!(state["genesis"]["height"], 0);
    assert_eq!(state["genesis"]["tx_id"], init.json()["genesis_tx_id"]);
    assert_eq!(
        state["genesis"]["value"]["identity"]["name"],
        "Acme Industries Ltd"
    );
    assert_eq!(state["shares"]["height"], 1);
    assert_eq!(state["rules"]["height"], 2);
    assert_eq!(state["rules"]["value"]["tie"], "reject");
    assert_eq!(state["shares"]["notarisation"]["id"], "notary-07");
    assert_eq!(state["shares"]["notarisation"]["name"], "Jane Roe");
    assert_eq!(
        state["shares"]["notarisation"]["address"],
        "12 High Street, London"
    );
    assert_eq!(
        state["shares"]["notarisation"]["at"],
        "2026-03-01T09:30:00Z"
    );

    let text = cli.run(&["show", "--company", "acme"]).ok();
    assert!(text.out().contains("Acme Industries Ltd"), "{}", text.out());
    assert!(
        text.out()
            .contains("Jane Roe (notary-07), 12 High Street, London at 2026-03-01T09:30:00Z"),
        "{}",
        text.out()
    );
}

#[test]
fn show_before_the_company_is_complete_says_what_is_missing() {
    let cli = Cli::new();
    cli.founded();
    let early = cli.run(&["show", "--company", "acme", "--at", "1"]);
    assert_eq!(early.code(), 2);
    assert!(
        early.err().contains("no voting-rules record is in force"),
        "{}",
        early.err()
    );
}

#[test]
fn shares_prints_each_holder_with_weight_and_signing_ability() {
    let cli = Cli::new();
    cli.founded();
    let shares = cli.run(&["shares", "--company", "acme"]).ok();
    let out = shares.out();
    assert!(
        out.contains("3 holder(s), 1000 share(s) in issue; one share, one vote"),
        "{out}"
    );
    assert!(
        out.contains("alice") && out.contains("500") && out.contains("can sign"),
        "{out}"
    );
    assert!(
        out.contains("carol") && out.contains("no key: cannot sign"),
        "{out}"
    );

    let json = cli
        .run(&["shares", "--company", "acme", "--json"])
        .ok()
        .json();
    assert_eq!(json["total_shares"], 1000);
    assert_eq!(json["holders"][0]["id"], "alice");
    assert_eq!(json["holders"][0]["weight"], 500);
    assert_eq!(json["holders"][2]["can_sign"], false);
}

#[test]
fn an_amendment_needs_the_id_in_force_and_history_lists_both() {
    let cli = Cli::new();
    cli.founded();
    let v1 = cli
        .run(&["shares", "--company", "acme", "--json"])
        .ok()
        .json()["record"]["tx_id"]
        .as_str()
        .expect("tx id")
        .to_owned();

    // Without --supersedes the amendment is stale.
    let stale = cli.with_notary(&[
        "publish-shares",
        "--company",
        "acme",
        "--file",
        "shares2.xml",
        "--signing-key",
        "k.key",
        "--timestamp",
        "3000",
    ]);
    assert_eq!(stale.code(), 2);
    assert!(stale.err().contains("stale amendment"), "{}", stale.err());

    let amended = cli
        .with_notary(&[
            "publish-shares",
            "--company",
            "acme",
            "--file",
            "shares2.xml",
            "--signing-key",
            "k.key",
            "--supersedes",
            &v1,
            "--timestamp",
            "3000",
            "--json",
        ])
        .ok();
    assert_eq!(amended.json()["height"], 3);
    assert_eq!(amended.json()["record"]["supersedes"], v1);

    let history = cli
        .run(&[
            "history",
            "--company",
            "acme",
            "--kind",
            "share-structure",
            "--json",
        ])
        .ok()
        .json();
    assert_eq!(history.as_array().map(Vec::len), Some(2));
    assert_eq!(history[1]["record"]["supersedes"], v1);

    // At height 2 the old register is still what was in force.
    let then = cli
        .run(&["shares", "--company", "acme", "--at", "2", "--json"])
        .ok()
        .json();
    assert_eq!(then["record"]["tx_id"], v1);
    assert_eq!(then["holders"].as_array().map(Vec::len), Some(3));
    let now = cli
        .run(&["shares", "--company", "acme", "--json"])
        .ok()
        .json();
    assert_eq!(now["holders"].as_array().map(Vec::len), Some(4));

    let bad_kind = cli.run(&["history", "--company", "acme", "--kind", "roll"]);
    assert_eq!(bad_kind.code(), 2);
}

#[test]
fn verify_structure_reports_intact_chains_and_a_break() {
    let cli = Cli::new();
    cli.founded();
    let verify = cli
        .run(&["verify-structure", "--company", "acme", "--json"])
        .ok();
    assert_eq!(verify.json()["intact"], true);
    assert_eq!(verify.json()["kinds"].as_array().map(Vec::len), Some(3));

    // Write a record around Irena, through Prunella's own API, that breaks the chain.
    let path = cli.dir.path().join("acme.chain");
    {
        use prunella_core::{Namespace, SchemaVersion, TransactionDraft, TxId};
        let store = prunella_store::LocalChainStore::open(&path).expect("open");
        let key = prunella_crypto::SigningKey::from_seed([9; 32]);
        let payload = irena_core::compose_record(
            irena_core::RecordKindV1::VotingRules,
            &irena_core::CompanyIdV1::new("acme").unwrap(),
            Some(TxId::from_hash(prunella_core::Hash::from_bytes([0x55; 32]))),
            &irena_core::NotarisationV1 {
                id: irena_core::NotaryIdV1::new("x").unwrap(),
                name: "X".to_owned(),
                address: None,
                at: irena_core::NotaryTimeV1::parse("2026-04-01T00:00:00Z").unwrap(),
                statement: None,
                source_digest: None,
            },
            RULES,
        )
        .expect("compose");
        let head = store.head().expect("head");
        let parent = store.get_block(head.height).expect("read").expect("block");
        let transaction = key.sign_transaction(TransactionDraft {
            namespace: Namespace::new("irena.rules.v1").unwrap(),
            schema_version: SchemaVersion(1),
            payload: payload.into_bytes(),
            signer: key.public_key(),
            nonce: 77,
        });
        let block = parent
            .header
            .child_draft(vec![transaction], 5000)
            .expect("draft")
            .build()
            .expect("build");
        store.append_block(block).expect("append");
    }
    let verify = cli.run(&["verify-structure", "--company", "acme", "--json"]);
    assert_eq!(verify.code(), 1, "{}", verify.err());
    assert_eq!(verify.json()["intact"], false);
    let rules_row = &verify.json()["kinds"][2];
    assert_eq!(rules_row["kind"], "voting-rules");
    assert_eq!(rules_row["intact"], false);
    assert!(
        rules_row["error"]
            .as_str()
            .unwrap()
            .contains("broken amendment chain")
    );
    // The other chains are still fine, and nothing was repaired.
    assert_eq!(verify.json()["kinds"][1]["intact"], true);
    assert_eq!(
        cli.run(&["verify-structure", "--company", "acme"]).code(),
        1
    );
    let text = cli.run(&["verify-structure", "--company", "acme"]);
    assert!(
        text.out().contains("nothing was repaired"),
        "{}",
        text.out()
    );
}

#[test]
fn every_notary_field_is_checked_and_bad_input_is_refused() {
    let cli = Cli::new();
    cli.founded();
    let base = [
        "publish-rules",
        "--company",
        "acme",
        "--file",
        "rules.xml",
        "--signing-key",
        "k.key",
    ];
    let head_before = cli
        .run(&["verify-structure", "--company", "acme", "--json"])
        .ok()
        .json();

    // Missing --notary-at: clap refuses before anything runs.
    let mut without_at: Vec<&str> = base.to_vec();
    without_at.extend_from_slice(&["--notary-id", "n", "--notary-name", "N"]);
    let run = cli.run(&without_at);
    assert_eq!(run.code(), 2);
    assert!(run.err().contains("--notary-at"), "{}", run.err());

    // A non-canonical time is refused by Irena.
    let mut bad_at: Vec<&str> = base.to_vec();
    bad_at.extend_from_slice(&[
        "--notary-id",
        "n",
        "--notary-name",
        "N",
        "--notary-at",
        "2026-03-01T09:30:00+01:00",
    ]);
    let run = cli.run(&bad_at);
    assert_eq!(run.code(), 2);
    assert!(run.err().contains("--notary-at"), "{}", run.err());

    // An invalid body is refused with the body's own issues.
    std::fs::write(
        cli.dir.path().join("bad.xml"),
        "<share-structure><holder id=\"a\" shares=\"0\"/></share-structure>",
    )
    .expect("write");
    let run = cli.with_notary(&[
        "publish-shares",
        "--company",
        "acme",
        "--file",
        "bad.xml",
        "--signing-key",
        "k.key",
        "--supersedes",
        "0000000000000000000000000000000000000000000000000000000000000000",
    ]);
    assert_eq!(run.code(), 2);

    let run = cli.run(&["show", "--company", "Acme Ltd"]);
    assert_eq!(run.code(), 2);
    assert!(run.err().contains("--company"), "{}", run.err());

    // None of that touched the chain.
    let head_after = cli
        .run(&["verify-structure", "--company", "acme", "--json"])
        .ok()
        .json();
    assert_eq!(head_before, head_after);
}

#[test]
fn the_export_shows_every_record_nested_and_readable() {
    let cli = Cli::new();
    cli.founded();
    let path = cli.dir.path().join("acme.chain");
    let store = prunella_store::LocalChainStore::open(&path).expect("open");
    let document =
        prunella_xml::export(&store, &prunella_xml::ExportRequest::full()).expect("export");
    let xml = prunella_xml::write_document(&document).expect("render");
    assert!(xml.contains("<payload encoding=\"xml\"><irena-record version=\"1.0\" kind=\"share-structure\" company=\"acme\">"), "{xml}");
    assert!(
        xml.contains("<holder id=\"carol\" shares=\"200\"/>"),
        "{xml}"
    );
    assert!(xml.contains("<notarisation id=\"notary-07\" name=\"Jane Roe\" address=\"12 High Street, London\" at=\"2026-03-01T09:30:00Z\"/>"), "{xml}");
}

// ---------------------------------------------------------------------------------
// Votes.
// ---------------------------------------------------------------------------------

/// A register whose holders' keys are the seeds 0x01 (alice) and 0x02 (bob); carol
/// has none.
fn keyed_register() -> String {
    let key = |seed: u8| prunella_crypto::SigningKey::from_seed([seed; 32]).public_key();
    format!(
        r#"<share-structure>
  <holder id="alice" key="{}" shares="500"/>
  <holder id="bob" key="{}" shares="300"/>
  <holder id="carol" shares="200"/>
</share-structure>
"#,
        key(1),
        key(2)
    )
}

impl Cli {
    /// A founded company whose holders can sign, and seed files for them.
    fn founded_for_voting(&self) {
        std::fs::write(self.dir.path().join("shares1.xml"), keyed_register()).expect("write");
        for seed in 1..=3u8 {
            std::fs::write(
                self.dir.path().join(format!("holder{seed}.key")),
                format!("{seed:02}").repeat(32),
            )
            .expect("key");
        }
        self.founded();
    }
    fn vote(&self, args: &[&str]) -> Run {
        let mut full = vec!["vote"];
        full.extend_from_slice(args);
        self.run(&full)
    }
    fn ballot(&self, voter: &str, choice: &str, seed: u8) -> String {
        let out = format!("{voter}.ballot");
        self.vote(&[
            "ballot",
            "--state",
            "v.state",
            "--voter",
            voter,
            "--choice",
            choice,
            "--signing-key",
            &format!("holder{seed}.key"),
            "--out",
            &out,
        ])
        .ok();
        out
    }
}

#[test]
fn a_vote_runs_end_to_end_and_the_freeze_holds_against_a_mid_vote_amendment() {
    let cli = Cli::new();
    cli.founded_for_voting();
    let digest = "d0".repeat(32);

    let new = cli
        .vote(&[
            "new",
            "--company",
            "acme",
            "--subject",
            "Approve the 2026 accounts",
            "--proposal-digest",
            &digest,
            "--state",
            "v.state",
            "--json",
        ])
        .ok();
    assert_eq!(new.json()["status"], "draft");

    let frozen = cli
        .vote(&["freeze", "--state", "v.state", "--at", "2", "--json"])
        .ok();
    assert_eq!(frozen.json()["status"], "frozen");
    assert_eq!(frozen.json()["snapshot"]["height"], 2);
    assert_eq!(
        frozen.json()["snapshot"]["electorate"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );
    let vote_id = frozen.json()["vote_id"].as_str().unwrap().to_owned();

    // Not open yet: a ballot is refused with both ends of the transition named.
    let alice = cli.ballot("alice", "yes", 1);
    let early = cli.vote(&["cast", "--state", "v.state", "--ballot", &alice]);
    assert_eq!(early.code(), 2);
    assert!(
        early
            .err()
            .contains("cannot cast a ballot in a vote that is frozen"),
        "{}",
        early.err()
    );

    cli.vote(&["open", "--state", "v.state"]).ok();
    cli.vote(&["cast", "--state", "v.state", "--ballot", &alice])
        .ok();
    let bob = cli.ballot("bob", "no", 2);
    cli.vote(&["cast", "--state", "v.state", "--ballot", &bob])
        .ok();

    // carol owns shares but registered no key.
    let carol = cli.ballot("carol", "yes", 3);
    let keyless = cli.vote(&["cast", "--state", "v.state", "--ballot", &carol]);
    assert_eq!(keyless.code(), 2);
    assert!(
        keyless.err().contains("no signing key"),
        "{}",
        keyless.err()
    );

    // alice again, changing her mind: the first ballot stands.
    let again = cli.ballot("alice", "no", 1);
    let duplicate = cli.vote(&["cast", "--state", "v.state", "--ballot", &again]);
    assert_eq!(duplicate.code(), 2);
    assert!(
        duplicate.err().contains("already cast"),
        "{}",
        duplicate.err()
    );

    // The register is amended mid-vote: bob sells everything to dave.
    let v1 = cli
        .run(&["shares", "--company", "acme", "--json"])
        .ok()
        .json()["record"]["tx_id"]
        .as_str()
        .unwrap()
        .to_owned();
    std::fs::write(
        cli.dir.path().join("shares-amended.xml"),
        keyed_register().replace("id=\"bob\"", "id=\"dave\""),
    )
    .expect("write");
    cli.with_notary(&[
        "publish-shares",
        "--company",
        "acme",
        "--file",
        "shares-amended.xml",
        "--signing-key",
        "k.key",
        "--supersedes",
        &v1,
        "--timestamp",
        "3000",
    ])
    .ok();
    let now = cli
        .run(&["shares", "--company", "acme", "--json"])
        .ok()
        .json();
    assert_eq!(now["holders"][2]["id"], "dave", "the chain moved on");

    let status = cli
        .vote(&["status", "--state", "v.state", "--json"])
        .ok()
        .json();
    assert_eq!(status["vote_id"], vote_id, "the vote did not");
    assert_eq!(status["ballots"], 2);

    cli.vote(&["close", "--state", "v.state"]).ok();
    let evaluated = cli.vote(&["evaluate", "--state", "v.state", "--json"]).ok();
    assert_eq!(evaluated.json()["summary"]["outcome"], "accepted");
    assert_eq!(evaluated.json()["summary"]["yes_weight"], 500);
    assert_eq!(evaluated.json()["summary"]["no_weight"], 300);
    assert_eq!(
        evaluated.json()["summary"]["total_weight"],
        1000,
        "the frozen electorate, not the amended one"
    );

    let finalized = cli
        .vote(&[
            "finalize",
            "--state",
            "v.state",
            "--signing-key",
            "k.key",
            "--timestamp",
            "4000",
            "--json",
        ])
        .ok();
    let tx = finalized.json()["tx_id"].as_str().unwrap().to_owned();
    assert_eq!(finalized.json()["height"], 4);
    assert_eq!(finalized.json()["record"]["snapshot"]["height"], 2);

    // Verification from nothing but the chain and the id.
    let verified = cli.vote(&["verify", "--tx", &tx]).ok();
    assert!(
        verified.out().contains("ResultReproduces"),
        "{}",
        verified.out()
    );
    assert!(
        verified.out().contains("exactly what the chain says"),
        "{}",
        verified.out()
    );
    let report = cli.vote(&["verify", "--tx", &tx, "--json"]).ok().json();
    assert_eq!(report["checks"].as_array().map(Vec::len), Some(9));
    assert!(
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["passed"] == true),
        "{report}"
    );

    // A finalised vote is finished.
    let again = cli.vote(&["finalize", "--state", "v.state", "--signing-key", "k.key"]);
    assert_eq!(again.code(), 2);
    assert!(
        again
            .err()
            .contains("cannot finalize a vote that is finalized"),
        "{}",
        again.err()
    );
}

#[test]
fn a_corrupted_record_fails_verification_by_name() {
    let cli = Cli::new();
    cli.founded_for_voting();
    let digest = "d0".repeat(32);
    cli.vote(&[
        "new",
        "--company",
        "acme",
        "--subject",
        "x",
        "--proposal-digest",
        &digest,
        "--state",
        "v.state",
    ])
    .ok();
    cli.vote(&["freeze", "--state", "v.state", "--at", "2"])
        .ok();
    cli.vote(&["open", "--state", "v.state"]).ok();
    let alice = cli.ballot("alice", "yes", 1);
    cli.vote(&["cast", "--state", "v.state", "--ballot", &alice])
        .ok();
    cli.vote(&["close", "--state", "v.state"]).ok();
    cli.vote(&["evaluate", "--state", "v.state"]).ok();
    let tx = cli
        .vote(&[
            "finalize",
            "--state",
            "v.state",
            "--signing-key",
            "k.key",
            "--timestamp",
            "3000",
            "--json",
        ])
        .ok()
        .json()["tx_id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Take the record off the chain, flip one byte inside the ballot's signature, and
    // put the corrupted copy on the chain as a new transaction.
    let path = cli.dir.path().join("acme.chain");
    let corrupted = {
        use prunella_canonical::Canonical;
        use prunella_core::{Namespace, SchemaVersion, TransactionDraft, TxId};
        let store = prunella_store::LocalChainStore::open(&path).expect("open");
        let located = store
            .get_transaction(&TxId::from_hex(&tx).unwrap())
            .unwrap()
            .unwrap();
        let mut record =
            irena_vote::FinalVoteRecordV1::from_canonical_bytes(&located.transaction.payload)
                .unwrap();
        let mut signature = record.ballots[0].signature.to_bytes();
        signature[5] ^= 0x01;
        record.ballots[0].signature = prunella_core::Signature::from_bytes(signature);
        let signer = prunella_crypto::SigningKey::from_seed([9; 32]);
        let head = store.head().unwrap();
        let parent = store.get_block(head.height).unwrap().unwrap();
        let transaction = signer.sign_transaction(TransactionDraft {
            namespace: Namespace::new("irena.vote.v1").unwrap(),
            schema_version: SchemaVersion(1),
            payload: record.canonical_bytes(),
            signer: signer.public_key(),
            nonce: 77,
        });
        let id = transaction.id;
        let block = parent
            .header
            .child_draft(vec![transaction], 5000)
            .unwrap()
            .build()
            .unwrap();
        store.append_block(block).unwrap();
        id.to_string()
    };

    let failed = cli.vote(&["verify", "--tx", &corrupted]);
    assert_eq!(failed.code(), 1, "{}", failed.err());
    assert!(
        failed.out().contains("FAIL BallotsVerify"),
        "{}",
        failed.out()
    );
    assert!(failed.out().contains("does not verify"), "{}", failed.out());
    // The ballot's digest moved with its signature, so the commitment no longer
    // derives either: both findings are reported, not just the first.
    assert!(
        failed.out().contains("FAIL CommitmentDerives"),
        "{}",
        failed.out()
    );
    assert!(
        failed.out().contains("2 check(s) failed"),
        "{}",
        failed.out()
    );
    // The genuine record still verifies: Prunella kept both, and only one is true.
    assert_eq!(cli.vote(&["verify", "--tx", &tx]).code(), 0);

    let missing = cli.vote(&["verify", "--tx", &"00".repeat(32)]);
    assert_eq!(missing.code(), 2);
}

#[test]
fn a_vote_needs_a_frozen_company_and_a_rejected_motion_exits_one() {
    let cli = Cli::new();
    cli.founded_for_voting();
    let digest = "d0".repeat(32);
    cli.vote(&[
        "new",
        "--company",
        "acme",
        "--subject",
        "x",
        "--proposal-digest",
        &digest,
        "--state",
        "v.state",
    ])
    .ok();

    // Nothing to vote on before the freeze.
    let early = cli.vote(&[
        "ballot",
        "--state",
        "v.state",
        "--voter",
        "alice",
        "--choice",
        "yes",
        "--signing-key",
        "holder1.key",
        "--out",
        "a.ballot",
    ]);
    assert_eq!(early.code(), 2);
    // Freezing where the company is incomplete is refused and leaves a draft.
    let incomplete = cli.vote(&["freeze", "--state", "v.state", "--at", "1"]);
    assert_eq!(incomplete.code(), 2);
    assert_eq!(
        cli.vote(&["status", "--state", "v.state", "--json"])
            .ok()
            .json()["status"],
        "draft"
    );

    cli.vote(&["freeze", "--state", "v.state"]).ok();
    cli.vote(&["open", "--state", "v.state"]).ok();
    let bob = cli.ballot("bob", "no", 2);
    cli.vote(&["cast", "--state", "v.state", "--ballot", &bob])
        .ok();
    let bad_choice = cli.vote(&[
        "ballot",
        "--state",
        "v.state",
        "--voter",
        "alice",
        "--choice",
        "maybe",
        "--signing-key",
        "holder1.key",
        "--out",
        "a.ballot",
    ]);
    assert_eq!(bad_choice.code(), 2);
    cli.vote(&["close", "--state", "v.state"]).ok();
    // 300 no against a 1/2-of-1000 quorum with no yes: rejected, exit 1.
    let evaluated = cli.vote(&["evaluate", "--state", "v.state"]);
    assert_eq!(evaluated.code(), 1, "{}", evaluated.err());
    assert!(evaluated.out().contains("Rejected"), "{}", evaluated.out());
}
