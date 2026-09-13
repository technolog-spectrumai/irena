# How a company decides, and how the chain proves it

This is the plain-language companion to [IRENA_V1.md](IRENA_V1.md). That document is
the specification — exact, normative, unforgiving. This one is the story it tells,
written for the people who will actually use the system: a shareholder who wants to
know their vote counted, a company secretary who runs the meeting, a notary who signs
off on the paperwork, an auditor asked to check the whole thing a year later, and a
developer meeting the codebase for the first time.

---

## The problem

A company changes. Shares move, rules are rewritten, decisions are taken. Every one of
those changes is supposed to rest on some authority — the shareholders approved it at a
meeting, the vote carried, the minutes say so.

In practice the authority and the change live in different places. The minutes sit in a
filing cabinet. The register sits in a spreadsheet. Somebody updated the spreadsheet,
and if you want to know *why*, you go and look for the minutes and hope they match. The
link between the decision and the change is a human promise.

Irena makes that link a matter of record. Not because anyone is assumed dishonest, but
because a year later nobody remembers, and ten years later nobody is left to ask.

---

## The one rule

**Nothing that decides may also change. Nothing that changes may decide.**

Everything else follows from that sentence.

A vote counts ballots. It cannot touch the share register — it has no way to, in the
code, not merely by convention. A resolution records that a decision was taken. It
cannot touch the register either. Only an *amendment* changes what the company is, and
an amendment is the same plain record it has always been: a new share register, signed,
notarised, replacing exactly one previous one.

What joins them is a chain of references, each pinned to an exact transaction:

```text
company state → meeting → vote → passed result → resolution → amendment → new company state
```

Read right to left, it answers "why is the register like this?" — because this
amendment; authorised by this resolution; which rests on this vote; which answered this
item; of this meeting. Every arrow is a separate record on the ledger, and every
reference is a transaction id, never a date, a name, or "the current register".

---

## Walking it through

Acme Industries has three shareholders. Alice holds 500 shares, Bob 300, Carol 200.
Carol wants out, and Alice will buy her. That needs a new share register, and a new
share register needs the shareholders' approval.

What follows is a real chain — the ids and heights below are from an actual run, not an
illustration.

### Block 0 — the company exists

```console
$ irena init --network acme-net --company acme --genesis company.xml --signing-key k.key \
      --notary-id notary-07 --notary-name "Jane Roe" --notary-at 2026-03-01T09:30:00Z
```

One document founds the company: who it is, who holds its shares (with the signing key
each shareholder will vote with), and the voting rules it decides under. That document
goes into block 0 as `irena.company.v1`, notarised by Jane Roe.

From here the company is never edited. It is *reconstructed* — read the genesis, apply
every amendment in order, and what you get is the company. Ask for height 3 and you get
the company as it was at height 3, regardless of what has happened since.

### Before the meeting — agreeing what "yes" means

The new register is written out as a file. Then:

```console
$ irena resolution digest --file new-register.xml
52a16af9…
```

This is the quiet hinge of the whole design. That digest is a fingerprint of the new
register — **the exact bytes**, not a description of them. Change one share count and
the fingerprint changes completely.

The agenda item will carry this fingerprint. So when the shareholders vote "yes", they
are not voting yes to *the idea* of buying Carol out. They are voting yes to this
register, these numbers, these holders. Later, when someone tries to execute the
decision, the only register that can be published is one whose fingerprint matches. A
different file — even a nearly identical one — is refused.

Shareholders vote on the thing itself. That is why no document can drift from what it
authorises.

### Block 1 — the meeting is convened

```console
$ irena meeting new --title "AGM 2026" --scheduled-at 2026-06-01T10:00:00Z --state m.state
$ irena meeting add-item --state m.state --title "Buy out carol" --proposal-digest 52a16af9…
$ irena meeting convene --state m.state --signing-key k.key …
convened for acme at height 1
meeting id: 3d5ebefa…
```

Convening puts the agenda on the chain. From that moment the agenda is fixed: items
cannot be added, removed or reworded. What the shareholders were called to decide is
what they will be asked to decide.

The meeting's identity *is* that transaction. There is no registry of meeting numbers
to keep, and nothing can later claim to be a meeting that was never convened.

### Opening — the electorate is frozen

```console
$ irena meeting open --state m.state
opened; 1 vote(s) frozen at height 1
  item 1: vote be4f2770…
```

Opening the meeting creates one vote per vote item, and each vote takes a snapshot of
the company as it stands: who holds shares, how many, which keys they sign with, and
what the voting rules say. That snapshot is the vote, for ever.

This matters more than it sounds. Suppose the register is amended while the meeting is
open — someone transfers shares the next morning. The vote does not notice. Bob, whose
shares moved after the freeze, still votes with the 300 he held when the meeting opened.
A shareholder who appeared afterwards cannot vote at all. Quorum is measured against
the electorate as it was.

That is not a limitation to work around. A vote is a decision by a specific group of
people at a specific moment, and the snapshot is what makes that literally true.

### The ballots

```console
$ irena meeting ballot --state m.state --item 1 --voter alice --choice yes \
      --signing-key alice.key --out alice.ballot
$ irena meeting cast --state m.state --item 1 --ballot alice.ballot
```

Alice signs her ballot on her own machine, with the key recorded in the share register.
The signature covers which vote, which voter, and which choice — so a ballot cannot be
moved to another item, another meeting, or another answer.

The ballot then goes to whoever is running the meeting, who casts it. They cannot forge
one: they do not have Alice's key. They cannot quietly drop one either, because the
final record commits to every ballot counted, and Alice can check hers is there.

Carol, in this company, holds shares but has registered no key. She counts towards
quorum — she owns the shares — but she cannot cast a ballot. The system says so plainly
rather than pretending otherwise.

Ballots are not secret here. That is a deliberate V1 choice, and the reason the whole
thing is checkable from the chain alone.

### Blocks 2 and 3 — the vote, then the meeting

```console
$ irena meeting close --state m.state
closed and counted
  item 1: accepted (yes 800 no 0 abstain 0)
$ irena meeting finalize --state m.state --signing-key k.key …
```

Closing hands the frozen electorate and the collected ballots to Bornite, the voting
engine. Bornite knows nothing about companies or shares — it sees voter ids, integer
weights and three choices, applies the rules, and returns a result with every
intermediate number shown. Given the same inputs it returns the same result on any
machine, for ever. It uses no floating point, no clock, and no randomness.

Finalising writes the vote's own record (block 2, `irena.vote.v1`) and then the
meeting's record (block 3, `irena.meeting.v1`). The vote record is self-contained: every
ballot, the frozen electorate, the result. Anyone can verify that one vote without
knowing a meeting existed.

**Nothing about the company has changed.** A motion carried. That is all.

### Block 4 — the resolution

```console
$ irena resolution create --meeting cd547983… --item 1 --vote 864aa72d… \
      --title "Resolution 1: buy out carol" \
      --target share-structure --file new-register.xml --state r.state
$ irena resolution finalize --state r.state --signing-key k.key …
recorded for acme at height 4
resolution: dc5a5ec4…
```

A resolution is the formal record that a decision was taken. It names three things,
each by transaction id: the meeting, the item on its agenda, and the vote that answered
that item. And it carries the new register itself, so an auditor reading the chain sees
what was decided without hunting for an attachment.

Before anything is written, the system checks all of it against the chain — not against
what the draft claims:

- the meeting verifies, every check, including its votes;
- item 1 exists on that meeting and is a vote item;
- that item was answered by exactly the vote named;
- that vote verifies, every check;
- **Bornite accepted it** — a rejected motion authorises nothing, and the attempt is
  refused with those words;
- the register the resolution carries has the fingerprint the agenda item committed to.

Only then does the resolution reach the chain. And still **nothing about the company has
changed**. `irena show` is identical before and after. A resolution describes authority;
it does not exercise it.

### Blocks 5 and 6 — the amendment, and the link

```console
$ irena resolution execute --state r.state --signing-key k.key …
executed at height 6
share-structure amendment: 73464c3a… (height 5, replacing 005e7492…)
execution record: c2ca894b…
```

Two transactions, in this order.

Block 5 is the **amendment** — an ordinary share-register record, exactly the kind that
existed before resolutions were invented, published through exactly the same code path,
superseding exactly the register the voters saw. This is the moment the company changes.
Someone who has never heard of resolutions still reconstructs the right company, because
the amendment is a normal amendment.

Block 6 is the **execution record**, which does nothing except say: this amendment was
authorised by that resolution, it replaced that register, and its fingerprint is the one
that was approved.

Now the register reads Alice 700, Bob 300. `irena history --kind share-structure` shows
the amendment sitting in the register's history like any other.

### The finished chain

```text
height 0   irena.company.v1      the company is founded
height 1   irena.meeting.v1      the meeting is convened, agenda fixed
height 2   irena.vote.v1         the vote: ballots, electorate, result
height 3   irena.meeting.v1      the meeting's record, naming its votes
height 4   irena.resolution.v1   the decision, and what it authorises
height 5   irena.shares.v1       THE AMENDMENT — the company changes here
height 6   irena.execution.v1    the link from the amendment back to its authority
```

Seven blocks. One decision. Every step separately checkable, and the one block that
changed the company is the plainest of them all.

---

## Checking it, a year later

```console
$ irena resolution verify --execution c2ca894b…
```

This needs nothing but the chain file and that one transaction id. No original files, no
trust in whoever ran the meeting, no access to anyone's key.

It works backwards through the whole story and says, in order, what it found:

| It checks | Which means, in plain terms |
|---|---|
| the execution record decodes | this really is an execution record |
| the resolution verifies | ...and all nine of its own checks below |
| — the meeting verifies | there was a real meeting, and its own record holds up |
| — the item is a vote item | the agenda really contained this question |
| — the vote answered that item | this is that question's vote, not another one |
| — the vote verifies | the ballots are signed by the registered holders, none added or dropped |
| — Bornite accepted it | the motion actually carried |
| — the proposal matches | the register in the resolution is the one they voted on |
| the resolution authorises this | it is an amendment resolution, for this part of the company |
| the amendment exists | the transaction named is a real share-register record |
| the amendment matches the resolution | byte for byte the approved register |
| it replaced the approved base | it superseded exactly what the voters saw |
| the amendment took effect | it is genuinely in the company's history, not orphaned |
| the heights are ordered | resolution, then amendment, then execution |
| executed once | no second execution of the same resolution |

All of them pass, and it says so:

> *the amendment is exactly what the shareholders authorised*

One fails, and it names which and why. Nothing is silently skipped, and a check that
could not run because an earlier one failed is left out rather than reported as passing.

---

## What the system refuses

These are the cases worth knowing about, because each one is a way things go wrong in
real companies.

**A motion that failed.** A rejected vote cannot become a resolution at all. The error
says so directly: *a rejected motion authorises nothing*.

**The wrong vote.** A resolution naming a vote that answered a different agenda item —
or a vote from a different meeting entirely — is refused, even though that vote is
perfectly valid in itself. Being a real vote is not the same as being *this* vote.

**A different document.** A resolution carrying a register that is not the one voted on
is refused. One changed digit is enough.

**A company that moved underneath.** The shareholders approved replacing one specific
register. If somebody amends the register between the vote and the execution, executing
would install a change onto a company the voters never saw. The system refuses, and says
the decision must go back to a meeting.

This is strict on purpose, and it has a cost worth stating: two resolutions from the
same meeting that both rewrite the register cannot both execute. The second was approved
against a register that no longer exists, so it needs re-approving. We preferred that to
the alternative, where a resolution quietly lands on something unfamiliar.

A resolution about the *rules* is unaffected by a change to the *register*, and vice
versa — each part is judged on its own.

**Doing it twice.** A resolution executes once. Three separate things prevent a second:
the resolution knows it is spent, the chain is checked for an existing execution, and
the amendment itself would be refused because the register it targets has already moved.

**Tampering after the fact.** Records cannot be edited — the ledger is append-only — so
the only attack is to publish a *new* record that lies. Every such lie is caught by name:
an execution pointing at another meeting's vote, an execution claiming a fingerprint
nobody approved, a second execution record for a resolution already carried out.

---

## What it does not promise

An honest system is clear about its edges.

**That the register names the real owners.** Irena records what the notary attested to.
If the register is wrong, every derived vote is faithfully wrong. The chain proves
consistency with what was recorded, never correspondence with the world.

**That a document says what you think.** A digest proves *which* document was meant. It
says nothing about what is written in it.

**That the right person acted.** V1 has **no authorisation roles**. Any key can publish
a resolution; the notarisation is the only authority, exactly as for company records.
The chain shows who signed and who notarised, and leaves the judgement to you. Adding
"who may do what" is a planned later stage.

**Secret ballots.** Ballots are in the record. That is what makes a meeting checkable
from the chain alone, and it is the wrong trade for some companies. It is not in V1.

**Anything about time.** Notaries write dates, and those dates are stored and shown —
but nothing is decided by them. The ledger's own order is the only order. A back-dated
notarisation changes the words in a record, never its place in the sequence.

---

## Who does what

| Who | What they do | What they cannot do |
|---|---|---|
| **Company secretary** | Drafts the agenda, convenes, opens, collects ballots, closes, finalises, drafts and executes resolutions | Forge a ballot; change an agenda after convening; execute a resolution the vote did not authorise |
| **Shareholder** | Signs their own ballot with their registered key; verifies afterwards that it was counted | Vote twice; vote without a registered key; vote on a question they were not asked |
| **Notary** | Attests to each record: who they are, when, and which external document backs it | Change what a record says once it is on the chain |
| **Auditor** | Verifies any record — a vote, a meeting, a resolution, an execution — from the chain alone | Need anything but the chain file and a transaction id |

---

## The three layers, and why they are separate

| Layer | What it knows | What it refuses to know |
|---|---|---|
| **Prunella** | Ordering, immutability, signatures, hashes. A ledger | What any record means |
| **Bornite** | Voter ids, integer weights, three choices, and the arithmetic on them | That shares, companies or keys exist |
| **Irena** | What a share is, what a meeting is, what authority means | Whether the register is true |

Bornite never learns that a share exists — Irena turns holders and share counts into
ids and weights before handing them over. Prunella never learns what a company is — it
stores opaque bytes and orders them. A test greps every file in the two engines for the
word "irena" and fails if it appears.

The point is not tidiness. It is that the voting arithmetic can be checked by someone
who knows nothing about companies, the ledger can be checked by someone who knows
nothing about voting, and a bug in either cannot become a bug in what the company is.

---

## Where to look next

| For | Read |
|---|---|
| The exact rules, every check, every record format | [IRENA_V1.md](IRENA_V1.md) — §7 votes, §8 meetings, §9 resolutions |
| Every command, with real output | [docs/irena-cli.md](docs/irena-cli.md) |
| How the voting arithmetic is defined | [BORNITE_V1.md](BORNITE_V1.md) |
| How the ledger guarantees what it guarantees | [PROTOCOL_V1.md](PROTOCOL_V1.md) |
| The design choices and what they cost | [README.md](README.md) — the *Decisions* table |
