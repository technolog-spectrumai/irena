# The governance bridge

`governance-bridge` is the only crate that knows both Prunella (the ledger) and Bornite
(the voting engine) exist. It stores voting rules and electorate rolls on a ledger,
records every amendment, and resolves what was in force at any height so that a vote
can be evaluated against ledger truth.

```
prunella-*  ←── governance-bridge ──→  bornite-*
```

Neither arrow points the other way. No `prunella-*` crate depends on, imports, or
mentions Bornite; no `bornite-*` crate depends on, imports, or mentions Prunella. A test
greps every source file in both families to make sure of it.

## The boundary, and why it is drawn where it is

The `<voting-rules>` document is **enough to organise a vote with no organisation
behind it at all**. It says how to count and what passing means — weights, exclusions,
quorum, threshold, abstentions, ties — and nothing about who is voting or why. The same
document organises a company meeting, a non-profit's membership vote, or a fleet of
drones deciding a peaceful transport deployment.

That is not a matter of prose. It is built into the schemas: there is exactly **one**
definition of the `voting-rules` element (`schemas/bornite-voting-rules-v1.xsd`), and
the standalone rules file and the on-ledger record both include it. A rules document
written for one purpose is stored on a ledger for another **byte for byte unchanged**
(`compose_record` embeds the caller's element verbatim and a test asserts it).

The electorate is equally generic: voter ids and integer weights. Where a weight comes
from — shares, one member one vote, a drone's node count — is not recorded and not the
engine's concern.

### Where organisation-specific truth goes

Only into the **notarisation**:

```xml
<notarisation notary="notary-07"
              statement="Share register as of 1 March"
              source-digest="c41d…"/>
```

`source-digest` is the digest of an external document the notary attests to — a share
register, minutes, a fleet manifest. The bridge stores it, reproduces it, reports it,
and **never parses it**. A company's share structure reaches the ledger this way,
through a manual notary step, without a single company type entering any crate. A
non-profit puts its membership minutes there; a swarm puts its fleet manifest there;
the bridge cannot tell the difference and does not try.

## Records

A record is one Prunella transaction. Its **payload is the exact XML bytes** of a
`<governance-record>` document; the bytes are the record, and whitespace is preserved
because a notarised document is not something to re-serialise.

| Namespace | Kind | Carries |
|---|---|---|
| `governance.rules` | `voting-rules` | a `<voting-rules>` element |
| `governance.roll` | `roll` | an `<electorate>` element |

```xml
<governance-record version="1.0" kind="voting-rules" subject="swarm-alpha"
                   supersedes="8f3a…">
  <notarisation notary="notary-07" statement="…" source-digest="…"/>
  <voting-rules version="1.0">
    …
  </voting-rules>
</governance-record>
```

* `subject` — an opaque label for what is governed (`acme-agm-2026`, `membership`,
  `swarm-alpha`). Compared for equality, never interpreted. Grammar: 1–64 bytes,
  `a-z0-9` first, then `a-z0-9._-`.
* `supersedes` — the transaction id of the record this one amends. Absent on a
  subject's first record of each kind.
* `<notarisation>` — optional; above.
* Exactly one of `<voting-rules>` or `<electorate>`, matching `kind`.

Schema: `schemas/governance-record-v1.xsd`. The reader is strict: an unknown element
or attribute, a `kind` that does not match the element carried, or an inner element
Bornite refuses, is an error.

## Rules in genesis

Prunella's `GenesisSpec` accepts transactions, so a chain's founding rules are a
`governance.rules` transaction **inside the genesis block**:

```rust
let spec = genesis_with_rules(network, &key, &subject, rules_xml, notarisation, 0)?;
let store = LocalChainStore::init_genesis(path, spec)?;
```

The rules are in force from height 0, and two parties given the same inputs derive
the same genesis hash. Nothing in Prunella changed to make this possible.

## Amendment and resolution

* **`rules_in_force(subject, at)`** / **`roll_in_force(subject, at)`** walk blocks
  `0..=at` in ledger order, keep the records for the subject and kind, and return the
  last. Block order is total and transaction order within a block is fixed, so two
  instances holding the same chain always resolve the same record.
* **`supersedes` must name the record currently in force.** Publishing a record that
  supersedes anything else — an older version, nothing when something is in force, a
  record of the other kind — is refused as a *stale amendment* before the ledger is
  touched. This is what stops two editors who have not seen each other's work from
  silently clobbering one another.
* **A subject's first record supersedes nothing.**
* **Rules and rolls have separate chains.** A roll cannot supersede a rules record.
* **`history(subject, kind, at)`** returns every version in ledger order, with its
  height, transaction id, signer and notarisation — so "what changed, when, and who
  attested to it" is answerable from the chain alone.
* **A broken chain is reported, never repaired.** If a record was written around the
  bridge (published straight through Prunella) with a `supersedes` that does not link,
  `history` and both `*_in_force` return `BrokenAmendmentChain` naming the exact
  record. Nothing is in force for the subject until it is resolved by someone who can
  see what happened. Resolution at a height *before* the break still works.

Resolving at a past height returns the record in force *then*. An amendment does not
rewrite history; it adds to it.

## Evaluating against the ledger

```rust
let result = evaluate_at(&store, &subject, at, &ballots)?;
// result.rules  – the rules record used: value, tx id, height, notarisation
// result.roll   – the roll record used
// result.evaluation – Bornite's VoteEvaluationV1, unchanged
```

`evaluate_at` resolves the rules and roll in force at `at`, hands them to Bornite, and
returns the evaluation together with the records it used. A test asserts that the
result is **identical** to evaluating the same rules, roll and ballots standalone —
living on a ledger changes nothing about what the rules mean.

## What the bridge does not do yet

Ballots and results are not written to the ledger. The design leaves room for
`governance.ballot` and `governance.result` namespaces, but on-ledger voting needs
ballot signing and result-recording rules that have not been specified, so it is left
out rather than guessed at.

## Assumptions

* The notary is trusted by whoever reads the record. The bridge records who attested
  and to what; it does not, and cannot, verify that the external document is what the
  notary says it is.
* Subjects are chosen by the operator and are stable. Two different things given the
  same subject share an amendment chain.
* A record's signer is whoever held the key; the bridge does not know who that is.
  Deciding who may publish is a policy question for whatever sits above the bridge.
