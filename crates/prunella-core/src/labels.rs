//! Chain and application labels, and the scalar newtypes that order a chain.

use crate::error::{CoreError, LabelRejection};
use borsh::{BorshDeserialize, BorshSerialize};

/// Maximum length in bytes of a network id or namespace.
pub const MAX_LABEL_LEN: usize = 64;

/// Validates the shared label grammar.
///
/// A label is 1 to 64 bytes, starts with a lowercase ASCII letter or digit, and
/// continues with lowercase ASCII letters, digits, `.`, `_` or `-`. Uppercase is
/// rejected rather than folded so that each label has exactly one textual form and two
/// labels can never differ only by case.
fn validate_label(value: &str) -> Result<(), LabelRejection> {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return Err(LabelRejection::Empty);
    };
    if value.len() > MAX_LABEL_LEN {
        return Err(LabelRejection::TooLong);
    }
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err(LabelRejection::BadFirstCharacter);
    }
    for character in characters {
        let permitted = character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || matches!(character, '.' | '_' | '-');
        if !permitted {
            return Err(LabelRejection::BadCharacter);
        }
    }
    Ok(())
}

/// Identifies a chain.
///
/// Every block carries the network id, so blocks from one chain can never be imported
/// into another. Prunella assigns no meaning to the value beyond equality.
#[derive(
    BorshSerialize, BorshDeserialize, Clone, PartialEq, Eq, PartialOrd, Ord, std::hash::Hash,
)]
pub struct NetworkId(String);

impl NetworkId {
    /// Validates and wraps a network id.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidNetworkId`] if the value violates the label grammar.
    pub fn new(value: impl Into<String>) -> Result<Self, CoreError> {
        let value = value.into();
        validate_label(&value).map_err(|reason| CoreError::InvalidNetworkId {
            value: value.clone(),
            reason,
        })?;
        Ok(Self(value))
    }

    /// Returns the label text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

crate::impl_label_text!(NetworkId);

/// Labels the application domain a transaction payload belongs to.
///
/// This is the only structured hint Prunella carries about payload meaning, and it is
/// never interpreted: it is a string used for grouping and filtered export. Prunella
/// does not know, and must not know, what any namespace stands for.
#[derive(
    BorshSerialize, BorshDeserialize, Clone, PartialEq, Eq, PartialOrd, Ord, std::hash::Hash,
)]
pub struct Namespace(String);

impl Namespace {
    /// Validates and wraps a namespace.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidNamespace`] if the value violates the label grammar.
    pub fn new(value: impl Into<String>) -> Result<Self, CoreError> {
        let value = value.into();
        validate_label(&value).map_err(|reason| CoreError::InvalidNamespace {
            value: value.clone(),
            reason,
        })?;
        Ok(Self(value))
    }

    /// Returns the label text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

crate::impl_label_text!(Namespace);

/// Version of the payload schema a transaction's producer used.
///
/// Opaque to Prunella: it is stored, hashed and reproduced exactly, never compared
/// against anything or used to decide how to read a payload.
#[derive(
    BorshSerialize,
    BorshDeserialize,
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    std::hash::Hash,
    serde::Serialize,
)]
pub struct SchemaVersion(pub u32);

impl core::fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Position of a block in the chain, starting at zero for genesis.
#[derive(
    BorshSerialize,
    BorshDeserialize,
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    std::hash::Hash,
    serde::Serialize,
)]
pub struct BlockHeight(pub u64);

impl BlockHeight {
    /// The height of the genesis block.
    pub const GENESIS: Self = Self(0);

    /// Returns the height of the block that would follow this one.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::HeightOverflow`] at [`u64::MAX`].
    pub fn next(self) -> Result<Self, CoreError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(CoreError::HeightOverflow { height: self.0 })
    }

    /// Returns the raw height.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }

    /// Returns true when this is the genesis height.
    #[must_use]
    pub const fn is_genesis(self) -> bool {
        self.0 == 0
    }
}

impl core::fmt::Display for BlockHeight {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{NetworkId, validate_label};
    use crate::error::LabelRejection;

    #[test]
    fn accepts_documented_grammar() {
        for value in ["a", "0", "demo", "com.example.governance", "x_1-2.3"] {
            assert!(validate_label(value).is_ok(), "{value} should be accepted");
        }
    }

    #[test]
    fn rejects_out_of_grammar_labels() {
        let cases = [
            ("", LabelRejection::Empty),
            ("-leading", LabelRejection::BadFirstCharacter),
            (".leading", LabelRejection::BadFirstCharacter),
            ("Upper", LabelRejection::BadFirstCharacter),
            ("has Upper", LabelRejection::BadCharacter),
            ("has space", LabelRejection::BadCharacter),
            ("has/slash", LabelRejection::BadCharacter),
            ("emoji\u{1f600}", LabelRejection::BadCharacter),
        ];
        for (value, expected) in cases {
            assert_eq!(validate_label(value), Err(expected), "for {value:?}");
        }
    }

    #[test]
    fn rejects_labels_over_the_length_limit() {
        let long = "a".repeat(super::MAX_LABEL_LEN + 1);
        assert_eq!(validate_label(&long), Err(LabelRejection::TooLong));
        assert!(validate_label(&"a".repeat(super::MAX_LABEL_LEN)).is_ok());
    }

    #[test]
    fn case_differences_are_never_folded_away() {
        assert!(NetworkId::new("demo").is_ok());
        assert!(NetworkId::new("Demo").is_err());
    }
}
