//! Independent verification of a final record from the chain alone.

use crate::ballot::ballot_commitment;
use crate::derive::derive_electorate;
use crate::error::VoteError;
use crate::lifecycle::bornite_ballots;
use crate::record::{EvaluationSummaryV1, FinalVoteRecordV1, RECORD_VERSION, VOTE_NAMESPACE};
use crate::snapshot::ElectorateEntryV1;
use irena_ledger::company_at;
use prunella_canonical::Canonical;
use prunella_core::{BlockHeight, TxId};
use prunella_store::LocalChainStore;

/// The checks a verifier runs, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CheckNameV1 {
    /// The transaction is in the vote namespace and its payload decodes as a V1 record
    /// that re-encodes to exactly the stored bytes.
    Decodes,
    /// The stored vote id is the digest of the stored snapshot.
    VoteIdDerives,
    /// The snapshot height precedes the record's own height.
    SnapshotPrecedesRecord,
    /// The founding record, register and rules in force at the snapshot height are
    /// exactly the transactions the snapshot pinned.
    RecordsResolve,
    /// The electorate re-derived from the pinned register equals the frozen one.
    ElectorateDerives,
    /// Every ballot is for this vote, from a frozen voter with a key, and its
    /// signature verifies against that key.
    BallotsVerify,
    /// The ballots are in strict voter order with no duplicates.
    BallotsOrdered,
    /// The commitment is the Merkle root over the ballots as stored.
    CommitmentDerives,
    /// Bornite, rerun on the pinned rules, the frozen electorate and the stored
    /// ballots, produces the stored result.
    ResultReproduces,
}

/// One check and how it came out.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CheckV1 {
    /// Which check.
    pub name: CheckNameV1,
    /// Whether it held.
    pub passed: bool,
    /// What was found, for a reader.
    pub detail: String,
}

/// The outcome of verifying one record.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct VerificationV1 {
    /// The transaction verified.
    pub tx_id: TxId,
    /// Its height.
    pub height: BlockHeight,
    /// The decoded record, if it decoded.
    pub record: Option<FinalVoteRecordV1>,
    /// Every check run, in order. A check that could not run because an earlier one
    /// failed is absent, not reported as passed.
    pub checks: Vec<CheckV1>,
}

impl VerificationV1 {
    /// Whether every check passed.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.checks.is_empty() && self.checks.iter().all(|check| check.passed)
    }

    /// The checks that failed.
    pub fn failures(&self) -> impl Iterator<Item = &CheckV1> {
        self.checks.iter().filter(|check| !check.passed)
    }

    fn check(&mut self, name: CheckNameV1, passed: bool, detail: impl Into<String>) -> bool {
        self.checks.push(CheckV1 {
            name,
            passed,
            detail: detail.into(),
        });
        passed
    }
}

/// Verifies the final record carried by transaction `tx_id`, from the chain alone.
///
/// Nothing the record says is trusted: the vote id, the snapshot's records, the
/// electorate, every signature, the commitment and the result are each re-derived
/// from the chain and compared. All checks that can run do, so a report names
/// everything wrong at once.
///
/// # Errors
///
/// [`VoteError::NoSuchTransaction`] if the chain has no such transaction, or the
/// chain's own errors. A record that fails verification is an `Ok` report with
/// failed checks, not an error.
pub fn verify(store: &LocalChainStore, tx_id: &TxId) -> Result<VerificationV1, VoteError> {
    let located = store
        .get_transaction(tx_id)?
        .ok_or(VoteError::NoSuchTransaction { tx_id: *tx_id })?;
    let transaction = &located.transaction;
    let mut report = VerificationV1 {
        tx_id: *tx_id,
        height: located.height,
        record: None,
        checks: Vec::new(),
    };

    // 1. Decode, and re-encode byte for byte.
    if transaction.namespace.as_str() != VOTE_NAMESPACE {
        report.check(
            CheckNameV1::Decodes,
            false,
            format!(
                "transaction is in namespace {}, not {VOTE_NAMESPACE}",
                transaction.namespace
            ),
        );
        return Ok(report);
    }
    let record = match FinalVoteRecordV1::from_canonical_bytes(&transaction.payload) {
        Ok(record) => record,
        Err(error) => {
            report.check(
                CheckNameV1::Decodes,
                false,
                format!("payload does not decode: {error}"),
            );
            return Ok(report);
        }
    };
    if record.version != RECORD_VERSION {
        report.check(
            CheckNameV1::Decodes,
            false,
            format!("record version {} is not {RECORD_VERSION}", record.version),
        );
        return Ok(report);
    }
    if record.canonical_bytes() != transaction.payload {
        report.check(
            CheckNameV1::Decodes,
            false,
            "record does not re-encode to the stored bytes",
        );
        return Ok(report);
    }
    report.check(CheckNameV1::Decodes, true, "canonical V1 record");
    report.record = Some(record.clone());

    // 2. Vote id.
    let derived_id = record.snapshot.id();
    report.check(
        CheckNameV1::VoteIdDerives,
        derived_id == record.vote_id,
        format!("stored {} derived {derived_id}", record.vote_id),
    );

    // 3. Snapshot precedes record.
    report.check(
        CheckNameV1::SnapshotPrecedesRecord,
        record.snapshot.height < located.height,
        format!(
            "snapshot at height {}, record at height {}",
            record.snapshot.height, located.height
        ),
    );

    // 4. Records resolve at the snapshot height.
    let company = match record.snapshot.company() {
        Ok(company) => company,
        Err(error) => {
            report.check(
                CheckNameV1::RecordsResolve,
                false,
                format!("company label: {error}"),
            );
            return Ok(report);
        }
    };
    let state = match company_at(store, &company, record.snapshot.height) {
        Ok(state) => state,
        Err(error) => {
            report.check(CheckNameV1::RecordsResolve, false, error.to_string());
            return Ok(report);
        }
    };
    let pinned = [
        (
            "genesis",
            state.genesis.tx_id,
            record.snapshot.genesis_tx_id,
        ),
        ("shares", state.shares.tx_id, record.snapshot.shares_tx_id),
        ("rules", state.rules.tx_id, record.snapshot.rules_tx_id),
    ];
    let moved: Vec<String> = pinned
        .iter()
        .filter(|(_, resolved, stored)| resolved != stored)
        .map(|(label, resolved, stored)| format!("{label}: in force {resolved}, pinned {stored}"))
        .collect();
    if !report.check(
        CheckNameV1::RecordsResolve,
        moved.is_empty(),
        if moved.is_empty() {
            format!(
                "genesis, register and rules at height {} are the pinned records",
                record.snapshot.height
            )
        } else {
            moved.join("; ")
        },
    ) {
        return Ok(report);
    }

    // 5. Electorate derives.
    match derive_electorate(&state.shares.value) {
        Ok(derived) => {
            let expected: Vec<ElectorateEntryV1> = derived
                .holders
                .iter()
                .map(|holder| ElectorateEntryV1 {
                    id: holder.id.as_str().to_owned(),
                    weight: holder.weight.value(),
                    excluded: false,
                    key: holder.key,
                })
                .collect();
            let same = expected == record.snapshot.electorate;
            report.check(
                CheckNameV1::ElectorateDerives,
                same,
                if same {
                    format!(
                        "{} voter(s), total weight {}",
                        expected.len(),
                        derived.total_weight
                    )
                } else {
                    "the frozen electorate is not what the pinned register derives".to_owned()
                },
            );
        }
        Err(error) => {
            report.check(CheckNameV1::ElectorateDerives, false, error.to_string());
        }
    }

    // 6. Ballots verify, each against the frozen snapshot.
    let mut bad = Vec::new();
    for ballot in &record.ballots {
        if let Err(rejection) = ballot.check(&record.snapshot) {
            bad.push(format!("{}: {rejection}", ballot.body.voter));
        }
    }
    report.check(
        CheckNameV1::BallotsVerify,
        bad.is_empty(),
        if bad.is_empty() {
            format!("{} ballot(s) signed by frozen voters", record.ballots.len())
        } else {
            bad.join("; ")
        },
    );

    // 7. Ballots ordered.
    report.check(
        CheckNameV1::BallotsOrdered,
        record.ballots_are_sorted(),
        if record.ballots_are_sorted() {
            "strict voter order, no duplicates"
        } else {
            "ballots are not in strict voter order"
        },
    );

    // 8. Commitment.
    let commitment = ballot_commitment(&record.ballots);
    report.check(
        CheckNameV1::CommitmentDerives,
        commitment == record.ballot_commitment,
        format!("stored {} derived {commitment}", record.ballot_commitment),
    );

    // 9. Result reproduces.
    let rerun = record
        .snapshot
        .electorate()
        .map_err(VoteError::Derivation)
        .and_then(|electorate| {
            let ballots = bornite_ballots(record.ballots.iter())?;
            Ok(bornite_eval::evaluate(
                &state.rules.value,
                &electorate,
                &ballots,
            )?)
        });
    match rerun {
        Ok(evaluation) => {
            let summary = EvaluationSummaryV1::of(&evaluation);
            let same = summary == record.evaluation;
            report.check(
                CheckNameV1::ResultReproduces,
                same,
                if same {
                    format!("{:?}: {}", evaluation.outcome, evaluation.reason)
                } else {
                    format!(
                        "rerun gives {:?} ({}), record says {:?} ({})",
                        evaluation.outcome,
                        evaluation.reason,
                        record.evaluation.outcome,
                        record.evaluation.reason
                    )
                },
            );
        }
        Err(error) => {
            report.check(CheckNameV1::ResultReproduces, false, error.to_string());
        }
    }

    Ok(report)
}
