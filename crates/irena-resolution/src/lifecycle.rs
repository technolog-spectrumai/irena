//! The resolution as a process.

use crate::error::ResolutionError;
use crate::record::{
    EXECUTION_NAMESPACE, EXECUTION_SCHEMA_VERSION, RESOLUTION_NAMESPACE, RESOLUTION_SCHEMA_VERSION,
    ResolutionExecutionV1, compose_execution, compose_resolution, read_execution_record,
};
use crate::resolution::{
    AmendmentTargetV1, AuthorityV1, ResolutionIdV1, ResolutionKindV1, ResolutionStatusV1,
};
use borsh::{BorshDeserialize, BorshSerialize};
use irena_core::{CompanyIdV1, NotarisationV1};
use irena_ledger::{company_now, publish, reconstruct};
use irena_meeting::verify_meeting;
use irena_vote::{FinalVoteRecordV1, verify};
use prunella_canonical::Canonical;
use prunella_core::{BlockHeight, Namespace, SchemaVersion, TransactionDraft, TxId};
use prunella_crypto::SigningKey;
use prunella_store::LocalChainStore;

/// Where an executed resolution landed.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ExecutedV1 {
    /// The company amendment the resolution authorised.
    pub amendment_tx: TxId,
    /// Where it is.
    pub amendment_height: BlockHeight,
    /// The execution record linking resolution to amendment.
    pub execution_tx: TxId,
    /// Where it is.
    pub execution_height: BlockHeight,
    /// The transaction the amendment superseded.
    pub replaced_tx: TxId,
}

/// A resolution from draft to executed.
///
/// A runtime state machine, like a vote or a meeting: `InvalidTransition { from, to }`
/// names both ends. The whole state is canonical Borsh, so a resolution lives in a
/// file between steps.
///
/// Two things reach the chain: the resolution record at [`ResolutionV1::finalize`],
/// and — for an amendment resolution — the amendment plus its execution record at
/// [`ResolutionV1::execute`]. A declarative resolution ends at `Finalized`.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ResolutionV1 {
    status: ResolutionStatusV1,
    company: String,
    title: String,
    authority: AuthorityV1,
    kind: ResolutionKindV1,
    finalized: Option<(TxId, BlockHeight)>,
    executed: Option<ExecutedV1>,
}

impl Canonical for ResolutionV1 {}

impl ResolutionV1 {
    /// Drafts a resolution: what it is called, what authorises it, and what it does.
    ///
    /// Nothing is checked against the chain yet; [`ResolutionV1::finalize`] does that.
    #[must_use]
    pub fn draft(title: impl Into<String>, authority: AuthorityV1, kind: ResolutionKindV1) -> Self {
        Self {
            status: ResolutionStatusV1::Draft,
            company: String::new(),
            title: title.into(),
            authority,
            kind,
            finalized: None,
            executed: None,
        }
    }

    /// Where the resolution is.
    #[must_use]
    pub const fn status(&self) -> ResolutionStatusV1 {
        self.status
    }

    /// The company label, once finalised; empty before.
    #[must_use]
    pub fn company(&self) -> &str {
        &self.company
    }

    /// The resolution's title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// What authorises it.
    #[must_use]
    pub const fn authority(&self) -> &AuthorityV1 {
        &self.authority
    }

    /// What it does.
    #[must_use]
    pub const fn kind(&self) -> &ResolutionKindV1 {
        &self.kind
    }

    /// The resolution id, once finalised.
    #[must_use]
    pub fn id(&self) -> Option<ResolutionIdV1> {
        self.finalized
            .map(|(tx_id, _)| ResolutionIdV1::from_tx(tx_id))
    }

    /// Where the resolution record landed.
    #[must_use]
    pub const fn finalized(&self) -> Option<(TxId, BlockHeight)> {
        self.finalized
    }

    /// Where the amendment and execution landed.
    #[must_use]
    pub const fn executed(&self) -> Option<ExecutedV1> {
        self.executed
    }

    fn expect_status(
        &self,
        required: ResolutionStatusV1,
        to: &'static str,
    ) -> Result<(), ResolutionError> {
        if self.status == required {
            Ok(())
        } else {
            Err(ResolutionError::InvalidTransition {
                from: self.status,
                to,
            })
        }
    }

    /// Finalises the resolution: checks its authority against the chain, then records
    /// it.
    ///
    /// Nothing the draft says is trusted. Read back from the chain:
    ///
    /// 1. the meeting's final record verifies, every check, including its votes;
    /// 2. the named agenda item exists and is a vote item;
    /// 3. that item was answered by exactly the vote the resolution names;
    /// 4. the vote verifies, every check;
    /// 5. **Bornite accepted it** — a rejected motion authorises nothing;
    /// 6. what the resolution carries is what the shareholders approved: its
    ///    [`ResolutionKindV1::approved_digest`] is the agenda item's proposal digest.
    ///
    /// # Errors
    ///
    /// [`ResolutionError::InvalidTransition`] unless a draft, or the named failure:
    /// [`ResolutionError::MeetingUnverified`], [`ResolutionError::NoSuchVoteItem`],
    /// [`ResolutionError::WrongVote`], [`ResolutionError::VoteUnverified`],
    /// [`ResolutionError::VoteRejected`], [`ResolutionError::ProposalMismatch`].
    pub fn finalize(
        &mut self,
        store: &LocalChainStore,
        key: &SigningKey,
        notarisation: &NotarisationV1,
        timestamp_millis: u64,
    ) -> Result<ResolutionIdV1, ResolutionError> {
        self.expect_status(ResolutionStatusV1::Draft, "finalize")?;
        let vote = self.check_authority(store)?;
        let state = company_now(store)?;
        if vote.snapshot.company != state.company.as_str() {
            return Err(ResolutionError::CompanyMismatch {
                expected: state.company.to_string(),
                found: vote.snapshot.company.clone(),
            });
        }
        let payload = compose_resolution(
            &state.company,
            notarisation,
            &self.title,
            &self.authority,
            &self.kind,
        )?;
        let (tx_id, height) = append(
            store,
            key,
            RESOLUTION_NAMESPACE,
            RESOLUTION_SCHEMA_VERSION,
            payload,
            timestamp_millis,
        )?;
        self.company = state.company.as_str().to_owned();
        self.finalized = Some((tx_id, height));
        self.status = ResolutionStatusV1::Finalized;
        Ok(ResolutionIdV1::from_tx(tx_id))
    }

    /// Checks the authority and returns the vote it rests on.
    fn check_authority(
        &self,
        store: &LocalChainStore,
    ) -> Result<FinalVoteRecordV1, ResolutionError> {
        let meeting = verify_meeting(store, &self.authority.meeting_tx)?;
        if !meeting.is_valid() {
            let failed: Vec<String> = meeting
                .failures()
                .map(|check| format!("{:?}", check.name))
                .collect();
            return Err(ResolutionError::MeetingUnverified {
                meeting_tx: self.authority.meeting_tx,
                detail: if failed.is_empty() {
                    "a referenced vote does not verify".to_owned()
                } else {
                    failed.join(", ")
                },
            });
        }
        let record = meeting.record.ok_or(ResolutionError::MeetingUnverified {
            meeting_tx: self.authority.meeting_tx,
            detail: "the transaction does not hold a final meeting record".to_owned(),
        })?;
        let entry = record
            .items
            .iter()
            .find(|entry| entry.item.number == self.authority.item_number)
            .filter(|entry| entry.item.body.is_vote())
            .ok_or(ResolutionError::NoSuchVoteItem {
                meeting_tx: self.authority.meeting_tx,
                item_number: self.authority.item_number,
            })?;
        let answered = entry.vote_tx_id.ok_or(ResolutionError::NoSuchVoteItem {
            meeting_tx: self.authority.meeting_tx,
            item_number: self.authority.item_number,
        })?;
        if answered != self.authority.vote_tx {
            return Err(ResolutionError::WrongVote {
                meeting_tx: self.authority.meeting_tx,
                item_number: self.authority.item_number,
                expected: answered,
                found: self.authority.vote_tx,
            });
        }

        let verification = verify(store, &self.authority.vote_tx)?;
        if !verification.is_valid() {
            let failed: Vec<String> = verification
                .failures()
                .map(|check| format!("{:?}", check.name))
                .collect();
            return Err(ResolutionError::VoteUnverified {
                vote_tx: self.authority.vote_tx,
                detail: failed.join(", "),
            });
        }
        let vote = verification.record.ok_or(ResolutionError::VoteUnverified {
            vote_tx: self.authority.vote_tx,
            detail: "the transaction does not hold a final vote record".to_owned(),
        })?;
        if !vote.evaluation.accepted() {
            return Err(ResolutionError::VoteRejected {
                vote_tx: self.authority.vote_tx,
                reason: vote.evaluation.reason.clone(),
            });
        }
        let approved = vote.snapshot.proposal_digest;
        let carried = self.kind.approved_digest();
        if carried != approved {
            return Err(ResolutionError::ProposalMismatch {
                expected: approved,
                found: carried,
            });
        }
        Ok(vote)
    }

    /// Executes an amendment resolution: publishes the amendment it authorises, then
    /// the execution record linking the two.
    ///
    /// The amendment is an **ordinary company amendment** (`irena_ledger::publish`),
    /// so the record that changes reconstructed state is the same one it has always
    /// been; the execution record only says which resolution authorised it.
    ///
    /// Two things are refused:
    ///
    /// * **A stale base.** The shareholders approved replacing one exact record. If
    ///   that record no longer provides its part, executing would replace something
    ///   they never saw, so it is refused ([`ResolutionError::StaleBase`]) and the
    ///   resolution must go back to a meeting.
    /// * **A second execution.** The chain is scanned for an execution of this
    ///   resolution ([`ResolutionError::AlreadyExecuted`]); even without that,
    ///   `publish` would refuse the second amendment as stale.
    ///
    /// # Errors
    ///
    /// [`ResolutionError::InvalidTransition`] unless finalised;
    /// [`ResolutionError::NothingToExecute`] for a declarative resolution; the two
    /// refusals above; or the chain's own errors.
    pub fn execute(
        &mut self,
        store: &LocalChainStore,
        key: &SigningKey,
        notarisation: &NotarisationV1,
        timestamp_millis: u64,
    ) -> Result<ExecutedV1, ResolutionError> {
        self.expect_status(ResolutionStatusV1::Finalized, "execute")?;
        let (target, body) = match &self.kind {
            ResolutionKindV1::Declarative { .. } => {
                return Err(ResolutionError::NothingToExecute);
            }
            ResolutionKindV1::Amendment { target, body } => (*target, body.clone()),
        };
        let resolution_id = self.id().expect("finalized");

        // Nobody has executed this resolution already.
        if let Some((execution_tx, height)) = find_execution(store, resolution_id.tx_id())? {
            return Err(ResolutionError::AlreadyExecuted {
                resolution_tx: resolution_id.tx_id(),
                execution_tx,
                height,
            });
        }

        // The company is still the one the shareholders approved against.
        let vote = self.check_authority(store)?;
        let approved = approved_base(&vote, target);
        let state = company_now(store)?;
        let current = state.provider_of(target.record_kind());
        if current != approved {
            return Err(ResolutionError::StaleBase {
                target,
                approved,
                current,
            });
        }

        // The amendment: an ordinary company record, superseding what was approved.
        let amendment = publish(
            store,
            key,
            target.record_kind(),
            &body,
            Some(approved),
            notarisation,
            timestamp_millis,
        )?;

        // The execution record: the link, pinned by transaction id.
        let execution = ResolutionExecutionV1 {
            resolution_id,
            amendment_tx: amendment.tx_id,
            target,
            replaced_tx: approved,
            body_digest: self.kind.approved_digest(),
        };
        let company = CompanyIdV1::new(self.company.clone())?;
        let payload = compose_execution(&company, notarisation, &execution)?;
        let (execution_tx, execution_height) = append(
            store,
            key,
            EXECUTION_NAMESPACE,
            EXECUTION_SCHEMA_VERSION,
            payload,
            timestamp_millis,
        )?;

        let executed = ExecutedV1 {
            amendment_tx: amendment.tx_id,
            amendment_height: amendment.height,
            execution_tx,
            execution_height,
            replaced_tx: approved,
        };
        self.executed = Some(executed);
        self.status = ResolutionStatusV1::Executed;
        Ok(executed)
    }
}

/// The record the voters saw providing the part an amendment replaces.
pub(crate) fn approved_base(vote: &FinalVoteRecordV1, target: AmendmentTargetV1) -> TxId {
    match target {
        AmendmentTargetV1::ShareStructure => vote.snapshot.shares_tx_id,
        AmendmentTargetV1::VotingRules => vote.snapshot.rules_tx_id,
    }
}

/// Finds an execution of `resolution_tx` on the chain, if there is one.
///
/// # Errors
///
/// The chain's own errors. A transaction in the execution namespace that does not read
/// is skipped rather than fatal: it cannot be an execution of this resolution, and
/// [`crate::verify_execution`] is where such a record is judged.
pub(crate) fn find_execution(
    store: &LocalChainStore,
    resolution_tx: TxId,
) -> Result<Option<(TxId, BlockHeight)>, ResolutionError> {
    let head = store.head()?;
    for block in store.iter_blocks(BlockHeight::GENESIS, head.height)? {
        let block = block?;
        for transaction in &block.transactions {
            if transaction.namespace.as_str() != EXECUTION_NAMESPACE {
                continue;
            }
            let Ok(text) = core::str::from_utf8(&transaction.payload) else {
                continue;
            };
            let Ok(record) = read_execution_record(text) else {
                continue;
            };
            if record.execution.resolution_id.tx_id() == resolution_tx {
                return Ok(Some((transaction.id, block.header.height)));
            }
        }
    }
    Ok(None)
}

/// The company at the height an execution record sits, for verification.
pub(crate) fn company_at(
    store: &LocalChainStore,
    at: BlockHeight,
) -> Result<irena_ledger::CompanyStateV1, ResolutionError> {
    Ok(reconstruct(store, at)?)
}

/// Appends one record in its own block.
fn append(
    store: &LocalChainStore,
    key: &SigningKey,
    namespace: &str,
    schema_version: u32,
    payload: String,
    timestamp_millis: u64,
) -> Result<(TxId, BlockHeight), ResolutionError> {
    let head = store.head()?;
    let parent = store
        .get_block(head.height)?
        .ok_or_else(|| ResolutionError::Chain {
            detail: format!("the chain head is {head} but no block is stored there"),
        })?;
    let height = head.height.next()?;
    let transaction = key.sign_transaction(TransactionDraft {
        namespace: Namespace::new(namespace)?,
        schema_version: SchemaVersion(schema_version),
        payload: payload.into_bytes(),
        signer: key.public_key(),
        nonce: height.value(),
    });
    let tx_id = transaction.id;
    let block = parent
        .header
        .child_draft(vec![transaction], timestamp_millis)?
        .build()?;
    store.append_block(block)?;
    Ok((tx_id, height))
}
