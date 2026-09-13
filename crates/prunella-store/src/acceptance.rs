//! The block-acceptance boundary.
//!
//! Storage never decides for itself whether a block may be committed. It builds an
//! [`AcceptanceContext`], hands it to a [`BlockAcceptancePolicy`], and commits only if
//! the policy returns an [`Accepted`] token. Because the only way to obtain that token
//! is [`AcceptanceContext::accept`], and because the token borrows the context it came
//! from, a policy cannot hand back a token minted for some other block.
//!
//! Today the only policy is [`LocalDeterministicPolicy`], which accepts a block exactly
//! when `prunella-verify` finds nothing wrong with it. A consensus engine would be a
//! second implementation of this one trait: it would decide acceptance however it
//! likes and then return the same token. Nothing in the storage format, the canonical
//! encoding or the verification rules would change. No consensus mechanism is
//! implemented here.

use core::marker::PhantomData;
use prunella_core::{Block, BlockHeader, ChainHead, Hash, NetworkId};
use prunella_verify::{BlockContext, Finding, TxIdLookup, check_block};

/// Proof that a policy accepted a specific block against a specific context.
///
/// The lifetime ties the token to the context it was minted from, so a token cannot be
/// cached and replayed for a later block.
pub struct Accepted<'a>(PhantomData<&'a ()>);

impl core::fmt::Debug for Accepted<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Accepted")
    }
}

/// Everything a policy is told about the chain a block would extend.
pub struct AcceptanceContext<'a> {
    /// The chain the block must belong to.
    pub network_id: &'a NetworkId,
    /// The genesis this chain was created with.
    pub genesis_hash: Hash,
    /// The current head, or `None` when the genesis block itself is being committed.
    pub head: Option<ChainHead>,
    /// The parent header, or `None` for genesis.
    pub parent: Option<&'a BlockHeader>,
    /// Transaction ids already committed to this chain, including any committed
    /// earlier in the same write transaction.
    pub committed_transactions: &'a dyn TxIdLookup,
}

impl<'a> AcceptanceContext<'a> {
    /// Mints the token that allows this block to be committed.
    ///
    /// Calling this is the decision. A policy that calls it has accepted the block.
    #[must_use]
    pub fn accept(&self) -> Accepted<'a> {
        Accepted(PhantomData)
    }

    /// Builds the verification context matching this acceptance context.
    #[must_use]
    pub fn block_context(&self) -> BlockContext<'a> {
        BlockContext {
            expected_network: self.network_id,
            parent: self.parent,
            expected_hash: None,
            committed_transactions: Some(self.committed_transactions),
        }
    }
}

/// Why a policy refused a block.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AcceptanceError {
    /// The block broke one or more deterministic rules.
    #[error("block failed local validation with {} finding(s): {}", .findings.len(), render(.findings))]
    Invalid {
        /// Every rule the block broke, with exact locations.
        findings: Vec<Finding>,
    },
    /// The policy declined for a reason of its own.
    ///
    /// Unused by [`LocalDeterministicPolicy`]; reserved for policies whose decision is
    /// not purely a function of the block and its parent.
    #[error("block was not accepted: {reason}")]
    Declined {
        /// The policy's explanation.
        reason: String,
    },
}

fn render(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// Decides whether a block may be committed.
///
/// This is the seam a consensus engine would occupy. Implementors may consult
/// whatever they like, but must return [`AcceptanceContext::accept`] to allow a commit.
pub trait BlockAcceptancePolicy: Send + Sync {
    /// A stable name, reported by tooling so operators can see which policy is in force.
    fn name(&self) -> &'static str;

    /// Judges one block.
    ///
    /// # Errors
    ///
    /// Returns [`AcceptanceError`] when the block must not be committed.
    fn evaluate<'a>(
        &self,
        context: &AcceptanceContext<'a>,
        block: &Block,
    ) -> Result<Accepted<'a>, AcceptanceError>;
}

/// Accepts a block exactly when deterministic local validation finds nothing wrong.
///
/// This is the only policy Prunella ships today, and it is the meaning of "a block may
/// be appended only after deterministic local validation succeeds": acceptance is a
/// pure function of the block, its parent and the transaction ids already committed,
/// so two instances presented with the same block in the same chain state always
/// reach the same decision.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalDeterministicPolicy;

impl BlockAcceptancePolicy for LocalDeterministicPolicy {
    fn name(&self) -> &'static str {
        "local-deterministic"
    }

    fn evaluate<'a>(
        &self,
        context: &AcceptanceContext<'a>,
        block: &Block,
    ) -> Result<Accepted<'a>, AcceptanceError> {
        let findings = check_block(&context.block_context(), block);
        if findings.is_empty() {
            Ok(context.accept())
        } else {
            Err(AcceptanceError::Invalid { findings })
        }
    }
}
