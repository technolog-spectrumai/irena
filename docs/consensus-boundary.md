# The consensus integration boundary

**No consensus mechanism is implemented.** Malachite is not integrated, and nothing in
this repository attempts to. This document describes the seam that was left for one, so
that adding it later does not mean rewriting storage or verification.

## Today's rule

> A block may be appended only after deterministic local validation succeeds.

## The seam

Storage never decides for itself whether a block may be committed. It builds an
`AcceptanceContext`, hands it to a `BlockAcceptancePolicy`, and commits only against the
`Accepted` token that policy returns.

```rust
pub struct Accepted<'a>(PhantomData<&'a ()>);

pub trait BlockAcceptancePolicy: Send + Sync {
    fn name(&self) -> &'static str;
    fn evaluate<'a>(&self, context: &AcceptanceContext<'a>, block: &Block)
        -> Result<Accepted<'a>, AcceptanceError>;
}

pub struct AcceptanceContext<'a> {
    pub network_id: &'a NetworkId,
    pub genesis_hash: Hash,
    pub head: Option<ChainHead>,          // None while genesis is being committed
    pub parent: Option<&'a BlockHeader>,  // None for genesis
    pub committed_transactions: &'a dyn TxIdLookup,
}
```

Two properties make this a real gate rather than a convention:

1. The only way to obtain an `Accepted` is `AcceptanceContext::accept`. A policy cannot
   fabricate one, and the commit path cannot run without one.
2. The token borrows the context it came from. A policy cannot cache a token minted for
   an earlier block and hand it back for a later one — the lifetime will not allow it.

## The only policy today

```rust
impl BlockAcceptancePolicy for LocalDeterministicPolicy {
    fn evaluate<'a>(&self, context: &AcceptanceContext<'a>, block: &Block)
        -> Result<Accepted<'a>, AcceptanceError>
    {
        let findings = check_block(&context.block_context(), block);
        if findings.is_empty() { Ok(context.accept()) }
        else { Err(AcceptanceError::Invalid { findings }) }
    }
}
```

Acceptance is a pure function of the block, its parent and the transaction ids already
committed. Two instances presented with the same block in the same chain state always
reach the same decision.

## Adding consensus later

A consensus engine becomes a second implementation of the same trait:

```rust
struct MalachiteConsensusPolicy { /* engine handle */ }

impl BlockAcceptancePolicy for MalachiteConsensusPolicy {
    fn name(&self) -> &'static str { "malachite" }

    fn evaluate<'a>(&self, context: &AcceptanceContext<'a>, block: &Block)
        -> Result<Accepted<'a>, AcceptanceError>
    {
        // 1. Apply the deterministic rules first — consensus never makes an invalid
        //    block valid.
        let findings = check_block(&context.block_context(), block);
        if !findings.is_empty() { return Err(AcceptanceError::Invalid { findings }); }

        // 2. Then apply whatever consensus requires: a commit certificate, a quorum of
        //    signatures, a proposer check, finality.
        if !self.is_decided(block) {
            return Err(AcceptanceError::Declined { reason: "not yet decided".into() });
        }
        Ok(context.accept())
    }
}

let store = LocalChainStore::open_with_policy(path, Box::new(policy))?;
```

Nothing else changes: not the canonical encoding, not the block hashes, not the storage
format, not the verification rules, not the XML format, not the CLI.

## What a policy may and may not do

**May:** consult network state, a validator set, a certificate, a clock, a quorum; keep
state of its own; decline for reasons that are not about the block's contents; be
non-deterministic across instances, since different nodes see different messages.

**May not:**

* **Accept a block that breaks the deterministic rules.** Consensus decides *which* of
  several valid histories wins; it never makes an invalid block valid. A policy that
  skips `check_block` breaks the core invariant.
* **Mutate a committed block.** There is no API for it, and there will not be.
* **Assume it will be called again for the same block.** Acceptance is evaluated inside
  the store's write transaction, immediately before the commit.
* **Assume the chain can fork.** The current store is strictly append-only and refuses
  a different block at a committed height (`ForkedHistory`). A consensus engine that
  needs to reorganise the chain would need a new capability in the store, designed
  deliberately and documented here — it is not something a policy can reach around.

## Verification is independent of policy

`prunella verify` applies the deterministic rules regardless of which policy is
installed. A chain built under a consensus policy still has to be a valid chain, and it
is checked as one. Policy governs what gets in; verification governs what is true.
