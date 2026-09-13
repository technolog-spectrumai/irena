//! Reading and composing Irena record documents.
//!
//! Reading a `<voting-rules>` body delegates to `bornite-xml`'s own parser, so the
//! rules bytes on the ledger are read by exactly the code that reads a standalone
//! rules file. The company bodies are parsed here with the same reader helpers and the
//! same strictness. Composing embeds the caller's body element **verbatim**: Irena
//! does not re-serialise a document a notary signed off on.

use crate::company::{CompanyGenesisV1, CompanyIdV1, IdentityV1};
use crate::error::{IrenaError, IssueV1};
use crate::notarisation::{NotarisationV1, NotaryIdV1, NotaryTimeV1};
use crate::record::{IrenaRecordV1, RECORD_VERSION, RecordBodyV1, RecordKindV1};
use crate::shares::{HolderV1, MAX_HOLDERS, ShareStructureV1};
use bornite_core::VoterIdV1;
use bornite_xml::{
    Attributes, XmlReader, describe, expect_empty, expect_eof, malformed, next_event, open,
    parse_voting_rules, root_start,
};
use prunella_core::{Hash, PublicKey};
use quick_xml::events::{BytesStart, Event};

/// Default limit on document size, in bytes.
pub const DEFAULT_MAX_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;

/// Reads an `<irena-record>` document with the default size limit.
///
/// # Errors
///
/// Returns [`IrenaError`] for anything that is not a valid V1 record: a malformed
/// envelope, an unknown attribute, a missing or invalid notarisation, a `kind` that
/// does not match the element carried, or a body its own reader refuses.
pub fn read_record(xml: &str) -> Result<IrenaRecordV1, IrenaError> {
    read_record_with_limit(xml, DEFAULT_MAX_DOCUMENT_BYTES)
}

/// Reads an `<irena-record>` document, refusing input larger than `max_bytes`.
///
/// # Errors
///
/// As [`read_record`], plus [`IrenaError::TooLarge`].
pub fn read_record_with_limit(xml: &str, max_bytes: u64) -> Result<IrenaRecordV1, IrenaError> {
    check_size(xml, max_bytes)?;
    let mut reader = open(xml);
    let root = root_start(&mut reader, "irena-record")?;

    let mut issues = Vec::new();
    let mut attributes = Attributes::of(&reader, "irena-record", &root)?;
    let version = require(&mut attributes, "version", &mut issues);
    let kind = require(&mut attributes, "kind", &mut issues);
    let company = require(&mut attributes, "company", &mut issues);
    let supersedes = attributes.take("supersedes");
    attributes.finish(&reader)?;
    if !issues.is_empty() {
        return Err(IrenaError::invalid(issues));
    }
    let version = version.expect("checked");
    if version != RECORD_VERSION {
        return Err(IrenaError::UnsupportedVersion { found: version });
    }
    let kind = kind.expect("checked");
    let declared = RecordKindV1::parse(&kind).ok_or_else(|| {
        malformed_at(
            &reader,
            format!(
                "unknown record kind {kind:?}; expected company-genesis, identity, share-structure or voting-rules"
            ),
        )
    })?;
    let company = CompanyIdV1::new(company.expect("checked"))?;
    let supersedes = match supersedes {
        Some(text) => Some(prunella_core::TxId::from_hex(&text).map_err(|error| {
            IrenaError::invalid(vec![IssueV1::InvalidValue {
                element: "irena-record",
                attribute: "supersedes",
                value: text.clone(),
                reason: error.to_string(),
            }])
        })?),
        None => None,
    };

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
                return Err(malformed_at(
                    &reader,
                    format!("unexpected {} in <irena-record>", describe(&other)),
                ));
            }
        };
        let name = child.name().as_ref().to_owned();
        match name.as_str() {
            "notarisation" => {
                if notarisation.is_some() || body.is_some() {
                    return Err(malformed_at(
                        &reader,
                        "notarisation must appear once, before the element it attests to",
                    ));
                }
                notarisation = Some(read_notarisation(&reader, &child)?);
                if !is_empty {
                    expect_empty(&mut reader, "notarisation")?;
                }
            }
            "company-genesis" => {
                refuse_second_body(&reader, body.is_some())?;
                if is_empty {
                    return Err(malformed_at(&reader, "<company-genesis> must have content"));
                }
                body = Some(RecordBodyV1::CompanyGenesis(parse_company_genesis(
                    &mut reader,
                    &child,
                )?));
            }
            "identity" => {
                refuse_second_body(&reader, body.is_some())?;
                let mut issues = Vec::new();
                let identity = parse_identity(&reader, &child, &mut issues)?;
                if !is_empty {
                    expect_empty(&mut reader, "identity")?;
                }
                if !issues.is_empty() {
                    return Err(IrenaError::invalid(issues));
                }
                body = Some(RecordBodyV1::Identity(identity.expect("no issues")));
            }
            "share-structure" => {
                refuse_second_body(&reader, body.is_some())?;
                body = Some(RecordBodyV1::ShareStructure(parse_share_structure(
                    &mut reader,
                    &child,
                    is_empty,
                )?));
            }
            "voting-rules" => {
                refuse_second_body(&reader, body.is_some())?;
                if is_empty {
                    return Err(malformed_at(&reader, "<voting-rules> must have content"));
                }
                body = Some(RecordBodyV1::VotingRules(parse_voting_rules(
                    &mut reader,
                    &child,
                )?));
            }
            other => {
                return Err(malformed_at(
                    &reader,
                    format!("<irena-record> has an unknown child <{other}>"),
                ));
            }
        }
    }
    expect_eof(&mut reader)?;

    let notarisation = notarisation.ok_or_else(|| {
        IrenaError::invalid(vec![IssueV1::MissingElement {
            parent: "irena-record",
            element: "notarisation",
        }])
    })?;
    let body = body.ok_or_else(|| {
        IrenaError::invalid(vec![IssueV1::MissingElement {
            parent: "irena-record",
            element: declared.element(),
        }])
    })?;
    if body.kind() != declared {
        return Err(IrenaError::KindMismatch {
            declared,
            carried: body.kind(),
        });
    }
    Ok(IrenaRecordV1 {
        company,
        supersedes,
        notarisation,
        body,
    })
}

/// Reads a standalone `<company-genesis>` document.
///
/// # Errors
///
/// Returns [`IrenaError`] for anything that is not a valid genesis body.
pub fn read_company_genesis_document(xml: &str) -> Result<CompanyGenesisV1, IrenaError> {
    check_size(xml, DEFAULT_MAX_DOCUMENT_BYTES)?;
    let mut reader = open(xml);
    let root = root_start(&mut reader, "company-genesis")?;
    let genesis = parse_company_genesis(&mut reader, &root)?;
    expect_eof(&mut reader)?;
    Ok(genesis)
}

/// Reads a standalone `<identity>` document.
///
/// # Errors
///
/// Returns [`IrenaError`] for anything that is not a valid identity element.
pub fn read_identity_document(xml: &str) -> Result<IdentityV1, IrenaError> {
    check_size(xml, DEFAULT_MAX_DOCUMENT_BYTES)?;
    let mut reader = open(xml);
    let root = loop {
        match next_event(&mut reader)? {
            Event::Decl(_) | Event::Comment(_) | Event::Text(_) | Event::PI(_) => {}
            Event::Start(root) if root.name().as_ref() == "identity" => break (root, false),
            Event::Empty(root) if root.name().as_ref() == "identity" => break (root, true),
            other => {
                return Err(malformed_at(
                    &reader,
                    format!("expected an <identity> root, found {}", describe(&other)),
                ));
            }
        }
    };
    let mut issues = Vec::new();
    let identity = parse_identity(&reader, &root.0, &mut issues)?;
    if !root.1 {
        expect_empty(&mut reader, "identity")?;
    }
    expect_eof(&mut reader)?;
    if !issues.is_empty() {
        return Err(IrenaError::invalid(issues));
    }
    Ok(identity.expect("no issues"))
}

/// Reads a standalone `<share-structure>` document.
///
/// # Errors
///
/// Returns [`IrenaError`] for anything that is not a valid share register.
pub fn read_share_structure_document(xml: &str) -> Result<ShareStructureV1, IrenaError> {
    check_size(xml, DEFAULT_MAX_DOCUMENT_BYTES)?;
    let mut reader = open(xml);
    let root = loop {
        match next_event(&mut reader)? {
            Event::Decl(_) | Event::Comment(_) | Event::Text(_) | Event::PI(_) => {}
            Event::Start(root) if root.name().as_ref() == "share-structure" => {
                break (root, false);
            }
            Event::Empty(root) if root.name().as_ref() == "share-structure" => {
                break (root, true);
            }
            other => {
                return Err(malformed_at(
                    &reader,
                    format!(
                        "expected a <share-structure> root, found {}",
                        describe(&other)
                    ),
                ));
            }
        }
    };
    let structure = parse_share_structure(&mut reader, &root.0, root.1)?;
    expect_eof(&mut reader)?;
    Ok(structure)
}

/// Reads a standalone `<voting-rules>` document: Bornite's reader, Irena's error type.
///
/// # Errors
///
/// Returns [`IrenaError`] for anything Bornite refuses.
pub fn read_voting_rules_document(xml: &str) -> Result<bornite_rules::VotingRulesV1, IrenaError> {
    check_size(xml, DEFAULT_MAX_DOCUMENT_BYTES)?;
    Ok(bornite_xml::read_rules_document(xml)?)
}

/// Composes a record document around a body element supplied as text.
///
/// `body` must be a standalone `<company-genesis>`, `<share-structure>` or
/// `<voting-rules>` element matching `kind`. A leading XML declaration and surrounding
/// whitespace are removed; the element itself is embedded **byte for byte**. The
/// composed document is read back before it is returned, so what the ledger receives
/// is known to parse to exactly what was asked for.
///
/// The result begins with `<irena-record` and ends with `</irena-record>`, with no
/// declaration and no trailing newline, so it is exactly one element and Prunella's
/// XML transport nests it readably inside the block.
///
/// # Errors
///
/// Returns [`IrenaError`] if `body` is not a valid element of the right kind, the
/// notarisation is invalid, or the composed record does not read back.
pub fn compose_record(
    kind: RecordKindV1,
    company: &CompanyIdV1,
    supersedes: Option<prunella_core::TxId>,
    notarisation: &NotarisationV1,
    body: &str,
) -> Result<String, IrenaError> {
    notarisation.validate()?;
    let element = strip_declaration(body);
    // Validate the body on its own terms first, so an error names the body document
    // rather than the envelope around it.
    match kind {
        RecordKindV1::CompanyGenesis => {
            read_company_genesis_document(element)?;
        }
        RecordKindV1::Identity => {
            read_identity_document(element)?;
        }
        RecordKindV1::ShareStructure => {
            read_share_structure_document(element)?;
        }
        RecordKindV1::VotingRules => {
            read_voting_rules_document(element)?;
        }
    }

    let mut xml = String::new();
    xml.push_str(&format!(
        "<irena-record version=\"{RECORD_VERSION}\" kind=\"{}\" company=\"{}\"",
        kind.as_str(),
        company.as_str()
    ));
    if let Some(supersedes) = supersedes {
        xml.push_str(&format!(" supersedes=\"{supersedes}\""));
    }
    xml.push_str(">\n  ");
    xml.push_str(&write_notarisation(notarisation));
    xml.push_str("\n  ");
    xml.push_str(element);
    xml.push_str("\n</irena-record>");

    let read_back = read_record(&xml)?;
    if read_back.kind() != kind
        || &read_back.company != company
        || read_back.supersedes != supersedes
        || &read_back.notarisation != notarisation
    {
        return Err(IrenaError::Malformed {
            position: 0,
            detail: "composed record did not read back as written".to_owned(),
        });
    }
    Ok(xml)
}

// ---------------------------------------------------------------------------------
// Element parsers.
// ---------------------------------------------------------------------------------

/// Renders a notarisation as one empty `<notarisation …/>` element.
///
/// Public so another Irena document format can carry the same element, written and
/// read by exactly this code.
#[must_use]
pub fn write_notarisation(notarisation: &NotarisationV1) -> String {
    let mut xml = String::from("<notarisation");
    xml.push_str(&format!(" id=\"{}\"", notarisation.id.as_str()));
    xml.push_str(&format!(
        " name=\"{}\"",
        escape_attribute(&notarisation.name)
    ));
    if let Some(address) = &notarisation.address {
        xml.push_str(&format!(" address=\"{}\"", escape_attribute(address)));
    }
    xml.push_str(&format!(" at=\"{}\"", notarisation.at.as_str()));
    if let Some(statement) = &notarisation.statement {
        xml.push_str(&format!(" statement=\"{}\"", escape_attribute(statement)));
    }
    if let Some(digest) = notarisation.source_digest {
        xml.push_str(&format!(" source-digest=\"{digest}\""));
    }
    xml.push_str("/>");
    xml
}

/// Parses a `<notarisation>` element's attributes; the caller consumes its end tag if
/// it has one.
///
/// # Errors
///
/// Returns [`IrenaError`] with every attribute problem collected.
pub fn read_notarisation(
    reader: &XmlReader<'_>,
    child: &BytesStart<'_>,
) -> Result<NotarisationV1, IrenaError> {
    let mut issues = Vec::new();
    let mut attributes = Attributes::of(reader, "notarisation", child)?;
    let id = require(&mut attributes, "id", &mut issues);
    let name = require(&mut attributes, "name", &mut issues);
    let address = attributes.take("address");
    let at = require(&mut attributes, "at", &mut issues);
    let statement = attributes.take("statement");
    let source_digest = attributes.take("source-digest");
    attributes.finish(reader)?;

    let id = id.and_then(|text| collect(NotaryIdV1::new(text), &mut issues));
    let at = at.and_then(|text| collect(NotaryTimeV1::parse(&text), &mut issues));
    let source_digest = source_digest.and_then(|text| {
        collect(
            Hash::from_hex(&text).map_err(|error| {
                IrenaError::invalid(vec![IssueV1::InvalidValue {
                    element: "notarisation",
                    attribute: "source-digest",
                    value: text.clone(),
                    reason: error.to_string(),
                }])
            }),
            &mut issues,
        )
    });
    if let Some(name) = &name
        && name.trim().is_empty()
    {
        issues.push(IssueV1::InvalidValue {
            element: "notarisation",
            attribute: "name",
            value: name.clone(),
            reason: "must not be empty".to_owned(),
        });
    }
    if !issues.is_empty() {
        return Err(IrenaError::invalid(issues));
    }
    Ok(NotarisationV1 {
        id: id.expect("checked"),
        name: name.expect("checked"),
        address,
        at: at.expect("checked"),
        statement,
        source_digest,
    })
}

/// Parses an `<identity>` element's attributes, collecting issues.
///
/// Returns `None` when a required attribute is missing; the issue is recorded.
fn parse_identity(
    reader: &XmlReader<'_>,
    child: &BytesStart<'_>,
    issues: &mut Vec<IssueV1>,
) -> Result<Option<IdentityV1>, IrenaError> {
    let mut attributes = Attributes::of(reader, "identity", child)?;
    let name = require(&mut attributes, "name", issues);
    let jurisdiction = attributes.take("jurisdiction");
    let registered_number = attributes.take("registered-number");
    attributes.finish(reader)?;
    Ok(name.map(|name| {
        let identity = IdentityV1 {
            name,
            jurisdiction,
            registered_number,
        };
        if let Err(error) = identity.validate() {
            collect::<()>(Err(error), issues);
        }
        identity
    }))
}

/// Parses the body of a `<company-genesis>` element whose start tag has been read.
///
/// The genesis is the whole company: `<identity>`, an optional `<incorporation>`,
/// the `<share-structure>` and `<governance>` wrapping Bornite's `<voting-rules>`.
fn parse_company_genesis(
    reader: &mut XmlReader<'_>,
    start: &BytesStart<'_>,
) -> Result<CompanyGenesisV1, IrenaError> {
    Attributes::of(reader, "company-genesis", start)?.finish(reader)?;
    let mut issues = Vec::new();
    let mut identity: Option<IdentityV1> = None;
    let mut incorporation: Option<Option<Hash>> = None;
    let mut shares: Option<ShareStructureV1> = None;
    let mut rules: Option<bornite_rules::VotingRulesV1> = None;
    // Presence is tracked apart from the parsed value: an element that was there but
    // invalid is reported for what is wrong with it, not also as missing.
    let (mut shares_seen, mut governance_seen) = (false, false);
    let repeated = |issues: &mut Vec<IssueV1>, present: bool, element: &'static str| {
        if present {
            issues.push(IssueV1::RepeatedElement {
                parent: "company-genesis",
                element,
            });
        }
    };

    loop {
        let event = next_event(reader)?;
        let (child, is_empty) = match event {
            Event::Text(_) | Event::Comment(_) => continue,
            Event::Empty(child) => (child, true),
            Event::Start(child) => (child, false),
            Event::End(_) => break,
            other => {
                return Err(malformed_at(
                    reader,
                    format!("unexpected {} in <company-genesis>", describe(&other)),
                ));
            }
        };
        let name = child.name().as_ref().to_owned();
        match name.as_str() {
            "identity" => {
                repeated(&mut issues, identity.is_some(), "identity");
                if let Some(parsed) = parse_identity(reader, &child, &mut issues)? {
                    identity = Some(parsed);
                }
                if !is_empty {
                    expect_empty(reader, "identity")?;
                }
            }
            "incorporation" => {
                repeated(&mut issues, incorporation.is_some(), "incorporation");
                let mut attributes = Attributes::of(reader, "incorporation", &child)?;
                let digest = attributes.take("document-digest");
                attributes.finish(reader)?;
                let digest = digest.and_then(|text| {
                    collect(
                        Hash::from_hex(&text).map_err(|error| {
                            IrenaError::invalid(vec![IssueV1::InvalidValue {
                                element: "incorporation",
                                attribute: "document-digest",
                                value: text.clone(),
                                reason: error.to_string(),
                            }])
                        }),
                        &mut issues,
                    )
                });
                incorporation = Some(digest);
                if !is_empty {
                    expect_empty(reader, "incorporation")?;
                }
            }
            "share-structure" => {
                repeated(&mut issues, shares_seen, "share-structure");
                shares_seen = true;
                match parse_share_structure(reader, &child, is_empty) {
                    Ok(parsed) => shares = Some(parsed),
                    Err(IrenaError::Invalid { issues: found }) => issues.extend(found),
                    Err(other) => return Err(other),
                }
            }
            "governance" => {
                repeated(&mut issues, governance_seen, "governance");
                governance_seen = true;
                Attributes::of(reader, "governance", &child)?.finish(reader)?;
                if is_empty {
                    return Err(malformed_at(
                        reader,
                        "<governance> must hold <voting-rules>",
                    ));
                }
                match parse_governance(reader) {
                    Ok(parsed) => rules = Some(parsed),
                    Err(IrenaError::Invalid { issues: found }) => issues.extend(found),
                    Err(other) => return Err(other),
                }
            }
            other => {
                return Err(malformed_at(
                    reader,
                    format!("<company-genesis> has an unknown child <{other}>"),
                ));
            }
        }
    }

    for (present, element) in [
        (identity.is_some(), "identity"),
        (shares_seen, "share-structure"),
        (governance_seen, "governance"),
    ] {
        if !present {
            issues.push(IssueV1::MissingElement {
                parent: "company-genesis",
                element,
            });
        }
    }
    if !issues.is_empty() {
        return Err(IrenaError::invalid(issues));
    }
    Ok(CompanyGenesisV1 {
        identity: identity.expect("checked"),
        incorporation_digest: incorporation.flatten(),
        shares: shares.expect("checked"),
        rules: rules.expect("checked"),
    })
}

/// Parses the inside of `<governance>`: exactly one `<voting-rules>`, Bornite's.
fn parse_governance(
    reader: &mut XmlReader<'_>,
) -> Result<bornite_rules::VotingRulesV1, IrenaError> {
    let mut rules = None;
    loop {
        match next_event(reader)? {
            Event::Text(_) | Event::Comment(_) => {}
            Event::Start(child) if child.name().as_ref() == "voting-rules" => {
                if rules.is_some() {
                    return Err(IrenaError::invalid(vec![IssueV1::RepeatedElement {
                        parent: "governance",
                        element: "voting-rules",
                    }]));
                }
                rules = Some(parse_voting_rules(reader, &child)?);
            }
            Event::Empty(child) if child.name().as_ref() == "voting-rules" => {
                return Err(malformed_at(reader, "<voting-rules> must have content"));
            }
            Event::End(_) => break,
            other => {
                return Err(malformed_at(
                    reader,
                    format!("unexpected {} in <governance>", describe(&other)),
                ));
            }
        }
    }
    rules.ok_or_else(|| {
        IrenaError::invalid(vec![IssueV1::MissingElement {
            parent: "governance",
            element: "voting-rules",
        }])
    })
}

/// Parses the body of a `<share-structure>` element whose start tag has been read.
///
/// `is_empty` says the start tag was `<share-structure/>`, so there is no end tag.
fn parse_share_structure(
    reader: &mut XmlReader<'_>,
    start: &BytesStart<'_>,
    is_empty: bool,
) -> Result<ShareStructureV1, IrenaError> {
    Attributes::of(reader, "share-structure", start)?.finish(reader)?;
    let mut issues = Vec::new();
    let mut holders = Vec::new();

    if !is_empty {
        loop {
            let event = next_event(reader)?;
            let (child, child_is_empty) = match event {
                Event::Text(_) | Event::Comment(_) => continue,
                Event::Empty(child) => (child, true),
                Event::Start(child) => (child, false),
                Event::End(_) => break,
                other => {
                    return Err(malformed_at(
                        reader,
                        format!("unexpected {} in <share-structure>", describe(&other)),
                    ));
                }
            };
            let name = child.name().as_ref().to_owned();
            if name != "holder" {
                return Err(malformed_at(
                    reader,
                    format!("<share-structure> has an unknown child <{name}>"),
                ));
            }
            if holders.len() >= MAX_HOLDERS {
                return Err(IrenaError::invalid(vec![IssueV1::TooManyHolders {
                    limit: MAX_HOLDERS,
                }]));
            }
            if let Some(holder) = parse_holder(reader, &child, &mut issues)? {
                holders.push(holder);
            }
            if !child_is_empty {
                expect_empty(reader, "holder")?;
            }
        }
    }

    if !issues.is_empty() {
        return Err(IrenaError::invalid(issues));
    }
    ShareStructureV1::new(holders)
}

fn parse_holder(
    reader: &XmlReader<'_>,
    child: &BytesStart<'_>,
    issues: &mut Vec<IssueV1>,
) -> Result<Option<HolderV1>, IrenaError> {
    let mut attributes = Attributes::of(reader, "holder", child)?;
    let id = require(&mut attributes, "id", issues);
    let key = attributes.take("key");
    let name = attributes.take("name");
    let shares = require(&mut attributes, "shares", issues);
    attributes.finish(reader)?;

    let id = id.and_then(|text| {
        collect(
            VoterIdV1::new(text.clone()).map_err(|error| {
                IrenaError::invalid(vec![IssueV1::InvalidValue {
                    element: "holder",
                    attribute: "id",
                    value: text,
                    reason: error.to_string(),
                }])
            }),
            issues,
        )
    });
    let key = match key {
        None => None,
        Some(text) => collect(
            PublicKey::from_hex(&text).map_err(|error| {
                IrenaError::invalid(vec![IssueV1::InvalidValue {
                    element: "holder",
                    attribute: "key",
                    value: text.clone(),
                    reason: error.to_string(),
                }])
            }),
            issues,
        ),
    };
    let shares = shares.and_then(|text| {
        collect(
            parse_u64(&text).ok_or_else(|| {
                IrenaError::invalid(vec![IssueV1::InvalidValue {
                    element: "holder",
                    attribute: "shares",
                    value: text.clone(),
                    reason: "must be a decimal integer from 0 to 18446744073709551615".to_owned(),
                }])
            }),
            issues,
        )
    });
    Ok(match (id, shares) {
        (Some(id), Some(shares)) => Some(HolderV1 {
            id,
            key,
            name,
            shares,
        }),
        _ => None,
    })
}

// ---------------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------------

/// Takes a required attribute, recording an Irena issue if absent.
fn require(
    attributes: &mut Attributes,
    name: &'static str,
    issues: &mut Vec<IssueV1>,
) -> Option<String> {
    let mut bornite_issues = Vec::new();
    let value = attributes.require(name, &mut bornite_issues);
    issues.extend(bornite_issues.into_iter().map(IssueV1::Xml));
    value
}

/// Keeps a value, or moves its issues into the running list.
///
/// Every constructor handed to this returns only [`IrenaError::Invalid`]; any other
/// error would be a programming mistake, and is surfaced as an issue rather than lost.
fn collect<T>(result: Result<T, IrenaError>, issues: &mut Vec<IssueV1>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            match error {
                IrenaError::Invalid { issues: found } => issues.extend(found),
                other => issues.push(IssueV1::InvalidValue {
                    element: "irena-record",
                    attribute: "?",
                    value: String::new(),
                    reason: other.to_string(),
                }),
            }
            None
        }
    }
}

/// Strict decimal: ASCII digits only, no sign, no whitespace, no leading `+`.
fn parse_u64(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn refuse_second_body(reader: &XmlReader<'_>, seen: bool) -> Result<(), IrenaError> {
    if seen {
        return Err(malformed_at(reader, "a record carries exactly one element"));
    }
    Ok(())
}

fn malformed_at(reader: &XmlReader<'_>, detail: impl Into<String>) -> IrenaError {
    malformed(reader, detail).into()
}

fn check_size(xml: &str, max_bytes: u64) -> Result<(), IrenaError> {
    let found = xml.len() as u64;
    if found > max_bytes {
        return Err(IrenaError::TooLarge {
            found,
            limit: max_bytes,
        });
    }
    Ok(())
}

/// Removes a leading XML declaration and surrounding whitespace, leaving the element.
/// Removes a leading XML declaration and surrounding whitespace, leaving the element.
///
/// This is exactly what [`compose_record`] does to a body before embedding it, so a
/// caller that digests or stores a body element normalises it the same way and gets
/// the same bytes the ledger will hold.
#[must_use]
pub fn normalise_body(text: &str) -> &str {
    strip_declaration(text)
}

fn strip_declaration(text: &str) -> &str {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("<?xml")
        && let Some(end) = rest.find("?>")
    {
        return rest[end + 2..].trim();
    }
    trimmed
}

/// Escapes text for use inside a double-quoted attribute value.
#[must_use]
pub fn escape_attribute(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
