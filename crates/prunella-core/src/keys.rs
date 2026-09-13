//! Opaque key and signature containers.
//!
//! These are byte containers only. This crate performs no cryptographic operations;
//! signing and verification live in `prunella-crypto`, which keeps the signature
//! scheme replaceable without touching the ledger types.

use crate::error::CoreError;
use borsh::{BorshDeserialize, BorshSerialize};

/// Length of an Ed25519 public key in bytes.
pub const PUBLIC_KEY_LEN: usize = 32;
/// Length of an Ed25519 signature in bytes.
pub const SIGNATURE_LEN: usize = 64;

/// A transaction signer's public key.
///
/// Prunella does not interpret who or what a key belongs to. Identity, authority and
/// key management are entirely the caller's concern.
#[derive(
    BorshSerialize, BorshDeserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, std::hash::Hash,
)]
pub struct PublicKey([u8; PUBLIC_KEY_LEN]);

impl PublicKey {
    /// Wraps raw key bytes.
    ///
    /// The bytes are not checked for being a valid curve point here; that happens in
    /// `prunella-crypto` at verification time.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; PUBLIC_KEY_LEN]) -> Self {
        Self(bytes)
    }

    /// Returns the raw key bytes.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; PUBLIC_KEY_LEN] {
        self.0
    }

    /// Returns the key as a byte slice.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Parses 64 lowercase hex characters.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::HexLength`] or [`CoreError::HexDigits`] if the input is not
    /// exactly 64 hex characters.
    pub fn from_hex(text: &str) -> Result<Self, CoreError> {
        crate::hex_bytes::<PUBLIC_KEY_LEN>("public key", text).map(Self)
    }
}

crate::impl_hex_text!(PublicKey, "public key");

/// A detached signature over a transaction's signing message.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, PartialEq, Eq)]
pub struct Signature([u8; SIGNATURE_LEN]);

impl Signature {
    /// Wraps raw signature bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; SIGNATURE_LEN]) -> Self {
        Self(bytes)
    }

    /// Returns the raw signature bytes.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; SIGNATURE_LEN] {
        self.0
    }

    /// Returns the signature as a byte slice.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Parses 128 lowercase hex characters.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::HexLength`] or [`CoreError::HexDigits`] if the input is not
    /// exactly 128 hex characters.
    pub fn from_hex(text: &str) -> Result<Self, CoreError> {
        crate::hex_bytes::<SIGNATURE_LEN>("signature", text).map(Self)
    }
}

crate::impl_hex_text!(Signature, "signature");
