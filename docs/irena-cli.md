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
| `1` | A finding: `verify-structure` found a broken chain; `vote evaluate` rejected the motion; `vote verify` found a record that does not hold |
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

The register in force, with each holder's voting weight as `irena-vote` derives it
(flat shares: one share, one vote) and whether they hold a signing key.

```console
$ irena shares --company acme
share register for acme at height 2
record: 9b7a… (height 1, supersedes none)
  notary:     Jane Roe (notary-07), 12 High Street, London at 2026-03-01T09:35:00Z
3 holder(s), 1000 share(s) in issue; one share, one vote; total weight 1000; 2 can sign
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

## `vote`

A vote is carried through its lifecycle as a **state file**: the vote's canonical
bytes, written after every step, so each step is one invocation and the same file on
another machine is the same vote. Ballots are files too, so a holder signs on their
own machine and hands the file over.

```console
$ irena vote new --company acme --subject "Approve the 2026 accounts" \
      --proposal-digest d0d0… --state v.state
$ irena vote freeze --state v.state --at 2
frozen at height 2: 3 voter(s), register 9b7a…, rules e410…
vote id: 79f6…
$ irena vote open --state v.state
```

Each holder signs a ballot with their registered key, and someone casts it:

```console
$ irena vote ballot --state v.state --voter alice --choice yes \
      --signing-key alice.key --out alice.ballot
$ irena vote cast --state v.state --ballot alice.ballot
accepted a yes ballot from alice; 1 ballot(s) so far
```

A ballot is refused, with the reason, if the vote is not open, the ballot is for
another vote, the voter is not in the frozen electorate, has no registered key, the
signature does not verify, or the voter already voted — in that order. The register
may be amended while the vote is open; the vote does not notice, because it was frozen.

```console
$ irena vote close --state v.state
$ irena vote evaluate --state v.state
evaluated: Accepted (threshold_met)
tally:   yes 500 no 300 abstain 0
quorum:  met (participation 800 of 1000)
threshold: yes 500 of 800 against 1/2, Above
$ irena vote finalize --state v.state --signing-key k.key
finalized at height 4 as transaction f9e5…
outcome: Accepted, 2 ballot(s), commitment 5406…
```

`evaluate` exits `1` for a rejected motion, as `bornite evaluate` does. `status` shows
where a vote is at any point.

### `vote verify`

Re-establishes a final record from nothing but the chain and its transaction id, and
names every check:

```console
$ irena vote verify --tx f9e5…
verification of f9e5… at height 4
  ok   Decodes                  canonical V1 record
  ok   VoteIdDerives            stored 79f6… derived 79f6…
  ok   SnapshotPrecedesRecord   snapshot at height 2, record at height 4
  ok   RecordsResolve           genesis, register and rules at height 2 are the pinned records
  ok   ElectorateDerives        3 voter(s), total weight 1000
  ok   BallotsVerify            2 ballot(s) signed by frozen voters
  ok   BallotsOrdered           strict voter order, no duplicates
  ok   CommitmentDerives        stored 5406… derived 5406…
  ok   ResultReproduces         Accepted: threshold_met
the record is exactly what the chain says it should be
```

A record with one byte of a ballot signature changed exits `1` with `FAIL
BallotsVerify` and `FAIL CommitmentDerives` — the ballot's digest moved with its
signature — and every other check still reported. See [IRENA_V1.md §7.6](../IRENA_V1.md).
