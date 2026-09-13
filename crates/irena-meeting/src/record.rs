//! The two meeting records on the chain, and their XML.
//!
//! Both are notarised XML in the style of the company records: readable inside the
//! block, the `<notarisation>` element written and read by `irena-core`'s own code.
//! Composition reads the document back before returning it, so what the ledger
//! receives is known to parse to exactly what was asked for.

use crate::error::MeetingError;
use crate::meeting::{AgendaBodyV1, AgendaItemV1, AgendaV1, MeetingIdV1, MeetingMetadataV1};
use bornite_xml::{
    Attributes, XmlReader, describe, expect_empty, expect_eof, malformed, next_event, open,
    root_start,
};
use irena_core::{
    CompanyIdV1, IrenaError, IssueV1, NotarisationV1, NotaryTimeV1, escape_attribute,
    read_notarisation, write_notarisation,
};
use prunella_core::{BlockHeight, Hash, TxId};
use quick_xml::events::{BytesStart, Event};

/// The Prunella namespace both meeting records are published under.
pub const MEETING_NAMESPACE: &str = "irena.meeting.v1";

/// The Prunella schema version a meeting transaction declares.
pub const MEETING_SCHEMA_VERSION: u32 = 1;

/// The only meeting record version this build reads and writes.
pub const MEETING_VERSION: &str = "1.0";

/// Default limit on document size, in bytes.
const MAX_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;

/// One agenda item as the final record reports it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct FinalItemV1 {
    /// The item, exactly as convened.
    pub item: AgendaItemV1,
    /// For a vote item, the transaction carrying its final vote record.
    pub vote_tx_id: Option<TxId>,
    /// For a vote item, the outcome as the final vote record states it.
    pub outcome: Option<String>,
}

/// What a finalised meeting leaves on the chain.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct MeetingFinalRecordV1 {
    /// The meeting: its convening transaction.
    pub meeting_id: MeetingIdV1,
    /// Channel, title, time, notice — repeated from the convening so the record is
    /// self-contained; verification checks they agree.
    pub metadata: MeetingMetadataV1,
    /// The height every vote item was frozen at.
    pub opened_at_height: BlockHeight,
    /// Every item, with each vote item's final vote transaction.
    pub items: Vec<FinalItemV1>,
}

impl MeetingFinalRecordV1 {
    /// The agenda as the final record carries it.
    ///
    /// # Errors
    ///
    /// [`MeetingError::InvalidAgenda`] if the items do not form one.
    pub fn agenda(&self) -> Result<AgendaV1, MeetingError> {
        AgendaV1::new(self.items.iter().map(|entry| entry.item.clone()).collect())
    }
}

/// The body of a meeting record.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MeetingRecordBodyV1 {
    /// The meeting was convened with this agenda.
    Convened {
        /// Channel, title, time, notice.
        metadata: MeetingMetadataV1,
        /// What was put before the shareholders.
        agenda: AgendaV1,
    },
    /// The meeting was finalised.
    Final(MeetingFinalRecordV1),
}

impl MeetingRecordBodyV1 {
    /// The `kind` attribute text.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Convened { .. } => "convened",
            Self::Final(_) => "final",
        }
    }
}

/// A parsed meeting record.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct MeetingRecordV1 {
    /// Which company.
    pub company: CompanyIdV1,
    /// Who attested to it, and when.
    pub notarisation: NotarisationV1,
    /// What it says.
    pub body: MeetingRecordBodyV1,
}

// ---------------------------------------------------------------------------------
// Composing.
// ---------------------------------------------------------------------------------

/// Composes the convening record.
///
/// # Errors
///
/// [`MeetingError::InvalidAgenda`] for bad metadata; [`MeetingError::Record`] if the
/// notarisation is invalid or the document does not read back.
pub fn compose_convened(
    company: &CompanyIdV1,
    notarisation: &NotarisationV1,
    metadata: &MeetingMetadataV1,
    agenda: &AgendaV1,
) -> Result<String, MeetingError> {
    metadata.validate()?;
    notarisation.validate()?;
    let mut xml = format!(
        "<irena-meeting version=\"{MEETING_VERSION}\" kind=\"convened\" company=\"{}\">\n  {}\n  <meeting{}>\n",
        company.as_str(),
        write_notarisation(notarisation),
        metadata_attributes(metadata),
    );
    write_agenda(
        &mut xml,
        agenda.items().iter().map(|item| (item, None, None)),
    );
    xml.push_str("  </meeting>\n</irena-meeting>");
    let read_back = read_meeting_record(&xml)?;
    let expected = MeetingRecordBodyV1::Convened {
        metadata: metadata.clone(),
        agenda: agenda.clone(),
    };
    if &read_back.company != company
        || &read_back.notarisation != notarisation
        || read_back.body != expected
    {
        return Err(MeetingError::Chain {
            detail: "composed convening record did not read back as written".to_owned(),
        });
    }
    Ok(xml)
}

/// Composes the final record.
///
/// # Errors
///
/// As [`compose_convened`].
pub fn compose_final(
    company: &CompanyIdV1,
    notarisation: &NotarisationV1,
    record: &MeetingFinalRecordV1,
) -> Result<String, MeetingError> {
    record.metadata.validate()?;
    record.agenda()?;
    notarisation.validate()?;
    let mut xml = format!(
        "<irena-meeting version=\"{MEETING_VERSION}\" kind=\"final\" company=\"{}\" meeting=\"{}\">\n  {}\n  <meeting{} opened-at-height=\"{}\">\n",
        company.as_str(),
        record.meeting_id,
        write_notarisation(notarisation),
        metadata_attributes(&record.metadata),
        record.opened_at_height.value(),
    );
    write_agenda(
        &mut xml,
        record
            .items
            .iter()
            .map(|entry| (&entry.item, entry.vote_tx_id, entry.outcome.as_deref())),
    );
    xml.push_str("  </meeting>\n</irena-meeting>");
    let read_back = read_meeting_record(&xml)?;
    if &read_back.company != company
        || &read_back.notarisation != notarisation
        || read_back.body != MeetingRecordBodyV1::Final(record.clone())
    {
        return Err(MeetingError::Chain {
            detail: "composed final record did not read back as written".to_owned(),
        });
    }
    Ok(xml)
}

fn metadata_attributes(metadata: &MeetingMetadataV1) -> String {
    let mut text = format!(
        " channel=\"{}\" title=\"{}\" scheduled-at=\"{}\"",
        escape_attribute(&metadata.channel),
        escape_attribute(&metadata.title),
        metadata.scheduled_at
    );
    if let Some(digest) = metadata.notice_digest {
        text.push_str(&format!(" notice-digest=\"{digest}\""));
    }
    text
}

fn write_agenda<'a>(
    xml: &mut String,
    items: impl Iterator<Item = (&'a AgendaItemV1, Option<TxId>, Option<&'a str>)>,
) {
    xml.push_str("    <agenda>\n");
    for (item, vote_tx, outcome) in items {
        xml.push_str(&format!(
            "      <item number=\"{}\" kind=\"{}\" title=\"{}\"",
            item.number,
            item.body.kind(),
            escape_attribute(&item.title)
        ));
        match &item.body {
            AgendaBodyV1::Informational { document_digest } => {
                xml.push_str(&format!(" document-digest=\"{document_digest}\""));
            }
            AgendaBodyV1::Vote { proposal_digest } => {
                xml.push_str(&format!(" proposal-digest=\"{proposal_digest}\""));
            }
        }
        if let Some(tx) = vote_tx {
            xml.push_str(&format!(" vote-tx=\"{tx}\""));
        }
        if let Some(outcome) = outcome {
            xml.push_str(&format!(" outcome=\"{}\"", escape_attribute(outcome)));
        }
        xml.push_str("/>\n");
    }
    xml.push_str("    </agenda>\n");
}

// ---------------------------------------------------------------------------------
// Reading.
// ---------------------------------------------------------------------------------

/// Reads an `<irena-meeting>` document.
///
/// # Errors
///
/// [`MeetingError::Record`] for anything that is not a valid meeting record — every
/// content issue collected — or [`MeetingError::InvalidAgenda`] if the items do not
/// form an agenda.
pub fn read_meeting_record(xml: &str) -> Result<MeetingRecordV1, MeetingError> {
    let found = xml.len() as u64;
    if found > MAX_DOCUMENT_BYTES {
        return Err(IrenaError::TooLarge {
            found,
            limit: MAX_DOCUMENT_BYTES,
        }
        .into());
    }
    let mut reader = open(xml);
    let root = root_start(&mut reader, "irena-meeting").map_err(IrenaError::from)?;
    let mut issues = Vec::new();
    let mut attributes =
        Attributes::of(&reader, "irena-meeting", &root).map_err(IrenaError::from)?;
    let version = require(&mut attributes, "version", &mut issues);
    let kind = require(&mut attributes, "kind", &mut issues);
    let company = require(&mut attributes, "company", &mut issues);
    let meeting = attributes.take("meeting");
    attributes.finish(&reader).map_err(IrenaError::from)?;
    if !issues.is_empty() {
        return Err(IrenaError::invalid(issues).into());
    }
    let version = version.expect("checked");
    if version != MEETING_VERSION {
        return Err(IrenaError::UnsupportedVersion { found: version }.into());
    }
    let kind = kind.expect("checked");
    let is_final = match kind.as_str() {
        "convened" => false,
        "final" => true,
        other => {
            return Err(malformed_at(
                &reader,
                format!("unknown meeting record kind {other:?}; expected convened or final"),
            ));
        }
    };
    let company = CompanyIdV1::new(company.expect("checked"))?;
    let meeting_id = match (is_final, meeting) {
        (true, Some(text)) => Some(MeetingIdV1::from_tx(
            TxId::from_hex(&text)
                .map_err(|error| invalid("irena-meeting", "meeting", &text, error.to_string()))?,
        )),
        (true, None) => {
            return Err(IrenaError::invalid(vec![IssueV1::MissingAttribute {
                element: "irena-meeting",
                attribute: "meeting",
            }])
            .into());
        }
        (false, Some(_)) => {
            return Err(malformed_at(
                &reader,
                "a convened record does not name a meeting; it is one",
            ));
        }
        (false, None) => None,
    };

    let mut notarisation = None;
    let mut body = None;
    loop {
        let event = next_event(&mut reader).map_err(IrenaError::from)?;
        let (child, is_empty) = match event {
            Event::Text(_) | Event::Comment(_) => continue,
            Event::Empty(child) => (child, true),
            Event::Start(child) => (child, false),
            Event::End(_) => break,
            other => {
                return Err(malformed_at(
                    &reader,
                    format!("unexpected {} in <irena-meeting>", describe(&other)),
                ));
            }
        };
        match child.name().as_ref() {
            "notarisation" => {
                if notarisation.is_some() || body.is_some() {
                    return Err(malformed_at(
                        &reader,
                        "notarisation must appear once, before the meeting element",
                    ));
                }
                notarisation = Some(read_notarisation(&reader, &child)?);
                if !is_empty {
                    expect_empty(&mut reader, "notarisation").map_err(IrenaError::from)?;
                }
            }
            "meeting" => {
                if body.is_some() {
                    return Err(malformed_at(
                        &reader,
                        "a record carries one meeting element",
                    ));
                }
                if is_empty {
                    return Err(malformed_at(&reader, "<meeting> must hold an agenda"));
                }
                body = Some(read_meeting(&mut reader, &child, is_final)?);
            }
            other => {
                return Err(malformed_at(
                    &reader,
                    format!("<irena-meeting> has an unknown child <{other}>"),
                ));
            }
        }
    }
    expect_eof(&mut reader).map_err(IrenaError::from)?;

    let notarisation = notarisation.ok_or_else(|| {
        IrenaError::invalid(vec![IssueV1::MissingElement {
            parent: "irena-meeting",
            element: "notarisation",
        }])
    })?;
    let (metadata, opened_at, items) = body.ok_or_else(|| {
        IrenaError::invalid(vec![IssueV1::MissingElement {
            parent: "irena-meeting",
            element: "meeting",
        }])
    })?;
    let body = if is_final {
        MeetingRecordBodyV1::Final(MeetingFinalRecordV1 {
            meeting_id: meeting_id.expect("checked"),
            metadata,
            opened_at_height: opened_at.expect("checked"),
            items,
        })
    } else {
        MeetingRecordBodyV1::Convened {
            metadata,
            agenda: AgendaV1::new(items.into_iter().map(|entry| entry.item).collect())?,
        }
    };
    Ok(MeetingRecordV1 {
        company,
        notarisation,
        body,
    })
}

type MeetingBody = (MeetingMetadataV1, Option<BlockHeight>, Vec<FinalItemV1>);

/// Parses `<meeting …>` with its `<agenda>`; `is_final` decides which attributes are
/// required and which are refused.
fn read_meeting(
    reader: &mut XmlReader<'_>,
    start: &BytesStart<'_>,
    is_final: bool,
) -> Result<MeetingBody, MeetingError> {
    let mut issues = Vec::new();
    let mut attributes = Attributes::of(reader, "meeting", start).map_err(IrenaError::from)?;
    let channel = require(&mut attributes, "channel", &mut issues);
    let title = require(&mut attributes, "title", &mut issues);
    let scheduled_at = require(&mut attributes, "scheduled-at", &mut issues);
    let notice = attributes.take("notice-digest");
    let opened_at = match (is_final, attributes.take("opened-at-height")) {
        (true, Some(text)) => Some(text),
        (true, None) => {
            issues.push(IssueV1::MissingAttribute {
                element: "meeting",
                attribute: "opened-at-height",
            });
            None
        }
        (false, Some(_)) => {
            issues.push(unused(
                "meeting",
                "opened-at-height",
                "the record is a convening",
            ));
            None
        }
        (false, None) => None,
    };
    attributes.finish(reader).map_err(IrenaError::from)?;

    let notice_digest = match notice {
        None => None,
        Some(text) => match Hash::from_hex(&text) {
            Ok(digest) => Some(digest),
            Err(error) => {
                issues.push(invalid_issue(
                    "meeting",
                    "notice-digest",
                    &text,
                    error.to_string(),
                ));
                None
            }
        },
    };
    if let Some(at) = &scheduled_at
        && let Err(error) = NotaryTimeV1::parse(at)
    {
        for issue in error.issues() {
            issues.push(match issue {
                IssueV1::InvalidValue { value, reason, .. } => IssueV1::InvalidValue {
                    element: "meeting",
                    attribute: "scheduled-at",
                    value: value.clone(),
                    reason: reason.clone(),
                },
                other => other.clone(),
            });
        }
    }
    if let Some(title) = &title
        && title.trim().is_empty()
    {
        issues.push(invalid_issue(
            "meeting",
            "title",
            title,
            "must not be empty".to_owned(),
        ));
    }
    if let Some(channel) = &channel
        && let Err(error) = irena_core::ChannelIdV1::new(channel.clone())
    {
        for issue in error.issues() {
            issues.push(match issue {
                IssueV1::InvalidValue { value, reason, .. } => IssueV1::InvalidValue {
                    element: "meeting",
                    attribute: "channel",
                    value: value.clone(),
                    reason: reason.clone(),
                },
                other => other.clone(),
            });
        }
    }
    let opened_at_height = opened_at.and_then(|text| match text.parse::<u64>() {
        Ok(height) if text.bytes().all(|b| b.is_ascii_digit()) => Some(BlockHeight(height)),
        _ => {
            issues.push(invalid_issue(
                "meeting",
                "opened-at-height",
                &text,
                "must be a decimal block height".to_owned(),
            ));
            None
        }
    });

    let mut items = None;
    loop {
        let event = next_event(reader).map_err(IrenaError::from)?;
        let (child, is_empty) = match event {
            Event::Text(_) | Event::Comment(_) => continue,
            Event::Empty(child) => (child, true),
            Event::Start(child) => (child, false),
            Event::End(_) => break,
            other => {
                return Err(malformed_at(
                    reader,
                    format!("unexpected {} in <meeting>", describe(&other)),
                ));
            }
        };
        if child.name().as_ref() != "agenda" {
            return Err(malformed_at(
                reader,
                format!("<meeting> has an unknown child <{}>", child.name().as_ref()),
            ));
        }
        if items.is_some() {
            issues.push(IssueV1::RepeatedElement {
                parent: "meeting",
                element: "agenda",
            });
        }
        Attributes::of(reader, "agenda", &child)
            .map_err(IrenaError::from)?
            .finish(reader)
            .map_err(IrenaError::from)?;
        items = Some(if is_empty {
            Vec::new()
        } else {
            read_items(reader, is_final, &mut issues)?
        });
    }
    let items = items.unwrap_or_else(|| {
        issues.push(IssueV1::MissingElement {
            parent: "meeting",
            element: "agenda",
        });
        Vec::new()
    });
    if !issues.is_empty() {
        return Err(IrenaError::invalid(issues).into());
    }
    Ok((
        MeetingMetadataV1 {
            channel: channel.expect("checked"),
            title: title.expect("checked"),
            scheduled_at: scheduled_at.expect("checked"),
            notice_digest,
        },
        opened_at_height,
        items,
    ))
}

fn read_items(
    reader: &mut XmlReader<'_>,
    is_final: bool,
    issues: &mut Vec<IssueV1>,
) -> Result<Vec<FinalItemV1>, MeetingError> {
    let mut items = Vec::new();
    loop {
        let event = next_event(reader).map_err(IrenaError::from)?;
        let (child, is_empty) = match event {
            Event::Text(_) | Event::Comment(_) => continue,
            Event::Empty(child) => (child, true),
            Event::Start(child) => (child, false),
            Event::End(_) => break,
            other => {
                return Err(malformed_at(
                    reader,
                    format!("unexpected {} in <agenda>", describe(&other)),
                ));
            }
        };
        if child.name().as_ref() != "item" {
            return Err(malformed_at(
                reader,
                format!("<agenda> has an unknown child <{}>", child.name().as_ref()),
            ));
        }
        let mut attributes = Attributes::of(reader, "item", &child).map_err(IrenaError::from)?;
        let number = require(&mut attributes, "number", issues);
        let kind = require(&mut attributes, "kind", issues);
        let title = require(&mut attributes, "title", issues);
        let document_digest = attributes.take("document-digest");
        let proposal_digest = attributes.take("proposal-digest");
        let vote_tx = attributes.take("vote-tx");
        let outcome = attributes.take("outcome");
        attributes.finish(reader).map_err(IrenaError::from)?;
        if !is_empty {
            expect_empty(reader, "item").map_err(IrenaError::from)?;
        }

        let number = number.and_then(|text| match text.parse::<u32>() {
            Ok(n) if text.bytes().all(|b| b.is_ascii_digit()) => Some(n),
            _ => {
                issues.push(invalid_issue(
                    "item",
                    "number",
                    &text,
                    "must be a decimal integer".to_owned(),
                ));
                None
            }
        });
        let digest_of = |name: &'static str, text: Option<String>, issues: &mut Vec<IssueV1>| {
            text.and_then(|text| match Hash::from_hex(&text) {
                Ok(digest) => Some(digest),
                Err(error) => {
                    issues.push(invalid_issue("item", name, &text, error.to_string()));
                    None
                }
            })
        };
        let body = match kind.as_deref() {
            Some("informational") => {
                if proposal_digest.is_some() {
                    issues.push(unused(
                        "item",
                        "proposal-digest",
                        "the item is informational",
                    ));
                }
                if vote_tx.is_some() || outcome.is_some() {
                    issues.push(unused("item", "vote-tx", "the item is informational"));
                }
                match document_digest {
                    Some(text) => digest_of("document-digest", Some(text), issues)
                        .map(|document_digest| AgendaBodyV1::Informational { document_digest }),
                    None => {
                        issues.push(IssueV1::MissingAttribute {
                            element: "item",
                            attribute: "document-digest",
                        });
                        None
                    }
                }
            }
            Some("vote") => {
                if document_digest.is_some() {
                    issues.push(unused("item", "document-digest", "the item is a vote"));
                }
                match proposal_digest {
                    Some(text) => digest_of("proposal-digest", Some(text), issues)
                        .map(|proposal_digest| AgendaBodyV1::Vote { proposal_digest }),
                    None => {
                        issues.push(IssueV1::MissingAttribute {
                            element: "item",
                            attribute: "proposal-digest",
                        });
                        None
                    }
                }
            }
            Some(other) => {
                issues.push(invalid_issue(
                    "item",
                    "kind",
                    other,
                    "expected informational or vote".to_owned(),
                ));
                None
            }
            None => None,
        };
        let is_vote = matches!(body, Some(AgendaBodyV1::Vote { .. }));
        let vote_tx_id = match (is_final && is_vote, vote_tx) {
            (true, Some(text)) => match TxId::from_hex(&text) {
                Ok(id) => Some(id),
                Err(error) => {
                    issues.push(invalid_issue("item", "vote-tx", &text, error.to_string()));
                    None
                }
            },
            (true, None) => {
                issues.push(IssueV1::MissingAttribute {
                    element: "item",
                    attribute: "vote-tx",
                });
                None
            }
            (false, Some(_)) if is_vote => {
                issues.push(unused("item", "vote-tx", "the record is a convening"));
                None
            }
            _ => None,
        };
        let outcome = match (is_final && is_vote, outcome) {
            (true, Some(text)) => Some(text),
            (true, None) => {
                issues.push(IssueV1::MissingAttribute {
                    element: "item",
                    attribute: "outcome",
                });
                None
            }
            (false, Some(_)) if is_vote => {
                issues.push(unused("item", "outcome", "the record is a convening"));
                None
            }
            _ => None,
        };
        if let (Some(number), Some(title), Some(body)) = (number, title, body) {
            items.push(FinalItemV1 {
                item: AgendaItemV1 {
                    number,
                    title,
                    body,
                },
                vote_tx_id,
                outcome,
            });
        }
    }
    Ok(items)
}

// ---------------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------------

fn require(
    attributes: &mut Attributes,
    name: &'static str,
    issues: &mut Vec<IssueV1>,
) -> Option<String> {
    let mut found = Vec::new();
    let value = attributes.require(name, &mut found);
    issues.extend(found.into_iter().map(IssueV1::Xml));
    value
}

/// An attribute that is present where it means nothing.
fn unused(element: &'static str, attribute: &'static str, reason: &'static str) -> IssueV1 {
    IssueV1::Xml(bornite_xml::XmlIssueV1::UnusedAttribute {
        element,
        attribute,
        reason,
    })
}

fn invalid_issue(
    element: &'static str,
    attribute: &'static str,
    value: &str,
    reason: String,
) -> IssueV1 {
    IssueV1::InvalidValue {
        element,
        attribute,
        value: value.to_owned(),
        reason,
    }
}

fn invalid(
    element: &'static str,
    attribute: &'static str,
    value: &str,
    reason: String,
) -> MeetingError {
    IrenaError::invalid(vec![invalid_issue(element, attribute, value, reason)]).into()
}

fn malformed_at(reader: &XmlReader<'_>, detail: impl Into<String>) -> MeetingError {
    IrenaError::from(malformed(reader, detail)).into()
}
