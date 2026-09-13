//! Full-chain verification from genesis to head.

use crate::finding::{Finding, FindingKind, Location};
use crate::rules::{BlockContext, TxIdLookup, check_block};
use crate::source::{BlockSource, SourceError};
use prunella_core::{BlockHeader, BlockHeight, ChainHead, Hash, NetworkId, TxId};
use std::cell::RefCell;
use std::collections::HashSet;

/// What to verify.
#[derive(Clone, Copy, Debug)]
pub struct VerifyOptions {
    /// First height to check. Defaults to genesis.
    pub from: Option<BlockHeight>,
    /// Last height to check. Defaults to the head, and is clamped to it.
    pub to: Option<BlockHeight>,
    /// Whether to reject a transaction id that appears in more than one block.
    ///
    /// Requires holding every transaction id seen so far in memory, which is linear in
    /// the number of transactions verified. Turn it off for a spot check of a long
    /// chain; leave it on for a real audit, because a replayed transaction is a
    /// genuine ledger defect.
    pub detect_duplicate_transactions: bool,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            from: None,
            to: None,
            detect_duplicate_transactions: true,
        }
    }
}

impl VerifyOptions {
    /// Verifies a contiguous height range instead of the whole chain.
    ///
    /// Genesis identity is still checked, because a range means nothing without
    /// knowing which chain it belongs to.
    #[must_use]
    pub fn range(from: BlockHeight, to: BlockHeight) -> Self {
        Self {
            from: Some(from),
            to: Some(to),
            ..Self::default()
        }
    }
}

/// The result of a verification pass.
///
/// A report is always produced, even for a badly damaged chain. `findings` is the
/// verdict; everything else is context for reading it.
#[derive(Clone, Debug, serde::Serialize)]
pub struct VerificationReport {
    /// The chain that was verified.
    pub network_id: NetworkId,
    /// The genesis hash the source recorded at creation.
    pub genesis_hash: Hash,
    /// The head at the time of the pass, if it could be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<ChainHead>,
    /// First height actually checked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_start: Option<BlockHeight>,
    /// Last height actually checked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_end: Option<BlockHeight>,
    /// Number of blocks read and checked.
    pub blocks_checked: u64,
    /// Number of transactions read and checked.
    pub transactions_checked: u64,
    /// Every defect found, in the order encountered.
    pub findings: Vec<Finding>,
}

impl VerificationReport {
    /// Returns true when nothing at all was found wrong.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.findings.is_empty()
    }
}

impl core::fmt::Display for VerificationReport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "network:     {}", self.network_id)?;
        writeln!(f, "genesis:     {}", self.genesis_hash)?;
        match self.head {
            Some(head) => writeln!(f, "head:        {head}")?,
            None => writeln!(f, "head:        unreadable")?,
        }
        if let (Some(start), Some(end)) = (self.range_start, self.range_end) {
            writeln!(f, "checked:     heights {start}..={end}")?;
        }
        writeln!(f, "blocks:      {}", self.blocks_checked)?;
        writeln!(f, "txs:         {}", self.transactions_checked)?;
        if self.findings.is_empty() {
            write!(f, "result:      valid")
        } else {
            writeln!(
                f,
                "result:      INVALID ({} finding(s))",
                self.findings.len()
            )?;
            for finding in &self.findings {
                writeln!(f, "  {finding}")?;
            }
            Ok(())
        }
    }
}

/// A running set of the transaction ids already seen during this pass.
struct SeenTransactions(RefCell<HashSet<TxId>>);

impl TxIdLookup for SeenTransactions {
    fn contains(&self, id: &TxId) -> Result<bool, SourceError> {
        Ok(self.0.borrow().contains(id))
    }
}

/// Verifies a chain and returns a structured report.
///
/// The pass walks blocks in order, because every block's validity depends on its
/// parent. A block that cannot be read ends the walk: without it, no later block's
/// parent is known, and continuing would produce a cascade of findings that all
/// describe the same one defect.
#[must_use]
pub fn verify_chain<S: BlockSource + ?Sized>(
    source: &S,
    options: VerifyOptions,
) -> VerificationReport {
    let mut report = VerificationReport {
        network_id: source.network_id().clone(),
        genesis_hash: source.genesis_hash(),
        head: None,
        range_start: None,
        range_end: None,
        blocks_checked: 0,
        transactions_checked: 0,
        findings: Vec::new(),
    };

    let head = match source.head() {
        Ok(head) => head,
        Err(error) => {
            report.findings.push(Finding::new(
                FindingKind::SourceFailure,
                Location::default(),
                format!("could not read the chain head: {error}"),
            ));
            return report;
        }
    };
    report.head = Some(head);

    check_genesis_identity(source, &mut report);

    let from = options.from.unwrap_or(BlockHeight::GENESIS);
    let to = options.to.map_or(head.height, |to| to.min(head.height));
    if from > to {
        return report;
    }
    report.range_start = Some(from);

    let seen = SeenTransactions(RefCell::new(HashSet::new()));
    let mut parent: Option<BlockHeader> = None;

    if !from.is_genesis() {
        match load_parent(source, from, &mut report) {
            Ok(header) => parent = Some(header),
            Err(()) => return report,
        }
    }

    let mut height = from;
    loop {
        let block = match source.block_at(height) {
            Ok(Some(block)) => block,
            Ok(None) => {
                report.findings.push(Finding::new(
                    FindingKind::MissingBlock,
                    Location::at_height(height),
                    format!("no block stored at height {height}, but the head is {head}"),
                ));
                break;
            }
            Err(error) => {
                report.findings.push(Finding::new(
                    FindingKind::DecodeError,
                    Location::at_height(height),
                    format!("block at height {height} could not be read: {error}"),
                ));
                break;
            }
        };

        let mut context = BlockContext {
            expected_network: source.network_id(),
            parent: parent.as_ref(),
            expected_hash: None,
            committed_transactions: None,
        };
        if options.detect_duplicate_transactions {
            context.committed_transactions = Some(&seen);
        }
        if height == head.height {
            context.expected_hash = Some(head.hash);
        }

        report.findings.extend(check_block(&context, &block));
        report.blocks_checked += 1;
        report.transactions_checked += block.transactions.len() as u64;
        report.range_end = Some(height);

        if options.detect_duplicate_transactions {
            let mut set = seen.0.borrow_mut();
            for transaction in &block.transactions {
                set.insert(transaction.id);
            }
        }

        if height == to {
            break;
        }
        match height.next() {
            Ok(next) => height = next,
            Err(_) => break,
        }
        parent = Some(block.header);
    }

    report
}

/// Checks that the block at height zero is the genesis this chain was created with.
fn check_genesis_identity<S: BlockSource + ?Sized>(source: &S, report: &mut VerificationReport) {
    match source.block_at(BlockHeight::GENESIS) {
        Ok(Some(genesis)) => {
            let actual = genesis.hash();
            if actual != report.genesis_hash {
                report.findings.push(Finding::new(
                    FindingKind::GenesisMismatch,
                    Location::at_height(BlockHeight::GENESIS).with_block_hash(actual),
                    format!(
                        "chain records genesis {} but the block at height 0 hashes to {actual}",
                        report.genesis_hash
                    ),
                ));
            }
            if &genesis.header.network_id != source.network_id() {
                report.findings.push(Finding::new(
                    FindingKind::NetworkMismatch,
                    Location::at_height(BlockHeight::GENESIS).with_block_hash(actual),
                    format!(
                        "chain records network {} but genesis declares {}",
                        source.network_id(),
                        genesis.header.network_id
                    ),
                ));
            }
        }
        Ok(None) => report.findings.push(Finding::new(
            FindingKind::MissingBlock,
            Location::at_height(BlockHeight::GENESIS),
            "the chain has no genesis block",
        )),
        Err(error) => report.findings.push(Finding::new(
            FindingKind::DecodeError,
            Location::at_height(BlockHeight::GENESIS),
            format!("the genesis block could not be read: {error}"),
        )),
    }
}

/// Loads the parent header needed to start a partial-range walk.
fn load_parent<S: BlockSource + ?Sized>(
    source: &S,
    from: BlockHeight,
    report: &mut VerificationReport,
) -> Result<BlockHeader, ()> {
    let parent_height = BlockHeight(from.value() - 1);
    match source.block_at(parent_height) {
        Ok(Some(block)) => Ok(block.header),
        Ok(None) => {
            report.findings.push(Finding::new(
                FindingKind::MissingBlock,
                Location::at_height(parent_height),
                format!("verification of heights from {from} needs the block at {parent_height}"),
            ));
            Err(())
        }
        Err(error) => {
            report.findings.push(Finding::new(
                FindingKind::DecodeError,
                Location::at_height(parent_height),
                format!("the parent block at height {parent_height} could not be read: {error}"),
            ));
            Err(())
        }
    }
}
