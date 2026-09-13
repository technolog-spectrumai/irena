//! How a payload is carried in a document: nested XML or base64.
//!
//! A payload is opaque bytes, and the chain commits to those exact bytes. Carrying them
//! as base64 is always correct and always unreadable. Format version 2 lets a payload
//! that *is* an XML element travel as that element, so an application's records are
//! legible inside the block that holds them, while still never letting the document's
//! layout touch a single payload byte.
//!
//! The decision is a pure function of the bytes ([`PayloadEncoding::choose`]), so two
//! exporters of the same chain write the same document. A payload is nested only when
//! it can be recovered exactly by cutting it back out of the source text: it must be
//! valid UTF-8 and consist of exactly one well-formed element with nothing before or
//! after it. Everything else — empty, binary, prose, a declaration-prefixed document,
//! a comment beside the element — goes as base64.

use quick_xml::Reader;
use quick_xml::events::Event;

/// How a `<payload>` element carries its bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayloadEncoding {
    /// Base64 text. The default when the attribute is absent.
    Base64,
    /// The payload bytes verbatim, as exactly one nested XML element.
    Xml,
}

impl PayloadEncoding {
    /// The attribute text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Base64 => "base64",
            Self::Xml => "xml",
        }
    }

    /// Parses the attribute text.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "base64" => Some(Self::Base64),
            "xml" => Some(Self::Xml),
            _ => None,
        }
    }

    /// Decides how `payload` is written.
    ///
    /// Nested if and only if [`is_single_element`] holds; base64 otherwise.
    #[must_use]
    pub fn choose(payload: &[u8]) -> Self {
        match core::str::from_utf8(payload) {
            Ok(text) if is_single_element(text) => Self::Xml,
            _ => Self::Base64,
        }
    }
}

/// Whether `text` is exactly one well-formed XML element and nothing else.
///
/// No leading or trailing bytes of any kind — not whitespace, not a declaration, not a
/// comment — because an importer recovers a nested payload by slicing the source from
/// the element's first `<` to its last `>`, and only a payload that is *exactly* that
/// slice survives the trip unchanged. Inside the element anything well-formed goes:
/// nested elements, attributes, text, CDATA, comments, entity references, namespaces.
///
/// This is a well-formedness check, not validation. It says nothing about what the
/// element means, which is the application's business.
#[must_use]
pub fn is_single_element(text: &str) -> bool {
    if !text.starts_with('<') || !text.ends_with('>') {
        return false;
    }
    let mut reader = Reader::from_str(text);
    let mut depth = 0usize;
    let mut seen_root = false;
    loop {
        match reader.read_event() {
            Err(_) => return false,
            Ok(Event::Eof) => return seen_root && depth == 0,
            Ok(Event::Start(_)) => {
                if seen_root && depth == 0 {
                    return false;
                }
                seen_root = true;
                depth += 1;
            }
            Ok(Event::End(_)) => {
                depth = match depth.checked_sub(1) {
                    Some(depth) => depth,
                    None => return false,
                };
            }
            Ok(Event::Empty(_)) => {
                if depth == 0 {
                    if seen_root {
                        return false;
                    }
                    seen_root = true;
                }
            }
            Ok(Event::GeneralRef(reference)) if depth > 0 => {
                // An undeclared entity is a well-formedness error, and a nested
                // payload has no DTD to declare one. Only character references and
                // the five predefined entities are allowed.
                let known = if reference.is_char_ref() {
                    matches!(reference.resolve_char_ref(), Ok(Some(_)))
                } else {
                    matches!(reference.as_ref(), "lt" | "gt" | "amp" | "apos" | "quot")
                };
                if !known {
                    return false;
                }
            }
            Ok(Event::Text(_) | Event::CData(_) | Event::Comment(_) | Event::PI(_))
                if depth > 0 => {}
            // Anything outside the element: text, a declaration, a doctype, a
            // comment, a processing instruction. Not a single element.
            Ok(_) => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_element_is_recognised() {
        for text in [
            "<a/>",
            "<a></a>",
            "<a b=\"1\"><c/>text<d>&amp;</d><![CDATA[<x>]]><!-- ok --></a>",
            "<r xmlns=\"urn:x\" xmlns:p=\"urn:p\"><p:q/></r>",
            "<a>\n  <b/>\n</a>",
            "<a>&#x41;&#65;&lt;&gt;&amp;&apos;&quot;</a>",
            "<a><b/><c/><d><e/></d></a>",
            "<a><?pi inside?></a>",
        ] {
            assert!(is_single_element(text), "{text:?}");
            assert_eq!(
                PayloadEncoding::choose(text.as_bytes()),
                PayloadEncoding::Xml
            );
        }
    }

    #[test]
    fn anything_else_is_base64() {
        for text in [
            "",
            " ",
            "hello",
            " <a/>",
            "<a/> ",
            "<a/>\n",
            "<?xml version=\"1.0\"?><a/>",
            "<!-- c --><a/>",
            "<a/><!-- c -->",
            "<a/><b/>",
            "<a>",
            "<a></b>",
            "</a>",
            "<a>&undefined;</a>",
            "<a>&#xD800;</a>",
            "<a>&#;</a>",
            "<a>&</a>",
            "<a><b></a>",
            "<!DOCTYPE a><a/>",
            "<?pi?><a/>",
            "<",
            ">",
            "<a/>>",
        ] {
            assert!(!is_single_element(text), "{text:?}");
            assert_eq!(
                PayloadEncoding::choose(text.as_bytes()),
                PayloadEncoding::Base64
            );
        }
        assert_eq!(
            PayloadEncoding::choose(&[b'<', 0xff, b'/', b'>']),
            PayloadEncoding::Base64
        );
        assert_eq!(
            PayloadEncoding::choose(b"<a>\xff</a>"),
            PayloadEncoding::Base64
        );
    }

    #[test]
    fn attribute_text_round_trips() {
        for encoding in [PayloadEncoding::Base64, PayloadEncoding::Xml] {
            assert_eq!(PayloadEncoding::parse(encoding.as_str()), Some(encoding));
        }
        assert_eq!(PayloadEncoding::parse("hex"), None);
    }
}
