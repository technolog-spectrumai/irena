# Irena CLI

The `irena` binary is a thin front end over `irena-ledger`, `irena-decision`,
`irena-vote`, `irena-meeting`, `irena-resolution` and `irena-core`. It reads files,
calls the libraries, and renders what comes back; no validation, reconstruction or
counting lives here. The output below is from the run that
[governance.md](../governance.md) walks through, founded from
[examples/genesis-three-channels.xml](../examples/genesis-three-channels.xml). `--json` on any command prints the
same information as JSON.

The chain file comes from `--chain`, the `IRENA_CHAIN` environment variable, or
`irena.chain`. **One company per chain**, so no command but `init` names the company.
An Irena chain is a plain Prunella chain: `prunella verify`, `prunella export` and
every other Prunella command work on it unchanged.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | A finding: `verify-structure` cannot reconstruct the company; `vote evaluate` rejected the motion; `vote verify`, `decision verify`, `meeting verify` or `resolution verify` found a record that does not hold |
| `2` | Invalid input or a refused operation (a stale amendment, an invalid body, a bad notary field, no company on the chain, a vote through an individual channel, a self-promoting amendment) |

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
whole company: identity, share register and the decision channels (see IRENA_V1.md
§1.1 and §1.3).

```console
$ prunella keygen --out k.key
$ irena --chain acme.chain init --network acme-net --company acme \
      --genesis genesis-three-channels.xml --signing-key k9.key \
      --notary-id notary-07 --notary-name "Jane Roe" --notary-at 2026-03-01T09:30:00Z
created acme.chain
genesis:        91ffaf72…
company:        acme
name:           Acme Industries Ltd
holders:        3 (1000 shares)
genesis record: fd171178…
```

`--genesis-timestamp` defaults to 0, so the same founding inputs produce the same
genesis hash on any machine. A genesis missing its register or its governance is
refused with every missing part named, and no chain is created.

## `publish-identity`, `publish-shares`, `publish-channels`, `publish-identities`, `publish-authorisation`

Publish an amendment to one part of the company, in its own block. `--supersedes` must
name the transaction currently providing that part — the genesis for the first
amendment, then the last amendment of the part; `show` prints it. `--signing-key` must
hold the current key of a person the company authorises for `company` records.

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

An unauthorised key is refused before anything is written, and the message names the
person who holds it:

```console
$ irena publish-shares --file shares-buyout.xml --signing-key k1.key --supersedes a29e… \
      --notary-id notary-07 --notary-name "Jane Roe" --notary-at 2026-03-01T09:30:00Z
error: unauthorised signer for company records: key 8a88…: alice holds this key but is
not a company signer in the authorisation in force
```

So is an amendment that would leave nobody able to amend the company again:

```console
$ irena publish-authorisation --file lockout.xml --signing-key k9.key --supersedes a29e… …
error: the record would lock the company out: no company signer holds a key; company
signers: carol (no key)
```

## `show`

The company reconstructed at a height — identity, register, channel set, identities
and authorisation, each with the record that provides it.

```console
$ irena show
company acme at height 25 (founded at height 0 by a29e…)
name:            Acme Industries Ltd
jurisdiction:    gb
registered no.:  01234567
identity     : a29e… (height 0, supersedes none)
  notary:     Jane Roe (notary-07) at 2026-03-01T09:30:00Z
shares       : 32a4… (height 5, supersedes a29e…)
  notary:     Jane Roe (notary-07) at 2026-03-01T09:30:00Z
channels     : bded… (height 19, supersedes e5bd…)
  notary:     Jane Roe (notary-07) at 2026-03-01T09:30:00Z
identities   : 7c5e… (height 21, supersedes a29e…)
  notary:     Jane Roe (notary-07) at 2026-03-01T09:30:00Z
authorisation: d7a4… (height 25, supersedes a29e…)
  notary:     Jane Roe (notary-07) at 2026-03-01T09:30:00Z
6 record(s) applied
```

`--at` defaults to the head. On a chain with no company, exit `2`.

## `identities`

Every person at a height: the key they currently sign with, and which families of
record they may sign. A person with no key is registered and unable to sign anything;
a signer who is not in the identities is named at the end, because the row counts for
nothing.

```console
$ irena identities
identities of acme at height 21
identities   : 7c5e… (height 21, supersedes a29e…)
  notary:     Jane Roe (notary-07) at 2026-03-01T09:30:00Z
authorisation: a29e… (height 0, supersedes none)
  notary:     Jane Roe (notary-07) at 2026-03-01T09:30:00Z
8 person(s); 6 hold a key; a person's key is their voice in every channel they sit on
  alice                    Alice Smith                    8a88e3dd7409f195…    may sign: nothing
  bob                      Bob Jones                      8139770ea87d175f…    may sign: nothing
  carol                    Carol White                    no key: cannot sign  may sign: nothing
  chen                     M. Chen                        ed4928c628d1c2c6…    may sign: nothing
  jane                     Jane Roe                       fd1724385aa0c75b…    may sign: company, governance
  okafor                   A. Okafor                      6e7a1cdd29b0b78f…    may sign: nothing
  quinn                    S. Quinn                       1398f62c6d1a457c…    may sign: nothing
  vance                    R. Vance                       no key: cannot sign  may sign: nothing
```

`--json` adds each person's `document-id` — an opaque passport or national-id number,
stored and never interpreted — and the raw signer rows.

## `shares`

The register at a height, with each holder's voting weight as the `share-register`
actor source resolves it (flat shares: one share, one vote) and whether the identities
in force at that height hold a key for them.

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

## `channels`

Every decision channel in the set in force at a height, **resolved** against the
company as it then stood: its mode, its actor source, and who its actors are with
their weights and signing ability. Individual channels are marked. A channel that does
not resolve — an individual channel whose source gives two people — is reported in
place, not hidden.

```console
$ irena channels
decision channels of acme at height 0
record: fd171178… (height 0, supersedes none)
  notary:     Jane Roe (notary-07) at 2026-03-01T09:30:00Z
3 channel(s)
  board                collective roster         3 actor(s), total weight 4, 2 can sign
      chen                     weight            2  can sign
      okafor                   weight            1  can sign
      vance                    weight            1  no key: cannot sign
      rules: {"weight":"electorate","exclusions_enabled":false,"quorum":{"type":"none"},…}
  ceo                  individual roster         1 actor(s), total weight 1, 1 can sign  — decides alone; may amend the channel set only downwards (self-demotion rule)
      chen                     weight            1  can sign
  shareholders         collective share-register 3 actor(s), total weight 1000, 2 can sign
      alice                    weight          500  can sign
      bob                      weight          300  can sign
      carol                    weight          200  no key: cannot sign
      rules: {"weight":"electorate","exclusions_enabled":true,"quorum":{"type":"fraction",…},…}
```

`publish-channels` replaces the whole set; `history --kind decision-channels` lists
every set that has been in force.

## `history`

Every record that has provided one part, in chain order: the genesis, then each
amendment of the part. `--kind` is `identity`, `share-structure`,
`decision-channels`, `identities` or `authorisation`.

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
company acme reconstructs at height 25: 6 record(s) applied, every link holds
  identity         1 version(s), provided by a29e…
  share-structure  2 version(s), provided by 32a4…
  decision-channels 3 version(s), provided by bded…
  identities       2 version(s), provided by 7c5e…
  authorisation    2 version(s), provided by d7a4…
nothing was repaired because nothing needed it
```

A break can only come from a record written around Irena through Prunella directly:
an amendment that does not supersede the current provider, a company record signed by
a key the company does not authorise, one that leaves nobody able to amend the company
again, a second genesis, a record for another company, an unreadable record. It is
named exactly and left alone; heights before it still reconstruct (`--at`).

## `vote`

A vote is carried through its lifecycle as a **state file**: the vote's canonical
bytes, written after every step, so each step is one invocation and the same file on
another machine is the same vote. Ballots are files too, so a holder signs on their
own machine and hands the file over. The vote learns its company from the chain.

```console
$ irena vote new --subject "Approve the 2026 accounts" --proposal-digest d0d0… --state v.state
$ irena vote freeze --state v.state --channel shareholders --at 0
frozen at height 0 through channel shareholders: 3 voter(s), register 5d02…, channels 5d02…
vote id: 79f6…
$ irena vote open --state v.state
```

`--channel` names a **collective** channel; its actors are the electorate and its rules
decide. `--channel ceo` exits `2`: *channel ceo is individual; it decides by one
signature, not by vote*.

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
  ok   RecordsResolve           genesis, register and channel set at height 0 are the pinned records
  ok   ChannelIsCollective      channel shareholders is collective
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

## `decision`

The individual counterpart of `vote`: one actor of an **individual** channel signs. A
decision is carried as a state file like a vote; the actor signs on their own machine.

```console
$ irena decision new --subject "Appoint auditors" --proposal-digest aaaa… --state d1.state
$ irena decision freeze --state d1.state --channel ceo
frozen at height 6 through channel ceo: actor chen (key ca93ac17…)
register b988c265…, channels fd171178…
decision id: b0069ff5…
$ irena decision sign --state d1.state --signing-key k4.key
signed by the actor
$ irena decision finalize --state d1.state --signing-key k9.key
recorded at height 7 as 01c40b29…
```

`freeze` refuses a collective channel (*it decides by vote, not by one signature*), a
channel that resolves to other than one actor, and an actor with no registered key.
`sign` refuses any key but the frozen actor's (*the signing key is not the registered
key of chen, the frozen actor*). `status` shows where a decision is.

### `decision verify`

```console
$ irena decision verify --tx 01c40b29…
decision record 01c40b29… at height 7
  ok   Decodes                  canonical V1 record
  ok   DecisionIdDerives        stored b0069ff5… derived b0069ff5…
  ok   SnapshotPrecedesRecord   snapshot at height 6, record at height 7
  ok   RecordsResolve           genesis, register and channel set at height 6 are the pinned records
  ok   ChannelIsIndividual      channel ceo is individual
  ok   ActorResolves            chen with the frozen key
  ok   SignatureVerifies        signed by chen
VALID: 7 check(s) re-established from the chain
```

## `meeting`

A meeting groups agenda items and the votes among them, and is a meeting **of one
collective channel** — `--channel shareholders`, `--channel board`, any collective
channel the company has. Like a vote it is carried as a **state file**, and like a vote
only formal facts reach the chain: the convening (the agenda) and the finalisation (the
outcomes). Everything between is local.

```console
$ irena meeting new --channel shareholders --title "Annual General Meeting 2026" \
      --scheduled-at 2026-06-01T10:00:00Z --notice-digest a0a0… --state m.state
$ irena meeting add-item --state m.state --title "Report of the directors" \
      --document-digest 1111…                                  # informational
$ irena meeting add-item --state m.state --title "Approve the 2026 accounts" \
      --proposal-digest 2222…                                  # a vote
$ irena meeting add-item --state m.state --title "Re-appoint the auditor" \
      --proposal-digest 3333…
```

An item is informational or a vote — exactly one of `--document-digest` and
`--proposal-digest`. Items are numbered from 1 in the order added.

```console
$ irena meeting convene --state m.state --signing-key k.key \
      --notary-id notary-07 --notary-name "Jane Roe" --notary-at 2026-05-01T09:00:00Z
convened for acme at height 1
meeting id: 0b8739ae…
```

Convening puts the agenda on the chain and fixes it; `add-item` afterwards is refused.
The meeting id is the convening transaction.

```console
$ irena meeting open --state m.state
opened; 2 vote(s) frozen at height 1
  item 2: vote bba0a937…
  item 3: vote f54d6ac6…
```

Each vote item freezes the company **on its own** at the current head, through the
meeting's channel. Amendments after that reach none of them: a holder removed
mid-meeting still votes, one added still cannot. A meeting of an individual channel
cannot open: *channel ceo is individual; it decides by one signature, not by vote*.

A board meeting is the same commands with `--channel board`; its electorate is the
roster at the roster's weights, and a shareholder's ballot is refused as *not in the
frozen electorate*. From the walk-through:

```console
$ irena meeting close --state board.state
closed and counted
  item 1: accepted (yes 2 no 1 abstain 0)
```

```console
$ irena meeting ballot --state m.state --item 2 --voter alice --choice yes \
      --signing-key alice.key --out a2.ballot
$ irena meeting cast --state m.state --item 2 --ballot a2.ballot
accepted a yes ballot from alice on item 2; 1 ballot(s) so far
```

A ballot names its item. Casting it against another item is refused (the vote ids
differ), as is casting against an informational item.

```console
$ irena meeting close --state m.state
closed and counted
  item 2: accepted (yes 500 no 300 abstain 0)
  item 3: rejected (yes 0 no 500 abstain 0)
$ irena meeting finalize --state m.state --signing-key k.key \
      --notary-id notary-07 --notary-name "Jane Roe" --notary-at 2026-06-01T12:00:00Z
finalized at height 5 as transaction e214ee97…
meeting 0b8739ae…, 3 item(s)
  item 2: accepted as eb39c5f2…
  item 3: rejected as c7ea6f59…
```

`finalize` writes every vote to the chain first, each its own transaction, then the
meeting record; an interrupted finalisation is resumed, not repeated. `show` prints the
agenda and where the meeting is at any point.

### `meeting verify`

Re-establishes the whole meeting from the chain and its transaction id — its own nine
checks, then every referenced vote's ten:

```console
$ irena meeting verify --tx e214ee97…
verification of meeting record e214ee97… at height 5
  ok   Decodes                final record of meeting 0b8739ae…
  ok   ConveningExists        convened at height 1 for acme
  ok   HeightsOrdered         convened at 1, opened at 1, finalised at 5
  ok   AgendaMatches          3 item(s), as convened
  ok   MetadataMatches        "Annual General Meeting 2026" of channel shareholders, scheduled 2026-06-01T10:00:00Z
  ok   CompanyReconstructs    acme at height 1: 3 holder(s)
  ok   VotesVerify            2 vote(s), each verified from the chain
  ok   VotesBelong            2 vote(s) match their agenda items
  ok   ItemsConsistent        3 item(s): 2 vote, 1 informational
  ok   item 2 vote eb39c5f2… (10 check(s))
  ok   item 3 vote c7ea6f59… (10 check(s))
the meeting is exactly what the chain says it was
```

A record whose agenda differs from the one convened fails `AgendaMatches`; one pointing
at another item's, another meeting's or another channel's vote fails `VotesBelong`
while that vote itself still passes — the forgery a per-vote check cannot see. See
[IRENA_V1.md §9.5](../IRENA_V1.md).

## `resolution`

A resolution turns a channel's approval — a passed vote at a meeting of a collective
channel, or a signed decision of an individual one — into company change. It is
carried as a **state file** like a vote or a meeting. Two things reach the chain: the resolution record at
`finalize`, and — for an amendment resolution — the amendment plus its execution
record at `execute`.

### `resolution digest`

Prints the proposal digest of an amendment body. This is what the meeting's agenda
item must carry, so the vote commits to exactly the body the resolution will execute.

```console
$ irena resolution digest --file new-register.xml
52a16af9…
use this as the agenda item's --proposal-digest so the vote commits to this body
```

### `create`, `finalize`

```console
$ irena resolution create --channel shareholders --meeting 7fcf6f08… --item 1 --vote 2d5ad305… \
      --title "Resolution 1: buy out carol" \
      --target share-structure --file shares-buyout.xml --state r1.state
$ irena resolution finalize --state r1.state --signing-key k9.key \
      --notary-id notary-07 --notary-name "Jane Roe" --notary-at 2026-03-01T09:30:00Z
recorded for acme at height 4
resolution: 78e062d9…
```

The authority is `--channel` plus either `--meeting`, `--item` and `--vote` (a
collective channel's vote) or `--decision` (an individual channel's decision):

```console
$ irena resolution create --channel ceo --decision 01c40b29… \
      --title "Resolution 2: appoint auditors" --document-digest aaaa… --state r2.state
```

`--target share-structure|decision-channels|identities|authorisation` with `--file`
makes an amendment resolution; `--document-digest` alone makes a declarative one. `finalize` checks the
whole chain of authority before writing anything — for a vote: the meeting verifies,
the item is a vote item, the named vote answered it, that vote verifies, **Bornite
accepted it**, it was through the channel named, and what the resolution carries
digests to what was approved; for a decision: it verifies, it was through the channel
named, and the digest matches. A rejected motion exits `2` with `the vote … was
rejected (threshold_not_met); a rejected motion authorises nothing`; a shareholders'
vote presented as the board's with `the resolution names channel board, but record …
was decided through channel shareholders`.

Recording a resolution changes nothing: `irena show` is identical afterwards.

### `execute`

```console
$ irena resolution execute --state r1.state --signing-key k9.key \
      --notary-id notary-07 --notary-name "Jane Roe" --notary-at 2026-03-01T09:30:00Z
executed at height 6
share-structure amendment: b988c265… (height 5, replacing fd171178…)
execution record: fb084c92…
```

The amendment is an ordinary company record — it appears in `irena history --kind
share-structure` like any other, and `irena show` now reports the new register.

Refused: a **declarative** resolution (nothing to execute); a **stale base**, where the
record the actors approved for replacement is no longer the one in force
(`the … approved for replacement was …, but … provides it now`); a **second
execution**, which the state machine and the chain both reject; and **self-promotion**
— a `decision-channels` amendment on an individual decision that would widen its
signer's reach. From the walk-through, the chief executive trying to shrink a board
they sit on:

```console
$ irena resolution execute --state r3.state …
error: channel ceo may not carry this amendment alone: board — a channel chen sits on — would change
```

The same person abolishing their own channel executes:

```console
$ irena resolution execute --state r4.state …
executed at height 14
decision-channels amendment: d8f43dbf… (height 13, replacing fd171178…)
execution record: 8b1ce256…
```

### `resolution verify`

```console
$ irena resolution verify --execution 8b1ce256…   # the ceo abolishing its own channel
verification of execution 8b1ce256… at height 14
  resolution f05850db…:
    ok   Decodes                      amendment resolution: "Resolution 4: abolish the ceo channel"
    ok   DecisionVerifies             decision cec4a9b0… verifies
    ok   ChannelMatches               individual channel ceo
    ok   ProposalMatches              the decision-channels body approved, 1f23046a… (1908 bytes)
    ok   CompanyMatches               acme
    ok   HeightsOrdered               decision recorded at 11, resolution at 12
    ok   SignerAuthorised             transaction signed by jane, a governance signer at height 12
  ok   Decodes                      execution of resolution f05850db…
  ok   ResolutionVerifies           resolution f05850db… verifies
  ok   ResolutionAuthorisesThis     a decision-channels amendment
  ok   AmendmentExists              a decision-channels record for acme
  ok   AmendmentMatchesResolution   the decision-channels the channel approved, 1f23046a…
  ok   AmendmentReplacedApprovedBase replaced fd171178…, the record the voters saw
  ok   SelfDemotionHolds            the channel set, amended on chen's own signature through channel ceo: chen keeps 1 seat(s) unchanged (board) and gains none; gives up ceo
  ok   AmendmentApplied             the amendment is in the company's decision-channels history at height 14
  ok   HeightsOrdered               resolution at 12, amendment at 13, execution at 14
  ok   SignerAuthorised             transaction signed by jane, a governance signer at height 14
  ok   ExecutedOnce                 the only execution of this resolution
the amendment is exactly what the channel authorised

$ irena resolution verify --execution 79573aef…   # the board creating a committee
  resolution 17a3ef27…:
    ok   MeetingVerifies              meeting e9b49a02… verifies at height 17
    ok   ItemIsAVote                  item 1: "Create an audit committee"
    ok   VoteAnsweredTheItem          item 1 was answered by bffac498…
    ok   VoteVerifies                 vote bffac498… verifies
    ok   VotePassed                   accepted (threshold_met)
    ok   ChannelMatches               collective channel board
    …
  ok   SelfDemotionHolds            not applicable: channel board decided collectively
  …
```

See [IRENA_V1.md §10.5](../IRENA_V1.md) for every check and what it proves.
