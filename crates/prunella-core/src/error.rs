//! Errors produced while constructing core ledger values.

/// Failure modes of core type construction.
///
/// Every variant names a value that could not be built. None of them describe I/O,
/// storage or verification failures; those belong to the crates that own them.
#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
pub enum CoreError {
    /// A network identifier did not satisfy the documented character rules.
    #[error("invalid network id {value:?}: {reason}")]
    InvalidNetworkId {
        /// The rejected value.
        value: String,
        /// Why it was rejected.
        reason: LabelRejection,
    },
    /// A namespace did not satisfy the documented character rules.
    #[error("invalid namespace {value:?}: {reason}")]
    InvalidNamespace {
        /// The rejected value.
        value: String,
        /// Why it was rejected.
        reason: LabelRejection,
    },
    /// A block carried more transactions than the header's count field can express.
    #[error("block carries {count} transactions, more than the {max} a block may hold")]
    TooManyTransactions {
        /// Number of transactions offered.
        count: usize,
        /// Maximum a block header can express.
        max: u32,
    },
    /// A hex string was not a valid encoding of a fixed-size byte value.
    #[error("expected {expected} hex characters for {kind}, found {found}")]
    HexLength {
        /// Name of the type being parsed.
        kind: &'static str,
        /// Number of characters required.
        expected: usize,
        /// Number of characters supplied.
        found: usize,
    },
    /// A hex string contained a character outside `0-9a-f`.
    #[error("invalid hex for {kind}: {detail}")]
    HexDigits {
        /// Name of the type being parsed.
        kind: &'static str,
        /// Description of the offending input.
        detail: String,
    },
    /// A block height increment would overflow.
    #[error("block height {height} cannot be incremented without overflow")]
    HeightOverflow {
        /// The height that could not be incremented.
        height: u64,
    },
}

/// Why a network id or namespace was rejected.
#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone, Copy)]
pub enum LabelRejection {
    /// The label was empty.
    #[error("must not be empty")]
    Empty,
    /// The label exceeded the maximum length.
    #[error("must be at most 64 bytes")]
    TooLong,
    /// The first character was not a lowercase letter or digit.
    #[error("must start with a lowercase letter or digit")]
    BadFirstCharacter,
    /// A later character was outside the permitted set.
    #[error("may only contain lowercase letters, digits, '.', '_' and '-'")]
    BadCharacter,
}
