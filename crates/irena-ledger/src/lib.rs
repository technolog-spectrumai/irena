//! Irena records on a Prunella ledger.
//!
//! This crate puts [`irena_core`] records on a chain and answers one question about
//! it: **what is the company at height `h`?** Its identity, its share register, its
//! decision channels, its identities and its authorisation, each with the transaction
//! that currently provides it. That is what "Irena knows who decides now" means
//! concretely.
//!
//! # How the company sits on the chain
//!
//! **One company per chain.** The company's genesis — the whole company: identity,
//! initial register, initial channels — is one Prunella transaction whose payload is one
//! `<irena-record>` element, normally in block 0. Every later change is another such
//! transaction amending exactly one part, published under a namespace per part
//! (`irena.company.v1`, `irena.shares.v1`, `irena.channels.v1`, `irena.identities.v1`,
//! `irena.authorisation.v1`) and naming the transaction it supersedes. Prunella stores
//! and orders them and never looks inside; its XML transport nests them readably
//! inside the block.
//!
//! # Who may write
//!
//! The transaction that carries a record is signed by a key, and the company itself
//! says whose key it may be: the `<identities>` in force name the person who holds
//! it, and the `<authorisation>` in force says whether that person may sign `company`
//! records. Both are taken from the company **as it was before the record**.
//! [`publish`] refuses an unauthorised key before writing; [`reconstruct`] stops at a
//! company record an unauthorised key signed, exactly as it stops at a broken
//! amendment link. The genesis signer is whoever founded the chain: the authorisation
//! inside the genesis applies from the next record on. And no record may leave the
//! company without a `company` signer holding a key — the lockout rule — refused at
//! publish and a break at reconstruction. Bare publishing is real power: a `company`
//! signer rewrites the register with no channel deciding anything. That is the
//! notary's route by design, and the authorisation is exactly who may take it.
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
//! Decide. `irena-decision` resolves a channel of a reconstructed company into actors
//! and `irena-vote` runs the lifecycle; this crate only says what the company is.

mod authority;
mod error;
mod ledger;
mod state;

pub use authority::{authorised_signer, lockout_after, signer_of};
pub use error::LedgerError;
pub use ledger::{company_now, genesis_with_company, history, publish};
pub use state::{CompanyStateV1, InForceV1, RecordRefV1, reconstruct};
