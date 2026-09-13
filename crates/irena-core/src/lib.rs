//! The Irena company model.
//!
//! Irena is the layer that knows what a company is. Below it sit two engines that do
//! not: Prunella, an immutable ledger that stores opaque payloads, and Bornite, a
//! voting engine that counts ids and integer weights. This crate defines the company
//! data those engines carry and count, and the XML that data travels as.
//!
//! # What is here
//!
//! * [`CompanyGenesisV1`] — the whole company as founded: its [`IdentityV1`], its
//!   initial [`ShareStructureV1`] and its initial governance, Bornite's
//!   `VotingRulesV1` reused **unchanged**. One document founds a company.
//! * [`ShareStructureV1`] — who holds how many shares, and the signing key each holder
//!   votes with. **Flat shares**: every share is one vote. Share classes are a later
//!   version of this body, not an attribute bolted onto it.
//! * [`IrenaRecordV1`] — the notarised envelope that puts the genesis, or an amendment
//!   to one part of it (identity, register, rules), on the ledger: which company, which
//!   record it amends, and who attested to it and when.
//!
//! # What is deliberately not here
//!
//! No ledger access, no voting, no meetings. This crate is data and its validation.
//! `irena-ledger` puts records on a chain and resolves what is in force; `irena-vote`
//! turns a share structure into a Bornite electorate and runs a vote.
//!
//! # Validation
//!
//! Every reader is strict: unknown elements and attributes are refused, never skipped,
//! and every content issue in a document is collected and reported together as
//! [`IrenaError::Invalid`]. The published schemas under `schemas/irena-*.xsd` are the
//! contract for other tooling; the readers enforce the same rules in code.

mod company;
mod error;
mod notarisation;
mod record;
mod shares;
mod xml;

pub use bornite_rules::VotingRulesV1;
pub use company::{CompanyGenesisV1, CompanyIdV1, IdentityV1};
pub use error::{IrenaError, IssueV1};
pub use notarisation::{NotarisationV1, NotaryIdV1, NotaryTimeV1};
pub use record::{
    IrenaRecordV1, RECORD_SCHEMA_VERSION, RECORD_VERSION, RecordBodyV1, RecordKindV1,
};
pub use shares::{HolderV1, MAX_HOLDERS, ShareStructureV1};
pub use xml::{
    DEFAULT_MAX_DOCUMENT_BYTES, compose_record, read_company_genesis_document,
    read_identity_document, read_record, read_record_with_limit, read_share_structure_document,
    read_voting_rules_document,
};
