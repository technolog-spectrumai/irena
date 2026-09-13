//! The resolution commands: the step that turns a channel's approval — a passed vote
//! or a signed decision — into company change.
//!
//! Like a vote or a meeting, a resolution is carried as a **state file**. Two things
//! reach the chain: the resolution record at `finalize`, and — for an amendment
//! resolution — the amendment plus its execution record at `execute`.

use crate::{Cli, EXIT_FINDING, EXIT_OK, NotaryArgs, emit, open, read, read_key, timestamp_for};
use irena_resolution::{
    AmendmentTargetV1, AuthorityV1, ResolutionKindV1, ResolutionV1, proposal_digest,
    verify_execution, verify_resolution,
};
use prunella_canonical::Canonical;
use prunella_core::{Hash, TxId};
use serde_json::json;
use std::path::{Path, PathBuf};

/// One step of a resolution.
#[derive(clap::Subcommand, Debug)]
pub(crate) enum ResolutionCommand {
    /// Print the proposal digest of an amendment body.
    ///
    /// This is the digest the meeting's agenda item must carry, so that the vote
    /// commits to exactly the body a resolution will later execute.
    Digest {
        /// The `<share-structure>` or `<decision-channels>` document.
        #[arg(long)]
        file: PathBuf,
    },
    /// Draft a resolution on a channel's approval: a passed vote at a meeting of a
    /// collective channel (`--meeting`, `--item`, `--vote`), or a signed decision of
    /// an individual channel (`--decision`).
    Create {
        /// The channel that approved.
        #[arg(long)]
        channel: String,
        /// The transaction carrying the meeting's final record.
        #[arg(long, requires_all = ["item", "vote"], conflicts_with = "decision")]
        meeting: Option<String>,
        /// The agenda item number the resolution rests on.
        #[arg(long, requires = "meeting")]
        item: Option<u32>,
        /// The transaction carrying that item's final vote record.
        #[arg(long, requires = "meeting")]
        vote: Option<String>,
        /// The transaction carrying the final decision record.
        #[arg(long)]
        decision: Option<String>,
        /// The resolution's title.
        #[arg(long)]
        title: String,
        /// `share-structure` or `decision-channels`, with `--file`. Omit for a declarative
        /// resolution, which takes `--document-digest`.
        #[arg(long, requires = "file")]
        target: Option<String>,
        /// The amendment body to authorise.
        #[arg(long, requires = "target", conflicts_with = "document_digest")]
        file: Option<PathBuf>,
        /// 64-character hex digest of the decision document, for a declarative
        /// resolution.
        #[arg(long)]
        document_digest: Option<String>,
        /// Where to write the resolution.
        #[arg(long)]
        state: PathBuf,
    },
    /// Record the resolution, checking its authority against the chain.
    Finalize {
        #[arg(long)]
        state: PathBuf,
        /// Hex seed file, as written by `prunella keygen`.
        #[arg(long)]
        signing_key: PathBuf,
        /// Block timestamp in milliseconds. Defaults to the system clock.
        #[arg(long)]
        timestamp: Option<u64>,
        #[command(flatten)]
        notary: NotaryArgs,
    },
    /// Execute an amendment resolution: publish the amendment it authorises.
    Execute {
        #[arg(long)]
        state: PathBuf,
        #[arg(long)]
        signing_key: PathBuf,
        #[arg(long)]
        timestamp: Option<u64>,
        #[command(flatten)]
        notary: NotaryArgs,
    },
    /// Show where a resolution is.
    Show {
        #[arg(long)]
        state: PathBuf,
    },
    /// Verify a resolution, or an execution, from nothing but the chain.
    Verify {
        /// The transaction id of a resolution record.
        #[arg(long, conflicts_with = "execution")]
        tx: Option<String>,
        /// The transaction id of an execution record. Verifies its resolution too.
        #[arg(long)]
        execution: Option<String>,
    },
}

pub(crate) fn run(cli: &Cli, command: &ResolutionCommand) -> Result<u8, String> {
    match command {
        ResolutionCommand::Digest { file } => {
            let digest = proposal_digest(&read(file)?);
            emit(
                cli.json,
                &format!(
                    "{digest}\nuse this as the agenda item's --proposal-digest so the vote commits to this body"
                ),
                &json!({ "proposal_digest": digest, "file": file.display().to_string() }),
            );
            Ok(EXIT_OK)
        }
        ResolutionCommand::Create {
            channel,
            meeting,
            item,
            vote,
            decision,
            title,
            target,
            file,
            document_digest,
            state,
        } => {
            let channel = irena_core::ChannelIdV1::new(channel)
                .map_err(|e| format!("--channel: {e}"))?
                .as_str()
                .to_owned();
            let authority = match (meeting, item, vote, decision) {
                (Some(meeting), Some(item), Some(vote), None) => AuthorityV1::Collective {
                    channel,
                    meeting_tx: TxId::from_hex(meeting).map_err(|e| format!("--meeting: {e}"))?,
                    item_number: *item,
                    vote_tx: TxId::from_hex(vote).map_err(|e| format!("--vote: {e}"))?,
                },
                (None, None, None, Some(decision)) => AuthorityV1::Individual {
                    channel,
                    decision_tx: TxId::from_hex(decision)
                        .map_err(|e| format!("--decision: {e}"))?,
                },
                _ => {
                    return Err(
                        "give either --meeting, --item and --vote (a collective channel's vote) or --decision (an individual channel's decision)"
                            .to_owned(),
                    );
                }
            };
            let kind = match (target, file, document_digest) {
                (Some(target), Some(file), None) => {
                    let target = AmendmentTargetV1::parse(target).ok_or_else(|| {
                        format!(
                            "--target must be share-structure or decision-channels, not {target:?}"
                        )
                    })?;
                    ResolutionKindV1::Amendment {
                        target,
                        body: read(file)?,
                    }
                }
                (None, None, Some(digest)) => ResolutionKindV1::Declarative {
                    document_digest: Hash::from_hex(digest)
                        .map_err(|e| format!("--document-digest: {e}"))?,
                },
                _ => {
                    return Err(
                        "give either --target with --file (an amendment) or --document-digest (declarative)"
                            .to_owned(),
                    );
                }
            };
            let resolution = ResolutionV1::draft(title.clone(), authority, kind);
            save(state, &resolution)?;
            report(cli, &resolution, &format!("drafted {title:?}"))
        }
        ResolutionCommand::Finalize {
            state,
            signing_key,
            timestamp,
            notary,
        } => {
            let store = open(&cli.chain)?;
            let mut resolution = load(state)?;
            let timestamp = timestamp_for(&store, *timestamp)?;
            let id = resolution
                .finalize(&store, &read_key(signing_key)?, &notary.build()?, timestamp)
                .map_err(|e| e.to_string())?;
            save(state, &resolution)?;
            report(
                cli,
                &resolution,
                &format!(
                    "recorded for {} at height {}\nresolution: {id}",
                    resolution.company(),
                    resolution.finalized().expect("finalized").1
                ),
            )
        }
        ResolutionCommand::Execute {
            state,
            signing_key,
            timestamp,
            notary,
        } => {
            let store = open(&cli.chain)?;
            let mut resolution = load(state)?;
            let timestamp = timestamp_for(&store, *timestamp)?;
            let executed = resolution
                .execute(&store, &read_key(signing_key)?, &notary.build()?, timestamp)
                .map_err(|e| e.to_string())?;
            save(state, &resolution)?;
            let target = resolution
                .kind()
                .target()
                .expect("an executed resolution amends");
            emit(
                cli.json,
                &format!(
                    "executed at height {}\n{target} amendment: {} (height {}, replacing {})\nexecution record: {}",
                    executed.execution_height,
                    executed.amendment_tx,
                    executed.amendment_height,
                    executed.replaced_tx,
                    executed.execution_tx
                ),
                &serde_json::to_value(executed).map_err(|e| e.to_string())?,
            );
            Ok(EXIT_OK)
        }
        ResolutionCommand::Show { state } => {
            let resolution = load(state)?;
            report(cli, &resolution, "")
        }
        ResolutionCommand::Verify { tx, execution } => {
            let store = open(&cli.chain)?;
            match (tx, execution) {
                (Some(tx), None) => {
                    let tx_id = TxId::from_hex(tx).map_err(|e| format!("--tx: {e}"))?;
                    let verification =
                        verify_resolution(&store, &tx_id).map_err(|e| e.to_string())?;
                    let mut lines = vec![format!(
                        "verification of resolution {tx_id} at height {}",
                        verification.height
                    )];
                    for check in &verification.checks {
                        lines.push(describe(check));
                    }
                    lines.push(verdict(
                        verification.is_valid(),
                        verification.failures().count(),
                        "the resolution rests on exactly what the chain says",
                    ));
                    emit(
                        cli.json,
                        &lines.join("\n"),
                        &serde_json::to_value(&verification).map_err(|e| e.to_string())?,
                    );
                    Ok(finding(verification.is_valid()))
                }
                (None, Some(tx)) => {
                    let tx_id = TxId::from_hex(tx).map_err(|e| format!("--execution: {e}"))?;
                    let verification =
                        verify_execution(&store, &tx_id).map_err(|e| e.to_string())?;
                    let mut lines = vec![format!(
                        "verification of execution {tx_id} at height {}",
                        verification.height
                    )];
                    if let Some(resolution) = &verification.resolution {
                        lines.push(format!("  resolution {}:", resolution.tx_id));
                        for check in &resolution.checks {
                            lines.push(format!("  {}", describe(check)));
                        }
                    }
                    for check in &verification.checks {
                        lines.push(describe(check));
                    }
                    let failed = verification.failures().count()
                        + verification
                            .resolution
                            .as_ref()
                            .map_or(0, |r| r.failures().count());
                    lines.push(verdict(
                        verification.is_valid(),
                        failed,
                        "the amendment is exactly what the channel authorised",
                    ));
                    emit(
                        cli.json,
                        &lines.join("\n"),
                        &serde_json::to_value(&verification).map_err(|e| e.to_string())?,
                    );
                    Ok(finding(verification.is_valid()))
                }
                _ => Err("give exactly one of --tx (a resolution) or --execution".to_owned()),
            }
        }
    }
}

fn describe(check: &irena_resolution::ResolutionCheckV1) -> String {
    format!(
        "  {} {:<28} {}",
        if check.passed { "ok  " } else { "FAIL" },
        check.name,
        check.detail
    )
}

fn verdict(valid: bool, failed: usize, success: &str) -> String {
    if valid {
        success.to_owned()
    } else {
        format!("{failed} check(s) failed")
    }
}

const fn finding(valid: bool) -> u8 {
    if valid { EXIT_OK } else { EXIT_FINDING }
}

fn load(path: &Path) -> Result<ResolutionV1, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    ResolutionV1::from_canonical_bytes(&bytes)
        .map_err(|e| format!("{} is not a resolution: {e}", path.display()))
}

fn save(path: &Path, resolution: &ResolutionV1) -> Result<(), String> {
    std::fs::write(path, resolution.canonical_bytes())
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

fn report(cli: &Cli, resolution: &ResolutionV1, headline: &str) -> Result<u8, String> {
    let mut lines = Vec::new();
    if !headline.is_empty() {
        lines.push(headline.to_owned());
    }
    lines.push(format!("status:     {}", resolution.status()));
    lines.push(format!("title:      {}", resolution.title()));
    lines.push(format!("kind:       {}", resolution.kind().as_str()));
    if let Some(target) = resolution.kind().target() {
        lines.push(format!("target:     {target}"));
    }
    if !resolution.company().is_empty() {
        lines.push(format!("company:    {}", resolution.company()));
    }
    match resolution.authority() {
        AuthorityV1::Collective {
            channel,
            meeting_tx,
            item_number,
            vote_tx,
        } => {
            lines.push(format!("channel:    {channel} (collective)"));
            lines.push(format!("meeting:    {meeting_tx}"));
            lines.push(format!("item:       {item_number}"));
            lines.push(format!("vote:       {vote_tx}"));
        }
        AuthorityV1::Individual {
            channel,
            decision_tx,
        } => {
            lines.push(format!("channel:    {channel} (individual)"));
            lines.push(format!("decision:   {decision_tx}"));
        }
    }
    lines.push(format!(
        "approved:   {}",
        resolution.kind().approved_digest()
    ));
    if let Some(id) = resolution.id() {
        lines.push(format!("resolution: {id}"));
    }
    if let Some(executed) = resolution.executed() {
        lines.push(format!(
            "amendment:  {} (height {}, replacing {})",
            executed.amendment_tx, executed.amendment_height, executed.replaced_tx
        ));
        lines.push(format!("execution:  {}", executed.execution_tx));
    }
    emit(
        cli.json,
        &lines.join("\n"),
        &json!({
            "status": resolution.status(),
            "title": resolution.title(),
            "kind": resolution.kind().as_str(),
            "target": resolution.kind().target(),
            "company": resolution.company(),
            "authority": resolution.authority(),
            "approved_digest": resolution.kind().approved_digest(),
            "resolution_id": resolution.id(),
            "finalized": resolution.finalized().map(|(tx, h)| json!({ "tx_id": tx, "height": h })),
            "executed": resolution.executed(),
        }),
    );
    Ok(EXIT_OK)
}
