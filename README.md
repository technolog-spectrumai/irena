# Irena

A company and its votes on an immutable ledger. Three layers in one workspace, two of
which know nothing about the third:

```
                    irena-*        ← the company layer: knows what a share is
                   ╱        ╲
          prunella-*        bornite-*
      ledger, opaque      voting arithmetic
```

| Layer | What it is | What it does not know |
|---|---|---|
| **Prunella** | An immutable, organisation-agnostic ledger | What any payload means |
| **Bornite** | A deterministic voting engine | Who is voting, or why |
| **Irena** | The company — genesis, share register, voting rules — notarised on the ledger, and its votes: frozen, signed, counted, recorded, verifiable | Whether the register names the real owners |

Irena imports both engines directly. Neither imports Irena; no Prunella crate mentions
Bornite and no Bornite crate mentions Prunella; a test greps every engine source file
to make sure. There is no bridge.

## The rule that shapes the whole design

**The voting rules must be enough to organise a vote with no organisation behind them
at all.** The same `<voting-rules>` document organises a company AGM, a non-profit's
membership vote, or — in future — a fleet of drones deciding a peaceful transport
deployment democratically. So:

* The rules document says how to count and what passing means — weights, exclusions,
  quorum, threshold, abstentions, ties — and **nothing** about who is voting or why.
* The electorate Bornite sees is voter ids and integer weights. Where a weight comes
  from — shares, one member one vote, a drone's node count — is Irena's business, and
  Bornite never learns it.
* One schema defines the `voting-rules` element, and the standalone file and the
  on-ledger record both include it. A rules file written for one purpose is stored on a
  ledger for another **byte for byte unchanged**.
* Company truth — the share register, the founding identity — reaches the ledger only
  through a **notarised record**: who attested to it (id, name, address), when they say
  the change took effect, and optionally the digest of an external document, entered
  by hand. Every change is a new record that names the one it amends, so the full
  history is on the chain, and Prunella's XML export shows each record **nested and
  readable inside its block**.

| Document | Covers |
|---|---|
| [IRENA_V1.md](IRENA_V1.md) | The company model, the record envelope, notarisation, amendment and resolution, the vote lifecycle and its verification, the TODO list |
| [BORNITE_V1.md](BORNITE_V1.md) | **Normative.** The frozen voting types, rules grammar and evaluation algorithm |
| [PROTOCOL_V1.md](PROTOCOL_V1.md) | **Normative.** The frozen ledger wire protocol |
| [docs/irena-cli.md](docs/irena-cli.md) | The `irena` binary |
| [docs/bornite-cli.md](docs/bornite-cli.md) | The `bornite` binary |
| [docs/cli.md](docs/cli.md) | The `prunella` binary |

```console
$ prunella keygen --out k.key
$ irena --chain acme.chain init --network acme-net --company acme \
      --genesis genesis.xml --signing-key k.key \
      --notary-id notary-07 --notary-name "Jane Roe" --notary-at 2026-03-01T09:30:00Z
$ irena --chain acme.chain publish-shares --company acme --file shares.xml \
      --signing-key k.key --notary-id notary-07 --notary-name "Jane Roe" \
      --notary-at 2026-03-01T09:35:00Z
$ irena --chain acme.chain publish-rules --company acme --file rules.xml \
      --signing-key k.key --notary-id notary-07 --notary-name "Jane Roe" \
      --notary-at 2026-03-01T09:40:00Z
$ irena --chain acme.chain show --company acme          # the company at the head
$ irena --chain acme.chain shares --company acme --at 1 # the register as it was then
$ prunella --chain acme.chain export --out acme.xml      # every record readable in its block

$ irena --chain acme.chain vote new --company acme --subject "Approve the accounts" \
      --proposal-digest d0d0… --state v.state
$ irena --chain acme.chain vote freeze --state v.state    # the company as it is now, fixed
$ irena --chain acme.chain vote open --state v.state
$ irena --chain acme.chain vote ballot --state v.state --voter alice --choice yes \
      --signing-key alice.key --out alice.ballot            # alice signs, on her machine
$ irena --chain acme.chain vote cast --state v.state --ballot alice.ballot
$ irena --chain acme.chain vote close --state v.state
$ irena --chain acme.chain vote evaluate --state v.state  # Bornite counts
$ irena --chain acme.chain vote finalize --state v.state --signing-key k.key
$ irena --chain acme.chain vote verify --tx f9e5…         # from the chain alone
$ bornite evaluate --rules rules.xml --vote vote.xml     # the same rules, no ledger at all
```

| Crate | Responsibility |
|---|---|
| [`irena-core`](crates/irena-core) | The company model — genesis, flat share register with signing keys, notarisation, record envelope — and its strict XML |
| [`irena-ledger`](crates/irena-ledger) | Records on a Prunella chain: publish, amendment chains, what the company is at any height |
| [`irena-vote`](crates/irena-vote) | Electorate derivation, the vote lifecycle, signed ballots, the final record and its verification from the chain alone |
| [`irena-cli`](crates/irena-cli) | The `irena` binary |

Not yet built, and listed as such in [IRENA_V1.md §8](IRENA_V1.md): share classes (the
company has flat shares), shareholder meetings, board meetings and decisions.

---

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
  alone, without the block's transactions; the tree is generic over its hash domains so
  an application can commit to lists of its own with the same code.
* **Lossless versioned XML export and import** — atomic, idempotent, dry-runnable —
  preserving payload bytes exactly, with a payload that is itself an XML element
  carried nested and readable rather than as base64.
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

# Bornite

A standalone deterministic voting engine. Frozen electorate + rules + ballots → the
same auditable result on every machine.

* No floating point (`clippy::float_arithmetic` is denied across the workspace).
  Fractions are exact numerator/denominator pairs; every comparison is a `u128`
  cross-multiplication that provably cannot overflow.
* No hash-map iteration, no clock, no randomness, no locale, no dependence on input
  order — each ruled out by construction and by a test.
* The tie rule is the boundary rule: the threshold comparison is always strict, and a
  result landing exactly on it is decided by `<tie treatment="reject|accept"/>`.
* Contradictions between rules and electorate are refused with every issue reported
  together, never resolved by picking a side.

| Crate | Responsibility |
|---|---|
| [`bornite-core`](crates/bornite-core) | Versioned types, exact fractions, checked weight arithmetic |
| [`bornite-rules`](crates/bornite-rules) | `VotingRulesV1`, rule-versus-electorate validation |
| [`bornite-eval`](crates/bornite-eval) | The fixed-order algorithm and `VoteEvaluationV1` |
| [`bornite-xml`](crates/bornite-xml) | Strict readers for `<voting-rules>` and `<vote>` documents |
| [`bornite-cli`](crates/bornite-cli) | The `bornite` binary |

Schemas for all three layers live under [`schemas/`](schemas/).

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
[`schemas/prunella-chain-v2.xsd`](schemas/prunella-chain-v2.xsd); version 1 documents
([`prunella-chain-v1.xsd`](schemas/prunella-chain-v1.xsd)) still import.

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
