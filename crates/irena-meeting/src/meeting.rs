//! What a meeting is: its id, metadata, agenda and status.

use crate::error::MeetingError;
use borsh::{BorshDeserialize, BorshSerialize};
use irena_core::{ChannelIdV1, NotaryTimeV1};
use prunella_core::{Hash, TxId};

/// Identifies a meeting: the transaction that convened it.
///
/// A convening transaction is unique and immutable once on the chain, so nothing else
/// is needed to name a meeting, and nothing can claim to be a meeting that was never
/// convened.
#[derive(
    BorshSerialize, BorshDeserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, std::hash::Hash,
)]
pub struct MeetingIdV1(TxId);

impl MeetingIdV1 {
    /// Wraps a convening transaction id.
    #[must_use]
    pub const fn from_tx(tx_id: TxId) -> Self {
        Self(tx_id)
    }

    /// The convening transaction.
    #[must_use]
    pub const fn tx_id(self) -> TxId {
        self.0
    }
}

impl core::fmt::Display for MeetingIdV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

impl core::fmt::Debug for MeetingIdV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

impl serde::Serialize for MeetingIdV1 {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&self.0, serializer)
    }
}

/// Where a meeting is in its life.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingStatusV1 {
    /// Agenda being assembled; nothing on the chain.
    Draft,
    /// The convening record is on the chain.
    Convened,
    /// Every vote item is frozen and accepting ballots.
    Open,
    /// Every vote is closed and counted; nothing more on the chain yet.
    Closed,
    /// Every vote and the final record are on the chain.
    Finalized,
}

impl core::fmt::Display for MeetingStatusV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Draft => "draft",
            Self::Convened => "convened",
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Finalized => "finalized",
        })
    }
}

/// What a meeting is about, apart from its agenda: whose meeting it is, what it is
/// called, and when.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct MeetingMetadataV1 {
    /// The decision channel this is a meeting of. A meeting is a meeting *of* one
    /// collective channel — the shareholders, the board, a committee — and every vote
    /// item freezes against that channel. A board meeting and a shareholders' meeting
    /// are the same code with a different id here.
    pub channel: String,
    /// The meeting's title. Non-empty, opaque.
    pub title: String,
    /// When the meeting is scheduled to be held, in the canonical notary form. Attested
    /// metadata: never used to order or gate anything.
    pub scheduled_at: String,
    /// Digest of the notice of meeting sent to shareholders, if one is referenced.
    pub notice_digest: Option<Hash>,
}

impl MeetingMetadataV1 {
    /// The channel id, validated.
    ///
    /// # Errors
    ///
    /// [`MeetingError::InvalidAgenda`] if the label is not a channel id.
    pub fn channel(&self) -> Result<ChannelIdV1, MeetingError> {
        ChannelIdV1::new(self.channel.clone()).map_err(|error| MeetingError::InvalidAgenda {
            detail: format!("channel: {error}"),
        })
    }

    /// Validates the channel id, the title and the time.
    ///
    /// # Errors
    ///
    /// [`MeetingError::InvalidAgenda`] naming what is wrong.
    pub fn validate(&self) -> Result<(), MeetingError> {
        self.channel()?;
        if self.title.trim().is_empty() {
            return Err(MeetingError::InvalidAgenda {
                detail: "the meeting title must not be empty".to_owned(),
            });
        }
        NotaryTimeV1::parse(&self.scheduled_at).map_err(|error| MeetingError::InvalidAgenda {
            detail: format!("scheduled-at: {error}"),
        })?;
        Ok(())
    }
}

/// What an agenda item is.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgendaBodyV1 {
    /// Something shareholders are shown, identified by the digest of the document.
    Informational {
        /// Digest of the document presented. Never interpreted.
        document_digest: Hash,
    },
    /// Something shareholders decide, identified by the digest of the proposal.
    Vote {
        /// Digest of the proposal document. Never interpreted.
        proposal_digest: Hash,
    },
}

impl AgendaBodyV1 {
    /// The attribute text of this kind.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Informational { .. } => "informational",
            Self::Vote { .. } => "vote",
        }
    }

    /// The digest the item carries, whichever kind it is.
    #[must_use]
    pub const fn digest(&self) -> Hash {
        match self {
            Self::Informational { document_digest } => *document_digest,
            Self::Vote { proposal_digest } => *proposal_digest,
        }
    }

    /// Whether this item is decided by a vote.
    #[must_use]
    pub const fn is_vote(&self) -> bool {
        matches!(self, Self::Vote { .. })
    }
}

/// One item on the agenda.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct AgendaItemV1 {
    /// Position on the agenda, from 1.
    pub number: u32,
    /// The item's title. For a vote item this is also the vote's subject.
    pub title: String,
    /// What the item is.
    pub body: AgendaBodyV1,
}

/// A validated agenda: at least one item, numbered 1..=n in order, titles non-empty.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct AgendaV1 {
    items: Vec<AgendaItemV1>,
}

impl AgendaV1 {
    /// Validates an agenda.
    ///
    /// # Errors
    ///
    /// [`MeetingError::InvalidAgenda`] naming the first problem.
    pub fn new(items: Vec<AgendaItemV1>) -> Result<Self, MeetingError> {
        if items.is_empty() {
            return Err(MeetingError::InvalidAgenda {
                detail: "an agenda needs at least one item".to_owned(),
            });
        }
        for (index, item) in items.iter().enumerate() {
            let expected = u32::try_from(index + 1).map_err(|_| MeetingError::InvalidAgenda {
                detail: "too many items".to_owned(),
            })?;
            if item.number != expected {
                return Err(MeetingError::InvalidAgenda {
                    detail: format!(
                        "items must be numbered from 1 in order: expected {expected}, found {}",
                        item.number
                    ),
                });
            }
            if item.title.trim().is_empty() {
                return Err(MeetingError::InvalidAgenda {
                    detail: format!("item {expected} has no title"),
                });
            }
        }
        Ok(Self { items })
    }

    /// The items, in order.
    #[must_use]
    pub fn items(&self) -> &[AgendaItemV1] {
        &self.items
    }

    /// The item with this number.
    #[must_use]
    pub fn item(&self, number: u32) -> Option<&AgendaItemV1> {
        usize::try_from(number)
            .ok()
            .and_then(|n| n.checked_sub(1))
            .and_then(|index| self.items.get(index))
    }

    /// Number of items.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Never true: an agenda has at least one item.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The vote items, in order.
    pub fn vote_items(&self) -> impl Iterator<Item = &AgendaItemV1> {
        self.items.iter().filter(|item| item.body.is_vote())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(number: u32, title: &str, vote: bool) -> AgendaItemV1 {
        AgendaItemV1 {
            number,
            title: title.to_owned(),
            body: if vote {
                AgendaBodyV1::Vote {
                    proposal_digest: Hash::from_bytes([number as u8; 32]),
                }
            } else {
                AgendaBodyV1::Informational {
                    document_digest: Hash::from_bytes([number as u8; 32]),
                }
            },
        }
    }

    #[test]
    fn an_agenda_is_numbered_from_one_in_order() {
        let agenda = AgendaV1::new(vec![item(1, "a", false), item(2, "b", true)]).expect("ok");
        assert_eq!(agenda.len(), 2);
        assert_eq!(agenda.item(2).unwrap().title, "b");
        assert!(agenda.item(0).is_none());
        assert!(agenda.item(3).is_none());
        assert_eq!(agenda.vote_items().count(), 1);
        for bad in [
            vec![],
            vec![item(2, "a", false)],
            vec![item(1, "a", false), item(3, "b", false)],
            vec![item(1, "a", false), item(1, "b", false)],
            vec![item(1, "  ", false)],
        ] {
            assert!(matches!(
                AgendaV1::new(bad),
                Err(MeetingError::InvalidAgenda { .. })
            ));
        }
    }

    #[test]
    fn metadata_is_checked() {
        let good = MeetingMetadataV1 {
            channel: "shareholders".to_owned(),
            title: "AGM".to_owned(),
            scheduled_at: "2026-06-01T10:00:00Z".to_owned(),
            notice_digest: None,
        };
        assert!(good.validate().is_ok());
        let mut bad = good.clone();
        bad.title = " ".to_owned();
        assert!(bad.validate().is_err());
        let mut bad = good.clone();
        bad.channel = "Shareholders".to_owned();
        assert!(bad.validate().is_err());
        let mut bad = good;
        bad.scheduled_at = "tomorrow".to_owned();
        assert!(bad.validate().is_err());
    }
}
