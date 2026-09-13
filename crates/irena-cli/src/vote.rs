//! The vote commands: a vote as a state file carried from step to step.
//!
//! A vote lives in a file (`--state`) holding the vote's canonical bytes, so each
//! lifecycle step is one invocation and the same file read on another day or another
//! machine is the same vote. Ballots are files too, so a holder signs on their own
//! machine and hands the file over.

use crate::{Cli, EXIT_FINDING, EXIT_OK, emit, open, read_key, timestamp_for};
use bornite_core::VoterIdV1;
use irena_vote::{BallotChoiceV1, SignedBallotV1, VoteV1, verify};
use prunella_canonical::Canonical;
use prunella_core::{Hash, TxId};
use serde_json::json;
use std::path::{Path, PathBuf};

/// One step of a vote.
#[derive(clap::Subcommand, Debug)]
pub(crate) enum VoteCommand {
    /// Start a vote: what about, and the digest of the proposal. The company is the
    /// chain's.
    New {
        /// What is being voted on, as a label. Never interpreted.
        #[arg(long)]
        subject: String,
        /// 64-character hex digest of the proposal document.
        #[arg(long)]
        proposal_digest: String,
        /// Where to write the vote.
        #[arg(long)]
        state: PathBuf,
    },
    /// Freeze the vote against the company as it is at a height.
    Freeze {
        #[arg(long)]
        state: PathBuf,
        /// The height to resolve the company at. Defaults to the head.
        #[arg(long)]
        at: Option<u64>,
    },
    /// Open the vote for ballots.
    Open {
        #[arg(long)]
        state: PathBuf,
    },
    /// Sign a ballot for a frozen vote, as a holder.
    Ballot {
        #[arg(long)]
        state: PathBuf,
        /// The holder's id in the share register.
        #[arg(long)]
        voter: String,
        /// `yes`, `no` or `abstain`.
        #[arg(long)]
        choice: String,
        /// The holder's hex seed file.
        #[arg(long)]
        signing_key: PathBuf,
        /// Where to write the signed ballot.
        #[arg(long)]
        out: PathBuf,
    },
    /// Accept a signed ballot into an open vote.
    Cast {
        #[arg(long)]
        state: PathBuf,
        /// A ballot file written by `vote ballot`.
        #[arg(long)]
        ballot: PathBuf,
    },
    /// Close the vote to further ballots.
    Close {
        #[arg(long)]
        state: PathBuf,
    },
    /// Count the vote against the frozen rules and electorate.
    Evaluate {
        #[arg(long)]
        state: PathBuf,
    },
    /// Write the final record to the chain.
    Finalize {
        #[arg(long)]
        state: PathBuf,
        /// Hex seed file for the transaction that carries the record.
        #[arg(long)]
        signing_key: PathBuf,
        /// Block timestamp in milliseconds. Defaults to the system clock.
        #[arg(long)]
        timestamp: Option<u64>,
    },
    /// Show where a vote is.
    Status {
        #[arg(long)]
        state: PathBuf,
    },
    /// Verify a final record from nothing but the chain and its transaction id.
    Verify {
        /// The transaction id of the final record.
        #[arg(long)]
        tx: String,
    },
}

pub(crate) fn run(cli: &Cli, command: &VoteCommand) -> Result<u8, String> {
    match command {
        VoteCommand::New {
            subject,
            proposal_digest,
            state,
        } => {
            let digest =
                Hash::from_hex(proposal_digest).map_err(|e| format!("--proposal-digest: {e}"))?;
            let vote = VoteV1::draft(subject.clone(), digest);
            save(state, &vote)?;
            report(cli, &vote, &format!("drafted vote on {subject:?}"))
        }
        VoteCommand::Freeze { state, at } => {
            let store = open(&cli.chain)?;
            let mut vote = load(state)?;
            let at = crate::height_or_head(&store, *at)?;
            let snapshot = vote.freeze(&store, at).map_err(|e| e.to_string())?.clone();
            save(state, &vote)?;
            report(
                cli,
                &vote,
                &format!(
                    "frozen at height {at}: {} voter(s), register {}, rules {}\nvote id: {}",
                    snapshot.electorate.len(),
                    snapshot.shares_tx_id,
                    snapshot.rules_tx_id,
                    snapshot.id()
                ),
            )
        }
        VoteCommand::Open { state } => {
            let mut vote = load(state)?;
            vote.open().map_err(|e| e.to_string())?;
            save(state, &vote)?;
            report(cli, &vote, "open for ballots")
        }
        VoteCommand::Ballot {
            state,
            voter,
            choice,
            signing_key,
            out,
        } => {
            let vote = load(state)?;
            let vote_id = vote
                .id()
                .ok_or("the vote is not frozen yet, so there is nothing to vote on")?;
            let voter = VoterIdV1::new(voter.clone()).map_err(|e| format!("--voter: {e}"))?;
            let choice = BallotChoiceV1::parse(choice)
                .ok_or_else(|| format!("--choice must be yes, no or abstain, not {choice:?}"))?;
            let ballot = SignedBallotV1::sign(&read_key(signing_key)?, vote_id, &voter, choice);
            std::fs::write(out, ballot.canonical_bytes())
                .map_err(|e| format!("could not write {}: {e}", out.display()))?;
            emit(
                cli.json,
                &format!(
                    "signed a {} ballot from {voter} for vote {vote_id}\nwritten to {}",
                    choice.as_str(),
                    out.display()
                ),
                &json!({ "vote_id": vote_id, "voter": voter, "choice": choice, "path": out.display().to_string() }),
            );
            Ok(EXIT_OK)
        }
        VoteCommand::Cast { state, ballot } => {
            let mut vote = load(state)?;
            let bytes = std::fs::read(ballot)
                .map_err(|e| format!("could not read {}: {e}", ballot.display()))?;
            let ballot = SignedBallotV1::from_canonical_bytes(&bytes)
                .map_err(|e| format!("{} is not a ballot: {e}", ballot.display()))?;
            let voter = ballot.body.voter.clone();
            let choice = ballot.body.choice;
            vote.cast(ballot).map_err(|e| e.to_string())?;
            save(state, &vote)?;
            report(
                cli,
                &vote,
                &format!(
                    "accepted a {} ballot from {voter}; {} ballot(s) so far",
                    choice.as_str(),
                    vote.ballots().count()
                ),
            )
        }
        VoteCommand::Close { state } => {
            let mut vote = load(state)?;
            vote.close().map_err(|e| e.to_string())?;
            save(state, &vote)?;
            report(
                cli,
                &vote,
                &format!("closed with {} ballot(s)", vote.ballots().count()),
            )
        }
        VoteCommand::Evaluate { state } => {
            let store = open(&cli.chain)?;
            let mut vote = load(state)?;
            let evaluation = vote.evaluate(&store).map_err(|e| e.to_string())?;
            save(state, &vote)?;
            emit(
                cli.json,
                &format!(
                    "evaluated: {:?} ({})\ntally:   yes {} no {} abstain {}\nquorum:  {} (participation {} of {})\nthreshold: yes {} of {} against {}, {:?}",
                    evaluation.outcome,
                    evaluation.reason,
                    evaluation.tally.yes_weight,
                    evaluation.tally.no_weight,
                    evaluation.tally.abstain_weight,
                    if evaluation.quorum.met {
                        "met"
                    } else {
                        "not met"
                    },
                    evaluation.participation.weight,
                    evaluation.electorate.effective_weight,
                    evaluation.threshold.yes_weight,
                    evaluation.threshold.denominator_weight,
                    evaluation.threshold.required_fraction,
                    evaluation.threshold.comparison,
                ),
                &json!({ "status": vote.status(), "evaluation": evaluation, "summary": vote.evaluation() }),
            );
            Ok(if evaluation.accepted() {
                EXIT_OK
            } else {
                EXIT_FINDING
            })
        }
        VoteCommand::Finalize {
            state,
            signing_key,
            timestamp,
        } => {
            let store = open(&cli.chain)?;
            let mut vote = load(state)?;
            let timestamp = timestamp_for(&store, *timestamp)?;
            let finalized = vote
                .finalize(&store, &read_key(signing_key)?, timestamp)
                .map_err(|e| e.to_string())?;
            save(state, &vote)?;
            emit(
                cli.json,
                &format!(
                    "finalized at height {} as transaction {}\noutcome: {:?}, {} ballot(s), commitment {}",
                    finalized.height,
                    finalized.tx_id,
                    finalized.record.evaluation.outcome,
                    finalized.record.ballots.len(),
                    finalized.record.ballot_commitment
                ),
                &serde_json::to_value(&finalized).map_err(|e| e.to_string())?,
            );
            Ok(EXIT_OK)
        }
        VoteCommand::Status { state } => {
            let vote = load(state)?;
            report(cli, &vote, "")
        }
        VoteCommand::Verify { tx } => {
            let store = open(&cli.chain)?;
            let tx_id = TxId::from_hex(tx).map_err(|e| format!("--tx: {e}"))?;
            let verification = verify(&store, &tx_id).map_err(|e| e.to_string())?;
            let mut lines = vec![format!(
                "verification of {tx_id} at height {}",
                verification.height
            )];
            for check in &verification.checks {
                lines.push(format!(
                    "  {} {:<24} {}",
                    if check.passed { "ok  " } else { "FAIL" },
                    format!("{:?}", check.name),
                    check.detail
                ));
            }
            lines.push(if verification.is_valid() {
                "the record is exactly what the chain says it should be".to_owned()
            } else {
                format!("{} check(s) failed", verification.failures().count())
            });
            emit(
                cli.json,
                &lines.join("\n"),
                &serde_json::to_value(&verification).map_err(|e| e.to_string())?,
            );
            Ok(if verification.is_valid() {
                EXIT_OK
            } else {
                EXIT_FINDING
            })
        }
    }
}

fn load(path: &Path) -> Result<VoteV1, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    VoteV1::from_canonical_bytes(&bytes)
        .map_err(|e| format!("{} is not a vote: {e}", path.display()))
}

fn save(path: &Path, vote: &VoteV1) -> Result<(), String> {
    std::fs::write(path, vote.canonical_bytes())
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

fn report(cli: &Cli, vote: &VoteV1, headline: &str) -> Result<u8, String> {
    let mut lines = Vec::new();
    if !headline.is_empty() {
        lines.push(headline.to_owned());
    }
    lines.push(format!("status:   {}", vote.status()));
    if !vote.company().is_empty() {
        lines.push(format!("company:  {}", vote.company()));
    }
    lines.push(format!("subject:  {}", vote.subject()));
    lines.push(format!("proposal: {}", vote.proposal_digest()));
    if let Some(snapshot) = vote.snapshot() {
        lines.push(format!("vote id:  {}", snapshot.id()));
        lines.push(format!(
            "frozen:   height {}, {} voter(s)",
            snapshot.height,
            snapshot.electorate.len()
        ));
    }
    lines.push(format!("ballots:  {}", vote.ballots().count()));
    if let Some(summary) = vote.evaluation() {
        lines.push(format!(
            "result:   {:?} ({})",
            summary.outcome, summary.reason
        ));
    }
    if let Some((tx_id, height)) = vote.finalized() {
        lines.push(format!("record:   {tx_id} at height {height}"));
    }
    emit(
        cli.json,
        &lines.join("\n"),
        &json!({
            "status": vote.status(),
            "company": vote.company(),
            "subject": vote.subject(),
            "proposal_digest": vote.proposal_digest(),
            "vote_id": vote.id(),
            "snapshot": vote.snapshot(),
            "ballots": vote.ballots().count(),
            "evaluation": vote.evaluation(),
            "finalized": vote.finalized().map(|(tx, h)| json!({ "tx_id": tx, "height": h })),
        }),
    );
    Ok(EXIT_OK)
}
