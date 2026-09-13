//! Irena records on a Prunella ledger.
//!
//! This crate puts [`irena_core`] records on a chain and answers one question about
//! it: **what is the company at height `h`?** Its genesis, its share register and its
//! voting rules, each as the record in force then, each with the transaction it came
//! from. That is what "Irena knows what the rules are now" means concretely.
//!
//! # How records sit on the chain
//!
//! A record is a Prunella transaction whose payload is one `<irena-record>` element
//! (`irena-core`), published under a namespace per kind — `irena.company.v1`,
//! `irena.shares.v1`, `irena.rules.v1`. Prunella stores and orders it and never looks
//! inside; its XML transport nests it readably inside the block. Every operation here
//! is deterministic over the chain's contents: block order is total and transaction
//! order within a block is fixed, so two instances holding the same chain resolve the
//! same record for any company at any height.
//!
//! # Amendments
//!
//! Each (company, kind) has its own amendment chain. A record that amends another must
//! name the record **currently in force**; amending a version you have not seen is
//! refused as stale before the ledger is touched. Walking a chain checks it link by
//! link, and a break — possible only if something wrote around this crate — is
//! reported, never repaired, and until it is resolved nothing is in force for that
//! (company, kind). The notary's date-time on a record plays no part in any of this:
//! the ledger's order is the only order.
//!
//! # What this crate does not do
//!
//! Vote. `irena-vote` derives an electorate from a resolved share register and runs
//! the lifecycle; this crate only says what the register is.

mod error;
mod ledger;

pub use error::LedgerError;
pub use ledger::{
    CompanyStateV1, InForceV1, RecordRefV1, company_at, genesis_in_force, genesis_with_company,
    history, in_force, publish, rules_in_force, shares_in_force,
};
