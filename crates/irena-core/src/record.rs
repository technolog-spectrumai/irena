//! The notarised envelope that carries company data on the ledger.
//!
//! One envelope, one body, three kinds with **separate amendment chains** — the share
//! register changes often, the voting rules rarely, the genesis almost never, and an
//! amendment to one must not have to name the other two.

use crate::company::{CompanyGenesisV1, CompanyIdV1};
use crate::notarisation::NotarisationV1;
use crate::shares::ShareStructureV1;
use bornite_rules::VotingRulesV1;
use prunella_core::TxId;

/// The only record version this build reads and writes.
pub const RECORD_VERSION: &str = "1.0";

/// The Prunella schema version every Irena transaction declares.
pub const RECORD_SCHEMA_VERSION: u32 = 1;

/// What a record carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordKindV1 {
    /// A `<company-genesis>` element.
    CompanyGenesis,
    /// A `<share-structure>` element.
    ShareStructure,
    /// A `<voting-rules>` element: Bornite's, unchanged.
    VotingRules,
}

impl RecordKindV1 {
    /// Every kind, in a fixed order.
    pub const ALL: [Self; 3] = [
        Self::CompanyGenesis,
        Self::ShareStructure,
        Self::VotingRules,
    ];

    /// The attribute text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CompanyGenesis => "company-genesis",
            Self::ShareStructure => "share-structure",
            Self::VotingRules => "voting-rules",
        }
    }

    /// The element name the body of this kind has.
    #[must_use]
    pub const fn element(self) -> &'static str {
        self.as_str()
    }

    /// The Prunella namespace records of this kind are published under.
    #[must_use]
    pub const fn namespace(self) -> &'static str {
        match self {
            Self::CompanyGenesis => "irena.company.v1",
            Self::ShareStructure => "irena.shares.v1",
            Self::VotingRules => "irena.rules.v1",
        }
    }

    /// Parses the attribute text.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

impl core::fmt::Display for RecordKindV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The element a record carries.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum RecordBodyV1 {
    /// The founding record.
    CompanyGenesis(CompanyGenesisV1),
    /// The share register.
    ShareStructure(ShareStructureV1),
    /// Voting rules.
    VotingRules(VotingRulesV1),
}

impl RecordBodyV1 {
    /// Which kind this body is.
    #[must_use]
    pub const fn kind(&self) -> RecordKindV1 {
        match self {
            Self::CompanyGenesis(_) => RecordKindV1::CompanyGenesis,
            Self::ShareStructure(_) => RecordKindV1::ShareStructure,
            Self::VotingRules(_) => RecordKindV1::VotingRules,
        }
    }
}

/// A parsed Irena record.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct IrenaRecordV1 {
    /// Which company.
    pub company: CompanyIdV1,
    /// The record of the same kind this one amends, if any.
    ///
    /// Must name the record currently in force for (company, kind), or be absent for
    /// the first. The ledger layer enforces that; the type only carries it.
    pub supersedes: Option<TxId>,
    /// Who attests to it, and when.
    pub notarisation: NotarisationV1,
    /// The element it carries.
    pub body: RecordBodyV1,
}

impl IrenaRecordV1 {
    /// Which kind this record is.
    #[must_use]
    pub const fn kind(&self) -> RecordKindV1 {
        self.body.kind()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_round_trip_and_have_distinct_namespaces() {
        let mut namespaces = Vec::new();
        for kind in RecordKindV1::ALL {
            assert_eq!(RecordKindV1::parse(kind.as_str()), Some(kind));
            namespaces.push(kind.namespace());
            assert!(prunella_core::Namespace::new(kind.namespace()).is_ok());
        }
        namespaces.sort_unstable();
        namespaces.dedup();
        assert_eq!(namespaces.len(), RecordKindV1::ALL.len());
        assert_eq!(RecordKindV1::parse("roll"), None);
    }
}
