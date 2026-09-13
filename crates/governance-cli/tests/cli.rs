//! End-to-end: found a chain with rules in genesis, amend, resolve at heights, evaluate.

use std::process::{Command, Output};
use tempfile::TempDir;

const RULES_V1: &str = r#"<voting-rules version="1.0">
  <weight type="electorate"/>
  <exclusions enabled="true"/>
  <quorum type="none"/>
  <threshold type="simple-majority" basis="votes-cast"/>
  <abstentions treatment="exclude"/>
  <tie treatment="reject"/>
</voting-rules>
"#;
const RULES_V2: &str = r#"<voting-rules version="1.0">
  <weight type="electorate"/>
  <exclusions enabled="true"/>
  <quorum type="none"/>
  <threshold type="simple-majority" basis="votes-cast"/>
  <abstentions treatment="exclude"/>
  <tie treatment="accept"/>
</voting-rules>
"#;
const ROLL: &str = r#"<electorate>
  <voter id="drone-01" weight="2"/>
  <voter id="drone-02"/>
  <voter id="drone-03"/>
</electorate>
"#;
const BALLOTS: &str = r#"<ballots>
  <ballot voter="drone-01" choice="yes"/>
  <ballot voter="drone-02" choice="no"/>
  <ballot voter="drone-03" choice="no"/>
</ballots>
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
}

impl Cli {
    fn new() -> Self {
        let dir = TempDir::new().expect("temp dir");
        for (name, text) in [
            ("v1.xml", RULES_V1),
            ("v2.xml", RULES_V2),
            ("roll.xml", ROLL),
            ("ballots.xml", BALLOTS),
        ] {
            std::fs::write(dir.path().join(name), text).expect("write");
        }
        std::fs::write(dir.path().join("k.key"), "01".repeat(32)).expect("key");
        Self { dir }
    }
    fn run(&self, args: &[&str]) -> Run {
        let output = Command::new(env!("CARGO_BIN_EXE_governance"))
            .current_dir(self.dir.path())
            .args(["--chain", "gov.chain"])
            .args(args)
            .output()
            .expect("run governance");
        Run(output)
    }
    fn found(&self) -> Run {
        let run = self.run(&[
            "init",
            "--network",
            "swarm",
            "--subject",
            "swarm-alpha",
            "--rules",
            "v1.xml",
            "--signing-key",
            "k.key",
            "--notary",
            "notary-07",
            "--statement",
            "founded",
            "--json",
        ]);
        assert_eq!(run.code(), 0, "{}", run.err());
        run
    }
}

#[test]
fn init_puts_the_rules_in_genesis() {
    let cli = Cli::new();
    let init = cli.found();
    let show = cli.run(&[
        "show-rules",
        "--subject",
        "swarm-alpha",
        "--at",
        "0",
        "--json",
    ]);
    assert_eq!(show.code(), 0, "{}", show.err());
    assert_eq!(show.json()["height"], 0);
    assert_eq!(show.json()["tx_id"], init.json()["rules_tx_id"]);
    assert_eq!(show.json()["value"]["tie"], "reject");
    assert_eq!(show.json()["notarisation"]["notary"], "notary-07");
}

#[test]
fn init_is_reproducible() {
    let a = Cli::new();
    let b = Cli::new();
    assert_eq!(
        a.found().json()["genesis_hash"],
        b.found().json()["genesis_hash"]
    );
}

#[test]
fn amendments_resolve_by_height_and_appear_in_history() {
    let cli = Cli::new();
    let v1 = cli.found().json()["rules_tx_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let roll = cli.run(&[
        "publish-roll",
        "--subject",
        "swarm-alpha",
        "--roll",
        "roll.xml",
        "--signing-key",
        "k.key",
        "--timestamp",
        "1000",
    ]);
    assert_eq!(roll.code(), 0, "{}", roll.err());

    let amend = cli.run(&[
        "publish-rules",
        "--subject",
        "swarm-alpha",
        "--rules",
        "v2.xml",
        "--signing-key",
        "k.key",
        "--supersedes",
        &v1,
        "--timestamp",
        "2000",
        "--json",
    ]);
    assert_eq!(amend.code(), 0, "{}", amend.err());
    let v2 = amend.json()["tx_id"].as_str().expect("id").to_owned();

    let then = cli.run(&[
        "show-rules",
        "--subject",
        "swarm-alpha",
        "--at",
        "1",
        "--json",
    ]);
    let now = cli.run(&["show-rules", "--subject", "swarm-alpha", "--json"]);
    assert_eq!(then.json()["tx_id"], v1);
    assert_eq!(now.json()["tx_id"], v2);
    assert_eq!(now.json()["value"]["tie"], "accept");

    let history = cli.run(&[
        "history",
        "--subject",
        "swarm-alpha",
        "--kind",
        "voting-rules",
        "--json",
    ]);
    let versions = history.json();
    assert_eq!(versions.as_array().expect("array").len(), 2);
    assert_eq!(versions[1]["record"]["supersedes"], v1);
}

#[test]
fn a_stale_amendment_is_refused() {
    let cli = Cli::new();
    let v1 = cli.found().json()["rules_tx_id"]
        .as_str()
        .expect("id")
        .to_owned();
    cli.run(&[
        "publish-rules",
        "--subject",
        "swarm-alpha",
        "--rules",
        "v2.xml",
        "--signing-key",
        "k.key",
        "--supersedes",
        &v1,
        "--timestamp",
        "2000",
    ]);
    let stale = cli.run(&[
        "publish-rules",
        "--subject",
        "swarm-alpha",
        "--rules",
        "v1.xml",
        "--signing-key",
        "k.key",
        "--supersedes",
        &v1,
        "--timestamp",
        "3000",
    ]);
    assert_eq!(stale.code(), 2);
    assert!(stale.err().contains("stale amendment"), "{}", stale.err());
}

#[test]
fn evaluate_uses_the_rules_in_force_at_the_height() {
    let cli = Cli::new();
    let v1 = cli.found().json()["rules_tx_id"]
        .as_str()
        .expect("id")
        .to_owned();
    cli.run(&[
        "publish-roll",
        "--subject",
        "swarm-alpha",
        "--roll",
        "roll.xml",
        "--signing-key",
        "k.key",
        "--timestamp",
        "1000",
    ]);

    // 2 yes vs 2 no is a tie: rejected under v1.
    let under_v1 = cli.run(&[
        "evaluate",
        "--subject",
        "swarm-alpha",
        "--ballots",
        "ballots.xml",
        "--json",
    ]);
    assert_eq!(under_v1.code(), 1, "{}", under_v1.err());
    assert_eq!(under_v1.json()["evaluation"]["reason"], "tie_rejected");

    cli.run(&[
        "publish-rules",
        "--subject",
        "swarm-alpha",
        "--rules",
        "v2.xml",
        "--signing-key",
        "k.key",
        "--supersedes",
        &v1,
        "--timestamp",
        "2000",
    ]);

    // Same ballots, same ledger, later height: the tie is now accepted.
    let under_v2 = cli.run(&[
        "evaluate",
        "--subject",
        "swarm-alpha",
        "--ballots",
        "ballots.xml",
        "--json",
    ]);
    assert_eq!(under_v2.code(), 0, "{}", under_v2.err());
    assert_eq!(under_v2.json()["evaluation"]["reason"], "tie_accepted");

    // And asking about height 1 still gives the old answer.
    let at_one = cli.run(&[
        "evaluate",
        "--subject",
        "swarm-alpha",
        "--ballots",
        "ballots.xml",
        "--at",
        "1",
        "--json",
    ]);
    assert_eq!(at_one.code(), 1);
}

#[test]
fn ledger_evaluation_matches_the_standalone_engine() {
    let cli = Cli::new();
    cli.found();
    cli.run(&[
        "publish-roll",
        "--subject",
        "swarm-alpha",
        "--roll",
        "roll.xml",
        "--signing-key",
        "k.key",
        "--timestamp",
        "1000",
    ]);
    let ledger = cli
        .run(&[
            "evaluate",
            "--subject",
            "swarm-alpha",
            "--ballots",
            "ballots.xml",
            "--json",
        ])
        .json();

    // The standalone engine, given the same rules and roll as a vote document.
    let vote = format!("<vote version=\"1.0\">\n{}{}</vote>\n", ROLL, BALLOTS);
    std::fs::write(cli.dir.path().join("vote.xml"), vote).expect("write");
    let standalone = Command::new(env!("CARGO_BIN_EXE_governance"))
        .current_dir(cli.dir.path())
        .args([
            "--chain",
            "gov.chain",
            "show-rules",
            "--subject",
            "swarm-alpha",
        ])
        .output()
        .expect("run");
    assert!(standalone.status.success());
    // Compare the evaluation payload field by field with what Bornite alone produces.
    let bornite = std::path::Path::new(env!("CARGO_BIN_EXE_governance")).with_file_name("bornite");
    if bornite.exists() {
        let alone = Command::new(&bornite)
            .current_dir(cli.dir.path())
            .args([
                "evaluate", "--rules", "v1.xml", "--vote", "vote.xml", "--json",
            ])
            .output()
            .expect("run bornite");
        let alone: serde_json::Value = serde_json::from_slice(&alone.stdout).expect("json");
        assert_eq!(
            ledger["evaluation"], alone,
            "living on a ledger must change nothing"
        );
    }
}

#[test]
fn nothing_in_force_and_bad_input_exit_two() {
    let cli = Cli::new();
    cli.found();
    let none = cli.run(&["show-roll", "--subject", "swarm-alpha"]);
    assert_eq!(none.code(), 2);
    assert!(
        none.err().contains("no roll record is in force"),
        "{}",
        none.err()
    );

    let unknown = cli.run(&["show-rules", "--subject", "nobody"]);
    assert_eq!(unknown.code(), 2);

    let bad_kind = cli.run(&["history", "--subject", "swarm-alpha", "--kind", "bylaws"]);
    assert_eq!(bad_kind.code(), 2);

    std::fs::write(
        cli.dir.path().join("bad.xml"),
        "<voting-rules version=\"1.0\"/>",
    )
    .expect("write");
    let bad = cli.run(&[
        "publish-rules",
        "--subject",
        "other",
        "--rules",
        "bad.xml",
        "--signing-key",
        "k.key",
    ]);
    assert_eq!(bad.code(), 2);
}
