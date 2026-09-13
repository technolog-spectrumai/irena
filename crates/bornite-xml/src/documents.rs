//! The two standalone documents.

use crate::error::XmlError;
use crate::reader::{
    Attributes, XmlReader, check_version, describe, malformed, next_event, parse_ballots,
    parse_electorate, parse_voting_rules,
};
use bornite_core::{BallotSetV1, ElectorateV1};
use bornite_rules::VotingRulesV1;
use quick_xml::events::Event;

/// Default limit on document size, in bytes. A rules file is a few hundred bytes; a
/// vote document with a large electorate is a few megabytes. Anything beyond this is
/// not a document this reader should be asked to hold in memory.
pub const DEFAULT_MAX_DOCUMENT_BYTES: u64 = 64 * 1024 * 1024;

/// A parsed vote document: a frozen electorate and the ballots cast.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoteDocumentV1 {
    /// The electorate.
    pub electorate: ElectorateV1,
    /// The ballots.
    pub ballots: BallotSetV1,
}

/// Reads a `<voting-rules>` document.
///
/// # Errors
///
/// Returns [`XmlError`] for anything that is not a valid V1 rules document.
pub fn read_rules_document(xml: &str) -> Result<VotingRulesV1, XmlError> {
    read_rules_document_with_limit(xml, DEFAULT_MAX_DOCUMENT_BYTES)
}

/// Reads a `<voting-rules>` document, refusing input over `max_bytes`.
///
/// # Errors
///
/// As [`read_rules_document`], plus [`XmlError::TooLarge`].
pub fn read_rules_document_with_limit(
    xml: &str,
    max_bytes: u64,
) -> Result<VotingRulesV1, XmlError> {
    check_size(xml, max_bytes)?;
    let mut reader = open(xml);
    let root = root_start(&mut reader, "voting-rules")?;
    let rules = parse_voting_rules(&mut reader, &root)?;
    expect_eof(&mut reader)?;
    Ok(rules)
}

/// Reads a `<vote>` document.
///
/// # Errors
///
/// Returns [`XmlError`] for anything that is not a valid V1 vote document.
pub fn read_vote_document(xml: &str) -> Result<VoteDocumentV1, XmlError> {
    read_vote_document_with_limit(xml, DEFAULT_MAX_DOCUMENT_BYTES)
}

/// Reads a `<vote>` document, refusing input over `max_bytes`.
///
/// # Errors
///
/// As [`read_vote_document`], plus [`XmlError::TooLarge`].
pub fn read_vote_document_with_limit(
    xml: &str,
    max_bytes: u64,
) -> Result<VoteDocumentV1, XmlError> {
    check_size(xml, max_bytes)?;
    let mut reader = open(xml);
    let root = root_start(&mut reader, "vote")?;
    let mut attributes = Attributes::of(&reader, "vote", &root)?;
    let mut issues = Vec::new();
    let version = attributes.require("version", &mut issues);
    attributes.finish(&reader)?;
    if let Some(version) = version {
        check_version(&version)?;
    }
    if !issues.is_empty() {
        return Err(XmlError::Invalid { issues });
    }

    let mut electorate = None;
    let mut ballots = None;
    loop {
        match next_event(&mut reader)? {
            Event::Text(_) | Event::Comment(_) => {}
            event @ (Event::Start(_) | Event::Empty(_)) => {
                let is_empty = matches!(event, Event::Empty(_));
                let (Event::Start(child) | Event::Empty(child)) = event else {
                    unreachable!("matched above")
                };
                let name = child.name().as_ref().to_owned();
                match name.as_str() {
                    "electorate" if electorate.is_none() => {
                        electorate = Some(parse_electorate(&mut reader, &child, is_empty)?);
                    }
                    "ballots" if ballots.is_none() => {
                        ballots = Some(parse_ballots(&mut reader, &child, is_empty)?);
                    }
                    "electorate" | "ballots" => {
                        return Err(malformed(
                            &reader,
                            format!("<vote> has more than one <{name}>"),
                        ));
                    }
                    other => {
                        return Err(malformed(
                            &reader,
                            format!("<vote> has an unknown child <{other}>"),
                        ));
                    }
                }
            }
            Event::End(_) => break,
            other => {
                return Err(malformed(
                    &reader,
                    format!("unexpected {} in <vote>", describe(&other)),
                ));
            }
        }
    }
    expect_eof(&mut reader)?;

    let mut issues = Vec::new();
    if electorate.is_none() {
        issues.push(crate::error::XmlIssueV1::MissingElement {
            parent: "vote",
            element: "electorate",
        });
    }
    if ballots.is_none() {
        issues.push(crate::error::XmlIssueV1::MissingElement {
            parent: "vote",
            element: "ballots",
        });
    }
    match (electorate, ballots) {
        (Some(electorate), Some(ballots)) => Ok(VoteDocumentV1 {
            electorate,
            ballots,
        }),
        _ => Err(XmlError::Invalid { issues }),
    }
}

fn check_size(xml: &str, max_bytes: u64) -> Result<(), XmlError> {
    let found = xml.len() as u64;
    if found > max_bytes {
        Err(XmlError::TooLarge {
            found,
            limit: max_bytes,
        })
    } else {
        Ok(())
    }
}

/// Opens a reader with the settings every Bornite document uses.
#[must_use]
pub fn open(xml: &str) -> XmlReader<'_> {
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(true);
    reader
}

/// Reads up to and including the root start tag, which must be `expected`.
///
/// # Errors
///
/// Returns [`XmlError::Malformed`] if the document is empty or its root is something else.
pub fn root_start<'a>(
    reader: &mut XmlReader<'a>,
    expected: &str,
) -> Result<quick_xml::events::BytesStart<'a>, XmlError> {
    loop {
        match next_event(reader)? {
            Event::Decl(_) | Event::Comment(_) | Event::Text(_) | Event::PI(_) => {}
            Event::Start(root) if root.name().as_ref() == expected => return Ok(root),
            Event::Empty(root) if root.name().as_ref() == expected => {
                return Err(malformed(reader, format!("<{expected}> must have content")));
            }
            Event::Start(other) | Event::Empty(other) => {
                return Err(malformed(
                    reader,
                    format!(
                        "expected a <{expected}> root, found <{}>",
                        other.name().as_ref()
                    ),
                ));
            }
            Event::Eof => return Err(malformed(reader, "document is empty")),
            other => {
                return Err(malformed(
                    reader,
                    format!("unexpected {} before the root", describe(&other)),
                ));
            }
        }
    }
}

/// Confirms nothing but whitespace and comments follows the root.
///
/// # Errors
///
/// Returns [`XmlError::Malformed`] for trailing content.
pub fn expect_eof(reader: &mut XmlReader<'_>) -> Result<(), XmlError> {
    loop {
        match next_event(reader)? {
            Event::Eof => return Ok(()),
            Event::Text(_) | Event::Comment(_) => {}
            other => {
                return Err(malformed(
                    reader,
                    format!("trailing {} after the root", describe(&other)),
                ));
            }
        }
    }
}
