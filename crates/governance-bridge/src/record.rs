//! Governance records: what the bridge stores as a transaction payload.
//!
//! A record wraps exactly the same `<voting-rules>` or `<electorate>` element a
//! standalone Bornite document contains. It adds three things and nothing else: which
//! subject it governs, which earlier record it amends, and an opaque notarisation.
//!
//! Organisation-specific truth — a share register, membership minutes, a fleet
//! manifest — enters only through the notarisation, as a digest of a document the
//! notary attests to. The bridge stores and reproduces it. It never parses it.

use crate::error::BridgeError;
use bornite_core::ElectorateV1;
use bornite_rules::VotingRulesV1;
use prunella_core::{Hash, TxId};

/// What a record carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordKindV1 {
    /// A `<voting-rules>` element.
    VotingRules,
    /// An `<electorate>` element: the roll.
    Roll,
}

impl RecordKindV1 {
    /// The attribute text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VotingRules => "voting-rules",
            Self::Roll => "roll",
        }
    }

    /// The Prunella namespace records of this kind are published under.
    #[must_use]
    pub const fn namespace(self) -> &'static str {
        match self {
            Self::VotingRules => "governance.rules",
            Self::Roll => "governance.roll",
        }
    }

    /// Parses the attribute text.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "voting-rules" => Some(Self::VotingRules),
            "roll" => Some(Self::Roll),
            _ => None,
        }
    }
}

impl core::fmt::Display for RecordKindV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An opaque label naming what is governed.
///
/// `acme-agm-2026`, `membership`, `swarm-alpha` — the bridge compares subjects for
/// equality and never interprets them. Grammar: 1–64 bytes, `a-z0-9` first, then
/// `a-z0-9._-`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct SubjectV1(String);

impl SubjectV1 {
    /// Validates and wraps a subject.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError::InvalidSubject`] if the grammar is not met.
    pub fn new(value: impl Into<String>) -> Result<Self, BridgeError> {
        let value = value.into();
        let reject = |reason| BridgeError::InvalidSubject {
            value: value.clone(),
            reason,
        };
        let mut bytes = value.bytes();
        let Some(first) = bytes.next() else {
            return Err(reject("must not be empty"));
        };
        if value.len() > 64 {
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

impl core::fmt::Display for SubjectV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::fmt::Debug for SubjectV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// Who attests to a record, and to what.
///
/// This is the only place organisation-specific truth touches the ledger, and it does
/// so as an opaque reference: the digest of some external document. What that document
/// is — a share register, minutes, a fleet manifest — is between the notary and whoever
/// reads the record. The bridge does not know and does not need to.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct NotarisationV1 {
    /// An opaque identifier for the notary.
    pub notary: String,
    /// Free text.
    pub statement: Option<String>,
    /// Digest of the external document attested to.
    pub source_digest: Option<Hash>,
}

/// The element a record carries.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum RecordBodyV1 {
    /// Voting rules.
    VotingRules(VotingRulesV1),
    /// An electorate roll.
    Roll(ElectorateV1),
}

impl RecordBodyV1 {
    /// Which kind this body is.
    #[must_use]
    pub const fn kind(&self) -> RecordKindV1 {
        match self {
            Self::VotingRules(_) => RecordKindV1::VotingRules,
            Self::Roll(_) => RecordKindV1::Roll,
        }
    }
}

/// A parsed governance record.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct GovernanceRecordV1 {
    /// What is governed.
    pub subject: SubjectV1,
    /// The record this one amends, if any.
    pub supersedes: Option<TxId>,
    /// Who attests to it.
    pub notarisation: Option<NotarisationV1>,
    /// The element it carries.
    pub body: RecordBodyV1,
}

impl GovernanceRecordV1 {
    /// Which kind this record is.
    #[must_use]
    pub const fn kind(&self) -> RecordKindV1 {
        self.body.kind()
    }
}
