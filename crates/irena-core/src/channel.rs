//! Decision channels: who may decide, and how.
//!
//! A channel is the one authority abstraction in Irena. It has an id, an **actor
//! source** (where the people come from) and a **mode** (how one decision is reached):
//!
//! ```text
//! channel      := id + actor source + mode
//! actor source := share-register | roster (members listed inline)
//! mode         := individual | collective(<voting-rules>)
//! ```
//!
//! `shareholders`, `board`, `ceo` and any future committee are *configurations* in a
//! notarised document, not code. Irena knows `share-register`, `roster`, `individual`
//! and `collective`. It does not know what a board is, and does not decide whether the
//! configured authority is legally correct: notarisation is the trust boundary.
//!
//! A channel says nothing about *what* its actors may decide. Scoping a channel to a
//! kind of decision is deliberately not in version 1.
//!
//! The channel set is one part of the company, replaced whole like the share register,
//! so the set at any height is the one record then in force.

use crate::error::{IrenaError, IssueV1};
use bornite_core::VoterIdV1;
use bornite_rules::VotingRulesV1;

/// Maximum length of a channel id, in bytes.
pub const MAX_CHANNEL_ID_LEN: usize = 64;

/// Largest number of members a roster may list.
pub const MAX_MEMBERS: usize = 100_000;

/// Largest number of channels a set may list.
pub const MAX_CHANNELS: usize = 1_000;

/// An opaque label identifying a channel within a company.
///
/// `shareholders`, `board`, `ceo`, `audit-committee` — compared for equality only and
/// never interpreted. Same grammar as a company id: 1–64 bytes, `a-z0-9` first, then
/// `a-z0-9._-`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct ChannelIdV1(String);

impl ChannelIdV1 {
    /// Validates and wraps a channel id.
    ///
    /// # Errors
    ///
    /// Returns [`IrenaError::Invalid`] with one [`IssueV1::InvalidValue`].
    pub fn new(value: impl Into<String>) -> Result<Self, IrenaError> {
        let value = value.into();
        let reject = |reason: &str| {
            IrenaError::invalid(vec![IssueV1::InvalidValue {
                element: "channel",
                attribute: "id",
                value: value.clone(),
                reason: reason.to_owned(),
            }])
        };
        let mut bytes = value.bytes();
        let Some(first) = bytes.next() else {
            return Err(reject("must not be empty"));
        };
        if value.len() > MAX_CHANNEL_ID_LEN {
            return Err(reject("must be at most 64 bytes"));
        }
        if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
            return Err(reject("must start with a lowercase letter or digit"));
        }
        if !bytes.all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        }) {
            return Err(reject(
                "may only contain lowercase letters, digits, '.', '_' and '-'",
            ));
        }
        Ok(Self(value))
    }

    /// The label text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for ChannelIdV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::fmt::Debug for ChannelIdV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl core::str::FromStr for ChannelIdV1 {
    type Err = IrenaError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::new(text)
    }
}

/// One member of a roster.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct MemberV1 {
    /// The member's id, which is also their voter id and their person id: a member
    /// becomes a Bornite voter with no translation, exactly as a shareholder does, and
    /// signs with their identity's key.
    pub id: VoterIdV1,
    /// A display name. Opaque.
    pub name: Option<String>,
    /// Voting weight within the channel. At least one; `1` unless the document says
    /// otherwise.
    pub weight: u64,
}

/// A validated roster: the members a channel lists inline.
///
/// Sorted by member id and free of duplicates from the moment it is built.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct RosterV1 {
    members: Vec<MemberV1>,
}

impl RosterV1 {
    /// Builds a roster, sorting by id and refusing what cannot be one.
    ///
    /// # Errors
    ///
    /// Returns [`IrenaError::Invalid`] listing every problem at once: an empty roster,
    /// duplicate ids, zero weights, a total past `u64::MAX`, too many members.
    /// `channel` names the channel in every issue.
    pub fn new(channel: &ChannelIdV1, mut members: Vec<MemberV1>) -> Result<Self, IrenaError> {
        let mut issues = Vec::new();
        if members.len() > MAX_MEMBERS {
            return Err(IrenaError::invalid(vec![IssueV1::TooManyMembers {
                channel: channel.clone(),
                limit: MAX_MEMBERS,
            }]));
        }
        if members.is_empty() {
            issues.push(IssueV1::EmptyRoster {
                channel: channel.clone(),
            });
        }
        members.sort_by(|left, right| left.id.cmp(&right.id));

        let mut previous: Option<&VoterIdV1> = None;
        let mut total: u64 = 0;
        let mut overflowed = false;
        for member in &members {
            if previous == Some(&member.id) {
                issues.push(IssueV1::DuplicateMember {
                    channel: channel.clone(),
                    id: member.id.clone(),
                });
            }
            previous = Some(&member.id);
            if member.weight == 0 {
                issues.push(IssueV1::ZeroWeight {
                    channel: channel.clone(),
                    id: member.id.clone(),
                });
            }
            match total.checked_add(member.weight) {
                Some(sum) => total = sum,
                None => overflowed = true,
            }
        }
        if overflowed {
            issues.push(IssueV1::TotalWeightOverflow {
                channel: channel.clone(),
                max: u64::MAX,
            });
        }
        if !issues.is_empty() {
            return Err(IrenaError::invalid(issues));
        }
        Ok(Self { members })
    }

    /// The members, in id order.
    #[must_use]
    pub fn members(&self) -> &[MemberV1] {
        &self.members
    }

    /// Finds a member by id.
    #[must_use]
    pub fn get(&self, id: &VoterIdV1) -> Option<&MemberV1> {
        self.members
            .binary_search_by(|member| member.id.cmp(id))
            .ok()
            .map(|index| &self.members[index])
    }

    /// Number of members.
    #[must_use]
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Whether the roster is empty. Never true for a validated roster.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

/// Where a channel's actors come from.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "source", content = "members", rename_all = "kebab-case")]
pub enum ActorSourceV1 {
    /// The company's share register in force at the height a decision is frozen at.
    /// Weight is the holding: one share, one vote.
    ShareRegister,
    /// Members listed in the channel itself, each with a declared weight.
    Roster(RosterV1),
}

impl ActorSourceV1 {
    /// The attribute text.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ShareRegister => "share-register",
            Self::Roster(_) => "roster",
        }
    }
}

/// How a channel reaches a decision.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum ChannelModeV1 {
    /// One actor signs. The source must resolve to exactly one actor at the height a
    /// decision is frozen at; that is checked at resolution, because it can depend on
    /// company state (a share register with one holder is one actor).
    Individual,
    /// The actors form a Bornite electorate and the nested rules decide.
    Collective {
        /// Bornite's rules, unchanged.
        rules: VotingRulesV1,
    },
}

impl ChannelModeV1 {
    /// The attribute text.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Individual => "individual",
            Self::Collective { .. } => "collective",
        }
    }

    /// Whether this is the individual mode.
    #[must_use]
    pub const fn is_individual(&self) -> bool {
        matches!(self, Self::Individual)
    }

    /// The rules, for a collective channel.
    #[must_use]
    pub const fn rules(&self) -> Option<&VotingRulesV1> {
        match self {
            Self::Individual => None,
            Self::Collective { rules } => Some(rules),
        }
    }
}

/// One channel: who may decide, and how.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DecisionChannelV1 {
    /// The channel's label.
    pub id: ChannelIdV1,
    /// Where its actors come from.
    pub actors: ActorSourceV1,
    /// How it decides.
    pub mode: ChannelModeV1,
}

/// A validated channel set: every channel the company decides through.
///
/// Sorted by channel id, non-empty and free of duplicates from the moment it is built.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct DecisionChannelsV1 {
    channels: Vec<DecisionChannelV1>,
}

impl DecisionChannelsV1 {
    /// Builds a channel set, sorting by id and refusing what cannot be one.
    ///
    /// # Errors
    ///
    /// Returns [`IrenaError::Invalid`] listing every problem: no channels, duplicate
    /// ids, too many channels.
    pub fn new(mut channels: Vec<DecisionChannelV1>) -> Result<Self, IrenaError> {
        let mut issues = Vec::new();
        if channels.len() > MAX_CHANNELS {
            return Err(IrenaError::invalid(vec![IssueV1::TooManyChannels {
                limit: MAX_CHANNELS,
            }]));
        }
        if channels.is_empty() {
            issues.push(IssueV1::NoChannels);
        }
        channels.sort_by(|left, right| left.id.cmp(&right.id));
        let mut previous: Option<&ChannelIdV1> = None;
        for channel in &channels {
            if previous == Some(&channel.id) {
                issues.push(IssueV1::DuplicateChannel {
                    id: channel.id.clone(),
                });
            }
            previous = Some(&channel.id);
        }
        if !issues.is_empty() {
            return Err(IrenaError::invalid(issues));
        }
        Ok(Self { channels })
    }

    /// The channels, in id order.
    #[must_use]
    pub fn channels(&self) -> &[DecisionChannelV1] {
        &self.channels
    }

    /// Finds a channel by id.
    #[must_use]
    pub fn get(&self, id: &ChannelIdV1) -> Option<&DecisionChannelV1> {
        self.channels
            .binary_search_by(|channel| channel.id.cmp(id))
            .ok()
            .map(|index| &self.channels[index])
    }

    /// Number of channels. At least one.
    #[must_use]
    pub fn len(&self) -> usize {
        self.channels.len()
    }

    /// Whether the set is empty. Never true for a validated set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.channels.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str, weight: u64) -> MemberV1 {
        MemberV1 {
            id: VoterIdV1::new(id).expect("id"),
            name: None,
            weight,
        }
    }

    fn id(text: &str) -> ChannelIdV1 {
        ChannelIdV1::new(text).expect("channel id")
    }

    fn rules() -> VotingRulesV1 {
        bornite_xml::read_rules_document(
            r#"<voting-rules version="1.0">
  <weight type="electorate"/>
  <exclusions enabled="true"/>
  <quorum type="fraction" numerator="1" denominator="2" basis="total-electorate"/>
  <threshold type="simple-majority" basis="votes-cast"/>
  <abstentions treatment="exclude"/>
  <tie treatment="reject"/>
</voting-rules>"#,
        )
        .expect("rules")
    }

    #[test]
    fn channel_ids_follow_the_label_grammar() {
        for good in ["shareholders", "board", "ceo", "audit-committee", "c.1"] {
            assert!(ChannelIdV1::new(good).is_ok(), "{good}");
        }
        for bad in ["", "Board", "-x", "a b", &"x".repeat(65)] {
            let error = ChannelIdV1::new(bad).expect_err(bad);
            assert!(
                matches!(
                    error.issues(),
                    [IssueV1::InvalidValue {
                        element: "channel",
                        attribute: "id",
                        ..
                    }]
                ),
                "{bad}: {error}"
            );
        }
    }

    #[test]
    fn a_roster_is_sorted_and_looked_up_by_id() {
        let roster = RosterV1::new(
            &id("board"),
            vec![member("vance", 1), member("chen", 2), member("okafor", 1)],
        )
        .expect("valid");
        let ids: Vec<&str> = roster.members().iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["chen", "okafor", "vance"]);
        assert_eq!(
            roster.get(&VoterIdV1::new("chen").unwrap()).unwrap().weight,
            2
        );
        assert!(roster.get(&VoterIdV1::new("dave").unwrap()).is_none());
        assert_eq!(roster.len(), 3);
    }

    #[test]
    fn every_roster_problem_is_reported_together() {
        let error = RosterV1::new(
            &id("board"),
            vec![
                member("a", 0),
                member("a", 5),
                member("b", u64::MAX),
                member("c", 1),
            ],
        )
        .expect_err("many problems");
        let issues = error.issues();
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, IssueV1::DuplicateMember { .. }))
        );
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, IssueV1::ZeroWeight { .. }))
        );
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, IssueV1::TotalWeightOverflow { .. }))
        );
        assert_eq!(issues.len(), 3, "{error}");

        let empty = RosterV1::new(&id("ceo"), Vec::new()).expect_err("empty");
        assert!(matches!(empty.issues(), [IssueV1::EmptyRoster { .. }]));
    }

    #[test]
    fn a_channel_set_is_sorted_non_empty_and_unique() {
        let ceo = DecisionChannelV1 {
            id: id("ceo"),
            actors: ActorSourceV1::Roster(
                RosterV1::new(&id("ceo"), vec![member("chen", 1)]).unwrap(),
            ),
            mode: ChannelModeV1::Individual,
        };
        let shareholders = DecisionChannelV1 {
            id: id("shareholders"),
            actors: ActorSourceV1::ShareRegister,
            mode: ChannelModeV1::Collective { rules: rules() },
        };
        let set = DecisionChannelsV1::new(vec![shareholders.clone(), ceo.clone()]).expect("set");
        assert_eq!(set.channels()[0].id, id("ceo"));
        assert_eq!(set.get(&id("shareholders")), Some(&shareholders));
        assert!(set.get(&id("board")).is_none());
        assert!(set.channels()[0].mode.is_individual());
        assert!(set.channels()[1].mode.rules().is_some());

        let none = DecisionChannelsV1::new(Vec::new()).expect_err("none");
        assert!(matches!(none.issues(), [IssueV1::NoChannels]));
        let twice = DecisionChannelsV1::new(vec![ceo.clone(), ceo]).expect_err("twice");
        assert!(matches!(twice.issues(), [IssueV1::DuplicateChannel { .. }]));
    }
}
