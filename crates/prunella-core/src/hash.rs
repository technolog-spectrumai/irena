//! Fixed-size digests and the identifiers built from them.

use crate::error::CoreError;
use borsh::{BorshDeserialize, BorshSerialize};
use prunella_canonical::{DIGEST_LEN, Digest};

/// A 32-byte BLAKE3 digest.
///
/// Used for block hashes, transaction roots and, wrapped in [`TxId`], transaction
/// identifiers. The textual form is always 64 lowercase hex characters.
#[derive(
    BorshSerialize, BorshDeserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, std::hash::Hash,
)]
pub struct Hash([u8; DIGEST_LEN]);

impl Hash {
    /// The all-zero hash, used as the previous hash of a genesis block.
    ///
    /// It is not a valid block hash: no block hashes to zero in practice, and
    /// verification treats a zero previous hash as meaningful only at height 0.
    pub const ZERO: Self = Self([0u8; DIGEST_LEN]);

    /// Wraps raw digest bytes.
    #[must_use]
    pub const fn from_bytes(bytes: Digest) -> Self {
        Self(bytes)
    }

    /// Returns the raw digest bytes.
    #[must_use]
    pub const fn to_bytes(self) -> Digest {
        self.0
    }

    /// Returns the digest as a byte slice.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns true when this is [`Hash::ZERO`].
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; DIGEST_LEN]
    }

    /// Parses 64 lowercase hex characters.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::HexLength`] or [`CoreError::HexDigits`] if the input is not
    /// exactly 64 hex characters.
    pub fn from_hex(text: &str) -> Result<Self, CoreError> {
        crate::hex_bytes::<DIGEST_LEN>("hash", text).map(Self)
    }
}

crate::impl_hex_text!(Hash, "hash");

/// A transaction identifier.
///
/// A distinct type from [`Hash`] so that a block hash can never be passed where a
/// transaction id is expected, and vice versa.
#[derive(
    BorshSerialize, BorshDeserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, std::hash::Hash,
)]
pub struct TxId(Hash);

impl TxId {
    /// Wraps a digest as a transaction id.
    #[must_use]
    pub const fn from_hash(hash: Hash) -> Self {
        Self(hash)
    }

    /// Returns the underlying digest.
    #[must_use]
    pub const fn hash(self) -> Hash {
        self.0
    }

    /// Returns the raw digest bytes.
    #[must_use]
    pub const fn to_bytes(self) -> Digest {
        self.0.to_bytes()
    }

    /// Returns the id as a byte slice.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Parses 64 lowercase hex characters.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::HexLength`] or [`CoreError::HexDigits`] if the input is not
    /// exactly 64 hex characters.
    pub fn from_hex(text: &str) -> Result<Self, CoreError> {
        crate::hex_bytes::<DIGEST_LEN>("transaction id", text).map(|bytes| Self(Hash(bytes)))
    }
}

crate::impl_hex_text!(TxId, "transaction id");
