//! The notarised envelope that carries company data on the ledger.
//!
//! One envelope, one body. The first record of a company is its genesis, which carries
//! everything; every later record amends exactly one part — identity, share register,
//! decision channels, identities or authorisation — and names the record that
//! currently provides that part. The
//! company at any height is the genesis plus the amendments up to there, applied in
//! chain order (`irena-ledger`).

use crate::authorisation::AuthorisationV1;
use crate::channel::DecisionChannelsV1;
use crate::company::{CompanyGenesisV1, CompanyIdV1, IdentityV1};
use crate::identities::IdentitiesV1;
use crate::notarisation::NotarisationV1;
use crate::shares::ShareStructureV1;
use prunella_core::TxId;

/// The only record version this build reads and writes.
pub const RECORD_VERSION: &str = "1.0";

/// The Prunella schema version every Irena transaction declares.
pub const RECORD_SCHEMA_VERSION: u32 = 1;

/// What a record carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordKindV1 {
    /// A `<company-genesis>` element: the whole company, once.
    CompanyGenesis,
    /// An `<identity>` element: amends who the company is.
    Identity,
    /// A `<share-structure>` element: amends the register.
    ShareStructure,
    /// A `<decision-channels>` element: amends who decides, and how.
    DecisionChannels,
    /// An `<identities>` element: amends who the persons are and which key each signs with.
    Identities,
    /// An `<authorisation>` element: amends who may sign which family of record.
    Authorisation,
}

impl RecordKindV1 {
    /// Every kind, in a fixed order.
    pub const ALL: [Self; 6] = [
        Self::CompanyGenesis,
        Self::Identity,
        Self::ShareStructure,
        Self::DecisionChannels,
        Self::Identities,
        Self::Authorisation,
    ];

    /// The kinds that amend one part of a founded company.
    pub const AMENDMENTS: [Self; 5] = [
        Self::Identity,
        Self::ShareStructure,
        Self::DecisionChannels,
        Self::Identities,
        Self::Authorisation,
    ];

    /// The attribute text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CompanyGenesis => "company-genesis",
            Self::Identity => "identity",
            Self::ShareStructure => "share-structure",
            Self::DecisionChannels => "decision-channels",
            Self::Identities => "identities",
            Self::Authorisation => "authorisation",
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
            Self::CompanyGenesis | Self::Identity => "irena.company.v1",
            Self::ShareStructure => "irena.shares.v1",
            Self::DecisionChannels => "irena.channels.v1",
            Self::Identities => "irena.identities.v1",
            Self::Authorisation => "irena.authorisation.v1",
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
    /// An amended identity.
    Identity(IdentityV1),
    /// An amended share register.
    ShareStructure(ShareStructureV1),
    /// An amended channel set.
    DecisionChannels(DecisionChannelsV1),
    /// Amended identities.
    Identities(IdentitiesV1),
    /// Amended authorisation.
    Authorisation(AuthorisationV1),
}

impl RecordBodyV1 {
    /// Which kind this body is.
    #[must_use]
    pub const fn kind(&self) -> RecordKindV1 {
        match self {
            Self::CompanyGenesis(_) => RecordKindV1::CompanyGenesis,
            Self::Identity(_) => RecordKindV1::Identity,
            Self::ShareStructure(_) => RecordKindV1::ShareStructure,
            Self::DecisionChannels(_) => RecordKindV1::DecisionChannels,
            Self::Identities(_) => RecordKindV1::Identities,
            Self::Authorisation(_) => RecordKindV1::Authorisation,
        }
    }
}

/// A parsed Irena record.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct IrenaRecordV1 {
    /// Which company.
    pub company: CompanyIdV1,
    /// The record that currently provides the part this one amends.
    ///
    /// Absent on a genesis; on an amendment it must name the genesis or the last
    /// amendment of the same part. The ledger layer enforces that; the type only
    /// carries it.
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
        assert_eq!(
            namespaces.len(),
            5,
            "genesis and identity share a namespace"
        );
        assert_eq!(RecordKindV1::parse("roll"), None);
    }
}
