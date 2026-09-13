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
