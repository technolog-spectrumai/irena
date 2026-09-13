//! Building documents from a chain.

use crate::document::{ChainDocument, DocumentBlock, DocumentKind, FORMAT_VERSION, Projection};
use crate::error::XmlError;
use prunella_core::{BlockHeight, Namespace};
use prunella_store::ChainStore;

/// What to export.
#[derive(Clone, Debug, Default)]
pub struct ExportRequest {
    /// First height, defaulting to genesis.
    pub from: Option<BlockHeight>,
    /// Last height, defaulting to the head and clamped to it.
    pub to: Option<BlockHeight>,
    /// Restrict transactions to one namespace, producing a projection.
    ///
    /// A projection is a partial view, not a backup. See [`DocumentKind::Projection`].
    pub namespace: Option<Namespace>,
    /// Producer's clock, recorded for operators and never verified.
    pub exported_at_millis: u64,
}

impl ExportRequest {
    /// A request for the whole chain.
    #[must_use]
    pub fn full() -> Self {
        Self::default()
    }

    /// A request for a contiguous height range.
    #[must_use]
    pub fn range(from: BlockHeight, to: BlockHeight) -> Self {
        Self {
            from: Some(from),
            to: Some(to),
            ..Self::default()
        }
    }

    /// Restricts the export to one namespace, making it a projection.
    #[must_use]
    pub fn filtered_to(mut self, namespace: Namespace) -> Self {
        self.namespace = Some(namespace);
        self
    }

    /// Records the producer's clock on the document.
    #[must_use]
    pub fn exported_at(mut self, millis: u64) -> Self {
        self.exported_at_millis = millis;
        self
    }
}

/// Reads a chain into a document.
///
/// The hashes written into the document are the hashes the blocks derive, read back
/// out of storage. Nothing is recomputed differently for export.
///
/// # Errors
///
/// Returns [`XmlError::Store`] if the chain could not be read, or
/// [`XmlError::NotRestorable`] if the requested range is empty.
pub fn export(store: &ChainStore, request: &ExportRequest) -> Result<ChainDocument, XmlError> {
    let head = store.head()?;
    let from = request.from.unwrap_or(BlockHeight::GENESIS);
    let to = request.to.map_or(head.height, |to| to.min(head.height));
    if from > to {
        return Err(XmlError::EmptyRange { from, to });
    }

    let kind = if request.namespace.is_some() {
        DocumentKind::Projection
    } else if from.is_genesis() && to == head.height {
        DocumentKind::Full
    } else {
        DocumentKind::Range
    };

    let mut blocks = Vec::new();
    for block in store.blocks_in_range(from, to)? {
        let mut block = block?;
        let declared_hash = block.hash();
        if let Some(namespace) = &request.namespace {
            // The header is kept intact, so a projection still carries the block's true
            // transaction count and root. That is deliberate: it lets a reader see that
            // transactions were removed, and it is exactly why a projection can never be
            // imported — the filtered list can no longer reproduce the root.
            block
                .transactions
                .retain(|transaction| &transaction.namespace == namespace);
        }
        blocks.push(DocumentBlock {
            block,
            declared_hash,
        });
    }

    Ok(ChainDocument {
        format_version: FORMAT_VERSION,
        kind,
        network_id: store.network_id().clone(),
        genesis_hash: store.genesis_hash(),
        range_start: from,
        range_end: to,
        exported_at_millis: request.exported_at_millis,
        projection: request
            .namespace
            .clone()
            .map(|filter_namespace| Projection { filter_namespace }),
        blocks,
    })
}
