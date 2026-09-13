# Irena CLI

The `irena` binary is a thin front end over `irena-ledger`, `irena-vote` and
`irena-core`. It reads files, calls the libraries, and renders what comes back; no
validation, reconstruction or counting lives here. `--json` on any command prints the
same information as JSON.

The chain file comes from `--chain`, the `IRENA_CHAIN` environment variable, or
`irena.chain`. **One company per chain**, so no command but `init` names the company.
An Irena chain is a plain Prunella chain: `prunella verify`, `prunella export` and
every other Prunella command work on it unchanged.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | A finding: `verify-structure` cannot reconstruct the company; `vote evaluate` rejected the motion; `vote verify` found a record that does not hold |
| `2` | Invalid input or a refused operation (a stale amendment, an invalid body, a bad notary field, no company on the chain) |

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

Creates a chain whose genesis block founds the company. The genesis document is the
whole company: identity, share register and governance (see IRENA_V1.md §1.1).

```console
$ prunella keygen --out k.key
$ irena --chain acme.chain init --network acme-net --company acme \
      --genesis company.xml --signing-key k.key \
      --notary-id notary-07 --notary-name "Jane Roe" \
      --notary-address "12 High Street, London" --notary-at 2026-03-01T09:30:00Z
created acme.chain
genesis:        7c1e…
company:        acme
name:           Acme Industries Ltd
holders:        3 (1000 shares)
genesis record: 5d02…
```

`--genesis-timestamp` defaults to 0, so the same founding inputs produce the same
genesis hash on any machine. A genesis missing its register or its governance is
refused with every missing part named, and no chain is created.

## `publish-identity`, `publish-shares`, `publish-rules`

Publish an amendment to one part of the company, in its own block. `--supersedes` must
name the transaction currently providing that part — the genesis for the first
amendment, then the last amendment of the part; `show` prints it.

```console
$ irena publish-shares --file shares-v2.xml --signing-key k.key --supersedes 5d02… \
      --notary-id notary-07 --notary-name "Jane Roe" --notary-at 2026-04-01T10:00:00Z
published share-structure for acme at height 1 as 9b7a…
supersedes: 5d02…
notary:     Jane Roe (notary-07) at 2026-04-01T10:00:00Z
```

With the id of a version that no longer provides the part, the command exits `2` with
`stale amendment: share-structure is currently provided by 9b7a…, but the record
supersedes 5d02…`, and nothing is written. `--timestamp` sets the block timestamp in
milliseconds; it defaults to the clock and is never earlier than the parent block's.

## `show`

The company reconstructed at a height — identity, register and rules, each with the
record that provides it.

```console
$ irena show --at 1
company acme at height 1 (founded at height 0 by 5d02…)
name:            Acme Industries Ltd
jurisdiction:    gb
registered no.:  01234567
identity: 5d02… (height 0, supersedes none)
  notary:     Jane Roe (notary-07), 12 High Street, London at 2026-03-01T09:30:00Z
shares  : 9b7a… (height 1, supersedes 5d02…)
  notary:     Jane Roe (notary-07) at 2026-04-01T10:00:00Z
rules   : 5d02… (height 0, supersedes none)
  notary:     Jane Roe (notary-07), 12 High Street, London at 2026-03-01T09:30:00Z
2 record(s) applied
```

`--at` defaults to the head. On a chain with no company, exit `2`.

## `shares`

The register at a height, with each holder's voting weight as `irena-vote` derives it
(flat shares: one share, one vote) and whether they hold a signing key.

```console
$ irena shares --at 0
share register of acme at height 0
record: 5d02… (height 0, supersedes none)
  notary:     Jane Roe (notary-07), 12 High Street, London at 2026-03-01T09:30:00Z
3 holder(s), 1000 share(s) in issue; one share, one vote; total weight 1000; 2 can sign
  alice                    shares          500  weight          500  can sign
  bob                      shares          300  weight          300  can sign
  carol                    shares          200  weight          200  no key: cannot sign
```

## `history`

Every record that has provided one part, in chain order: the genesis, then each
amendment of the part.

```console
$ irena history --kind share-structure
2 record(s) have provided share-structure up to height 1
height 0      5d02…  company-genesis  supersedes none
  notary:     Jane Roe (notary-07), 12 High Street, London at 2026-03-01T09:30:00Z
height 1      9b7a…  share-structure  supersedes 5d02…
  notary:     Jane Roe (notary-07) at 2026-04-01T10:00:00Z
```

## `verify-structure`

Reconstructs the company record by record and reports. Exits `1` if it cannot.

```console
$ irena verify-structure
company acme reconstructs at height 1: 2 record(s) applied, every link holds
  identity         1 version(s), provided by 5d02…
  share-structure  2 version(s), provided by 9b7a…
  voting-rules     1 version(s), provided by 5d02…
nothing was repaired because nothing needed it
```

A break can only come from a record written around Irena through Prunella directly:
an amendment that does not supersede the current provider, a second genesis, a record
for another company, an unreadable record. It is named exactly and left alone; heights
before it still reconstruct (`--at`).

## `vote`

A vote is carried through its lifecycle as a **state file**: the vote's canonical
bytes, written after every step, so each step is one invocation and the same file on
another machine is the same vote. Ballots are files too, so a holder signs on their
own machine and hands the file over. The vote learns its company from the chain.

```console
$ irena vote new --subject "Approve the 2026 accounts" --proposal-digest d0d0… --state v.state
$ irena vote freeze --state v.state --at 0
frozen at height 0: 3 voter(s), register 5d02…, rules 5d02…
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
finalized at height 2 as transaction f9e5…
outcome: Accepted, 2 ballot(s), commitment 5406…
```

`evaluate` exits `1` for a rejected motion, as `bornite evaluate` does. `status` shows
where a vote is at any point.

### `vote verify`

Re-establishes a final record from nothing but the chain and its transaction id, and
names every check:

```console
$ irena vote verify --tx f9e5…
verification of f9e5… at height 2
  ok   Decodes                  canonical V1 record
  ok   VoteIdDerives            stored 79f6… derived 79f6…
  ok   SnapshotPrecedesRecord   snapshot at height 0, record at height 2
  ok   RecordsResolve           genesis, register and rules at height 0 are the pinned records
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
