//! Core ledger types for Prunella and the hash derivations that bind them.
//!
//! This crate is deliberately ignorant of what a ledger is used for. It has no notion
//! of organizations, accounts, permissions, governance or business rules. Application
//! meaning lives entirely in two places, neither of which Prunella interprets:
//!
//! * [`Transaction::payload`], an opaque byte string, and
//! * [`Namespace`], a label used for grouping and filtered export.
//!
//! # Derived values
//!
//! Four values are derived rather than supplied, each in its own hash domain:
//!
//! | Value | Domain | Covers |
//! |---|---|---|
//! | signing message | `PRUNELLA/v1/tx-sign` | namespace, schema version, payload, signer, nonce |
//! | transaction id | `PRUNELLA/v1/tx-id` | the above, plus the signature |
//! | transaction root | `PRUNELLA/v1/tx-root` | transaction count, then each id in order |
//! | block hash | `PRUNELLA/v1/block-header` | the whole header |
//!
//! Every one of them is computed from `prunella-canonical` encodings. No hash in
//! Prunella is ever derived from `Debug`, `Display`, JSON or XML text.
//!
//! # Immutability
//!
//! Nothing here offers a way to mutate a committed block. Blocks are produced from a
//! [`BlockDraft`], which derives `tx_root` and `tx_count` so they cannot be made to
//! disagree with the transaction list, and are read-only from then on.
//!
//! # Serialization
//!
//! The `serde` implementations in this crate are for reporting only. They render hex
//! text and carry extra convenience fields, and are never used as hash pre-images.

mod block;
mod error;
mod hash;
mod head;
mod keys;
mod labels;
mod transaction;

pub use block::{Block, BlockDraft, BlockHeader, GenesisSpec, HEADER_VERSION};
pub use error::{CoreError, LabelRejection};
pub use hash::{Hash, TxId};
pub use head::ChainHead;
pub use keys::{PUBLIC_KEY_LEN, PublicKey, SIGNATURE_LEN, Signature};
pub use labels::{BlockHeight, MAX_LABEL_LEN, Namespace, NetworkId, SchemaVersion};
pub use transaction::{Transaction, TransactionDraft};

/// Protocol version 1 names for the wire types.
///
/// Prunella Protocol V1 is the only protocol this build implements: [`BlockHeader::version`]
/// is always [`HEADER_VERSION`], and a header declaring anything else is rejected. These
/// aliases exist so code that wants to be explicit about which protocol version it is
/// handling can say so. When a version 2 arrives it will introduce distinct types
/// alongside these, rather than changing what `TransactionV1` means.
pub mod v1 {
    /// Protocol version 1 transaction. See [`Transaction`](super::Transaction).
    pub type TransactionV1 = super::Transaction;
    /// Protocol version 1 block header. See [`BlockHeader`](super::BlockHeader).
    pub type BlockHeaderV1 = super::BlockHeader;
    /// Protocol version 1 block. See [`Block`](super::Block).
    pub type BlockV1 = super::Block;
}

/// Parses a fixed-size byte value from strictly lowercase hex.
///
/// Uppercase is rejected rather than accepted, so each value has exactly one textual
/// form and two renderings of the same bytes can never differ.
pub(crate) fn hex_bytes<const N: usize>(
    kind: &'static str,
    text: &str,
) -> Result<[u8; N], CoreError> {
    if text.len() != N * 2 {
        return Err(CoreError::HexLength {
            kind,
            expected: N * 2,
            found: text.len(),
        });
    }
    if let Some(offending) = text
        .chars()
        .find(|c| !c.is_ascii_digit() && !matches!(c, 'a'..='f'))
    {
        return Err(CoreError::HexDigits {
            kind,
            detail: format!("expected lowercase hex, found {offending:?}"),
        });
    }
    let mut bytes = [0u8; N];
    hex::decode_to_slice(text, &mut bytes).map_err(|error| CoreError::HexDigits {
        kind,
        detail: error.to_string(),
    })?;
    Ok(bytes)
}

/// Implements the shared text and reporting behaviour of fixed-size byte newtypes.
macro_rules! impl_hex_text {
    ($type:ty, $kind:literal) => {
        impl core::fmt::Display for $type {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                for byte in self.as_bytes() {
                    write!(f, "{byte:02x}")?;
                }
                Ok(())
            }
        }

        impl core::fmt::Debug for $type {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                core::fmt::Display::fmt(self, f)
            }
        }

        impl core::str::FromStr for $type {
            type Err = $crate::CoreError;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                Self::from_hex(text)
            }
        }

        impl serde::Serialize for $type {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl $type {
            /// Returns the value rendered as lowercase hex.
            #[must_use]
            pub fn to_hex(&self) -> String {
                self.to_string()
            }

            /// The human-facing name of this value's type, used in parse errors.
            pub const KIND: &'static str = $kind;
        }
    };
}

/// Implements the shared text and reporting behaviour of validated string labels.
macro_rules! impl_label_text {
    ($type:ty) => {
        impl core::fmt::Display for $type {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl core::fmt::Debug for $type {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "{:?}", self.as_str())
            }
        }

        impl core::str::FromStr for $type {
            type Err = $crate::CoreError;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                Self::new(text)
            }
        }

        impl serde::Serialize for $type {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }
    };
}

pub(crate) use {impl_hex_text, impl_label_text};
