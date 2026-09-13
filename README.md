# Prunella

A standalone, organization-agnostic blockchain and immutable ledger, in Rust.

Prunella knows nothing about companies, governance, voters, shareholders or any business
rule. Application meaning lives entirely in opaque transaction payload bytes and an
uninterpreted namespace label. That restriction is the point: a ledger that understands
what it stores will eventually be asked to make decisions about it, and those decisions
become rules two instances can disagree about.

## Protocol V1 is frozen

[`PROTOCOL_V1.md`](PROTOCOL_V1.md) is the normative specification: every canonical type,
field order, integer representation, Borsh rule, domain tag and derivation. It does not
change. A future format change is a V2, introduced as new types alongside V1.

Permanent golden vectors under [`test-vectors/v1/`](test-vectors/v1/) hold the freeze.
Each carries human-readable input plus expected canonical bytes, hashes, signatures and
inclusion proofs, and is checked against the implementation **and** an independent
encoder written from the specification alone — so a dependency upgrade cannot move a
V1 value without failing a test that names it.

## Core invariant

> Given the same genesis and the same ordered valid blocks, every correct Prunella
> instance derives and verifies the same immutable chain.

```console
$ prunella --chain a.chain init --network prunella.example
genesis: 4cfcf0687ebd6e97e1ae8aab69ed46793088cfb705c63c91475e1fd75fed507c

$ prunella --chain b.chain init --network prunella.example   # different machine, no contact
genesis: 4cfcf0687ebd6e97e1ae8aab69ed46793088cfb705c63c91475e1fd75fed507c
```

## What it does

* **Immutable, append-only chain.** No API rewrites a committed block. No code path
  repairs an inconsistent one.
* **Deterministic canonical serialization.** Every hash is derived from a specified byte
  encoding with one representation per value. No hash is ever taken from `Debug`,
  `Display`, JSON or XML text.
* **BLAKE3 hashing, Ed25519 signatures**, with strict verification.
* **Full verification from genesis to head**, reporting every defect with its exact
  height, transaction index and identifier.
* **Merkle transaction roots with inclusion proofs**, verifiable against a block header
  alone, without the block's transactions.
* **Lossless versioned XML export and import** — atomic, idempotent, dry-runnable —
  preserving payload bytes exactly.
* **A storage abstraction** with one local persistent implementation: atomic appends,
  height/hash/transaction indexes, clean close and reopen.
* **A narrow acceptance boundary** so consensus can be added later without touching
  storage, encoding or verification.

## What it deliberately does not do

No consensus and no Malachite. No Python, PyO3, maturin or Django. No PostgreSQL. No
HTTP server. No application-specific indexes. No company-specific types. No mutable
committed blocks. No silent chain repair.

A block may be appended only after deterministic local validation succeeds.

## Quick start

```console
$ cargo build --release
$ export PRUNELLA_CHAIN=demo.chain

$ prunella keygen --out signer.key
$ prunella init --network demo
$ prunella append --signing-key signer.key --namespace app.demo --nonce 1 \
      --payload-hex 48656c6c6f
$ prunella verify
$ prunella proof <tx-id> --out proof.bin
$ prunella export --out backup.xml
$ prunella --chain restored.chain import --in backup.xml --create
$ prunella --chain restored.chain verify
```

Both chains now have the same genesis hash, the same head hash and the same block hash
at every height.

## Workspace

```
prunella-canonical  →  prunella-core  →  prunella-crypto  →  prunella-verify
                                                                    ↓
                                                             prunella-store
                                                                    ↓
                                                              prunella-xml
                                                                    ↓
                                                              prunella-cli
```

| Crate | Responsibility |
|---|---|
| [`prunella-canonical`](crates/prunella-canonical) | Deterministic encoding, domain-separated BLAKE3 |
| [`prunella-core`](crates/prunella-core) | Ledger types and the four hash derivations |
| [`prunella-crypto`](crates/prunella-crypto) | Ed25519 signing and strict verification |
| [`prunella-verify`](crates/prunella-verify) | The block rules, chain walking, structured reports |
| [`prunella-store`](crates/prunella-store) | The `ChainStorage` contract, a redb implementation, the acceptance boundary |
| [`prunella-xml`](crates/prunella-xml) | Versioned XML transport |
| [`prunella-cli`](crates/prunella-cli) | The `prunella` binary |
| [`prunella-conformance`](crates/prunella-conformance) | Golden vectors and an independent encoder that freeze V1 |

## Documentation

| Document | Covers |
|---|---|
| [PROTOCOL_V1.md](PROTOCOL_V1.md) | **Normative.** The frozen wire protocol |
| [architecture.md](docs/architecture.md) | Crate layout, why the boundaries fall where they do |
| [api.md](docs/api.md) | The public Rust API of every crate |
| [canonicalization.md](docs/canonicalization.md) | The encoding, the domain tags, every derivation |
| [storage.md](docs/storage.md) | On-disk tables, immutability, atomicity, consistency |
| [validation.md](docs/validation.md) | Every rule and the finding it produces |
| [xml-format.md](docs/xml-format.md) | Schema, versioning, kinds, import semantics |
| [cli.md](docs/cli.md) | Every command, with examples and exit codes |
| [backup-recovery.md](docs/backup-recovery.md) | Backups, restores, what damage looks like |
| [consensus-boundary.md](docs/consensus-boundary.md) | The seam a consensus engine would occupy |

The XML schema is published at
[`schemas/prunella-chain-v1.xsd`](schemas/prunella-chain-v1.xsd).

## Development

```console
$ cargo fmt --all
$ cargo clippy --all-targets --all-features -- -D warnings
$ cargo test --workspace
```

`cargo test --workspace` includes the conformance suite and the property/fuzz suites;
they run in CI as-is, with no separate step and no nightly toolchain. Property tests use
[proptest](https://docs.rs/proptest) with fixed case counts, so a run is bounded and
reproducible.

Requires Rust 1.94 or newer (edition 2024).
