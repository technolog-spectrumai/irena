//! Strict XML readers for Bornite documents.
//!
//! Two standalone documents are read here — `<voting-rules>` and `<vote>` — and the
//! element parsers behind them are public so that another format can embed a
//! `<voting-rules>` or `<electorate>` element and parse it with exactly this code. That
//! is how the same rules bytes mean the same rules whether they sit in a file or in a
//! ledger record.
//!
//! The readers are strict: an unknown element or attribute is refused, never skipped;
//! attributes an element type does not use are refused, not ignored; every content
//! issue in a document is collected and reported together. The schemas under
//! `schemas/bornite-*.xsd` are the published contract; the readers enforce the same
//! rules in code, plus the two conditional-attribute rules XSD 1.0 cannot express.
//!
//! There is no writer. Bornite never produces a document; it reads what it is given.

mod documents;
mod error;
mod reader;

pub use documents::{
    DEFAULT_MAX_DOCUMENT_BYTES, VoteDocumentV1, expect_eof, open, read_rules_document,
    read_rules_document_with_limit, read_vote_document, read_vote_document_with_limit, root_start,
};
pub use error::{XmlError, XmlIssueV1};
pub use reader::{
    Attributes, VERSION, XmlReader, check_version, describe, expect_empty, malformed, next_event,
    parse_ballots, parse_electorate, parse_voting_rules,
};
