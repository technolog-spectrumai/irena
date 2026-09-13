//! Deterministic canonical serialization and domain-separated hashing for Prunella.
//!
//! This crate is deliberately ledger-agnostic: it knows nothing about blocks,
//! transactions or chains. It provides exactly two primitives that the rest of
//! Prunella builds on:
//!
//! 1. A **canonical byte encoding** ([`encode`] / [`decode`]) with a single valid
//!    representation per value, used as the pre-image for every hash and signature.
//! 2. **Domain-separated hashing** ([`hash_domain`], [`DomainHasher`]) so that the
//!    same bytes hashed for two different purposes can never collide.
//!
//! # The canonical subset
//!
//! The encoding is [Borsh](https://borsh.io), restricted to a subset chosen so that
//! exactly one byte string can represent a given value:
//!
//! * fixed-width unsigned integers, encoded little-endian;
//! * fixed-size byte arrays, encoded verbatim;
//! * `Vec<u8>`, `String` and `Vec<T>`, encoded as a `u32` little-endian length
//!   followed by the elements;
//! * structs, encoded as their fields in declaration order.
//!
//! Types that are hashed or signed **must not** contain maps, sets, floating point
//! numbers or `Option`s: map and set iteration order is not guaranteed, floats have
//! multiple bit patterns for the same value, and `Option` invites an encoder to treat
//! an absent value and a default value as interchangeable.
//!
//! Human-facing renderings (`Debug`, `Display`, JSON, XML) are never valid hash
//! pre-images. Only [`encode`] output is.
//!
//! # Stability
//!
//! Chain hashes depend on these bytes, so the Borsh dependency is pinned to a single
//! major version and the encoding is covered by golden vectors in the crate's tests.
//! Any upstream change to the encoding breaks those tests loudly instead of silently
//! moving every hash in every chain.

use borsh::{BorshDeserialize, BorshSerialize};

/// Domain separation tags.
///
/// Every hash computed anywhere in Prunella is prefixed with one of these tags, so a
/// byte string that is a valid pre-image for one purpose cannot be reinterpreted as a
/// pre-image for another.
pub mod domain {
    /// Pre-image of the bytes a transaction signer signs over.
    pub const TX_SIGN: &str = "PRUNELLA/v1/tx-sign";
    /// Pre-image of a transaction identifier.
    pub const TX_ID: &str = "PRUNELLA/v1/tx-id";
    /// Pre-image of the transaction root of a block with **no** transactions.
    ///
    /// A non-empty block's root is a Merkle tree over [`TX_LEAF`] and [`TX_NODE`]
    /// hashes; the empty tree needs a value of its own that no leaf or node can equal.
    pub const TX_ROOT: &str = "PRUNELLA/v1/tx-root";
    /// Pre-image of a Merkle leaf: one transaction id.
    pub const TX_LEAF: &str = "PRUNELLA/v1/tx-leaf";
    /// Pre-image of a Merkle interior node: left child hash then right child hash.
    pub const TX_NODE: &str = "PRUNELLA/v1/tx-node";
    /// Pre-image of a block hash.
    pub const BLOCK_HEADER: &str = "PRUNELLA/v1/block-header";
}

/// Length in bytes of every digest produced by this crate.
pub const DIGEST_LEN: usize = 32;

/// A raw 32-byte BLAKE3 digest, before any Prunella type wraps it.
pub type Digest = [u8; DIGEST_LEN];

/// Failure modes of canonical encoding and decoding.
#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
#[non_exhaustive]
pub enum CanonicalError {
    /// The value could not be encoded. Impossible for the canonical subset.
    #[error("canonical encoding failed: {0}")]
    Encode(String),
    /// The bytes are not a valid encoding of the requested type.
    #[error("canonical decoding failed: {0}")]
    Decode(String),
    /// Decoding succeeded but did not consume the whole input.
    ///
    /// Canonical encodings have exactly one representation, so trailing bytes always
    /// mean the input is not canonical, even when a prefix of it decodes cleanly.
    #[error("canonical decoding left {trailing} unconsumed byte(s)")]
    TrailingBytes {
        /// Number of bytes left over after a successful decode.
        trailing: usize,
    },
}

/// A type with a single canonical byte representation.
///
/// Implementors must stay inside the canonical subset documented at the crate root.
pub trait Canonical: BorshSerialize + BorshDeserialize + Sized {
    /// Returns the canonical encoding of this value.
    ///
    /// # Panics
    ///
    /// Panics if the value cannot be encoded, which is unreachable for the canonical
    /// subset: encoding writes into an in-memory buffer and none of the permitted
    /// types can fail to serialize. A panic here means an implementor stepped outside
    /// the subset, which is a bug rather than a runtime condition to recover from.
    #[must_use]
    fn canonical_bytes(&self) -> Vec<u8> {
        encode(self).expect("types in the canonical subset always encode")
    }

    /// Decodes a value from its canonical encoding, rejecting trailing bytes.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalError::Decode`] if the bytes are not a valid encoding, or
    /// [`CanonicalError::TrailingBytes`] if they encode a value followed by anything
    /// else.
    fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, CanonicalError> {
        decode(bytes)
    }
}

/// Encodes a value into its canonical byte representation.
///
/// # Errors
///
/// Returns [`CanonicalError::Encode`] if the underlying serializer fails. This cannot
/// happen for the canonical subset; the fallible signature exists so that a type
/// outside the subset surfaces an error instead of corrupting a hash.
pub fn encode<T: BorshSerialize + ?Sized>(value: &T) -> Result<Vec<u8>, CanonicalError> {
    borsh::to_vec(value).map_err(|error| CanonicalError::Encode(error.to_string()))
}

/// Decodes a value from canonical bytes, rejecting any trailing input.
///
/// # Errors
///
/// Returns [`CanonicalError::Decode`] if the bytes are not a valid encoding of `T`,
/// or [`CanonicalError::TrailingBytes`] if decoding leaves input unconsumed.
pub fn decode<T: BorshDeserialize>(bytes: &[u8]) -> Result<T, CanonicalError> {
    let mut cursor = bytes;
    let value =
        T::deserialize(&mut cursor).map_err(|error| CanonicalError::Decode(error.to_string()))?;
    if !cursor.is_empty() {
        return Err(CanonicalError::TrailingBytes {
            trailing: cursor.len(),
        });
    }
    Ok(value)
}

/// Incremental hasher bound to a domain separation tag.
///
/// The tag is absorbed as a little-endian `u32` length followed by the tag bytes, so
/// no tag can be a prefix of another tag plus payload. Use this when the pre-image is
/// a sequence of byte strings that should not be concatenated in memory first.
#[derive(Clone)]
pub struct DomainHasher {
    inner: blake3::Hasher,
}

impl DomainHasher {
    /// Starts a hash in the given domain.
    ///
    /// # Panics
    ///
    /// Panics if the tag is longer than [`u32::MAX`] bytes. Tags are compile-time
    /// constants, so this is unreachable in practice.
    #[must_use]
    pub fn new(tag: &str) -> Self {
        let tag_len = u32::try_from(tag.len()).expect("domain tags are short constants");
        let mut inner = blake3::Hasher::new();
        inner.update(&tag_len.to_le_bytes());
        inner.update(tag.as_bytes());
        Self { inner }
    }

    /// Absorbs more pre-image bytes.
    pub fn update(&mut self, bytes: &[u8]) -> &mut Self {
        self.inner.update(bytes);
        self
    }

    /// Finishes the hash and returns the digest.
    #[must_use]
    pub fn finalize(&self) -> Digest {
        *self.inner.finalize().as_bytes()
    }
}

impl core::fmt::Debug for DomainHasher {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DomainHasher").finish_non_exhaustive()
    }
}

/// Hashes `bytes` within the domain named by `tag`.
///
/// Equivalent to `blake3(u32_le(tag.len()) || tag || bytes)`.
#[must_use]
pub fn hash_domain(tag: &str, bytes: &[u8]) -> Digest {
    let mut hasher = DomainHasher::new(tag);
    hasher.update(bytes);
    hasher.finalize()
}

/// Encodes `value` canonically and hashes the result within the domain named by `tag`.
///
/// This is the only way Prunella derives a hash from a structured value.
#[must_use]
pub fn hash_canonical<T: Canonical>(tag: &str, value: &T) -> Digest {
    hash_domain(tag, &value.canonical_bytes())
}
