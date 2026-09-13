//! Independent verification of a final decision record from the chain alone.

use crate::error::DecisionError;
use crate::record::{DECISION_NAMESPACE, FinalDecisionRecordV1, RECORD_VERSION};
use crate::resolve::resolve_channel;
use irena_ledger::reconstruct;
use prunella_canonical::Canonical;
use prunella_core::{BlockHeight, TxId};
use prunella_store::LocalChainStore;

/// The checks a verifier runs, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DecisionCheckNameV1 {
    /// The transaction is in the decision namespace and its payload decodes as a V1
    /// record that re-encodes to exactly the stored bytes.
    Decodes,
    /// The stored decision id is the digest of the stored snapshot.
    DecisionIdDerives,
    /// The snapshot height precedes the record's own height.
    SnapshotPrecedesRecord,
    /// The founding record, register and channel set in force at the snapshot height
    /// are exactly the transactions the snapshot pinned.
    RecordsResolve,
    /// The pinned channel exists at that height and is individual.
    ChannelIsIndividual,
    /// The channel resolves to exactly the frozen actor, with the frozen key.
    ActorResolves,
    /// The signature verifies against the frozen key over the frozen snapshot.
    SignatureVerifies,
}

/// One check and how it came out.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DecisionCheckV1 {
    /// Which check.
    pub name: DecisionCheckNameV1,
    /// Whether it held.
    pub passed: bool,
    /// What was found, for a reader.
    pub detail: String,
}

/// The outcome of verifying one record.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DecisionVerificationV1 {
    /// The transaction verified.
    pub tx_id: TxId,
    /// Its height.
    pub height: BlockHeight,
    /// The decoded record, if it decoded.
    pub record: Option<FinalDecisionRecordV1>,
    /// Every check run, in order. A check that could not run because an earlier one
    /// failed is absent, not reported as passed.
    pub checks: Vec<DecisionCheckV1>,
}

impl DecisionVerificationV1 {
    /// Whether every check passed.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.checks.is_empty() && self.checks.iter().all(|check| check.passed)
    }

    /// The checks that failed.
    pub fn failures(&self) -> impl Iterator<Item = &DecisionCheckV1> {
        self.checks.iter().filter(|check| !check.passed)
    }

    fn check(
        &mut self,
        name: DecisionCheckNameV1,
        passed: bool,
        detail: impl Into<String>,
    ) -> bool {
        self.checks.push(DecisionCheckV1 {
            name,
            passed,
            detail: detail.into(),
        });
        passed
    }
}

/// Verifies the final decision record carried by transaction `tx_id`, from the chain
/// alone.
///
/// Nothing the record says is trusted: the id, the pinned records, the channel's mode,
/// its actor and key, and the signature are each re-established from the chain and
/// compared.
///
/// # Errors
///
/// [`DecisionError::NoSuchTransaction`] if the chain has no such transaction, or the
/// chain's own errors. A record that fails verification is an `Ok` report with failed
/// checks, not an error.
pub fn verify_decision(
    store: &LocalChainStore,
    tx_id: &TxId,
) -> Result<DecisionVerificationV1, DecisionError> {
    let located = store
        .get_transaction(tx_id)?
        .ok_or(DecisionError::NoSuchTransaction { tx_id: *tx_id })?;
    let transaction = &located.transaction;
    let mut report = DecisionVerificationV1 {
        tx_id: *tx_id,
        height: located.height,
        record: None,
        checks: Vec::new(),
    };

    // 1. Decode, and re-encode byte for byte.
    if transaction.namespace.as_str() != DECISION_NAMESPACE {
        report.check(
            DecisionCheckNameV1::Decodes,
            false,
            format!(
                "transaction is in namespace {}, not {DECISION_NAMESPACE}",
                transaction.namespace
            ),
        );
        return Ok(report);
    }
    let record = match FinalDecisionRecordV1::from_canonical_bytes(&transaction.payload) {
        Ok(record) => record,
        Err(error) => {
            report.check(
                DecisionCheckNameV1::Decodes,
                false,
                format!("payload does not decode: {error}"),
            );
            return Ok(report);
        }
    };
    if record.version != RECORD_VERSION {
        report.check(
            DecisionCheckNameV1::Decodes,
            false,
            format!("record version {} is not {RECORD_VERSION}", record.version),
        );
        return Ok(report);
    }
    if record.canonical_bytes() != transaction.payload {
        report.check(
            DecisionCheckNameV1::Decodes,
            false,
            "record does not re-encode to the stored bytes",
        );
        return Ok(report);
    }
    report.check(DecisionCheckNameV1::Decodes, true, "canonical V1 record");
    report.record = Some(record.clone());

    // 2. Decision id.
    let derived_id = record.snapshot.id();
    report.check(
        DecisionCheckNameV1::DecisionIdDerives,
        derived_id == record.decision_id,
        format!("stored {} derived {derived_id}", record.decision_id),
    );

    // 3. Snapshot precedes record.
    report.check(
        DecisionCheckNameV1::SnapshotPrecedesRecord,
        record.snapshot.height < located.height,
        format!(
            "snapshot at height {}, record at height {}",
            record.snapshot.height, located.height
        ),
    );

    // 4. Records resolve at the snapshot height.
    let labels = record
        .snapshot
        .company()
        .and_then(|company| record.snapshot.channel().map(|channel| (company, channel)));
    let (company, channel) = match labels {
        Ok(labels) => labels,
        Err(error) => {
            report.check(
                DecisionCheckNameV1::RecordsResolve,
                false,
                format!("label: {error}"),
            );
            return Ok(report);
        }
    };
    let state = match reconstruct(store, record.snapshot.height) {
        Ok(state) => state,
        Err(error) => {
            report.check(
                DecisionCheckNameV1::RecordsResolve,
                false,
                error.to_string(),
            );
            return Ok(report);
        }
    };
    if state.company != company {
        report.check(
            DecisionCheckNameV1::RecordsResolve,
            false,
            format!(
                "the chain holds company {}, the record claims {company}",
                state.company
            ),
        );
        return Ok(report);
    }
    let pinned = [
        (
            "genesis",
            state.genesis_tx_id,
            record.snapshot.genesis_tx_id,
        ),
        ("shares", state.shares.tx_id, record.snapshot.shares_tx_id),
        (
            "channels",
            state.channels.tx_id,
            record.snapshot.channels_tx_id,
        ),
    ];
    let moved: Vec<String> = pinned
        .iter()
        .filter(|(_, resolved, stored)| resolved != stored)
        .map(|(label, resolved, stored)| format!("{label}: in force {resolved}, pinned {stored}"))
        .collect();
    if !report.check(
        DecisionCheckNameV1::RecordsResolve,
        moved.is_empty(),
        if moved.is_empty() {
            format!(
                "genesis, register and channel set at height {} are the pinned records",
                record.snapshot.height
            )
        } else {
            moved.join("; ")
        },
    ) {
        return Ok(report);
    }

    // 5. The channel is individual, and 6. resolves to the frozen actor.
    match resolve_channel(&state, &channel) {
        Ok(resolved) => match resolved.sole_actor() {
            Some(actor) => {
                report.check(
                    DecisionCheckNameV1::ChannelIsIndividual,
                    true,
                    format!("channel {channel} is individual"),
                );
                let same = actor.id.as_str() == record.snapshot.actor
                    && actor.key == Some(record.snapshot.key);
                report.check(
                    DecisionCheckNameV1::ActorResolves,
                    same,
                    if same {
                        format!("{} with the frozen key", actor.id)
                    } else {
                        format!(
                            "channel {channel} resolves to {} (key {}), the record froze {} (key {})",
                            actor.id,
                            actor
                                .key
                                .map_or_else(|| "none".to_owned(), |key| key.to_string()),
                            record.snapshot.actor,
                            record.snapshot.key
                        )
                    },
                );
            }
            None => {
                report.check(
                    DecisionCheckNameV1::ChannelIsIndividual,
                    false,
                    format!("channel {channel} is collective"),
                );
            }
        },
        Err(error) => {
            report.check(
                DecisionCheckNameV1::ChannelIsIndividual,
                false,
                error.to_string(),
            );
        }
    }

    // 7. Signature.
    match record.check_signature() {
        Ok(()) => {
            report.check(
                DecisionCheckNameV1::SignatureVerifies,
                true,
                format!("signed by {}", record.snapshot.actor),
            );
        }
        Err(error) => {
            report.check(
                DecisionCheckNameV1::SignatureVerifies,
                false,
                error.to_string(),
            );
        }
    }

    Ok(report)
}
