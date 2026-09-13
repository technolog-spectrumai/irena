//! Transactions and the two hash derivations that bind them together.
//!
//! A transaction carries an opaque payload. Prunella never parses, validates or
//! interprets those bytes; it guarantees only that they are reproduced exactly and
//! that they were signed by the declared signer.

use crate::error::CoreError;
use crate::hash::{Hash, TxId};
use crate::keys::{PublicKey, Signature};
use crate::labels::{Namespace, SchemaVersion};
use borsh::{BorshDeserialize, BorshSerialize};
use prunella_canonical::{Canonical, Digest, domain, hash_canonical};

/// A signed, immutable ledger entry.
///
/// Both `id` and `signature` are derived values. A transaction is only valid when
/// `id` equals [`Transaction::compute_id`] and `signature` verifies against `signer`
/// over [`Transaction::signing_message`]. Neither property is assumed by this type;
/// both are checked by `prunella-verify`.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct Transaction {
    /// Identifier derived from every other field, including the signature.
    pub id: TxId,
    /// Application domain label. Opaque to Prunella.
    pub namespace: Namespace,
    /// Payload schema version. Opaque to Prunella.
    pub schema_version: SchemaVersion,
    /// Opaque application payload, reproduced byte for byte.
    pub payload: Vec<u8>,
    /// Public key of the signer.
    pub signer: PublicKey,
    /// Signer-scoped ordinal, carried and hashed but not interpreted.
    ///
    /// Prunella does not enforce nonce ordering or uniqueness: doing so would require
    /// per-signer account state, which is an application concern. The field exists so
    /// that two otherwise identical payloads from the same signer produce distinct
    /// transaction ids.
    pub nonce: u64,
    /// Signature over [`Transaction::signing_message`].
    pub signature: Signature,
}

impl Canonical for Transaction {}

impl Transaction {
    /// Returns the 32-byte message a signer signs.
    ///
    /// Covers every field except `id` and `signature`. Signing a short digest rather
    /// than the full pre-image keeps signing cost independent of payload size.
    #[must_use]
    pub fn signing_message(&self) -> Digest {
        TxSigningBody {
            namespace: self.namespace.clone(),
            schema_version: self.schema_version,
            payload: self.payload.clone(),
            signer: self.signer,
            nonce: self.nonce,
        }
        .signing_message()
    }

    /// Recomputes the transaction id from the transaction's contents.
    ///
    /// The id commits to the signature as well as to the signed body, so replacing a
    /// signature is detected by the id check even before signature verification runs.
    #[must_use]
    pub fn compute_id(&self) -> TxId {
        TxId::from_hash(Hash::from_bytes(hash_canonical(
            domain::TX_ID,
            &TxIdBody {
                namespace: self.namespace.clone(),
                schema_version: self.schema_version,
                payload: self.payload.clone(),
                signer: self.signer,
                nonce: self.nonce,
                signature: self.signature,
            },
        )))
    }

    /// Returns true when `id` matches the recomputed value.
    #[must_use]
    pub fn has_consistent_id(&self) -> bool {
        self.id == self.compute_id()
    }

    /// Computes the transaction root over an ordered list of transactions.
    ///
    /// The root is a linear digest over the count followed by each transaction id in
    /// order: `hash(TX_ROOT, u32_le(count) || id_0 || .. || id_n)`. Absorbing the count
    /// first means a shorter list can never share a pre-image with a longer one.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::TooManyTransactions`] if the list is longer than [`u32::MAX`].
    pub fn compute_root(transactions: &[Self]) -> Result<Hash, CoreError> {
        let count =
            u32::try_from(transactions.len()).map_err(|_| CoreError::TooManyTransactions {
                count: transactions.len(),
                max: u32::MAX,
            })?;
        let mut hasher = prunella_canonical::DomainHasher::new(domain::TX_ROOT);
        hasher.update(&count.to_le_bytes());
        for transaction in transactions {
            hasher.update(transaction.id.as_bytes());
        }
        Ok(Hash::from_bytes(hasher.finalize()))
    }
}

/// An unsigned transaction body.
///
/// Produced by an application, handed to a signer, and turned into a [`Transaction`]
/// by [`TransactionDraft::into_transaction`]. Keeping the draft separate makes it
/// impossible to construct a transaction whose id was computed before its signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionDraft {
    /// Application domain label.
    pub namespace: Namespace,
    /// Payload schema version.
    pub schema_version: SchemaVersion,
    /// Opaque application payload.
    pub payload: Vec<u8>,
    /// Public key of the intended signer.
    pub signer: PublicKey,
    /// Signer-scoped ordinal.
    pub nonce: u64,
}

impl TransactionDraft {
    /// Returns the 32-byte message the signer must sign.
    #[must_use]
    pub fn signing_message(&self) -> Digest {
        TxSigningBody {
            namespace: self.namespace.clone(),
            schema_version: self.schema_version,
            payload: self.payload.clone(),
            signer: self.signer,
            nonce: self.nonce,
        }
        .signing_message()
    }

    /// Attaches a signature and derives the transaction id.
    #[must_use]
    pub fn into_transaction(self, signature: Signature) -> Transaction {
        let mut transaction = Transaction {
            id: TxId::from_hash(Hash::ZERO),
            namespace: self.namespace,
            schema_version: self.schema_version,
            payload: self.payload,
            signer: self.signer,
            nonce: self.nonce,
            signature,
        };
        transaction.id = transaction.compute_id();
        transaction
    }
}

/// Canonical pre-image of a transaction's signing message.
///
/// Field order is part of the format and must never change.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
struct TxSigningBody {
    namespace: Namespace,
    schema_version: SchemaVersion,
    payload: Vec<u8>,
    signer: PublicKey,
    nonce: u64,
}

impl Canonical for TxSigningBody {}

impl TxSigningBody {
    fn signing_message(&self) -> Digest {
        hash_canonical(domain::TX_SIGN, self)
    }
}

/// Canonical pre-image of a transaction id.
///
/// Field order is part of the format and must never change.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
struct TxIdBody {
    namespace: Namespace,
    schema_version: SchemaVersion,
    payload: Vec<u8>,
    signer: PublicKey,
    nonce: u64,
    signature: Signature,
}

impl Canonical for TxIdBody {}

impl serde::Serialize for Transaction {
    /// Renders a transaction for human and machine readers.
    ///
    /// This is a presentation format. It is never a hash pre-image: the payload is
    /// rendered as hex text, and the field set differs from the canonical encoding.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Transaction", 8)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("namespace", self.namespace.as_str())?;
        state.serialize_field("schema_version", &self.schema_version)?;
        state.serialize_field("payload_len", &self.payload.len())?;
        state.serialize_field("payload_hex", &hex::encode(&self.payload))?;
        state.serialize_field("signer", &self.signer)?;
        state.serialize_field("nonce", &self.nonce)?;
        state.serialize_field("signature", &self.signature)?;
        state.end()
    }
}
