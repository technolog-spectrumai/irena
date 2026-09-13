# Bornite CLI

The `bornite` binary is a thin front end over the Bornite libraries. It reads files,
calls the engine, and renders what comes back; no counting, comparing or parsing lives
here.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | The motion was **accepted** (or, for `validate-rules`, the document is valid) |
| `1` | The motion was **rejected** |
| `2` | The input was invalid or the vote could not be evaluated |

A rejection is a real answer. It is kept apart from bad input so a script can tell
"the vote failed" from "the file is broken".

## `validate-rules`

```console
$ bornite validate-rules --in rules.xml
rules.xml is a valid voting-rules document
weight:      Electorate
exclusions:  enabled
quorum:      Fraction { fraction: 1/2, basis: EffectiveElectorate }
threshold:   SimpleMajority { basis: VotesCast }
abstentions: Exclude
tie:         Reject
```

`--json` prints the typed rules. Every issue in an invalid document is reported
together:

```console
$ bornite validate-rules --in bad.xml
error: bad.xml is not a valid voting-rules document: 2 issue(s): <weight type="shares"> is not valid: expected one of equal, electorate; <tie treatment="maybe"> is not valid: expected one of reject, accept
```

## `evaluate`

```console
$ bornite evaluate --rules rules.xml --vote vote.xml
outcome:        Accepted
reason:         threshold_met
electorate:     3 voters, 1 excluded, 2 effective; weight 6 total, 4 effective
participation:  2 ballots weighing 4; 0 silent weighing 0
tally:          yes 3 (1), no 0 (0), abstain 1 (1)
quorum:         at least 1/2 of EffectiveElectorate weighing 4 — actual 4 — met
threshold:      yes 3 against 1/2 of VotesCast weighing 3 — Above — met
```

`--json` prints the complete `VoteEvaluationV1` — every intermediate quantity, both
requirements with what they were measured against, and the typed reason — so the result
can be audited without re-running anything. See `BORNITE_V1.md` §6.

The vote document holds the electorate and the ballots:

```xml
<vote version="1.0">
  <electorate>
    <voter id="drone-01" weight="3"/>
    <voter id="drone-02"/>
    <voter id="drone-03" weight="2" excluded="true"/>
  </electorate>
  <ballots>
    <ballot voter="drone-01" choice="yes"/>
    <ballot voter="drone-02" choice="abstain"/>
  </ballots>
</vote>
```

Ballot order does not matter; the same ballots in any order produce byte-identical
output.
