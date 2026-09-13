//! Reading and composing governance record documents.
//!
//! Reading delegates the inner element to `bornite-xml`'s own parsers, so the rules
//! bytes on the ledger are read by exactly the code that reads a standalone rules file.
//! Composing embeds the caller's inner element **verbatim**: the bridge does not
//! re-serialise a document a notary signed off on.

use crate::error::BridgeError;
use crate::record::{GovernanceRecordV1, NotarisationV1, RecordBodyV1, RecordKindV1, SubjectV1};
use bornite_xml::{
    Attributes, XmlReader, check_version, describe, expect_empty, expect_eof, malformed,
    next_event, open, parse_electorate, parse_voting_rules, root_start,
};
use prunella_core::{Hash, TxId};
use quick_xml::events::Event;

/// Reads a `<governance-record>` document.
///
/// # Errors
///
/// Returns [`BridgeError`] for anything that is not a valid V1 record: a malformed
/// envelope, an unknown attribute, a `kind` that does not match the element carried,
/// or an inner element `bornite-xml` refuses.
pub fn read_record(xml: &str) -> Result<GovernanceRecordV1, BridgeError> {
    let mut reader = open(xml);
    let root = root_start(&mut reader, "governance-record")?;

    let mut issues = Vec::new();
    let mut attributes = Attributes::of(&reader, "governance-record", &root)?;
    let version = attributes.require("version", &mut issues);
    let kind = attributes.require("kind", &mut issues);
    let subject = attributes.require("subject", &mut issues);
    let supersedes = attributes.take("supersedes");
    attributes.finish(&reader)?;
    if !issues.is_empty() {
        return Err(bornite_xml::XmlError::Invalid { issues }.into());
    }
    let version = version.expect("checked");
    check_version(&version)?;
    let kind = kind.expect("checked");
    let declared = RecordKindV1::parse(&kind).ok_or_else(|| BridgeError::Malformed {
        detail: format!("unknown record kind {kind:?}; expected voting-rules or roll"),
    })?;
    let subject = SubjectV1::new(subject.expect("checked"))?;
    let supersedes = supersedes
        .map(|text| TxId::from_hex(&text))
        .transpose()
        .map_err(|error| BridgeError::Malformed {
            detail: format!("supersedes: {error}"),
        })?;

    let mut notarisation = None;
    let mut body = None;
    loop {
        let event = next_event(&mut reader)?;
        let (child, is_empty) = match event {
            Event::Text(_) | Event::Comment(_) => continue,
            Event::Empty(child) => (child, true),
            Event::Start(child) => (child, false),
            Event::End(_) => break,
            other => {
                return Err(malformed(
                    &reader,
                    format!("unexpected {} in <governance-record>", describe(&other)),
                )
                .into());
            }
        };
        let name = child.name().as_ref().to_owned();
        match name.as_str() {
            "notarisation" => {
                if notarisation.is_some() || body.is_some() {
                    return Err(BridgeError::Malformed {
                        detail: "notarisation must appear once, before the element it attests to"
                            .to_owned(),
                    });
                }
                notarisation = Some(read_notarisation(&reader, &child)?);
                if !is_empty {
                    expect_empty(&mut reader, "notarisation")?;
                }
            }
            "voting-rules" => {
                if body.is_some() {
                    return Err(BridgeError::Malformed {
                        detail: "a record carries exactly one element".to_owned(),
                    });
                }
                if is_empty {
                    return Err(malformed(&reader, "<voting-rules> must have content").into());
                }
                body = Some(RecordBodyV1::VotingRules(parse_voting_rules(
                    &mut reader,
                    &child,
                )?));
            }
            "electorate" => {
                if body.is_some() {
                    return Err(BridgeError::Malformed {
                        detail: "a record carries exactly one element".to_owned(),
                    });
                }
                body = Some(RecordBodyV1::Roll(parse_electorate(
                    &mut reader,
                    &child,
                    is_empty,
                )?));
            }
            other => {
                return Err(malformed(
                    &reader,
                    format!("<governance-record> has an unknown child <{other}>"),
                )
                .into());
            }
        }
    }
    expect_eof(&mut reader)?;

    let body = body.ok_or_else(|| BridgeError::Malformed {
        detail: "a record must carry a <voting-rules> or <electorate> element".to_owned(),
    })?;
    if body.kind() != declared {
        return Err(BridgeError::KindMismatch {
            declared,
            carried: body.kind(),
        });
    }
    Ok(GovernanceRecordV1 {
        subject,
        supersedes,
        notarisation,
        body,
    })
}

fn read_notarisation(
    reader: &XmlReader<'_>,
    child: &quick_xml::events::BytesStart<'_>,
) -> Result<NotarisationV1, BridgeError> {
    let mut issues = Vec::new();
    let mut attributes = Attributes::of(reader, "notarisation", child)?;
    let notary = attributes.require("notary", &mut issues);
    let statement = attributes.take("statement");
    let source_digest = attributes.take("source-digest");
    attributes.finish(reader)?;
    if !issues.is_empty() {
        return Err(bornite_xml::XmlError::Invalid { issues }.into());
    }
    let source_digest = source_digest
        .map(|text| Hash::from_hex(&text))
        .transpose()
        .map_err(|error| BridgeError::Malformed {
            detail: format!("source-digest: {error}"),
        })?;
    Ok(NotarisationV1 {
        notary: notary.expect("checked"),
        statement,
        source_digest,
    })
}

/// Composes a record document around an inner element supplied as text.
///
/// `inner` must be a standalone `<voting-rules>` or `<electorate>` element matching
/// `kind`. A leading XML declaration and surrounding whitespace are removed; the
/// element itself is embedded **byte for byte**. The composed document is read back
/// before it is returned, so what the ledger receives is known to parse.
///
/// # Errors
///
/// Returns [`BridgeError`] if `inner` is not a valid element of the right kind, or the
/// composed record does not read back.
pub fn compose_record(
    kind: RecordKindV1,
    subject: &SubjectV1,
    supersedes: Option<TxId>,
    notarisation: Option<&NotarisationV1>,
    inner: &str,
) -> Result<String, BridgeError> {
    let element = strip_declaration(inner);
    // Validate the inner element on its own terms first, so an error names the inner
    // document rather than the envelope around it.
    match kind {
        RecordKindV1::VotingRules => {
            bornite_xml::read_rules_document(element)?;
        }
        RecordKindV1::Roll => {
            read_electorate_document(element)?;
        }
    }

    let mut xml = String::new();
    xml.push_str(&format!(
        "<governance-record version=\"1.0\" kind=\"{}\" subject=\"{}\"",
        kind.as_str(),
        subject.as_str()
    ));
    if let Some(supersedes) = supersedes {
        xml.push_str(&format!(" supersedes=\"{supersedes}\""));
    }
    xml.push_str(">\n");
    if let Some(notarisation) = notarisation {
        xml.push_str(&format!(
            "  <notarisation notary=\"{}\"",
            escape_attribute(&notarisation.notary)
        ));
        if let Some(statement) = &notarisation.statement {
            xml.push_str(&format!(" statement=\"{}\"", escape_attribute(statement)));
        }
        if let Some(digest) = notarisation.source_digest {
            xml.push_str(&format!(" source-digest=\"{digest}\""));
        }
        xml.push_str("/>\n");
    }
    xml.push_str(element);
    xml.push_str("\n</governance-record>\n");

    let read_back = read_record(&xml)?;
    if read_back.kind() != kind
        || &read_back.subject != subject
        || read_back.supersedes != supersedes
    {
        return Err(BridgeError::Malformed {
            detail: "composed record did not read back as written".to_owned(),
        });
    }
    Ok(xml)
}

/// Reads a standalone `<electorate>` document.
///
/// # Errors
///
/// Returns [`BridgeError`] for anything that is not a valid electorate element.
pub fn read_electorate_document(xml: &str) -> Result<bornite_core::ElectorateV1, BridgeError> {
    let mut reader = open(xml);
    let root = loop {
        match next_event(&mut reader)? {
            Event::Decl(_) | Event::Comment(_) | Event::Text(_) => {}
            Event::Start(root) if root.name().as_ref() == "electorate" => break (root, false),
            Event::Empty(root) if root.name().as_ref() == "electorate" => break (root, true),
            other => {
                return Err(malformed(
                    &reader,
                    format!("expected an <electorate> root, found {}", describe(&other)),
                )
                .into());
            }
        }
    };
    let electorate = parse_electorate(&mut reader, &root.0, root.1)?;
    expect_eof(&mut reader)?;
    Ok(electorate)
}

/// Removes a leading XML declaration and surrounding whitespace, leaving the element.
fn strip_declaration(text: &str) -> &str {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("<?xml")
        && let Some(end) = rest.find("?>")
    {
        return rest[end + 2..].trim();
    }
    trimmed
}

fn escape_attribute(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
