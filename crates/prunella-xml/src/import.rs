//! Applying documents to a chain.
//!
//! Import refuses rather than adapts. A document that does not continue the target
//! chain exactly is rejected with the height at which it stops making sense, and the
//! chain is left untouched.
//!
//! A dry run performs every check a real import performs and writes nothing, so
//! "the dry run passed" means the import will be accepted for the same reasons.

use crate::document::{ChainDocument, DocumentKind};
use crate::error::XmlError;
use prunella_core::{Block, BlockHeader, BlockHeight, ChainHead, GenesisSpec, TxId};
use prunella_store::{AppendOutcome, ChainStore, ExistingBlockPolicy};
use prunella_verify::{BlockContext, Finding, SourceError, TxIdLookup, check_block};
use std::collections::HashSet;
use std::path::Path;

/// What an import would do, computed without writing anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportPlan {
    /// The document's kind.
    pub kind: DocumentKind,
    /// First height in the document.
    pub range_start: BlockHeight,
    /// Last height in the document.
    pub range_end: BlockHeight,
    /// Blocks the document carries.
    pub blocks_in_document: u64,
    /// Blocks already committed, identically, and therefore not reapplied.
    pub blocks_already_present: u64,
    /// Blocks that would be newly committed.
    pub blocks_to_append: u64,
    /// The head the chain would have afterwards.
    pub resulting_head: ChainHead,
}

impl core::fmt::Display for ImportPlan {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "kind:             {}", self.kind)?;
        writeln!(
            f,
            "document heights: {}..={}",
            self.range_start, self.range_end
        )?;
        writeln!(f, "blocks in file:   {}", self.blocks_in_document)?;
        writeln!(f, "already present:  {}", self.blocks_already_present)?;
        writeln!(f, "to append:        {}", self.blocks_to_append)?;
        write!(f, "resulting head:   {}", self.resulting_head)
    }
}

/// Checks a document against a chain without writing anything.
///
/// # Errors
///
/// Returns the same errors a real import would: [`XmlError::NotImportable`],
/// [`XmlError::NetworkMismatch`], [`XmlError::GenesisMismatch`],
/// [`XmlError::DeclaredHashMismatch`], [`XmlError::Gap`], [`XmlError::Fork`] or
/// [`XmlError::InvalidBlocks`].
pub fn plan_import(store: &ChainStore, document: &ChainDocument) -> Result<ImportPlan, XmlError> {
    check_document_identity(store, document)?;
    check_declared_hashes(document)?;

    let head = store.head()?;
    let mut already_present = 0u64;
    let mut to_append: Vec<&Block> = Vec::new();

    for entry in &document.blocks {
        let height = entry.block.header.height;
        if height.value() <= head.height.value() {
            let committed = store
                .block_at(height)?
                .ok_or_else(|| XmlError::NotRestorable {
                    detail: format!("the chain claims height {height} but holds no block there"),
                })?;
            if committed == entry.block {
                already_present += 1;
                continue;
            }
            return Err(XmlError::Fork {
                height,
                existing: committed.hash(),
                offered: entry.block.hash(),
            });
        }
        to_append.push(&entry.block);
    }

    let Some(first) = to_append.first() else {
        return Ok(ImportPlan {
            kind: document.kind,
            range_start: document.range_start,
            range_end: document.range_end,
            blocks_in_document: document.block_count(),
            blocks_already_present: already_present,
            blocks_to_append: 0,
            resulting_head: head,
        });
    };

    let expected = head.height.next()?;
    if first.header.height != expected {
        return Err(XmlError::Gap {
            head: head.height,
            expected,
            found: first.header.height,
        });
    }

    let findings = validate_run(store, &head, &to_append)?;
    if !findings.is_empty() {
        return Err(XmlError::InvalidBlocks { findings });
    }

    let last = to_append.last().expect("checked non-empty");
    Ok(ImportPlan {
        kind: document.kind,
        range_start: document.range_start,
        range_end: document.range_end,
        blocks_in_document: document.block_count(),
        blocks_already_present: already_present,
        blocks_to_append: to_append.len() as u64,
        resulting_head: ChainHead::new(last.header.height, last.hash()),
    })
}

/// Applies a document to a chain atomically.
///
/// The blocks are committed in a single database transaction by
/// [`ChainStore::append_blocks`], so a rejection anywhere leaves the chain exactly as
/// it was. Re-importing a document that was already applied succeeds as a no-op.
///
/// # Errors
///
/// As [`plan_import`], plus [`XmlError::Store`] if the write failed.
pub fn import(store: &ChainStore, document: &ChainDocument) -> Result<AppendOutcome, XmlError> {
    plan_import(store, document)?;
    let blocks: Vec<Block> = document
        .blocks
        .iter()
        .map(|entry| entry.block.clone())
        .collect();
    Ok(store.append_blocks(blocks, ExistingBlockPolicy::SkipIfIdentical)?)
}

/// Creates a chain from a full backup and applies it.
///
/// The genesis block is rebuilt from the document's own height-zero block and must
/// hash to the genesis the document declares, so a backup cannot found a chain under a
/// genesis it does not actually contain.
///
/// # Errors
///
/// Returns [`XmlError::NotRestorable`] if the document cannot found a chain, plus the
/// errors of [`import`].
pub fn restore(
    path: impl AsRef<Path>,
    document: &ChainDocument,
) -> Result<(ChainStore, AppendOutcome), XmlError> {
    if document.kind != DocumentKind::Full {
        return Err(XmlError::NotRestorable {
            detail: format!(
                "a {} document is not a complete chain backup; only a full export can create a chain",
                document.kind
            ),
        });
    }
    let genesis_entry = document
        .blocks
        .first()
        .ok_or_else(|| XmlError::NotRestorable {
            detail: "the document carries no blocks".to_owned(),
        })?;
    if !genesis_entry.block.header.height.is_genesis() {
        return Err(XmlError::NotRestorable {
            detail: format!(
                "the first block is at height {}, not at genesis",
                genesis_entry.block.header.height
            ),
        });
    }

    let rebuilt = GenesisSpec {
        network_id: genesis_entry.block.header.network_id.clone(),
        timestamp_millis: genesis_entry.block.header.timestamp_millis,
        transactions: genesis_entry.block.transactions.clone(),
    }
    .build()?;
    if rebuilt.hash() != document.genesis_hash {
        return Err(XmlError::NotRestorable {
            detail: format!(
                "the document declares genesis {} but its height-zero block rebuilds to {}",
                document.genesis_hash,
                rebuilt.hash()
            ),
        });
    }

    let store = ChainStore::create(
        path,
        GenesisSpec {
            network_id: rebuilt.header.network_id.clone(),
            timestamp_millis: rebuilt.header.timestamp_millis,
            transactions: rebuilt.transactions.clone(),
        },
    )?;
    let outcome = import(&store, document)?;
    Ok((store, outcome))
}

/// Rejects a document that is not for this chain, or not importable at all.
fn check_document_identity(store: &ChainStore, document: &ChainDocument) -> Result<(), XmlError> {
    if !document.is_importable() {
        return Err(XmlError::NotImportable {
            kind: document.kind.to_string(),
            namespace: document
                .projection
                .as_ref()
                .map_or_else(|| "unknown".to_owned(), |p| p.filter_namespace.to_string()),
        });
    }
    if &document.network_id != store.network_id() {
        return Err(XmlError::NetworkMismatch {
            expected: store.network_id().clone(),
            found: Box::new(document.network_id.clone()),
        });
    }
    if document.genesis_hash != store.genesis_hash() {
        return Err(XmlError::GenesisMismatch {
            expected: store.genesis_hash(),
            found: document.genesis_hash,
        });
    }
    Ok(())
}

/// Confirms that every block hashes to the hash the document declared for it.
///
/// This is the one place the declared hashes are used. They are compared against the
/// value derived from the block's own contents and then discarded; no chain hash is
/// ever taken from the document's text.
fn check_declared_hashes(document: &ChainDocument) -> Result<(), XmlError> {
    for entry in &document.blocks {
        let computed = entry.block.hash();
        if computed != entry.declared_hash {
            return Err(XmlError::DeclaredHashMismatch {
                height: entry.block.header.height,
                declared: entry.declared_hash,
                computed,
            });
        }
    }
    Ok(())
}

/// Applies the deterministic rules to a run of blocks that would extend the chain.
fn validate_run(
    store: &ChainStore,
    head: &ChainHead,
    blocks: &[&Block],
) -> Result<Vec<Finding>, XmlError> {
    let parent_block = store
        .block_at(head.height)?
        .ok_or_else(|| XmlError::NotRestorable {
            detail: format!("the chain head is {head} but no block is stored there"),
        })?;

    let mut findings = Vec::new();
    let mut parent: BlockHeader = parent_block.header;
    let mut pending: HashSet<TxId> = HashSet::new();

    for block in blocks {
        let lookup = PendingAware {
            store,
            pending: &pending,
        };
        let context = BlockContext {
            expected_network: store.network_id(),
            parent: Some(&parent),
            expected_hash: None,
            committed_transactions: Some(&lookup),
        };
        findings.extend(check_block(&context, block));
        for transaction in &block.transactions {
            pending.insert(transaction.id);
        }
        parent = block.header.clone();
    }
    Ok(findings)
}

/// Transaction ids already committed, plus those an in-progress import would add.
struct PendingAware<'a> {
    store: &'a ChainStore,
    pending: &'a HashSet<TxId>,
}

impl TxIdLookup for PendingAware<'_> {
    fn contains(&self, id: &TxId) -> Result<bool, SourceError> {
        if self.pending.contains(id) {
            return Ok(true);
        }
        self.store.contains(id)
    }
}
