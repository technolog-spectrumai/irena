# Irena V1 — the company layer

Irena is the layer that knows what a company is. Beneath it sit two engines that do
not, and are kept that way:

| Layer | Guarantees | Knows nothing about |
|---|---|---|
| **Prunella** | A record, once appended, is immutable and ordered | What any payload means |
| **Bornite** | The arithmetic on voter ids, integer weights and choices | Shares, keys, companies |
| **Irena** | The company structure resolved at a height is what the chain says; a decision was taken through a channel that existed at the frozen height, by the actors it resolved to; ballots and decisions were signed by their registered keys; a result is what Bornite produces from that electorate | Whether the register names the real owners, or whether the configured authority is legally correct |

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

Sections 1–6 are the company on the chain, including the **decision channels** that say
who may decide and how (§1.3); §7 is a vote — a collective channel's decision; §8 an
individual channel's decision; §9 a meeting that groups votes; §10 the resolution that
turns either kind of decision into company change; §11 what comes next.

This document is exact and normative. For the same story told in plain language, walked
through a real chain, see [governance.md](governance.md); for the documents themselves,
[examples/](examples/README.md).

## 1. What a company is, to Irena

**One company per chain**, founded by one record and amended one part at a time:

| Part | Element | Set by | Amended by a record of kind | Namespace |
|---|---|---|---|---|
| Identity | `<identity>` | the genesis | `identity` | `irena.company.v1` |
| Share register | `<share-structure>` | the genesis | `share-structure` | `irena.shares.v1` |
| Decision channels | `<decision-channels>` (nested in `<governance>`) | the genesis | `decision-channels` | `irena.channels.v1` |
| Identities | `<identities>` | the genesis | `identities` | `irena.identities.v1` |
| Authorisation | `<authorisation>` | the genesis | `authorisation` | `irena.authorisation.v1` |

**The company at height `h`** is its genesis plus every amendment up to `h`, applied
in chain order (`irena_ledger::reconstruct`, §4). Every part is always present — the
genesis carries all five — so there is never a partial company, and each part knows
which transaction currently provides it.

### 1.1 `<company-genesis>` — the whole company

```xml
<company-genesis>
  <identity name="Acme Industries Ltd" jurisdiction="gb" registered-number="01234567"/>
  <incorporation document-digest="9a3f…"/>
  <share-structure>
    <holder id="alice" name="Alice Smith" shares="500"/>
    …
  </share-structure>
  <identities>
    <person id="alice" name="Alice Smith" document-id="GB-P-540027183" key="4e9c…"/>
    …
  </identities>
  <governance>
    <decision-channels>
      <channel id="shareholders" mode="collective">
        <actors source="share-register"/>
        <voting-rules version="1.0">…</voting-rules>
      </channel>
      …
    </decision-channels>
  </governance>
  <authorisation>
    <signer person="jane" records="company"/>
    <signer person="jane" records="governance"/>
  </authorisation>
</company-genesis>
```

One document founds a company. `<identity>` (required; `name` non-empty, the rest
free text) says who it is; `<incorporation>` (optional) names the document that
created it by digest; `<share-structure>` (required, §1.2) is the initial register;
`<identities>` (required, §1.4) is the one key table; `<governance>` (required) holds
the active governance configuration: exactly one `<decision-channels>` element, the
channel set (§1.3); `<authorisation>` (required, §1.5) says who may sign records. Irena stores and reproduces the
identity and compares none of it: on the ledger a company is its `company` label (§2),
and this is what the label stands for. An `identity` amendment carries a standalone
`<identity …/>` element with the same attributes.

### 1.2 `<share-structure>` — flat shares

```xml
<share-structure>
  <holder id="alice" name="Alice Smith" shares="500"/>
  <holder id="bob"                      shares="300"/>
  <holder id="carol"                    shares="200"/>
</share-structure>
```

**Every share is one vote.** A holder is a holding, and one element carries both.
There are no share classes in V1 (§11).

| Attribute | | |
|---|---|---|
| `id` | required | The holder's id, which is also their Bornite voter id and their id in `<identities>` — same grammar (1–128 bytes of `A-Z a-z 0-9 . _ : + @ -`, byte-for-byte comparison), so a holder becomes a voter, or a person, with no translation |
| `name` | optional | Opaque |
| `shares` | required | A decimal integer ≥ 1 |

**A holder carries no key.** Whatever a holder signs is checked against the key their
identity holds in the identities record in force at the frozen height (§1.4). A holder
whose identity holds no key — or who has no identity at all — owns their shares and
counts towards quorum, but can never cast a valid ballot.

Refused, with every issue collected and reported together:

| | |
|---|---|
| Duplicate holder id | Two entries for one holder |
| `shares="0"` | A holder of nothing is a mistake, not a zero-weight member |
| Total shares past `u64::MAX` | Checked arithmetic, reported not wrapped |
| An `id` outside the voter-id grammar, `shares` that is not a plain decimal | Value issues |
| An unknown attribute or child element | Structural: refused, never skipped. A `key` attribute is one of these: keys moved to `<identities>` and a document written for the older shape is refused outright, not read with the key dropped |

The register is sorted by id from the moment it is built, so nothing downstream can
observe an order that depends on how the document listed the holders. An empty register
is a valid document; whether it makes sense is the ledger's business.

### 1.3 `<decision-channels>` — who decides, and how

A **decision channel** is the one authority abstraction in Irena:

```text
channel      := id + actor source + mode + scope?
actor source := share-register | roster (members listed inline)
mode         := individual | collective(<voting-rules>)
scope        := the company parts this channel may amend
```

```xml
<decision-channels>
  <channel id="shareholders" mode="collective">
    <actors source="share-register"/>
    <voting-rules version="1.0">…</voting-rules>
  </channel>
  <channel id="board" mode="collective">
    <actors source="roster">
      <member id="chen"   name="M. Chen" weight="2"/>
      <member id="okafor"/>
      <member id="vance"/>
    </actors>
    <voting-rules version="1.0">…</voting-rules>
  </channel>
  <channel id="ceo" mode="individual">
    <actors source="roster">
      <member id="chen"/>
    </actors>
    <scope><amend part="decision-channels"/></scope>
  </channel>
</decision-channels>
```

`shareholders`, `board`, `ceo` and any future committee are **configurations in this
document, never types in Rust**. Irena knows `share-register`, `roster`, `individual`
and `collective`. It does not know what a board is, encodes no legal system or company
type, and does not decide whether the configured authority is legally correct:
**notarisation is the trust boundary** (§3). A channel says nothing about *what* its
actors may decide; scoping a channel to a kind of decision is deliberately not in V1
(§11).

| | | |
|---|---|---|
| `channel/@id` | required | A label: same grammar as a company id (1–64 bytes, `a-z0-9` first, then `a-z0-9._-`). Compared for equality, never interpreted |
| `channel/@mode` | required | `individual` — one actor signs; `collective` — the actors form a Bornite electorate under the nested rules |
| `actors/@source` | required | `share-register` — the holders of the register in force when a decision is frozen, weight = shares (§1.2); `roster` — the `<member>`s listed inline |
| `member/@id` | required | A Bornite voter id, exactly as a holder's (§1.2): a member becomes a voter, or a person (§1.4), with no translation |
| `member/@name` | optional | Opaque |
| `scope` | optional | The company parts this channel may amend, one `<amend part="…"/>` each. **Absent: the channel amends nothing** and may carry declarative resolutions only. An empty `<scope/>` is refused; omit the element, which says the same thing on purpose |
| `amend/@part` | required | `identity`, `share-structure`, `decision-channels`, `identities` or `authorisation`. Not `company-genesis`: a company is founded once, and a channel amends a founded one |
| `member/@weight` | optional | A decimal integer ≥ 1; default `1` |
| `voting-rules` | exactly when `collective` | Bornite's element, **unchanged** — see [BORNITE_V1.md](BORNITE_V1.md). Its schema is reused by `xs:include` and its bytes parsed by `bornite_xml::parse_voting_rules`, so the same bytes mean the same rules in a file, a genesis or an amendment. Nothing about a company appears inside it |

Refused, every issue collected: no channel at all; a duplicate channel id; a `roster`
that is empty, lists a member twice, or gives a member zero weight; a total weight past
`u64::MAX`; `<member>`s under `share-register`; `<voting-rules>` on an `individual`
channel or missing from a `collective` one; an empty scope, a part listed twice, or a
part that is not an amendment; an unknown source, mode, attribute or element.

**Silence denies.** A channel set written before scopes existed, or by someone who did
not think about them, grants no power over the company at all. That is the safe
direction to be wrong in, and it is what a company whose share register is kept by an
outside authority needs: with no channel scoped to `share-structure`, the register on
the chain is a mirror that only an authorised signer may bring up to date, and no
decision of any body can move it. The scope that applies to a decision is the one in
the channel set it was **frozen against**, so narrowing a channel does not invalidate
what it decided before.

**The channel set is one part of the company, replaced whole** — like the register,
and through the same `supersedes` mechanism (§4). The set at any height is the one
record then in force. Changing one channel rewrites the set; per-channel supersession
is a recorded future upgrade (§11).

**Resolution** (`irena_decision::resolve_channel`) is where a channel meets the company:
given the company reconstructed at a height, it finds the channel in the set in force,
resolves its actors from its source *as the company then stands*, and hands back ids,
integer weights, each actor's key **as the identities then held it** (§1.4) and — for a
collective channel — the rules. For an `individual`
channel it additionally requires that **exactly one actor resolved**. That check can
only be made at resolution: a `share-register` source in individual mode is one actor
in a single-member company and two the day a second holder is admitted, and the
document alone cannot say which. `irena channels --at h` shows every channel resolved
at a height and marks the individual ones.

### 1.4 `<identities>` — the one key table

```xml
<identities>
  <person id="alice" name="Alice Smith" document-id="GB-P-540027183" key="8a88…"/>
  <person id="carol" name="Carol White"/>                    <!-- registered, cannot sign -->
  <person id="jane"  name="Jane Roe" key="fd17…"/>
</identities>
```

A **person** has a stable id, an optional name, an optional external document number
and at most one current key. The id is the actor-id grammar (§1.2), so a person *is*
the holder, the roster member and the signer of that id — no translation, no mapping
table.

| Attribute | | |
|---|---|---|
| `id` | required | Also their id as a holder, a member or a signer |
| `name` | optional | Opaque |
| `document-id` | optional | An external identity document number — a national id, a passport — in whatever form the notary uses. **Opaque**: stored, reproduced, never interpreted, never compared |
| `key` | optional | The Ed25519 public key this person currently signs with, 64 lowercase hex characters. Absent: registered, and unable to sign anything |

Refused, every issue collected: two persons with one id; **one key on two persons** (a
signature must name exactly one person); more persons than the reader accepts; a bad
id or key. An empty `<identities/>` is a valid document.

**Why one table.** A person's key is their voice in every channel they sit on. With
keys inline on holders and members, one person in three channels had three copies and
no way to rotate without amending every place at once. Here, rotation is one
identities amendment: every channel sees the new key from that height on, and anything
frozen earlier keeps the key it froze — a vote frozen before the rotation still
verifies with the old key, because its snapshot pins the identities record it was
frozen against.

### 1.5 `<authorisation>` — who may sign records

```xml
<authorisation>
  <signer person="jane" records="company"/>
  <signer person="jane" records="governance"/>
</authorisation>
```

Every record reaches the ledger as a Prunella transaction signed by some key. This
record says whose key that may be, per **family**:

| `records` | Covers |
|---|---|
| `company` | Amendments to any company part: identity, register, channel set, identities, authorisation |
| `governance` | Meetings, votes, decisions, resolutions and executions |

A row names a person (by identity id) and a family. Refused: a duplicate row; **no
`company` row at all** — the document half of the lockout rule (§4).

**Bare publishing is real power.** A `company` signer may rewrite the register with no
channel deciding anything. That is the notary's route by design — a registrar filing a
transfer, a secretary correcting a name — and this record is exactly *who* may take
it. What a channel decides and what a signer may write are two separate questions in
V1; scoping a channel to a kind of decision is still not in it (§11).

## 2. The record envelope

Every record on the ledger is one `<irena-record>` element:

```xml
<irena-record version="1.0" kind="share-structure" company="acme" supersedes="5d02…">
  <notarisation id="notary-07" name="Jane Roe" address="12 High Street, London"
                at="2026-03-01T09:30:00Z" statement="Filed at Companies House"
                source-digest="c41d…"/>
  <share-structure>…</share-structure>
</irena-record>
```

| Attribute | | |
|---|---|---|
| `version` | required | `1.0`. Anything else is refused |
| `kind` | required | `company-genesis`, `identity`, `share-structure`, `decision-channels`, `identities` or `authorisation`; must match the element carried |
| `company` | required | An opaque label: 1–64 bytes, `a-z0-9` first, then `a-z0-9._-`. Compared for equality, never interpreted |
| `supersedes` | optional | The transaction id of the record currently providing the part this one amends: the genesis, or the last amendment of that part. Absent on the genesis (§4) |

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

## 4. Amendment and reconstruction

Nothing is ever edited. The company is **reconstructed** from the chain:

* **The first Irena record founds the company.** `reconstruct(store, at)` walks every
  block from genesis to `at`. The first transaction in an Irena namespace must be a
  `company-genesis` (normally in block 0, where `irena init` puts it; a company may
  also be founded later on an existing chain). It sets all three parts.
* **Every later record amends one part.** An `identity`, `share-structure`,
  `decision-channels`, `identities` or `authorisation` record replaces that part in
  full and must name in `supersedes` exactly the transaction currently providing it —
  the genesis, or the last amendment of the same part. Amending the register never
  touches the channel set.
* **A company record must be signed by a `company` signer.** The transaction signer is
  looked up in the identities in force and their row in the authorisation in force,
  both taken from the company **as it was before the record**: a record cannot
  authorise its own signer. `publish` refuses an unauthorised key before writing
  (`UnauthorisedSigner`); reconstruction stops at one written around Irena
  (`UnauthorisedRecord`), reported and never repaired, exactly like a broken link.
  The **genesis signer is unchecked**: whoever founds the chain founds the company,
  and the authorisation inside the genesis applies from the next record on.
* **The lockout rule.** No record may leave the company without at least one `company`
  signer holding a key. An identities or authorisation record that would is refused at
  publish (`LockedOut`) and is a break at reconstruction (`Lockout`); a genesis born
  that way is refused outright. A company can never lose the ability to amend itself.
* **A stale amendment is refused before the ledger is touched.** `publish` reconstructs
  the company at the head and checks `supersedes` against it; naming a version that no
  longer provides the part is how two editors clobber each other.
* **Determinism.** Block order is total and transaction order within a block is fixed,
  so two instances holding the same chain reconstruct the same company at every height,
  and a past height never changes because of anything appended since.
* **One company per chain.** A second `company-genesis`, or a record naming another
  company, is an error (`SecondGenesis`, `ForeignCompany`), as is an amendment before
  any genesis (`NoGenesisFirst`).
* **A break is reported, never repaired.** A record in an Irena namespace that does not
  link (`BrokenAmendmentChain`), does not parse or is of the wrong kind for its
  namespace (`UnreadableRecord`) can only have been written around Irena, through
  Prunella directly. Reconstruction stops there with the exact height and transaction;
  heights before the break still reconstruct; nothing can be published on a broken
  chain. `irena verify-structure` reconstructs and reports.
* **Transactions in other namespaces are not Irena's business.** A Prunella chain can
  carry anything else alongside a company; Irena reads only its five namespaces.

The signer of the transaction and the notary inside the record are two different
things: the signer put the record on the chain, the notary vouched for it. V1 checks
the signer against the company's own authorisation (§1.5) and imposes no policy on the
notary beyond recording who they said they were.

## 5. Public API

| Crate | |
|---|---|
| `irena-core` | `CompanyIdV1`, `CompanyGenesisV1`, `IdentityV1`, `ShareStructureV1`, `HolderV1`; `DecisionChannelsV1`, `DecisionChannelV1`, `ChannelIdV1`, `ActorSourceV1`, `RosterV1`, `MemberV1`, `ChannelModeV1`; `IdentitiesV1`, `PersonV1`, `AuthorisationV1`, `SignerV1`, `RecordFamilyV1`; `NotarisationV1`, `NotaryIdV1`, `NotaryTimeV1`, `IrenaRecordV1`, `RecordKindV1`, `RecordBodyV1`; `read_record`, `compose_record`, `read_company_genesis_document`, `read_identity_document`, `read_share_structure_document`, `read_decision_channels_document`, `read_identities_document`, `read_authorisation_document`; `IrenaError`, `IssueV1` |
| `irena-ledger` | `genesis_with_company`, `publish`, `reconstruct`, `company_now`, `history`; `authorised_signer`, `signer_of`, `lockout_after`; `CompanyStateV1` (`provider_of`, `history_of`), `InForceV1<T>`, `RecordRefV1`; `LedgerError` |
| `irena-decision` | `resolve_channel`, `ResolvedChannelV1`, `ActorSetV1`, `ActorV1`, `actors_of`, `actors_of_register`, `actors_of_roster`; `DecisionV1` (`draft`, `freeze`, `sign`, `finalize`, `final_record`), `DecisionStatusV1`, `DecisionSnapshotV1`, `DecisionIdV1`; `FinalDecisionRecordV1`; `verify_decision`, `signer_authorised_at`, `check_signer_authorised`, `DecisionVerificationV1`, `DecisionCheckV1`, `DecisionCheckNameV1`; `DecisionError` |
| `irena-vote` | `VoteV1` (`draft`, `freeze`, `open`, `cast`, `close`, `evaluate`, `finalize`, `final_record`), `VoteStatusV1`, `VoteSnapshotV1`, `VoteIdV1`; `SignedBallotV1`, `BallotBodyV1`, `BallotChoiceV1`, `ballot_commitment`, `COMMITMENT_TAGS`; `FinalVoteRecordV1`, `EvaluationSummaryV1`; `verify`, `VerificationV1`, `CheckV1`, `CheckNameV1`; `VoteError`, `BallotRejectionV1`; re-exports `irena-decision`'s resolution |
| `irena-meeting` | `MeetingIdV1`, `MeetingStatusV1`, `MeetingMetadataV1`, `AgendaV1`, `AgendaItemV1`, `AgendaBodyV1`; `MeetingV1` (`draft`, `add_item`, `convene`, `open`, `cast`, `close`, `finalize`, `final_record`); `MeetingFinalRecordV1`, `FinalItemV1`, `MeetingRecordV1`, `compose_convened`, `compose_final`, `read_meeting_record`; `verify_meeting`, `MeetingVerificationV1`, `MeetingCheckV1`, `MeetingCheckNameV1`; `MeetingError` |
| `irena-resolution` | `ResolutionIdV1`, `ResolutionKindV1`, `AmendmentTargetV1`, `ResolutionStatusV1`, `AuthorityV1` (`Collective`, `Individual`), `ApprovalV1`, `proposal_digest`; `ResolutionV1` (`draft`, `finalize`, `execute`), `ExecutedV1`; `self_demotion`, `identity_demotion`, `authorisation_demotion`, `SelfDemotionV1`; `ResolutionRecordV1`, `ResolutionExecutionV1`, `compose_resolution`, `compose_execution`, `read_resolution_record`, `read_execution_record`; `verify_resolution`, `verify_execution`, `ResolutionVerificationV1`, `ExecutionVerificationV1`, `ResolutionCheckV1`; `ResolutionError` |
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

## 7. A vote — a collective channel's decision

`irena-vote` owns the idea of a vote as a **process**. Bornite still only counts,
Prunella still only stores, and neither learns anything from it. Nothing in the crate
knows what a share is: a vote asks a channel (§1.3) for its electorate, and the same
code runs a shareholders' vote, a board vote and any other collective channel a company
configures.

```
Draft ──freeze──▶ Frozen ──open──▶ Open ──close──▶ Closed ──evaluate──▶ Evaluated ──finalize──▶ Finalized
```

It is a runtime state machine: every operation checks the status first and refuses
with `InvalidTransition { from, to }` naming both, so an invalid transition is
something a caller reports, not something the type system hides. The whole state is
canonical Borsh, so a vote can be written to a file between steps (`irena vote
--state`) and read back on another day or another machine to exactly the same vote.

### 7.1 Resolving the channel — where Irena earns its place

```
weight(actor) = shares(holder)          share-register: one share, one vote
weight(actor) = member/@weight          roster: the declared weight, default 1
```

`irena_decision::resolve_channel` (§1.3) turns a channel into a Bornite `ElectorateV1`:
actor id becomes voter id with no translation, the source's weight becomes weight,
nobody is excluded (neither source has a notion of exclusion). Every holder has ≥ 1
share and every member ≥ 1 weight by validation, so every actor is a voter; an actor
**without a signing key** is still a voter — they count towards quorum — but can never
cast a valid ballot, and the resolution reports who can. Bornite receives ids and
integers and never learns whether a weight came from a shareholding or a seat. One
`match` on the source is the whole company/mathematics boundary; when share classes
arrive (§11) the first line changes and nothing else.

### 7.2 Freeze

`freeze(store, at, channel)` reconstructs the company at exactly height `at` (§4),
resolves `channel` there, requires it to be **collective** (`NotCollective` otherwise),
and records everything in an immutable `VoteSnapshotV1`. The vote learns which company
it is about from the chain — one company per chain — so a draft names only a subject
and a proposal:

| Field | |
|---|---|
| `company`, `subject`, `proposal_digest` | What is being voted on. The proposal is identified by its digest and never interpreted |
| `height` | Where the company was resolved |
| `genesis_tx_id`, `shares_tx_id`, `channels_tx_id`, `identities_tx_id` | The founding transaction and the transactions providing the register, the channel set and the identities there, **pinned by transaction id**. A Prunella transaction id commits to the payload bytes, so pinning the id pins the exact register, channel set and key table — a key rotated after the freeze reaches no ballot of this vote |
| `channel` | The channel voted through; its rules, in that pinned channel set, decide |
| `electorate` | Every actor in id order: id, weight, excluded (always false), registered key |

**The vote id is the digest of the snapshot** (`hash(IRENA/vote/v1/id,
canonical(snapshot))`). Two votes frozen from identical inputs are the same vote; a
different proposal, height, channel or register is a different one. Later amendments to
the register or the channel set are irrelevant to this vote for ever after: evaluation
and verification both resolve at the snapshot height, never at the head.

### 7.3 Ballots

A ballot body is `{ vote_id, voter, choice ∈ {yes, no, abstain} }`, canonical Borsh. A
holder signs `hash(IRENA/vote/v1/ballot-sign, canonical(body))` with the key in the
frozen register. `cast` accepts a ballot only while **Open**, and checks in this order,
reporting the first failure as a typed `BallotRejectionV1`:

1. right vote (`vote_id` matches);
2. well-formed voter id;
3. voter is in the frozen electorate;
4. voter is not excluded;
5. the frozen electorate entry carries a key — the one the identities held at the freeze;
6. the signature verifies against that key — Prunella's Ed25519 verifier, **never
   Bornite**, which never sees a signature;
7. the voter has not already cast a ballot. The first ballot stands.

Ballots live in a map keyed by voter, so duplicates are impossible and no order ever
depends on arrival. A ballot for one vote cannot be replayed in another: the vote id
is inside what was signed.

### 7.4 Evaluate

`evaluate(store)` re-reads the channel set the snapshot pinned — at the snapshot
height, checks it is that exact transaction, and takes the frozen channel's rules from
it — rebuilds the Bornite electorate from the frozen entries, converts the accepted
ballots, and calls `bornite_eval::evaluate`. Bornite's full `VoteEvaluationV1` is returned for reporting;
what the vote keeps, and what the final record carries, is `EvaluationSummaryV1`:
every number in the result (outcome, reason code, electorate before and after
exclusions, participation, the three-way tally, the quorum as applied, the threshold
as applied) in a canonical form. The rules are not echoed because the snapshot pins
them on the chain.

### 7.5 The final record

`finalize(store, key, ts)` reruns the evaluation, requires it to match what was stored,
and appends `FinalVoteRecordV1` — canonical Borsh under namespace `irena.vote.v1` — in
its own block:

| Field | |
|---|---|
| `version` | 1 |
| `vote_id` | Stored as well as derivable, so tampering with either it or the snapshot is caught as a disagreement between the two |
| `snapshot` | §7.2 |
| `ballot_commitment` | The Merkle root over the accepted ballots' digests, in voter order, built with `prunella_core::merkle::root` under Irena's own `TreeTags` (`IRENA/vote/v1/{leaf,node,empty}`) — one implementation, a separate domain, and inclusion proofs for free, so a holder can prove their ballot was counted |
| `ballots` | Every accepted signed ballot, in strict voter order |
| `evaluation` | §7.4 |

The ballots are *in* the record. That is what makes it verifiable from the chain
alone, and it is acceptable only because ballots here are not secret.

### 7.6 Independent verification

`verify(store, tx_id)` re-establishes a record from nothing but the chain. Every
check is named, every finding reported; a check that could not run because an earlier
one failed is absent, not counted as passed:

| Check | Holds when |
|---|---|
| `Decodes` | The transaction is in `irena.vote.v1` and its payload decodes as a V1 record that re-encodes to exactly the stored bytes |
| `VoteIdDerives` | The stored vote id is the digest of the stored snapshot |
| `SnapshotPrecedesRecord` | The snapshot height is below the record's own height |
| `RecordsResolve` | The genesis, register, channel set and identities in force at the snapshot height are exactly the pinned transactions |
| `SignerAuthorised` | The transaction signer is a `governance` signer's current key under the company at the record's own height |
| `ChannelIsCollective` | The pinned channel exists in that channel set and is collective |
| `ElectorateDerives` | The electorate re-resolved from the pinned channel equals the frozen one |
| `BallotsVerify` | Every ballot is for this vote, from a frozen voter with a key, with a signature that verifies against it |
| `BallotsOrdered` | Strict voter order, no duplicates |
| `CommitmentDerives` | The commitment is the root over the ballots as stored |
| `ResultReproduces` | Bornite, rerun on the pinned channel's rules, the frozen electorate and the stored ballots, gives the stored summary |

A record claiming a register that was never in force at its own declared height fails
`RecordsResolve` however well-formed it is. A shareholders' record relabelled as the
board's fails `ElectorateDerives` — the board resolves to different people — and one
relabelled as an individual channel's fails `ChannelIsCollective`, with no rules to
rerun. A corrupted ballot signature fails
`BallotsVerify` *and* `CommitmentDerives`, because the ballot's digest moved with it;
both are reported.

### 7.7 What a verified record does and does not say

It says: at height `h` the chain named this channel, its actors with these weights
and these keys, and these rules; these ballots were signed with those keys; counted under those rules
by Bornite, this is the result; and none of that has changed since. It does not say
that the register named the real owners, that the proposal document says what anyone
believes, or that a key was used by the person it was registered to. Those are the
notary's (§3) and the company's business, and the boundary is drawn on purpose.

## 8. An individual decision

`irena-decision` owns the individual counterpart of a vote: one actor of an
**individual** channel (§1.3) signs. Both freeze the company at a height, pin the
records they were decided against by transaction id, and end as a canonical record on
the chain that a verifier re-establishes from the chain alone. A decision has no
ballots and no count: the frozen actor's one signature over the frozen snapshot is the
whole decision.

```text
Draft ──freeze──▶ Frozen ──sign──▶ Signed ──finalize──▶ Finalized
```

A runtime state machine like a vote, canonical Borsh between steps (`irena decision
--state`), so the actor signs on their own machine.

* **freeze(store, at, channel)** reconstructs the company at `at`, resolves `channel`
  (§1.3), requires it to be individual (`NotIndividual`) and to have resolved to an
  actor with a registered key (`NoKey`), and records a `DecisionSnapshotV1`: `company`,
  `subject`, `proposal_digest`, `height`, `genesis_tx_id`, `shares_tx_id`,
  `channels_tx_id`, `identities_tx_id`, `channel`, `actor`, `key`. **The decision id is the digest of the
  snapshot** (`hash(IRENA/decision/v1/id, canonical(snapshot))`).
* **sign(key)** signs `hash(IRENA/decision/v1/statement, canonical(snapshot))` with
  Prunella's Ed25519 and refuses any key but the frozen actor's (`WrongKey`).
* **finalize(store, key, ts)** verifies the signature again, checks the pinned channel
  set still resolves at the snapshot height, and appends `FinalDecisionRecordV1`
  `{ version, decision_id, snapshot, signature }` — canonical Borsh under namespace
  `irena.decision.v1` — in its own block.

`verify_decision(store, tx_id)` re-establishes it, every check named:

| Check | Holds when |
|---|---|
| `Decodes` | The transaction is in `irena.decision.v1` and its payload decodes as a V1 record that re-encodes to exactly the stored bytes |
| `DecisionIdDerives` | The stored id is the digest of the stored snapshot |
| `SnapshotPrecedesRecord` | The snapshot height is below the record's own height |
| `RecordsResolve` | The genesis, register, channel set and identities in force at the snapshot height are exactly the pinned transactions |
| `SignerAuthorised` | The transaction signer is a `governance` signer's current key under the company at the record's own height |
| `ChannelIsIndividual` | The pinned channel exists in that channel set and is individual |
| `ActorResolves` | The channel resolves to exactly the frozen actor, with the frozen key |
| `SignatureVerifies` | The signature verifies against that key over the frozen snapshot |

A decision through a collective channel fails `ChannelIsIndividual`; one claiming
another actor or another key fails `ActorResolves`; a signature by anyone else fails
`SignatureVerifies`. Amendments after the freeze reach it no more than they reach a
vote: a decision taken under a channel that has since been abolished still verifies,
because it pinned the channel set it was taken under.

Decisions are events, not company parts: §4's reconstruction ignores the namespace, and
a decision changes the company only through a resolution (§10), exactly as a vote does.

## 9. A meeting of a channel

`irena-meeting` is a company-level container for agenda items and the votes among
them, held by **one collective channel** (§1.3): the shareholders, the board, a
committee — whichever the company configured. It adds no arithmetic and no new kind of
company truth: Bornite still counts, Prunella still stores, §7 still runs every vote
through the channel, §4 still says what the company is. What a meeting adds is
**grouping and formality** — which items were put before the channel's actors, when,
by whom, and which final vote records answered them. A board meeting and a
shareholders' meeting are the same code with a different channel id; a meeting of an
individual channel cannot open, because one person holds no vote (§8 is where they
decide).

```text
Draft ──convene──▶ Convened ──open──▶ Open ──close──▶ Closed ──finalize──▶ Finalized
```

A runtime state machine, like a vote: `InvalidTransition { from, to }` names both ends.
The whole state, including each item's `VoteV1`, is canonical Borsh, so a meeting lives
in a file between steps.

### 9.1 The agenda

An item is a number (from 1, in order), a title, and one of two bodies:

| Kind | Carries | Means |
|---|---|---|
| `informational` | `document-digest` | Something shareholders are shown |
| `vote` | `proposal-digest` | Something shareholders decide |

The agenda is assembled while the meeting is a draft and **fixed by convening**: what
was put before the shareholders is what they were called to decide. An agenda needs at
least one item; titles are non-empty; both digests are opaque and never interpreted.

### 9.2 Two records, and only two

Only formal facts reach the chain, both as notarised XML under namespace
`irena.meeting.v1`, both readable inside their block:

```xml
<irena-meeting version="1.0" kind="final" company="acme" meeting="0b87…">
  <notarisation id="notary-07" name="Jane Roe" at="2026-06-01T12:00:00Z"/>
  <meeting channel="shareholders" title="Annual General Meeting 2026"
           scheduled-at="2026-06-01T10:00:00Z" notice-digest="a0a0…" opened-at-height="1">
    <agenda>
      <item number="1" kind="informational" title="Report of the directors" document-digest="1111…"/>
      <item number="2" kind="vote" title="Approve the 2026 accounts" proposal-digest="2222…"
            vote-tx="eb39…" outcome="accepted"/>
    </agenda>
  </meeting>
</irena-meeting>
```

* **convened** (`kind="convened"`) — the channel, the metadata and the full agenda. **Its transaction id
  is the [`MeetingIdV1`]**: a convening is unique and immutable once on the chain, so
  nothing else is needed to name a meeting, and nothing can claim to be a meeting that
  was never convened. It carries no `vote-tx`, no `outcome`, no `opened-at-height`.
* **final** (`kind="final"`) — the meeting id, the metadata and agenda repeated so the
  record is self-contained (verification checks they agree), the height the votes were
  frozen at, and for each vote item the transaction carrying its final vote record and
  the outcome that record states.

Everything between the two — adding an item, opening, casting, closing — is local. A
meeting's UI state is not the company's business.

Meetings are **events, not company parts**: a meeting record carries no `supersedes`,
and §4's reconstruction ignores this namespace entirely, so no meeting can change what
the company is. (Turning a passed motion *into* an amendment is §10.)

### 9.3 Opening: every vote freezes on its own

`open` creates one §7 vote per vote item **through the meeting's channel**, each
freezing the company at the current head **independently**: its own snapshot, its own
electorate, its own id. The vote's subject
is `item <n>: <title>`, so two items with the same proposal are still two different
votes and a vote can be traced back to its item.

Amendments to the register or the channel set after that height reach **none** of them: a
holder removed mid-meeting still votes, one added still cannot, and the quorum is
measured against the electorate as it was. A meeting whose items are all informational
opens too; there is simply nothing to decide.

`cast(item, ballot)` routes a ballot to that item's vote, which applies §7.3's checks
unchanged. A ballot for item 2 is not a ballot for item 3: the subjects differ, so the
vote ids differ, and the wrong vote refuses it.

### 9.4 Finalising

`finalize` writes **every vote to the chain first**, each in its own transaction as §7.5
describes, then the final meeting record. A vote already finalised is left alone, so an
interrupted finalisation is resumed rather than repeated. Because each vote is its own
record, a shareholder can verify one vote without the meeting, and the meeting record
is only the index over them.

### 9.5 Verification

`verify_meeting(store, tx_id)` re-establishes a meeting from the chain alone. Every
check is named; a check that could not run because an earlier one failed is absent, not
counted as passed. The whole thing is valid only when every check *and* every
referenced vote's own ten checks (§7.6) pass:

| Check | Holds when |
|---|---|
| `Decodes` | The transaction is in `irena.meeting.v1` and holds a **final** meeting record |
| `ConveningExists` | The meeting id names a transaction that is a **convening** record for the same company |
| `HeightsOrdered` | convened ≤ opened < finalised |
| `AgendaMatches` | The agenda in the final record is exactly the agenda convened |
| `MetadataMatches` | Channel, title, schedule and notice are exactly those convened |
| `CompanyReconstructs` | The company reconstructs at the convening height and is the record's company |
| `SignerAuthorised` | **Both** records — the convening and the final — were signed by a `governance` signer's current key, each judged against the company at its own height |
| `VotesVerify` | Every vote item names a transaction `irena_vote::verify` accepts |
| `VotesBelong` | Each vote is the one *this* item called for: right company, **through the meeting's channel**, right subject, right proposal digest, frozen at the declared opening height, finalised before the meeting, and the outcome the record claims |
| `ItemsConsistent` | Informational items carry no vote; vote items carry one |

`VotesVerify` and `VotesBelong` are separate on purpose. A real, valid vote from
another item, another meeting or another channel passes the first and fails the second
— the forgery that a per-vote check cannot see.

## 10. A resolution, and the loop it closes

```text
company state → channel ─┬─ meeting → vote → passed result ─┬─ resolution → amendment → new company state
                         └─ signed decision ────────────────┘
```

Every arrow is a separate record, and the separation is the whole point:

* A **vote** (§7) or a **decision** (§8) never changes the company. A vote counts
  ballots against a frozen electorate and says what a collective channel decided; a
  decision records what an individual channel's sole actor signed.
* A **resolution** never changes it either. It is the formal record that a decision
  *was* taken, pinned to the exact channel and the exact finalised record through which
  it decided — for a collective channel the meeting's final record, the agenda item and
  the vote; for an individual channel the decision record. It describes **authority**,
  nothing more, and both kinds of authority end at the same fact: a proposal digest one
  channel approved (`ApprovalV1`). Everything after the authority check is one code
  path.
* An **amendment** (§4) is what actually changes reconstructed state, and it is the
  same ordinary record it has always been. An **execution** record links the two, so
  the chain says which resolution authorised which amendment.

Reconstruction (§4) ignores the resolution and execution namespaces entirely. A
resolution cannot change the company even in principle; only the amendment it
authorises can, and that amendment is published through the same `publish` path, with
the same `supersedes` rule, as any other.

### 10.1 Two kinds

| Kind | Carries | Changes reconstructed state |
|---|---|---|
| `declarative` | `document-digest` — the decision document the shareholders voted on | no |
| `amendment` | `target` (`share-structure` or `decision-channels`) and the **amendment body, verbatim** | yes, through one ordinary amendment |

### 10.2 What the channel approved

The agenda item's `proposal-digest` (§9.1) is where the loop closes. For an amendment
resolution it is

```text
proposal_digest(body) = hash(IRENA/resolution/v1/proposal, normalise_body(body))
```

— the digest of **exactly the bytes that will be published**, normalised the way
`compose_record` normalises a body before embedding it, so a declaration or
surrounding whitespace is not a difference but a single changed share count is. The
actors decide on the register or the channel set themselves; an execution can only
publish a body that digests to what they approved. The body is embedded and recovered
as bytes, comments and all, never rebuilt from parse events. `irena resolution digest --file`
prints the value to put on the agenda.

For a declarative resolution the digest is the external decision document's, and must
equally be what was voted on or signed: the resolution points at the same document the
actors saw. Irena never reads it.

### 10.3 Recording a resolution

`finalize` checks the authority **against the chain** before anything is written. Not
one of these is taken on trust from the draft. For a **collective** authority:

1. the meeting's final record verifies, every check, including its votes (§9.5);
2. the named agenda item exists on it and is a vote item;
3. that item was answered by exactly the vote the resolution names;
4. that vote verifies, every check (§7.6) — which includes that its channel was
   collective at the frozen height;
5. **Bornite accepted it** — a rejected motion authorises nothing;
6. the vote was through the channel the resolution names (`WrongChannel` otherwise);
7. what the resolution carries is what was approved (§10.2).

For an **individual** authority: the decision record verifies, every check (§8) —
which includes that its channel was individual at the frozen height and resolved to
its signer — then 6 and 7 as above. A collective authority naming an individual
channel, or the reverse, fails at the vote's or decision's own verification.

Only then is the `<irena-resolution>` record published, under namespace
`irena.resolution.v1`. Its transaction id is the [`ResolutionIdV1`].

### 10.4 Executing an amendment resolution

`execute` publishes two transactions, in this order:

1. the **amendment** — `irena_ledger::publish` with the target's record kind, the
   resolution's body, and `supersedes` = the record the actors saw;
2. the **execution record** (`irena.execution.v1`), pinning the resolution, the
   amendment, the target, the replaced record and the approved digest.

The amendment first, so the execution record can pin it by transaction id. Because the
amendment is an ordinary company record, a reader who knows nothing about resolutions
still reconstructs the right company.

Three things are refused:

* **A stale base.** The actors approved replacing one exact record. If that record no
  longer provides its part — someone amended it in between — executing would replace
  something they never saw, so it is refused (`StaleBase`) and the resolution must be
  decided again. This is what "pin everything by transaction id, never by mutable
  current state" means in practice. A resolution on the *channel set* still executes if
  only the register moved: each part is judged on its own. Two resolutions on one
  decision meet the same rule: the second finalises (the decision is genuine) and
  cannot execute.
* **A second execution.** The chain is scanned for an existing execution of the same
  resolution (`AlreadyExecuted`); and even without that check `publish` would refuse
  the second amendment as a stale amendment, because the first moved the provider.
* **Out of scope.** The channel must be scoped to the part being amended, in the
  channel set it decided under (§1.3). A channel with no scope amends nothing, so an
  out-of-scope resolution is refused when it is recorded, before any execution
  (`OutOfScope`). This is about the subject matter; the next rule is about the signer.
* **Self-promotion.** An amendment resting on an **individual** decision must satisfy
  the **self-demotion rule for its target**. With `signer` the channel's sole actor,
  `old` the part superseded and `new` the body — for a `decision-channels` amendment,
  and *seats(set, id)* the ids of the channels whose resolved actors include `id`:
  * **R1** — *seats(new, signer)* ⊆ *seats(old, signer)*: the signer gains no seat;
  * **R2** — every channel in *seats(new, signer)* is identical in `old` and `new`:
    same mode, same actors, same rules.

  So a sole director may abolish their own channel, hand the company to a collective
  they are not sole in, or leave themselves alone — and may not add themselves
  anywhere, widen their own channel, thin out a collective they sit on, or change its
  rules. Two set comparisons, no scoring, no ordering, no expressions
  (`SelfPromotion`, and `irena_resolution::self_demotion` to ask beforehand). Sources
  resolve against the register and identities in force where the amendment lands.

  Two parts beside the channel set can hand one person the same power by another
  route, so each has its own rule, applied the same way and reported the same way:
  * **identities** (`identity_demotion`) — every person other than the signer is
    identical in `old` and `new`; persons may be added, none removed or changed. *Only
    your own key rotates on your own signature*: rewriting somebody else's key is
    voting as them, in every channel they sit on.
  * **authorisation** (`authorisation_demotion`) — the signer's own rows in `new` are a
    subset of their rows in `old`. *You may drop your own publishing right, never grant
    yourself one.*

  **Stated plainly**: the rules bound the signer's *own* reach. They do not stop an
  individual channel from rewriting a channel its actor is not part of, registering a
  new person, or authorising somebody else, and an amendment carried by a meeting is
  unrestricted. All are configuration hazards the notarisation attests to; `irena
  channels` marks every individual channel and `irena identities` shows who may sign
  what, so a reader knows to look.

A declarative resolution has nothing to execute and says so (`NothingToExecute`).

### 10.5 Verification

`verify_resolution` and `verify_execution` re-establish everything from the chain. The
execution verifier runs the resolution's checks first and reports them alongside its
own eleven. Which resolution checks run depends on the authority; the last five are
common:

| Resolution check | Holds when |
|---|---|
| `Decodes` | The transaction is in `irena.resolution.v1` and holds a V1 resolution |
| `MeetingVerifies` | *collective* — The meeting it names verifies, every check |
| `ItemIsAVote` | *collective* — The agenda item exists on that meeting and is a vote item |
| `VoteAnsweredTheItem` | *collective* — The vote it names is the one that answered that item |
| `VoteVerifies` | *collective* — That vote verifies, every check, including `ChannelIsCollective` |
| `VotePassed` | *collective* — Bornite accepted the motion |
| `DecisionVerifies` | *individual* — The decision it names verifies, every check (§8), including `ChannelIsIndividual` and `ActorResolves` |
| `ChannelMatches` | The vote or decision was through the channel the resolution names |
| `WithinChannelScope` | That channel was scoped to the part this resolution amends, in the channel set it decided under. A declarative resolution passes and says so |
| `ProposalMatches` | What the resolution carries digests to what was approved |
| `CompanyMatches` | Resolution, approval and chain are the same company |
| `HeightsOrdered` | The meeting or decision was recorded before the resolution was |
| `SignerAuthorised` | The transaction signer is a `governance` signer's current key under the company at the resolution's own height |

| Execution check | Holds when |
|---|---|
| `Decodes` | The transaction is in `irena.execution.v1` and holds a V1 execution |
| `ResolutionVerifies` | The resolution it names passes every check above |
| `ResolutionAuthorisesThis` | That resolution is an amendment resolution, for this target |
| `AmendmentExists` | The amendment is an ordinary company record of that kind, for this company |
| `AmendmentMatchesResolution` | Its body is byte for byte the resolution's, and digests to what the channel approved |
| `AmendmentReplacedApprovedBase` | It superseded exactly the record the actors approved for replacement |
| `SelfDemotionHolds` | An amendment on an individual decision satisfies the rule for its target — channel set (R1 and R2), identities or authorisation — re-applied against the company just before the amendment; for a collective decision or a register amendment it passes and says *not applicable* |
| `AmendmentApplied` | It is in the company's history for that part at the execution's height: it took effect |
| `HeightsOrdered` | resolution < amendment ≤ execution |
| `SignerAuthorised` | The execution's transaction signer is a `governance` signer's current key under the company at the execution's own height |
| `ExecutedOnce` | No earlier execution of the same resolution exists |

### 10.6 What a verified execution does and does not say

It says: this channel existed at the frozen height and resolved to these actors with
these weights and keys; they passed this exact proposal at this meeting under these
rules, or the sole actor signed it; a notary attested to the resolution; and the
company record now in force is byte for byte what they approved, replacing exactly
what they saw — and, if one person rewrote who decides, only downwards.

It does not say that the register named the real owners (§3), that the configured
channels are the ones the law recognises, that anyone was entitled to *propose* the
resolution, or that the channels the company configured are the ones an outside body
would recognise. It does say that the key that published each record was, at that
height, a `governance` signer's current key under the company's own authorisation
(§1.5) — and that the channel was scoped to the part it amended, in the set it decided
under (§1.3). Who may write and what each channel may decide are both company data now,
checked like everything else.

## 11. Not in V1 — the road ahead

Recorded here so they are decisions, not omissions. The stages are planned, in this
order, and none is started:

1. ~~Shareholder meetings and votes.~~ **Done** — §9.
2. ~~Resolutions and resulting company-state changes.~~ **Done** — §10.
3. ~~Board membership, meetings and decisions.~~ **Done, without a board type** —
   §1.3: a board is a collective channel over a roster, a sole executive an individual
   one, and both decide through §7–§10 unchanged.
4. ~~Identities and authorisation.~~ **Done** — §1.4 and §1.5: one key table, and who
   may sign which family of record, checked at publish, at reconstruction and by every
   governance verifier.
5. ~~Scoped channels.~~ **Done** — §1.3: a channel amends the parts it lists and
   nothing else, refused when the resolution is recorded and re-checked by
   `WithinChannelScope`.
6. **Placidia coordination and UI.** The Tauri front end and whatever coordinates
   several instances; nothing in the crates below assumes either.

Next to the channel abstraction, recorded rather than built: **per-channel
supersession**, if
rewriting the whole set on every change proves costly. **More actor sources** (a
register of another company, an external roster by digest) as one more `match` arm.
**A wider self-demotion rule**, if the documented gaps — an individual channel
rewriting a channel it is not part of, registering a new person, or authorising
somebody else — turn out to matter in practice. **Notary ids bound to identities**, so
the person attesting and the person signing can be checked against each other.

Also deliberately absent from V1: **share classes** with votes-per-share (the company
this is built for has flat shares; when classes arrive they are a new version of the
`<share-structure>` body — the flat body stays readable forever — and the register
source's weight line gains one factor in exactly one place), secret ballots,
delegation, proxies, networking, consensus.
