//! Parsing a document from XML.
//!
//! The parser is strict on purpose. An unknown element, an unknown attribute, a
//! missing attribute or a value outside its documented form is an error, not something
//! to skip: a chain backup that silently ignores the parts it does not recognise is
//! not a backup.
//!
//! Nothing read here is trusted as authoritative. Blocks are rebuilt into core types
//! and every hash is re-derived through `prunella-canonical`; the hashes the document
//! declares are kept only so a mismatch can be reported.

use crate::document::{
    ChainDocument, DocumentBlock, DocumentKind, Projection, SUPPORTED_FORMAT_VERSIONS,
    XML_NAMESPACE, XML_NAMESPACE_V1, namespace_for_version,
};
use crate::error::XmlError;
use crate::payload::{PayloadEncoding, is_single_element};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use prunella_core::{
    Block, BlockHeader, BlockHeight, Hash, Namespace, NetworkId, PublicKey, SchemaVersion,
    Signature, Transaction, TxId,
};
use quick_xml::NsReader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;

/// Default limit on document size, in bytes.
///
/// A bound exists so that a hostile or truncated file cannot be turned into unbounded
/// allocation before a single rule has been applied.
pub const DEFAULT_MAX_DOCUMENT_BYTES: u64 = 1 << 30;

/// Largest number of blocks a document may declare or carry.
///
/// A separate bound from the byte limit because `block-count` is an attribute the
/// document supplies: without this, a few bytes of untrusted input could ask a reader
/// to reserve capacity for billions of blocks before one had been parsed.
pub const MAX_BLOCKS: u64 = 16_000_000;

/// Largest number of transactions one block may carry in a document.
///
/// The protocol's own limit is `u32::MAX` (PROTOCOL_V1.md §11). This is tighter because
/// a document is untrusted input and each transaction costs at least a key and a
/// signature. It is import policy, not a protocol rule: two implementations with
/// different bounds still agree on every hash.
pub const MAX_TRANSACTIONS_PER_BLOCK: usize = 4_000_000;

/// Largest base64 content accepted for one element, in characters.
///
/// Bounds the buffer for a single `<payload>` independently of the whole-document
/// limit, so one enormous element cannot force a large allocation from a document that
/// is otherwise small.
pub const MAX_BINARY_FIELD_CHARS: usize = 96 * 1024 * 1024;

/// Parses a document with the default size limit.
///
/// # Errors
///
/// Returns [`XmlError::Malformed`], [`XmlError::UnsupportedFormatVersion`],
/// [`XmlError::TooLarge`] or [`XmlError::Core`] if the input is not a valid document.
pub fn read_document(xml: &str) -> Result<ChainDocument, XmlError> {
    read_document_with_limit(xml, DEFAULT_MAX_DOCUMENT_BYTES)
}

/// Parses a document, refusing input larger than `max_bytes`.
///
/// # Errors
///
/// As [`read_document`].
pub fn read_document_with_limit(xml: &str, max_bytes: u64) -> Result<ChainDocument, XmlError> {
    let length = xml.len() as u64;
    if length > max_bytes {
        return Err(XmlError::TooLarge {
            found: length,
            limit: max_bytes,
        });
    }
    Parser {
        reader: NsReader::from_str(xml),
        source: xml,
        namespace: None,
        format_version: None,
        root_namespace: None,
    }
    .document()
}

struct Parser<'a> {
    reader: NsReader<&'a [u8]>,
    /// The whole input, so a nested payload can be cut out of it byte for byte.
    source: &'a str,
    /// The document's namespace once the root element has been seen.
    namespace: Option<&'static str>,
    /// The declared format version once the root element has been seen.
    format_version: Option<u32>,
    /// The namespace the root element resolved to, checked once the version is known.
    root_namespace: Option<String>,
}

impl<'a> Parser<'a> {
    fn document(mut self) -> Result<ChainDocument, XmlError> {
        let root = loop {
            match self.event()? {
                Event::Decl(_) | Event::Text(_) | Event::Comment(_) => {}
                Event::Start(element) => break element,
                Event::Empty(_) => {
                    return Err(self.malformed(
                        "a prunella-chain document needs at least a root element with content",
                    ));
                }
                Event::Eof => return Err(self.malformed("document is empty")),
                other => return Err(self.malformed(format!("unexpected {}", describe(&other)))),
            }
        };
        self.expect_name(&root, "prunella-chain")?;

        let mut document = self.root_attributes(&root)?;
        let namespace = namespace_for_version(document.format_version)
            .ok_or_else(|| self.malformed("unreachable: version accepted without a namespace"))?;
        if self.root_namespace.as_deref() != Some(namespace) {
            return Err(self.malformed(format!(
                "a format-version {} document must be in the {namespace} namespace, found {}",
                document.format_version,
                self.root_namespace.as_deref().unwrap_or("no namespace")
            )));
        }
        self.namespace = Some(namespace);
        self.format_version = Some(document.format_version);
        let mut previous: Option<BlockHeight> = None;

        loop {
            match self.event()? {
                Event::Text(_) | Event::Comment(_) => {}
                Event::Empty(element) if self.is_named(&element, "projection")? => {
                    document.projection = Some(self.projection(&element)?);
                }
                Event::Start(element) if self.is_named(&element, "block")? => {
                    if document.blocks.len() as u64 >= MAX_BLOCKS {
                        return Err(XmlError::TooManyBlocks {
                            found: MAX_BLOCKS + 1,
                            limit: MAX_BLOCKS,
                        });
                    }
                    let entry = self.block(&element, &document.network_id)?;
                    let height = entry.block.header.height;
                    if let Some(previous_height) = previous
                        && height.value() != previous_height.value() + 1
                    {
                        return Err(XmlError::NonContiguousDocument {
                            height,
                            previous: previous_height,
                        });
                    }
                    previous = Some(height);
                    document.blocks.push(entry);
                }
                Event::End(_) => break,
                other => return Err(self.malformed(format!("unexpected {}", describe(&other)))),
            }
        }

        self.expect_eof()?;
        self.finish(document, previous)
    }

    /// Cross-checks the document-level declarations against the blocks actually present.
    fn finish(
        &self,
        mut document: ChainDocument,
        last_height: Option<BlockHeight>,
    ) -> Result<ChainDocument, XmlError> {
        match (document.kind, &document.projection) {
            (DocumentKind::Projection, None) => {
                return Err(self.malformed(
                    "a projection document must carry a projection element naming its filter",
                ));
            }
            (DocumentKind::Full | DocumentKind::Range, Some(_)) => {
                return Err(
                    self.malformed("only a projection document may carry a projection element")
                );
            }
            _ => {}
        }

        if let Some(last) = last_height {
            let first = document.blocks[0].block.header.height;
            if first != document.range_start || last != document.range_end {
                return Err(self.malformed(format!(
                    "document declares heights {}..={} but carries {first}..={last}",
                    document.range_start, document.range_end
                )));
            }
            if document.kind == DocumentKind::Full && !first.is_genesis() {
                return Err(self.malformed(format!(
                    "a full document must start at height 0, this one starts at {first}"
                )));
            }
        } else if document.blocks.is_empty() && document.range_start <= document.range_end {
            return Err(self.malformed(format!(
                "document declares heights {}..={} but carries no blocks",
                document.range_start, document.range_end
            )));
        }

        document.blocks.shrink_to_fit();
        Ok(document)
    }

    fn root_attributes(&self, element: &BytesStart<'_>) -> Result<ChainDocument, XmlError> {
        let mut format_version = None;
        let mut kind = None;
        let mut network_id = None;
        let mut genesis_hash = None;
        let mut range_start = None;
        let mut range_end = None;
        let mut block_count = None;
        let mut exported_at_millis = None;

        for (name, value) in self.attributes(element)? {
            match name.as_str() {
                "xmlns" => {
                    if value != XML_NAMESPACE && value != XML_NAMESPACE_V1 {
                        return Err(self.malformed(format!(
                            "document namespace is {value:?}, expected {XML_NAMESPACE:?} or {XML_NAMESPACE_V1:?}"
                        )));
                    }
                }
                "format-version" => format_version = Some(self.integer::<u32>(&name, &value)?),
                "kind" => {
                    kind = Some(DocumentKind::parse(&value).ok_or_else(|| {
                        self.malformed(format!("unknown document kind {value:?}"))
                    })?);
                }
                "network-id" => network_id = Some(NetworkId::new(value)?),
                "genesis-hash" => genesis_hash = Some(Hash::from_hex(&value)?),
                "range-start" => range_start = Some(BlockHeight(self.integer(&name, &value)?)),
                "range-end" => range_end = Some(BlockHeight(self.integer(&name, &value)?)),
                "block-count" => block_count = Some(self.integer::<u64>(&name, &value)?),
                "exported-at-millis" => exported_at_millis = Some(self.integer(&name, &value)?),
                other => return Err(self.unknown_attribute("prunella-chain", other)),
            }
        }

        let format_version = self.require(format_version, "prunella-chain", "format-version")?;
        if !SUPPORTED_FORMAT_VERSIONS.contains(&format_version) {
            return Err(XmlError::UnsupportedFormatVersion {
                found: format_version,
                supported: SUPPORTED_FORMAT_VERSIONS.to_vec(),
            });
        }

        let range_start = self.require(range_start, "prunella-chain", "range-start")?;
        let range_end = self.require(range_end, "prunella-chain", "range-end")?;
        if range_start > range_end {
            return Err(self.malformed(format!(
                "document declares an empty range {range_start}..={range_end}"
            )));
        }
        let declared_span = range_end
            .value()
            .checked_sub(range_start.value())
            .and_then(|span| span.checked_add(1))
            .ok_or_else(|| self.malformed("declared height range is larger than u64"))?;
        let block_count = self.require(block_count, "prunella-chain", "block-count")?;
        if block_count != declared_span {
            return Err(self.malformed(format!(
                "document declares {block_count} block(s) but a height range of {declared_span}"
            )));
        }
        if block_count > MAX_BLOCKS {
            return Err(XmlError::TooManyBlocks {
                found: block_count,
                limit: MAX_BLOCKS,
            });
        }
        // Reserve only what a document of this declared size could plausibly hold. The
        // count is untrusted input, so it caps the reservation rather than setting it.
        let reserve = usize::try_from(block_count.min(4096)).unwrap_or(0);

        Ok(ChainDocument {
            format_version,
            kind: self.require(kind, "prunella-chain", "kind")?,
            network_id: self.require(network_id, "prunella-chain", "network-id")?,
            genesis_hash: self.require(genesis_hash, "prunella-chain", "genesis-hash")?,
            range_start,
            range_end,
            exported_at_millis: self.require(
                exported_at_millis,
                "prunella-chain",
                "exported-at-millis",
            )?,
            projection: None,
            blocks: Vec::with_capacity(reserve),
        })
    }

    fn projection(&self, element: &BytesStart<'_>) -> Result<Projection, XmlError> {
        let mut filter_namespace = None;
        for (name, value) in self.attributes(element)? {
            match name.as_str() {
                "filter-namespace" => filter_namespace = Some(Namespace::new(value)?),
                other => return Err(self.unknown_attribute("projection", other)),
            }
        }
        Ok(Projection {
            filter_namespace: self.require(filter_namespace, "projection", "filter-namespace")?,
        })
    }

    fn block(
        &mut self,
        element: &BytesStart<'_>,
        network_id: &NetworkId,
    ) -> Result<DocumentBlock, XmlError> {
        let mut height = None;
        let mut declared_hash = None;
        for (name, value) in self.attributes(element)? {
            match name.as_str() {
                "height" => height = Some(BlockHeight(self.integer(&name, &value)?)),
                "hash" => declared_hash = Some(Hash::from_hex(&value)?),
                other => return Err(self.unknown_attribute("block", other)),
            }
        }
        let height = self.require(height, "block", "height")?;
        let declared_hash = self.require(declared_hash, "block", "hash")?;

        let mut header: Option<BlockHeader> = None;
        let mut transactions: Option<Vec<Transaction>> = None;

        loop {
            match self.event()? {
                Event::Text(_) | Event::Comment(_) => {}
                Event::Empty(child) if self.is_named(&child, "header")? => {
                    header = Some(self.header(&child, height, network_id)?);
                }
                Event::Empty(child) if self.is_named(&child, "transactions")? => {
                    self.transactions_attributes(&child)?;
                    transactions = Some(Vec::new());
                }
                Event::Start(child) if self.is_named(&child, "transactions")? => {
                    self.transactions_attributes(&child)?;
                    transactions = Some(self.transactions()?);
                }
                Event::End(_) => break,
                other => return Err(self.malformed(format!("unexpected {}", describe(&other)))),
            }
        }

        let header = self.require(header, "block", "header")?;
        let transactions = transactions.ok_or_else(|| {
            self.malformed(format!(
                "block at height {height} has no transactions element"
            ))
        })?;

        Ok(DocumentBlock {
            block: Block {
                header,
                transactions,
            },
            declared_hash,
        })
    }

    fn header(
        &self,
        element: &BytesStart<'_>,
        height: BlockHeight,
        network_id: &NetworkId,
    ) -> Result<BlockHeader, XmlError> {
        let mut version = None;
        let mut previous_hash = None;
        let mut tx_root = None;
        let mut tx_count = None;
        let mut timestamp_millis = None;

        for (name, value) in self.attributes(element)? {
            match name.as_str() {
                "version" => version = Some(self.integer(&name, &value)?),
                "previous-hash" => previous_hash = Some(Hash::from_hex(&value)?),
                "tx-root" => tx_root = Some(Hash::from_hex(&value)?),
                "tx-count" => tx_count = Some(self.integer(&name, &value)?),
                "timestamp-millis" => timestamp_millis = Some(self.integer(&name, &value)?),
                other => return Err(self.unknown_attribute("header", other)),
            }
        }

        Ok(BlockHeader {
            version: self.require(version, "header", "version")?,
            network_id: network_id.clone(),
            height,
            previous_hash: self.require(previous_hash, "header", "previous-hash")?,
            tx_root: self.require(tx_root, "header", "tx-root")?,
            tx_count: self.require(tx_count, "header", "tx-count")?,
            timestamp_millis: self.require(timestamp_millis, "header", "timestamp-millis")?,
        })
    }

    /// The transactions element carries a count only in a projection, where it records
    /// how many of the block's transactions survived the filter.
    fn transactions_attributes(&self, element: &BytesStart<'_>) -> Result<(), XmlError> {
        for (name, _) in self.attributes(element)? {
            if name != "included-count" {
                return Err(self.unknown_attribute("transactions", &name));
            }
        }
        Ok(())
    }

    fn transactions(&mut self) -> Result<Vec<Transaction>, XmlError> {
        let mut transactions = Vec::new();
        loop {
            match self.event()? {
                Event::Text(_) | Event::Comment(_) => {}
                Event::Start(child) if self.is_named(&child, "transaction")? => {
                    if transactions.len() >= MAX_TRANSACTIONS_PER_BLOCK {
                        return Err(XmlError::TooManyTransactions {
                            limit: MAX_TRANSACTIONS_PER_BLOCK,
                        });
                    }
                    let expected_index = u32::try_from(transactions.len())
                        .map_err(|_| self.malformed("too many transactions in one block"))?;
                    transactions.push(self.transaction(&child, expected_index)?);
                }
                Event::End(_) => break,
                other => return Err(self.malformed(format!("unexpected {}", describe(&other)))),
            }
        }
        Ok(transactions)
    }

    fn transaction(
        &mut self,
        element: &BytesStart<'_>,
        expected_index: u32,
    ) -> Result<Transaction, XmlError> {
        let mut index = None;
        let mut id = None;
        let mut namespace = None;
        let mut schema_version = None;
        let mut nonce = None;

        for (name, value) in self.attributes(element)? {
            match name.as_str() {
                "index" => index = Some(self.integer::<u32>(&name, &value)?),
                "id" => id = Some(TxId::from_hex(&value)?),
                "namespace" => namespace = Some(Namespace::new(value)?),
                "schema-version" => {
                    schema_version = Some(SchemaVersion(self.integer(&name, &value)?))
                }
                "nonce" => nonce = Some(self.integer(&name, &value)?),
                other => return Err(self.unknown_attribute("transaction", other)),
            }
        }

        let index = self.require(index, "transaction", "index")?;
        if index != expected_index {
            return Err(self.malformed(format!(
                "transaction indexes must ascend from zero: expected {expected_index}, found {index}"
            )));
        }

        let mut signer: Option<PublicKey> = None;
        let mut payload: Option<Vec<u8>> = None;
        let mut signature: Option<Signature> = None;

        loop {
            match self.event()? {
                Event::Text(_) | Event::Comment(_) => {}
                Event::Empty(child) => match self.child_name(&child)? {
                    "signer" => {
                        self.no_attributes(&child, "signer")?;
                        signer = Some(PublicKey::from_bytes(self.fixed(&[])?));
                    }
                    "payload" => {
                        if self.payload_encoding(&child)? == PayloadEncoding::Xml {
                            return Err(self.malformed(
                                "an xml-encoded payload cannot be empty: it must hold one element",
                            ));
                        }
                        payload = Some(Vec::new());
                    }
                    "signature" => {
                        self.no_attributes(&child, "signature")?;
                        signature = Some(Signature::from_bytes(self.fixed(&[])?));
                    }
                    other => return Err(self.unknown_element("transaction", other)),
                },
                Event::Start(child) => match self.child_name(&child)? {
                    "signer" => {
                        self.no_attributes(&child, "signer")?;
                        let bytes = self.binary_text("signer")?;
                        signer = Some(PublicKey::from_bytes(self.fixed(&bytes)?));
                    }
                    "payload" => {
                        payload = Some(match self.payload_encoding(&child)? {
                            PayloadEncoding::Base64 => self.binary_text("payload")?,
                            PayloadEncoding::Xml => self.nested_payload()?,
                        });
                    }
                    "signature" => {
                        self.no_attributes(&child, "signature")?;
                        let bytes = self.binary_text("signature")?;
                        signature = Some(Signature::from_bytes(self.fixed(&bytes)?));
                    }
                    other => return Err(self.unknown_element("transaction", other)),
                },
                Event::End(_) => break,
                other => return Err(self.malformed(format!("unexpected {}", describe(&other)))),
            }
        }

        Ok(Transaction {
            id: self.require(id, "transaction", "id")?,
            namespace: self.require(namespace, "transaction", "namespace")?,
            schema_version: self.require(schema_version, "transaction", "schema-version")?,
            payload: payload.ok_or_else(|| self.missing_element("transaction", "payload"))?,
            signer: signer.ok_or_else(|| self.missing_element("transaction", "signer"))?,
            nonce: self.require(nonce, "transaction", "nonce")?,
            signature: signature.ok_or_else(|| self.missing_element("transaction", "signature"))?,
        })
    }

    /// The `encoding` attribute of a payload element, defaulting to base64.
    ///
    /// Version 1 documents have no such attribute; one appearing there is an unknown
    /// attribute, as it was to a version 1 reader.
    fn payload_encoding(&self, element: &BytesStart<'_>) -> Result<PayloadEncoding, XmlError> {
        let mut encoding = None;
        for (name, value) in self.attributes(element)? {
            match name.as_str() {
                "encoding" if self.format_version != Some(1) => {
                    encoding = Some(PayloadEncoding::parse(&value).ok_or_else(|| {
                        self.malformed(format!("unknown payload encoding {value:?}"))
                    })?);
                }
                other => return Err(self.unknown_attribute("payload", other)),
            }
        }
        Ok(encoding.unwrap_or(PayloadEncoding::Base64))
    }

    fn no_attributes(&self, element: &BytesStart<'_>, name: &str) -> Result<(), XmlError> {
        if let Some((attribute, _)) = self.attributes(element)?.into_iter().next() {
            return Err(self.unknown_attribute(name, &attribute));
        }
        Ok(())
    }

    /// Reads a nested payload: the exact source bytes of the one element inside
    /// `<payload encoding="xml">`, and consumes the payload's end tag.
    ///
    /// Nothing is rebuilt from parse events, because a parser normalises line endings,
    /// attribute quoting and entity spelling and the chain committed to the original
    /// bytes. The reader is used only to find where the element starts and ends; the
    /// payload is the slice of the input between those positions. The nested element
    /// is not required to be in the document's namespace, or in any namespace: it is
    /// the application's, and Prunella does not look inside it.
    ///
    /// Whitespace between the payload tags and the element is permitted and dropped,
    /// so a reformatted document still yields the same bytes. Anything else beside the
    /// element — text, a comment, a second element — is refused: the payload would be
    /// ambiguous.
    fn nested_payload(&mut self) -> Result<Vec<u8>, XmlError> {
        let mut element: Option<(usize, usize)> = None;
        let mut depth = 0usize;
        let mut start = 0usize;
        loop {
            let before = self.position()?;
            let event = self.raw_event()?;
            match event {
                Event::Start(_) => {
                    if depth == 0 {
                        if element.is_some() {
                            return Err(self.malformed(
                                "an xml-encoded payload must hold exactly one element, found a second",
                            ));
                        }
                        start = before;
                    }
                    depth += 1;
                }
                Event::Empty(_) => {
                    if depth == 0 {
                        if element.is_some() {
                            return Err(self.malformed(
                                "an xml-encoded payload must hold exactly one element, found a second",
                            ));
                        }
                        element = Some((before, self.position()?));
                    }
                }
                Event::End(_) => {
                    if depth == 0 {
                        // The payload's own end tag.
                        break;
                    }
                    depth -= 1;
                    if depth == 0 {
                        element = Some((start, self.position()?));
                    }
                }
                Event::Text(text) if depth == 0 => {
                    if !text
                        .xml10_content()
                        .trim_matches(|c: char| c.is_ascii_whitespace())
                        .is_empty()
                    {
                        return Err(self.malformed(
                            "an xml-encoded payload may hold only one element and whitespace, found text",
                        ));
                    }
                }
                Event::Text(_)
                | Event::CData(_)
                | Event::Comment(_)
                | Event::GeneralRef(_)
                | Event::PI(_)
                    if depth > 0 => {}
                Event::Eof => return Err(self.malformed("document ends inside a payload")),
                other => {
                    return Err(self.malformed(format!(
                        "an xml-encoded payload may hold only one element, found {}",
                        describe(&other)
                    )));
                }
            }
            if let Some((_, end)) = element
                && end.saturating_sub(start) > MAX_BINARY_FIELD_CHARS
            {
                return Err(XmlError::FieldTooLarge {
                    element: "payload".to_owned(),
                    found: end - start,
                    limit: MAX_BINARY_FIELD_CHARS,
                });
            }
        }
        let (from, to) = element.ok_or_else(|| {
            self.malformed("an xml-encoded payload must hold exactly one element, found none")
        })?;
        let bytes = self
            .source
            .get(from..to)
            .ok_or_else(|| self.malformed("payload element positions fall outside the input"))?;
        if !is_single_element(bytes) {
            return Err(self.malformed(
                "the bytes cut out for an xml-encoded payload are not one well-formed element",
            ));
        }
        Ok(bytes.as_bytes().to_vec())
    }

    /// The reader's byte offset into the input.
    fn position(&self) -> Result<usize, XmlError> {
        usize::try_from(self.reader.buffer_position())
            .map_err(|_| self.malformed("input position does not fit in memory"))
    }

    /// Reads the base64 text of an element and consumes its end tag.
    ///
    /// Whitespace is stripped before decoding, because the writer indents the document
    /// and XSD base64 content permits whitespace. The decoded bytes are therefore
    /// identical regardless of how the document was laid out.
    fn binary_text(&mut self, name: &str) -> Result<Vec<u8>, XmlError> {
        let mut text = String::new();
        loop {
            match self.event()? {
                Event::Text(chunk) => text.push_str(&chunk.xml10_content()),
                Event::CData(chunk) => text.push_str(chunk.as_ref()),
                Event::Comment(_) => {}
                Event::End(_) => break,
                other => {
                    return Err(self.malformed(format!(
                        "{name} must hold only text, found {}",
                        describe(&other)
                    )));
                }
            }
        }
        if text.len() > MAX_BINARY_FIELD_CHARS {
            return Err(XmlError::FieldTooLarge {
                element: name.to_owned(),
                found: text.len(),
                limit: MAX_BINARY_FIELD_CHARS,
            });
        }
        let compact: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
        BASE64
            .decode(compact.as_bytes())
            .map_err(|error| self.malformed(format!("{name} is not valid base64: {error}")))
    }

    fn fixed<const N: usize>(&self, bytes: &[u8]) -> Result<[u8; N], XmlError> {
        <[u8; N]>::try_from(bytes)
            .map_err(|_| self.malformed(format!("expected {N} bytes, found {}", bytes.len())))
    }

    fn child_name<'e>(&self, element: &'e BytesStart<'e>) -> Result<&'e str, XmlError> {
        Ok(element.local_name().into_inner())
    }

    /// Reads one event without any namespace check, for the inside of a nested payload.
    fn raw_event(&mut self) -> Result<Event<'a>, XmlError> {
        match self.reader.read_resolved_event() {
            Ok((_, event)) => Ok(event),
            Err(error) => Err(self.malformed(error.to_string())),
        }
    }

    /// Reads one event of the document proper, requiring every element to be in the
    /// document's namespace.
    ///
    /// Until the root element has been read the namespace is unknown; the root's own
    /// namespace is recorded and checked against its declared version afterwards.
    fn event(&mut self) -> Result<Event<'a>, XmlError> {
        let outcome = self.reader.read_resolved_event();
        let (namespace, event) = match outcome {
            Ok(pair) => pair,
            Err(error) => return Err(self.malformed(error.to_string())),
        };
        if !matches!(event, Event::Start(_) | Event::Empty(_) | Event::End(_)) {
            return Ok(event);
        }
        let bound = match &namespace {
            ResolveResult::Bound(bound) => Some(bound.as_ref()),
            _ => None,
        };
        match self.namespace {
            None => {
                self.root_namespace = bound.map(ToOwned::to_owned);
            }
            Some(expected) if bound == Some(expected) => {}
            Some(expected) => {
                return Err(
                    self.malformed(format!("every element must be in the {expected} namespace"))
                );
            }
        }
        Ok(event)
    }

    fn expect_eof(&mut self) -> Result<(), XmlError> {
        loop {
            match self.event()? {
                Event::Eof => return Ok(()),
                Event::Text(_) | Event::Comment(_) | Event::Decl(_) => {}
                other => {
                    return Err(self.malformed(format!(
                        "trailing {} after the root element",
                        describe(&other)
                    )));
                }
            }
        }
    }

    fn expect_name(&self, element: &BytesStart<'_>, expected: &str) -> Result<(), XmlError> {
        if self.child_name(element)? == expected {
            Ok(())
        } else {
            Err(self.malformed(format!(
                "expected a {expected} element, found {}",
                self.child_name(element)?
            )))
        }
    }

    fn is_named(&self, element: &BytesStart<'_>, expected: &str) -> Result<bool, XmlError> {
        Ok(self.child_name(element)? == expected)
    }

    fn attributes(&self, element: &BytesStart<'_>) -> Result<Vec<(String, String)>, XmlError> {
        let mut pairs = Vec::new();
        for attribute in element.attributes() {
            let attribute =
                attribute.map_err(|error| self.malformed(format!("bad attribute: {error}")))?;
            let key = attribute.key.as_ref().to_owned();
            let value = attribute
                .normalized_value(XmlVersion::Explicit1_0)
                .map_err(|error| self.malformed(format!("bad attribute value: {error}")))?
                .into_owned();
            pairs.push((key, value));
        }
        Ok(pairs)
    }

    fn integer<T: core::str::FromStr>(&self, name: &str, value: &str) -> Result<T, XmlError> {
        value
            .parse()
            .map_err(|_| self.malformed(format!("{name}={value:?} is not a valid number")))
    }

    fn require<T>(&self, value: Option<T>, element: &str, attribute: &str) -> Result<T, XmlError> {
        value.ok_or_else(|| {
            self.malformed(format!("{element} is missing the {attribute} attribute"))
        })
    }

    fn unknown_attribute(&self, element: &str, attribute: &str) -> XmlError {
        self.malformed(format!("{element} has an unknown attribute {attribute:?}"))
    }

    fn unknown_element(&self, parent: &str, element: &str) -> XmlError {
        self.malformed(format!("{parent} has an unknown child element {element:?}"))
    }

    fn missing_element(&self, parent: &str, element: &str) -> XmlError {
        self.malformed(format!("{parent} is missing its {element} element"))
    }

    fn malformed(&self, detail: impl Into<String>) -> XmlError {
        XmlError::Malformed {
            position: self.reader.buffer_position(),
            detail: detail.into(),
        }
    }
}

fn describe(event: &Event<'_>) -> &'static str {
    match event {
        Event::Start(_) => "an opening tag",
        Event::End(_) => "a closing tag",
        Event::Empty(_) => "an empty element",
        Event::Text(_) => "text",
        Event::CData(_) => "a CDATA section",
        Event::Comment(_) => "a comment",
        Event::Decl(_) => "an XML declaration",
        Event::PI(_) => "a processing instruction",
        Event::DocType(_) => "a doctype",
        Event::GeneralRef(_) => "an entity reference",
        Event::Eof => "the end of the document",
    }
}
