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
| **Irena** | The company — genesis, share register, **decision channels** — notarised on the ledger; its votes and signed decisions, frozen, counted, recorded, verifiable; the meetings that group votes; and the resolutions that turn a channel's approval into company change | Whether the register names the real owners, or whether the configured channels are the ones the law recognises |

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
* Company truth — the share register, the founding identity, **who may decide and
  how** — reaches the ledger only through a **notarised record**: who attested to it (id, name, address), when they say
  the change took effect, and optionally the digest of an external document, entered
  by hand. Every change is a new record that names the one it amends, so the full
  history is on the chain, and Prunella's XML export shows each record **nested and
  readable inside its block**.

## Decision channels

Who may decide, and how, is company data — not code. A **decision channel** has an id,
an **actor source** and a **mode**:

```text
channel      := id + actor source + mode + scope?
actor source := share-register | roster (members listed inline)
mode         := individual | collective(<voting-rules>)
scope        := the company parts this channel may amend; absent, it amends none
```

```xml
<decision-channels>
  <channel id="shareholders" mode="collective">      <!-- the holders vote, weight = shares -->
    <actors source="share-register"/>
    <voting-rules version="1.0">…</voting-rules>
  </channel>
  <channel id="board" mode="collective">             <!-- three directors vote, weighted -->
    <actors source="roster">
      <member id="chen"   weight="2"/>
      <member id="okafor"/>
      <member id="vance"/>
    </actors>
    <voting-rules version="1.0">…</voting-rules>
  </channel>
  <channel id="ceo" mode="individual">               <!-- one director signs -->
    <actors source="roster">
      <member id="chen"/>
    </actors>
    <scope><amend part="decision-channels"/></scope>  <!-- and nothing else -->
  </channel>
</decision-channels>
```

**Silence denies.** A channel with no `<scope>` records declarative decisions and
amends nothing, so a channel written without thinking about scope holds no power over
the company. A channel with one may amend exactly the parts it lists, and the scope
that applies is the one in the channel set the decision was frozen against.

Nobody carries a key here. A holder or a member is an **id**, and the key that id
signs with lives once, in the identities record:

```xml
<identities>
  <person id="chen" name="M. Chen" document-id="SG-S8811234K" key="ca93…"/>
  <person id="vance" name="R. Vance"/>            <!-- no key: counts, cannot sign -->
  <person id="jane" name="Jane Roe" key="fd17…"/> <!-- holds nothing, writes everything -->
</identities>
<authorisation>
  <signer person="jane" records="company"/>       <!-- amendments to any part -->
  <signer person="jane" records="governance"/>    <!-- meetings, votes, decisions… -->
</authorisation>
```

`shareholders`, `board` and `ceo` are labels a notary chose. Irena knows four words —
`share-register`, `roster`, `individual`, `collective` — and no organ, office or legal
system. The same code runs every configuration:

| Configuration | What Irena does | Looks like |
|---|---|---|
| `share-register` + `collective` | resolves the holders, weight = shares, runs a vote under the nested rules | a shareholders' meeting |
| `roster` + `collective` | resolves the members at their declared weights, runs a vote under the nested rules | a board, a committee |
| `roster` + `individual` | resolves to exactly one member, who signs a decision | a sole executive |
| `share-register` + `individual` | resolves to the one holder, who signs | a single-member company |

A channel is resolved at a height, like everything else: a vote or a decision freezes
the company, resolves the channel *as the company then stood*, and pins the register and
the channel set by transaction id. A resolution then names the channel and the exact
record it decided through — a meeting item and vote, or a signed decision — and the
rest of the loop (proposal digest, stale-base rule, amendment, execution record,
verification) is one code path whichever it was. The channel set is itself a company
part, amended through the same notarised `supersedes` mechanism as the register, so
*who decides* has a history on the chain like everything else.

One person rewriting who decides is held to the **self-demotion rule**: a channel-set
amendment executed on an individual decision must leave its signer with no seat they
did not already hold, and every seat they keep unchanged. A sole director may abolish
their own channel or hand the company to a collective; they may not add themselves
anywhere or thin out a board they sit on. The same rule guards the two other ways one
person could take the company: on an individual decision only the signer's **own**
identity entry may change — persons may be added, nobody else may be changed or
removed, so a sole director rotates their own key and never anyone else's — and the
signer's own **authorisation** rows may only shrink, so nobody may make themselves a
publisher. Irena refuses at execution and any reader re-checks it from the chain. What
it does *not* do, stated plainly: stop an individual channel from rewriting a channel
its actor is not part of, registering a new person, or authorising somebody else —
configuration hazards the notary attests to, and `irena channels` marks every
individual channel.

### Who may write to the chain

Every record reaches the ledger as a transaction signed by some key, and the company
itself says whose key that may be. The **identities** record is the one key table: a
person has a stable id — the same id they hold shares or a seat under — an optional
name, an optional opaque document number (a passport, a national id), and at most one
current key. Rotating a key is one identities amendment, and every channel that person
sits on sees the new key from that height on, while anything frozen earlier keeps the
key it froze. The **authorisation** record says which family of record each person may
sign: `company` (amendments to any part) or `governance` (meetings, votes, decisions,
resolutions, executions). Publishing refuses an unauthorised key before writing;
reconstruction stops at a company record an unauthorised key signed, exactly as it
stops at a broken amendment link; and every governance verifier reports a named
`SignerAuthorised` check.

Two consequences, stated plainly. **Bare publishing is real power**: a `company`
signer rewrites the register with no channel deciding anything. That is the notary's
route by design, and the authorisation record is exactly *who* may take it. And a
company must always keep one `company` signer holding a key — the **lockout rule** —
so no record can leave a company nobody can ever amend again.

Real, validated documents for each configuration are under
[examples/](examples/README.md); the [governance.md](governance.md) walk-through runs
all three on one chain.

| Document | Covers |
|---|---|
| [governance.md](governance.md) | **Start here.** How a company decides and how the chain proves it, in plain language, walked through a real chain: a shareholders' vote, a sole executive's decisions — one of them refused — and a board vote |
| [gui.md](gui.md) | **The interface, before it exists.** What each of the five people who use this comes to do, the six views they read, the six things they do, and how the system says no — functional, in plain language, no screens drawn |
| [gui-plan.md](gui-plan.md) | The order to build the interface in: a spiral of nine rounds, each usable on its own and each a broader case than the last, with a box to tick per step |
| [examples/](examples/README.md) | Real, schema-validated documents: a three-channel genesis, a single-member company, a weighted committee, and the two channel-set amendments the walk-through executes and refuses |
| [IRENA_V1.md](IRENA_V1.md) | The company model, decision channels, the record envelope, notarisation, amendment and reconstruction, votes, individual decisions, meetings, resolutions, and the road ahead |
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
$ irena --chain acme.chain channels                 # every channel, resolved at the head
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
$ irena --chain acme.chain resolution verify --execution 8b1c…   # decision → amendment

$ bornite evaluate --rules rules.xml --vote vote.xml     # the same rules, no ledger at all
```

| Crate | Responsibility |
|---|---|
| [`irena-core`](crates/irena-core) | The company model — a genesis that is the whole company (identity, flat share register with signing keys, the decision channel set with nested voting rules), notarisation, the record envelope — and its strict XML |
| [`irena-ledger`](crates/irena-ledger) | Prunella integration: found, amend, and **reconstruct** the company at any height from the genesis and the amendments in chain order |
| [`irena-decision`](crates/irena-decision) | A channel resolved against the company into actors, weights and keys; and the individual decision — one actor signs — recorded and verified like a vote |
| [`irena-vote`](crates/irena-vote) | The collective decision: the vote lifecycle through a channel, signed ballots, the final record and its verification from the chain alone |
| [`irena-meeting`](crates/irena-meeting) | Meetings of a collective channel: an agenda of informational and vote items, convened and finalised on the ledger, every vote frozen on its own |
| [`irena-resolution`](crates/irena-resolution) | The governance-closing loop: a channel's approval — a passed vote or a signed decision — becomes a formal resolution, and an amendment resolution authorises exactly one company amendment, bounded by the self-demotion rule |
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
| **One key table: identities own the keys** | A holder or a member is an id; their key is whatever their identity holds at the frozen height, so one amendment rotates a key everywhere at once and a person appearing in three channels has one key, not three. Cost: a second format break — `<holder key>` and `<member key>` are gone, and a genesis needs `<identities>` and `<authorisation>` (Irena's format is not frozen; only PROTOCOL_V1 and BORNITE_V1 are) | A person could carry more attested attributes without touching any channel; `document-id` is already there, stored and never interpreted |
| **Authorisation governs record transactions, by family** | Two families, `company` and `governance`; publishing refuses an unauthorised key, reconstruction breaks at one, every governance verifier names `SignerAuthorised`. Cost: a `company` signer may rewrite the register with no channel deciding — the notary's route, by design | Scoped channels (what a channel may decide) are the next step; per-kind rather than per-family signing is one wider enum |
| **Refuse at reconstruction, not just at publish** | An unauthorised company record written around Irena stops the walk at that transaction, reported and never repaired, so a rogue key cannot quietly become part of the company | — |
| **The lockout rule is mechanical** | An identities or authorisation record leaving no `company` signer with a key is refused at publish and is a break at reconstruction, so a company can never lose the ability to amend itself. The genesis signer itself is unchecked: whoever founds the chain founds the company, and the authorisation inside applies from the next record on | — |
| **Notarisation required on every record** — id, name, optional address, `at` in canonical UTC | Real-world authority enters in one place; `at` is attested metadata and never orders anything | Stage 4 can bind notary ids to keys; a notarisation could carry more attestations without changing the envelope |
| **Bornite's `<voting-rules>` nested unchanged, once per collective channel** | The same rules bytes mean the same rules in a file, a genesis or an amendment; Bornite never sees a company; a board's rules and the shareholders' sit side by side in one channel set | — |
| **Decision channels: `id + actor source + mode + scope`, no organ types** | `shareholders`, `board`, `ceo` are configurations; Irena knows `share-register`, `roster`, `individual`, `collective` and no legal system; a test runs all three through the same `execute` | More actor sources are one `match` arm each |
| **Scope is a list of parts, and silence denies** | A channel amends exactly what it lists, and a channel with no scope amends nothing at all. That is how "the register is not ours to decide" becomes mechanical, which is what a company whose register is kept by an outside authority actually needs. Cost: a third format break, and every channel document must now say what it may do | A scope could later narrow by *kind* of change rather than by part; anything finer starts becoming a permissions language, which is still refused |
| **Clean break: `<governance>` holds `<decision-channels>`, never a bare `<voting-rules>`** | No implicit `shareholders` channel hardcoded in Rust; old genesis documents stop parsing (Irena's format is not frozen; only PROTOCOL_V1 and BORNITE_V1 are) | — |
| **The channel set is a company part, replaced whole** | One provider per part, reconstruction unchanged, the register's own supersession mechanism; changing one channel rewrites the set | Per-channel supersession if whole-set rewrites prove costly |
| **Individual = the source resolves to exactly one actor** | No `actor=` selector, no notion of an office; a `ceo` is a roster of one, a single-member company its own register; rotation is a channel-set amendment. Cost: validity depends on state, so a `share-register` individual channel stops resolving the day a second holder is admitted — reported, never guessed | — |
| **Individual decisions are their own chain record** | `irena.decision.v1`, verified independently like a vote; a resolution names a channel and one transaction whichever the mode, and everything after the authority check is one code path (`ApprovalV1`) | — |
| **A meeting is a meeting of one channel** | A board meeting and a shareholders' meeting are the same code with a different id; `VotesBelong` catches a vote from another channel; per-item channels are not built | Per-item channels if a mixed meeting is ever wanted |
| **Self-demotion, not a policy engine** | Two set comparisons — no new seat, no changed seat — refused at `execute` and re-checked by `SelfDemotionHolds`; a sole director may abolish themselves, never promote themselves. It bounds the signer's own reach; the scope bounds the subject matter, and the two are checked independently | Widen either rule if a gap matters |
| **Records pinned by transaction id, never by height or time** | A vote snapshot pins the exact bytes it was decided against; amendments after the freeze cannot reach it | — |
| **Vote state as a canonical Borsh file; ballots as files** | Every lifecycle step is one command; a holder signs on their own machine | Stage 5 UI drives the same `VoteV1` in memory; a meeting holds several |
| **A meeting is a container, not a company part** | Meeting records carry no `supersedes` and reconstruction ignores their namespace, so no meeting can silently change the company | Turning a passed motion into an amendment is stage 2, and adds a record kind rather than changing this one |
| **The meeting id is its convening transaction** | Unique and unforgeable without a nonce or a registry; nothing can claim to be a meeting that was never convened | — |
| **Each vote item freezes independently at `meeting open`** | Every vote has its own snapshot, electorate and id, and verifies alone without the meeting; one command fixes them all at one height | Per-item opening, or freezing at a scheduled height, are both additions to `open` rather than changes to a vote |
| **A vote's subject is `item <n>: <title>`** | Two items with the same proposal are still two votes, and a vote traces back to its item; `VotesBelong` can detect a swapped reference | A structured item reference would replace the string without touching the vote |
| **Only convening and finalisation reach the chain** | The formal facts are on the ledger; drafting, casting and counting stay local, so a UI cannot fill the chain with noise | Intermediate attestations (a quorum roll call) would be new record kinds |
| **`VotesVerify` and `VotesBelong` are separate checks** | A real, valid vote from another meeting passes the first and fails the second — a forgery a per-vote check cannot see | — |
| **Votes never change the company; resolutions only describe authority** | Four record kinds in a row (meeting, vote, resolution, execution) and the amendment is still the one record that changes reconstructed state; reconstruction ignores all four namespaces | A future kind of authority (a board decision) plugs in at the resolution step without touching amendments |
| **The proposal digest is the digest of the amendment body** | The actors decide on the register or the channel set themselves, so what is executed is provably what was approved — no document can drift from what it authorises | A proposal envelope wrapping prose *and* body would let the voted document read better while binding the same bytes |
| **A resolution carries the body, not just its digest** | The chain is self-contained: an auditor reads what was decided without any external file | — |
| **Execution refuses a stale base** | A resolution passed against a company that has since changed cannot land on one the voters never saw; the second of two competing resolutions must be re-voted | Per-part judgement already softens it (a channel-set resolution survives a register change); a rebase-and-reconfirm step could soften it further |
| **The execution record is the only link from amendment to authority** | The company record format is untouched, so every existing amendment stays valid and readable | An optional `authority` attribute on the amendment envelope would make the link visible from the amendment's side too |
| **Identity siblings of self-demotion** | On an individual decision only the signer's own identity entry may change, and their own authorisation rows may only shrink — so one person cannot rotate another's key (voting as them) or make themselves a publisher. Two more set comparisons, no policy language | Widen either rule if the remaining gaps matter |
| **Ballots are not secret and live in the final record** | A record is verifiable from the chain alone, ten named checks | Secret ballots would need a different commitment scheme and are explicitly out of scope |
| **Final vote record as canonical Borsh, not XML** | Byte-exact re-encoding is one of the verification checks; the export shows it as base64 | An XML rendering for readers is a projection that can be added without changing what is verified |
| **Strict readers, issues collected and sorted** | Unknown elements refused; a document with three problems is fixed in one round | — |
| **Breaks reported, never repaired** | Anything written around Irena stops reconstruction at that exact transaction | — |

## Road ahead

Planned, in order — see [IRENA_V1.md §11](IRENA_V1.md):

1. ~~shareholder meetings and votes using Bornite~~ — **done**, see
   [IRENA_V1.md §9](IRENA_V1.md);
2. ~~resolutions and the company-state changes they cause~~ — **done**, see
   [IRENA_V1.md §10](IRENA_V1.md);
3. ~~board membership, meetings and decisions~~ — **done without a board type**: a
   board is a collective channel over a roster, see [IRENA_V1.md §1.3](IRENA_V1.md);
4. ~~identities and authorisation~~ — **done**: one key table and who may sign which
   family of record, see [IRENA_V1.md §1.4–§1.5](IRENA_V1.md);
5. ~~scoped channels~~ — **done**: a channel amends the parts it lists and nothing
   else, see [IRENA_V1.md §1.3](IRENA_V1.md);
6. Placidia coordination and UI — the interface is designed in [gui.md](gui.md) and
   scheduled in [gui-plan.md](gui-plan.md).

Next to the channels, recorded rather than built: per-channel supersession, more actor
sources, a wider self-demotion rule, notary ids bound to identities.

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
