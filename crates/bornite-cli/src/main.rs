//! The `bornite` command line interface.
//!
//! Two commands, each a thin call into the libraries. No counting, comparing or
//! parsing happens here; the CLI reads files, calls Bornite, and renders what comes
//! back.
//!
//! Exit codes: `0` the motion was accepted; `1` the motion was rejected; `2` the input
//! was invalid or could not be evaluated. A rejection is a real answer, so it is kept
//! apart from bad input.

use bornite_eval::{OutcomeV1, VoteEvaluationV1, evaluate};
use bornite_rules::VotingRulesV1;
use bornite_xml::{read_rules_document, read_vote_document};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const EXIT_ACCEPTED: u8 = 0;
const EXIT_REJECTED: u8 = 1;
const EXIT_INVALID: u8 = 2;

/// Bornite: a deterministic voting engine.
#[derive(Parser, Debug)]
#[command(name = "bornite", version, about, long_about = None)]
struct Cli {
    /// Emit JSON instead of text.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Parse and validate a voting-rules document, printing the typed rules.
    ValidateRules {
        /// The rules document.
        #[arg(long, short)]
        r#in: PathBuf,
    },
    /// Evaluate a vote against a rules document.
    Evaluate {
        /// The voting-rules document.
        #[arg(long)]
        rules: PathBuf,
        /// The vote document: electorate and ballots.
        #[arg(long)]
        vote: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let outcome = match &cli.command {
        Command::ValidateRules { r#in } => validate_rules(r#in, cli.json),
        Command::Evaluate { rules, vote } => evaluate_vote(rules, vote, cli.json),
    };
    match outcome {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(EXIT_INVALID)
        }
    }
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))
}

fn validate_rules(path: &Path, json: bool) -> Result<u8, String> {
    let rules = read_rules_document(&read(path)?).map_err(|error| {
        format!(
            "{} is not a valid voting-rules document: {error}",
            path.display()
        )
    })?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&rules).map_err(|error| error.to_string())?
        );
    } else {
        println!("{} is a valid voting-rules document", path.display());
        print!("{}", render_rules(&rules));
    }
    Ok(EXIT_ACCEPTED)
}

fn evaluate_vote(rules_path: &Path, vote_path: &Path, json: bool) -> Result<u8, String> {
    let rules = read_rules_document(&read(rules_path)?).map_err(|error| {
        format!(
            "{} is not a valid voting-rules document: {error}",
            rules_path.display()
        )
    })?;
    let vote = read_vote_document(&read(vote_path)?).map_err(|error| {
        format!(
            "{} is not a valid vote document: {error}",
            vote_path.display()
        )
    })?;
    let evaluation = evaluate(&rules, &vote.electorate, &vote.ballots)
        .map_err(|error| format!("the vote could not be evaluated: {error}"))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&evaluation).map_err(|error| error.to_string())?
        );
    } else {
        print!("{}", render_evaluation(&evaluation));
    }
    Ok(match evaluation.outcome {
        OutcomeV1::Accepted => EXIT_ACCEPTED,
        OutcomeV1::Rejected => EXIT_REJECTED,
    })
}

fn render_rules(rules: &VotingRulesV1) -> String {
    let mut text = String::new();
    text.push_str(&format!("weight:      {:?}\n", rules.weight));
    text.push_str(&format!(
        "exclusions:  {}\n",
        if rules.exclusions_enabled {
            "enabled"
        } else {
            "disabled"
        }
    ));
    text.push_str(&format!("quorum:      {:?}\n", rules.quorum));
    text.push_str(&format!("threshold:   {:?}\n", rules.threshold));
    text.push_str(&format!("abstentions: {:?}\n", rules.abstentions));
    text.push_str(&format!("tie:         {:?}\n", rules.tie));
    text
}

fn render_evaluation(result: &VoteEvaluationV1) -> String {
    let e = &result.electorate;
    let p = &result.participation;
    let t = &result.tally;
    let q = &result.quorum;
    let th = &result.threshold;
    format!(
        "outcome:        {:?}\n\
         reason:         {}\n\
         electorate:     {} voters, {} excluded, {} effective; weight {} total, {} effective\n\
         participation:  {} ballots weighing {}; {} silent weighing {}\n\
         tally:          yes {} ({}), no {} ({}), abstain {} ({})\n\
         quorum:         {} — actual {} — {}\n\
         threshold:      yes {} against {} of {:?} weighing {} — {:?} — {}\n",
        result.outcome,
        result.reason,
        e.voter_count,
        e.excluded_count,
        e.effective_voter_count,
        e.total_weight,
        e.effective_weight,
        p.ballot_count,
        p.weight,
        p.non_participant_count,
        p.non_participant_weight,
        t.yes_weight,
        t.yes_count,
        t.no_weight,
        t.no_count,
        t.abstain_weight,
        t.abstain_count,
        render_requirement(&q.requirement),
        q.actual_weight,
        if q.met { "met" } else { "NOT met" },
        th.yes_weight,
        th.required_fraction,
        th.basis,
        th.denominator_weight,
        th.comparison,
        if th.met { "met" } else { "NOT met" },
    )
}

fn render_requirement(requirement: &bornite_eval::QuorumRequirementV1) -> String {
    use bornite_eval::QuorumRequirementV1 as Q;
    match requirement {
        Q::None => "none".to_owned(),
        Q::Absolute { weight } => format!("at least {weight}"),
        Q::Fraction {
            fraction,
            basis,
            basis_weight,
        } => {
            format!("at least {fraction} of {basis:?} weighing {basis_weight}")
        }
    }
}
