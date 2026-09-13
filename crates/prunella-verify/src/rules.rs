//! The deterministic block rules.
//!
//! This module is the single implementation of what makes a block acceptable. The
//! block-acceptance policy in `prunella-store`, full-chain verification and XML import
//! all call [`check_block`]; none of them reimplements any part of it. That is what
//! makes the core invariant hold: every correct instance applies the same rules to the
//! same bytes and reaches the same conclusion.

use crate::finding::{Finding, FindingKind, Location};
use crate::source::SourceError;
use prunella_core::{Block, BlockHeader, BlockHeight, HEADER_VERSION, Hash, NetworkId, TxId};
use std::collections::HashSet;

/// Answers whether a transaction id is already committed somewhere in a chain.
///
/// Supplied by whoever has chain state to consult: the store consults its transaction
/// index, chain verification consults the ids it has already walked past, and a caller
/// with no state at all supplies nothing and the check is skipped.
pub trait TxIdLookup {
    /// Returns true when the chain already contains this transaction id.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError`] if the underlying state could not be read.
    fn contains(&self, id: &TxId) -> Result<bool, SourceError>;
}

/// Everything [`check_block`] needs to judge one block.
pub struct BlockContext<'a> {
    /// The chain the block must belong to.
    pub expected_network: &'a NetworkId,
    /// The parent header, or `None` when a genesis block is expected.
    pub parent: Option<&'a BlockHeader>,
    /// A hash the block was looked up or announced by, cross-checked against the
    /// hash recomputed from the block's own contents.
    pub expected_hash: Option<Hash>,
    /// Chain state used to reject transaction ids that are already committed.
    pub committed_transactions: Option<&'a dyn TxIdLookup>,
}

impl<'a> BlockContext<'a> {
    /// A context expecting the genesis block of `network`.
    #[must_use]
    pub fn genesis(network: &'a NetworkId) -> Self {
        Self {
            expected_network: network,
            parent: None,
            expected_hash: None,
            committed_transactions: None,
        }
    }

    /// A context expecting the child of `parent`.
    #[must_use]
    pub fn child_of(network: &'a NetworkId, parent: &'a BlockHeader) -> Self {
        Self {
            expected_network: network,
            parent: Some(parent),
            expected_hash: None,
            committed_transactions: None,
        }
    }

    /// Cross-checks the block against a hash it was announced or indexed by.
    #[must_use]
    pub fn expecting_hash(mut self, hash: Hash) -> Self {
        self.expected_hash = Some(hash);
        self
    }

    /// Supplies chain state for the chain-level duplicate transaction check.
    #[must_use]
    pub fn with_committed_transactions(mut self, lookup: &'a dyn TxIdLookup) -> Self {
        self.committed_transactions = Some(lookup);
        self
    }
}

/// Applies every deterministic rule to one block and returns each violation found.
///
/// All rules are evaluated: the function never stops at the first failure, so a single
/// pass reports every defect in the block rather than making an operator fix one
/// problem at a time. An empty result means the block is acceptable.
#[must_use]
pub fn check_block(context: &BlockContext<'_>, block: &Block) -> Vec<Finding> {
    let mut findings = Vec::new();
    let header = &block.header;
    let computed_hash = header.block_hash();
    let at = || Location::at_height(header.height).with_block_hash(computed_hash);

    if header.version != HEADER_VERSION {
        findings.push(Finding::new(
            FindingKind::UnsupportedHeaderVersion,
            at(),
            format!(
                "header version {} is not supported (expected {HEADER_VERSION})",
                header.version
            ),
        ));
    }

    if &header.network_id != context.expected_network {
        findings.push(Finding::new(
            FindingKind::NetworkMismatch,
            at(),
            format!(
                "block belongs to network {} but this chain is {}",
                header.network_id, context.expected_network
            ),
        ));
    }

    check_linkage(context, header, &at, &mut findings);
    check_transaction_commitments(block, &at, &mut findings);
    check_transactions(context, block, &at, &mut findings);

    if let Some(expected) = context.expected_hash
        && expected != computed_hash
    {
        findings.push(Finding::new(
            FindingKind::BlockHashMismatch,
            at(),
            format!("block was looked up as {expected} but its contents hash to {computed_hash}"),
        ));
    }

    findings
}

/// Height, parent linkage and timestamp ordering.
fn check_linkage(
    context: &BlockContext<'_>,
    header: &BlockHeader,
    at: &impl Fn() -> Location,
    findings: &mut Vec<Finding>,
) {
    match context.parent {
        None => {
            if header.height != BlockHeight::GENESIS {
                findings.push(Finding::new(
                    FindingKind::HeightOutOfOrder,
                    at(),
                    format!(
                        "expected the genesis block at height 0, found height {}",
                        header.height
                    ),
                ));
            }
            if !header.previous_hash.is_zero() {
                findings.push(Finding::new(
                    FindingKind::PreviousHashMismatch,
                    at(),
                    format!(
                        "genesis must have a zero previous hash, found {}",
                        header.previous_hash
                    ),
                ));
            }
        }
        Some(parent) => {
            let expected_height = parent.height.value().checked_add(1);
            if Some(header.height.value()) != expected_height {
                findings.push(Finding::new(
                    FindingKind::HeightOutOfOrder,
                    at(),
                    format!(
                        "expected height {} after parent height {}, found {}",
                        expected_height.map_or_else(|| "overflow".to_owned(), |h| h.to_string()),
                        parent.height,
                        header.height
                    ),
                ));
            }
            let parent_hash = parent.block_hash();
            if header.previous_hash != parent_hash {
                findings.push(Finding::new(
                    FindingKind::PreviousHashMismatch,
                    at(),
                    format!(
                        "expected previous hash {parent_hash} (parent at height {}), found {}",
                        parent.height, header.previous_hash
                    ),
                ));
            }
            if header.timestamp_millis < parent.timestamp_millis {
                findings.push(Finding::new(
                    FindingKind::TimestampRegression,
                    at(),
                    format!(
                        "timestamp {} precedes parent timestamp {}",
                        header.timestamp_millis, parent.timestamp_millis
                    ),
                ));
            }
        }
    }
}

/// The header's commitments to the transaction list.
fn check_transaction_commitments(
    block: &Block,
    at: &impl Fn() -> Location,
    findings: &mut Vec<Finding>,
) {
    let actual_count = block.transactions.len();
    match u32::try_from(actual_count) {
        Ok(count) if count == block.header.tx_count => {}
        Ok(count) => findings.push(Finding::new(
            FindingKind::TxCountMismatch,
            at(),
            format!(
                "header declares {} transactions, block carries {count}",
                block.header.tx_count
            ),
        )),
        Err(_) => findings.push(Finding::new(
            FindingKind::TxCountMismatch,
            at(),
            format!("block carries {actual_count} transactions, more than a header can express"),
        )),
    }

    match prunella_core::Transaction::compute_root(&block.transactions) {
        Ok(root) if root == block.header.tx_root => {}
        Ok(root) => findings.push(Finding::new(
            FindingKind::TxRootMismatch,
            at(),
            format!(
                "header declares transaction root {} but the transactions hash to {root}",
                block.header.tx_root
            ),
        )),
        Err(error) => findings.push(Finding::new(
            FindingKind::TxRootMismatch,
            at(),
            format!("transaction root could not be computed: {error}"),
        )),
    }
}

/// Per-transaction identity, signature and uniqueness.
fn check_transactions(
    context: &BlockContext<'_>,
    block: &Block,
    at: &impl Fn() -> Location,
    findings: &mut Vec<Finding>,
) {
    let mut seen: HashSet<TxId> = HashSet::with_capacity(block.transactions.len());

    for (index, transaction) in block.transactions.iter().enumerate() {
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        let here = || at().with_transaction(index, transaction.id);

        let computed_id = transaction.compute_id();
        if computed_id != transaction.id {
            findings.push(Finding::new(
                FindingKind::TxIdMismatch,
                here(),
                format!(
                    "transaction declares id {} but its contents derive {computed_id}",
                    transaction.id
                ),
            ));
        }

        if let Err(error) = prunella_crypto::verify_transaction(transaction) {
            findings.push(Finding::new(
                FindingKind::SignatureInvalid,
                here(),
                error.to_string(),
            ));
        }

        if !seen.insert(transaction.id) {
            findings.push(Finding::new(
                FindingKind::DuplicateTxIdInBlock,
                here(),
                format!(
                    "transaction id {} appears more than once in this block",
                    transaction.id
                ),
            ));
        }

        if let Some(lookup) = context.committed_transactions {
            match lookup.contains(&transaction.id) {
                Ok(true) => findings.push(Finding::new(
                    FindingKind::DuplicateTxIdInChain,
                    here(),
                    format!(
                        "transaction id {} is already committed in this chain",
                        transaction.id
                    ),
                )),
                Ok(false) => {}
                Err(error) => findings.push(Finding::new(
                    FindingKind::SourceFailure,
                    here(),
                    format!(
                        "could not check whether the transaction is already committed: {error}"
                    ),
                )),
            }
        }
    }
}
