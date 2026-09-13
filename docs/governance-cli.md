# Governance CLI

The `governance` binary puts voting rules and electorate rolls on a Prunella ledger,
resolves what is in force at a height, and evaluates ballots against ledger truth.
Every command is a thin call into `governance-bridge`; the only thing the binary adds
is a clock for block timestamps, which the library deliberately does not have.

Global: `--chain <path>` (env `PRUNELLA_CHAIN`, default `prunella.chain`), `--json`.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success; for `evaluate`, the motion was accepted |
| `1` | The motion was rejected |
| `2` | Invalid input, or the operation was refused |

## Notarisation

Every publishing command accepts `--notary <id>`, and with it `--statement <text>` and
`--source-digest <hex64>`. The digest names an external document — a share register,
minutes, a fleet manifest — that the notary attests to. It is stored and reproduced,
never parsed.

## Commands

### `init`

```console
$ governance --chain gov.chain init --network swarm --subject swarm-alpha \
      --rules rules.xml --signing-key k.key --notary notary-07 --statement "founded"
created gov.chain
genesis: 3e0f…
rules record: 9b21…
```

Creates a chain whose **genesis block carries the rules record**. `--genesis-timestamp`
defaults to `0`, so the same inputs give the same genesis hash anywhere. The key file
is the hex seed `prunella keygen` writes.

### `publish-rules` / `publish-roll`

```console
$ governance --chain gov.chain publish-roll --subject swarm-alpha \
      --roll roll.xml --signing-key k.key
$ governance --chain gov.chain publish-rules --subject swarm-alpha \
      --rules rules-v2.xml --signing-key k.key --supersedes 9b21…
```

`--supersedes` must name the record currently in force for the subject; it is required
when one is and forbidden when none is. Anything else is refused as a stale amendment
and nothing is written. `--timestamp` overrides the clock and may not precede the
parent block's.

### `show-rules` / `show-roll`

```console
$ governance --chain gov.chain show-rules --subject swarm-alpha --at 0
$ governance --chain gov.chain show-roll --subject swarm-alpha
```

The record in force at `--at` (default: the head), with the transaction it came from,
its height, what it superseded, and who notarised it.

### `history`

```console
$ governance --chain gov.chain history --subject swarm-alpha --kind voting-rules
2 voting-rules version(s) for swarm-alpha up to height 3
height 0      9b21…  supersedes none  notary notary-07
height 3      c7d4…  supersedes 9b21…  notary none
```

### `evaluate`

```console
$ governance --chain gov.chain evaluate --subject swarm-alpha --ballots ballots.xml --at 2
```

Resolves the rules and roll in force at `--at`, evaluates the `<ballots>` document
against them with Bornite, and prints the result together with the records used. With
`--json` the payload's `evaluation` field is Bornite's `VoteEvaluationV1`, byte for
byte what `bornite evaluate` would print for the same inputs.

The ballots document:

```xml
<ballots>
  <ballot voter="drone-01" choice="yes"/>
  <ballot voter="drone-02" choice="no"/>
</ballots>
```

## A complete session

```console
$ prunella keygen --out k.key
$ governance --chain gov.chain init --network swarm --subject swarm-alpha \
      --rules rules.xml --signing-key k.key
$ governance --chain gov.chain publish-roll --subject swarm-alpha \
      --roll roll.xml --signing-key k.key
$ governance --chain gov.chain evaluate --subject swarm-alpha --ballots ballots.xml
$ governance --chain gov.chain publish-rules --subject swarm-alpha \
      --rules rules-v2.xml --signing-key k.key --supersedes <id from init>
$ governance --chain gov.chain evaluate --subject swarm-alpha --ballots ballots.xml --at 1
$ governance --chain gov.chain evaluate --subject swarm-alpha --ballots ballots.xml
$ prunella --chain gov.chain verify
```

The chain is an ordinary Prunella chain throughout: `prunella verify`, `export` and
`import` all work on it unchanged.
