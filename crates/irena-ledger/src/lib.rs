//! Irena records on a Prunella ledger.
//!
//! This crate puts [`irena_core`] records on a chain and answers one question about
//! it: **what is the company at height `h`?** Its identity, its share register and its
//! voting rules, each with the transaction that currently provides it. That is what
//! "Irena knows what the rules are now" means concretely.
//!
//! # How the company sits on the chain
//!
//! **One company per chain.** The company's genesis — the whole company: identity,
//! initial register, initial rules — is one Prunella transaction whose payload is one
//! `<irena-record>` element, normally in block 0. Every later change is another such
//! transaction amending exactly one part, published under a namespace per part
//! (`irena.company.v1`, `irena.shares.v1`, `irena.rules.v1`) and naming the transaction
//! it supersedes. Prunella stores and orders them and never looks inside; its XML
//! transport nests them readably inside the block.
//!
//! # Reconstruction, not mutation
//!
//! Nothing is ever edited. [`reconstruct`] reads the genesis and applies every
//! amendment in chain order — block order is total and transaction order within a
//! block is fixed — so two instances holding the same chain arrive at the same company
//! at every height, and a past height never changes because of anything appended
//! since. An amendment that does not supersede the transaction currently providing
//! its part, a second genesis, a record for another company, or an unreadable record
//! in an Irena namespace can only have been written around this crate; each stops
//! reconstruction with an error naming the exact transaction, and is never repaired.
//! The notary's date-time on a record plays no part in any of this: the ledger's order
//! is the only order.
//!
//! # What this crate does not do
//!
//! Vote. `irena-vote` derives an electorate from a reconstructed register and runs the
//! lifecycle; this crate only says what the register is.

mod error;
mod ledger;
mod state;

pub use error::LedgerError;
pub use ledger::{company_now, genesis_with_company, history, publish};
pub use state::{CompanyStateV1, InForceV1, RecordRefV1, reconstruct};
