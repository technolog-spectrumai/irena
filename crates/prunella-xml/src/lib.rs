//! Lossless, versioned XML export and import for Prunella chains.
//!
//! # XML is transport, never authority
//!
//! A document carries the hashes its producer derived, but an importer never believes
//! them. Every block is rebuilt into `prunella-core` types and re-derived through
//! `prunella-canonical`; the declared hashes are compared against those derivations and
//! then discarded. No chain hash is ever computed from XML text, so how a document is
//! indented, escaped or line-wrapped cannot change a single hash.
//!
//! # What is preserved
//!
//! Network id, genesis hash, every block height and hash, transaction order, payload
//! bytes exactly as they were, signer keys and signatures. A full export re-imported
//! into a new chain yields identical block hashes at every height.
//!
//! # Kinds of document
//!
//! * [`DocumentKind::Full`] — genesis through head. A complete backup, and the only
//!   kind that can found a new chain through [`restore`].
//! * [`DocumentKind::Range`] — a contiguous height range, for incremental transfer.
//! * [`DocumentKind::Projection`] — a namespace-filtered view. **Not a backup.** It
//!   omits transactions, so its blocks cannot reproduce their own transaction root.
//!   It is marked as a projection in the document, and import refuses it outright.
//!
//! # Import guarantees
//!
//! Import is atomic: blocks are committed in one database transaction, so a rejection
//! anywhere leaves the chain byte-for-byte as it was. It is idempotent: re-importing a
//! document already applied succeeds as a no-op. It refuses gaps, forks, wrong parents,
//! foreign networks, tampering and malformed input, naming the height at which the
//! document stopped making sense. [`plan_import`] runs every one of those checks and
//! writes nothing.
//!
//! The document schema is published as `schemas/prunella-chain-v1.xsd`. The parser
//! implements the equivalent structural checks in code, because Rust has no mature XSD
//! validator; the schema is the contract for other tooling.

mod document;
mod error;
mod export;
mod import;
mod read;
mod write;

pub use document::{
    ChainDocument, DocumentBlock, DocumentKind, FORMAT_VERSION, Projection, XML_NAMESPACE,
};
pub use error::XmlError;
pub use export::{ExportRequest, export};
pub use import::{ImportPlan, import, plan_import, restore};
pub use read::{
    DEFAULT_MAX_DOCUMENT_BYTES, MAX_BINARY_FIELD_CHARS, MAX_BLOCKS, MAX_TRANSACTIONS_PER_BLOCK,
    read_document, read_document_with_limit,
};
pub use write::write_document;
