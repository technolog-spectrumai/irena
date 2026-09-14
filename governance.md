# How a company decides, and how the chain proves it

This is the plain-language companion to [IRENA_V1.md](IRENA_V1.md). That document is
the specification — exact, normative, unforgiving. This one is the story it tells,
written for the people who will actually use the system: a shareholder who wants to
know their vote counted, a director who signs things, a company secretary who runs the
meeting, a notary who signs off on the paperwork, an auditor asked to check the whole
thing a year later, and a developer meeting the codebase for the first time.

---

## The problem

A company changes. Shares move, rules are rewritten, decisions are taken. Every one of
those changes is supposed to rest on some authority — the shareholders approved it at a
meeting, the board resolved it, the chief executive had the power to sign it.

In practice the authority and the change live in different places. The minutes sit in a
filing cabinet. The register sits in a spreadsheet. The articles that say who may decide
what sit in a drawer. Somebody updated the spreadsheet, and if you want to know *why*
— and whether they were allowed to — you go and look for the minutes and the articles
and hope they match. The link between the decision and the change is a human promise.

Irena makes that link a matter of record. Not because anyone is assumed dishonest, but
because a year later nobody remembers, and ten years later nobody is left to ask.

---

## The two rules

**Nothing that decides may also change. Nothing that changes may decide.**

A vote counts ballots. It cannot touch the share register — it has no way to, in the
code, not merely by convention. A signed decision records that one person decided. It
cannot touch the register either. A resolution records that a decision was taken; same
again. Only an *amendment* changes what the company is, and an amendment is the same
plain record it has always been: a new share register, or a new list of who decides,
signed, notarised, replacing exactly one previous one.

**Who may decide is itself a record.**

The company does not have "the shareholders" and "the board" built in. It has a list of
**decision channels**, on the chain, notarised like everything else. Each channel says
two things: *where the people come from* — the share register, or a list of members
written into the channel — and *how they decide* — one of them signs, or all of them
vote under a set of voting rules. The names on that list (`shareholders`, `board`,
`ceo`, `audit-committee`) are labels the notary chose. The software knows four words:
`share-register`, `roster`, `individual`, `collective`. It does not know what a board
is, and it does not know what the law says a board may do. What it knows is that a
change to who decides is an amendment like any other, with a history, and that a person
who rewrites that list alone may only ever write themselves *down*.

**What each channel may decide is itself a record.**

A channel does not simply decide. It says which parts of the company it may change:
the register, the list of who decides, the key table, the list of who may write. A
channel that says nothing may record decisions and change nothing at all, which is the
safe direction to be wrong in. So a chief executive can be given the power to reshape
the board and no power whatsoever over the share register, and that is not a policy
someone remembers to apply. It is a line in the document, refused before the record is
written and re-checked by anyone reading the chain a year later.

This matters most where the register is not the company's to decide. Where the
shareholder register is kept by an outside authority and entry in it is what makes
somebody a shareholder, the register on this chain is a **mirror**. Give no channel
`share-structure` in its scope and the mirror can only be brought up to date by the
company's authorised writer, filing what the keeper recorded. No meeting, no vote and
no signature moves it.

**Who may write is itself a record.**

Every record reaches the ledger signed by a key, and the company says whose key that
may be. One list — the **identities** — holds every person the company knows: the id
they hold shares or a seat under, a name, sometimes a passport or national-id number,
and the one key they currently sign with. A second — the **authorisation** — says which
kind of record each of them may put on the chain: amendments to the company, or the
records of governance (meetings, votes, decisions, resolutions). Because the key lives
in one place, rotating it is one amendment, and every channel that person sits on sees
the new key the moment it lands; anything already frozen keeps the key it froze.

Two things follow, and both are said out loud rather than hidden. A person authorised
for company records can rewrite the register **with no channel deciding anything** —
that is the company secretary's route, and the list is exactly who may take it. And no
record may leave the company without at least one such person holding a key, so a
company can never lose the ability to amend itself.

What joins all of it is a chain of references, each pinned to an exact transaction:

```text
                     ┌─ meeting → vote → passed result ─┐
company state → channel                                  → resolution → amendment → new company state
                     └─ one signed decision ─────────────┘
```

Read right to left, it answers "why is the register like this?" — because this
amendment; authorised by this resolution; which rests on this vote, of this meeting, of
this channel — or on this signature, through that channel. Every arrow is a separate
record on the ledger, and every reference is a transaction id, never a date, a name, or
"the current register".

---

## Walking it through

Acme Industries has three shareholders — Alice with 500 shares, Bob with 300, Carol
with 200 — a board of three directors, and a chief executive, M. Chen, who is also the
board's chair. Jane Roe, the company secretary, holds no shares and no seat and files
every record. What follows is a real chain of twenty-six blocks, from an actual run.
The ids are shortened here; the full output is in
[docs/irena-cli.md](docs/irena-cli.md), and the documents are under
[examples/](examples/README.md).

### Block 0 — the company exists, and so does its list of who decides

```console
$ irena init --network acme-net --company acme --genesis genesis-three-channels.xml \
      --signing-key k9.key --notary-id notary-07 --notary-name "Jane Roe" …
genesis record: fd171178…
```

One document founds the company: who it is, who holds its shares, who its people are
and what key each signs with, its decision channels, and who may put records on the
chain. It goes into block 0 as
`irena.company.v1`, notarised by Jane Roe. From here the company is never edited. It is
*reconstructed* — read the genesis, apply every amendment in order — and asking for
height 6 gives you the company as it was at height 6, whatever has happened since.

```console
$ irena channels
3 channel(s)
  board          collective  roster          3 actor(s), total weight 4, 2 can sign
      chen       weight 2  can sign
      okafor     weight 1  can sign
      vance      weight 1  no key: cannot sign
  ceo            individual  roster          1 actor(s)  — decides alone; may amend the channel set only downwards
      chen       weight 1  can sign
  shareholders   collective  share-register  3 actor(s), total weight 1000, 2 can sign
      alice      weight 500  can sign
      bob        weight 300  can sign
      carol      weight 200  no key: cannot sign

$ irena identities
7 person(s); 5 hold a key; a person's key is their voice in every channel they sit on
  alice     Alice Smith   8a88e3dd7409f195…    may sign: nothing
  bob       Bob Jones     8139770ea87d175f…    may sign: nothing
  carol     Carol White   no key: cannot sign  may sign: nothing
  chen      M. Chen       ca93ac1705187071…    may sign: nothing
  jane      Jane Roe      fd1724385aa0c75b…    may sign: company, governance
  okafor    A. Okafor     6e7a1cdd29b0b78f…    may sign: nothing
  vance     R. Vance      no key: cannot sign  may sign: nothing
```

Neither the register nor the channels carry a key: they carry ids, and this one list
says what each id signs with. Carol and Vance are in the company and hold no key — they
count towards quorum wherever they sit and can sign nothing. Jane Roe holds no shares
and sits on no channel; she is the company secretary, and she is the only person who
may put a record on this chain. Alice owns half the company and may not write to it at
all:

```console
$ irena publish-shares --file shares-buyout.xml --signing-key alice.key --supersedes a29e… …
error: unauthorised signer for company records: key 8a88…: alice holds this key but is
not a company signer in the authorisation in force
```

That is the separation the company chose when it was founded: deciding and writing are
different jobs, and the second one is a list you can read.

Three ways to decide, three configurations of the same thing. The shareholders' channel
takes its people from the register, so Alice's weight is her 500 shares. The board is a
roster written into the channel, with the chair's weight set to 2. The `ceo` channel is
a roster of one person, in *individual* mode: Chen signs, nobody votes. Notice the
warning on that line. The system does not know whether a company should have a chief
executive who can act alone; it knows that it *has* one, and says so every time you
look.

### Blocks 1–6 — the shareholders buy Carol out

Carol wants out, and Alice will buy her. That needs a new share register, and a new
share register needs the shareholders' approval.

```console
$ irena resolution digest --file shares-buyout.xml
616a3e8c…
```

This is the quiet hinge of the whole design. That digest is a fingerprint of the new
register — **the exact bytes**, not a description of them. Change one share count and
the fingerprint changes completely. The agenda item will carry it, so when the
shareholders vote "yes" they are not voting yes to *the idea* of buying Carol out. They
are voting yes to this register, these numbers, these holders. Later, the only register
that can be published is one whose fingerprint matches.

```console
$ irena meeting new --channel shareholders --title "Annual General Meeting 2026" …
$ irena meeting add-item --title "Buy out carol" --proposal-digest 616a3e8c… …
$ irena meeting convene …
convened for acme at height 1
meeting id: 71393ce9…
```

A meeting is a meeting *of* a channel. This one is the shareholders'; a board meeting
is the same command with `--channel board`. Convening puts the agenda on the chain
(block 1) and fixes it: what the shareholders were called to decide is what they will
be asked to decide. The meeting's identity *is* that transaction.

```console
$ irena meeting open …
opened; 1 vote(s) frozen at height 1
  item 1: vote 3251404d…
```

Opening creates one vote per vote item, and each vote takes a snapshot of the company
as it stands — *resolving the channel*: who the shareholders are at this height, how
many shares each holds, which keys they sign with, and what the channel's voting rules
say. That snapshot is the vote, for ever. Amend the register tomorrow and this vote does
not notice; Bob still votes with the 300 he held when the meeting opened. A vote is a
decision by a specific group of people at a specific moment, and the snapshot is what
makes that literally true.

Alice and Bob each sign a ballot on their own machine with the key in the register, and
the secretary casts them. Carol holds shares but has registered no key: she counts
towards quorum — she owns the shares — but cannot cast a ballot, and the system says
so rather than pretending otherwise. Ballots are not secret here; that is a deliberate
choice, and the reason the whole thing is checkable from the chain alone.

```console
$ irena meeting close …
  item 1: accepted (yes 800 no 0 abstain 0)
$ irena meeting finalize …                     # block 2: the vote, block 3: the meeting
```

Closing hands the frozen electorate and the ballots to Bornite, the voting engine,
which knows nothing about companies or shares: it sees voter ids, integer weights and
three choices, applies the rules, and returns a result with every intermediate number
shown. Finalising writes the vote's own record (block 2, `irena.vote.v1`) and the
meeting's (block 3, `irena.meeting.v1`). **Nothing about the company has changed.** A
motion carried. That is all.

```console
$ irena resolution create --channel shareholders --meeting 7fcf6f08… --item 1 \
      --vote 2d5ad305… --title "Resolution 1: buy out carol" \
      --target share-structure --file shares-buyout.xml --state r1.state
$ irena resolution finalize --state r1.state …
recorded for acme at height 4
resolution: 78e062d9…
```

A resolution is the formal record that a decision was taken. It names the channel and
— because this channel decides by vote — the meeting, the item on its agenda, and the
vote that answered it, each by transaction id. It carries the new register itself, so an
auditor reading the chain sees what was decided without hunting for an attachment.
Before anything is written, everything is checked against the chain, not against what
the draft claims: the meeting verifies, the item is a vote item, that vote answered it,
the vote verifies, **Bornite accepted it**, the vote really was through the channel
named, and the register carried has the fingerprint that was approved. Still nothing
about the company has changed.

```console
$ irena resolution execute --state r1.state …
executed at height 6
share-structure amendment: b988c265… (height 5, replacing fd171178…)
execution record: fb084c92…
```

Block 5 is the **amendment** — an ordinary share-register record, published through
exactly the same code path as one nobody voted on, superseding exactly the register the
voters saw. This is the moment the company changes. Block 6 is the **execution
record**, which does nothing except say: this amendment was authorised by that
resolution. The register now reads Alice 700, Bob 300.

### Blocks 7–8 — the chief executive decides alone

Chen, as `ceo`, appoints the auditors. No meeting: an individual channel decides by one
signature.

```console
$ irena decision new --subject "Appoint auditors" --proposal-digest aaaa… --state d1.state
$ irena decision freeze --state d1.state --channel ceo
frozen at height 6 through channel ceo: actor chen (key ca93ac17…)
$ irena decision sign --state d1.state --signing-key k4.key
$ irena decision finalize --state d1.state …          # block 7: 01c40b29…
```

Freezing resolves the channel exactly as a vote would — and refuses unless the channel
is individual and resolves to *exactly one* person with a registered key. Signing
refuses any key but Chen's. Finalising writes the decision to the chain (block 7,
`irena.decision.v1`), where it verifies on its own:

```console
$ irena decision verify --tx 01c40b29…
  ok   ChannelIsIndividual      channel ceo is individual
  ok   ActorResolves            chen with the frozen key
  ok   SignatureVerifies        signed by chen
VALID: 7 check(s) re-established from the chain
```

```console
$ irena resolution create --channel ceo --decision 01c40b29… \
      --title "Resolution 2: appoint auditors" --document-digest aaaa… --state r2.state
$ irena resolution finalize --state r2.state …        # block 8: 22f33988…
```

The resolution looks the same as the shareholders' did, except that its authority is a
decision rather than a meeting item and a vote. From here on the two are literally the
same code: both end at the same fact, *a channel approved this fingerprint*, and
everything after that — the fingerprint check, the amendment, the execution record, the
verification — does not ask which it was. This one is *declarative*: it records a
decision and changes nothing, so there is nothing to execute.

### Blocks 9–10 — the chief executive tries to shrink the board

Chen would like a board of one.

```console
$ irena decision freeze --state d2.state --channel ceo …
$ irena decision sign … ; irena decision finalize …    # block 9: c5d9e392…
$ irena resolution create --channel ceo --decision c5d9e392… \
      --target decision-channels --file thins-board.xml …
$ irena resolution finalize --state r3.state …         # block 10: 5a9ec237…
$ irena resolution execute --state r3.state …
error: channel ceo may not carry this amendment alone: board — a channel chen sits on — would change
```

The decision is genuine and the resolution is recorded — Chen really did sign this.
What is refused is *executing* it, by the **self-demotion rule**: a change to the list
of who decides, carried by one person alone, may not leave that person with a seat they
did not have, and may not change a seat they keep. Chen sits on the board; the board
would change; refused. The rule is two set comparisons and nothing more — no scoring,
no ranking of offices, no legal opinion — and it is honest about its limit: it stops
Chen promoting Chen. It would not stop Chen rewriting a committee Chen is not part of.
That is why every individual channel is marked in `irena channels`, and why the
notarisation on the channel-set record matters.

### Blocks 11–14 — the chief executive abolishes their own channel

```console
$ irena resolution create --channel ceo --decision cec4a9b0… \
      --target decision-channels --file ceo-abolished.xml …    # block 11 decision, 12 resolution
$ irena resolution execute --state r4.state …
executed at height 14
decision-channels amendment: d8f43dbf… (height 13, replacing fd171178…)
$ irena channels
2 channel(s)
  board          collective  roster          3 actor(s), total weight 4, 2 can sign
  shareholders   collective  share-register  2 actor(s), total weight 1000, 2 can sign
```

Writing yourself down is allowed. The channel set is amended (block 13,
`irena.channels.v1`) through the very same mechanism as the share register — an
ordinary amendment, superseding the founding record — and the execution record (block
14) links it back. The `ceo` channel is gone; nothing more can ever be decided through
it. The decision Chen signed *through* it, a block earlier, still verifies for ever: it
pinned the channel set it was taken under.

### Blocks 15–20 — the board creates an audit committee

```console
$ irena meeting new --channel board --title "Board meeting, June 2026" …
$ irena meeting add-item --title "Create an audit committee" --proposal-digest 2d848ada… …
$ irena meeting convene … ; irena meeting open …
  item 1: vote 1aa09107…
$ irena meeting ballot --voter chen --choice yes --signing-key k4.key …
$ irena meeting ballot --voter okafor --choice no --signing-key k5.key …
$ irena meeting close …
  item 1: accepted (yes 2 no 1 abstain 0)
```

The same `meeting` commands, `--channel board`. The electorate is the roster — Chen,
Okafor, Vance — at the roster's weights, so Chen's 2 carries the motion against
Okafor's 1. Vance, who registered no key, counts and cannot vote, exactly as Carol did
among the shareholders. A shareholder's ballot would be refused here: Alice is not on
the board.

```console
$ irena resolution create --channel board --meeting 1adaaf97… --item 1 --vote bffac498… \
      --target decision-channels --file audit-committee.xml …
$ irena resolution finalize … ; irena resolution execute …
$ irena channels
3 channel(s)
  audit-committee  collective  roster          3 actor(s), total weight 6, 3 can sign
  board            collective  roster          3 actor(s), total weight 4, 2 can sign
  shareholders     collective  share-register  2 actor(s), total weight 1000, 2 can sign
```

A committee with unequal weights and a two-thirds threshold now exists (blocks 18, 19,
20), and no committee type was added anywhere. The self-demotion rule did not apply —
the board decided *collectively* — and the verification says so rather than staying
silent about it.

### Blocks 21–24 — the chair's key is rotated

M. Chen's laptop is replaced, and with it the key they sign with. Nothing about the
board changes, nothing about the channels changes: one record says which key is Chen's
now, and Jane files it.

```console
$ irena publish-identities --file identities-rotate-chen.xml --signing-key k9.key \
      --supersedes a29e… --notary-id notary-07 …
published identities for acme at height 21 as 7c5e…

$ irena identities
8 person(s); 6 hold a key
  chen      M. Chen       ed4928c628d1c2c6…    may sign: nothing
  quinn     S. Quinn      1398f62c6d1a457c…    may sign: nothing
  …
```

The same record registers S. Quinn, who sits on the audit committee created three
blocks earlier and until now could be counted but could not sign. At the next board
meeting, the old key is simply nobody's:

```console
$ irena meeting cast --state board2.state --item 1 --ballot chen-old.ballot
error: item 1: ballot rejected: the signature from chen does not verify

$ irena meeting cast --state board2.state --item 1 --ballot chen-new.ballot
accepted a yes ballot from chen on item 1; 1 ballot(s) so far
```

One record, every channel. And the votes already on the chain are untouched: each of
them froze the identities record it was decided against, so the board's vote at block
16 still verifies today with the key Chen held then.

### Block 25 — a second secretary, and the one thing nobody may do

The company adds Okafor beside Jane as someone who may file amendments. That is an
authorisation amendment, and it is filed the ordinary way. The refusal worth seeing is
the other one — an authorisation that would leave nobody able to amend the company
again:

```console
$ irena publish-authorisation --file lockout.xml --signing-key k9.key --supersedes a29e… …
error: the record would lock the company out: no company signer holds a key; company
signers: carol (no key)
```

Carol is a real person in the company; she has never registered a key. A company whose
only authorised writer cannot sign is a company nobody can ever amend, so the record is
refused before it is written — and the same record forced onto the chain around Irena
stops reconstruction where it sits.

### The finished chain

```text
height  0   irena.company.v1      the company is founded: register and three channels
height  1   irena.meeting.v1      shareholders' meeting convened, agenda fixed
height  2   irena.vote.v1         the shareholders' vote: 800–0
height  3   irena.meeting.v1      the meeting's record, naming its vote
height  4   irena.resolution.v1   resolution 1, on the shareholders' vote
height  5   irena.shares.v1       AMENDMENT — Alice 700, Bob 300
height  6   irena.execution.v1    the link from the amendment back to resolution 1
height  7   irena.decision.v1     Chen's signed decision: appoint auditors
height  8   irena.resolution.v1   resolution 2, declarative, on that decision
height  9   irena.decision.v1     Chen's signed decision: a smaller board
height 10   irena.resolution.v1   resolution 3 — recorded; its execution REFUSED
height 11   irena.decision.v1     Chen's signed decision: abolish the ceo channel
height 12   irena.resolution.v1   resolution 4, on that decision
height 13   irena.channels.v1     AMENDMENT — two channels, no ceo
height 14   irena.execution.v1    the link back to resolution 4
height 15   irena.meeting.v1      board meeting convened
height 16   irena.vote.v1         the board's vote: 2–1
height 17   irena.meeting.v1      the board meeting's record
height 18   irena.resolution.v1   resolution 5, on the board's vote
height 19   irena.channels.v1     AMENDMENT — an audit committee
height 20   irena.execution.v1    the link back to resolution 5
height 21   irena.identities.v1   AMENDMENT — Chen's new key, and Quinn registered
height 22   irena.meeting.v1      board meeting convened
height 23   irena.vote.v1         the board's vote, signed with the new key
height 24   irena.meeting.v1      the board meeting's record
height 25   irena.authorisation.v1 AMENDMENT — a second secretary
```

Twenty-six blocks, five decisions, three channels, five amendments. Every step
separately checkable, and the blocks that changed the company are the plainest of them
all.

---

## Checking it, a year later

```console
$ irena resolution verify --execution 8b1ce256…
```

This needs nothing but the chain file and that one transaction id. No original files, no
trust in whoever ran the meeting or signed the decision, no access to anyone's key.

It works backwards through the whole story and says, in order, what it found. For the
execution that abolished the `ceo` channel:

| It checks | Which means, in plain terms |
|---|---|
| the execution record decodes | this really is an execution record |
| the resolution verifies | ...and all of its own checks below |
| — the decision verifies | a real, signed decision: the channel was individual at that height, it resolved to Chen with that key, and the signature is Chen's |
| — the channel matches | the decision really was through the channel the resolution names |
| — the proposal matches | the channel set in the resolution is the one Chen signed for |
| — the company matches | resolution, decision and chain are one company |
| — the heights are ordered | decision, then resolution |
| — the signer was authorised | the key that filed the resolution was, at that height, a person the company authorised for governance records |
| the resolution authorises this | it is an amendment resolution, for this part of the company |
| the amendment exists | the transaction named is a real channel-set record |
| the amendment matches the resolution | byte for byte the approved document |
| it replaced the approved base | it superseded exactly what Chen saw |
| **self-demotion holds** | *chen keeps 1 seat unchanged (board) and gains none; gives up ceo* |
| the amendment took effect | it is genuinely in the company's history, not orphaned |
| the heights are ordered | resolution, then amendment, then execution |
| the signer was authorised | the key that filed the execution was one the company authorised, at that height |
| executed once | no second execution of the same resolution |

For a resolution that rests on a vote, the decision check is replaced by five: the
meeting verifies, the item is a vote item, the vote answered that item, the vote
verifies (which includes that its channel was collective and resolved to exactly the
frozen electorate), and Bornite accepted it. The self-demotion line then reads *not
applicable: channel board decided collectively* — stated, not skipped.

All of them pass, and it says so. One fails, and it names which and why. A check that
could not run because an earlier one failed is left out rather than reported as passing.

---

## What the system refuses

Each of these is a way things go wrong in real companies.

**A motion that failed.** A rejected vote cannot become a resolution. *A rejected motion
authorises nothing.*

**The wrong vote, or the wrong channel.** A resolution naming a vote that answered a
different item, or a vote from a different meeting, is refused even though that vote is
valid in itself. So is a shareholders' vote presented as the board's, or a decision
presented as the shareholders'. Being a real vote is not the same as being *this* vote.

**A vote through a channel that decides alone, or a signature through one that votes.**
A meeting of the `ceo` channel cannot open: one person holds no vote. A decision through
the `board` cannot freeze: the board decides by voting. Both refusals name the channel.

**A channel with the wrong number of people.** An individual channel whose source
resolves to two people — a single-member company's `owner` channel the day a second
shareholder is admitted — stops resolving, and says how many it found.

**A different document.** A resolution carrying a document that is not the one voted on
or signed for is refused. One changed digit is enough.

**A company that moved underneath.** The actors approved replacing one specific record.
If somebody amends that record between the decision and the execution, executing would
install a change onto a company they never saw. Refused, and the decision must be taken
again. Two resolutions on one signed decision meet the same rule: the second is recorded
— the signature is genuine — and cannot execute. Strict on purpose; a resolution about
the channel set is unaffected by a change to the register, and vice versa.

**Promoting oneself.** A change to who decides, carried by one person alone, that gives
them a seat they did not have or changes a seat they keep. Refused at execution, named
in the refusal, and re-checked by anyone who verifies the chain later. The same rule
guards the two quieter routes to the same place: on one person's signature, only *their
own* entry in the identities may change — rewriting somebody else's key is voting as
that person, everywhere they sit — and their own right to file records may only shrink,
never grow. A sole director may hand back their key or their signing right; they may
not take anyone else's, or hand themselves one.

**A decision outside the channel's remit.** A chief executive whose channel is not
scoped to the register cannot rewrite it, however genuine the signature and however
willing the secretary. *Channel ceo may not amend the share-structure.* A channel that
names no scope at all may record decisions and change nothing.

**A key that may not write.** Alice owns half the company and cannot file an amendment;
Chen chairs the board and cannot file the record of the board's own vote. Who decides
and who writes are two lists, and the second one is checked before anything reaches the
chain — and again by every reader afterwards, so a record forced onto the ledger by an
unauthorised key stops reconstruction where it sits rather than quietly becoming part of
the company.

**Locking the company out.** No record may leave the company without at least one
authorised writer who holds a key. A company that cannot be amended by anybody is a
company nobody meant to create.

**Doing it twice.** A resolution executes once: the resolution knows it is spent, the
chain is checked for an existing execution, and the amendment itself would be refused
because the record it targets has already moved.

**Tampering after the fact.** Records cannot be edited, so the only attack is to publish
a *new* record that lies. Every such lie is caught by name — including an amendment to
the channel set written straight to the ledger with an execution record claiming Chen's
decision authorised it: *self-demotion* fails, because a reader re-applies the rule to
the chain as it was. A governance record filed by a key the company never authorised
fails its own named check, *signer authorised*, whoever else's signature it carries
inside.

---

## What it does not promise

An honest system is clear about its edges.

**That the register names the real owners, or that the channels are the ones the law
recognises.** Irena records what the notary attested to. If the register is wrong,
every derived vote is faithfully wrong; if the channel set gives a person power they
should not have, every decision through it is faithfully theirs. The chain proves
consistency with what was recorded, never correspondence with the world or the law.
**Notarisation is the trust boundary**, and it is where a real-world authority — the
articles, a court, a registrar — enters.

**That a lone actor cannot do harm outside their own seats.** The self-demotion rules
bound a signer's *own* reach. They do not stop an individual channel from rewriting a
channel its actor is not part of, registering a new person, or authorising somebody
else, and a change carried by a meeting is unrestricted. Saying what a channel may
decide — a *scope* — is the next planned step, and deliberately not a permissions
language.

**That the writers are honest.** A person authorised for company records can rewrite
the register with no channel deciding anything. That is the secretary's route, by
design: the alternative is a company that cannot correct a filing. What the system
gives you is not prevention but a list — who may do it, on the chain, amended like
everything else — and a record of every time they did.

**That a document says what you think.** A digest proves *which* document was meant. It
says nothing about what is written in it.

**That the right person held the key.** The company says which key is whose and which
of them may write; it cannot say whether the person holding that key is the person the
notary met. A stolen key is the identity, until an amendment says otherwise — which is
why rotation is one record and why every past decision keeps the key it froze. The
chain shows who signed and who notarised, and leaves that judgement to you.

**Secret ballots.** Ballots are in the record. That is what makes a meeting checkable
from the chain alone, and it is the wrong trade for some companies. It is not in V1.

**Anything about time.** Notaries write dates, and those dates are stored and shown —
but nothing is decided by them. The ledger's own order is the only order.

---

## Who does what

| Who | What they do | What they cannot do |
|---|---|---|
| **Company secretary** | The person the company authorises to write: convenes meetings, opens, collects ballots, closes, finalises, files resolutions, executions and amendments | Forge a ballot or a decision; change an agenda after convening; execute a resolution the channel did not authorise; take away the last authorised writer |
| **Shareholder, director, committee member** | An *actor* of a channel: signs their own ballot with the key their identity holds, or — as the sole actor of an individual channel — signs a decision; verifies afterwards that it counted | Vote twice; act without a registered key; act on a question they were not asked; act through a channel they are not an actor of; **put a record on the chain unless the company authorised them to** |
| **A person who decides alone** | Signs decisions through their individual channel; may give that channel up, rotate their own key, or drop their own right to file records | Widen their own reach by their own signature: no new seat, nobody else's key, no new right to file |
| **Notary** | Attests to each record — who they are, when, which external document backs it — including the record that says who decides | Change what a record says once it is on the chain |
| **Auditor** | Verifies any record — a vote, a decision, a meeting, a resolution, an execution — from the chain alone | Need anything but the chain file and a transaction id |

---

## The three layers, and why they are separate

| Layer | What it knows | What it refuses to know |
|---|---|---|
| **Prunella** | Ordering, immutability, signatures, hashes. A ledger | What any record means |
| **Bornite** | Voter ids, integer weights, three choices, and the arithmetic on them | That shares, seats, companies or keys exist |
| **Irena** | What a share is, what a channel is, what a meeting is, what authority means | Whether the register is true, or the channels lawful |

Bornite never learns that a share or a seat exists — Irena resolves a channel into ids
and weights before handing them over, and the same electorate type carries a
shareholding and a board seat. Prunella never learns what a company is. A test greps
every file in the two engines for the word "irena" and fails if it appears.

The point is not tidiness. It is that the voting arithmetic can be checked by someone
who knows nothing about companies, the ledger can be checked by someone who knows
nothing about voting, and a bug in either cannot become a bug in what the company is.

---

## Where to look next

| For | Read |
|---|---|
| The exact rules, every check, every record format | [IRENA_V1.md](IRENA_V1.md) — §1.3 channels, §7 votes, §8 decisions, §9 meetings, §10 resolutions |
| The documents in this walk-through, validated | [examples/](examples/README.md) |
| Every command, with real output | [docs/irena-cli.md](docs/irena-cli.md) |
| How the voting arithmetic is defined | [BORNITE_V1.md](BORNITE_V1.md) |
| How the ledger guarantees what it guarantees | [PROTOCOL_V1.md](PROTOCOL_V1.md) |
| The design choices and what they cost | [README.md](README.md) — the *Decisions* table |
