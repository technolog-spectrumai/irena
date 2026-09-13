//! Resolutions: the step that closes the governance loop.
//!
//! ```text
//! company state → meeting → vote → passed result → resolution → amendment → new company state
//! ```
//!
//! Every arrow is a separate record on the chain, and the separation is the point:
//!
//! * A **vote** never changes the company. It counts ballots against a frozen
//!   electorate and says what the shareholders decided (`irena-vote`).
//! * A **resolution** never changes the company either. It is the formal record that a
//!   decision *was* taken, pinned to the exact meeting agenda item and the exact
//!   finalised vote that authorised it. It describes **authority**, nothing more.
//! * An **amendment** is what actually changes reconstructed company state, and it is
//!   the same ordinary `irena-ledger` record it has always been (`irena-core`
//!   §`RecordKindV1`). An execution record links the two, so the chain says which
//!   resolution authorised which amendment.
//!
//! # Two kinds, and only two
//!
//! * [`ResolutionKindV1::Declarative`] — a formal decision that changes no
//!   reconstructed state. It names the document the shareholders voted on by digest.
//! * [`ResolutionKindV1::Amendment`] — authorises exactly one company amendment,
//!   replacing the share register or the voting rules. The resolution **carries the
//!   amendment body**, and the agenda item's proposal digest is the digest of exactly
//!   those bytes ([`proposal_digest`]), so what is executed is provably what was
//!   approved.
//!
//! # Nothing is pinned to mutable state
//!
//! A resolution names its meeting, its agenda item and its vote by transaction id. An
//! execution names its resolution and its amendment by transaction id. Heights are
//! recorded only to be checked for order; no timestamp decides anything, and no
//! decision is taken against "the current company" — the company an execution replaces
//! must still be the one the voters saw (§[`ResolutionV1::execute`]).

mod demotion;
mod error;
mod lifecycle;
mod record;
mod resolution;
mod verify;

pub use demotion::{SelfDemotionV1, self_demotion};
pub use error::ResolutionError;
pub use lifecycle::{ExecutedV1, ResolutionV1};
pub use record::{
    EXECUTION_NAMESPACE, EXECUTION_SCHEMA_VERSION, ExecutionRecordV1, RESOLUTION_NAMESPACE,
    RESOLUTION_SCHEMA_VERSION, RESOLUTION_VERSION, ResolutionExecutionV1, ResolutionRecordV1,
    compose_execution, compose_resolution, read_execution_record, read_resolution_record,
};
pub use resolution::{
    AmendmentTargetV1, ApprovalV1, AuthorityV1, PROPOSAL_TAG, ResolutionIdV1, ResolutionKindV1,
    ResolutionStatusV1, proposal_digest,
};
pub use verify::{
    ExecutionCheckNameV1, ExecutionVerificationV1, ResolutionCheckNameV1, ResolutionCheckV1,
    ResolutionVerificationV1, verify_execution, verify_resolution,
};
