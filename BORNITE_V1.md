# Bornite V1 — deterministic voting

**Status: frozen.** Every type, rule and step in this document is fixed. A change to
any of them is not a revision of V1; it is a V2, introduced as new types alongside
these.

Bornite takes a frozen electorate, a set of voting rules and a set of ballots, and
returns the same auditable result on every machine. It knows nothing about who the
voters are, what is being decided, or where the result goes. The same rules organise a
company meeting, a non-profit's membership vote, or a fleet of drones deciding a
peaceful deployment — and they are enough to do so with no organisation behind them
at all.

---

## 1. Determinism guarantees

A result depends on exactly three inputs — rules, electorate, ballots — and on nothing
else. In particular it does not depend on:

| Not an input | How that is enforced |
|---|---|
| Floating point | `clippy::float_arithmetic` is denied across the workspace |
| Hash-map iteration order | `BTreeMap`/`BTreeSet` only; a test greps every Bornite source for `HashMap`/`HashSet` |
| Input ordering | Electorates and ballot sets sort at construction; a property test permutes both and asserts an identical result |
| Time, locale, randomness | No clock, no random source, no case folding of user input |
| Which error was hit first | Every validation issue is collected and reported, sorted |

## 2. Arithmetic

* A **weight** is a `u64` of at least 1. Zero is refused.
* A **total** is a `u64` of at least 0. Every addition is checked; a sum past `u64::MAX`
  is `WeightOverflow`, never a wrap.
* A **fraction** is `numerator / denominator` with both `u64` and the denominator at
  least 1. It is never reduced, divided or rounded. Equality is structural: `1/2` and
  `2/4` are different values that compare as equal shares.
* A **rule fraction** must also be a proportion: `numerator <= denominator`. A share
  above the whole is a mistake, not a strict rule.
* **Every comparison is a cross-multiplication in `u128` and is total.** To ask whether
  `value` is above, exactly at, or below `n/d` of `basis`:

  ```
  compare  value × d   against   n × basis
  ```

  All four operands are `u64`, and `(2^64 − 1)^2 < 2^128`, so the products cannot
  overflow. This is asserted by test at `u64::MAX × u64::MAX`, not assumed. The
  evaluation therefore has exactly one failure mode — a weight sum overflowing — and it
  fails loudly.

## 3. Types

| Type | Meaning |
|---|---|
| `VoterIdV1` | 1–128 bytes of `A-Z a-z 0-9 . _ : + @ -`. Compared **byte for byte**; `Alice` and `alice` are two voters. Never interpreted. |
| `WeightV1` | An integer ≥ 1. |
| `VoterV1` | `id`, `weight`, `excluded: bool`. Why a voter is excluded is not recorded. |
| `ElectorateV1` | Voters sorted by id, no duplicates. |
| `ChoiceV1` | `yes`, `no`, `abstain`. |
| `BallotV1` | `voter`, `choice`. |
| `BallotSetV1` | Ballots sorted by voter, no duplicates. One voter, one ballot; a second is refused, not treated as a correction. |
| `FractionV1` | `numerator`, `denominator ≥ 1`. |
| `VotingRulesV1` | §4. |
| `VoteEvaluationV1` | §6. |

## 4. Rules

```xml
<voting-rules version="1.0">
  <weight type="equal | electorate"/>
  <exclusions enabled="true | false"/>
  <quorum type="none"/>
  <quorum type="absolute" weight="N"/>
  <quorum type="fraction" numerator="N" denominator="D"
          basis="total-electorate | effective-electorate"/>
  <threshold type="simple-majority"
             basis="votes-cast | effective-electorate | total-electorate"/>
  <threshold type="fraction" numerator="N" denominator="D" basis="…"/>
  <abstentions treatment="exclude | include"/>
  <tie treatment="reject | accept"/>
</voting-rules>
```

Exactly one of each element, in any order. `version` is exactly `1.0`. An attribute a
rule type does not use (a `weight` on `type="none"`) is refused, not ignored.

| Rule | Meaning |
|---|---|
| `weight` | `equal`: every voter counts 1. `electorate`: each voter counts their declared weight. |
| `exclusions` | Whether voters marked `excluded` are removed from the effective electorate. |
| `quorum` | The participating weight required before the vote can decide anything. `absolute` is a fixed weight; `fraction` is a share of the named basis. |
| `threshold` | The share of the basis the YES weight must reach. `simple-majority` is exactly `1/2`. |
| `abstentions` | Whether abstentions stay in a `votes-cast` denominator. `exclude`: YES is measured against YES + NO. `include`: against everyone who voted. Under an electorate basis this rule cannot change the denominator; it is still recorded, and the result says so (§6). |
| `tie` | What happens when the threshold is met **exactly** (§5.8). |

### 4.1 Contradictions

These pairings of rules and electorate are refused, with every issue reported together:

| Contradiction | Issue |
|---|---|
| `weight="equal"` but a voter declares a weight other than 1 | `contradictory_weight` (one per voter) |
| `exclusions enabled="false"` but a voter is marked `excluded` | `contradictory_exclusion` (one per voter) |
| `quorum type="absolute"` above the electorate's total weight under the weight rule | `unachievable_quorum` |
| A rule fraction above 1 | `improper_fraction` |
| Weights summing past `u64::MAX` | `weight_overflow` |

Bornite never resolves a contradiction by picking a side. A caller who says "equal
weights" over an electorate that declares weights has made a mistake, and the engine's
job is to say so.

## 5. Evaluation

The steps run in this order, always, and none is skipped when an earlier one decides
the outcome.

### 5.1 Validate

Rules against electorate, per §4.1. Any issue stops evaluation with a typed error.

### 5.2 Apply exclusions

A voter is **effective** unless `exclusions` is enabled and the voter is marked
`excluded`.

### 5.3 Resolve weights

Each voter's weight is 1 under `equal` and their declared weight under `electorate`.

```
total_weight     = Σ weight over all voters
effective_weight = Σ weight over effective voters
```

### 5.4 Validate ballots

Every issue is collected and reported together, sorted by voter:

| Issue | When |
|---|---|
| `unknown_voter` | The ballot names a voter not in the electorate |
| `excluded_voter` | The ballot comes from a voter who is not effective |

Duplicate ballots cannot reach this step: a ballot set refuses them at construction.

### 5.5 Participation and quorum

```
participating_weight = Σ weight over voters who cast any ballot, including abstain
```

| Quorum | Met when |
|---|---|
| `none` | always |
| `absolute N` | `participating_weight >= N` |
| `fraction n/d of basis` | `participating_weight × d >= n × basis_weight` |

`basis_weight` is `total_weight` or `effective_weight` as named. Quorum is
**inclusive**: exactly the requirement meets it. The tie rule does not apply here.
A share of nothing is met by nothing (`0 >= n/d × 0`).

### 5.6 Tally

YES, NO and ABSTAIN weight and count, each a checked sum.

### 5.7 Threshold denominator

| `basis` | `denominator_weight` |
|---|---|
| `votes-cast`, abstentions `exclude` | `yes_weight + no_weight` |
| `votes-cast`, abstentions `include` | `participating_weight` |
| `effective-electorate` | `effective_weight` |
| `total-electorate` | `total_weight` |

### 5.8 Threshold and tie

With the threshold fraction `n/d` (`1/2` for simple majority):

```
compare  yes_weight × d   against   n × denominator_weight
  greater  → comparison Above
  equal    → comparison Exactly   (a tie)
  less     → comparison Below
```

Then, in this order of precedence:

| Condition | Outcome | Reason |
|---|---|---|
| Quorum not met | Rejected | `quorum_not_met` |
| `denominator_weight = 0` | Rejected | `empty_threshold_basis` |
| Above | Accepted | `threshold_met` |
| Below | Rejected | `threshold_not_met` |
| Exactly, tie `accept` | Accepted | `tie_accepted` |
| Exactly, tie `reject` | Rejected | `tie_rejected` |

**The tie rule is the boundary rule.** The comparison is always strict; a result that
lands exactly on the threshold is a tie whatever the threshold is. `reject` makes every
threshold "more than"; `accept` makes it "at least". Simple majority with `reject` is
the ordinary "more than half"; a `1/1` threshold with `accept` is unanimity.

A quorum failure still has the threshold computed and reported. It did not decide
anything, but an auditor can see what would have.

### 5.9 Produce the evaluation

## 6. `VoteEvaluationV1`

| Field | Contents |
|---|---|
| `version` | `1` |
| `rules` | The rules echoed, so the result is self-describing |
| `electorate` | `voter_count`, `excluded_count`, `effective_voter_count`, `total_weight`, `effective_weight` |
| `participation` | `ballot_count`, `weight`, `non_participant_count`, `non_participant_weight` |
| `tally` | `yes_weight`, `no_weight`, `abstain_weight`, `yes_count`, `no_count`, `abstain_count` |
| `quorum` | `requirement` (`none` / `absolute {weight}` / `fraction {fraction, basis, basis_weight}`), `actual_weight`, `met` |
| `threshold` | `basis`, `denominator_weight`, `required_fraction`, `abstentions_affected_denominator`, `yes_weight`, `comparison`, `met` |
| `outcome` | `accepted` / `rejected` |
| `reason` | One of the six reason codes in §5.8 |

`abstentions_affected_denominator` is `true` only under a `votes-cast` basis. Under an
electorate basis the abstention rule was valid and is echoed in `rules`, but it could
not have changed anything, and this field says so rather than leaving it to be inferred.

Invariants an auditor can check from the result alone:

```
yes_count + no_count + abstain_count            = participation.ballot_count
yes_weight + no_weight + abstain_weight         = participation.weight
participation.weight + non_participant_weight   = electorate.effective_weight
effective_voter_count + excluded_count          = electorate.voter_count
outcome = accepted   ⇒   quorum.met ∧ threshold.met
```

## 7. Documents

Two standalone documents, no XML namespace, `version="1.0"`:

* `<voting-rules>` — §4. Schema: `schemas/bornite-voting-rules-v1.xsd`.
* `<vote>` — an `<electorate>` of `<voter id weight? excluded?>` and a `<ballots>` of
  `<ballot voter choice>`. `weight` defaults to 1, `excluded` to `false`. Schema:
  `schemas/bornite-vote-v1.xsd`.

Readers are strict: an unknown element or attribute is refused. The `voting-rules` and
`electorate` element types are defined once in the schemas and reused, unchanged, by any
document that embeds them — that reuse is what keeps a rules document meaning the same
thing wherever it appears.

## 8. What V1 does not do

Voting classes, delegation, proxies, secret ballots, cryptographic authentication of
ballots, ranked or multi-option choices, persistence, networking. A ballot is a claim
that a voter chose something; establishing that the claim is genuine is the caller's
concern.

## 9. Assumptions

* `u64` weights and totals suffice for any electorate V1 will see.
* A frozen electorate is genuinely frozen: the caller does not change it between
  validation and evaluation. Bornite evaluates one snapshot.
* Voter ids are stable identifiers the caller controls. Bornite does not know, and does
  not need to know, what they denote.
