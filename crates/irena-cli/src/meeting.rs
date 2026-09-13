//! The meeting commands: a meeting as a state file carried from step to step.
//!
//! Like a vote, a meeting lives in a file (`--state`) holding its canonical bytes, so
//! each lifecycle step is one invocation. Only `convene` and `finalize` write to the
//! chain; everything between them is local.

use crate::{Cli, EXIT_FINDING, EXIT_OK, NotaryArgs, emit, open, read_key, timestamp_for};
use bornite_core::VoterIdV1;
use irena_meeting::{AgendaBodyV1, MeetingMetadataV1, ShareholderMeetingV1, verify_meeting};
use irena_vote::{BallotChoiceV1, SignedBallotV1};
use prunella_canonical::Canonical;
use prunella_core::{Hash, TxId};
use serde_json::json;
use std::path::{Path, PathBuf};

/// One step of a meeting.
#[derive(clap::Subcommand, Debug)]
pub(crate) enum MeetingCommand {
    /// Start a meeting: its title, when it is to be held, and the notice.
    New {
        /// The meeting's title.
        #[arg(long)]
        title: String,
        /// When the meeting is to be held: `YYYY-MM-DDTHH:MM:SSZ`, UTC.
        #[arg(long)]
        scheduled_at: String,
        /// 64-character hex digest of the notice of meeting.
        #[arg(long)]
        notice_digest: Option<String>,
        /// Where to write the meeting.
        #[arg(long)]
        state: PathBuf,
    },
    /// Add an agenda item. Items are numbered from 1 in the order added.
    AddItem {
        #[arg(long)]
        state: PathBuf,
        /// The item's title. For a vote item this is also the vote's subject.
        #[arg(long)]
        title: String,
        /// 64-character hex digest of the document shown to shareholders.
        #[arg(long, conflicts_with = "proposal_digest")]
        document_digest: Option<String>,
        /// 64-character hex digest of the proposal to be decided. Makes it a vote item.
        #[arg(long, conflicts_with = "document_digest")]
        proposal_digest: Option<String>,
    },
    /// Convene the meeting: put the agenda on the chain. The agenda is fixed from here.
    Convene {
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
    /// Open the meeting: freeze a vote for every vote item.
    Open {
        #[arg(long)]
        state: PathBuf,
    },
    /// Sign a ballot for one item's vote, as a holder.
    Ballot {
        #[arg(long)]
        state: PathBuf,
        /// The agenda item number.
        #[arg(long)]
        item: u32,
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
    /// Accept a signed ballot into one item's vote.
    Cast {
        #[arg(long)]
        state: PathBuf,
        #[arg(long)]
        item: u32,
        /// A ballot file written by `meeting ballot`.
        #[arg(long)]
        ballot: PathBuf,
    },
    /// Close the meeting: close and count every vote.
    Close {
        #[arg(long)]
        state: PathBuf,
    },
    /// Finalize the meeting: every vote to the chain, then the meeting record.
    Finalize {
        #[arg(long)]
        state: PathBuf,
        #[arg(long)]
        signing_key: PathBuf,
        #[arg(long)]
        timestamp: Option<u64>,
        #[command(flatten)]
        notary: NotaryArgs,
    },
    /// Show where a meeting is, with its agenda.
    Show {
        #[arg(long)]
        state: PathBuf,
    },
    /// Verify a finalised meeting from nothing but the chain and its transaction id.
    Verify {
        /// The transaction id of the final meeting record.
        #[arg(long)]
        tx: String,
    },
}

pub(crate) fn run(cli: &Cli, command: &MeetingCommand) -> Result<u8, String> {
    match command {
        MeetingCommand::New {
            title,
            scheduled_at,
            notice_digest,
            state,
        } => {
            let notice_digest = notice_digest
                .as_deref()
                .map(Hash::from_hex)
                .transpose()
                .map_err(|e| format!("--notice-digest: {e}"))?;
            let metadata = MeetingMetadataV1 {
                title: title.clone(),
                scheduled_at: scheduled_at.clone(),
                notice_digest,
            };
            metadata.validate().map_err(|e| e.to_string())?;
            let meeting = ShareholderMeetingV1::draft(metadata);
            save(state, &meeting)?;
            report(cli, &meeting, &format!("drafted the meeting {title:?}"))
        }
        MeetingCommand::AddItem {
            state,
            title,
            document_digest,
            proposal_digest,
        } => {
            let mut meeting = load(state)?;
            let body = match (document_digest, proposal_digest) {
                (Some(text), None) => AgendaBodyV1::Informational {
                    document_digest: Hash::from_hex(text)
                        .map_err(|e| format!("--document-digest: {e}"))?,
                },
                (None, Some(text)) => AgendaBodyV1::Vote {
                    proposal_digest: Hash::from_hex(text)
                        .map_err(|e| format!("--proposal-digest: {e}"))?,
                },
                _ => {
                    return Err(
                        "give exactly one of --document-digest (informational) or --proposal-digest (vote)"
                            .to_owned(),
                    );
                }
            };
            let item = meeting
                .add_item(title.clone(), body)
                .map_err(|e| e.to_string())?
                .clone();
            save(state, &meeting)?;
            report(
                cli,
                &meeting,
                &format!(
                    "added item {} ({}): {title:?}",
                    item.number,
                    item.body.kind()
                ),
            )
        }
        MeetingCommand::Convene {
            state,
            signing_key,
            timestamp,
            notary,
        } => {
            let store = open(&cli.chain)?;
            let mut meeting = load(state)?;
            let timestamp = timestamp_for(&store, *timestamp)?;
            let id = meeting
                .convene(&store, &read_key(signing_key)?, &notary.build()?, timestamp)
                .map_err(|e| e.to_string())?;
            save(state, &meeting)?;
            report(
                cli,
                &meeting,
                &format!(
                    "convened for {} at height {}\nmeeting id: {id}",
                    meeting.company(),
                    meeting.convened().expect("convened").1
                ),
            )
        }
        MeetingCommand::Open { state } => {
            let store = open(&cli.chain)?;
            let mut meeting = load(state)?;
            let at = meeting.open(&store).map_err(|e| e.to_string())?;
            save(state, &meeting)?;
            let votes: Vec<String> = meeting
                .votes()
                .map(|(number, vote)| {
                    format!("  item {number}: vote {}", vote.id().expect("frozen"))
                })
                .collect();
            report(
                cli,
                &meeting,
                &format!(
                    "opened; {} vote(s) frozen at height {at}\n{}",
                    votes.len(),
                    votes.join("\n")
                ),
            )
        }
        MeetingCommand::Ballot {
            state,
            item,
            voter,
            choice,
            signing_key,
            out,
        } => {
            let meeting = load(state)?;
            let vote = meeting
                .vote(*item)
                .ok_or_else(|| format!("item {item} has no vote; open the meeting first"))?;
            let vote_id = vote
                .id()
                .ok_or("the meeting is not open yet, so there is nothing to vote on")?;
            let voter = VoterIdV1::new(voter.clone()).map_err(|e| format!("--voter: {e}"))?;
            let choice = BallotChoiceV1::parse(choice)
                .ok_or_else(|| format!("--choice must be yes, no or abstain, not {choice:?}"))?;
            let ballot = SignedBallotV1::sign(&read_key(signing_key)?, vote_id, &voter, choice);
            std::fs::write(out, ballot.canonical_bytes())
                .map_err(|e| format!("could not write {}: {e}", out.display()))?;
            emit(
                cli.json,
                &format!(
                    "signed a {} ballot from {voter} on item {item} ({})\nwritten to {}",
                    choice.as_str(),
                    vote.subject(),
                    out.display()
                ),
                &json!({
                    "item": item,
                    "vote_id": vote_id,
                    "voter": voter,
                    "choice": choice,
                    "path": out.display().to_string(),
                }),
            );
            Ok(EXIT_OK)
        }
        MeetingCommand::Cast {
            state,
            item,
            ballot,
        } => {
            let mut meeting = load(state)?;
            let bytes = std::fs::read(ballot)
                .map_err(|e| format!("could not read {}: {e}", ballot.display()))?;
            let signed = SignedBallotV1::from_canonical_bytes(&bytes)
                .map_err(|e| format!("{} is not a ballot: {e}", ballot.display()))?;
            let voter = signed.body.voter.clone();
            let choice = signed.body.choice;
            meeting.cast(*item, signed).map_err(|e| e.to_string())?;
            let cast = meeting.vote(*item).map_or(0, |vote| vote.ballots().count());
            save(state, &meeting)?;
            report(
                cli,
                &meeting,
                &format!(
                    "accepted a {} ballot from {voter} on item {item}; {cast} ballot(s) so far",
                    choice.as_str()
                ),
            )
        }
        MeetingCommand::Close { state } => {
            let store = open(&cli.chain)?;
            let mut meeting = load(state)?;
            meeting.close(&store).map_err(|e| e.to_string())?;
            save(state, &meeting)?;
            let outcomes: Vec<String> = meeting
                .votes()
                .map(|(number, vote)| {
                    let summary = vote.evaluation().expect("counted");
                    format!(
                        "  item {number}: {} (yes {} no {} abstain {})",
                        if summary.accepted() {
                            "accepted"
                        } else {
                            "rejected"
                        },
                        summary.yes_weight,
                        summary.no_weight,
                        summary.abstain_weight
                    )
                })
                .collect();
            report(
                cli,
                &meeting,
                &format!("closed and counted\n{}", outcomes.join("\n")),
            )
        }
        MeetingCommand::Finalize {
            state,
            signing_key,
            timestamp,
            notary,
        } => {
            let store = open(&cli.chain)?;
            let mut meeting = load(state)?;
            let timestamp = timestamp_for(&store, *timestamp)?;
            let finalized = meeting
                .finalize(&store, &read_key(signing_key)?, &notary.build()?, timestamp)
                .map_err(|e| e.to_string())?;
            save(state, &meeting)?;
            let votes: Vec<String> = finalized
                .record
                .items
                .iter()
                .filter_map(|entry| {
                    entry.vote_tx_id.map(|tx| {
                        format!(
                            "  item {}: {} as {tx}",
                            entry.item.number,
                            entry.outcome.as_deref().unwrap_or("?")
                        )
                    })
                })
                .collect();
            emit(
                cli.json,
                &format!(
                    "finalized at height {} as transaction {}\nmeeting {}, {} item(s)\n{}",
                    finalized.height,
                    finalized.tx_id,
                    finalized.record.meeting_id,
                    finalized.record.items.len(),
                    votes.join("\n")
                ),
                &serde_json::to_value(&finalized).map_err(|e| e.to_string())?,
            );
            Ok(EXIT_OK)
        }
        MeetingCommand::Show { state } => {
            let meeting = load(state)?;
            report(cli, &meeting, "")
        }
        MeetingCommand::Verify { tx } => {
            let store = open(&cli.chain)?;
            let tx_id = TxId::from_hex(tx).map_err(|e| format!("--tx: {e}"))?;
            let verification = verify_meeting(&store, &tx_id).map_err(|e| e.to_string())?;
            let mut lines = vec![format!(
                "verification of meeting record {tx_id} at height {}",
                verification.height
            )];
            for check in &verification.checks {
                lines.push(format!(
                    "  {} {:<22} {}",
                    if check.passed { "ok  " } else { "FAIL" },
                    format!("{:?}", check.name),
                    check.detail
                ));
            }
            for (number, vote) in &verification.votes {
                let failed = vote.failures().count();
                lines.push(format!(
                    "  {} item {number} vote {} ({} check(s){})",
                    if vote.is_valid() { "ok  " } else { "FAIL" },
                    vote.tx_id,
                    vote.checks.len(),
                    if failed == 0 {
                        String::new()
                    } else {
                        format!(", {failed} failed")
                    }
                ));
            }
            lines.push(if verification.is_valid() {
                "the meeting is exactly what the chain says it was".to_owned()
            } else {
                format!(
                    "{} check(s) failed",
                    verification.failures().count()
                        + verification
                            .votes
                            .iter()
                            .filter(|(_, vote)| !vote.is_valid())
                            .count()
                )
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

fn load(path: &Path) -> Result<ShareholderMeetingV1, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    ShareholderMeetingV1::from_canonical_bytes(&bytes)
        .map_err(|e| format!("{} is not a meeting: {e}", path.display()))
}

fn save(path: &Path, meeting: &ShareholderMeetingV1) -> Result<(), String> {
    std::fs::write(path, meeting.canonical_bytes())
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

fn report(cli: &Cli, meeting: &ShareholderMeetingV1, headline: &str) -> Result<u8, String> {
    let mut lines = Vec::new();
    if !headline.is_empty() {
        lines.push(headline.to_owned());
    }
    lines.push(format!("status:    {}", meeting.status()));
    lines.push(format!("title:     {}", meeting.metadata().title));
    lines.push(format!("scheduled: {}", meeting.metadata().scheduled_at));
    if !meeting.company().is_empty() {
        lines.push(format!("company:   {}", meeting.company()));
    }
    if let Some(id) = meeting.id() {
        lines.push(format!("meeting:   {id}"));
    }
    if let Some(at) = meeting.opened_at() {
        lines.push(format!("opened at: height {at}"));
    }
    lines.push(format!("agenda:    {} item(s)", meeting.items().len()));
    for item in meeting.items() {
        let vote = meeting.vote(item.number);
        lines.push(format!(
            "  {:>2}. {:<11} {}{}",
            item.number,
            item.body.kind(),
            item.title,
            vote.map_or_else(String::new, |vote| {
                let ballots = vote.ballots().count();
                match vote.evaluation() {
                    Some(summary) => format!(
                        "  [{}, {ballots} ballot(s)]",
                        if summary.accepted() {
                            "accepted"
                        } else {
                            "rejected"
                        }
                    ),
                    None => format!("  [{ballots} ballot(s)]"),
                }
            })
        ));
    }
    if let Some((tx_id, height)) = meeting.finalized() {
        lines.push(format!("record:    {tx_id} at height {height}"));
    }
    emit(
        cli.json,
        &lines.join("\n"),
        &json!({
            "status": meeting.status(),
            "title": meeting.metadata().title,
            "scheduled_at": meeting.metadata().scheduled_at,
            "notice_digest": meeting.metadata().notice_digest,
            "company": meeting.company(),
            "meeting_id": meeting.id(),
            "convened": meeting.convened().map(|(tx, h)| json!({ "tx_id": tx, "height": h })),
            "opened_at": meeting.opened_at(),
            "items": meeting.items().iter().map(|item| {
                let vote = meeting.vote(item.number);
                json!({
                    "number": item.number,
                    "kind": item.body.kind(),
                    "title": item.title,
                    "digest": item.body.digest(),
                    "vote_id": vote.and_then(irena_vote::VoteV1::id),
                    "ballots": vote.map(|v| v.ballots().count()),
                    "outcome": vote.and_then(|v| v.evaluation().map(|s| {
                        if s.accepted() { "accepted" } else { "rejected" }
                    })),
                    "vote_tx_id": vote.and_then(|v| v.finalized().map(|(tx, _)| tx)),
                })
            }).collect::<Vec<_>>(),
            "finalized": meeting.finalized().map(|(tx, h)| json!({ "tx_id": tx, "height": h })),
        }),
    );
    Ok(EXIT_OK)
}
