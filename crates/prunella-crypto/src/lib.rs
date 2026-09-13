//! Ed25519 signing and strict verification for Prunella transactions.
//!
//! This crate is the only place in Prunella that performs a cryptographic signature
//! operation. `prunella-core` holds keys and signatures as opaque byte containers, so
//! the signature scheme can be replaced here without touching the ledger types, the
//! canonical encoding or the storage format.
//!
//! # What is signed
//!
//! Signers sign [`Transaction::signing_message`], the 32-byte digest that covers the
//! namespace, schema version, payload, signer and nonce. Signing a digest rather than
//! the full pre-image keeps signing cost independent of payload size.
//!
//! # Strictness
//!
//! Verification uses [`VerifyingKey::verify_strict`], which rejects small-order public
//! keys and non-canonically encoded signature components. A ledger must not accept two
//! different signature encodings for one signed message, because the transaction id
//! commits to the signature bytes.

use ed25519_dalek::{Signer, VerifyingKey};
use prunella_core::{
    PUBLIC_KEY_LEN, PublicKey, SIGNATURE_LEN, Signature, Transaction, TransactionDraft,
};

/// Failure modes of signing and verification.
#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
pub enum CryptoError {
    /// The signer's public key bytes are not a valid Ed25519 point.
    #[error("public key {signer} is not a valid ed25519 verifying key")]
    MalformedPublicKey {
        /// The offending key, as hex.
        signer: String,
    },
    /// The signature bytes are not a valid Ed25519 signature encoding.
    #[error("signature is not a valid ed25519 signature encoding")]
    MalformedSignature,
    /// The signature did not verify against the key and message.
    #[error("signature does not verify for signer {signer}")]
    SignatureMismatch {
        /// The signer the signature was checked against, as hex.
        signer: String,
    },
    /// The system random source was unavailable.
    #[error("could not read from the system random source: {0}")]
    RandomSource(String),
}

/// An Ed25519 private key.
///
/// Prunella imposes no key management policy. This type can load and export raw bytes;
/// where those bytes live, how they are protected and who may use them are entirely
/// the caller's decisions.
pub struct SigningKey(ed25519_dalek::SigningKey);

impl SigningKey {
    /// Generates a key from the operating system's random source.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::RandomSource`] if the system random source fails.
    pub fn generate() -> Result<Self, CryptoError> {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).map_err(|error| CryptoError::RandomSource(error.to_string()))?;
        Ok(Self::from_seed(seed))
    }

    /// Builds a key from a 32-byte seed.
    ///
    /// Deterministic: the same seed always yields the same key, which is what makes
    /// test chains reproducible.
    #[must_use]
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self(ed25519_dalek::SigningKey::from_bytes(&seed))
    }

    /// Returns the 32-byte seed.
    #[must_use]
    pub fn to_seed(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    /// Returns the matching public key.
    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        PublicKey::from_bytes(self.0.verifying_key().to_bytes())
    }

    /// Signs an arbitrary message.
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> Signature {
        Signature::from_bytes(self.0.sign(message).to_bytes())
    }

    /// Signs a transaction draft and derives the resulting transaction.
    ///
    /// The draft's declared signer is overwritten with this key's public key, so a
    /// transaction can never claim a signer that did not sign it.
    #[must_use]
    pub fn sign_transaction(&self, mut draft: TransactionDraft) -> Transaction {
        draft.signer = self.public_key();
        let signature = self.sign(&draft.signing_message());
        draft.into_transaction(signature)
    }
}

impl core::fmt::Debug for SigningKey {
    /// Renders without the secret, so a key cannot leak through a log line.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SigningKey")
            .field("public_key", &self.public_key())
            .finish()
    }
}

/// Verifies a signature over an arbitrary message.
///
/// # Errors
///
/// Returns [`CryptoError::MalformedPublicKey`] if the key is not a valid curve point,
/// or [`CryptoError::SignatureMismatch`] if the signature does not verify.
pub fn verify(key: &PublicKey, message: &[u8], signature: &Signature) -> Result<(), CryptoError> {
    let verifying_key =
        VerifyingKey::from_bytes(&key.to_bytes()).map_err(|_| CryptoError::MalformedPublicKey {
            signer: key.to_hex(),
        })?;
    let signature = ed25519_dalek::Signature::from_bytes(&signature.to_bytes());
    verifying_key
        .verify_strict(message, &signature)
        .map_err(|_| CryptoError::SignatureMismatch {
            signer: key.to_hex(),
        })
}

/// Verifies a transaction's signature against its recomputed signing message.
///
/// This does not check the transaction id; that is a structural rule and belongs to
/// `prunella-verify`, which reports both failures with their exact location.
///
/// # Errors
///
/// Returns [`CryptoError::MalformedPublicKey`] or [`CryptoError::SignatureMismatch`].
pub fn verify_transaction(transaction: &Transaction) -> Result<(), CryptoError> {
    verify(
        &transaction.signer,
        &transaction.signing_message(),
        &transaction.signature,
    )
}

/// Length of a signing key seed in bytes.
pub const SEED_LEN: usize = 32;

const _: () = {
    assert!(SEED_LEN == PUBLIC_KEY_LEN);
    assert!(SIGNATURE_LEN == 64);
};
