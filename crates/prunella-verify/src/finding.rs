//! Structured verification findings and the locations they point at.

use prunella_core::{BlockHeight, Hash, TxId};

/// Where in a chain a finding was produced.
///
/// Every field is optional because not every finding has every coordinate: a missing
/// block has a height but no hash, and a chain-level problem may have neither. What is
/// present is always exact — a location never approximates.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Location {
    /// Height of the block the finding concerns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<BlockHeight>,
    /// Hash of that block, as recomputed from its contents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_hash: Option<Hash>,
    /// Index of the transaction within the block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_index: Option<u32>,
    /// Identifier the transaction declares for itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_id: Option<TxId>,
}

impl Location {
    /// A location naming only a block height.
    #[must_use]
    pub fn at_height(height: BlockHeight) -> Self {
        Self {
            height: Some(height),
            ..Self::default()
        }
    }

    /// Adds the block hash.
    #[must_use]
    pub fn with_block_hash(mut self, hash: Hash) -> Self {
        self.block_hash = Some(hash);
        self
    }

    /// Adds the transaction index and declared id.
    #[must_use]
    pub fn with_transaction(mut self, index: u32, id: TxId) -> Self {
        self.tx_index = Some(index);
        self.tx_id = Some(id);
        self
    }
}

impl core::fmt::Display for Location {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut parts: Vec<String> = Vec::new();
        if let Some(height) = self.height {
            parts.push(format!("height {height}"));
        }
        if let Some(hash) = self.block_hash {
            parts.push(format!("block {hash}"));
        }
        if let Some(index) = self.tx_index {
            parts.push(format!("transaction #{index}"));
        }
        if let Some(id) = self.tx_id {
            parts.push(format!("id {id}"));
        }
        if parts.is_empty() {
            f.write_str("chain")
        } else {
            f.write_str(&parts.join(", "))
        }
    }
}

/// What kind of defect a finding reports.
///
/// Every variant means the chain is not valid. There is no warning level: a ledger
/// that is almost consistent is inconsistent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    /// The header declares a format version this build does not understand.
    UnsupportedHeaderVersion,
    /// The block belongs to a different chain.
    NetworkMismatch,
    /// The recorded genesis hash does not match the block at height zero.
    GenesisMismatch,
    /// The block's height does not follow its parent's.
    HeightOutOfOrder,
    /// A block that should exist within the verified range is absent.
    MissingBlock,
    /// The block does not point at its parent's hash.
    PreviousHashMismatch,
    /// The block's recomputed hash differs from the hash it was looked up by.
    BlockHashMismatch,
    /// The header's transaction root does not match the transactions carried.
    TxRootMismatch,
    /// The header's transaction count does not match the transactions carried.
    TxCountMismatch,
    /// A transaction's declared id does not match its recomputed id.
    TxIdMismatch,
    /// A transaction's signature does not verify against its signer.
    SignatureInvalid,
    /// The same transaction id appears twice within one block.
    DuplicateTxIdInBlock,
    /// A transaction id already committed elsewhere in the chain appears again.
    DuplicateTxIdInChain,
    /// A block declares an earlier timestamp than its parent.
    TimestampRegression,
    /// Stored bytes could not be decoded into a block.
    DecodeError,
    /// The chain could not be read.
    SourceFailure,
}

impl FindingKind {
    /// Returns a stable snake_case identifier, suitable for machine consumption.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedHeaderVersion => "unsupported_header_version",
            Self::NetworkMismatch => "network_mismatch",
            Self::GenesisMismatch => "genesis_mismatch",
            Self::HeightOutOfOrder => "height_out_of_order",
            Self::MissingBlock => "missing_block",
            Self::PreviousHashMismatch => "previous_hash_mismatch",
            Self::BlockHashMismatch => "block_hash_mismatch",
            Self::TxRootMismatch => "tx_root_mismatch",
            Self::TxCountMismatch => "tx_count_mismatch",
            Self::TxIdMismatch => "tx_id_mismatch",
            Self::SignatureInvalid => "signature_invalid",
            Self::DuplicateTxIdInBlock => "duplicate_tx_id_in_block",
            Self::DuplicateTxIdInChain => "duplicate_tx_id_in_chain",
            Self::TimestampRegression => "timestamp_regression",
            Self::DecodeError => "decode_error",
            Self::SourceFailure => "source_failure",
        }
    }
}

impl core::fmt::Display for FindingKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One defect, with the exact place it was found.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Finding {
    /// What is wrong.
    pub kind: FindingKind,
    /// Where it is wrong.
    pub location: Location,
    /// Human-readable specifics, such as the expected and actual values.
    ///
    /// This text is for operators. It is never hashed, never parsed, and never part of
    /// any decision the code makes.
    pub detail: String,
}

impl Finding {
    /// Builds a finding.
    #[must_use]
    pub fn new(kind: FindingKind, location: Location, detail: impl Into<String>) -> Self {
        Self {
            kind,
            location,
            detail: detail.into(),
        }
    }
}

impl core::fmt::Display for Finding {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "[{}] at {}: {}", self.kind, self.location, self.detail)
    }
}
