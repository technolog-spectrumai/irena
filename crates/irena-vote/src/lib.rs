//! A company vote on the ledger.
//!
//! This crate connects the company (`irena-ledger`) to the arithmetic (`bornite-eval`)
//! and records the outcome where it cannot be changed (`prunella-store`). It owns the
//! idea of a vote as a *process*; Bornite still only counts, Prunella still only
//! stores, and neither learns anything from this crate.
//!
//! # The shape of a vote
//!
//! ```text
//! Draft ──freeze──▶ Frozen ──open──▶ Open ──close──▶ Closed ──evaluate──▶ Evaluated ──finalize──▶ Finalized
//! ```
//!
//! * **freeze** resolves the company at one height — share register and voting rules,
//!   each pinned by the transaction id of the record in force — derives the electorate
//!   from the register, and records all of it in an immutable [`VoteSnapshotV1`]. The
//!   vote's id is the digest of that snapshot. Amendments to the register or the rules
//!   after this height are irrelevant to this vote for ever after.
//! * **cast** accepts a ballot only while the vote is open, and only from a holder in
//!   the frozen electorate who registered a signing key, with a signature that
//!   verifies over the ballot's canonical bytes. One ballot per holder.
//! * **evaluate** hands the frozen electorate and the ballots to `bornite_eval` and
//!   keeps what comes back.
//! * **finalize** writes a [`FinalVoteRecordV1`] — snapshot, every accepted ballot, a
//!   Merkle commitment over them, and the result — to the chain under `irena.vote.v1`.
//!
//! # Where Irena earns its place
//!
//! [`derive_electorate`] is the whole company/mathematics boundary in one function:
//! it turns holders and share counts into voter ids and integer weights, and Bornite
//! never learns that a share exists. With flat shares, `weight = shares`.
//!
//! # Verification
//!
//! [`verify`] re-establishes a final record from nothing but the chain and a
//! transaction id: the snapshot's records re-resolve at its height, the electorate
//! re-derives, every ballot's signature verifies against the register, the commitment
//! re-derives, and Bornite reruns to an identical result. Every check is named and
//! every finding reported; a record that claims a register that was never in force at
//! its own declared height fails however well-formed it is.
//!
//! Ballots are not secret: they are in the record, which is what makes it verifiable
//! from the chain alone.

mod ballot;
mod derive;
mod error;
mod lifecycle;
mod record;
mod snapshot;
mod verify;

pub use ballot::{
    BALLOT_SIGN_TAG, BallotBodyV1, BallotChoiceV1, COMMITMENT_TAGS, SignedBallotV1,
    ballot_commitment,
};
pub use derive::{DerivedHolderV1, ElectorateDerivationV1, derive_electorate};
pub use error::{BallotRejectionV1, VoteError};
pub use lifecycle::{FinalizedV1, VoteStatusV1, VoteV1};
pub use record::{
    EvaluationSummaryV1, FinalVoteRecordV1, RECORD_VERSION, VOTE_NAMESPACE, VOTE_SCHEMA_VERSION,
};
pub use snapshot::{ElectorateEntryV1, VOTE_ID_TAG, VoteIdV1, VoteSnapshotV1};
pub use verify::{CheckNameV1, CheckV1, VerificationV1, verify};
