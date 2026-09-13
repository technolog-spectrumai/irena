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
| **Irena** | The company — genesis, share register, voting rules — notarised on the ledger; its votes, frozen, signed, counted, recorded, verifiable; the shareholder meetings that group them; and the resolutions that turn a passed vote into company change | Whether the register names the real owners |

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
| [governance.md](governance.md) | **Start here.** How a company decides and how the chain proves it, in plain language, walked through a real seven-block example |
| [IRENA_V1.md](IRENA_V1.md) | The company model, the record envelope, notarisation, amendment and reconstruction, the vote lifecycle, shareholder meetings, resolutions, and the road ahead |
| [BORNITE_V1.md](BORNITE_V1.md) | **Normative.** The frozen voting types, rules grammar and evaluation algorithm |
| [PROTOCOL_V1.md](PROTOCOL_V1.md) | **Normative.** The frozen ledger wire protocol |
| [docs/irena-cli.md](docs/irena-cli.md) | The `irena` binary |
| [docs/bornite-cli.md](docs/bornite-cli.md) | The `bornite` binary |
| [docs/cli.md](docs/cli.md) | The `prunella` binary |

```console
$ prunella keygen --out k.key
$ irena --chain acme.chain init --network acme-net --company acme \
      --genesis company.xml --signing-key k.key \
      --notary-id notary-07 --notary-name "Jane Roe" --notary-at 2026-03-01T09:30:00Z
$ irena --chain acme.chain show                     # identity, register, rules at the head
$ irena --chain acme.chain publish-shares --file shares-v2.xml --signing-key k.key \
      --supersedes <tx that provides the register> --notary-id notary-07 \
      --notary-name "Jane Roe" --notary-at 2026-04-01T10:00:00Z
$ irena --chain acme.chain shares --at 0            # the register as it was founded
$ prunella --chain acme.chain export --out acme.xml  # every record readable in its block

$ irena --chain acme.chain vote new --subject "Approve the accounts" \
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

$ irena --chain acme.chain meeting new --title "AGM 2026" \
      --scheduled-at 2026-06-01T10:00:00Z --state m.state
$ irena --chain acme.chain meeting add-item --state m.state \
      --title "Approve the accounts" --proposal-digest 2222…
$ irena --chain acme.chain meeting convene --state m.state --signing-key k.key …
$ irena --chain acme.chain meeting open --state m.state    # one frozen vote per item
$ irena --chain acme.chain meeting cast --state m.state --item 2 --ballot a2.ballot
$ irena --chain acme.chain meeting close --state m.state
$ irena --chain acme.chain meeting finalize --state m.state --signing-key k.key …
$ irena --chain acme.chain meeting verify --tx e214…      # the meeting and every vote

$ irena --chain acme.chain resolution digest --file new-register.xml  # what to vote on
$ irena --chain acme.chain resolution create --meeting cd54… --item 1 --vote 864a… \
      --title "Buy out carol" --target share-structure --file new-register.xml --state r.state
$ irena --chain acme.chain resolution finalize --state r.state --signing-key k.key …
$ irena --chain acme.chain resolution execute --state r.state --signing-key k.key …
$ irena --chain acme.chain resolution verify --execution c2ca…   # vote → amendment

$ bornite evaluate --rules rules.xml --vote vote.xml     # the same rules, no ledger at all
```

| Crate | Responsibility |
|---|---|
| [`irena-core`](crates/irena-core) | The company model — a genesis that is the whole company (identity, flat share register with signing keys, nested voting rules), notarisation, the record envelope — and its strict XML |
| [`irena-ledger`](crates/irena-ledger) | Prunella integration: found, amend, and **reconstruct** the company at any height from the genesis and the amendments in chain order |
| [`irena-vote`](crates/irena-vote) | Electorate derivation, the vote lifecycle, signed ballots, the final record and its verification from the chain alone |
| [`irena-meeting`](crates/irena-meeting) | Shareholder meetings: an agenda of informational and vote items, convened and finalised on the ledger, every vote frozen on its own |
| [`irena-resolution`](crates/irena-resolution) | The governance-closing loop: a passed vote becomes a formal resolution, and an amendment resolution authorises exactly one company amendment |
| [`irena-cli`](crates/irena-cli) | The `irena` binary |

## Decisions

Each choice, what it costs, and where it can go. Recorded so the trade-offs are read
before they are re-argued.

| Choice | Consequence | Possible upgrade / simplification / integration |
|---|---|---|
| **Three layers, no bridge.** Irena imports Prunella and Bornite; neither imports Irena or the other | Company semantics live in one place; a test greps the engines for `irena`/`bornite`/`prunella` | Any other application (a non-profit, a swarm) is another layer beside Irena reusing both engines unchanged |
| **Rust only, Borsh canonical bytes, BLAKE3, Ed25519 strict** | One representation per value; no floats anywhere (`clippy::float_arithmetic` denied) | A V2 protocol is new types beside V1, never a change to V1 |
| **Prunella V1 frozen with golden vectors and an independent oracle** | Dependency upgrades cannot move a V1 value silently; Merkle roots were changed *before* the freeze so inclusion proofs are V1 | Consensus attaches at the documented acceptance seam without touching encoding or storage |
| **Nested XML payloads (transport v2)** | Company records are readable inside exported blocks; the importer cuts raw source bytes so ids match; v1 documents still read | Any application whose payload is one XML element gets the same readability for free |
| **One Merkle implementation, many domains** (`TreeTags`) | Irena's ballot commitment reuses Prunella's tree and proofs in its own domain | Any future list commitment (meeting minutes, board decisions) is one `TreeTags` constant |
| **One company per chain; the genesis is the whole company** | No `--company` on any command; a fresh chain is a company from block 0; no partial company ever exists | Several companies would be several chains — or, later, a Placidia-level index over chains |
| **Reconstruction, not resolution per kind** | The company at height *h* is genesis + amendments applied in order; each part knows which transaction provides it | Deltas (add a holder) instead of full replacements are a new record kind applied by the same walk |
| **Full-replacement amendments** | Simple to verify and to read; a register change repeats the whole register | A delta kind if registers grow large; the reconstruction walk does not change |
| **Flat shares: one share, one vote** | `weight = shares` in one function; a keyless holder counts towards quorum but cannot sign | Share classes are a new `<share-structure>` body version and one extra factor in that function |
| **Signing keys live in the share register** | No key table anywhere else; keys are amended like any company data | Stage 4 identities can add key rotation as an `identity`-like part without touching votes |
| **Notarisation required on every record** — id, name, optional address, `at` in canonical UTC | Real-world authority enters in one place; `at` is attested metadata and never orders anything | Stage 4 can bind notary ids to keys; a notarisation could carry more attestations without changing the envelope |
| **Bornite's `<voting-rules>` nested unchanged** | The same rules bytes mean the same rules in a file, a genesis or an amendment; Bornite never sees a company | Board rules (stage 3) are another `<voting-rules>` under another part |
| **Records pinned by transaction id, never by height or time** | A vote snapshot pins the exact bytes it was decided against; amendments after the freeze cannot reach it | — |
| **Vote state as a canonical Borsh file; ballots as files** | Every lifecycle step is one command; a holder signs on their own machine | Stage 5 UI drives the same `VoteV1` in memory; a meeting holds several |
| **A meeting is a container, not a company part** | Meeting records carry no `supersedes` and reconstruction ignores their namespace, so no meeting can silently change the company | Turning a passed motion into an amendment is stage 2, and adds a record kind rather than changing this one |
| **The meeting id is its convening transaction** | Unique and unforgeable without a nonce or a registry; nothing can claim to be a meeting that was never convened | — |
| **Each vote item freezes independently at `meeting open`** | Every vote has its own snapshot, electorate and id, and verifies alone without the meeting; one command fixes them all at one height | Per-item opening, or freezing at a scheduled height, are both additions to `open` rather than changes to a vote |
| **A vote's subject is `item <n>: <title>`** | Two items with the same proposal are still two votes, and a vote traces back to its item; `VotesBelong` can detect a swapped reference | A structured item reference would replace the string without touching the vote |
| **Only convening and finalisation reach the chain** | The formal facts are on the ledger; drafting, casting and counting stay local, so a UI cannot fill the chain with noise | Intermediate attestations (a quorum roll call) would be new record kinds |
| **`VotesVerify` and `VotesBelong` are separate checks** | A real, valid vote from another meeting passes the first and fails the second — a forgery a per-vote check cannot see | — |
| **Votes never change the company; resolutions only describe authority** | Four record kinds in a row (meeting, vote, resolution, execution) and the amendment is still the one record that changes reconstructed state; reconstruction ignores all four namespaces | A future kind of authority (a board decision) plugs in at the resolution step without touching amendments |
| **The proposal digest is the digest of the amendment body** | Shareholders vote on the register or the rules themselves, so what is executed is provably what was approved — no document can drift from what it authorises | A proposal envelope wrapping prose *and* body would let the voted document read better while binding the same bytes |
| **A resolution carries the body, not just its digest** | The chain is self-contained: an auditor reads what was decided without any external file | — |
| **Execution refuses a stale base** | A resolution passed against a company that has since changed cannot land on one the voters never saw; the second of two competing resolutions must be re-voted | Per-part judgement already softens it (a rules resolution survives a register change); a rebase-and-reconfirm step could soften it further |
| **The execution record is the only link from amendment to authority** | The company record format is untouched, so every existing amendment stays valid and readable | An optional `authority` attribute on the amendment envelope would make the link visible from the amendment's side too |
| **No authorisation roles in V1** | Any key may publish a resolution; the notarisation is the only authority, exactly as for company records | Stage 4 adds who may sign what, checked at reconstruction |
| **Ballots are not secret and live in the final record** | A record is verifiable from the chain alone, nine named checks | Secret ballots would need a different commitment scheme and are explicitly out of scope |
| **Final vote record as canonical Borsh, not XML** | Byte-exact re-encoding is one of the verification checks; the export shows it as base64 | An XML rendering for readers is a projection that can be added without changing what is verified |
| **Strict readers, issues collected and sorted** | Unknown elements refused; a document with three problems is fixed in one round | — |
| **Breaks reported, never repaired** | Anything written around Irena stops reconstruction at that exact transaction | — |

## Road ahead

Planned, in order — see [IRENA_V1.md §10](IRENA_V1.md):

1. ~~shareholder meetings and votes using Bornite~~ — **done**, see
   [IRENA_V1.md §8](IRENA_V1.md);
2. ~~resolutions and the company-state changes they cause~~ — **done**, see
   [IRENA_V1.md §9](IRENA_V1.md);
3. board membership, meetings and decisions;
4. identities and authorisation;
5. Placidia coordination and UI.

Also deliberately absent: share classes (the company has flat shares), secret ballots,
delegation, proxies, networking, consensus.

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
