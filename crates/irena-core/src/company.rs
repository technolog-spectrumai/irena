//! The company itself: its label on the ledger and its founding identity.

use crate::error::{IrenaError, IssueV1};
use prunella_core::Hash;

/// Maximum length of a company id, in bytes.
pub const MAX_COMPANY_ID_LEN: usize = 64;

/// An opaque label identifying a company on a ledger.
///
/// `acme`, `acme-industries-ltd`, `co.01234567` — compared for equality only, never
/// interpreted. Grammar: 1–64 bytes, `a-z0-9` first, then `a-z0-9._-`, so it is also a
/// valid Prunella label and can be used in file names without escaping.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct CompanyIdV1(String);

impl CompanyIdV1 {
    /// Validates and wraps a company id.
    ///
    /// # Errors
    ///
    /// Returns [`IrenaError::Invalid`] with one [`IssueV1::InvalidValue`] naming what is
    /// wrong.
    pub fn new(value: impl Into<String>) -> Result<Self, IrenaError> {
        let value = value.into();
        let reject = |reason: &str| {
            IrenaError::invalid(vec![IssueV1::InvalidValue {
                element: "irena-record",
                attribute: "company",
                value: value.clone(),
                reason: reason.to_owned(),
            }])
        };
        let mut bytes = value.bytes();
        let Some(first) = bytes.next() else {
            return Err(reject("must not be empty"));
        };
        if value.len() > MAX_COMPANY_ID_LEN {
            return Err(reject("must be at most 64 bytes"));
        }
        if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
            return Err(reject("must start with a lowercase letter or digit"));
        }
        if !bytes.all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        }) {
            return Err(reject(
                "may only contain lowercase letters, digits, '.', '_' and '-'",
            ));
        }
        Ok(Self(value))
    }

    /// The label text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for CompanyIdV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::fmt::Debug for CompanyIdV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl core::str::FromStr for CompanyIdV1 {
    type Err = IrenaError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::new(text)
    }
}

/// Who the company is, as the founding record states it.
///
/// Free text throughout. Irena stores and reproduces it and compares none of it: a
/// company is identified by its [`CompanyIdV1`], and this is what that id stands for.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct IdentityV1 {
    /// The registered name. Non-empty.
    pub name: String,
    /// The jurisdiction of incorporation, in whatever form the notary uses (`gb`,
    /// `us-de`, `Delaware`).
    pub jurisdiction: Option<String>,
    /// The registrar's number for the company.
    pub registered_number: Option<String>,
}

impl IdentityV1 {
    /// Validates the identity: the name must be non-empty.
    ///
    /// # Errors
    ///
    /// Returns [`IrenaError::Invalid`] listing every problem.
    pub fn validate(&self) -> Result<(), IrenaError> {
        if self.name.trim().is_empty() {
            return Err(IrenaError::invalid(vec![IssueV1::InvalidValue {
                element: "identity",
                attribute: "name",
                value: self.name.clone(),
                reason: "must not be empty".to_owned(),
            }]));
        }
        Ok(())
    }
}

/// The founding record: the whole company as it starts.
///
/// One document founds a company — who it is, who holds its shares, and who decides
/// how. Everything a later record can amend is here first, so the state at any
/// height is this document plus the amendments up to that height, and nothing else.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CompanyGenesisV1 {
    /// Who the company is.
    pub identity: IdentityV1,
    /// Digest of the certificate of incorporation or equivalent, if one is referenced.
    ///
    /// Opaque: Irena never sees the document, only commits to which one was meant.
    pub incorporation_digest: Option<Hash>,
    /// The initial share register.
    pub shares: crate::shares::ShareStructureV1,
    /// The initial governance configuration: every channel the company decides
    /// through, each with its actor source and mode.
    pub channels: crate::channel::DecisionChannelsV1,
    /// The initial identities: every person and the key they sign with.
    pub identities: crate::identities::IdentitiesV1,
    /// The initial authorisation: who may sign which family of record.
    pub authorisation: crate::authorisation::AuthorisationV1,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn company_ids_follow_the_label_grammar() {
        for good in ["acme", "a", "co.01234567", "acme-industries_ltd", "9lives"] {
            assert!(CompanyIdV1::new(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "Acme",
            "-acme",
            ".a",
            "acme inc",
            "a/b",
            &"x".repeat(65),
        ] {
            let error = CompanyIdV1::new(bad).expect_err(bad);
            assert!(
                matches!(
                    error.issues(),
                    [IssueV1::InvalidValue {
                        element: "irena-record",
                        attribute: "company",
                        ..
                    }]
                ),
                "{bad}: {error}"
            );
        }
    }

    #[test]
    fn an_identity_needs_a_name() {
        let identity = IdentityV1 {
            name: "  ".to_owned(),
            jurisdiction: None,
            registered_number: None,
        };
        let error = identity.validate().expect_err("blank name");
        assert_eq!(error.issues().len(), 1);
    }
}
