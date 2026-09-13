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
    ChainDocument, DocumentBlock, DocumentKind, FORMAT_VERSION, Projection, XML_NAMESPACE,
};
use crate::error::XmlError;
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
    }
    .document()
}

struct Parser<'a> {
    reader: NsReader<&'a [u8]>,
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
        let mut previous: Option<BlockHeight> = None;

        loop {
            match self.event()? {
                Event::Text(_) | Event::Comment(_) => {}
                Event::Empty(element) if self.is_named(&element, "projection")? => {
                    document.projection = Some(self.projection(&element)?);
                }
                Event::Start(element) if self.is_named(&element, "block")? => {
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
                    if value != XML_NAMESPACE {
                        return Err(self.malformed(format!(
                            "document namespace is {value:?}, expected {XML_NAMESPACE:?}"
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
        if format_version != FORMAT_VERSION {
            return Err(XmlError::UnsupportedFormatVersion {
                found: format_version,
                supported: FORMAT_VERSION,
            });
        }

        let range_start = self.require(range_start, "prunella-chain", "range-start")?;
        let range_end = self.require(range_end, "prunella-chain", "range-end")?;
        if range_start > range_end {
            return Err(self.malformed(format!(
                "document declares an empty range {range_start}..={range_end}"
            )));
        }

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
            blocks: Vec::with_capacity(
                self.require(block_count, "prunella-chain", "block-count")?
                    .min(4096) as usize,
            ),
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
                    let expected_index = transactions.len() as u32;
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
                    "signer" => signer = Some(PublicKey::from_bytes(self.fixed(&[])?)),
                    "payload" => payload = Some(Vec::new()),
                    "signature" => signature = Some(Signature::from_bytes(self.fixed(&[])?)),
                    other => return Err(self.unknown_element("transaction", other)),
                },
                Event::Start(child) => {
                    let name = self.child_name(&child)?.to_owned();
                    let bytes = self.binary_text(&name)?;
                    match name.as_str() {
                        "signer" => signer = Some(PublicKey::from_bytes(self.fixed(&bytes)?)),
                        "payload" => payload = Some(bytes),
                        "signature" => signature = Some(Signature::from_bytes(self.fixed(&bytes)?)),
                        other => return Err(self.unknown_element("transaction", other)),
                    }
                }
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

    fn event(&mut self) -> Result<Event<'a>, XmlError> {
        let outcome = self.reader.read_resolved_event();
        let (namespace, event) = match outcome {
            Ok(pair) => pair,
            Err(error) => return Err(self.malformed(error.to_string())),
        };
        match (&event, namespace) {
            (Event::Start(_) | Event::Empty(_) | Event::End(_), ResolveResult::Bound(bound))
                if bound.as_ref() == XML_NAMESPACE => {}
            (Event::Start(_) | Event::Empty(_) | Event::End(_), _) => {
                return Err(self.malformed(format!(
                    "every element must be in the {XML_NAMESPACE} namespace"
                )));
            }
            _ => {}
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
