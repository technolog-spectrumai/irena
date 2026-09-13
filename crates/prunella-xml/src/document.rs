//! The in-memory form of a Prunella XML document.
//!
//! A document is a transport container. It carries real `prunella-core` blocks plus the
//! hashes the producer declared for them, so an importer can cross-check what it was
//! told against what the bytes actually derive.

use prunella_core::{Block, BlockHeight, Hash, Namespace, NetworkId};

/// The XML namespace of format version 2, the version this build writes.
pub const XML_NAMESPACE: &str = "urn:prunella:chain:2";

/// The XML namespace of format version 1, which this build still reads.
pub const XML_NAMESPACE_V1: &str = "urn:prunella:chain:1";

/// The document format version this build writes.
///
/// Version 2 differs from version 1 in one way: a `<payload>` may carry its bytes as a
/// nested XML element (`encoding="xml"`) instead of base64. Everything else, and every
/// hash, is unchanged.
pub const FORMAT_VERSION: u32 = 2;

/// The document format versions this build reads, ascending.
pub const SUPPORTED_FORMAT_VERSIONS: [u32; 2] = [1, 2];

/// The XML namespace a format version uses, or `None` if the version is not read.
#[must_use]
pub const fn namespace_for_version(version: u32) -> Option<&'static str> {
    match version {
        1 => Some(XML_NAMESPACE_V1),
        2 => Some(XML_NAMESPACE),
        _ => None,
    }
}

/// What a document claims to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentKind {
    /// Genesis through head: a complete chain backup.
    Full,
    /// A contiguous height range, for incremental transfer.
    Range,
    /// A namespace-filtered view.
    ///
    /// **Not a backup.** A projection omits transactions, so its blocks can no longer
    /// reproduce their own transaction root, and no importer can rebuild a chain from
    /// one. It exists to hand an application the subset it cares about, and it is
    /// marked so that nobody mistakes it for a chain they could restore from.
    Projection,
}

impl DocumentKind {
    /// The attribute text for this kind.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Range => "range",
            Self::Projection => "projection",
        }
    }

    /// Parses the attribute text.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "full" => Some(Self::Full),
            "range" => Some(Self::Range),
            "projection" => Some(Self::Projection),
            _ => None,
        }
    }

    /// Whether a document of this kind may be imported into a chain.
    #[must_use]
    pub const fn is_importable(self) -> bool {
        matches!(self, Self::Full | Self::Range)
    }
}

impl core::fmt::Display for DocumentKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The filter a projection was produced with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Projection {
    /// Only transactions in this namespace are present.
    pub filter_namespace: Namespace,
}

/// One block as it appears in a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentBlock {
    /// The block, rebuilt from the document's contents.
    pub block: Block,
    /// The hash the producer declared for it.
    ///
    /// Kept separate from the block so that a mismatch between what a document claims
    /// and what its own contents derive is detectable. The declared value is never
    /// used as the block's hash.
    pub declared_hash: Hash,
}

/// A parsed or assembled Prunella XML document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainDocument {
    /// Document format version.
    pub format_version: u32,
    /// What the document claims to be.
    pub kind: DocumentKind,
    /// The chain the blocks come from.
    pub network_id: NetworkId,
    /// The genesis hash of that chain.
    pub genesis_hash: Hash,
    /// First height present.
    pub range_start: BlockHeight,
    /// Last height present.
    pub range_end: BlockHeight,
    /// Producer's clock at export time, in milliseconds since the Unix epoch.
    ///
    /// Informational only. It is not hashed, not verified and not interpreted; it
    /// exists so an operator can tell two backups apart.
    pub exported_at_millis: u64,
    /// Present exactly when the kind is [`DocumentKind::Projection`].
    pub projection: Option<Projection>,
    /// The blocks, in ascending height order.
    pub blocks: Vec<DocumentBlock>,
}

impl ChainDocument {
    /// Number of blocks carried.
    #[must_use]
    pub fn block_count(&self) -> u64 {
        self.blocks.len() as u64
    }

    /// Whether this document can be imported into a chain.
    #[must_use]
    pub fn is_importable(&self) -> bool {
        self.kind.is_importable()
    }
}
