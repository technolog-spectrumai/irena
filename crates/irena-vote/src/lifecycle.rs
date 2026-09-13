//! The vote as a process.

use crate::ballot::SignedBallotV1;
use crate::derive::derive_electorate;
use crate::error::VoteError;
use crate::record::{EvaluationSummaryV1, FinalVoteRecordV1, VOTE_NAMESPACE, VOTE_SCHEMA_VERSION};
use crate::snapshot::{ElectorateEntryV1, VoteIdV1, VoteSnapshotV1};
use bornite_core::{BallotSetV1, BallotV1, VoterIdV1};
use bornite_eval::VoteEvaluationV1;
use borsh::{BorshDeserialize, BorshSerialize};
use irena_ledger::reconstruct;
use prunella_canonical::Canonical;
use prunella_core::{BlockHeight, Hash, Namespace, SchemaVersion, TransactionDraft, TxId};
use prunella_crypto::SigningKey;
use prunella_store::LocalChainStore;
use std::collections::BTreeMap;

/// Where a vote is in its life.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoteStatusV1 {
    /// Defined, nothing frozen yet.
    Draft,
    /// The company is resolved and the electorate derived; not yet accepting ballots.
    Frozen,
    /// Accepting ballots.
    Open,
    /// No longer accepting ballots; not yet counted.
    Closed,
    /// Counted; not yet on the chain.
    Evaluated,
    /// On the chain.
    Finalized,
}

impl core::fmt::Display for VoteStatusV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Draft => "draft",
            Self::Frozen => "frozen",
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Evaluated => "evaluated",
            Self::Finalized => "finalized",
        })
    }
}

/// Where a finalised vote landed.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct FinalizedV1 {
    /// The transaction carrying the record.
    pub tx_id: TxId,
    /// The block it was committed in.
    pub height: BlockHeight,
    /// The record as written.
    pub record: FinalVoteRecordV1,
}

/// A vote from draft to final record.
///
/// A runtime state machine: every operation checks the status first and refuses with
/// [`VoteError::InvalidTransition`] naming both, so an invalid transition is something
/// a caller can report rather than something the type system hides. The whole state
/// is canonical Borsh, so a vote can be written to a file between steps and read back
/// on another day or another machine to exactly the same vote.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct VoteV1 {
    status: VoteStatusV1,
    company: String,
    subject: String,
    proposal_digest: Hash,
    snapshot: Option<VoteSnapshotV1>,
    ballots: BTreeMap<String, SignedBallotV1>,
    evaluation: Option<EvaluationSummaryV1>,
    finalized: Option<(TxId, BlockHeight)>,
}

impl Canonical for VoteV1 {}

impl VoteV1 {
    /// Starts a vote: what about, and the digest of the proposal.
    ///
    /// The company is the chain's — one company per chain — and is filled in at
    /// freeze time. The proposal is identified by its digest and never interpreted.
    #[must_use]
    pub fn draft(subject: impl Into<String>, proposal_digest: Hash) -> Self {
        Self {
            status: VoteStatusV1::Draft,
            company: String::new(),
            subject: subject.into(),
            proposal_digest,
            snapshot: None,
            ballots: BTreeMap::new(),
            evaluation: None,
            finalized: None,
        }
    }

    /// Where the vote is.
    #[must_use]
    pub const fn status(&self) -> VoteStatusV1 {
        self.status
    }

    /// The company label, once frozen; empty before.
    #[must_use]
    pub fn company(&self) -> &str {
        &self.company
    }

    /// What is being voted on.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// The proposal digest.
    #[must_use]
    pub const fn proposal_digest(&self) -> Hash {
        self.proposal_digest
    }

    /// The snapshot, once frozen.
    #[must_use]
    pub const fn snapshot(&self) -> Option<&VoteSnapshotV1> {
        self.snapshot.as_ref()
    }

    /// The vote id, once frozen.
    #[must_use]
    pub fn id(&self) -> Option<VoteIdV1> {
        self.snapshot.as_ref().map(VoteSnapshotV1::id)
    }

    /// The accepted ballots, in voter id order.
    pub fn ballots(&self) -> impl Iterator<Item = &SignedBallotV1> {
        self.ballots.values()
    }

    /// The result, once evaluated.
    #[must_use]
    pub const fn evaluation(&self) -> Option<&EvaluationSummaryV1> {
        self.evaluation.as_ref()
    }

    /// Where the record landed, once finalised.
    #[must_use]
    pub const fn finalized(&self) -> Option<(TxId, BlockHeight)> {
        self.finalized
    }

    fn expect_status(&self, required: VoteStatusV1, to: &'static str) -> Result<(), VoteError> {
        if self.status == required {
            Ok(())
        } else {
            Err(VoteError::InvalidTransition {
                from: self.status,
                to,
            })
        }
    }

    /// Freezes the vote against the company as it is at `at`.
    ///
    /// Resolves the founding record, the share register and the voting rules in force
    /// at exactly that height, derives the electorate from the register, and records
    /// everything in the snapshot. From here on nothing appended to the chain can
    /// change what this vote is decided against.
    ///
    /// # Errors
    ///
    /// [`VoteError::InvalidTransition`] unless the vote is a draft; the ledger's errors
    /// if the company is not complete at `at`; [`VoteError::Derivation`] if the
    /// register cannot become an electorate.
    pub fn freeze(
        &mut self,
        store: &LocalChainStore,
        at: BlockHeight,
    ) -> Result<&VoteSnapshotV1, VoteError> {
        self.expect_status(VoteStatusV1::Draft, "freeze")?;
        let state = reconstruct(store, at)?;
        self.company = state.company.as_str().to_owned();
        let derived = derive_electorate(&state.shares.value)?;
        let electorate = derived
            .holders
            .iter()
            .map(|holder| ElectorateEntryV1 {
                id: holder.id.as_str().to_owned(),
                weight: holder.weight.value(),
                excluded: false,
                key: holder.key,
            })
            .collect();
        self.snapshot = Some(VoteSnapshotV1 {
            company: self.company.clone(),
            subject: self.subject.clone(),
            proposal_digest: self.proposal_digest,
            height: at,
            genesis_tx_id: state.genesis_tx_id,
            shares_tx_id: state.shares.tx_id,
            rules_tx_id: state.rules.tx_id,
            electorate,
        });
        self.status = VoteStatusV1::Frozen;
        Ok(self.snapshot.as_ref().expect("just set"))
    }

    /// Opens the vote for ballots.
    ///
    /// # Errors
    ///
    /// [`VoteError::InvalidTransition`] unless the vote is frozen.
    pub fn open(&mut self) -> Result<(), VoteError> {
        self.expect_status(VoteStatusV1::Frozen, "open")?;
        self.status = VoteStatusV1::Open;
        Ok(())
    }

    /// Accepts a ballot.
    ///
    /// Only while open. The ballot is checked in a fixed order — right vote, voter in
    /// the frozen electorate, not excluded, has a registered key, signature verifies,
    /// not already voted — and the first failure is reported. Signature verification
    /// is Prunella's Ed25519 verifier; Bornite never sees a signature.
    ///
    /// # Errors
    ///
    /// [`VoteError::InvalidTransition`] unless open; [`VoteError::Ballot`] naming why
    /// the ballot was refused.
    pub fn cast(&mut self, ballot: SignedBallotV1) -> Result<(), VoteError> {
        self.expect_status(VoteStatusV1::Open, "cast a ballot in")?;
        let snapshot = self.snapshot.as_ref().expect("open implies frozen");
        let voter = ballot.check(snapshot).map_err(VoteError::Ballot)?;
        if self.ballots.contains_key(voter.as_str()) {
            return Err(VoteError::Ballot(
                crate::error::BallotRejectionV1::AlreadyVoted {
                    voter: voter.to_string(),
                },
            ));
        }
        self.ballots.insert(voter.as_str().to_owned(), ballot);
        Ok(())
    }

    /// Closes the vote to further ballots.
    ///
    /// # Errors
    ///
    /// [`VoteError::InvalidTransition`] unless open.
    pub fn close(&mut self) -> Result<(), VoteError> {
        self.expect_status(VoteStatusV1::Open, "close")?;
        self.status = VoteStatusV1::Closed;
        Ok(())
    }

    /// Counts the vote.
    ///
    /// The rules are the record the snapshot pinned, re-read from the chain at the
    /// snapshot height and checked to be that exact transaction; the electorate and
    /// ballots are the frozen ones. Bornite's full result is returned for reporting;
    /// its numeric summary is what the vote keeps and what the final record carries.
    ///
    /// # Errors
    ///
    /// [`VoteError::InvalidTransition`] unless closed; [`VoteError::RulesMoved`] if the
    /// chain no longer resolves the pinned rules at that height (a broken chain);
    /// Bornite's own error if it refuses the inputs.
    pub fn evaluate(&mut self, store: &LocalChainStore) -> Result<VoteEvaluationV1, VoteError> {
        self.expect_status(VoteStatusV1::Closed, "evaluate")?;
        let evaluation = self.evaluate_now(store)?;
        self.evaluation = Some(EvaluationSummaryV1::of(&evaluation));
        self.status = VoteStatusV1::Evaluated;
        Ok(evaluation)
    }

    /// Runs Bornite over the frozen inputs, whatever the status.
    fn evaluate_now(&self, store: &LocalChainStore) -> Result<VoteEvaluationV1, VoteError> {
        let snapshot = self.snapshot.as_ref().expect("past draft implies frozen");
        let state = reconstruct(store, snapshot.height)?;
        let rules = state.rules;
        if rules.tx_id != snapshot.rules_tx_id || state.company.as_str() != snapshot.company {
            return Err(VoteError::RulesMoved {
                height: snapshot.height,
                expected: snapshot.rules_tx_id,
                found: rules.tx_id,
            });
        }
        let electorate = snapshot.electorate().map_err(VoteError::Derivation)?;
        let ballots = bornite_ballots(self.ballots.values())?;
        Ok(bornite_eval::evaluate(&rules.value, &electorate, &ballots)?)
    }

    /// The record this vote would leave on the chain.
    ///
    /// # Errors
    ///
    /// [`VoteError::InvalidTransition`] unless evaluated or finalised.
    pub fn final_record(&self) -> Result<FinalVoteRecordV1, VoteError> {
        if !matches!(
            self.status,
            VoteStatusV1::Evaluated | VoteStatusV1::Finalized
        ) {
            return Err(VoteError::InvalidTransition {
                from: self.status,
                to: "build the final record of",
            });
        }
        Ok(FinalVoteRecordV1::assemble(
            self.snapshot.clone().expect("evaluated implies frozen"),
            self.ballots.values().cloned().collect(),
            self.evaluation.clone().expect("evaluated implies a result"),
        ))
    }

    /// Writes the final record to the chain in its own block.
    ///
    /// The evaluation is rerun first and must match what was stored: a vote whose
    /// result cannot be reproduced at the moment of finalisation is not finalised.
    ///
    /// # Errors
    ///
    /// [`VoteError::InvalidTransition`] unless evaluated; [`VoteError::ResultMismatch`]
    /// if the rerun disagrees; the chain's errors.
    pub fn finalize(
        &mut self,
        store: &LocalChainStore,
        key: &SigningKey,
        timestamp_millis: u64,
    ) -> Result<FinalizedV1, VoteError> {
        self.expect_status(VoteStatusV1::Evaluated, "finalize")?;
        let rerun = EvaluationSummaryV1::of(&self.evaluate_now(store)?);
        if Some(&rerun) != self.evaluation.as_ref() {
            return Err(VoteError::ResultMismatch);
        }
        let record = self.final_record()?;

        let head = store.head()?;
        let parent = store
            .get_block(head.height)?
            .ok_or_else(|| VoteError::Chain {
                detail: format!("the chain head is {head} but no block is stored there"),
            })?;
        let height = head.height.next()?;
        let transaction = key.sign_transaction(TransactionDraft {
            namespace: Namespace::new(VOTE_NAMESPACE)?,
            schema_version: SchemaVersion(VOTE_SCHEMA_VERSION),
            payload: record.canonical_bytes(),
            signer: key.public_key(),
            nonce: height.value(),
        });
        let tx_id = transaction.id;
        let block = parent
            .header
            .child_draft(vec![transaction], timestamp_millis)?
            .build()?;
        store.append_block(block)?;

        self.finalized = Some((tx_id, height));
        self.status = VoteStatusV1::Finalized;
        Ok(FinalizedV1 {
            tx_id,
            height,
            record,
        })
    }
}

/// Bornite's view of a set of accepted ballots.
pub(crate) fn bornite_ballots<'a>(
    ballots: impl Iterator<Item = &'a SignedBallotV1>,
) -> Result<BallotSetV1, VoteError> {
    let ballots = ballots
        .map(|ballot| {
            Ok(BallotV1 {
                voter: VoterIdV1::new(ballot.body.voter.clone()).map_err(VoteError::Derivation)?,
                choice: ballot.body.choice.to_bornite(),
            })
        })
        .collect::<Result<Vec<_>, VoteError>>()?;
    BallotSetV1::new(ballots).map_err(VoteError::Derivation)
}
