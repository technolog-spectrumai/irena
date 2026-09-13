//! End-to-end tests driving the built binary.

use std::path::PathBuf;
use std::process::{Command, Output};
use tempfile::TempDir;

const RULES: &str = r#"<voting-rules version="1.0">
  <weight type="electorate"/>
  <exclusions enabled="true"/>
  <quorum type="fraction" numerator="1" denominator="2" basis="effective-electorate"/>
  <threshold type="simple-majority" basis="votes-cast"/>
  <abstentions treatment="exclude"/>
  <tie treatment="reject"/>
</voting-rules>
"#;

const VOTE: &str = r#"<vote version="1.0">
  <electorate>
    <voter id="drone-01" weight="3"/>
    <voter id="drone-02"/>
    <voter id="drone-03" weight="2" excluded="true"/>
  </electorate>
  <ballots>
    <ballot voter="drone-01" choice="yes"/>
    <ballot voter="drone-02" choice="abstain"/>
  </ballots>
</vote>
"#;

struct Cli {
    directory: TempDir,
}

struct Run {
    output: Output,
}

impl Run {
    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.output.stdout).into_owned()
    }
    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.output.stderr).into_owned()
    }
    fn code(&self) -> i32 {
        self.output.status.code().unwrap_or(-1)
    }
    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.stdout())
            .unwrap_or_else(|e| panic!("not json ({e}):\n{}", self.stdout()))
    }
}

impl Cli {
    fn new() -> Self {
        let directory = TempDir::new().expect("temp dir");
        std::fs::write(directory.path().join("rules.xml"), RULES).expect("write");
        std::fs::write(directory.path().join("vote.xml"), VOTE).expect("write");
        Self { directory }
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.directory.path().join(name);
        std::fs::write(&path, text).expect("write");
        path
    }
    fn run(&self, args: &[&str]) -> Run {
        let output = Command::new(env!("CARGO_BIN_EXE_bornite"))
            .current_dir(self.directory.path())
            .args(args)
            .output()
            .expect("run bornite");
        Run { output }
    }
}

#[test]
fn validate_rules_accepts_the_example_and_prints_typed_rules() {
    let cli = Cli::new();
    let run = cli.run(&["validate-rules", "--in", "rules.xml", "--json"]);
    assert_eq!(run.code(), 0, "{}", run.stderr());
    let json = run.json();
    assert_eq!(json["weight"], "electorate");
    assert_eq!(json["quorum"]["type"], "fraction");
    assert_eq!(json["quorum"]["fraction"]["numerator"], 1);
    assert_eq!(json["threshold"]["type"], "simple-majority");
    assert_eq!(json["tie"], "reject");

    let text = cli.run(&["validate-rules", "--in", "rules.xml"]);
    assert_eq!(text.code(), 0);
    assert!(
        text.stdout().contains("valid voting-rules document"),
        "{}",
        text.stdout()
    );
}

#[test]
fn validate_rules_reports_every_issue_and_exits_two() {
    let cli = Cli::new();
    cli.write(
        "bad.xml",
        &RULES
            .replace(r#"type="electorate""#, r#"type="shares""#)
            .replace(r#"treatment="reject""#, r#"treatment="maybe""#),
    );
    let run = cli.run(&["validate-rules", "--in", "bad.xml"]);
    assert_eq!(run.code(), 2);
    assert!(run.stderr().contains("2 issue(s)"), "{}", run.stderr());
    assert!(run.stderr().contains("shares"), "{}", run.stderr());
    assert!(run.stderr().contains("maybe"), "{}", run.stderr());
}

#[test]
fn evaluate_accepts_a_passing_vote_and_exits_zero() {
    let cli = Cli::new();
    let run = cli.run(&[
        "evaluate",
        "--rules",
        "rules.xml",
        "--vote",
        "vote.xml",
        "--json",
    ]);
    assert_eq!(run.code(), 0, "{}", run.stderr());
    let json = run.json();
    assert_eq!(json["outcome"], "accepted");
    assert_eq!(json["reason"], "threshold_met");
    assert_eq!(json["electorate"]["effective_weight"], 4);
    assert_eq!(json["tally"]["yes_weight"], 3);
    assert_eq!(json["tally"]["abstain_weight"], 1);
    assert_eq!(json["quorum"]["met"], true);
    assert_eq!(
        json["threshold"]["denominator_weight"], 3,
        "abstentions excluded"
    );
}

#[test]
fn evaluate_reports_a_rejected_vote_with_exit_one() {
    let cli = Cli::new();
    cli.write("no.xml", &VOTE.replace(r#"choice="yes""#, r#"choice="no""#));
    let run = cli.run(&[
        "evaluate",
        "--rules",
        "rules.xml",
        "--vote",
        "no.xml",
        "--json",
    ]);
    assert_eq!(run.code(), 1, "{}", run.stderr());
    assert_eq!(run.json()["outcome"], "rejected");
    assert_eq!(run.json()["reason"], "threshold_not_met");
}

#[test]
fn evaluate_reports_a_missed_quorum() {
    let cli = Cli::new();
    // Only drone-02 (weight 1) votes; effective weight is 4; half is 2.
    cli.write(
        "thin.xml",
        &VOTE.replace(r#"<ballot voter="drone-01" choice="yes"/>"#, ""),
    );
    let run = cli.run(&[
        "evaluate",
        "--rules",
        "rules.xml",
        "--vote",
        "thin.xml",
        "--json",
    ]);
    assert_eq!(run.code(), 1);
    assert_eq!(run.json()["reason"], "quorum_not_met");
    assert_eq!(run.json()["quorum"]["met"], false);
}

#[test]
fn the_tie_rule_flips_an_exact_tie() {
    let cli = Cli::new();
    let tied = VOTE
        .replace(
            r#"<voter id="drone-01" weight="3"/>"#,
            r#"<voter id="drone-01" weight="1"/>"#,
        )
        .replace(
            r#"<ballot voter="drone-02" choice="abstain"/>"#,
            r#"<ballot voter="drone-02" choice="no"/>"#,
        );
    cli.write("tie.xml", &tied);

    let reject = cli.run(&[
        "evaluate",
        "--rules",
        "rules.xml",
        "--vote",
        "tie.xml",
        "--json",
    ]);
    assert_eq!(reject.code(), 1);
    assert_eq!(reject.json()["reason"], "tie_rejected");
    assert_eq!(reject.json()["threshold"]["comparison"], "exactly");

    cli.write(
        "accept.xml",
        &RULES.replace(
            r#"<tie treatment="reject"/>"#,
            r#"<tie treatment="accept"/>"#,
        ),
    );
    let accept = cli.run(&[
        "evaluate",
        "--rules",
        "accept.xml",
        "--vote",
        "tie.xml",
        "--json",
    ]);
    assert_eq!(accept.code(), 0);
    assert_eq!(accept.json()["reason"], "tie_accepted");
}

#[test]
fn ballot_order_does_not_change_the_output() {
    let cli = Cli::new();
    let shuffled = VOTE.replace(
        r#"    <ballot voter="drone-01" choice="yes"/>
    <ballot voter="drone-02" choice="abstain"/>"#,
        r#"    <ballot voter="drone-02" choice="abstain"/>
    <ballot voter="drone-01" choice="yes"/>"#,
    );
    assert_ne!(shuffled, VOTE, "the replacement must have applied");
    cli.write("shuffled.xml", &shuffled);
    let a = cli.run(&[
        "evaluate",
        "--rules",
        "rules.xml",
        "--vote",
        "vote.xml",
        "--json",
    ]);
    let b = cli.run(&[
        "evaluate",
        "--rules",
        "rules.xml",
        "--vote",
        "shuffled.xml",
        "--json",
    ]);
    assert_eq!(a.stdout(), b.stdout());
}

#[test]
fn invalid_input_exits_two_with_the_reason() {
    let cli = Cli::new();
    cli.write(
        "ghost.xml",
        &VOTE.replace(r#"voter="drone-02""#, r#"voter="ghost""#),
    );
    let run = cli.run(&["evaluate", "--rules", "rules.xml", "--vote", "ghost.xml"]);
    assert_eq!(run.code(), 2);
    assert!(run.stderr().contains("ghost"), "{}", run.stderr());

    let missing = cli.run(&["evaluate", "--rules", "rules.xml", "--vote", "absent.xml"]);
    assert_eq!(missing.code(), 2);
    assert!(
        missing.stderr().contains("could not read"),
        "{}",
        missing.stderr()
    );
}

#[test]
fn contradictory_rules_and_electorate_exit_two() {
    let cli = Cli::new();
    cli.write(
        "equal.xml",
        &RULES.replace(r#"type="electorate""#, r#"type="equal""#),
    );
    let run = cli.run(&["evaluate", "--rules", "equal.xml", "--vote", "vote.xml"]);
    assert_eq!(run.code(), 2);
    assert!(run.stderr().contains("declares weight"), "{}", run.stderr());
}
