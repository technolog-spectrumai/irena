//! End-to-end: found a company, amend its parts, show it at heights, verify it, and
//! run a vote on it.

use std::process::{Command, Output};
use tempfile::TempDir;

const RULES: &str = r#"<voting-rules version="1.0">
  <weight type="electorate"/>
  <exclusions enabled="true"/>
  <quorum type="fraction" numerator="1" denominator="2" basis="total-electorate"/>
  <threshold type="simple-majority" basis="votes-cast"/>
  <abstentions treatment="exclude"/>
  <tie treatment="reject"/>
</voting-rules>
"#;

/// alice (500, key from seed 0x01), bob (300, seed 0x02), carol (200, no key).
fn register() -> String {
    let key = |seed: u8| prunella_crypto::SigningKey::from_seed([seed; 32]).public_key();
    format!(
        r#"<share-structure>
  <holder id="alice" key="{}" name="Alice Smith" shares="500"/>
  <holder id="bob" key="{}" shares="300"/>
  <holder id="carol" shares="200"/>
</share-structure>
"#,
        key(1),
        key(2)
    )
}

/// The whole company, as founded.
fn genesis() -> String {
    format!(
        r#"<company-genesis>
  <identity name="Acme Industries Ltd" jurisdiction="gb" registered-number="01234567"/>
  {}
  <governance>
  {RULES}
  </governance>
</company-genesis>
"#,
        register()
    )
}

const IDENTITY_V2: &str = r#"<identity name="Acme Industries plc" jurisdiction="gb" registered-number="01234567"/>
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
        std::fs::write(dir.path().join("genesis.xml"), genesis()).expect("write");
        std::fs::write(dir.path().join("identity2.xml"), IDENTITY_V2).expect("write");
        std::fs::write(
            dir.path().join("shares2.xml"),
            register().replace("id=\"bob\"", "id=\"dave\""),
        )
        .expect("write");
        std::fs::write(
            dir.path().join("rules2.xml"),
            RULES.replace("reject", "accept"),
        )
        .expect("write");
        std::fs::write(dir.path().join("k.key"), "09".repeat(32)).expect("key");
        for seed in 1..=3u8 {
            std::fs::write(
                dir.path().join(format!("holder{seed}.key")),
                format!("{seed:02}").repeat(32),
            )
            .expect("key");
        }
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
    /// A founded company: identity, register and rules all in block 0.
    fn founded(&self) -> Run {
        self.with_notary(&[
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
        .ok()
    }
    /// The transaction currently providing a part.
    fn provider(&self, part: &str) -> String {
        self.run(&["show", "--json"]).ok().json()[part]["tx_id"]
            .as_str()
            .expect("tx id")
            .to_owned()
    }
    fn amend(&self, command: &str, file: &str, part: &str, timestamp: &str) -> Run {
        let supersedes = self.provider(part);
        self.with_notary(&[
            command,
            "--file",
            file,
            "--signing-key",
            "k.key",
            "--supersedes",
            &supersedes,
            "--timestamp",
            timestamp,
            "--json",
        ])
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
fn init_founds_the_whole_company_in_genesis() {
    let cli = Cli::new();
    let init = cli.founded();
    assert_eq!(init.json()["company"], "acme");
    let genesis_tx = init.json()["genesis_tx_id"].as_str().unwrap().to_owned();

    let state = cli.run(&["show", "--json"]).ok().json();
    assert_eq!(state["company"], "acme");
    assert_eq!(state["genesis_height"], 0);
    assert_eq!(state["genesis_tx_id"], genesis_tx);
    for part in ["identity", "shares", "rules"] {
        assert_eq!(state[part]["height"], 0, "{part}");
        assert_eq!(
            state[part]["tx_id"], genesis_tx,
            "{part} is provided by the genesis"
        );
        assert!(state[part]["supersedes"].is_null(), "{part}");
        assert_eq!(state[part]["notarisation"]["id"], "notary-07");
        assert_eq!(state[part]["notarisation"]["name"], "Jane Roe");
        assert_eq!(
            state[part]["notarisation"]["address"],
            "12 High Street, London"
        );
        assert_eq!(state[part]["notarisation"]["at"], "2026-03-01T09:30:00Z");
    }
    assert_eq!(state["identity"]["value"]["name"], "Acme Industries Ltd");
    assert_eq!(state["shares"]["value"].as_array().map(Vec::len), Some(3));
    assert_eq!(state["rules"]["value"]["tie"], "reject");
    assert_eq!(state["applied"].as_array().map(Vec::len), Some(1));

    let text = cli.run(&["show"]).ok();
    assert!(text.out().contains("Acme Industries Ltd"), "{}", text.out());
    assert!(text.out().contains("founded at height 0"), "{}", text.out());
    assert!(
        text.out()
            .contains("Jane Roe (notary-07), 12 High Street, London at 2026-03-01T09:30:00Z"),
        "{}",
        text.out()
    );
    assert!(text.out().contains("1 record(s) applied"), "{}", text.out());
}

#[test]
fn a_plain_chain_has_no_company() {
    let cli = Cli::new();
    let path = cli.dir.path().join("acme.chain");
    prunella_store::LocalChainStore::init_genesis(
        &path,
        prunella_core::GenesisSpec::new(prunella_core::NetworkId::new("plain").unwrap()),
    )
    .expect("plain chain");
    let show = cli.run(&["show"]);
    assert_eq!(show.code(), 2);
    assert!(
        show.err().contains("no company is founded"),
        "{}",
        show.err()
    );
}

#[test]
fn shares_prints_each_holder_with_weight_and_signing_ability() {
    let cli = Cli::new();
    cli.founded();
    let shares = cli.run(&["shares"]).ok();
    let out = shares.out();
    assert!(out.contains("3 holder(s), 1000 share(s) in issue; one share, one vote; total weight 1000; 2 can sign"), "{out}");
    assert!(
        out.contains("alice") && out.contains("500") && out.contains("can sign"),
        "{out}"
    );
    assert!(
        out.contains("carol") && out.contains("no key: cannot sign"),
        "{out}"
    );
    let json = cli.run(&["shares", "--json"]).ok().json();
    assert_eq!(json["total_shares"], 1000);
    assert_eq!(json["holders"][0]["id"], "alice");
    assert_eq!(json["holders"][0]["weight"], 500);
    assert_eq!(json["holders"][2]["can_sign"], false);
}

#[test]
fn each_part_amends_on_its_own_and_past_heights_keep_their_company() {
    let cli = Cli::new();
    let genesis_tx = cli.founded().json()["genesis_tx_id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Without --supersedes clap refuses; with a stale one Irena refuses.
    let missing = cli.with_notary(&[
        "publish-shares",
        "--file",
        "shares2.xml",
        "--signing-key",
        "k.key",
    ]);
    assert_eq!(missing.code(), 2);
    assert!(missing.err().contains("--supersedes"), "{}", missing.err());

    let shares = cli
        .amend("publish-shares", "shares2.xml", "shares", "1000")
        .ok();
    assert_eq!(shares.json()["height"], 1);
    assert_eq!(shares.json()["record"]["supersedes"], genesis_tx);
    let shares_tx = shares.json()["tx_id"].as_str().unwrap().to_owned();

    let stale = cli.with_notary(&[
        "publish-shares",
        "--file",
        "shares2.xml",
        "--signing-key",
        "k.key",
        "--supersedes",
        &genesis_tx,
        "--timestamp",
        "2000",
    ]);
    assert_eq!(stale.code(), 2);
    assert!(stale.err().contains("stale amendment"), "{}", stale.err());
    assert!(stale.err().contains(&shares_tx), "{}", stale.err());

    let identity = cli
        .amend("publish-identity", "identity2.xml", "identity", "2000")
        .ok();
    assert_eq!(
        identity.json()["record"]["supersedes"],
        genesis_tx,
        "identity was still the genesis's"
    );
    let rules = cli
        .amend("publish-rules", "rules2.xml", "rules", "3000")
        .ok();
    assert_eq!(rules.json()["record"]["supersedes"], genesis_tx);

    let now = cli.run(&["show", "--json"]).ok().json();
    assert_eq!(now["identity"]["value"]["name"], "Acme Industries plc");
    assert_eq!(now["shares"]["value"][2]["id"], "dave");
    assert_eq!(now["rules"]["value"]["tie"], "accept");
    assert_eq!(now["applied"].as_array().map(Vec::len), Some(4));

    // The company at height 1: the new register, the old name and rules.
    let then = cli.run(&["show", "--at", "1", "--json"]).ok().json();
    assert_eq!(then["shares"]["tx_id"], shares_tx);
    assert_eq!(then["identity"]["value"]["name"], "Acme Industries Ltd");
    assert_eq!(then["rules"]["tx_id"], genesis_tx);
    assert_eq!(then["applied"].as_array().map(Vec::len), Some(2));
    let founded = cli.run(&["shares", "--at", "0", "--json"]).ok().json();
    assert_eq!(founded["holders"][1]["id"], "bob");

    // History of a part: the genesis, then its amendments.
    let history = cli
        .run(&["history", "--kind", "share-structure", "--json"])
        .ok()
        .json();
    assert_eq!(history.as_array().map(Vec::len), Some(2));
    assert_eq!(history[0]["record"]["body"]["kind"], "company-genesis");
    assert_eq!(history[1]["tx_id"], shares_tx);
    let text = cli.run(&["history", "--kind", "voting-rules"]).ok();
    assert!(
        text.out()
            .contains("2 record(s) have provided voting-rules"),
        "{}",
        text.out()
    );
    assert_eq!(cli.run(&["history", "--kind", "company-genesis"]).code(), 2);
    assert_eq!(cli.run(&["history", "--kind", "roll"]).code(), 2);
}

#[test]
fn verify_structure_reports_an_intact_company_and_a_break() {
    let cli = Cli::new();
    cli.founded();
    cli.amend("publish-shares", "shares2.xml", "shares", "1000")
        .ok();
    let verify = cli.run(&["verify-structure", "--json"]).ok();
    assert_eq!(verify.json()["intact"], true);
    assert_eq!(verify.json()["applied"], 2);
    assert_eq!(verify.json()["providers"][1]["kind"], "share-structure");
    assert_eq!(verify.json()["providers"][1]["versions"], 2);

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
    let verify = cli.run(&["verify-structure", "--json"]);
    assert_eq!(verify.code(), 1, "{}", verify.err());
    assert_eq!(verify.json()["intact"], false);
    assert!(
        verify.json()["error"]
            .as_str()
            .unwrap()
            .contains("broken amendment chain")
    );
    let text = cli.run(&["verify-structure"]);
    assert!(
        text.out().contains("BROKEN") && text.out().contains("nothing was repaired"),
        "{}",
        text.out()
    );
    // Before the break the company still reconstructs; nothing was repaired.
    assert_eq!(cli.run(&["verify-structure", "--at", "1"]).code(), 0);
    assert_eq!(cli.run(&["show"]).code(), 2);
}

#[test]
fn every_notary_field_is_checked_and_bad_input_is_refused() {
    let cli = Cli::new();
    cli.founded();
    let genesis_tx = cli.provider("rules");
    let base = [
        "publish-rules",
        "--file",
        "rules2.xml",
        "--signing-key",
        "k.key",
        "--supersedes",
        &genesis_tx,
    ];
    let before = cli.run(&["show", "--json"]).ok().json();

    let mut without_at: Vec<&str> = base.to_vec();
    without_at.extend_from_slice(&["--notary-id", "n", "--notary-name", "N"]);
    let run = cli.run(&without_at);
    assert_eq!(run.code(), 2);
    assert!(run.err().contains("--notary-at"), "{}", run.err());

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

    std::fs::write(
        cli.dir.path().join("bad.xml"),
        "<share-structure><holder id=\"a\" shares=\"0\"/></share-structure>",
    )
    .expect("write");
    let run = cli.with_notary(&[
        "publish-shares",
        "--file",
        "bad.xml",
        "--signing-key",
        "k.key",
        "--supersedes",
        &genesis_tx,
    ]);
    assert_eq!(run.code(), 2);
    assert!(run.err().contains("zero shares"), "{}", run.err());

    // A genesis that is not a whole company is refused at init.
    let other = Cli::new();
    std::fs::write(
        other.dir.path().join("half.xml"),
        "<company-genesis><identity name=\"Half\"/></company-genesis>",
    )
    .expect("write");
    let run = other.with_notary(&[
        "init",
        "--network",
        "n",
        "--company",
        "half",
        "--genesis",
        "half.xml",
        "--signing-key",
        "k.key",
    ]);
    assert_eq!(run.code(), 2);
    assert!(
        run.err().contains("share-structure") && run.err().contains("governance"),
        "{}",
        run.err()
    );
    assert_eq!(other.run(&["show"]).code(), 2, "no chain was created");

    assert_eq!(
        cli.run(&["show", "--json"]).ok().json(),
        before,
        "nothing touched the chain"
    );
}

#[test]
fn the_export_shows_the_whole_company_nested_and_readable() {
    let cli = Cli::new();
    cli.founded();
    let store =
        prunella_store::LocalChainStore::open(cli.dir.path().join("acme.chain")).expect("open");
    let document =
        prunella_xml::export(&store, &prunella_xml::ExportRequest::full()).expect("export");
    let xml = prunella_xml::write_document(&document).expect("render");
    assert!(xml.contains("<payload encoding=\"xml\"><irena-record version=\"1.0\" kind=\"company-genesis\" company=\"acme\">"), "{xml}");
    assert!(
        xml.contains("<holder id=\"carol\" shares=\"200\"/>"),
        "{xml}"
    );
    assert!(xml.contains("<governance>"), "{xml}");
    assert!(xml.contains("<notarisation id=\"notary-07\" name=\"Jane Roe\" address=\"12 High Street, London\" at=\"2026-03-01T09:30:00Z\"/>"), "{xml}");
}

// ---------------------------------------------------------------------------------
// Votes.
// ---------------------------------------------------------------------------------

#[test]
fn a_vote_runs_end_to_end_and_the_freeze_holds_against_a_mid_vote_amendment() {
    let cli = Cli::new();
    cli.founded();
    let digest = "d0".repeat(32);

    let new = cli
        .vote(&[
            "new",
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

    let frozen = cli.vote(&["freeze", "--state", "v.state", "--json"]).ok();
    assert_eq!(frozen.json()["status"], "frozen");
    assert_eq!(
        frozen.json()["company"],
        "acme",
        "the company comes from the chain"
    );
    assert_eq!(frozen.json()["snapshot"]["height"], 0);
    assert_eq!(
        frozen.json()["snapshot"]["electorate"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );
    let vote_id = frozen.json()["vote_id"].as_str().unwrap().to_owned();

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

    let carol = cli.ballot("carol", "yes", 3);
    let keyless = cli.vote(&["cast", "--state", "v.state", "--ballot", &carol]);
    assert_eq!(keyless.code(), 2);
    assert!(
        keyless.err().contains("no signing key"),
        "{}",
        keyless.err()
    );

    let again = cli.ballot("alice", "no", 1);
    let duplicate = cli.vote(&["cast", "--state", "v.state", "--ballot", &again]);
    assert_eq!(duplicate.code(), 2);
    assert!(
        duplicate.err().contains("already cast"),
        "{}",
        duplicate.err()
    );

    // The register is amended mid-vote: bob's shares go to dave.
    cli.amend("publish-shares", "shares2.xml", "shares", "3000")
        .ok();
    assert_eq!(
        cli.run(&["shares", "--json"]).ok().json()["holders"][2]["id"],
        "dave",
        "the chain moved on"
    );
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
    assert_eq!(finalized.json()["height"], 2);
    assert_eq!(finalized.json()["record"]["snapshot"]["height"], 0);

    let verified = cli.vote(&["verify", "--tx", &tx]).ok();
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
    cli.founded();
    let digest = "d0".repeat(32);
    cli.vote(&[
        "new",
        "--subject",
        "x",
        "--proposal-digest",
        &digest,
        "--state",
        "v.state",
    ])
    .ok();
    cli.vote(&["freeze", "--state", "v.state"]).ok();
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
    assert_eq!(
        cli.vote(&["verify", "--tx", &tx]).code(),
        0,
        "the genuine record still verifies"
    );
    assert_eq!(cli.vote(&["verify", "--tx", &"00".repeat(32)]).code(), 2);
}

#[test]
fn a_vote_needs_a_frozen_company_and_a_rejected_motion_exits_one() {
    let cli = Cli::new();
    cli.founded();
    let digest = "d0".repeat(32);
    cli.vote(&[
        "new",
        "--subject",
        "x",
        "--proposal-digest",
        &digest,
        "--state",
        "v.state",
    ])
    .ok();
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
    let evaluated = cli.vote(&["evaluate", "--state", "v.state"]);
    assert_eq!(evaluated.code(), 1, "{}", evaluated.err());
    assert!(evaluated.out().contains("Rejected"), "{}", evaluated.out());
}
