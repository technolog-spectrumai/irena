# Architecture

Prunella is a standalone immutable ledger. It is organization-agnostic by construction:
nothing in it knows about companies, governance, voters, shareholders or any business
rule. Application meaning lives in exactly two places, neither of which Prunella
interprets:

* `Transaction::payload` — an opaque byte string, reproduced exactly and never parsed;
* `Namespace` — a label used for grouping and filtered export, compared only for
  equality.

That restriction is not a matter of taste. A ledger that understands what it stores
will eventually be asked to make decisions about it, and those decisions become rules
that two instances can disagree about.

## The core invariant

> Given the same genesis and the same ordered valid blocks, every correct Prunella
> instance derives and verifies the same immutable chain.

Everything below exists to make that true:

| Mechanism | What it rules out |
|---|---|
| A frozen specification with independent golden vectors | An upgrade quietly changing a V1 value |
| One canonical encoding, with trailing bytes rejected | Two byte strings meaning the same value |
| Domain-separated hashing | One pre-image serving two purposes |
| Derived, never supplied, header fields | A header disagreeing with its own block |
| A single implementation of the block rules | Two components reaching different verdicts |
| Genesis derived from a specification | Two instances starting from different roots |

## Crate layout

```
prunella-canonical  →  prunella-core  →  prunella-crypto  →  prunella-verify
                                                                    ↓
                                                             prunella-store
                                                                    ↓
                                                              prunella-xml
                                                                    ↓
                                                              prunella-cli
```

The graph is acyclic and every crate is testable on its own.

| Crate | Responsibility | Knows nothing about |
|---|---|---|
| `prunella-canonical` | Deterministic encoding, domain-separated BLAKE3 | Ledgers |
| `prunella-core` | Types and the four hash derivations | I/O, signatures, storage |
| `prunella-crypto` | Ed25519 signing and strict verification | Blocks, chains, storage |
| `prunella-verify` | The block rules, chain walking, reports | How a chain is stored |
| `prunella-store` | redb persistence, the acceptance boundary | XML, presentation |
| `prunella-xml` | Versioned XML transport | Hashing rules (it re-derives) |
| `prunella-cli` | Presentation | Everything else — it only calls libraries |
| `prunella-conformance` | Golden vectors, an independent encoder | Nothing: it deliberately shares no code with the implementation |

### Why verification sits below storage

`prunella-verify` defines `BlockSource`, a read-only view of a chain, and
`prunella-store::LocalChainStore` implements it. Verification therefore never depends on
redb, and the same verification code walks a persisted chain and an in-memory one. It
also means the block rules can be applied to a candidate block before anything is
written, which is what makes import atomic.

### Why crypto is isolated

`prunella-core` holds public keys and signatures as opaque byte containers and performs
no cryptography. The signature scheme can be replaced by rewriting `prunella-crypto`
alone; the ledger types, the canonical encoding and the storage format are unaffected.

## Where meaning stops

```
application  →  payload bytes + namespace label    (Prunella stores and hashes these)
                --------------------------------
prunella     →  identity, ordering, immutability, verification
```

Prunella will tell you that a transaction was signed by a key, committed at a height,
and has not changed since. It will never tell you what it means. That is the
application's job, and keeping the line here is what lets one ledger serve unrelated
applications without any of them leaking into it.

## Deliberate omissions

* **No consensus.** A block is appended only after deterministic local validation
  succeeds. The seam where a consensus engine would attach is documented in
  [consensus-boundary.md](consensus-boundary.md) and is not filled in.
* **No application indexes.** Only chain-native lookups exist: by height, by block
  hash, by transaction id. A namespace index would be the first step toward Prunella
  having an opinion about namespaces.
* **No mutation and no repair.** There is no API that rewrites a committed block, and
  no code path that quietly fixes an inconsistent chain. See
  [backup-recovery.md](backup-recovery.md).
* **No network, no HTTP, no database server.** One chain is one local file.
