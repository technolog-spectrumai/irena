# Backup and recovery

Prunella never repairs a chain. This document explains what to do instead, and why that
is the right trade.

## Why nothing auto-repairs

A ledger's value is that what it says today is what it said yesterday. A store that
silently rebuilds an index, re-derives a head, or drops an undecodable block has made
the chain self-consistent again and destroyed the evidence of what went wrong. You would
no longer be able to tell a disk fault from tampering, or say when either began.

So `LocalChainStore::open` reports `Inconsistent` naming exactly what disagrees with what,
and stops. Recovery is a deliberate act, performed from a backup you chose.

## Taking a backup

```console
$ prunella --chain demo.chain verify
$ prunella --chain demo.chain export --out backup-$(date +%Y%m%d).xml
```

Verify first. A backup of a chain you have not verified is a backup of a possibly
damaged chain.

A `full` export preserves everything needed to reconstruct the chain exactly: network
id, genesis hash, every block height and hash, transaction order, payload bytes, signer
keys and signatures. It is plain text, so it survives formats and tooling that the redb
file will not.

### Incremental backups

```console
$ prunella --chain demo.chain export --out inc-0004-0009.xml --from 4 --to 9
```

A `range` export transfers new blocks without resending the whole chain. Ranges must be
applied in ascending order with no gaps; importing a range that does not continue the
target is refused with the height the chain was waiting for.

### What is not a backup

`--namespace` produces a projection. It omits transactions, so its blocks cannot
reproduce their own transaction roots, and it can neither be imported nor create a
chain. It is for handing an application the subset it cares about. Use an unfiltered
export for backups.

## Restoring

```console
$ prunella --chain restored.chain import --in backup.xml --create
$ prunella --chain restored.chain verify
```

`--create` rebuilds the genesis block from the document's own height-zero block and
checks that it hashes to the genesis the document declares, so a backup cannot found a
chain under a genesis it does not actually contain.

Always verify after restoring. An import that succeeded proves the document was accepted
block by block; a verify proves the resulting chain reads back as one whole.

### Restoring into an existing chain

```console
$ prunella --chain existing.chain import --in backup.xml --dry-run
$ prunella --chain existing.chain import --in backup.xml
```

Blocks already committed identically are skipped. A block that contradicts one already
committed is refused as a fork, with the height where the histories diverge. Import
never rewrites committed history, so this is safe to run against a chain that is
partially up to date.

## What damage looks like

| Symptom | What it means |
|---|---|
| `open` returns `Inconsistent: head records … but the block there hashes to …` | The head record and the block disagree |
| `open` returns `Inconsistent: chain records genesis … but the block at height 0 hashes to …` | The chain's root was replaced |
| `open` returns `NotAChain` | The file is not a Prunella chain, or its metadata is gone |
| `verify` reports `decode_error` at a height | Stored bytes at that height are not a block |
| `verify` reports `previous_hash_mismatch` at height N and N+1 | A block at N was rewritten; N+1's finding is the consequence |
| `verify` reports `signature_invalid` and `tx_id_mismatch` together | A payload was changed after signing |
| `verify` reports `missing_block` | A height inside the chain holds nothing; the walk stops there |

## A recovery drill

Worth running before you need it.

```console
# 1. Confirm the chain is sound, and record its head.
$ prunella --chain demo.chain verify
$ prunella --chain demo.chain status --json | grep -A3 '"head"'

# 2. Back it up.
$ prunella --chain demo.chain export --out drill.xml

# 3. Restore into a new file.
$ prunella --chain drill.chain import --in drill.xml --create

# 4. Verify the restored chain and compare heads.
$ prunella --chain drill.chain verify
$ prunella --chain drill.chain status --json | grep -A3 '"head"'
```

The two heads must be identical. If they are, your backup reconstructs the chain
exactly; if they are not, find out why before you need the backup for real.

## Operational notes

* **One chain is one file.** Copying it while a process holds it open may capture a
  torn state; prefer an XML export, or copy while nothing is writing.
* **Back up keys separately.** A chain backup contains public keys and signatures. It
  does not contain, and must not contain, signing keys.
* **Keep more than one generation.** Backups inherit whatever was wrong with the chain
  at the moment they were taken, which is why step 1 of every backup is `verify`.
* **A dry run is free.** `import --dry-run` runs every check a real import runs and
  writes nothing.
