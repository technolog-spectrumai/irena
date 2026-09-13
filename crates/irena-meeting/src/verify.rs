//! Independent verification of a meeting from the chain alone.

use crate::error::MeetingError;
use crate::meeting::{AgendaBodyV1, AgendaV1, MeetingMetadataV1};
use crate::record::{
    MEETING_NAMESPACE, MeetingFinalRecordV1, MeetingRecordBodyV1, MeetingRecordV1,
    read_meeting_record,
};
use irena_ledger::reconstruct;
use irena_vote::{VerificationV1, verify};
use prunella_core::{BlockHeight, TxId};
use prunella_store::LocalChainStore;

/// The checks a verifier runs, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MeetingCheckNameV1 {
    /// The transaction is in the meeting namespace and holds a final meeting record.
    Decodes,
    /// The convening transaction exists, is a convening record, and is for the same
    /// company as the final record.
    ConveningExists,
    /// The convening precedes the opening, which precedes the final record.
    HeightsOrdered,
    /// The agenda in the final record is exactly the agenda convened.
    AgendaMatches,
    /// The metadata in the final record is exactly the metadata convened.
    MetadataMatches,
    /// The company reconstructs at the convening height and is the record's company.
    CompanyReconstructs,
    /// Every vote item names a transaction that `irena-vote` verifies.
    VotesVerify,
    /// Every referenced vote is the one this item, this company and this meeting
    /// called for: through the meeting's channel, right subject, right proposal
    /// digest, frozen at the opening height, finalised before the meeting was.
    VotesBelong,
    /// Every informational item's digest is present, and no item carries a vote
    /// reference it should not.
    ItemsConsistent,
}

/// One check and how it came out.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct MeetingCheckV1 {
    /// Which check.
    pub name: MeetingCheckNameV1,
    /// Whether it held.
    pub passed: bool,
    /// What was found, for a reader.
    pub detail: String,
}

/// The outcome of verifying one meeting.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct MeetingVerificationV1 {
    /// The transaction verified.
    pub tx_id: TxId,
    /// Its height.
    pub height: BlockHeight,
    /// The decoded record, if it decoded.
    pub record: Option<MeetingFinalRecordV1>,
    /// Every check run, in order. A check that could not run because an earlier one
    /// failed is absent, not reported as passed.
    pub checks: Vec<MeetingCheckV1>,
    /// The verification of every referenced vote, in agenda order.
    pub votes: Vec<(u32, VerificationV1)>,
}

impl MeetingVerificationV1 {
    /// Whether every check passed, here and in every vote.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.checks.is_empty()
            && self.checks.iter().all(|check| check.passed)
            && self.votes.iter().all(|(_, vote)| vote.is_valid())
    }

    /// The checks that failed.
    pub fn failures(&self) -> impl Iterator<Item = &MeetingCheckV1> {
        self.checks.iter().filter(|check| !check.passed)
    }

    fn check(&mut self, name: MeetingCheckNameV1, passed: bool, detail: impl Into<String>) -> bool {
        self.checks.push(MeetingCheckV1 {
            name,
            passed,
            detail: detail.into(),
        });
        passed
    }
}

/// Verifies the meeting finalised by transaction `tx_id`, from the chain alone.
///
/// Nothing the record says is trusted. The convening record is read back and compared
/// attribute by attribute, the company is reconstructed at the convening height, and
/// every referenced vote is verified through `irena_vote::verify` *and* checked to be
/// the vote this item called for. All checks that can run do, so a report names
/// everything wrong at once.
///
/// # Errors
///
/// [`MeetingError::NoSuchTransaction`] if the chain has no such transaction, or the
/// chain's own errors. A meeting that fails verification is an `Ok` report with failed
/// checks, not an error.
pub fn verify_meeting(
    store: &LocalChainStore,
    tx_id: &TxId,
) -> Result<MeetingVerificationV1, MeetingError> {
    let located = store
        .get_transaction(tx_id)?
        .ok_or(MeetingError::NoSuchTransaction { tx_id: *tx_id })?;
    let mut report = MeetingVerificationV1 {
        tx_id: *tx_id,
        height: located.height,
        record: None,
        checks: Vec::new(),
        votes: Vec::new(),
    };

    // 1. Decode.
    let Some(final_record_holder) = decode(&located.transaction, &mut report) else {
        return Ok(report);
    };
    let (company, record) = final_record_holder;
    report.record = Some(record.clone());

    // 2. The convening record.
    let convening_tx = record.meeting_id.tx_id();
    let Some(convening) = store.get_transaction(&convening_tx)? else {
        report.check(
            MeetingCheckNameV1::ConveningExists,
            false,
            format!("no transaction {convening_tx} is on the chain"),
        );
        return Ok(report);
    };
    let convened = match read_convening(&convening.transaction) {
        Ok(convened) => convened,
        Err(detail) => {
            report.check(MeetingCheckNameV1::ConveningExists, false, detail);
            return Ok(report);
        }
    };
    if !report.check(
        MeetingCheckNameV1::ConveningExists,
        convened.company.as_str() == company,
        if convened.company.as_str() == company {
            format!("convened at height {} for {company}", convening.height)
        } else {
            format!(
                "the convening is for company {}, the final record for {company}",
                convened.company
            )
        },
    ) {
        return Ok(report);
    }
    let MeetingRecordBodyV1::Convened { metadata, agenda } = convened.body else {
        unreachable!("read_convening refuses anything else")
    };

    // 3. Heights.
    let ordered =
        convening.height <= record.opened_at_height && record.opened_at_height < located.height;
    report.check(
        MeetingCheckNameV1::HeightsOrdered,
        ordered,
        format!(
            "convened at {}, opened at {}, finalised at {}",
            convening.height, record.opened_at_height, located.height
        ),
    );

    // 4. Agenda and 5. metadata.
    let final_agenda = record.agenda();
    let agenda_matches = matches!(&final_agenda, Ok(found) if found == &agenda);
    report.check(
        MeetingCheckNameV1::AgendaMatches,
        agenda_matches,
        if agenda_matches {
            format!("{} item(s), as convened", agenda.len())
        } else {
            describe_agenda_difference(&agenda, final_agenda.as_ref().ok())
        },
    );
    report.check(
        MeetingCheckNameV1::MetadataMatches,
        metadata == record.metadata,
        describe_metadata(&metadata, &record.metadata),
    );

    // 6. The company.
    match reconstruct(store, convening.height) {
        Ok(state) => {
            let same = state.company.as_str() == company;
            report.check(
                MeetingCheckNameV1::CompanyReconstructs,
                same,
                if same {
                    format!(
                        "{} at height {}: {} holder(s)",
                        state.company,
                        convening.height,
                        state.shares.value.len()
                    )
                } else {
                    format!(
                        "the chain holds company {}, the record claims {company}",
                        state.company
                    )
                },
            );
        }
        Err(error) => {
            report.check(
                MeetingCheckNameV1::CompanyReconstructs,
                false,
                error.to_string(),
            );
        }
    }

    // 7. and 8. Every referenced vote.
    let mut unverified = Vec::new();
    let mut foreign = Vec::new();
    for entry in &record.items {
        let expected_subject = format!("item {}: {}", entry.item.number, entry.item.title);
        let AgendaBodyV1::Vote { proposal_digest } = entry.item.body else {
            continue;
        };
        let Some(vote_tx) = entry.vote_tx_id else {
            unverified.push(format!("item {}: no vote transaction", entry.item.number));
            continue;
        };
        match verify(store, &vote_tx) {
            Ok(verification) => {
                if !verification.is_valid() {
                    let failed: Vec<String> = verification
                        .failures()
                        .map(|check| format!("{:?}", check.name))
                        .collect();
                    unverified.push(format!(
                        "item {}: vote {vote_tx} fails {}",
                        entry.item.number,
                        failed.join(", ")
                    ));
                }
                if let Some(vote) = &verification.record {
                    let snapshot = &vote.snapshot;
                    let mut wrong = Vec::new();
                    if snapshot.company != company {
                        wrong.push(format!("company {}", snapshot.company));
                    }
                    if snapshot.channel != metadata.channel {
                        wrong.push(format!(
                            "channel {}, the meeting is of {}",
                            snapshot.channel, metadata.channel
                        ));
                    }
                    if snapshot.subject != expected_subject {
                        wrong.push(format!("subject {:?}", snapshot.subject));
                    }
                    if snapshot.proposal_digest != proposal_digest {
                        wrong.push(format!("proposal {}", snapshot.proposal_digest));
                    }
                    if snapshot.height != record.opened_at_height {
                        wrong.push(format!("frozen at height {}", snapshot.height));
                    }
                    let vote_height = store
                        .get_transaction(&vote_tx)?
                        .map_or(BlockHeight(u64::MAX), |located| located.height);
                    if vote_height >= located.height {
                        wrong.push(format!(
                            "finalised at height {vote_height}, not before the meeting"
                        ));
                    }
                    let stated = if vote.evaluation.accepted() {
                        "accepted"
                    } else {
                        "rejected"
                    };
                    if entry.outcome.as_deref() != Some(stated) {
                        wrong.push(format!(
                            "outcome {:?}, the vote says {stated}",
                            entry.outcome.as_deref().unwrap_or("none")
                        ));
                    }
                    if !wrong.is_empty() {
                        foreign.push(format!("item {}: {}", entry.item.number, wrong.join("; ")));
                    }
                }
                report.votes.push((entry.item.number, verification));
            }
            Err(error) => {
                unverified.push(format!("item {}: {error}", entry.item.number));
            }
        }
    }
    let vote_items = record
        .items
        .iter()
        .filter(|entry| entry.item.body.is_vote())
        .count();
    report.check(
        MeetingCheckNameV1::VotesVerify,
        unverified.is_empty(),
        if unverified.is_empty() {
            format!("{vote_items} vote(s), each verified from the chain")
        } else {
            unverified.join("; ")
        },
    );
    report.check(
        MeetingCheckNameV1::VotesBelong,
        foreign.is_empty(),
        if foreign.is_empty() {
            format!("{vote_items} vote(s) match their agenda items")
        } else {
            foreign.join("; ")
        },
    );

    // 9. Items internally consistent.
    let mut inconsistent = Vec::new();
    for entry in &record.items {
        match entry.item.body {
            AgendaBodyV1::Informational { .. } => {
                if entry.vote_tx_id.is_some() || entry.outcome.is_some() {
                    inconsistent.push(format!(
                        "item {} is informational but carries a vote",
                        entry.item.number
                    ));
                }
            }
            AgendaBodyV1::Vote { .. } => {
                if entry.vote_tx_id.is_none() {
                    inconsistent.push(format!("item {} has no vote", entry.item.number));
                }
            }
        }
    }
    report.check(
        MeetingCheckNameV1::ItemsConsistent,
        inconsistent.is_empty(),
        if inconsistent.is_empty() {
            format!(
                "{} item(s): {vote_items} vote, {} informational",
                record.items.len(),
                record.items.len() - vote_items
            )
        } else {
            inconsistent.join("; ")
        },
    );

    Ok(report)
}

/// Decodes the final record, or records why it is not one.
fn decode(
    transaction: &prunella_core::Transaction,
    report: &mut MeetingVerificationV1,
) -> Option<(String, MeetingFinalRecordV1)> {
    if transaction.namespace.as_str() != MEETING_NAMESPACE {
        report.check(
            MeetingCheckNameV1::Decodes,
            false,
            format!(
                "transaction is in namespace {}, not {MEETING_NAMESPACE}",
                transaction.namespace
            ),
        );
        return None;
    }
    let text = match core::str::from_utf8(&transaction.payload) {
        Ok(text) => text,
        Err(error) => {
            report.check(
                MeetingCheckNameV1::Decodes,
                false,
                format!("payload is not UTF-8: {error}"),
            );
            return None;
        }
    };
    let record = match read_meeting_record(text) {
        Ok(record) => record,
        Err(error) => {
            report.check(MeetingCheckNameV1::Decodes, false, error.to_string());
            return None;
        }
    };
    match record.body {
        MeetingRecordBodyV1::Final(final_record) => {
            report.check(
                MeetingCheckNameV1::Decodes,
                true,
                format!("final record of meeting {}", final_record.meeting_id),
            );
            Some((record.company.as_str().to_owned(), final_record))
        }
        MeetingRecordBodyV1::Convened { .. } => {
            report.check(
                MeetingCheckNameV1::Decodes,
                false,
                "the transaction holds a convening record, not a final one",
            );
            None
        }
    }
}

/// Reads a convening record, or says why the transaction is not one.
fn read_convening(transaction: &prunella_core::Transaction) -> Result<MeetingRecordV1, String> {
    if transaction.namespace.as_str() != MEETING_NAMESPACE {
        return Err(format!(
            "the convening transaction is in namespace {}, not {MEETING_NAMESPACE}",
            transaction.namespace
        ));
    }
    let text = core::str::from_utf8(&transaction.payload)
        .map_err(|error| format!("the convening payload is not UTF-8: {error}"))?;
    let record = read_meeting_record(text).map_err(|error| error.to_string())?;
    if !matches!(record.body, MeetingRecordBodyV1::Convened { .. }) {
        return Err("the meeting id names a final record, not a convening".to_owned());
    }
    Ok(record)
}

fn describe_agenda_difference(convened: &AgendaV1, found: Option<&AgendaV1>) -> String {
    let Some(found) = found else {
        return "the final record's items are not an agenda".to_owned();
    };
    if found.len() != convened.len() {
        return format!(
            "the meeting was convened with {} item(s), the final record has {}",
            convened.len(),
            found.len()
        );
    }
    let differing: Vec<String> = convened
        .items()
        .iter()
        .zip(found.items())
        .filter(|(a, b)| a != b)
        .map(|(a, _)| format!("item {}", a.number))
        .collect();
    format!("{} differs from the agenda convened", differing.join(", "))
}

fn describe_metadata(convened: &MeetingMetadataV1, found: &MeetingMetadataV1) -> String {
    if convened == found {
        return format!(
            "{:?} of channel {}, scheduled {}",
            found.title, found.channel, found.scheduled_at
        );
    }
    let mut differences = Vec::new();
    if convened.channel != found.channel {
        differences.push(format!("channel {} vs {}", convened.channel, found.channel));
    }
    if convened.title != found.title {
        differences.push(format!("title {:?} vs {:?}", convened.title, found.title));
    }
    if convened.scheduled_at != found.scheduled_at {
        differences.push(format!(
            "scheduled {} vs {}",
            convened.scheduled_at, found.scheduled_at
        ));
    }
    if convened.notice_digest != found.notice_digest {
        differences.push("notice digest".to_owned());
    }
    differences.join("; ")
}
