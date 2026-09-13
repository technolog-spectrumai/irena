# Irena V1 — the company layer

Irena is the layer that knows what a company is. Beneath it sit two engines that do
not, and are kept that way:

| Layer | Guarantees | Knows nothing about |
|---|---|---|
| **Prunella** | A record, once appended, is immutable and ordered | What any payload means |
| **Bornite** | The arithmetic on voter ids, integer weights and choices | Shares, keys, companies |
| **Irena** | The company structure resolved at a height is what the chain says; ballots were signed by the registered holders; a result is what Bornite produces from that structure | Whether the register names the real owners |

```
                    irena-*        ← the company layer: knows what a share is
                   ╱        ╲
          prunella-*        bornite-*
      ledger, opaque      voting arithmetic
```

Irena imports both. Neither imports Irena, and neither mentions the other; a test
greps every engine source file for the words `irena`, `bornite` and `prunella` to keep
it so. There is no bridge crate: a layer that only passes things through is a layer
that should not exist.

This document covers **stage A**: the company on the chain. Stage B, connecting it to
voting, is documented in its own section once built.

## 1. What a company is, to Irena

Three records, each on its own amendment chain because they change at different rates:

| Record | Element | Namespace on the ledger | Changes |
|---|---|---|---|
| Founding | `<company-genesis>` | `irena.company.v1` | almost never |
| Share register | `<share-structure>` | `irena.shares.v1` | often |
| Voting rules | `<voting-rules>` | `irena.rules.v1` | rarely |

**The company at height `h`** is the record of each kind in force at `h`, resolved
together (`irena_ledger::company_at`). A company with any of the three missing is not
yet a company that can vote, and Irena says which is missing rather than returning a
partial answer.

### 1.1 `<company-genesis>`

```xml
<company-genesis>
  <identity name="Acme Industries Ltd" jurisdiction="gb" registered-number="01234567"/>
  <incorporation document-digest="9a3f…"/>
</company-genesis>
```

Free text throughout, except the digest. `name` is required and non-empty. Irena
stores and reproduces it and compares none of it: a company is identified on the
ledger by its `company` label (§2), and this is what the label stands for.

### 1.2 `<share-structure>` — flat shares

```xml
<share-structure>
  <holder id="alice" key="4e9c…" name="Alice Smith" shares="500"/>
  <holder id="bob"   key="7b21…"                    shares="300"/>
  <holder id="carol"                                shares="200"/>   <!-- no key -->
</share-structure>
```

**Every share is one vote.** A holder is a holding, and one element carries both.
There are no share classes in V1 (§7).

| Attribute | | |
|---|---|---|
| `id` | required | The holder's id, which is also their Bornite voter id — same grammar (1–128 bytes of `A-Z a-z 0-9 . _ : + @ -`, byte-for-byte comparison), so a holder becomes a voter with no translation |
| `key` | optional | The Ed25519 public key the holder signs ballots with, 64 lowercase hex characters |
| `name` | optional | Opaque |
| `shares` | required | A decimal integer ≥ 1 |

**The register carries the holders' signing keys.** That is what lets a vote check that
a ballot came from a registered holder without any key table of its own: keys are
company data, on the chain, amended through the same chain as everything else. A
holder without a key owns their shares and counts towards quorum, but can never cast a
valid ballot.

Refused, with every issue collected and reported together:

| | |
|---|---|
| Duplicate holder id | Two entries for one holder |
| Duplicate signing key across holders | One key voting for two holders would let one person cast two ballots Bornite cannot tell apart |
| `shares="0"` | A holder of nothing is a mistake, not a zero-weight member |
| Total shares past `u64::MAX` | Checked arithmetic, reported not wrapped |
| An `id` outside the voter-id grammar, a `key` that is not 64 hex characters, `shares` that is not a plain decimal | Value issues |
| An unknown attribute or child element | Structural: refused, never skipped |

The register is sorted by id from the moment it is built, so nothing downstream can
observe an order that depends on how the document listed the holders. An empty register
is a valid document; whether it makes sense is the ledger's business.

### 1.3 `<voting-rules>`

Bornite's element, **unchanged** — see [BORNITE_V1.md](BORNITE_V1.md). Its schema is
reused by `xs:include` and its bytes are parsed by `bornite_xml::parse_voting_rules`,
so the same bytes mean the same rules whether they sit in a standalone file, on a
company's ledger, or anywhere else. Nothing about a company appears inside it. That is
the rule that shapes the whole design: the rules must be enough to organise a vote
with no organisation behind them — a company AGM, a non-profit's membership vote, or a
fleet of drones deciding a peaceful deployment democratically.

## 2. The record envelope

Every record on the ledger is one `<irena-record>` element:

```xml
<irena-record version="1.0" kind="share-structure" company="acme" supersedes="8f3a…">
  <notarisation id="notary-07" name="Jane Roe" address="12 High Street, London"
                at="2026-03-01T09:30:00Z" statement="Filed at Companies House"
                source-digest="c41d…"/>
  <share-structure>…</share-structure>
</irena-record>
```

| Attribute | | |
|---|---|---|
| `version` | required | `1.0`. Anything else is refused |
| `kind` | required | `company-genesis`, `share-structure` or `voting-rules`; must match the element carried |
| `company` | required | An opaque label: 1–64 bytes, `a-z0-9` first, then `a-z0-9._-`. Compared for equality, never interpreted |
| `supersedes` | optional | The transaction id of the record of this kind currently in force for the company. Absent on the first (§4) |

The body element is embedded **byte for byte** (`irena_core::compose_record`): Irena
does not re-serialise a document a notary signed off on. A leading XML declaration and
surrounding whitespace on the supplied body are removed; the element itself is not
touched. The composed record begins with `<irena-record` and ends with
`</irena-record>` with nothing around it, so it is exactly one element and Prunella's
XML transport carries it nested and readable inside the block rather than as base64
(see [docs/xml-format.md](docs/xml-format.md)).

Schemas: [`schemas/irena-record-v1.xsd`](schemas/irena-record-v1.xsd),
[`schemas/irena-company-v1.xsd`](schemas/irena-company-v1.xsd). The readers enforce the
same rules in code plus the ones XSD 1.0 cannot express; a test validates composed
records with `xmllint` where it is installed.

## 3. Notarisation

Every record is put on the ledger by hand, by someone with the authority and the
documents to say the company is now like this. `<notarisation>` is required on every
record and says who that was:

| Attribute | | |
|---|---|---|
| `id` | required | The notary's stable identifier, same grammar as a voter id |
| `name` | required | The notary's name, non-empty |
| `address` | optional | Free text |
| `at` | required | When the notary says the change took effect: RFC 3339 in UTC at seconds precision, **exactly** `YYYY-MM-DDTHH:MM:SSZ`. Offsets, fractional seconds, lowercase letters, leap seconds and impossible dates are refused. One instant has one spelling, and the string's byte order is its chronological order |
| `statement` | optional | Free text |
| `source-digest` | optional | Digest of an external document the notary attests to — a certificate of incorporation, a signed register, minutes |

Irena stores and reproduces all of it and **interprets none of it**. In particular
`at` is attested metadata: it is never used to order, resolve, expire or sequence
anything. The ledger's own order is the only order (§4). It is there so a reader of the
chain can see when the notary says the change took effect, and because it is part of
the record's bytes, it is part of what the chain commits to — two notarisations that
differ only in `at` produce different transaction ids.

The notarisation is also where real-world authority enters and where Irena's
guarantees stop. Irena can show that the register in force at a height was attested by
notary `notary-07` on the date they gave, citing a document with a given digest. It
cannot show that the document says what anyone believes, or that the register reflects
who really owns the company.

## 4. Amendment and resolution

* **Each (company, kind) has its own amendment chain.** Amending the register does not
  touch the rules; the founding record can be amended (a name change) without either.
* **A record must supersede the record in force.** `supersedes` must name exactly the
  transaction id of the record of that kind currently in force for the company, or be
  absent when there is none. Anything else is a **stale amendment**, refused before the
  ledger is touched. Amending a version you have not seen is how two editors clobber
  each other.
* **Resolution walks the chain in ledger order.** `history(company, kind, at)` visits
  every block from genesis to `at`, every transaction in the kind's namespace whose
  record names the company, and checks link by link that each supersedes the one
  before. The record in force is the last one. Block order is total and transaction
  order within a block is fixed, so two instances holding the same chain resolve the
  same record at every height, and a past height resolves to what was in force then
  regardless of anything appended since.
* **A break is reported, never repaired.** A record in an Irena namespace that does not
  link, does not parse, or is of the wrong kind for its namespace can only have been
  written around Irena, through Prunella directly. It is an error naming the exact
  height and transaction (`BrokenAmendmentChain`, `UnreadableRecord`), and until it is
  resolved nothing is in force for that (company, kind). Other kinds, other companies
  and heights before the break are unaffected. `irena verify-structure` walks all
  three chains and reports.
* **Transactions in other namespaces are not Irena's business.** A Prunella chain can
  carry anything else alongside a company; Irena reads only its three namespaces.

The signer of the transaction and the notary inside the record are two different
things: the signer put the record on the chain, the notary vouched for it. V1 records
who both were and imposes no policy on either.

## 5. Public API

| Crate | |
|---|---|
| `irena-core` | `CompanyIdV1`, `CompanyGenesisV1`, `IdentityV1`, `ShareStructureV1`, `HolderV1`, `NotarisationV1`, `NotaryIdV1`, `NotaryTimeV1`, `IrenaRecordV1`, `RecordKindV1`, `RecordBodyV1`; `read_record`, `compose_record`, `read_company_genesis_document`, `read_share_structure_document`, `read_voting_rules_document`; `IrenaError`, `IssueV1` |
| `irena-ledger` | `genesis_with_company`, `publish`, `history`, `in_force`, `genesis_in_force`, `shares_in_force`, `rules_in_force`, `company_at`; `RecordRefV1`, `InForceV1<T>`, `CompanyStateV1`; `LedgerError` |
| `irena-cli` | The `irena` binary — [docs/irena-cli.md](docs/irena-cli.md) |

Every reader is strict (unknown elements and attributes refused, never skipped) and
every content problem in a document is collected and reported together, sorted, so the
report does not depend on the order problems were noticed in.

## 6. What changed beneath Irena

Two additive changes to Prunella, neither touching [PROTOCOL_V1.md](PROTOCOL_V1.md):

* **XML transport version 2** carries a payload that is exactly one well-formed XML
  element as that element, verbatim, so company records are readable inside the block.
  The importer cuts the element's exact source bytes back out rather than rebuilding
  it, so the transaction id commits to the same bytes on both sides. Everything else
  still travels as base64, and version 1 documents still read.
* **One Merkle implementation, many domains.** `prunella_core::merkle` is generic over
  a `TreeTags` triple, so an application can commit to a list of its own (stage B's
  ballots) with the same tree and the same proofs in a domain that can never collide
  with a block's. Prunella's roots are bit-identical and the frozen V1 golden vectors
  prove it.

Bornite is untouched.

## 7. Not in V1 — TODO

Recorded here so they are decisions, not omissions:

* **Share classes** with votes-per-share (ordinary, preferred, non-voting). The company
  this is built for has flat shares. When classes arrive they are a new version of the
  `<share-structure>` body — the flat body stays readable forever — and the electorate
  derivation gains one factor in exactly one place.
* **Shareholder meetings**, votes as meeting business, **board meetings** and **board
  decisions**: the next stages. Nothing here is shaped around guesses about them.
* Any policy on who may sign a record transaction, secret ballots, delegation, proxies,
  networking, consensus, a GUI.
