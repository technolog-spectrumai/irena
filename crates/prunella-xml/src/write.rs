//! Rendering a document as XML.
//!
//! The XML is a faithful rendering of values that already exist: every hash written
//! here was derived through `prunella-canonical` before it reached this module. No
//! chain hash is ever computed from XML text.

use crate::document::{ChainDocument, DocumentKind, FORMAT_VERSION, XML_NAMESPACE};
use crate::error::XmlError;
use crate::payload::PayloadEncoding;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

/// Renders a document as an XML string.
///
/// # Errors
///
/// Returns [`XmlError::Write`] if the underlying writer fails, which cannot happen for
/// the in-memory buffer used here.
pub fn write_document(document: &ChainDocument) -> Result<String, XmlError> {
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    render(&mut writer, document)?;
    String::from_utf8(writer.into_inner())
        .map_err(|error| XmlError::Write(format!("rendered document is not UTF-8: {error}")))
}

fn render(writer: &mut Writer<Vec<u8>>, document: &ChainDocument) -> Result<(), XmlError> {
    write_event(
        writer,
        Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)),
    )?;

    let mut root = BytesStart::new("prunella-chain");
    root.push_attribute(("xmlns", XML_NAMESPACE));
    root.push_attribute(("format-version", FORMAT_VERSION.to_string().as_str()));
    root.push_attribute(("kind", document.kind.as_str()));
    root.push_attribute(("network-id", document.network_id.as_str()));
    root.push_attribute(("genesis-hash", document.genesis_hash.to_hex().as_str()));
    root.push_attribute(("range-start", document.range_start.to_string().as_str()));
    root.push_attribute(("range-end", document.range_end.to_string().as_str()));
    root.push_attribute(("block-count", document.block_count().to_string().as_str()));
    root.push_attribute((
        "exported-at-millis",
        document.exported_at_millis.to_string().as_str(),
    ));
    write_event(writer, Event::Start(root))?;

    if let Some(projection) = &document.projection {
        let mut element = BytesStart::new("projection");
        element.push_attribute(("filter-namespace", projection.filter_namespace.as_str()));
        write_event(writer, Event::Empty(element))?;
    }

    for entry in &document.blocks {
        render_block(writer, entry, document.kind)?;
    }

    write_event(writer, Event::End(BytesEnd::new("prunella-chain")))
}

fn render_block(
    writer: &mut Writer<Vec<u8>>,
    entry: &crate::document::DocumentBlock,
    kind: DocumentKind,
) -> Result<(), XmlError> {
    let header = &entry.block.header;

    let mut block = BytesStart::new("block");
    block.push_attribute(("height", header.height.to_string().as_str()));
    block.push_attribute(("hash", entry.declared_hash.to_hex().as_str()));
    write_event(writer, Event::Start(block))?;

    let mut header_element = BytesStart::new("header");
    header_element.push_attribute(("version", header.version.to_string().as_str()));
    header_element.push_attribute(("previous-hash", header.previous_hash.to_hex().as_str()));
    header_element.push_attribute(("tx-root", header.tx_root.to_hex().as_str()));
    header_element.push_attribute(("tx-count", header.tx_count.to_string().as_str()));
    header_element.push_attribute((
        "timestamp-millis",
        header.timestamp_millis.to_string().as_str(),
    ));
    write_event(writer, Event::Empty(header_element))?;

    let mut transactions = BytesStart::new("transactions");
    if kind == DocumentKind::Projection {
        // A projection carries fewer transactions than the header commits to. Saying so
        // explicitly means a reader never has to infer it from a count mismatch.
        transactions.push_attribute((
            "included-count",
            entry.block.transactions.len().to_string().as_str(),
        ));
    }
    if entry.block.transactions.is_empty() {
        write_event(writer, Event::Empty(transactions))?;
    } else {
        write_event(writer, Event::Start(transactions))?;
        for (index, transaction) in entry.block.transactions.iter().enumerate() {
            render_transaction(writer, index, transaction)?;
        }
        write_event(writer, Event::End(BytesEnd::new("transactions")))?;
    }

    write_event(writer, Event::End(BytesEnd::new("block")))
}

fn render_transaction(
    writer: &mut Writer<Vec<u8>>,
    index: usize,
    transaction: &prunella_core::Transaction,
) -> Result<(), XmlError> {
    let mut element = BytesStart::new("transaction");
    element.push_attribute(("index", index.to_string().as_str()));
    element.push_attribute(("id", transaction.id.to_hex().as_str()));
    element.push_attribute(("namespace", transaction.namespace.as_str()));
    element.push_attribute((
        "schema-version",
        transaction.schema_version.to_string().as_str(),
    ));
    element.push_attribute(("nonce", transaction.nonce.to_string().as_str()));
    write_event(writer, Event::Start(element))?;

    write_binary(writer, "signer", &transaction.signer.to_bytes())?;
    write_payload(writer, &transaction.payload)?;
    write_binary(writer, "signature", &transaction.signature.to_bytes())?;

    write_event(writer, Event::End(BytesEnd::new("transaction")))
}

/// Writes a payload as a nested element when it is one, and as base64 otherwise.
///
/// A nested payload is written byte for byte: the stored bytes go straight into the
/// output, unescaped and unindented, so the importer can cut exactly them back out.
/// An empty payload is an empty element, as in version 1, and carries no encoding.
fn write_payload(writer: &mut Writer<Vec<u8>>, payload: &[u8]) -> Result<(), XmlError> {
    if payload.is_empty() {
        return write_event(writer, Event::Empty(BytesStart::new("payload")));
    }
    let encoding = PayloadEncoding::choose(payload);
    let mut element = BytesStart::new("payload");
    element.push_attribute(("encoding", encoding.as_str()));
    write_event(writer, Event::Start(element))?;
    match encoding {
        PayloadEncoding::Base64 => {
            write_event(writer, Event::Text(BytesText::new(&BASE64.encode(payload))))?;
        }
        PayloadEncoding::Xml => {
            // `choose` established the bytes are UTF-8 and one well-formed element.
            let text = core::str::from_utf8(payload).map_err(|error| {
                XmlError::Write(format!("nested payload is not UTF-8: {error}"))
            })?;
            write_event(writer, Event::Text(BytesText::from_escaped(text)))?;
        }
    }
    write_event(writer, Event::End(BytesEnd::new("payload")))
}

/// Writes a byte string as base64 text.
///
/// An empty byte string is written as an empty element rather than empty text, so the
/// distinction between "no payload" and "payload of zero bytes" never depends on
/// whitespace handling.
fn write_binary(writer: &mut Writer<Vec<u8>>, name: &str, bytes: &[u8]) -> Result<(), XmlError> {
    if bytes.is_empty() {
        return write_event(writer, Event::Empty(BytesStart::new(name)));
    }
    write_event(writer, Event::Start(BytesStart::new(name)))?;
    write_event(writer, Event::Text(BytesText::new(&BASE64.encode(bytes))))?;
    write_event(writer, Event::End(BytesEnd::new(name)))
}

fn write_event(writer: &mut Writer<Vec<u8>>, event: Event<'_>) -> Result<(), XmlError> {
    writer
        .write_event(event)
        .map_err(|error| XmlError::Write(error.to_string()))
}
