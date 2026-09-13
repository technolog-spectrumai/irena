//! Meetings of a decision channel.
//!
//! A meeting is a company-level container for agenda items and the votes among them,
//! held by one **collective decision channel**: the shareholders, the board, a
//! committee — whichever channel the company configured. It adds no arithmetic and no
//! new kind of company truth: Bornite still counts, Prunella still stores,
//! `irena-vote` still runs every vote through the channel, and `irena-ledger` still
//! says what the company is. What a meeting adds is *grouping and formality*: which
//! items were put before the channel's actors, when, by whom, and which final vote
//! records answered them. A board meeting and a shareholders' meeting are the same
//! code with a different channel id.
//!
//! # Lifecycle
//!
//! ```text
//! Draft ──convene──▶ Convened ──open──▶ Open ──close──▶ Closed ──finalize──▶ Finalized
//! ```
//!
//! * **convene** writes the *convened* record to the chain — channel, title, scheduled
//!   time, the full agenda, notarisation. Its transaction id is the [`MeetingIdV1`].
//! * **open** creates one `irena-vote` vote per vote item through the meeting's
//!   channel, each freezing the company at the current head on its own: its own
//!   snapshot, its own id. Company amendments after that height reach none of them.
//!   A meeting of an individual channel cannot open: one person does not hold a vote.
//! * **cast** routes a ballot to the vote of one item.
//! * **close** closes and evaluates every vote.
//! * **finalize** finalises every vote to the chain (each its own transaction, resumed
//!   if interrupted), then writes the *final* record: meeting id, metadata, the agenda
//!   with each vote item's final vote transaction, every document digest, notarisation.
//!
//! Only those two records reach the chain. Everything between them is the meeting's
//! local state, canonical Borsh, carried in a file between steps.
//!
//! # Verification
//!
//! [`verify_meeting`] re-establishes a final record from the chain alone: the
//! convening record exists and agrees with it, the company reconstructs at the
//! convening height, and every referenced vote verifies through `irena_vote::verify`
//! *and* is the vote this item, this meeting, this channel and this company called
//! for. A missing, foreign or tampered vote reference is a named failing check.

mod error;
mod lifecycle;
mod meeting;
mod record;
mod verify;

pub use error::MeetingError;
pub use lifecycle::{MeetingFinalizedV1, MeetingV1};
pub use meeting::{
    AgendaBodyV1, AgendaItemV1, AgendaV1, MeetingIdV1, MeetingMetadataV1, MeetingStatusV1,
};
pub use record::{
    FinalItemV1, MEETING_NAMESPACE, MEETING_SCHEMA_VERSION, MEETING_VERSION, MeetingFinalRecordV1,
    MeetingRecordBodyV1, MeetingRecordV1, compose_convened, compose_final, read_meeting_record,
};
pub use verify::{MeetingCheckNameV1, MeetingCheckV1, MeetingVerificationV1, verify_meeting};
