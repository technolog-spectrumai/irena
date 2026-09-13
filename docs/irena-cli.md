# Irena CLI

The `irena` binary is a thin front end over `irena-ledger` and `irena-core`. It reads
files, calls the libraries, and renders what comes back; no validation, resolution or
composition lives here. `--json` on any command prints the same information as JSON.

The chain file comes from `--chain`, the `IRENA_CHAIN` environment variable, or
`irena.chain`. An Irena chain is a plain Prunella chain: `prunella verify`,
`prunella export` and every other Prunella command work on it unchanged.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | A finding: `verify-structure` found a broken amendment chain |
| `2` | Invalid input or a refused operation (a stale amendment, an invalid body, a bad notary field) |

## Notarisation arguments

Every publishing command requires them; a record without a notary is not a record.

| Argument | | |
|---|---|---|
| `--notary-id` | required | Letters, digits, `. _ : + @ -` |
| `--notary-name` | required | |
| `--notary-address` | optional | |
| `--notary-at` | required | `YYYY-MM-DDTHH:MM:SSZ`, UTC, exactly |
| `--statement` | optional | |
| `--source-digest` | optional | 64 hex characters |

## `init`

Creates a chain whose genesis block carries the company's founding record.

```console
$ prunella keygen --out k.key
$ irena --chain acme.chain init --network acme-net --company acme \
      --genesis genesis.xml --signing-key k.key \
      --notary-id notary-07 --notary-name "Jane Roe" \
      --notary-address "12 High Street, London" --notary-at 2026-03-01T09:30:00Z
created acme.chain
genesis:        7c1e…
company:        acme
name:           Acme Industries Ltd
genesis record: 5d02…
```

`--genesis-timestamp` defaults to 0, so the same founding inputs produce the same
genesis hash on any machine.

## `publish-shares`, `publish-rules`, `publish-genesis`

Publish a record of one kind in its own block, amending the record of that kind in
force if there is one.

```console
$ irena publish-shares --company acme --file shares.xml --signing-key k.key \
      --notary-id notary-07 --notary-name "Jane Roe" --notary-at 2026-03-01T09:35:00Z
published share-structure for acme at height 1 as 9b7a…
supersedes: none
notary:     Jane Roe (notary-07) at 2026-03-01T09:35:00Z
```

An amendment must name the record it replaces:

```console
$ irena publish-shares --company acme --file shares-v2.xml --signing-key k.key \
      --supersedes 9b7a… --notary-id notary-07 --notary-name "Jane Roe" \
      --notary-at 2026-04-01T10:00:00Z
```

Without `--supersedes`, or with the id of a version that is no longer in force, the
command exits `2` with `stale amendment for company acme: share-structure in force is
9b7a…, but the record supersedes none`, and nothing is written. `--timestamp` sets the
block timestamp in milliseconds; it defaults to the clock and is never earlier than
the parent block's.

## `show`

What the company is at a height — identity, register and rules resolved together.

```console
$ irena show --company acme --at 2
company acme at height 2
name:            Acme Industries Ltd
jurisdiction:    gb
registered no.:  01234567
genesis: 5d02… (height 0, supersedes none)
  notary:     Jane Roe (notary-07), 12 High Street, London at 2026-03-01T09:30:00Z
shares : 9b7a… (height 1, supersedes none)
  notary:     Jane Roe (notary-07), 12 High Street, London at 2026-03-01T09:35:00Z
rules  : e410… (height 2, supersedes none)
  notary:     Jane Roe (notary-07), 12 High Street, London at 2026-03-01T09:40:00Z
```

`--at` defaults to the head. Asking at a height where one of the three is not yet in
force exits `2` naming the missing kind.

## `shares`

The register in force, with each holder's voting weight (flat shares: one share, one
vote) and whether they hold a signing key.

```console
$ irena shares --company acme
share register for acme at height 2
record: 9b7a… (height 1, supersedes none)
  notary:     Jane Roe (notary-07), 12 High Street, London at 2026-03-01T09:35:00Z
3 holder(s), 1000 share(s) in issue; one share, one vote
  alice                    shares          500  weight          500  can sign
  bob                      shares          300  weight          300  can sign
  carol                    shares          200  weight          200  no key: cannot sign
```

## `history`

Every version of one kind of record, in ledger order, with the link each carries.

```console
$ irena history --company acme --kind share-structure
2 share-structure version(s) for acme up to height 3
height 1      9b7a…  supersedes none
  notary:     Jane Roe (notary-07), 12 High Street, London at 2026-03-01T09:35:00Z
height 3      c2d8…  supersedes 9b7a…
  notary:     Jane Roe (notary-07), 12 High Street, London at 2026-04-01T10:00:00Z
```

## `verify-structure`

Walks all three amendment chains and reports. Exits `1` if any is broken.

```console
$ irena verify-structure --company acme
structure of acme up to height 3
  company-genesis  1 version(s), chain intact, in force: 5d02…
  share-structure  2 version(s), chain intact, in force: c2d8…
  voting-rules     1 version(s), chain intact, in force: e410…
every amendment chain links; nothing was repaired because nothing needed it
```

A break can only come from a record written around Irena through Prunella directly. It
is named exactly — height, transaction, what it should have superseded and what it
claims — and left alone.
