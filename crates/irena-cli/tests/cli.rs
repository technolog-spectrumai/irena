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

/// The channel set: shareholders (share register, collective, `rules`); board (chair
/// seed 5 weight 2, dir-a seed 6, dir-b seed 7; collective, simple majority, no
/// quorum); ceo (chair, individual).
fn channels(rules: &str) -> String {
    let key = |seed: u8| prunella_crypto::SigningKey::from_seed([seed; 32]).public_key();
    format!(
        r#"<decision-channels>
  <channel id="shareholders" mode="collective">
    <actors source="share-register"/>
    {rules}
  </channel>
  <channel id="board" mode="collective">
    <actors source="roster">
      <member id="chair" key="{}" name="M. Chen" weight="2"/>
      <member id="dir-a" key="{}"/>
      <member id="dir-b" key="{}"/>
    </actors>
    <voting-rules version="1.0">
      <weight type="electorate"/>
      <exclusions enabled="false"/>
      <quorum type="none"/>
      <threshold type="simple-majority" basis="votes-cast"/>
      <abstentions treatment="exclude"/>
      <tie treatment="reject"/>
    </voting-rules>
  </channel>
  <channel id="ceo" mode="individual">
    <actors source="roster">
      <member id="chair" key="{}" name="M. Chen"/>
    </actors>
  </channel>
</decision-channels>
"#,
        key(5),
        key(6),
        key(7),
        key(5)
    )
}

/// The whole company, as founded.
fn genesis() -> String {
    format!(
        r#"<company-genesis>
  <identity name="Acme Industries Ltd" jurisdiction="gb" registered-number="01234567"/>
  {}
  <governance>
  {}
  </governance>
</company-genesis>
"#,
        register(),
        channels(RULES)
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
        std::fs::write(dir.path().join("shares1.xml"), register()).expect("write");
        std::fs::write(
            dir.path().join("shares2.xml"),
            register().replace("id=\"bob\"", "id=\"dave\""),
        )
        .expect("write");
        std::fs::write(
            dir.path().join("channels2.xml"),
            channels(&RULES.replace("reject", "accept")),
        )
        .expect("write");
        std::fs::write(dir.path().join("k.key"), "09".repeat(32)).expect("key");
        for seed in [1u8, 2, 3, 5, 6, 7] {
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
    for part in ["identity", "shares", "channels"] {
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
    assert_eq!(
        state["channels"]["value"][2]["mode"]["rules"]["tie"],
        "reject"
    );
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
        .amend("publish-channels", "channels2.xml", "channels", "3000")
        .ok();
    assert_eq!(rules.json()["record"]["supersedes"], genesis_tx);

    let now = cli.run(&["show", "--json"]).ok().json();
    assert_eq!(now["identity"]["value"]["name"], "Acme Industries plc");
    assert_eq!(now["shares"]["value"][2]["id"], "dave");
    assert_eq!(
        now["channels"]["value"][2]["mode"]["rules"]["tie"],
        "accept"
    );
    assert_eq!(now["applied"].as_array().map(Vec::len), Some(4));

    // The company at height 1: the new register, the old name and rules.
    let then = cli.run(&["show", "--at", "1", "--json"]).ok().json();
    assert_eq!(then["shares"]["tx_id"], shares_tx);
    assert_eq!(then["identity"]["value"]["name"], "Acme Industries Ltd");
    assert_eq!(then["channels"]["tx_id"], genesis_tx);
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
    let text = cli.run(&["history", "--kind", "decision-channels"]).ok();
    assert!(
        text.out()
            .contains("2 record(s) have provided decision-channels"),
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
            irena_core::RecordKindV1::DecisionChannels,
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
            &channels(RULES),
        )
        .expect("compose");
        let head = store.head().expect("head");
        let parent = store.get_block(head.height).expect("read").expect("block");
        let transaction = key.sign_transaction(TransactionDraft {
            namespace: Namespace::new("irena.channels.v1").unwrap(),
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
    let genesis_tx = cli.provider("channels");
    let base = [
        "publish-channels",
        "--file",
        "channels2.xml",
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

    let frozen = cli
        .vote(&[
            "freeze",
            "--state",
            "v.state",
            "--channel",
            "shareholders",
            "--json",
        ])
        .ok();
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
    assert_eq!(report["checks"].as_array().map(Vec::len), Some(10));
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
    cli.vote(&["freeze", "--state", "v.state", "--channel", "shareholders"])
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

    cli.vote(&["freeze", "--state", "v.state", "--channel", "shareholders"])
        .ok();
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

// ---------------------------------------------------------------------------------
// Meetings.
// ---------------------------------------------------------------------------------

impl Cli {
    fn meeting(&self, args: &[&str]) -> Run {
        let mut full = vec!["meeting"];
        full.extend_from_slice(args);
        self.run(&full)
    }
    /// A drafted meeting: one informational item and two vote items.
    fn agenda(&self) {
        self.meeting(&[
            "new",
            "--channel",
            "shareholders",
            "--title",
            "Annual General Meeting 2026",
            "--scheduled-at",
            "2026-06-01T10:00:00Z",
            "--notice-digest",
            &"a0".repeat(32),
            "--state",
            "m.state",
        ])
        .ok();
        self.meeting(&[
            "add-item",
            "--state",
            "m.state",
            "--title",
            "Report of the directors",
            "--document-digest",
            &"11".repeat(32),
        ])
        .ok();
        self.meeting(&[
            "add-item",
            "--state",
            "m.state",
            "--title",
            "Approve the 2026 accounts",
            "--proposal-digest",
            &"22".repeat(32),
        ])
        .ok();
        self.meeting(&[
            "add-item",
            "--state",
            "m.state",
            "--title",
            "Re-appoint the auditor",
            "--proposal-digest",
            &"33".repeat(32),
        ])
        .ok();
    }
    fn meeting_ballot(&self, item: &str, voter: &str, choice: &str, seed: u8) -> String {
        let out = format!("{voter}-{item}.ballot");
        self.meeting(&[
            "ballot",
            "--state",
            "m.state",
            "--item",
            item,
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
fn a_meeting_runs_end_to_end_with_several_votes_and_verifies() {
    let cli = Cli::new();
    cli.founded();
    cli.agenda();

    let drafted = cli
        .meeting(&["show", "--state", "m.state", "--json"])
        .ok()
        .json();
    assert_eq!(drafted["status"], "draft");
    assert_eq!(drafted["items"].as_array().map(Vec::len), Some(3));
    assert_eq!(drafted["items"][0]["kind"], "informational");
    assert_eq!(drafted["items"][1]["kind"], "vote");
    assert!(drafted["meeting_id"].is_null());

    let convened = cli
        .with_notary(&[
            "meeting",
            "convene",
            "--state",
            "m.state",
            "--signing-key",
            "k.key",
            "--timestamp",
            "1000",
            "--json",
        ])
        .ok();
    assert_eq!(convened.json()["status"], "convened");
    assert_eq!(convened.json()["company"], "acme");
    let meeting_id = convened.json()["meeting_id"].as_str().unwrap().to_owned();
    assert_eq!(convened.json()["convened"]["height"], 1);

    // Ballots need an open meeting.
    let early = cli.meeting(&[
        "ballot",
        "--state",
        "m.state",
        "--item",
        "2",
        "--voter",
        "alice",
        "--choice",
        "yes",
        "--signing-key",
        "holder1.key",
        "--out",
        "x.ballot",
    ]);
    assert_eq!(early.code(), 2);

    let opened = cli.meeting(&["open", "--state", "m.state", "--json"]).ok();
    assert_eq!(opened.json()["status"], "open");
    assert_eq!(opened.json()["opened_at"], 1);
    // Two votes, with different ids: each froze the company on its own.
    let two = opened.json()["items"][1]["vote_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let three = opened.json()["items"][2]["vote_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(two, three);
    assert!(
        opened.json()["items"][0]["vote_id"].is_null(),
        "informational"
    );

    // Item 2 passes, item 3 fails.
    let alice2 = cli.meeting_ballot("2", "alice", "yes", 1);
    let bob2 = cli.meeting_ballot("2", "bob", "no", 2);
    let alice3 = cli.meeting_ballot("3", "alice", "no", 1);
    cli.meeting(&[
        "cast", "--state", "m.state", "--item", "2", "--ballot", &alice2,
    ])
    .ok();
    cli.meeting(&[
        "cast", "--state", "m.state", "--item", "2", "--ballot", &bob2,
    ])
    .ok();
    cli.meeting(&[
        "cast", "--state", "m.state", "--item", "3", "--ballot", &alice3,
    ])
    .ok();

    // A ballot for item 2 is not a ballot for item 3.
    let wrong = cli.meeting(&[
        "cast", "--state", "m.state", "--item", "3", "--ballot", &alice2,
    ]);
    assert_eq!(wrong.code(), 2);
    assert!(wrong.err().contains("item 3"), "{}", wrong.err());
    // Nor is an informational item votable.
    let info = cli.meeting(&[
        "cast", "--state", "m.state", "--item", "1", "--ballot", &alice2,
    ]);
    assert_eq!(info.code(), 2);
    assert!(info.err().contains("informational"), "{}", info.err());

    // The register is amended mid-meeting; the frozen votes do not notice.
    cli.amend("publish-shares", "shares2.xml", "shares", "1500")
        .ok();

    let closed = cli.meeting(&["close", "--state", "m.state", "--json"]).ok();
    assert_eq!(closed.json()["status"], "closed");
    assert_eq!(closed.json()["items"][1]["outcome"], "accepted");
    assert_eq!(closed.json()["items"][2]["outcome"], "rejected");
    assert_eq!(closed.json()["items"][1]["ballots"], 2);

    let finalized = cli
        .with_notary(&[
            "meeting",
            "finalize",
            "--state",
            "m.state",
            "--signing-key",
            "k.key",
            "--timestamp",
            "2000",
            "--json",
        ])
        .ok();
    let tx = finalized.json()["tx_id"].as_str().unwrap().to_owned();
    assert_eq!(finalized.json()["record"]["meeting_id"], meeting_id);
    assert_eq!(
        finalized.json()["record"]["items"].as_array().map(Vec::len),
        Some(3)
    );

    // Verification: the meeting's own checks plus every vote's.
    let verified = cli.meeting(&["verify", "--tx", &tx]).ok();
    assert!(
        verified.out().contains("ok   VotesBelong"),
        "{}",
        verified.out()
    );
    assert!(
        verified.out().contains("exactly what the chain says"),
        "{}",
        verified.out()
    );
    let report = cli.meeting(&["verify", "--tx", &tx, "--json"]).ok().json();
    assert_eq!(report["checks"].as_array().map(Vec::len), Some(9));
    assert_eq!(report["votes"].as_array().map(Vec::len), Some(2));
    assert!(
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["passed"] == true),
        "{report}"
    );

    // Each vote also verifies on its own, without the meeting.
    for index in [1, 2] {
        let vote_tx = finalized.json()["record"]["items"][index]["vote_tx_id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(cli.vote(&["verify", "--tx", &vote_tx]).code(), 0);
    }

    // A finalised meeting is finished.
    let again = cli.with_notary(&[
        "meeting",
        "finalize",
        "--state",
        "m.state",
        "--signing-key",
        "k.key",
    ]);
    assert_eq!(again.code(), 2);
    assert!(
        again
            .err()
            .contains("cannot finalize a meeting that is finalized"),
        "{}",
        again.err()
    );
}

#[test]
fn a_meeting_with_no_votes_is_still_a_meeting() {
    let cli = Cli::new();
    cli.founded();
    cli.meeting(&[
        "new",
        "--channel",
        "shareholders",
        "--title",
        "Information session",
        "--scheduled-at",
        "2026-06-01T10:00:00Z",
        "--state",
        "m.state",
    ])
    .ok();
    cli.meeting(&[
        "add-item",
        "--state",
        "m.state",
        "--title",
        "Report",
        "--document-digest",
        &"11".repeat(32),
    ])
    .ok();
    cli.with_notary(&[
        "meeting",
        "convene",
        "--state",
        "m.state",
        "--signing-key",
        "k.key",
        "--timestamp",
        "1000",
    ])
    .ok();
    cli.meeting(&["open", "--state", "m.state"]).ok();
    cli.meeting(&["close", "--state", "m.state"]).ok();
    let finalized = cli
        .with_notary(&[
            "meeting",
            "finalize",
            "--state",
            "m.state",
            "--signing-key",
            "k.key",
            "--timestamp",
            "2000",
            "--json",
        ])
        .ok();
    let tx = finalized.json()["tx_id"].as_str().unwrap().to_owned();
    let report = cli.meeting(&["verify", "--tx", &tx, "--json"]).ok().json();
    assert_eq!(report["votes"].as_array().map(Vec::len), Some(0));
    assert!(
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["passed"] == true),
        "{report}"
    );
}

#[test]
fn meeting_input_is_checked_and_lifecycle_order_is_enforced() {
    let cli = Cli::new();
    cli.founded();

    // Bad metadata at creation.
    let bad_time = cli.meeting(&[
        "new",
        "--channel",
        "shareholders",
        "--title",
        "AGM",
        "--scheduled-at",
        "soon",
        "--state",
        "m.state",
    ]);
    assert_eq!(bad_time.code(), 2);
    let bad_digest = cli.meeting(&[
        "new",
        "--channel",
        "shareholders",
        "--title",
        "AGM",
        "--scheduled-at",
        "2026-06-01T10:00:00Z",
        "--notice-digest",
        "zz",
        "--state",
        "m.state",
    ]);
    assert_eq!(bad_digest.code(), 2);

    cli.meeting(&[
        "new",
        "--channel",
        "shareholders",
        "--title",
        "AGM",
        "--scheduled-at",
        "2026-06-01T10:00:00Z",
        "--state",
        "m.state",
    ])
    .ok();
    // An item is one kind or the other.
    let neither = cli.meeting(&["add-item", "--state", "m.state", "--title", "x"]);
    assert_eq!(neither.code(), 2);
    let both = cli.meeting(&[
        "add-item",
        "--state",
        "m.state",
        "--title",
        "x",
        "--document-digest",
        &"11".repeat(32),
        "--proposal-digest",
        &"22".repeat(32),
    ]);
    assert_eq!(both.code(), 2);
    // An empty agenda cannot be convened.
    let empty = cli.with_notary(&[
        "meeting",
        "convene",
        "--state",
        "m.state",
        "--signing-key",
        "k.key",
        "--timestamp",
        "1000",
    ]);
    assert_eq!(empty.code(), 2);
    assert!(empty.err().contains("at least one item"), "{}", empty.err());

    cli.meeting(&[
        "add-item",
        "--state",
        "m.state",
        "--title",
        "Approve",
        "--proposal-digest",
        &"22".repeat(32),
    ])
    .ok();
    // Opening before convening, closing before opening.
    assert_eq!(cli.meeting(&["open", "--state", "m.state"]).code(), 2);
    assert_eq!(cli.meeting(&["close", "--state", "m.state"]).code(), 2);
    cli.with_notary(&[
        "meeting",
        "convene",
        "--state",
        "m.state",
        "--signing-key",
        "k.key",
        "--timestamp",
        "1000",
    ])
    .ok();
    // The agenda is fixed once convened.
    let late = cli.meeting(&[
        "add-item",
        "--state",
        "m.state",
        "--title",
        "Late",
        "--document-digest",
        &"44".repeat(32),
    ]);
    assert_eq!(late.code(), 2);
    assert!(
        late.err()
            .contains("cannot add an item to a meeting that is convened"),
        "{}",
        late.err()
    );
    // And finalizing before closing.
    assert_eq!(
        cli.with_notary(&[
            "meeting",
            "finalize",
            "--state",
            "m.state",
            "--signing-key",
            "k.key"
        ])
        .code(),
        2
    );

    let missing = cli.meeting(&["verify", "--tx", &"00".repeat(32)]);
    assert_eq!(missing.code(), 2);
}

// ---------------------------------------------------------------------------------
// Resolutions.
// ---------------------------------------------------------------------------------

impl Cli {
    fn resolution(&self, args: &[&str]) -> Run {
        let mut full = vec!["resolution"];
        full.extend_from_slice(args);
        self.run(&full)
    }
    /// The proposal digest a body must be voted on under.
    fn digest_of(&self, file: &str) -> String {
        self.resolution(&["digest", "--file", file, "--json"])
            .ok()
            .json()["proposal_digest"]
            .as_str()
            .expect("digest")
            .to_owned()
    }
    /// Holds a one-item meeting on `digest` and returns (meeting tx, vote tx).
    fn decide(&self, title: &str, digest: &str, pass: bool, base: u64) -> (String, String) {
        self.meeting(&[
            "new",
            "--channel",
            "shareholders",
            "--title",
            "AGM",
            "--scheduled-at",
            "2026-06-01T10:00:00Z",
            "--state",
            "m.state",
        ])
        .ok();
        self.meeting(&[
            "add-item",
            "--state",
            "m.state",
            "--title",
            title,
            "--proposal-digest",
            digest,
        ])
        .ok();
        self.with_notary(&[
            "meeting",
            "convene",
            "--state",
            "m.state",
            "--signing-key",
            "k.key",
            "--timestamp",
            &base.to_string(),
        ])
        .ok();
        self.meeting(&["open", "--state", "m.state"]).ok();
        let choice = if pass { "yes" } else { "no" };
        for (voter, seed) in [("alice", 1u8), ("bob", 2)] {
            let ballot = format!("{voter}-r.ballot");
            self.meeting(&[
                "ballot",
                "--state",
                "m.state",
                "--item",
                "1",
                "--voter",
                voter,
                "--choice",
                choice,
                "--signing-key",
                &format!("holder{seed}.key"),
                "--out",
                &ballot,
            ])
            .ok();
            self.meeting(&[
                "cast", "--state", "m.state", "--item", "1", "--ballot", &ballot,
            ])
            .ok();
        }
        self.meeting(&["close", "--state", "m.state"]).ok();
        let finalized = self
            .with_notary(&[
                "meeting",
                "finalize",
                "--state",
                "m.state",
                "--signing-key",
                "k.key",
                "--timestamp",
                &(base + 100).to_string(),
                "--json",
            ])
            .ok();
        let meeting_tx = finalized.json()["tx_id"].as_str().unwrap().to_owned();
        let vote_tx = finalized.json()["record"]["items"][0]["vote_tx_id"]
            .as_str()
            .unwrap()
            .to_owned();
        (meeting_tx, vote_tx)
    }
}

#[test]
fn a_passed_vote_becomes_a_resolution_that_changes_the_company() {
    let cli = Cli::new();
    cli.founded();
    let before = cli.run(&["show", "--json"]).ok().json();
    let approved_base = before["shares"]["tx_id"].as_str().unwrap().to_owned();

    // The shareholders vote on the new register itself.
    let digest = cli.digest_of("shares2.xml");
    let (meeting_tx, vote_tx) = cli.decide("Replace the register", &digest, true, 1000);

    let drafted = cli
        .resolution(&[
            "create",
            "--channel",
            "shareholders",
            "--meeting",
            &meeting_tx,
            "--item",
            "1",
            "--vote",
            &vote_tx,
            "--title",
            "Resolution 1: replace the register",
            "--target",
            "share-structure",
            "--file",
            "shares2.xml",
            "--state",
            "r.state",
            "--json",
        ])
        .ok();
    assert_eq!(drafted.json()["status"], "draft");
    assert_eq!(drafted.json()["kind"], "amendment");
    assert_eq!(drafted.json()["target"], "share-structure");
    assert_eq!(drafted.json()["approved_digest"], digest);

    let finalized = cli
        .with_notary(&[
            "resolution",
            "finalize",
            "--state",
            "r.state",
            "--signing-key",
            "k.key",
            "--timestamp",
            "2000",
            "--json",
        ])
        .ok();
    assert_eq!(finalized.json()["status"], "finalized");
    let resolution_tx = finalized.json()["resolution_id"]
        .as_str()
        .unwrap()
        .to_owned();
    // Recording it changed nothing.
    assert_eq!(
        cli.run(&["show", "--json"]).ok().json()["shares"]["tx_id"],
        approved_base
    );

    let executed = cli
        .with_notary(&[
            "resolution",
            "execute",
            "--state",
            "r.state",
            "--signing-key",
            "k.key",
            "--timestamp",
            "3000",
            "--json",
        ])
        .ok();
    let amendment_tx = executed.json()["amendment_tx"].as_str().unwrap().to_owned();
    let execution_tx = executed.json()["execution_tx"].as_str().unwrap().to_owned();
    assert_eq!(executed.json()["replaced_tx"], approved_base);

    // Now the company has changed, through an ordinary amendment.
    let after = cli.run(&["show", "--json"]).ok().json();
    assert_eq!(after["shares"]["tx_id"], amendment_tx);
    assert_eq!(after["shares"]["supersedes"], approved_base);
    assert_eq!(
        cli.run(&["shares", "--json"]).ok().json()["holders"][2]["id"],
        "dave"
    );
    // And the amendment appears in the register's history like any other.
    let history = cli
        .run(&["history", "--kind", "share-structure", "--json"])
        .ok()
        .json();
    assert_eq!(history.as_array().map(Vec::len), Some(2));
    assert_eq!(history[1]["tx_id"], amendment_tx);

    // Both records verify from the chain alone.
    let report = cli.resolution(&["verify", "--tx", &resolution_tx]).ok();
    assert!(report.out().contains("ok   VotePassed"), "{}", report.out());
    assert!(
        report
            .out()
            .contains("rests on exactly what the chain says"),
        "{}",
        report.out()
    );
    let report = cli
        .resolution(&["verify", "--execution", &execution_tx])
        .ok();
    assert!(
        report.out().contains("ok   AmendmentApplied"),
        "{}",
        report.out()
    );
    assert!(report.out().contains("authorised"), "{}", report.out());
    let json = cli
        .resolution(&["verify", "--execution", &execution_tx, "--json"])
        .ok()
        .json();
    assert_eq!(json["checks"].as_array().map(Vec::len), Some(10));
    assert_eq!(
        json["resolution"]["checks"].as_array().map(Vec::len),
        Some(10)
    );
    assert!(
        json["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["passed"] == true),
        "{json}"
    );

    // Executing again is refused.
    let again = cli.with_notary(&[
        "resolution",
        "execute",
        "--state",
        "r.state",
        "--signing-key",
        "k.key",
    ]);
    assert_eq!(again.code(), 2);
    assert!(
        again
            .err()
            .contains("cannot execute a resolution that is executed"),
        "{}",
        again.err()
    );
}

#[test]
fn a_resolution_can_replace_the_channel_set_and_a_declarative_one_changes_nothing() {
    let cli = Cli::new();
    cli.founded();

    // Voting rules.
    let digest = cli.digest_of("channels2.xml");
    let (meeting_tx, vote_tx) = cli.decide("Adopt new rules", &digest, true, 1000);
    cli.resolution(&[
        "create",
        "--channel",
        "shareholders",
        "--meeting",
        &meeting_tx,
        "--item",
        "1",
        "--vote",
        &vote_tx,
        "--title",
        "Resolution 1: new rules",
        "--target",
        "decision-channels",
        "--file",
        "channels2.xml",
        "--state",
        "r.state",
    ])
    .ok();
    cli.with_notary(&[
        "resolution",
        "finalize",
        "--state",
        "r.state",
        "--signing-key",
        "k.key",
        "--timestamp",
        "2000",
    ])
    .ok();
    let executed = cli
        .with_notary(&[
            "resolution",
            "execute",
            "--state",
            "r.state",
            "--signing-key",
            "k.key",
            "--timestamp",
            "3000",
            "--json",
        ])
        .ok();
    let state = cli.run(&["show", "--json"]).ok().json();
    assert_eq!(state["channels"]["tx_id"], executed.json()["amendment_tx"]);
    assert_eq!(
        state["channels"]["value"][2]["mode"]["rules"]["tie"], "accept",
        "the amended rules are in force"
    );
    assert!(
        cli.resolution(&[
            "verify",
            "--execution",
            executed.json()["execution_tx"].as_str().unwrap()
        ])
        .code()
            == 0
    );

    // Declarative: a decision recorded, nothing changed.
    let before = cli.run(&["show", "--json"]).ok().json();
    let document = "dd".repeat(32);
    let (meeting_tx, vote_tx) = cli.decide("Receive the report", &document, true, 4000);
    cli.resolution(&[
        "create",
        "--channel",
        "shareholders",
        "--meeting",
        &meeting_tx,
        "--item",
        "1",
        "--vote",
        &vote_tx,
        "--title",
        "Resolution 2: report received",
        "--document-digest",
        &document,
        "--state",
        "d.state",
        "--json",
    ])
    .ok();
    let finalized = cli
        .with_notary(&[
            "resolution",
            "finalize",
            "--state",
            "d.state",
            "--signing-key",
            "k.key",
            "--timestamp",
            "5000",
            "--json",
        ])
        .ok();
    assert_eq!(finalized.json()["kind"], "declarative");
    let declarative_tx = finalized.json()["resolution_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(cli.resolution(&["verify", "--tx", &declarative_tx]).code() == 0);

    // There is nothing to execute, and the company is untouched.
    let nothing = cli.with_notary(&[
        "resolution",
        "execute",
        "--state",
        "d.state",
        "--signing-key",
        "k.key",
    ]);
    assert_eq!(nothing.code(), 2);
    assert!(
        nothing.err().contains("nothing to execute"),
        "{}",
        nothing.err()
    );
    let after = cli.run(&["show", "--json"]).ok().json();
    assert_eq!(after["shares"]["tx_id"], before["shares"]["tx_id"]);
    assert_eq!(after["channels"]["tx_id"], before["channels"]["tx_id"]);
    assert_eq!(after["identity"]["tx_id"], before["identity"]["tx_id"]);
}

#[test]
fn a_rejected_or_mismatched_vote_authorises_nothing() {
    let cli = Cli::new();
    cli.founded();
    let digest = cli.digest_of("shares2.xml");

    // A rejected motion.
    let (meeting_tx, vote_tx) = cli.decide("Replace the register", &digest, false, 1000);
    cli.resolution(&[
        "create",
        "--channel",
        "shareholders",
        "--meeting",
        &meeting_tx,
        "--item",
        "1",
        "--vote",
        &vote_tx,
        "--title",
        "Resolution 1",
        "--target",
        "share-structure",
        "--file",
        "shares2.xml",
        "--state",
        "r.state",
    ])
    .ok();
    let before = cli.run(&["show", "--json"]).ok().json();
    let refused = cli.with_notary(&[
        "resolution",
        "finalize",
        "--state",
        "r.state",
        "--signing-key",
        "k.key",
        "--timestamp",
        "2000",
    ]);
    assert_eq!(refused.code(), 2);
    assert!(
        refused.err().contains("authorises nothing"),
        "{}",
        refused.err()
    );

    // A passed motion, but the resolution carries a different body.
    let (meeting_tx, vote_tx) = cli.decide("Replace the register", &digest, true, 3000);
    cli.resolution(&[
        "create",
        "--channel",
        "shareholders",
        "--meeting",
        &meeting_tx,
        "--item",
        "1",
        "--vote",
        &vote_tx,
        "--title",
        "Resolution 2",
        "--target",
        "share-structure",
        "--file",
        "shares1.xml",
        "--state",
        "w.state",
    ])
    .ok();
    let mismatched = cli.with_notary(&[
        "resolution",
        "finalize",
        "--state",
        "w.state",
        "--signing-key",
        "k.key",
        "--timestamp",
        "4000",
    ]);
    assert_eq!(mismatched.code(), 2);
    assert!(
        mismatched
            .err()
            .contains("does not match the approved proposal"),
        "{}",
        mismatched.err()
    );

    // A vote that did not answer that item.
    let (other_meeting, _) = cli.decide("Something else", &digest, true, 5000);
    cli.resolution(&[
        "create",
        "--channel",
        "shareholders",
        "--meeting",
        &other_meeting,
        "--item",
        "1",
        "--vote",
        &vote_tx,
        "--title",
        "Resolution 3",
        "--target",
        "share-structure",
        "--file",
        "shares2.xml",
        "--state",
        "x.state",
    ])
    .ok();
    let crossed = cli.with_notary(&[
        "resolution",
        "finalize",
        "--state",
        "x.state",
        "--signing-key",
        "k.key",
        "--timestamp",
        "6000",
    ]);
    assert_eq!(crossed.code(), 2);
    assert!(
        crossed.err().contains("was answered by"),
        "{}",
        crossed.err()
    );

    // Nothing reached the company.
    let after = cli.run(&["show", "--json"]).ok().json();
    assert_eq!(after["shares"]["tx_id"], before["shares"]["tx_id"]);
}

#[test]
fn a_resolution_whose_base_moved_is_refused_and_input_is_checked() {
    let cli = Cli::new();
    cli.founded();
    let digest = cli.digest_of("shares2.xml");
    let (meeting_tx, vote_tx) = cli.decide("Replace the register", &digest, true, 1000);
    cli.resolution(&[
        "create",
        "--channel",
        "shareholders",
        "--meeting",
        &meeting_tx,
        "--item",
        "1",
        "--vote",
        &vote_tx,
        "--title",
        "Resolution 1",
        "--target",
        "share-structure",
        "--file",
        "shares2.xml",
        "--state",
        "r.state",
    ])
    .ok();
    cli.with_notary(&[
        "resolution",
        "finalize",
        "--state",
        "r.state",
        "--signing-key",
        "k.key",
        "--timestamp",
        "2000",
    ])
    .ok();

    // Someone amends the register directly in between.
    std::fs::write(
        cli.dir.path().join("shares3.xml"),
        register().replace("shares=\"200\"", "shares=\"201\""),
    )
    .expect("write");
    cli.amend("publish-shares", "shares3.xml", "shares", "2500")
        .ok();

    let stale = cli.with_notary(&[
        "resolution",
        "execute",
        "--state",
        "r.state",
        "--signing-key",
        "k.key",
        "--timestamp",
        "3000",
    ]);
    assert_eq!(stale.code(), 2);
    assert!(stale.err().contains("has since changed"), "{}", stale.err());

    // Input checks.
    assert_eq!(
        cli.resolution(&[
            "create",
            "--channel",
            "shareholders",
            "--meeting",
            "zz",
            "--item",
            "1",
            "--vote",
            &vote_tx,
            "--title",
            "x",
            "--document-digest",
            &digest,
            "--state",
            "y.state"
        ])
        .code(),
        2
    );
    assert_eq!(
        cli.resolution(&[
            "create",
            "--channel",
            "shareholders",
            "--meeting",
            &meeting_tx,
            "--item",
            "1",
            "--vote",
            &vote_tx,
            "--title",
            "x",
            "--state",
            "y.state"
        ])
        .code(),
        2
    );
    assert_eq!(
        cli.resolution(&[
            "create",
            "--channel",
            "shareholders",
            "--meeting",
            &meeting_tx,
            "--item",
            "1",
            "--vote",
            &vote_tx,
            "--title",
            "x",
            "--target",
            "identity",
            "--file",
            "shares2.xml",
            "--state",
            "y.state"
        ])
        .code(),
        2
    );
    assert_eq!(cli.resolution(&["verify"]).code(), 2);
    assert_eq!(
        cli.resolution(&["verify", "--tx", &"00".repeat(32)]).code(),
        2
    );
    // The digest command is a pure function of the file.
    assert_eq!(cli.digest_of("shares2.xml"), digest);
    assert_ne!(cli.digest_of("shares1.xml"), digest);
}

// ---------------------------------------------------------------------------------
// Decision channels: resolved, decided through individually, and bounded.
// ---------------------------------------------------------------------------------

impl Cli {
    fn decision(&self, args: &[&str]) -> Run {
        let mut full = vec!["decision"];
        full.extend_from_slice(args);
        self.run(&full)
    }
    /// Freezes, signs (as the chair, seed 5) and finalises a decision through the
    /// ceo channel on `digest`; returns the decision transaction.
    fn decide_alone(&self, subject: &str, digest: &str, timestamp: u64) -> String {
        self.decision(&[
            "new",
            "--subject",
            subject,
            "--proposal-digest",
            digest,
            "--state",
            "d.state",
        ])
        .ok();
        let frozen = self
            .decision(&["freeze", "--state", "d.state", "--channel", "ceo", "--json"])
            .ok();
        assert_eq!(frozen.json()["snapshot"]["actor"], "chair");
        assert_eq!(frozen.json()["snapshot"]["channel"], "ceo");
        self.decision(&["sign", "--state", "d.state", "--signing-key", "holder5.key"])
            .ok();
        let finalized = self
            .decision(&[
                "finalize",
                "--state",
                "d.state",
                "--signing-key",
                "k.key",
                "--timestamp",
                &timestamp.to_string(),
                "--json",
            ])
            .ok();
        finalized.json()["finalized"]["tx_id"]
            .as_str()
            .expect("tx")
            .to_owned()
    }
}

#[test]
fn channels_resolve_and_an_individual_decision_becomes_a_resolution() {
    let cli = Cli::new();
    cli.founded();

    // Every channel, resolved: the register, a weighted roster, one person.
    let channels = cli.run(&["channels", "--json"]).ok().json();
    let ids: Vec<&str> = channels["channels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["board", "ceo", "shareholders"]);
    assert_eq!(channels["channels"][0]["total_weight"], 4);
    assert_eq!(channels["channels"][1]["actors"][0]["id"], "chair");
    assert_eq!(channels["channels"][2]["total_weight"], 1000);
    assert!(channels["channels"][2]["rules"]["tie"] == "reject");
    let text = cli.run(&["channels"]).ok();
    assert!(text.out().contains("decides alone"), "{}", text.out());
    assert!(text.out().contains("3 channel(s)"), "{}", text.out());

    // The ceo, alone, replaces the share register: decision → resolution → amendment.
    let digest = cli.digest_of("shares2.xml");
    let decision_tx = cli.decide_alone("Register bob's transfer to dave", &digest, 1000);
    let verify = cli.decision(&["verify", "--tx", &decision_tx]).ok();
    assert!(verify.out().contains("VALID"), "{}", verify.out());
    assert!(
        verify.out().contains("SignatureVerifies"),
        "{}",
        verify.out()
    );

    cli.resolution(&[
        "create",
        "--channel",
        "ceo",
        "--decision",
        &decision_tx,
        "--title",
        "Resolution 1: the transfer",
        "--target",
        "share-structure",
        "--file",
        "shares2.xml",
        "--state",
        "r.state",
    ])
    .ok();
    let shown = cli.resolution(&["show", "--state", "r.state"]).ok();
    assert!(shown.out().contains("ceo (individual)"), "{}", shown.out());
    cli.with_notary(&[
        "resolution",
        "finalize",
        "--state",
        "r.state",
        "--signing-key",
        "k.key",
        "--timestamp",
        "2000",
    ])
    .ok();
    let executed = cli
        .with_notary(&[
            "resolution",
            "execute",
            "--state",
            "r.state",
            "--signing-key",
            "k.key",
            "--timestamp",
            "3000",
            "--json",
        ])
        .ok();
    let amendment_tx = executed.json()["amendment_tx"].as_str().unwrap().to_owned();
    let execution_tx = executed.json()["execution_tx"].as_str().unwrap().to_owned();
    assert_eq!(
        cli.run(&["show", "--json"]).ok().json()["shares"]["tx_id"],
        amendment_tx
    );
    let report = cli
        .resolution(&["verify", "--execution", &execution_tx])
        .ok();
    assert!(
        report.out().contains("SelfDemotionHolds"),
        "{}",
        report.out()
    );
    assert!(report.out().contains("not applicable"), "{}", report.out());
    assert!(
        report.out().contains("DecisionVerifies"),
        "{}",
        report.out()
    );

    // Only the actor can sign; a vote cannot go through an individual channel; a
    // decision cannot go through a collective one.
    cli.decision(&[
        "new",
        "--subject",
        "x",
        "--proposal-digest",
        &digest,
        "--state",
        "d2.state",
    ])
    .ok();
    cli.decision(&["freeze", "--state", "d2.state", "--channel", "ceo"])
        .ok();
    let wrong = cli.decision(&[
        "sign",
        "--state",
        "d2.state",
        "--signing-key",
        "holder1.key",
    ]);
    assert_eq!(wrong.code(), 2);
    assert!(
        wrong.err().contains("not the registered key"),
        "{}",
        wrong.err()
    );
    let collective = cli.decision(&["freeze", "--state", "d2.state", "--channel", "board"]);
    assert_eq!(collective.code(), 2, "{}", collective.err());
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
    let individual = cli.vote(&["freeze", "--state", "v.state", "--channel", "ceo"]);
    assert_eq!(individual.code(), 2);
    assert!(
        individual.err().contains("individual"),
        "{}",
        individual.err()
    );
}

#[test]
fn an_individual_channel_may_only_demote_itself() {
    let cli = Cli::new();
    cli.founded();

    // The ceo thins the board it sits on: refused at execution, and the refusal names
    // the rule.
    let thinned = channels(RULES).replace(
        &format!(
            "      <member id=\"dir-a\" key=\"{}\"/>\n      <member id=\"dir-b\" key=\"{}\"/>\n",
            prunella_crypto::SigningKey::from_seed([6; 32]).public_key(),
            prunella_crypto::SigningKey::from_seed([7; 32]).public_key()
        ),
        "",
    );
    assert!(!thinned.contains("dir-a"), "fixture edited");
    std::fs::write(cli.dir.path().join("thinned.xml"), &thinned).unwrap();
    let digest = cli.digest_of("thinned.xml");
    let decision_tx = cli.decide_alone("Shrink the board", &digest, 1000);
    cli.resolution(&[
        "create",
        "--channel",
        "ceo",
        "--decision",
        &decision_tx,
        "--title",
        "Resolution 1: a smaller board",
        "--target",
        "decision-channels",
        "--file",
        "thinned.xml",
        "--state",
        "r.state",
    ])
    .ok();
    cli.with_notary(&[
        "resolution",
        "finalize",
        "--state",
        "r.state",
        "--signing-key",
        "k.key",
        "--timestamp",
        "2000",
    ])
    .ok();
    let refused = cli.with_notary(&[
        "resolution",
        "execute",
        "--state",
        "r.state",
        "--signing-key",
        "k.key",
        "--timestamp",
        "3000",
    ]);
    assert_eq!(refused.code(), 2);
    assert!(
        refused.err().contains("may not carry this amendment alone"),
        "{}",
        refused.err()
    );
    assert!(refused.err().contains("board"), "{}", refused.err());
    let head_before = cli.run(&["show", "--json"]).ok().json()["channels"]["tx_id"].clone();

    // The ceo abolishes itself: allowed, and the execution's verification says why.
    let abolished = {
        let full = channels(RULES);
        let start = full.find("  <channel id=\"ceo\"").unwrap();
        let end = full[start..].find("</channel>\n").unwrap() + start + "</channel>\n".len();
        format!("{}{}", &full[..start], &full[end..])
    };
    assert!(
        !abolished.contains("\"ceo\""),
        "fixture edited:\n{abolished}"
    );
    std::fs::write(cli.dir.path().join("abolished.xml"), &abolished).unwrap();
    let digest = cli.digest_of("abolished.xml");
    let decision_tx = cli.decide_alone("Abolish the ceo channel", &digest, 4000);
    cli.resolution(&[
        "create",
        "--channel",
        "ceo",
        "--decision",
        &decision_tx,
        "--title",
        "Resolution 2: no more ceo",
        "--target",
        "decision-channels",
        "--file",
        "abolished.xml",
        "--state",
        "r2.state",
    ])
    .ok();
    cli.with_notary(&[
        "resolution",
        "finalize",
        "--state",
        "r2.state",
        "--signing-key",
        "k.key",
        "--timestamp",
        "5000",
    ])
    .ok();
    let executed = cli
        .with_notary(&[
            "resolution",
            "execute",
            "--state",
            "r2.state",
            "--signing-key",
            "k.key",
            "--timestamp",
            "6000",
            "--json",
        ])
        .ok();
    let now = cli.run(&["show", "--json"]).ok().json();
    assert_ne!(now["channels"]["tx_id"], head_before);
    assert_eq!(now["channels"]["tx_id"], executed.json()["amendment_tx"]);
    let listed = cli.run(&["channels"]).ok();
    assert!(listed.out().contains("2 channel(s)"), "{}", listed.out());
    assert!(!listed.out().contains("decides alone"), "{}", listed.out());
    let execution_tx = executed.json()["execution_tx"].as_str().unwrap().to_owned();
    let report = cli
        .resolution(&["verify", "--execution", &execution_tx])
        .ok();
    assert!(report.out().contains("gives up ceo"), "{}", report.out());

    // The channel is gone: nothing more can be decided through it.
    cli.decision(&[
        "new",
        "--subject",
        "x",
        "--proposal-digest",
        &digest,
        "--state",
        "d3.state",
    ])
    .ok();
    let gone = cli.decision(&["freeze", "--state", "d3.state", "--channel", "ceo"]);
    assert_eq!(gone.code(), 2);
    assert!(
        gone.err().contains("no decision channel ceo"),
        "{}",
        gone.err()
    );
}
