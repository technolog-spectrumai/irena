//! Decision channels, resolved: who may decide, resolved into people; and the
//! individual decision, taken by one of them.
//!
//! `irena-core` defines a [`irena_core::DecisionChannelV1`] as data: an id, an actor
//! source and a mode. This crate is where that data meets the company. Given the
//! company reconstructed at a height, [`resolve_channel`] turns a channel into an
//! [`ActorSetV1`] — ids, integer weights, signing keys — and says whether it decides
//! by one signature or by a vote:
//!
//! * **Individual** — the source must resolve to exactly one actor, and that actor's
//!   one signature over a frozen snapshot is the whole decision: [`DecisionV1`], here.
//! * **Collective** — the actors form a Bornite electorate under the channel's nested
//!   rules, and a vote decides: `irena-vote`, which asks this crate for the electorate.
//!
//! One `match` on the actor source is the whole company/mathematics boundary. Bornite
//! receives ids and weights and never learns whether a weight came from a shareholding
//! or a seat; nothing here knows what a board is.
//!
//! # The individual decision
//!
//! ```text
//! Draft ──freeze──▶ Frozen ──sign──▶ Signed ──finalize──▶ Finalized
//! ```
//!
//! **freeze** resolves the channel at a height, requires it to be individual, and pins
//! the founding record, the register, the channel set, the actor and their key in a
//! [`DecisionSnapshotV1`] whose digest is the decision id. **sign** takes the actor's
//! key and refuses any other. **finalize** writes a [`FinalDecisionRecordV1`] under
//! `irena.decision.v1`. [`verify_decision`] re-establishes it from the chain alone:
//! the pinned records re-resolve, the channel is still individual there, it resolves
//! to that actor with that key, and the signature verifies.
//!
//! # What this crate does not decide
//!
//! Whether the configured authority is legally correct. A channel that names one
//! person "ceo" makes that person able to sign decisions through it; whether they
//! should be able to is what the notarisation on the channel-set record attests.

mod decision;
mod error;
mod record;
mod resolve;
mod verify;

pub use decision::{
    DECISION_ID_TAG, DECISION_SIGN_TAG, DecisionIdV1, DecisionSnapshotV1, DecisionStatusV1,
    DecisionV1,
};
pub use error::DecisionError;
pub use record::{
    DECISION_NAMESPACE, DECISION_SCHEMA_VERSION, FinalDecisionRecordV1, FinalizedDecisionV1,
    RECORD_VERSION,
};
pub use resolve::{
    ActorSetV1, ActorV1, ResolvedChannelV1, actors_of, actors_of_register, actors_of_roster,
    resolve_channel,
};
pub use verify::{DecisionCheckNameV1, DecisionCheckV1, DecisionVerificationV1, verify_decision};
