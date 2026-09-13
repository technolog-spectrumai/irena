//! Independent verification of a resolution and its execution, from the chain alone.

use crate::demotion::self_demotion;
use crate::error::ResolutionError;
use crate::lifecycle::{company_at, find_execution};
use crate::record::{
    EXECUTION_NAMESPACE, RESOLUTION_NAMESPACE, ResolutionExecutionV1, ResolutionRecordV1,
    body_matches, read_execution_record, read_resolution_record,
};
use crate::resolution::{AmendmentTargetV1, ApprovalV1, AuthorityV1, ResolutionKindV1};
use bornite_core::VoterIdV1;
use irena_core::{RecordBodyV1, read_record};
use irena_decision::verify_decision;
use irena_meeting::verify_meeting;
use prunella_core::{BlockHeight, Transaction, TxId};
use prunella_store::LocalChainStore;

/// The checks a resolution verifier runs, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ResolutionCheckNameV1 {
    /// The transaction is in the resolution namespace and holds a V1 resolution.
    Decodes,
    /// Collective authority: the meeting it names verifies from the chain, every check.
    MeetingVerifies,
    /// Collective authority: the agenda item it names exists on that meeting and is a
    /// vote item.
    ItemIsAVote,
    /// Collective authority: the vote it names is the vote that answered that item.
    VoteAnsweredTheItem,
    /// Collective authority: that vote verifies from the chain, every check — which
    /// includes that its channel was collective at the frozen height.
    VoteVerifies,
    /// Collective authority: Bornite accepted the motion.
    VotePassed,
    /// Individual authority: the decision it names verifies from the chain, every
    /// check — which includes that its channel was individual at the frozen height
    /// and resolved to its signer.
    DecisionVerifies,
    /// The vote or decision was through the channel the resolution names.
    ChannelMatches,
    /// What the resolution carries is what was approved: its digest is the proposal
    /// digest the channel decided on.
    ProposalMatches,
    /// The resolution is for the company this chain holds, and so is the approval.
    CompanyMatches,
    /// The meeting or decision was recorded before the resolution was.
    HeightsOrdered,
}

/// The checks an execution verifier runs, in order. The resolution's own checks run
/// first and are reported alongside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExecutionCheckNameV1 {
    /// The transaction is in the execution namespace and holds a V1 execution.
    Decodes,
    /// The resolution it names verifies, every check.
    ResolutionVerifies,
    /// That resolution is an amendment resolution, and for the same target.
    ResolutionAuthorisesThis,
    /// The amendment it names is an ordinary company record of that kind, for this
    /// company.
    AmendmentExists,
    /// The amendment's body is byte for byte what the resolution carries, and digests
    /// to what the vote approved.
    AmendmentMatchesResolution,
    /// The amendment superseded exactly the record the actors approved for
    /// replacement.
    AmendmentReplacedApprovedBase,
    /// A channel-set amendment on an individual decision satisfies the self-demotion
    /// rule: the signer's reach did not grow. Passes trivially for any other
    /// amendment, and says so.
    SelfDemotionHolds,
    /// The amendment is part of the company reconstructed at the execution's height:
    /// it actually took effect.
    AmendmentApplied,
    /// Resolution before amendment before execution.
    HeightsOrdered,
    /// No other execution of the same resolution exists on the chain.
    ExecutedOnce,
}

/// One check and how it came out.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ResolutionCheckV1 {
    /// Which check, as text so one type serves both verifiers.
    pub name: String,
    /// Whether it held.
    pub passed: bool,
    /// What was found, for a reader.
    pub detail: String,
}

/// The outcome of verifying one resolution.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ResolutionVerificationV1 {
    /// The transaction verified.
    pub tx_id: TxId,
    /// Its height.
    pub height: BlockHeight,
    /// The decoded record, if it decoded.
    pub record: Option<ResolutionRecordV1>,
    /// Every check run, in order.
    pub checks: Vec<ResolutionCheckV1>,
}

impl ResolutionVerificationV1 {
    /// Whether every check passed.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.checks.is_empty() && self.checks.iter().all(|check| check.passed)
    }

    /// The checks that failed.
    pub fn failures(&self) -> impl Iterator<Item = &ResolutionCheckV1> {
        self.checks.iter().filter(|check| !check.passed)
    }

    fn check(
        &mut self,
        name: ResolutionCheckNameV1,
        passed: bool,
        detail: impl Into<String>,
    ) -> bool {
        self.checks.push(ResolutionCheckV1 {
            name: format!("{name:?}"),
            passed,
            detail: detail.into(),
        });
        passed
    }
}

/// The outcome of verifying one execution.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ExecutionVerificationV1 {
    /// The transaction verified.
    pub tx_id: TxId,
    /// Its height.
    pub height: BlockHeight,
    /// The decoded record, if it decoded.
    pub record: Option<ResolutionExecutionV1>,
    /// Every check run, in order.
    pub checks: Vec<ResolutionCheckV1>,
    /// The verification of the resolution it rests on.
    pub resolution: Option<ResolutionVerificationV1>,
}

impl ExecutionVerificationV1 {
    /// Whether every check passed, here and in the resolution.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.checks.is_empty()
            && self.checks.iter().all(|check| check.passed)
            && self
                .resolution
                .as_ref()
                .is_some_and(ResolutionVerificationV1::is_valid)
    }

    /// The checks that failed here.
    pub fn failures(&self) -> impl Iterator<Item = &ResolutionCheckV1> {
        self.checks.iter().filter(|check| !check.passed)
    }

    fn check(
        &mut self,
        name: ExecutionCheckNameV1,
        passed: bool,
        detail: impl Into<String>,
    ) -> bool {
        self.checks.push(ResolutionCheckV1 {
            name: format!("{name:?}"),
            passed,
            detail: detail.into(),
        });
        passed
    }
}

/// Verifies the resolution recorded by transaction `tx_id`, from the chain alone.
///
/// Nothing the record says is trusted: the meeting, the agenda item, the vote and the
/// proposal digest are each re-established from the chain.
///
/// # Errors
///
/// [`ResolutionError::NoSuchTransaction`], or the chain's own errors. A resolution
/// that fails verification is an `Ok` report with failed checks, not an error.
pub fn verify_resolution(
    store: &LocalChainStore,
    tx_id: &TxId,
) -> Result<ResolutionVerificationV1, ResolutionError> {
    let located = store
        .get_transaction(tx_id)?
        .ok_or(ResolutionError::NoSuchTransaction { tx_id: *tx_id })?;
    let mut report = ResolutionVerificationV1 {
        tx_id: *tx_id,
        height: located.height,
        record: None,
        checks: Vec::new(),
    };
    let Some(record) = decode_resolution(&located.transaction, &mut report) else {
        return Ok(report);
    };
    report.record = Some(record.clone());
    verify_authority(store, &record, located.height, &mut report)?;
    Ok(report)
}

/// The authority checks, shared by both verifiers. Returns the approval when every
/// check that depends on it held.
fn verify_authority(
    store: &LocalChainStore,
    record: &ResolutionRecordV1,
    at: BlockHeight,
    report: &mut ResolutionVerificationV1,
) -> Result<Option<ApprovalV1>, ResolutionError> {
    let (approval, decided_height) = match &record.authority {
        AuthorityV1::Collective {
            channel,
            meeting_tx,
            item_number,
            vote_tx,
        } => {
            let Some(vote) = verify_collective(store, *meeting_tx, *item_number, *vote_tx, report)?
            else {
                return Ok(None);
            };
            let decided_height = store
                .get_transaction(meeting_tx)?
                .map(|located| located.height);
            (
                ApprovalV1 {
                    channel: channel.clone(),
                    actor: None,
                    company: vote.snapshot.company.clone(),
                    proposal_digest: vote.snapshot.proposal_digest,
                    height: vote.snapshot.height,
                    shares_tx_id: vote.snapshot.shares_tx_id,
                    channels_tx_id: vote.snapshot.channels_tx_id,
                    through_tx: *vote_tx,
                },
                (
                    decided_height,
                    "meeting finalised",
                    vote.snapshot.channel.clone(),
                ),
            )
        }
        AuthorityV1::Individual {
            channel,
            decision_tx,
        } => {
            let verification = verify_decision(store, decision_tx)?;
            if !report.check(
                ResolutionCheckNameV1::DecisionVerifies,
                verification.is_valid(),
                if verification.is_valid() {
                    format!("decision {decision_tx} verifies")
                } else {
                    verification
                        .failures()
                        .map(|check| format!("{:?}", check.name))
                        .collect::<Vec<_>>()
                        .join(", ")
                },
            ) {
                return Ok(None);
            }
            let Some(decision) = verification.record else {
                report.check(
                    ResolutionCheckNameV1::ChannelMatches,
                    false,
                    "the decision transaction holds no final record",
                );
                return Ok(None);
            };
            (
                ApprovalV1 {
                    channel: channel.clone(),
                    actor: Some(decision.snapshot.actor.clone()),
                    company: decision.snapshot.company.clone(),
                    proposal_digest: decision.snapshot.proposal_digest,
                    height: decision.snapshot.height,
                    shares_tx_id: decision.snapshot.shares_tx_id,
                    channels_tx_id: decision.snapshot.channels_tx_id,
                    through_tx: *decision_tx,
                },
                (
                    Some(verification.height),
                    "decision recorded",
                    decision.snapshot.channel.clone(),
                ),
            )
        }
    };
    let (decided_height, decided_label, decided_channel) = decided_height;

    // The channel.
    let same_channel = decided_channel == approval.channel;
    report.check(
        ResolutionCheckNameV1::ChannelMatches,
        same_channel,
        if same_channel {
            format!("{} channel {}", record.authority.mode(), approval.channel)
        } else {
            format!(
                "the resolution names channel {}, the record was decided through {decided_channel}",
                approval.channel
            )
        },
    );

    // What was approved.
    let approved = approval.proposal_digest;
    let carried = record.kind.approved_digest();
    report.check(
        ResolutionCheckNameV1::ProposalMatches,
        carried == approved,
        if carried == approved {
            match &record.kind {
                ResolutionKindV1::Declarative { .. } => {
                    format!("the decision document approved, {approved}")
                }
                ResolutionKindV1::Amendment { target, body } => format!(
                    "the {target} body approved, {approved} ({} bytes)",
                    body.len()
                ),
            }
        } else {
            format!("the channel approved {approved}, the resolution carries {carried}")
        },
    );

    // The company.
    let chain = company_at(store, at)?;
    let same = record.company == chain.company && approval.company == chain.company.as_str();
    report.check(
        ResolutionCheckNameV1::CompanyMatches,
        same,
        if same {
            format!("{}", chain.company)
        } else {
            format!(
                "chain holds {}, resolution says {}, approval says {}",
                chain.company, record.company, approval.company
            )
        },
    );

    // Heights.
    let ordered = decided_height.is_some_and(|height| height < at);
    report.check(
        ResolutionCheckNameV1::HeightsOrdered,
        ordered,
        match decided_height {
            Some(height) => format!("{decided_label} at {height}, resolution at {at}"),
            None => "the authority's transaction is not on the chain".to_owned(),
        },
    );

    Ok(Some(approval))
}

/// The collective authority checks: meeting, item, vote, result.
fn verify_collective(
    store: &LocalChainStore,
    meeting_tx: TxId,
    item_number: u32,
    vote_tx: TxId,
    report: &mut ResolutionVerificationV1,
) -> Result<Option<irena_vote::FinalVoteRecordV1>, ResolutionError> {
    // The meeting.
    let meeting = verify_meeting(store, &meeting_tx)?;
    if !report.check(
        ResolutionCheckNameV1::MeetingVerifies,
        meeting.is_valid(),
        if meeting.is_valid() {
            format!(
                "meeting {} verifies at height {}",
                meeting
                    .record
                    .as_ref()
                    .map_or_else(|| meeting_tx.to_string(), |r| r.meeting_id.to_string()),
                meeting.height
            )
        } else {
            meeting
                .failures()
                .map(|check| format!("{:?}", check.name))
                .collect::<Vec<_>>()
                .join(", ")
        },
    ) {
        return Ok(None);
    }
    let Some(final_record) = meeting.record else {
        report.check(
            ResolutionCheckNameV1::ItemIsAVote,
            false,
            "the meeting transaction holds no final record",
        );
        return Ok(None);
    };

    // The agenda item.
    let entry = final_record
        .items
        .iter()
        .find(|entry| entry.item.number == item_number);
    let is_vote = entry.is_some_and(|entry| entry.item.body.is_vote());
    if !report.check(
        ResolutionCheckNameV1::ItemIsAVote,
        is_vote,
        match entry {
            None => format!("the meeting has no item {item_number}"),
            Some(entry) if !is_vote => format!(
                "item {} is informational: {:?}",
                entry.item.number, entry.item.title
            ),
            Some(entry) => format!("item {}: {:?}", entry.item.number, entry.item.title),
        },
    ) {
        return Ok(None);
    }
    let entry = entry.expect("checked");

    // The vote answered that item.
    let answered = entry.vote_tx_id;
    if !report.check(
        ResolutionCheckNameV1::VoteAnsweredTheItem,
        answered == Some(vote_tx),
        match answered {
            Some(answered) if answered == vote_tx => {
                format!("item {} was answered by {answered}", entry.item.number)
            }
            Some(answered) => format!(
                "item {} was answered by {answered}, the resolution names {vote_tx}",
                entry.item.number
            ),
            None => format!("item {} has no vote", entry.item.number),
        },
    ) {
        return Ok(None);
    }

    // The vote itself.
    let verification = irena_vote::verify(store, &vote_tx)?;
    if !report.check(
        ResolutionCheckNameV1::VoteVerifies,
        verification.is_valid(),
        if verification.is_valid() {
            format!("vote {vote_tx} verifies")
        } else {
            verification
                .failures()
                .map(|check| format!("{:?}", check.name))
                .collect::<Vec<_>>()
                .join(", ")
        },
    ) {
        return Ok(None);
    }
    let Some(vote) = verification.record else {
        report.check(
            ResolutionCheckNameV1::VotePassed,
            false,
            "the vote transaction holds no final record",
        );
        return Ok(None);
    };
    let passed = vote.evaluation.accepted();
    report.check(
        ResolutionCheckNameV1::VotePassed,
        passed,
        format!(
            "{} ({})",
            if passed { "accepted" } else { "rejected" },
            vote.evaluation.reason
        ),
    );
    Ok(Some(vote))
}

/// Verifies the execution recorded by transaction `tx_id`, from the chain alone.
///
/// Runs the resolution's own checks first, then proves that the amendment on the chain
/// is the one the resolution authorised, replaced what the shareholders approved, and
/// actually took effect.
///
/// # Errors
///
/// As [`verify_resolution`].
pub fn verify_execution(
    store: &LocalChainStore,
    tx_id: &TxId,
) -> Result<ExecutionVerificationV1, ResolutionError> {
    let located = store
        .get_transaction(tx_id)?
        .ok_or(ResolutionError::NoSuchTransaction { tx_id: *tx_id })?;
    let mut report = ExecutionVerificationV1 {
        tx_id: *tx_id,
        height: located.height,
        record: None,
        checks: Vec::new(),
        resolution: None,
    };
    let Some(execution) = decode_execution(&located.transaction, &mut report) else {
        return Ok(report);
    };
    report.record = Some(execution.clone());

    // 2. The resolution it rests on.
    let resolution_tx = execution.resolution_id.tx_id();
    let Some(resolution_located) = store.get_transaction(&resolution_tx)? else {
        report.check(
            ExecutionCheckNameV1::ResolutionVerifies,
            false,
            format!("no transaction {resolution_tx} is on the chain"),
        );
        return Ok(report);
    };
    let mut resolution_report = ResolutionVerificationV1 {
        tx_id: resolution_tx,
        height: resolution_located.height,
        record: None,
        checks: Vec::new(),
    };
    let decoded = decode_resolution(&resolution_located.transaction, &mut resolution_report);
    let approval = match &decoded {
        Some(record) => {
            resolution_report.record = Some(record.clone());
            verify_authority(
                store,
                record,
                resolution_located.height,
                &mut resolution_report,
            )?
        }
        None => None,
    };
    let resolution_valid = resolution_report.is_valid();
    report.resolution = Some(resolution_report);
    if !report.check(
        ExecutionCheckNameV1::ResolutionVerifies,
        resolution_valid,
        if resolution_valid {
            format!("resolution {resolution_tx} verifies")
        } else {
            format!("resolution {resolution_tx} does not verify")
        },
    ) {
        return Ok(report);
    }
    let resolution = decoded.expect("valid implies decoded");
    let approval = approval.expect("valid implies an approval");

    // 3. It authorises this.
    let target = resolution.kind.target();
    if !report.check(
        ExecutionCheckNameV1::ResolutionAuthorisesThis,
        target == Some(execution.target),
        match target {
            None => "the resolution is declarative; it authorises no amendment".to_owned(),
            Some(target) if target == execution.target => format!("a {target} amendment"),
            Some(target) => format!(
                "the resolution authorises a {target} amendment, the execution claims {}",
                execution.target
            ),
        },
    ) {
        return Ok(report);
    }
    let body = resolution.kind.body().expect("amendment implies a body");

    // 4. The amendment record.
    let Some(amendment_located) = store.get_transaction(&execution.amendment_tx)? else {
        report.check(
            ExecutionCheckNameV1::AmendmentExists,
            false,
            format!("no transaction {} is on the chain", execution.amendment_tx),
        );
        return Ok(report);
    };
    let amendment_text = core::str::from_utf8(&amendment_located.transaction.payload).ok();
    let amendment = amendment_text.and_then(|text| read_record(text).ok());
    let right_kind = amendment
        .as_ref()
        .is_some_and(|record| record.kind() == execution.target.record_kind());
    let right_company = amendment
        .as_ref()
        .is_some_and(|record| record.company == resolution.company);
    if !report.check(
        ExecutionCheckNameV1::AmendmentExists,
        right_kind && right_company,
        match &amendment {
            None => format!(
                "transaction {} does not hold an irena record",
                execution.amendment_tx
            ),
            Some(record) if !right_kind => format!(
                "transaction {} is a {} record, not {}",
                execution.amendment_tx,
                record.kind(),
                execution.target.record_kind()
            ),
            Some(record) if !right_company => format!(
                "the amendment is for company {}, the resolution for {}",
                record.company, resolution.company
            ),
            Some(record) => format!("a {} record for {}", record.kind(), record.company),
        },
    ) {
        return Ok(report);
    }
    let amendment = amendment.expect("checked");

    // 5. It is the approved body.
    let published = match &amendment.body {
        RecordBodyV1::ShareStructure(_) | RecordBodyV1::DecisionChannels(_) => {
            published_body(amendment_text.expect("read"), body)
        }
        _ => false,
    };
    let digest_matches = body_matches(body, execution.body_digest)
        && execution.body_digest == approval.proposal_digest;
    report.check(
        ExecutionCheckNameV1::AmendmentMatchesResolution,
        published && digest_matches,
        if published && digest_matches {
            format!(
                "the {} the channel approved, {}",
                execution.target, execution.body_digest
            )
        } else if !published {
            "the amendment's body is not the body the resolution carries".to_owned()
        } else {
            format!(
                "the execution claims digest {}, the resolution's body digests to {}, the vote approved {}",
                execution.body_digest,
                crate::resolution::proposal_digest(body),
                approval.proposal_digest
            )
        },
    );

    // 6. It replaced what was approved.
    let approved = approval.base_of(execution.target);
    let replaced = amendment.supersedes;
    report.check(
        ExecutionCheckNameV1::AmendmentReplacedApprovedBase,
        replaced == Some(approved) && execution.replaced_tx == approved,
        match replaced {
            Some(replaced) if replaced == approved && execution.replaced_tx == approved => {
                format!("replaced {approved}, the record the voters saw")
            }
            Some(replaced) => format!(
                "the voters approved replacing {approved}; the amendment replaced {replaced}, the execution says {}",
                execution.replaced_tx
            ),
            None => format!(
                "the amendment supersedes nothing; the voters approved replacing {approved}"
            ),
        },
    );

    // 7. One person rewriting who decides: only ever downwards.
    match (&execution.target, &approval.actor, &amendment.body) {
        (AmendmentTargetV1::DecisionChannels, Some(actor), RecordBodyV1::DecisionChannels(new)) => {
            // The company just before the amendment: the set it replaced, and the
            // register the sources resolved against.
            let before = amendment_located
                .height
                .value()
                .checked_sub(1)
                .map(BlockHeight)
                .ok_or_else(|| ResolutionError::Chain {
                    detail: "an amendment cannot sit in the genesis block".to_owned(),
                })
                .and_then(|height| company_at(store, height));
            let verdict = before.and_then(|state| {
                let signer =
                    VoterIdV1::new(actor.clone()).map_err(|error| ResolutionError::Chain {
                        detail: format!("the decision's actor is not a voter id: {error}"),
                    })?;
                self_demotion(&signer, &state.channels.value, new, &state.shares.value)
            });
            match verdict {
                Ok(verdict) => {
                    report.check(
                        ExecutionCheckNameV1::SelfDemotionHolds,
                        verdict.holds,
                        format!(
                            "channel {} on {actor}'s own signature: {}",
                            approval.channel, verdict.detail
                        ),
                    );
                }
                Err(error) => {
                    report.check(
                        ExecutionCheckNameV1::SelfDemotionHolds,
                        false,
                        error.to_string(),
                    );
                }
            }
        }
        (AmendmentTargetV1::DecisionChannels, None, _) => {
            report.check(
                ExecutionCheckNameV1::SelfDemotionHolds,
                true,
                format!(
                    "not applicable: channel {} decided collectively",
                    approval.channel
                ),
            );
        }
        _ => {
            report.check(
                ExecutionCheckNameV1::SelfDemotionHolds,
                true,
                format!("not applicable: a {} amendment", execution.target),
            );
        }
    }

    // 8. It took effect.
    let applied = match company_at(store, located.height) {
        Ok(state) => state
            .history_of(execution.target.record_kind())
            .iter()
            .any(|found| found.tx_id == execution.amendment_tx),
        Err(_) => false,
    };
    report.check(
        ExecutionCheckNameV1::AmendmentApplied,
        applied,
        if applied {
            format!(
                "the amendment is in the company's {} history at height {}",
                execution.target, located.height
            )
        } else {
            "the amendment is not part of the reconstructed company".to_owned()
        },
    );

    // 9. Heights.
    let ordered = resolution_located.height < amendment_located.height
        && amendment_located.height <= located.height;
    report.check(
        ExecutionCheckNameV1::HeightsOrdered,
        ordered,
        format!(
            "resolution at {}, amendment at {}, execution at {}",
            resolution_located.height, amendment_located.height, located.height
        ),
    );

    // 10. Once only.
    let first = find_execution(store, resolution_tx)?;
    let once = first.is_some_and(|(found, _)| found == *tx_id);
    report.check(
        ExecutionCheckNameV1::ExecutedOnce,
        once,
        match first {
            Some((found, _)) if found == *tx_id => {
                "the only execution of this resolution".to_owned()
            }
            Some((found, height)) => format!(
                "resolution {resolution_tx} was already executed at height {height} by {found}"
            ),
            None => "unreachable: this execution is on the chain".to_owned(),
        },
    );

    Ok(report)
}

/// Whether the amendment record published exactly the body the resolution carries.
///
/// `irena_core::compose_record` embeds a body verbatim, so the published record's text
/// contains those exact bytes. Comparing the text is comparing the bytes the chain
/// committed to.
fn published_body(record_text: &str, body: &str) -> bool {
    record_text.contains(body)
}

fn decode_resolution(
    transaction: &Transaction,
    report: &mut ResolutionVerificationV1,
) -> Option<ResolutionRecordV1> {
    if transaction.namespace.as_str() != RESOLUTION_NAMESPACE {
        report.check(
            ResolutionCheckNameV1::Decodes,
            false,
            format!(
                "transaction is in namespace {}, not {RESOLUTION_NAMESPACE}",
                transaction.namespace
            ),
        );
        return None;
    }
    let text = match core::str::from_utf8(&transaction.payload) {
        Ok(text) => text,
        Err(error) => {
            report.check(
                ResolutionCheckNameV1::Decodes,
                false,
                format!("payload is not UTF-8: {error}"),
            );
            return None;
        }
    };
    match read_resolution_record(text) {
        Ok(record) => {
            report.check(
                ResolutionCheckNameV1::Decodes,
                true,
                format!("{} resolution: {:?}", record.kind.as_str(), record.title),
            );
            Some(record)
        }
        Err(error) => {
            report.check(ResolutionCheckNameV1::Decodes, false, error.to_string());
            None
        }
    }
}

fn decode_execution(
    transaction: &Transaction,
    report: &mut ExecutionVerificationV1,
) -> Option<ResolutionExecutionV1> {
    if transaction.namespace.as_str() != EXECUTION_NAMESPACE {
        report.check(
            ExecutionCheckNameV1::Decodes,
            false,
            format!(
                "transaction is in namespace {}, not {EXECUTION_NAMESPACE}",
                transaction.namespace
            ),
        );
        return None;
    }
    let text = match core::str::from_utf8(&transaction.payload) {
        Ok(text) => text,
        Err(error) => {
            report.check(
                ExecutionCheckNameV1::Decodes,
                false,
                format!("payload is not UTF-8: {error}"),
            );
            return None;
        }
    };
    match read_execution_record(text) {
        Ok(record) => {
            report.check(
                ExecutionCheckNameV1::Decodes,
                true,
                format!("execution of resolution {}", record.execution.resolution_id),
            );
            Some(record.execution)
        }
        Err(error) => {
            report.check(ExecutionCheckNameV1::Decodes, false, error.to_string());
            None
        }
    }
}
