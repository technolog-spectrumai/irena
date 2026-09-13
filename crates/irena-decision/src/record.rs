//! The final record: what goes on the chain when an individual decision is finalised.

use crate::decision::{DecisionIdV1, DecisionSnapshotV1};
use crate::error::DecisionError;
use borsh::{BorshDeserialize, BorshSerialize};
use prunella_canonical::Canonical;
use prunella_core::{BlockHeight, Signature, TxId};

/// The Prunella namespace final decision records are published under.
pub const DECISION_NAMESPACE: &str = "irena.decision.v1";

/// The Prunella schema version a final decision record transaction declares.
pub const DECISION_SCHEMA_VERSION: u32 = 1;

/// The record version, first field of the canonical bytes.
pub const RECORD_VERSION: u16 = 1;

/// What a finalised individual decision leaves on the chain.
///
/// Canonical Borsh, published under [`DECISION_NAMESPACE`]. Prunella stores it without
/// knowing what it is; [`crate::verify_decision`] re-establishes every part of it from
/// the chain alone.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct FinalDecisionRecordV1 {
    /// Always [`RECORD_VERSION`].
    pub version: u16,
    /// The decision id, which is the snapshot's digest; stored so tampering with either
    /// is caught as a disagreement between the two.
    pub decision_id: DecisionIdV1,
    /// Everything the decision was taken against.
    pub snapshot: DecisionSnapshotV1,
    /// The actor's Ed25519 signature over [`DecisionSnapshotV1::signing_message`].
    pub signature: Signature,
}

impl Canonical for FinalDecisionRecordV1 {}

impl FinalDecisionRecordV1 {
    /// Assembles a record from its parts, deriving the id.
    #[must_use]
    pub fn assemble(snapshot: DecisionSnapshotV1, signature: Signature) -> Self {
        Self {
            version: RECORD_VERSION,
            decision_id: snapshot.id(),
            snapshot,
            signature,
        }
    }

    /// Verifies the signature against the frozen key.
    ///
    /// # Errors
    ///
    /// [`DecisionError::BadSignature`].
    pub fn check_signature(&self) -> Result<(), DecisionError> {
        prunella_crypto::verify(
            &self.snapshot.key,
            &self.snapshot.signing_message(),
            &self.signature,
        )
        .map_err(|error| DecisionError::BadSignature {
            detail: error.to_string(),
        })
    }
}

/// Where a finalised decision landed.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct FinalizedDecisionV1 {
    /// The transaction carrying the record.
    pub tx_id: TxId,
    /// The block it was committed in.
    pub height: BlockHeight,
    /// The record as written.
    pub record: FinalDecisionRecordV1,
}
