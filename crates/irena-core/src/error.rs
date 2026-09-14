//! Why a document or a value was refused.

use bornite_core::VoterIdV1;

/// One thing wrong with a company document's content.
///
/// Issues are collected across a whole document and reported together, sorted, so a
/// document with three problems is fixed in one round and two readers report the same
/// list in the same order.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(tag = "issue", rename_all = "snake_case")]
#[non_exhaustive]
pub enum IssueV1 {
    /// A problem Bornite's reader found in an embedded element or attribute.
    #[error(transparent)]
    Xml(bornite_xml::XmlIssueV1),
    /// An attribute holds a value outside its allowed form.
    #[error("<{element} {attribute}=\"{value}\"> is not valid: {reason}")]
    InvalidValue {
        /// The element.
        element: &'static str,
        /// The attribute.
        attribute: &'static str,
        /// The value found.
        value: String,
        /// What was expected.
        reason: String,
    },
    /// A required attribute is absent.
    #[error("<{element}> is missing the {attribute} attribute")]
    MissingAttribute {
        /// The element.
        element: &'static str,
        /// The attribute.
        attribute: &'static str,
    },
    /// A required child element is absent.
    #[error("<{parent}> is missing its <{element}> child")]
    MissingElement {
        /// The parent.
        parent: &'static str,
        /// The missing child.
        element: &'static str,
    },
    /// A child element appears more than once where one is allowed.
    #[error("<{parent}> has more than one <{element}> child")]
    RepeatedElement {
        /// The parent.
        parent: &'static str,
        /// The repeated child.
        element: &'static str,
    },
    /// Two holders share an id.
    #[error("holder {id} is declared more than once")]
    DuplicateHolder {
        /// The repeated id.
        id: VoterIdV1,
    },
    /// Two persons share a signing key.
    ///
    /// One key for two persons would let one signature be two people's.
    #[error("{first} and {second} declare the same signing key")]
    DuplicateKey {
        /// The person listed first, in id order.
        first: VoterIdV1,
        /// The other.
        second: VoterIdV1,
    },
    /// Two persons share an id.
    #[error("person {id} is declared more than once")]
    DuplicatePerson {
        /// The repeated id.
        id: VoterIdV1,
    },
    /// An identities record lists more persons than the reader accepts.
    #[error("identities list more than {limit} persons")]
    TooManyPersons {
        /// The limit.
        limit: usize,
    },
    /// The same signer row appears twice.
    #[error("{person} is listed twice as a {family} signer")]
    DuplicateSigner {
        /// The person.
        person: VoterIdV1,
        /// The family.
        family: crate::RecordFamilyV1,
    },
    /// Nobody may sign company amendments.
    #[error("no person may sign company records; the company could never be amended")]
    NoCompanySigner,
    /// A holder holds nothing.
    #[error("holder {id} holds zero shares; a holder of nothing is a mistake, not a member")]
    ZeroShares {
        /// The holder.
        id: VoterIdV1,
    },
    /// The shares add up past what a weight total can hold.
    #[error("total shares exceed {max}")]
    TotalSharesOverflow {
        /// The largest representable total.
        max: u64,
    },
    /// A share structure lists more holders than the reader accepts.
    #[error("share structure lists more than {limit} holders")]
    TooManyHolders {
        /// The limit.
        limit: usize,
    },
    /// A company declares no decision channel.
    #[error("the company declares no decision channel; nothing could ever decide anything")]
    NoChannels,
    /// Two channels share an id.
    #[error("channel {id} is declared more than once")]
    DuplicateChannel {
        /// The repeated id.
        id: crate::ChannelIdV1,
    },
    /// A channel set lists more channels than the reader accepts.
    #[error("the channel set lists more than {limit} channels")]
    TooManyChannels {
        /// The limit.
        limit: usize,
    },
    /// A roster lists nobody.
    #[error("channel {channel} has an empty roster; a channel with no actors cannot decide")]
    EmptyRoster {
        /// The channel.
        channel: crate::ChannelIdV1,
    },
    /// Two members of one roster share an id.
    #[error("channel {channel} lists member {id} more than once")]
    DuplicateMember {
        /// The channel.
        channel: crate::ChannelIdV1,
        /// The repeated id.
        id: VoterIdV1,
    },
    /// A member carries no weight.
    #[error("channel {channel}: member {id} has zero weight; a member of nothing is a mistake")]
    ZeroWeight {
        /// The channel.
        channel: crate::ChannelIdV1,
        /// The member.
        id: VoterIdV1,
    },
    /// The weights of a roster add up past what a weight total can hold.
    #[error("channel {channel}: total weight exceeds {max}")]
    TotalWeightOverflow {
        /// The channel.
        channel: crate::ChannelIdV1,
        /// The largest representable total.
        max: u64,
    },
    /// A roster lists more members than the reader accepts.
    #[error("channel {channel} lists more than {limit} members")]
    TooManyMembers {
        /// The channel.
        channel: crate::ChannelIdV1,
        /// The limit.
        limit: usize,
    },
    /// A channel carries an element its mode does not allow.
    #[error("channel {channel} is {mode} and must not carry <{element}>")]
    UnexpectedElement {
        /// The channel.
        channel: crate::ChannelIdV1,
        /// The channel's mode.
        mode: &'static str,
        /// The element found.
        element: &'static str,
    },
}

/// Why a record or document was refused.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "error", rename_all = "snake_case")]
#[non_exhaustive]
pub enum IrenaError {
    /// The document is not well-formed, or has an element where none is allowed.
    ///
    /// Structural problems stop parsing where they are found, because nothing after an
    /// unexpected element can be trusted to mean what it appears to.
    #[error("malformed document at byte {position}: {detail}")]
    Malformed {
        /// Byte offset where parsing stopped.
        position: u64,
        /// What was wrong.
        detail: String,
    },
    /// The document's content is wrong in one or more places.
    #[error("{} issue(s): {}", .issues.len(), render(.issues))]
    Invalid {
        /// Every issue, sorted.
        issues: Vec<IssueV1>,
    },
    /// The document declares a version this build does not read.
    #[error("record version {found:?} is not supported; this build reads \"1.0\"")]
    UnsupportedVersion {
        /// The version found.
        found: String,
    },
    /// The record's `kind` does not match the element it carries.
    #[error("record declares kind {declared} but carries a {carried} element")]
    KindMismatch {
        /// The `kind` attribute.
        declared: crate::RecordKindV1,
        /// What the body actually is.
        carried: crate::RecordKindV1,
    },
    /// The input is larger than the reader will accept.
    #[error("document is {found} bytes, over the {limit} byte limit")]
    TooLarge {
        /// Bytes supplied.
        found: u64,
        /// The limit.
        limit: u64,
    },
}

impl IrenaError {
    /// Wraps a non-empty issue list, sorted and deduplicated.
    ///
    /// Sorting makes the report independent of the order problems were noticed in.
    #[must_use]
    pub fn invalid(mut issues: Vec<IssueV1>) -> Self {
        issues.sort();
        issues.dedup();
        Self::Invalid { issues }
    }

    /// The issues, if this is [`IrenaError::Invalid`].
    #[must_use]
    pub fn issues(&self) -> &[IssueV1] {
        match self {
            Self::Invalid { issues } => issues,
            _ => &[],
        }
    }
}

impl From<bornite_xml::XmlError> for IrenaError {
    fn from(error: bornite_xml::XmlError) -> Self {
        match error {
            bornite_xml::XmlError::Malformed { position, detail } => {
                Self::Malformed { position, detail }
            }
            bornite_xml::XmlError::Invalid { issues } => {
                Self::invalid(issues.into_iter().map(IssueV1::Xml).collect())
            }
            bornite_xml::XmlError::UnsupportedVersion { found } => {
                Self::UnsupportedVersion { found }
            }
            bornite_xml::XmlError::TooLarge { found, limit } => Self::TooLarge { found, limit },
            other => Self::Malformed {
                position: 0,
                detail: other.to_string(),
            },
        }
    }
}

fn render(issues: &[IssueV1]) -> String {
    issues
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}
