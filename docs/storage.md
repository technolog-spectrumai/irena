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

## The abstraction and its implementation

`ChainStorage` is the contract every chain store satisfies; `LocalChainStore` is the one
implementation. The trait carries the invariants — genesis exactly once, contiguous
heights, matching parents and network ids, unique transaction ids, verified signatures,
recomputed roots and hashes, immutable committed blocks, atomic appends, no silent
repair — and its `verify_from` defers to `prunella-verify`, so no implementation can
grow its own opinion of what a valid chain is.

**A successful append means locally accepted after full deterministic validation.** It
does not mean distributed finality. There is no consensus here, and this module is
written so that adding one later changes nothing in it: see
[consensus-boundary.md](consensus-boundary.md).

## Immutability

No API rewrites or removes a committed block. There is no update method and no delete
method, and `append_block` never overwrites.

Offering a block at a height that is already committed has exactly two outcomes:

| Offered block | Result |
|---|---|
| Byte-identical to the committed one | `AppendStatus::AlreadyPresent`, nothing written |
| Different in any way | `StoreError::ForkedHistory`, nothing written |

`AlreadyPresent` is a success, not an error: re-offering a block the chain already holds
is how a retried or replayed transfer behaves. Rewriting history is the thing that must
never happen, and a *different* block at a committed height is exactly that, so it is
refused. The comparison is over the whole block rather than its hash alone, so a block
that hashes the same but differs in some byte the header does not commit to is still
caught.

## Atomicity

`LocalChainStore::append_blocks` opens one redb write transaction, validates and inserts
every block along with every index entry, updates the head and the transaction count,
then commits. A rejection anywhere returns before the commit, so the chain, the hash
index, the transaction index and the head record are all left exactly as they were —
either the complete block and all its indexes are stored, or nothing changed. This is
what makes a failed append and a failed XML import equally safe.

`append_block` is the single-block case of the same path.

## Durability

Write transactions are committed with `redb::Durability::Immediate`, set explicitly
rather than relied on as a default: a ledger whose commits were buffered could lose a
block it had already reported as appended. By the time `append_block` returns, the block
is on disk.

`LocalChainStore::close` releases the file and reports whether that succeeded; dropping
the store does the same silently. Because every commit is already durable, an abrupt
drop — or a process that dies — loses nothing that was reported as appended, and redb's
commit protocol means a half-written commit is not visible on reopen.

## Opening a chain

`LocalChainStore::open` performs a constant-time consistency check:

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

* `get_block(height)` — decodes the stored bytes; undecodable bytes give
  `CorruptBlock` naming the height, never a panic.
* `get_block_by_hash(hash)` — consults the hash index, then reads by height.
* `get_transaction(id)` — consults the transaction index, reads the block, and confirms the
  transaction at that index really has that id. A disagreeing index is reported as
  `Inconsistent`, not trusted.
* `iter_blocks(start, end)` — iterates from a single read snapshot, so a concurrent
  append cannot make the range internally inconsistent.

## Concurrency

redb provides MVCC: readers see a consistent snapshot and never block writers. One
process may hold the database file at a time. Multiple `LocalChainStore` handles within a
process share the underlying database through redb's own locking.
