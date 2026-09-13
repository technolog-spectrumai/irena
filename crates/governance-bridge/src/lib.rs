//! The bridge between Prunella (the ledger) and Bornite (the voting engine).
//!
//! This is the only crate that knows both exist. It stores Bornite voting rules and
//! electorate rolls on a Prunella ledger as transaction payloads, records every
//! amendment, and resolves what was in force at any height so a vote can be evaluated
//! against ledger truth.
//!
//! # The boundary it keeps
//!
//! * A record's payload is a `<governance-record>` wrapping **exactly** the
//!   `<voting-rules>` or `<electorate>` element a standalone Bornite document contains,
//!   embedded byte for byte. One schema defines the element; the ledger reuses it.
//! * The rules carry no organisational meaning. The same record organises a company
//!   meeting, a non-profit's membership vote, or a drone swarm deciding a peaceful
//!   deployment, and it is enough to hold a vote with no organisation behind it at all.
//! * Organisation-specific truth — a share register, minutes, a fleet manifest — enters
//!   only as an opaque [`NotarisationV1`]: a notary id and the digest of an external
//!   document. The bridge stores it and never parses it.
//! * Neither `prunella-*` nor `bornite-*` depends on this crate or knows it exists.
//!
//! # Amendments
//!
//! A record that amends another must name the record **currently in force**; amending
//! a version you have not seen is refused as stale. Walking a subject's history checks
//! the chain link by link, and a break — possible only if something wrote around the
//! bridge — is reported, never repaired.

mod error;
mod ledger;
mod record;
mod xml;

pub use error::BridgeError;
pub use ledger::{
    EvaluationAtV1, InForceV1, RECORD_SCHEMA_VERSION, RecordRefV1, evaluate_at, genesis_with_rules,
    history, publish_roll, publish_rules, roll_in_force, rules_in_force,
};
pub use record::{GovernanceRecordV1, NotarisationV1, RecordBodyV1, RecordKindV1, SubjectV1};
pub use xml::{compose_record, read_ballots_document, read_electorate_document, read_record};
