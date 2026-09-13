//! The two records on the chain, and their XML.
//!
//! Both are notarised XML in the style of the company and meeting records: readable
//! inside the block, the `<notarisation>` element written and read by `irena-core`'s
//! own code. Composition reads the document back before returning it.
//!
//! An amendment resolution carries the amendment body **verbatim**, nested. Reading it
//! back recovers the exact source bytes by slicing the document, never by
//! re-serialising parse events — the same discipline `prunella-xml` uses for nested
//! payloads, and for the same reason: the digest the shareholders approved is over
//! those bytes.

use crate::error::ResolutionError;
use crate::resolution::{
    AmendmentTargetV1, AuthorityV1, ResolutionIdV1, ResolutionKindV1, proposal_digest,
};
use bornite_xml::{
    Attributes, XmlReader, describe, expect_empty, expect_eof, malformed, next_event, open,
    root_start,
};
use irena_core::{
    ChannelIdV1, CompanyIdV1, IrenaError, IssueV1, NotarisationV1, escape_attribute,
    normalise_body, read_notarisation, write_notarisation,
};
use prunella_core::{Hash, TxId};
use quick_xml::events::Event;

/// The Prunella namespace resolution records are published under.
pub const RESOLUTION_NAMESPACE: &str = "irena.resolution.v1";

/// The Prunella namespace execution records are published under.
pub const EXECUTION_NAMESPACE: &str = "irena.execution.v1";

/// The Prunella schema version a resolution transaction declares.
pub const RESOLUTION_SCHEMA_VERSION: u32 = 1;

/// The Prunella schema version an execution transaction declares.
pub const EXECUTION_SCHEMA_VERSION: u32 = 1;

/// The only record version this build reads and writes.
pub const RESOLUTION_VERSION: &str = "1.0";

/// Default limit on document size, in bytes.
const MAX_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;

/// A parsed resolution record.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ResolutionRecordV1 {
    /// Which company.
    pub company: CompanyIdV1,
    /// Who attested to it, and when.
    pub notarisation: NotarisationV1,
    /// The resolution's title, for a reader. Opaque.
    pub title: String,
    /// What authorised it, pinned by transaction id.
    pub authority: AuthorityV1,
    /// What it does.
    pub kind: ResolutionKindV1,
}

/// What an execution leaves on the chain: the link from a resolution to the amendment
/// it authorised.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ResolutionExecutionV1 {
    /// The resolution executed.
    pub resolution_id: ResolutionIdV1,
    /// The company amendment it authorised — an ordinary `irena-ledger` record.
    pub amendment_tx: TxId,
    /// Which part was replaced.
    pub target: AmendmentTargetV1,
    /// The transaction the amendment superseded: the record the shareholders approved
    /// for replacement.
    pub replaced_tx: TxId,
    /// The digest of the body executed, which is the digest the vote approved.
    pub body_digest: Hash,
}

/// A parsed execution record.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ExecutionRecordV1 {
    /// Which company.
    pub company: CompanyIdV1,
    /// Who attested to it, and when.
    pub notarisation: NotarisationV1,
    /// What it records.
    pub execution: ResolutionExecutionV1,
}

// ---------------------------------------------------------------------------------
// Composing.
// ---------------------------------------------------------------------------------

/// Composes a resolution record.
///
/// # Errors
///
/// [`ResolutionError::Record`] if the notarisation or the body is invalid, or the
/// document does not read back as written.
pub fn compose_resolution(
    company: &CompanyIdV1,
    notarisation: &NotarisationV1,
    title: &str,
    authority: &AuthorityV1,
    kind: &ResolutionKindV1,
) -> Result<String, ResolutionError> {
    notarisation.validate()?;
    if title.trim().is_empty() {
        return Err(IrenaError::invalid(vec![IssueV1::InvalidValue {
            element: "irena-resolution",
            attribute: "title",
            value: title.to_owned(),
            reason: "must not be empty".to_owned(),
        }])
        .into());
    }
    let channel = authority.channel_id()?;
    let mut xml = format!(
        "<irena-resolution version=\"{RESOLUTION_VERSION}\" kind=\"{}\" company=\"{}\" title=\"{}\" channel=\"{}\"",
        kind.as_str(),
        company.as_str(),
        escape_attribute(title),
        channel.as_str(),
    );
    match authority {
        AuthorityV1::Collective {
            meeting_tx,
            item_number,
            vote_tx,
            ..
        } => xml.push_str(&format!(
            " meeting=\"{meeting_tx}\" item=\"{item_number}\" vote=\"{vote_tx}\""
        )),
        AuthorityV1::Individual { decision_tx, .. } => {
            xml.push_str(&format!(" decision=\"{decision_tx}\""));
        }
    }
    match kind {
        ResolutionKindV1::Declarative { document_digest } => {
            xml.push_str(&format!(" document-digest=\"{document_digest}\">\n  "));
            xml.push_str(&write_notarisation(notarisation));
            xml.push_str("\n</irena-resolution>");
        }
        ResolutionKindV1::Amendment { target, body } => {
            // The body is validated on its own terms first, so an error names the
            // amendment document rather than the envelope around it.
            let body = normalise_body(body);
            validate_body(*target, body)?;
            xml.push_str(&format!(" target=\"{target}\">\n  "));
            xml.push_str(&write_notarisation(notarisation));
            xml.push_str("\n  <amendment>");
            xml.push_str(body);
            xml.push_str("</amendment>\n</irena-resolution>");
        }
    }
    // The body is embedded normalised, so the record reads back with the normalised
    // form. Compare against that: a declaration or surrounding whitespace on the
    // caller's body is not a difference, and `approved_digest` already ignores it.
    let normalised = match kind {
        ResolutionKindV1::Declarative { .. } => kind.clone(),
        ResolutionKindV1::Amendment { target, body } => ResolutionKindV1::Amendment {
            target: *target,
            body: normalise_body(body).to_owned(),
        },
    };
    let read_back = read_resolution_record(&xml)?;
    if &read_back.company != company
        || &read_back.notarisation != notarisation
        || read_back.title != title
        || &read_back.authority != authority
        || read_back.kind != normalised
    {
        return Err(ResolutionError::Chain {
            detail: "composed resolution did not read back as written".to_owned(),
        });
    }
    Ok(xml)
}

/// Checks that a body is a valid document of its target's kind.
fn validate_body(target: AmendmentTargetV1, body: &str) -> Result<(), ResolutionError> {
    match target {
        AmendmentTargetV1::ShareStructure => {
            irena_core::read_share_structure_document(body)?;
        }
        AmendmentTargetV1::DecisionChannels => {
            irena_core::read_decision_channels_document(body)?;
        }
    }
    Ok(())
}

/// Composes an execution record.
///
/// # Errors
///
/// As [`compose_resolution`].
pub fn compose_execution(
    company: &CompanyIdV1,
    notarisation: &NotarisationV1,
    execution: &ResolutionExecutionV1,
) -> Result<String, ResolutionError> {
    notarisation.validate()?;
    let xml = format!(
        "<irena-execution version=\"{RESOLUTION_VERSION}\" company=\"{}\" resolution=\"{}\" amendment=\"{}\" target=\"{}\" replaced=\"{}\" body-digest=\"{}\">\n  {}\n</irena-execution>",
        company.as_str(),
        execution.resolution_id,
        execution.amendment_tx,
        execution.target,
        execution.replaced_tx,
        execution.body_digest,
        write_notarisation(notarisation),
    );
    let read_back = read_execution_record(&xml)?;
    if &read_back.company != company
        || &read_back.notarisation != notarisation
        || &read_back.execution != execution
    {
        return Err(ResolutionError::Chain {
            detail: "composed execution did not read back as written".to_owned(),
        });
    }
    Ok(xml)
}

// ---------------------------------------------------------------------------------
// Reading.
// ---------------------------------------------------------------------------------

/// Reads an `<irena-resolution>` document.
///
/// # Errors
///
/// [`ResolutionError::Record`] for anything that is not a valid resolution record,
/// with every content issue collected.
pub fn read_resolution_record(xml: &str) -> Result<ResolutionRecordV1, ResolutionError> {
    check_size(xml)?;
    let mut reader = open(xml);
    let root = root_start(&mut reader, "irena-resolution").map_err(IrenaError::from)?;
    let mut issues = Vec::new();
    let mut attributes =
        Attributes::of(&reader, "irena-resolution", &root).map_err(IrenaError::from)?;
    let version = require(&mut attributes, "version", &mut issues);
    let kind = require(&mut attributes, "kind", &mut issues);
    let company = require(&mut attributes, "company", &mut issues);
    let title = require(&mut attributes, "title", &mut issues);
    let channel = require(&mut attributes, "channel", &mut issues);
    let meeting = attributes.take("meeting");
    let item = attributes.take("item");
    let vote = attributes.take("vote");
    let decision = attributes.take("decision");
    let document_digest = attributes.take("document-digest");
    let target = attributes.take("target");
    attributes.finish(&reader).map_err(IrenaError::from)?;
    if !issues.is_empty() {
        return Err(IrenaError::invalid(issues).into());
    }
    check_version(&version.expect("checked"))?;
    let company = CompanyIdV1::new(company.expect("checked"))?;
    let title = title.expect("checked");
    if title.trim().is_empty() {
        return Err(invalid(
            "irena-resolution",
            "title",
            &title,
            "must not be empty",
        ));
    }
    let channel = channel.expect("checked");
    let channel = ChannelIdV1::new(channel.clone())
        .map_err(|error| invalid("irena-resolution", "channel", &channel, &error.to_string()))?
        .as_str()
        .to_owned();
    let authority = match (meeting, item, vote, decision) {
        (Some(meeting), Some(item), Some(vote), None) => AuthorityV1::Collective {
            channel,
            meeting_tx: tx_of("meeting", &meeting)?,
            item_number: number_of("item", &item)?,
            vote_tx: tx_of("vote", &vote)?,
        },
        (None, None, None, Some(decision)) => AuthorityV1::Individual {
            channel,
            decision_tx: tx_of("decision", &decision)?,
        },
        (None, None, None, None) => {
            return Err(IrenaError::invalid(vec![IssueV1::MissingAttribute {
                element: "irena-resolution",
                attribute: "meeting, item and vote, or decision",
            }])
            .into());
        }
        _ => {
            return Err(malformed_at(
                &reader,
                "a resolution rests on one authority: meeting, item and vote together, or decision alone",
            ));
        }
    };
    let declared = kind.expect("checked");

    let mut notarisation = None;
    let mut body: Option<String> = None;
    loop {
        let before = position(&reader)?;
        let event = next_event(&mut reader).map_err(IrenaError::from)?;
        let (child, is_empty) = match event {
            Event::Text(_) | Event::Comment(_) => continue,
            Event::Empty(child) => (child, true),
            Event::Start(child) => (child, false),
            Event::End(_) => break,
            other => {
                return Err(malformed_at(
                    &reader,
                    format!("unexpected {} in <irena-resolution>", describe(&other)),
                ));
            }
        };
        match child.name().as_ref() {
            "notarisation" => {
                if notarisation.is_some() || body.is_some() {
                    return Err(malformed_at(
                        &reader,
                        "notarisation must appear once, before the amendment",
                    ));
                }
                notarisation = Some(read_notarisation(&reader, &child)?);
                if !is_empty {
                    expect_empty(&mut reader, "notarisation").map_err(IrenaError::from)?;
                }
            }
            "amendment" => {
                if body.is_some() {
                    return Err(malformed_at(&reader, "a resolution carries one amendment"));
                }
                if is_empty {
                    return Err(malformed_at(
                        &reader,
                        "<amendment> must hold a body element",
                    ));
                }
                let _ = before;
                let inner_start = position(&reader)?;
                body = Some(capture_body(&mut reader, xml, inner_start)?);
            }
            other => {
                return Err(malformed_at(
                    &reader,
                    format!("<irena-resolution> has an unknown child <{other}>"),
                ));
            }
        }
    }
    expect_eof(&mut reader).map_err(IrenaError::from)?;

    let notarisation = notarisation.ok_or_else(|| missing("irena-resolution", "notarisation"))?;
    let kind = match (declared.as_str(), document_digest, target, body) {
        ("declarative", Some(text), None, None) => ResolutionKindV1::Declarative {
            document_digest: hash_of("irena-resolution", "document-digest", &text)?,
        },
        ("declarative", None, _, _) => {
            return Err(IrenaError::invalid(vec![IssueV1::MissingAttribute {
                element: "irena-resolution",
                attribute: "document-digest",
            }])
            .into());
        }
        ("declarative", Some(_), _, _) => {
            return Err(malformed_at(
                &reader,
                "a declarative resolution carries no target and no amendment",
            ));
        }
        ("amendment", None, Some(text), Some(body)) => {
            let target = AmendmentTargetV1::parse(&text).ok_or_else(|| {
                invalid(
                    "irena-resolution",
                    "target",
                    &text,
                    "expected share-structure or decision-channels",
                )
            })?;
            validate_body(target, &body)?;
            ResolutionKindV1::Amendment { target, body }
        }
        ("amendment", Some(_), _, _) => {
            return Err(malformed_at(
                &reader,
                "an amendment resolution carries its body, not a document digest",
            ));
        }
        ("amendment", None, None, _) => {
            return Err(IrenaError::invalid(vec![IssueV1::MissingAttribute {
                element: "irena-resolution",
                attribute: "target",
            }])
            .into());
        }
        ("amendment", None, Some(_), None) => {
            return Err(missing("irena-resolution", "amendment").into());
        }
        (other, ..) => {
            return Err(malformed_at(
                &reader,
                format!("unknown resolution kind {other:?}; expected declarative or amendment"),
            ));
        }
    };
    Ok(ResolutionRecordV1 {
        company,
        notarisation,
        title,
        authority,
        kind,
    })
}

/// Reads an `<irena-execution>` document.
///
/// # Errors
///
/// As [`read_resolution_record`].
pub fn read_execution_record(xml: &str) -> Result<ExecutionRecordV1, ResolutionError> {
    check_size(xml)?;
    let mut reader = open(xml);
    let root = root_start(&mut reader, "irena-execution").map_err(IrenaError::from)?;
    let mut issues = Vec::new();
    let mut attributes =
        Attributes::of(&reader, "irena-execution", &root).map_err(IrenaError::from)?;
    let version = require(&mut attributes, "version", &mut issues);
    let company = require(&mut attributes, "company", &mut issues);
    let resolution = require(&mut attributes, "resolution", &mut issues);
    let amendment = require(&mut attributes, "amendment", &mut issues);
    let target = require(&mut attributes, "target", &mut issues);
    let replaced = require(&mut attributes, "replaced", &mut issues);
    let body_digest = require(&mut attributes, "body-digest", &mut issues);
    attributes.finish(&reader).map_err(IrenaError::from)?;
    if !issues.is_empty() {
        return Err(IrenaError::invalid(issues).into());
    }
    check_version(&version.expect("checked"))?;
    let company = CompanyIdV1::new(company.expect("checked"))?;
    let target_text = target.expect("checked");
    let target = AmendmentTargetV1::parse(&target_text).ok_or_else(|| {
        invalid(
            "irena-execution",
            "target",
            &target_text,
            "expected share-structure or decision-channels",
        )
    })?;
    let execution = ResolutionExecutionV1 {
        resolution_id: ResolutionIdV1::from_tx(tx_of("resolution", &resolution.expect("checked"))?),
        amendment_tx: tx_of("amendment", &amendment.expect("checked"))?,
        target,
        replaced_tx: tx_of("replaced", &replaced.expect("checked"))?,
        body_digest: hash_of(
            "irena-execution",
            "body-digest",
            &body_digest.expect("checked"),
        )?,
    };

    let mut notarisation = None;
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
                    format!("unexpected {} in <irena-execution>", describe(&other)),
                ));
            }
        };
        if child.name().as_ref() != "notarisation" {
            return Err(malformed_at(
                &reader,
                format!(
                    "<irena-execution> has an unknown child <{}>",
                    child.name().as_ref()
                ),
            ));
        }
        if notarisation.is_some() {
            return Err(malformed_at(&reader, "notarisation must appear once"));
        }
        notarisation = Some(read_notarisation(&reader, &child)?);
        if !is_empty {
            expect_empty(&mut reader, "notarisation").map_err(IrenaError::from)?;
        }
    }
    expect_eof(&mut reader).map_err(IrenaError::from)?;

    Ok(ExecutionRecordV1 {
        company,
        notarisation: notarisation.ok_or_else(|| missing("irena-execution", "notarisation"))?,
        execution,
    })
}

/// Recovers the exact source bytes inside `<amendment>` — the body as it was
/// embedded, comments and all — and consumes the amendment's end tag.
///
/// Nothing is rebuilt from parse events: a parser normalises line endings, attribute
/// quoting and entity spelling, and the digest the actors approved is over the
/// original bytes. The reader is used only to find where the end tag begins; the body
/// is the slice of the document from just after `<amendment>` to just before
/// `</amendment>`, trimmed — which is exactly what [`compose_resolution`] embedded.
/// Whether that slice is one valid document of the target's kind is the target
/// reader's decision, made by the caller.
fn capture_body(
    reader: &mut XmlReader<'_>,
    source: &str,
    inner_start: usize,
) -> Result<String, ResolutionError> {
    let mut depth = 0usize;
    loop {
        let before = position(reader)?;
        match next_event(reader).map_err(IrenaError::from)? {
            Event::Start(_) => depth += 1,
            Event::End(_) => {
                if depth == 0 {
                    let body = source
                        .get(inner_start..before)
                        .ok_or_else(|| ResolutionError::Chain {
                            detail: "amendment positions do not lie on character boundaries"
                                .to_owned(),
                        })?
                        .trim();
                    if body.is_empty() {
                        return Err(malformed_at(reader, "<amendment> must hold a body element"));
                    }
                    return Ok(body.to_owned());
                }
                depth -= 1;
            }
            Event::Empty(_)
            | Event::Text(_)
            | Event::CData(_)
            | Event::Comment(_)
            | Event::PI(_)
            | Event::GeneralRef(_) => {}
            Event::Eof => {
                return Err(malformed_at(reader, "document ends inside an amendment"));
            }
            other => {
                return Err(malformed_at(
                    reader,
                    format!("unexpected {} in <amendment>", describe(&other)),
                ));
            }
        }
    }
}

pub(crate) fn body_matches(body: &str, approved: Hash) -> bool {
    proposal_digest(body) == approved
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

fn position(reader: &XmlReader<'_>) -> Result<usize, ResolutionError> {
    usize::try_from(reader.buffer_position()).map_err(|_| ResolutionError::Chain {
        detail: "document position does not fit in memory".to_owned(),
    })
}

fn check_size(xml: &str) -> Result<(), ResolutionError> {
    let found = xml.len() as u64;
    if found > MAX_DOCUMENT_BYTES {
        return Err(IrenaError::TooLarge {
            found,
            limit: MAX_DOCUMENT_BYTES,
        }
        .into());
    }
    Ok(())
}

fn check_version(version: &str) -> Result<(), ResolutionError> {
    if version == RESOLUTION_VERSION {
        Ok(())
    } else {
        Err(IrenaError::UnsupportedVersion {
            found: version.to_owned(),
        }
        .into())
    }
}

fn tx_of(attribute: &'static str, text: &str) -> Result<TxId, ResolutionError> {
    TxId::from_hex(text)
        .map_err(|error| invalid("irena-resolution", attribute, text, &error.to_string()))
}

fn hash_of(
    element: &'static str,
    attribute: &'static str,
    text: &str,
) -> Result<Hash, ResolutionError> {
    Hash::from_hex(text).map_err(|error| invalid(element, attribute, text, &error.to_string()))
}

fn number_of(attribute: &'static str, text: &str) -> Result<u32, ResolutionError> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid(
            "irena-resolution",
            attribute,
            text,
            "must be a decimal integer",
        ));
    }
    text.parse()
        .map_err(|_| invalid("irena-resolution", attribute, text, "is too large"))
}

fn invalid(
    element: &'static str,
    attribute: &'static str,
    value: &str,
    reason: &str,
) -> ResolutionError {
    IrenaError::invalid(vec![IssueV1::InvalidValue {
        element,
        attribute,
        value: value.to_owned(),
        reason: reason.to_owned(),
    }])
    .into()
}

fn missing(parent: &'static str, element: &'static str) -> IrenaError {
    IrenaError::invalid(vec![IssueV1::MissingElement { parent, element }])
}

fn malformed_at(reader: &XmlReader<'_>, detail: impl Into<String>) -> ResolutionError {
    IrenaError::from(malformed(reader, detail)).into()
}
