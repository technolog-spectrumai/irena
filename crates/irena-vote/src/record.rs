//! The final record: what goes on the chain when a vote is finalised.

use crate::ballot::{SignedBallotV1, ballot_commitment};
use crate::snapshot::{VoteIdV1, VoteSnapshotV1};
use bornite_eval::{ComparisonV1, OutcomeV1, QuorumRequirementV1, ReasonCodeV1, VoteEvaluationV1};
use borsh::{BorshDeserialize, BorshSerialize};
use prunella_canonical::Canonical;
use prunella_core::Hash;

/// The Prunella namespace final records are published under.
pub const VOTE_NAMESPACE: &str = "irena.vote.v1";

/// The Prunella schema version a final record transaction declares.
pub const VOTE_SCHEMA_VERSION: u32 = 1;

/// The record version, first field of the canonical bytes.
pub const RECORD_VERSION: u16 = 1;

/// A Bornite decision, as stored.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredOutcomeV1 {
    /// The motion passed.
    Accepted,
    /// The motion failed.
    Rejected,
}

/// Where the YES weight landed relative to the required share, as stored.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredComparisonV1 {
    /// Strictly above.
    Above,
    /// Exactly: a tie.
    Exactly,
    /// Strictly below.
    Below,
}

/// Every number in a Bornite evaluation, in a canonical form the chain can hold.
///
/// Bornite's own result type carries the rules it ran under and is built for
/// reporting, not for canonical encoding. This is its exact numeric content: outcome,
/// reason, the electorate before and after exclusions, participation, the three-way
/// tally, the quorum as applied and the threshold as applied. The rules are not
/// echoed here because the snapshot pins them by transaction id on the chain. A
/// verifier reruns Bornite and compares summaries; equality means every one of these
/// numbers came out the same.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct EvaluationSummaryV1 {
    /// The decision.
    pub outcome: StoredOutcomeV1,
    /// Why, as Bornite's stable reason code text.
    pub reason: String,
    /// Every voter, including excluded ones.
    pub voter_count: u64,
    /// Voters removed by the exclusion rule.
    pub excluded_count: u64,
    /// Voters left after exclusions.
    pub effective_voter_count: u64,
    /// Weight of every voter under the weight rule.
    pub total_weight: u64,
    /// Weight of the effective voters.
    pub effective_weight: u64,
    /// Ballots cast.
    pub ballot_count: u64,
    /// Weight of the voters who cast a ballot.
    pub participation_weight: u64,
    /// Effective voters who cast nothing.
    pub non_participant_count: u64,
    /// Their weight.
    pub non_participant_weight: u64,
    /// Weight voting YES.
    pub yes_weight: u64,
    /// Weight voting NO.
    pub no_weight: u64,
    /// Weight abstaining.
    pub abstain_weight: u64,
    /// Ballots voting YES.
    pub yes_count: u64,
    /// Ballots voting NO.
    pub no_count: u64,
    /// Ballots abstaining.
    pub abstain_count: u64,
    /// The quorum requirement as applied: the required weight, or none.
    pub quorum_required_weight: Option<u64>,
    /// The participating weight measured against it.
    pub quorum_actual_weight: u64,
    /// Whether the quorum was met.
    pub quorum_met: bool,
    /// The threshold denominator weight, after the abstention rule.
    pub threshold_denominator_weight: u64,
    /// Required share, numerator.
    pub threshold_numerator: u64,
    /// Required share, denominator.
    pub threshold_denominator: u64,
    /// Whether the abstention rule could change the denominator.
    pub abstentions_affected_denominator: bool,
    /// The YES weight measured.
    pub threshold_yes_weight: u64,
    /// Where it landed.
    pub comparison: StoredComparisonV1,
    /// Whether the threshold was met, after the tie rule.
    pub threshold_met: bool,
}

impl EvaluationSummaryV1 {
    /// Projects a Bornite evaluation into its stored form.
    #[must_use]
    pub fn of(evaluation: &VoteEvaluationV1) -> Self {
        let quorum_required_weight = match evaluation.quorum.requirement {
            QuorumRequirementV1::None => None,
            QuorumRequirementV1::Absolute { weight } => Some(weight.value()),
            QuorumRequirementV1::Fraction {
                fraction,
                basis_weight,
                ..
            } => {
                // The smallest weight that meets the share, computed exactly. Bornite
                // decided `met` itself; this is the requirement rendered as a number.
                let basis = u128::from(basis_weight.value());
                let numerator = u128::from(fraction.numerator);
                let denominator = u128::from(fraction.denominator.get());
                let required = (basis * numerator).div_ceil(denominator);
                Some(u64::try_from(required).unwrap_or(u64::MAX))
            }
        };
        Self {
            outcome: match evaluation.outcome {
                OutcomeV1::Accepted => StoredOutcomeV1::Accepted,
                OutcomeV1::Rejected => StoredOutcomeV1::Rejected,
            },
            reason: reason_text(evaluation.reason).to_owned(),
            voter_count: evaluation.electorate.voter_count,
            excluded_count: evaluation.electorate.excluded_count,
            effective_voter_count: evaluation.electorate.effective_voter_count,
            total_weight: evaluation.electorate.total_weight.value(),
            effective_weight: evaluation.electorate.effective_weight.value(),
            ballot_count: evaluation.participation.ballot_count,
            participation_weight: evaluation.participation.weight.value(),
            non_participant_count: evaluation.participation.non_participant_count,
            non_participant_weight: evaluation.participation.non_participant_weight.value(),
            yes_weight: evaluation.tally.yes_weight.value(),
            no_weight: evaluation.tally.no_weight.value(),
            abstain_weight: evaluation.tally.abstain_weight.value(),
            yes_count: evaluation.tally.yes_count,
            no_count: evaluation.tally.no_count,
            abstain_count: evaluation.tally.abstain_count,
            quorum_required_weight,
            quorum_actual_weight: evaluation.quorum.actual_weight.value(),
            quorum_met: evaluation.quorum.met,
            threshold_denominator_weight: evaluation.threshold.denominator_weight.value(),
            threshold_numerator: evaluation.threshold.required_fraction.numerator,
            threshold_denominator: evaluation.threshold.required_fraction.denominator.get(),
            abstentions_affected_denominator: evaluation.threshold.abstentions_affected_denominator,
            threshold_yes_weight: evaluation.threshold.yes_weight.value(),
            comparison: match evaluation.threshold.comparison {
                ComparisonV1::Above => StoredComparisonV1::Above,
                ComparisonV1::Exactly => StoredComparisonV1::Exactly,
                ComparisonV1::Below => StoredComparisonV1::Below,
            },
            threshold_met: evaluation.threshold.met,
        }
    }

    /// Whether the motion passed.
    #[must_use]
    pub const fn accepted(&self) -> bool {
        matches!(self.outcome, StoredOutcomeV1::Accepted)
    }
}

fn reason_text(reason: ReasonCodeV1) -> &'static str {
    reason.as_str()
}

/// What a finalised vote leaves on the chain.
///
/// Canonical Borsh, published under [`VOTE_NAMESPACE`]. Prunella stores it without
/// knowing what it is; [`crate::verify`] re-establishes every part of it from the
/// chain alone.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct FinalVoteRecordV1 {
    /// Always [`RECORD_VERSION`].
    pub version: u16,
    /// The vote id, which is the snapshot's digest; stored so tampering with either
    /// is caught as a disagreement between the two.
    pub vote_id: VoteIdV1,
    /// Everything the vote was decided against.
    pub snapshot: VoteSnapshotV1,
    /// The Merkle root over the accepted ballots' digests, in voter order.
    pub ballot_commitment: Hash,
    /// The accepted ballots, in strict voter id order.
    pub ballots: Vec<SignedBallotV1>,
    /// The result.
    pub evaluation: EvaluationSummaryV1,
}

impl Canonical for FinalVoteRecordV1 {}

impl FinalVoteRecordV1 {
    /// Assembles a record from its parts, deriving the id and the commitment.
    #[must_use]
    pub fn assemble(
        snapshot: VoteSnapshotV1,
        ballots: Vec<SignedBallotV1>,
        evaluation: EvaluationSummaryV1,
    ) -> Self {
        Self {
            version: RECORD_VERSION,
            vote_id: snapshot.id(),
            ballot_commitment: ballot_commitment(&ballots),
            snapshot,
            ballots,
            evaluation,
        }
    }

    /// Whether the ballots are in strict voter id order, which the commitment relies on.
    #[must_use]
    pub fn ballots_are_sorted(&self) -> bool {
        self.ballots
            .windows(2)
            .all(|pair| pair[0].body.voter < pair[1].body.voter)
    }
}
