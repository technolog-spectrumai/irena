//! The decision commands: an individual decision as a state file carried from step
//! to step.
//!
//! The individual counterpart of `vote`. A decision lives in a file (`--state`)
//! holding its canonical bytes; the sole actor of an individual channel signs it on
//! their own machine, and it is finalised to the chain like a vote's final record.

use crate::{Cli, EXIT_FINDING, EXIT_OK, emit, open, read_key, timestamp_for};
use irena_core::ChannelIdV1;
use irena_decision::{DecisionV1, verify_decision};
use prunella_canonical::Canonical;
use prunella_core::{Hash, TxId};
use serde_json::json;
use std::path::{Path, PathBuf};

/// One step of an individual decision.
#[derive(clap::Subcommand, Debug)]
pub(crate) enum DecisionCommand {
    /// Start a decision: what about, and the digest of the proposal.
    New {
        /// What is being decided, as a label. Never interpreted.
        #[arg(long)]
        subject: String,
        /// 64-character hex digest of the proposal document.
        #[arg(long)]
        proposal_digest: String,
        /// Where to write the decision.
        #[arg(long)]
        state: PathBuf,
    },
    /// Freeze the decision against the company as it is at a height, through an
    /// individual channel: the channel's sole actor and key are fixed from here.
    Freeze {
        #[arg(long)]
        state: PathBuf,
        /// The individual channel to decide through.
        #[arg(long)]
        channel: String,
        /// The height to resolve the company at. Defaults to the head.
        #[arg(long)]
        at: Option<u64>,
    },
    /// Sign the frozen decision as its actor.
    Sign {
        #[arg(long)]
        state: PathBuf,
        /// The actor's hex seed file.
        #[arg(long)]
        signing_key: PathBuf,
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
    /// Show where a decision is.
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

pub(crate) fn run(cli: &Cli, command: &DecisionCommand) -> Result<u8, String> {
    match command {
        DecisionCommand::New {
            subject,
            proposal_digest,
            state,
        } => {
            let digest =
                Hash::from_hex(proposal_digest).map_err(|e| format!("--proposal-digest: {e}"))?;
            let decision = DecisionV1::draft(subject.clone(), digest);
            save(state, &decision)?;
            report(cli, &decision, &format!("drafted decision on {subject:?}"))
        }
        DecisionCommand::Freeze { state, channel, at } => {
            let store = open(&cli.chain)?;
            let mut decision = load(state)?;
            let channel = ChannelIdV1::new(channel).map_err(|e| format!("--channel: {e}"))?;
            let at = crate::height_or_head(&store, *at)?;
            let snapshot = decision
                .freeze(&store, at, &channel)
                .map_err(|e| e.to_string())?
                .clone();
            save(state, &decision)?;
            report(
                cli,
                &decision,
                &format!(
                    "frozen at height {at} through channel {channel}: actor {} (key {})\nregister {}, channels {}\ndecision id: {}",
                    snapshot.actor,
                    snapshot.key,
                    snapshot.shares_tx_id,
                    snapshot.channels_tx_id,
                    snapshot.id()
                ),
            )
        }
        DecisionCommand::Sign { state, signing_key } => {
            let mut decision = load(state)?;
            decision
                .sign(&read_key(signing_key)?)
                .map_err(|e| e.to_string())?;
            save(state, &decision)?;
            report(cli, &decision, "signed by the actor")
        }
        DecisionCommand::Finalize {
            state,
            signing_key,
            timestamp,
        } => {
            let store = open(&cli.chain)?;
            let mut decision = load(state)?;
            let timestamp = timestamp_for(&store, *timestamp)?;
            let finalized = decision
                .finalize(&store, &read_key(signing_key)?, timestamp)
                .map_err(|e| e.to_string())?;
            save(state, &decision)?;
            report(
                cli,
                &decision,
                &format!(
                    "recorded at height {} as {}",
                    finalized.height, finalized.tx_id
                ),
            )
        }
        DecisionCommand::Status { state } => {
            let decision = load(state)?;
            report(cli, &decision, "")
        }
        DecisionCommand::Verify { tx } => {
            let store = open(&cli.chain)?;
            let tx = TxId::from_hex(tx).map_err(|e| format!("--tx: {e}"))?;
            let report = verify_decision(&store, &tx).map_err(|e| e.to_string())?;
            let mut lines = vec![format!(
                "decision record {} at height {}",
                report.tx_id, report.height
            )];
            for check in &report.checks {
                lines.push(format!(
                    "  {} {:<24} {}",
                    if check.passed { "ok  " } else { "FAIL" },
                    format!("{:?}", check.name),
                    check.detail
                ));
            }
            let failed = report.failures().count();
            lines.push(if report.is_valid() {
                format!(
                    "VALID: {} check(s) re-established from the chain",
                    report.checks.len()
                )
            } else {
                format!("INVALID: {failed} check(s) failed")
            });
            emit(
                cli.json,
                &lines.join("\n"),
                &serde_json::to_value(&report).map_err(|e| e.to_string())?,
            );
            Ok(if report.is_valid() {
                EXIT_OK
            } else {
                EXIT_FINDING
            })
        }
    }
}

fn load(path: &Path) -> Result<DecisionV1, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    DecisionV1::from_canonical_bytes(&bytes)
        .map_err(|e| format!("{} is not a decision: {e}", path.display()))
}

fn save(path: &Path, decision: &DecisionV1) -> Result<(), String> {
    std::fs::write(path, decision.canonical_bytes())
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

fn report(cli: &Cli, decision: &DecisionV1, headline: &str) -> Result<u8, String> {
    let mut lines = Vec::new();
    if !headline.is_empty() {
        lines.push(headline.to_owned());
    }
    lines.push(format!("status:   {}", decision.status()));
    lines.push(format!("subject:  {}", decision.subject()));
    lines.push(format!("proposal: {}", decision.proposal_digest()));
    if let Some(snapshot) = decision.snapshot() {
        lines.push(format!("company:  {}", snapshot.company));
        lines.push(format!("channel:  {}", snapshot.channel));
        lines.push(format!("actor:    {}", snapshot.actor));
        lines.push(format!("height:   {}", snapshot.height));
        lines.push(format!("id:       {}", snapshot.id()));
    }
    if let Some((tx, height)) = decision.finalized() {
        lines.push(format!("record:   {tx} (height {height})"));
    }
    emit(
        cli.json,
        &lines.join("\n"),
        &json!({
            "status": decision.status(),
            "subject": decision.subject(),
            "proposal_digest": decision.proposal_digest(),
            "snapshot": decision.snapshot(),
            "decision_id": decision.id(),
            "signed": decision.signature().is_some(),
            "finalized": decision.finalized().map(|(tx, h)| json!({ "tx_id": tx, "height": h })),
        }),
    );
    Ok(EXIT_OK)
}
