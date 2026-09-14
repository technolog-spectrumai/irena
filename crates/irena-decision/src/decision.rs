//! An individual decision as a process: one actor of an individual channel signs.
//!
//! The collective counterpart is a vote (`irena-vote`). Both freeze the company at a
//! height, pin the records they were decided against by transaction id, and end as a
//! canonical record on the chain that a verifier re-establishes from the chain alone.
//! A decision has no ballots and no count: the frozen actor's one signature over the
//! frozen snapshot is the whole decision.

use crate::error::DecisionError;
use crate::record::{
    DECISION_NAMESPACE, DECISION_SCHEMA_VERSION, FinalDecisionRecordV1, FinalizedDecisionV1,
};
use crate::resolve::resolve_channel;
use borsh::{BorshDeserialize, BorshSerialize};
use irena_core::{ChannelIdV1, CompanyIdV1, RecordFamilyV1};
use irena_ledger::{authorised_signer, company_now, reconstruct};
use prunella_canonical::{Canonical, hash_canonical};
use prunella_core::{
    BlockHeight, Hash, Namespace, PublicKey, SchemaVersion, Signature, TransactionDraft, TxId,
};
use prunella_crypto::SigningKey;
use prunella_store::LocalChainStore;

/// Domain tag for a decision id: the digest of the canonical snapshot.
pub const DECISION_ID_TAG: &str = "IRENA/decision/v1/id";

/// Domain tag for the message the actor signs: the canonical snapshot.
pub const DECISION_SIGN_TAG: &str = "IRENA/decision/v1/statement";

/// Identifies a decision: the digest of its frozen snapshot.
#[derive(
    BorshSerialize, BorshDeserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, std::hash::Hash,
)]
pub struct DecisionIdV1(Hash);

impl DecisionIdV1 {
    /// Wraps a digest.
    #[must_use]
    pub const fn from_hash(hash: Hash) -> Self {
        Self(hash)
    }

    /// The digest.
    #[must_use]
    pub const fn hash(self) -> Hash {
        self.0
    }
}

impl core::fmt::Display for DecisionIdV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

impl core::fmt::Debug for DecisionIdV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

impl serde::Serialize for DecisionIdV1 {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&self.0, serializer)
    }
}

/// Everything a decision is taken against, fixed at freeze time.
///
/// Records are pinned by transaction id, so pinning the id pins the exact channel set,
/// register and identities. The actor and key are included so the snapshot can be
/// checked on its own, and re-resolved from the pinned records by a verifier.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DecisionSnapshotV1 {
    /// Which company.
    pub company: String,
    /// What is being decided, as an opaque label.
    pub subject: String,
    /// Digest of the proposal document. Never interpreted.
    pub proposal_digest: Hash,
    /// The height the company was resolved at.
    pub height: BlockHeight,
    /// The founding record in force at that height.
    pub genesis_tx_id: TxId,
    /// The share register in force at that height.
    pub shares_tx_id: TxId,
    /// The channel set in force at that height.
    pub channels_tx_id: TxId,
    /// The identities in force at that height: where the actor's key came from.
    pub identities_tx_id: TxId,
    /// The channel decided through.
    pub channel: String,
    /// The channel's sole actor.
    pub actor: String,
    /// The key the actor's identity held at that height.
    pub key: PublicKey,
}

impl Canonical for DecisionSnapshotV1 {}

impl DecisionSnapshotV1 {
    /// The decision id: the digest of this snapshot's canonical bytes.
    #[must_use]
    pub fn id(&self) -> DecisionIdV1 {
        DecisionIdV1(Hash::from_bytes(hash_canonical(DECISION_ID_TAG, self)))
    }

    /// The message the actor signs: `hash(DECISION_SIGN_TAG, canonical(snapshot))`.
    #[must_use]
    pub fn signing_message(&self) -> [u8; 32] {
        hash_canonical(DECISION_SIGN_TAG, self)
    }

    /// The company label, validated.
    ///
    /// # Errors
    ///
    /// [`irena_core::IrenaError`] if a decoded snapshot carries a label that is not one.
    pub fn company(&self) -> Result<CompanyIdV1, irena_core::IrenaError> {
        CompanyIdV1::new(self.company.clone())
    }

    /// The channel id, validated.
    ///
    /// # Errors
    ///
    /// [`irena_core::IrenaError`] if a decoded snapshot carries a label that is not one.
    pub fn channel(&self) -> Result<ChannelIdV1, irena_core::IrenaError> {
        ChannelIdV1::new(self.channel.clone())
    }
}

/// Where a decision is in its life.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatusV1 {
    /// Defined, nothing frozen yet.
    Draft,
    /// The company is resolved and the actor fixed; not yet signed.
    Frozen,
    /// Signed by the actor; not yet on the chain.
    Signed,
    /// On the chain.
    Finalized,
}

impl core::fmt::Display for DecisionStatusV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Draft => "draft",
            Self::Frozen => "frozen",
            Self::Signed => "signed",
            Self::Finalized => "finalized",
        })
    }
}

/// An individual decision from draft to final record.
///
/// A runtime state machine like a vote: every operation checks the status first and
/// refuses with [`DecisionError::InvalidTransition`] naming both ends. The whole state
/// is canonical Borsh, so a decision lives in a file between steps and the actor can
/// sign on their own machine.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DecisionV1 {
    status: DecisionStatusV1,
    subject: String,
    proposal_digest: Hash,
    snapshot: Option<DecisionSnapshotV1>,
    signature: Option<Signature>,
    finalized: Option<(TxId, BlockHeight)>,
}

impl Canonical for DecisionV1 {}

impl DecisionV1 {
    /// Starts a decision: what about, and the digest of the proposal.
    #[must_use]
    pub fn draft(subject: impl Into<String>, proposal_digest: Hash) -> Self {
        Self {
            status: DecisionStatusV1::Draft,
            subject: subject.into(),
            proposal_digest,
            snapshot: None,
            signature: None,
            finalized: None,
        }
    }

    /// Where the decision is.
    #[must_use]
    pub const fn status(&self) -> DecisionStatusV1 {
        self.status
    }

    /// What is being decided.
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
    pub const fn snapshot(&self) -> Option<&DecisionSnapshotV1> {
        self.snapshot.as_ref()
    }

    /// The decision id, once frozen.
    #[must_use]
    pub fn id(&self) -> Option<DecisionIdV1> {
        self.snapshot.as_ref().map(DecisionSnapshotV1::id)
    }

    /// The signature, once signed.
    #[must_use]
    pub const fn signature(&self) -> Option<&Signature> {
        self.signature.as_ref()
    }

    /// Where the record landed, once finalised.
    #[must_use]
    pub const fn finalized(&self) -> Option<(TxId, BlockHeight)> {
        self.finalized
    }

    fn expect_status(
        &self,
        wanted: DecisionStatusV1,
        to: &'static str,
    ) -> Result<(), DecisionError> {
        if self.status == wanted {
            Ok(())
        } else {
            Err(DecisionError::InvalidTransition {
                from: self.status,
                to,
            })
        }
    }

    /// Freezes the decision against the company as it is at `at`, through `channel`.
    ///
    /// Resolves the channel, requires it to be individual, and records its sole actor
    /// and key. From here on nothing appended to the chain can change who this
    /// decision is for.
    ///
    /// # Errors
    ///
    /// [`DecisionError::InvalidTransition`] unless a draft; the resolution errors;
    /// [`DecisionError::NotIndividual`]; [`DecisionError::NoKey`] if the actor cannot
    /// sign.
    pub fn freeze(
        &mut self,
        store: &LocalChainStore,
        at: BlockHeight,
        channel: &ChannelIdV1,
    ) -> Result<&DecisionSnapshotV1, DecisionError> {
        self.expect_status(DecisionStatusV1::Draft, "freeze")?;
        let state = reconstruct(store, at)?;
        let resolved = resolve_channel(&state, channel)?;
        let actor = resolved
            .sole_actor()
            .ok_or_else(|| DecisionError::NotIndividual {
                channel: channel.clone(),
            })?;
        let key = actor.key.ok_or_else(|| DecisionError::NoKey {
            channel: channel.clone(),
            actor: actor.id.to_string(),
        })?;
        self.snapshot = Some(DecisionSnapshotV1 {
            company: state.company.as_str().to_owned(),
            subject: self.subject.clone(),
            proposal_digest: self.proposal_digest,
            height: at,
            genesis_tx_id: state.genesis_tx_id,
            shares_tx_id: state.shares.tx_id,
            channels_tx_id: state.channels.tx_id,
            identities_tx_id: state.identities.tx_id,
            channel: channel.as_str().to_owned(),
            actor: actor.id.as_str().to_owned(),
            key,
        });
        self.status = DecisionStatusV1::Frozen;
        Ok(self.snapshot.as_ref().expect("just set"))
    }

    /// Signs the frozen snapshot with the actor's key.
    ///
    /// # Errors
    ///
    /// [`DecisionError::InvalidTransition`] unless frozen; [`DecisionError::WrongKey`]
    /// if `key` is not the frozen actor's registered key.
    pub fn sign(&mut self, key: &SigningKey) -> Result<&Signature, DecisionError> {
        self.expect_status(DecisionStatusV1::Frozen, "sign")?;
        let snapshot = self.snapshot.as_ref().expect("frozen");
        if key.public_key() != snapshot.key {
            return Err(DecisionError::WrongKey {
                actor: snapshot.actor.clone(),
            });
        }
        self.signature = Some(key.sign(&snapshot.signing_message()));
        self.status = DecisionStatusV1::Signed;
        Ok(self.signature.as_ref().expect("just set"))
    }

    /// The record this decision would leave on the chain.
    ///
    /// # Errors
    ///
    /// [`DecisionError::InvalidTransition`] unless signed or finalised.
    pub fn final_record(&self) -> Result<FinalDecisionRecordV1, DecisionError> {
        if !matches!(
            self.status,
            DecisionStatusV1::Signed | DecisionStatusV1::Finalized
        ) {
            return Err(DecisionError::InvalidTransition {
                from: self.status,
                to: "build the final record of",
            });
        }
        Ok(FinalDecisionRecordV1::assemble(
            self.snapshot.clone().expect("signed implies frozen"),
            self.signature.expect("signed"),
        ))
    }

    /// Writes the final record to the chain in its own block.
    ///
    /// `key` signs the transaction, and must be the current key of a `governance`
    /// signer under the company at the chain head: who may put a decision on the
    /// chain is the company's own authorisation, not whoever holds a key. The
    /// signature is verified again first and the channel re-resolved at the snapshot
    /// height: a decision that cannot be reproduced at the moment of finalisation is
    /// not finalised.
    ///
    /// # Errors
    ///
    /// [`DecisionError::InvalidTransition`] unless signed; [`DecisionError::BadSignature`];
    /// [`DecisionError::Ledger`] carrying `UnauthorisedSigner` for a key that may not
    /// sign governance records; [`DecisionError::ChannelsMoved`]; the chain's errors.
    pub fn finalize(
        &mut self,
        store: &LocalChainStore,
        key: &SigningKey,
        timestamp_millis: u64,
    ) -> Result<FinalizedDecisionV1, DecisionError> {
        self.expect_status(DecisionStatusV1::Signed, "finalize")?;
        let record = self.final_record()?;
        record.check_signature()?;
        authorised_signer(
            &company_now(store)?,
            RecordFamilyV1::Governance,
            &key.public_key(),
        )?;
        let state = reconstruct(store, record.snapshot.height)?;
        if state.channels.tx_id != record.snapshot.channels_tx_id
            || state.company.as_str() != record.snapshot.company
        {
            return Err(DecisionError::ChannelsMoved {
                height: record.snapshot.height,
                expected: record.snapshot.channels_tx_id,
                found: state.channels.tx_id,
            });
        }

        let head = store.head()?;
        let parent = store
            .get_block(head.height)?
            .ok_or_else(|| DecisionError::Chain {
                detail: format!("the chain head is {head} but no block is stored there"),
            })?;
        let height = head.height.next()?;
        let transaction = key.sign_transaction(TransactionDraft {
            namespace: Namespace::new(DECISION_NAMESPACE)?,
            schema_version: SchemaVersion(DECISION_SCHEMA_VERSION),
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
        self.status = DecisionStatusV1::Finalized;
        Ok(FinalizedDecisionV1 {
            tx_id,
            height,
            record,
        })
    }
}
