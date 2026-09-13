//! Export and import failures.

use prunella_core::{BlockHeight, Hash, NetworkId};
use prunella_verify::Finding;

/// Failure modes of XML export and import.
///
/// Import refuses rather than adapts. Every variant here describes a document that was
/// not applied, and in every case the target chain is left exactly as it was.
#[derive(Debug, thiserror::Error)]
pub enum XmlError {
    /// The document is not well-formed XML, or is not shaped like a Prunella document.
    #[error("malformed document at byte {position}: {detail}")]
    Malformed {
        /// Byte offset where parsing stopped.
        position: u64,
        /// What was wrong.
        detail: String,
    },
    /// The document declares a format version this build does not understand.
    #[error("document format version {found} is not supported (this build reads {supported})")]
    UnsupportedFormatVersion {
        /// The version declared.
        found: u32,
        /// The version this build reads.
        supported: u32,
    },
    /// The document belongs to a different chain.
    #[error("document is from network {found} but the chain is {expected}")]
    NetworkMismatch {
        /// The chain the target holds.
        expected: NetworkId,
        /// The chain the document claims.
        found: Box<NetworkId>,
    },
    /// The document is from a chain with a different genesis.
    #[error("document declares genesis {found} but the chain's genesis is {expected}")]
    GenesisMismatch {
        /// The target chain's genesis.
        expected: Hash,
        /// The document's genesis.
        found: Hash,
    },
    /// A block's contents do not hash to the hash the document declared for it.
    #[error(
        "block at height {height} is declared as {declared} but its contents hash to {computed}"
    )]
    DeclaredHashMismatch {
        /// Where the mismatch is.
        height: BlockHeight,
        /// What the document claimed.
        declared: Hash,
        /// What the contents derive.
        computed: Hash,
    },
    /// The document's heights are not a contiguous ascending run.
    #[error("document heights are not contiguous: {height} follows {previous}")]
    NonContiguousDocument {
        /// The height that broke the run.
        height: BlockHeight,
        /// The height before it.
        previous: BlockHeight,
    },
    /// The document does not continue the target chain.
    #[error(
        "chain is at height {head}, so an import must start at height {expected}, but the document starts at {found}"
    )]
    Gap {
        /// The target's head height.
        head: BlockHeight,
        /// The height the chain is waiting for.
        expected: BlockHeight,
        /// Where the document starts.
        found: BlockHeight,
    },
    /// The document contradicts a block already committed.
    #[error(
        "document would replace the block at height {height}: chain holds {existing}, document has {offered}"
    )]
    Fork {
        /// Where the histories diverge.
        height: BlockHeight,
        /// What the chain holds.
        existing: Hash,
        /// What the document offers.
        offered: Hash,
    },
    /// One or more blocks broke the deterministic rules.
    #[error("document contains {} invalid block finding(s): {}", .findings.len(), render(.findings))]
    InvalidBlocks {
        /// Every rule broken, with exact locations.
        findings: Vec<Finding>,
    },
    /// A namespace-filtered projection was offered for import.
    #[error(
        "this document is a {kind} filtered to namespace {namespace}, which is a partial view \
         rather than a chain backup, and cannot be imported"
    )]
    NotImportable {
        /// The document's kind.
        kind: String,
        /// The namespace it was filtered to.
        namespace: String,
    },
    /// A restore was asked for with a document that cannot found a chain.
    #[error("cannot create a chain from this document: {detail}")]
    NotRestorable {
        /// Why the document cannot found a chain.
        detail: String,
    },
    /// The document exceeded the configured size limit.
    #[error("document is {found} bytes, over the {limit} byte limit")]
    TooLarge {
        /// Size of the input.
        found: u64,
        /// The configured limit.
        limit: u64,
    },
    /// The chain could not be read or written.
    #[error(transparent)]
    Store(#[from] prunella_store::StoreError),
    /// A value in the document is not a valid core type.
    #[error(transparent)]
    Core(#[from] prunella_core::CoreError),
    /// The document could not be written out.
    #[error("could not write the document: {0}")]
    Write(String),
}

fn render(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}
