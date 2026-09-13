# Storage format

One chain is one file: a [redb](https://docs.rs/redb) database holding four tables.
Block values are `prunella-canonical` encodings, so the bytes on disk are the bytes
that were hashed. A stored block cannot mean one thing to storage and something else to
verification.

Store format version: **1** (`prunella_store::STORE_FORMAT_VERSION`).

## Tables

| Table | Key | Value |
|---|---|---|
| `prunella_meta` | `&str` | chain bookkeeping, see below |
| `prunella_blocks` | `u64` height | canonical `Block` bytes |
| `prunella_height_by_hash` | 32-byte block hash | `u64` height |
| `prunella_tx_location` | 32-byte transaction id | `u64` height LE, then `u32` index LE |

### `prunella_meta`

| Key | Value |
|---|---|
| `format_version` | `u32` little-endian |
| `network_id` | UTF-8 text |
| `genesis_hash` | 32 bytes |
| `head_height` | `u64` little-endian |
| `head_hash` | 32 bytes |
| `transaction_count` | `u64` little-endian |

`genesis_hash` is recorded independently of the block at height 0. That redundancy is
the point: if someone replaces the genesis block with a self-consistent substitute, the
block still verifies against itself, and only the separately recorded hash reveals that
the chain's root was swapped.

### What is deliberately absent

There is no namespace index and no signer index. Only chain-native lookups exist —
by height, by block hash, by transaction id. A namespace-filtered export scans the
requested range instead. An index over namespaces would be the first place Prunella
started having an opinion about what a namespace means.

## Immutability

No API rewrites or removes a committed block. `append_block` at an already-occupied
height fails with `HeightOccupied`, and the only write path that tolerates a committed
height is `ExistingBlockPolicy::SkipIfIdentical`, which skips a byte-identical block and
refuses a different one as `ForkedHistory`.

## Atomicity

`ChainStore::append_blocks` opens one redb write transaction, validates and inserts
every block, updates the head and the transaction count, then commits. A rejection
anywhere returns before the commit, so the chain is left with no block written. This is
what makes XML import all-or-nothing.

## Opening a chain

`ChainStore::open` performs a constant-time consistency check:

* the metadata table exists and carries a supported `format_version`;
* the block at height 0 exists and hashes to the recorded `genesis_hash`;
* that block declares the recorded `network_id`;
* the block at `head_height` exists and hashes to `head_hash`.

Any disagreement returns `StoreError::Inconsistent` naming exactly what disagrees with
what. **Nothing is repaired.** The store does not rebuild an index, re-derive a head, or
drop a damaged block, because a ledger that quietly fixes itself has destroyed the
evidence an operator needs to find out what happened. Recovery is a deliberate act; see
[backup-recovery.md](backup-recovery.md).

This check is not a full audit. It is cheap and looks only at the chain's own
bookkeeping. Walking every block and re-deriving every hash is `prunella verify`.

## Reading

* `block_at(height)` — decodes the stored bytes; undecodable bytes give
  `CorruptBlock` naming the height, never a panic.
* `block_by_hash(hash)` — consults the hash index, then reads by height.
* `transaction(id)` — consults the transaction index, reads the block, and confirms the
  transaction at that index really has that id. A disagreeing index is reported as
  `Inconsistent`, not trusted.
* `blocks_in_range(from, to)` — iterates from a single read snapshot, so a concurrent
  append cannot make the range internally inconsistent.

## Concurrency

redb provides MVCC: readers see a consistent snapshot and never block writers. One
process may hold the database file at a time. Multiple `ChainStore` handles within a
process share the underlying database through redb's own locking.
