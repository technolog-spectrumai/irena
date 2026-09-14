# What using this looks like

A functional design for the Irena interface, written from the point of view of the
people who sit in front of it. No screens are drawn here and no components are named.
What follows is what each person comes to do, what they see, what they can do next, and
what happens when the system says no.

Everything described here sits on top of what already exists: the company model in
[IRENA_V1.md](IRENA_V1.md), the decision loop walked through in
[governance.md](governance.md), and the commands in
[docs/irena-cli.md](docs/irena-cli.md). Nothing here asks the libraries for anything
they cannot already do. Where a limit shows through to the user, it is named rather
than hidden.

This is a first pass. The open questions at the end are the ones worth arguing about
next.

---

## 1. Who this is for

Five people, who between them do everything. They are not five accounts. Each is a
person the company already names in its own records, and what they may do is read off
the chain rather than granted by an administrator.

| | Who they are | Why they open it | How often |
|---|---|---|---|
| **The secretary** | The person the company authorises to write records | To run a meeting, file an amendment, record a resolution | Weekly at most, and always against a deadline |
| **The member** | A shareholder, a director, a committee member | To cast a ballot they were asked for, and to check it counted | A few times a year, from somewhere else, on their own machine |
| **The sole decider** | The one actor of an individual channel | To record a decision they took alone | Occasionally, and usually in a hurry |
| **The notary** | The person attesting that a record reflects the world | To read what is about to be filed and put their name to it | Rarely, and with the most at stake |
| **The reader** | An auditor, an investor's lawyer, a court, a curious shareholder | To find out whether a thing really happened | Once, years later, with no context |

Two consequences run through the whole design.

**Most people here are novices every time.** A shareholder who votes twice a year has
forgotten everything by the second time. Nothing may depend on remembering a previous
session.

**The reader has nothing.** No account, no explanation, no access to anyone. They have
a chain file and, at best, a transaction id. Everything they need must be derivable
from those two things, which is exactly what the verification layer already promises.

---

## 2. Four convictions

These are not interface preferences. They come from how the system works, and an
interface that contradicts any of them would be lying about what the user is doing.

### The company is a reading, not a database

There is no stored "current company". There is a chain, and a company reconstructed
from it at a chosen height. So the window carries a permanent **as at** control, and
moving it re-reads everything on screen at once: the register, the people, the
channels, all of it, as they stood then.

The rule that follows is strict and worth stating plainly to the user: **you may read
at any height, and you may only ever act at the head.** When the as-at control is
anywhere but the head, the window says so, and every verb is unavailable. Not greyed
out with a shrug, but with a sentence: *you are reading the company as it was at height
six; to act, return to the present.*

### Nothing is ever edited

There is no Save anywhere on company data. No field is editable in place. There is no
undo, because there is nothing to undo into.

The verbs are: **convene, cast, close, decide, resolve, execute, file.** Each produces a
record that joins the chain and stays there. The interface should feel less like a form
and more like a desk where you prepare a document, read it once more, and commit it.

This also means the interface never shows a half-changed company. A thing is either
prepared and not yet on the chain, or on the chain and permanent.

### A refusal is the main thing this does

The value of the system is that it refuses. A conventional application treats an error
as a failure of the user; here, the refusal *is* the feature, and the interface should
be proud of it rather than apologetic.

Three rules for every refusal:

- **Refuse early.** If the chair may not carry this amendment through this channel, say
  so while they are choosing the channel, not after they have held a meeting.
- **Name the rule in the company's own words.** Not "operation failed" but *channel ceo
  may not amend the share register; it may amend the channel set.*
- **Offer the legitimate path.** Whenever one exists: *the shareholders may. Would you
  like to put it to them?*

### Signing is personal

A key belongs to a person and never leaves their machine. The interface must never ask
for someone else's key, never offer to hold one, and never imply that the secretary can
vote on a member's behalf.

That makes the things passed between people into real objects in the interface: a
request to vote, a signed ballot coming back, a meeting half-run and picked up
tomorrow. They are not plumbing. They are the way a company with more than one person
in it actually works.

---

## 3. Opening a chain, and saying who you are

### The chain

You open a chain file the way you open a document. The application is a reader of that
file; it does not host the company, own it, or keep a copy of it anywhere else.

If the file is not a chain, say so plainly. If it is a chain with no company on it, say
that instead, because it is a real state and a confusing one: *this chain holds no
company. Nothing has founded one.*

Recently opened chains are offered on the way in. Nothing else is remembered.

### The banner

Across the top, always, and identical in every view:

- the company's registered name, and its label on the ledger
- the height being read, with the as-at control
- whether the chain reconstructs
- who you are acting as

### Who you are

You are not logged in. You declare yourself by loading your key, and the application
then reads off the chain what that key can do. If you load nothing, you are a reader,
which is a first-class way to use this rather than a degraded one.

This is worth dwelling on because it inverts the usual arrangement. Nobody grants you a
role in the application. The company's own identities record says which person holds
your key; its authorisation record says which families of record that person may sign;
its channels say which decisions that person takes part in. The interface computes what
you may do and shows it back to you.

So after loading a key, you get a short, plain statement of your standing. For a
shareholder:

> You are Alice Smith. You hold 500 of 1000 shares. You vote in **shareholders**. You
> may not put records on the chain.

For the secretary:

> You are Jane Roe. You hold no shares and sit on no channel. You may file **company**
> and **governance** records.

For someone whose key the company does not know:

> This key belongs to nobody in this company. You can read everything and do nothing.

That last one is not an error. It is the honest description of a stranger's key, and it
is exactly what an auditor holding their own key should see.

### Switching

You can put your key away without closing the chain, and the window returns to reading.
Any preparation you had underway is kept, because it is a document on your disk, not a
session.

---

## 4. The views you read

Six views. Each exists to answer one question, and each says which question that is.

### Company

*What is this company, right now?*

The landing view. The registered name, the jurisdiction, the registration number, and
then the five parts the company is made of: who it is, who holds its shares, who
decides, who its people are, and who may write. Each part shows the record that
currently provides it and the notary who attested to that record, because in this
system those two facts are inseparable from the content.

Under that, three statements of health, each either reassuring or alarming, and each
opening onto the explanation:

- **The chain reconstructs.** Every record links to the one it replaced, and nothing has
  been written around the system. If this fails, it names the exact transaction where
  the chain stops making sense, and it does not offer to fix it.
- **Somebody can still write.** At least one authorised writer holds a key. If this ever
  failed, the company could never be amended again, which is why the system refuses to
  let it happen; showing it is a reassurance, not a warning.
- **Every channel resolves.** Each channel still turns into a real set of people. A
  channel that has stopped resolving is the interesting case: an individual channel
  whose source now yields two people, for instance, the day a second shareholder was
  admitted. It is not broken, but nothing can be decided through it until it is amended.

From here you can go to any part, to the history, or to whatever you are entitled to do.

### Shares

*Who owns this company, and what is each holding worth in a vote?*

The register: each holder, their holding, the voting weight that holding produces, and
whether the person behind that id currently holds a key and could therefore sign a
ballot. A holder with no key is not an error; they own their shares and count towards
quorum and simply cannot vote. The view says that in words rather than leaving an empty
column.

One line at the top of this view carries unusual weight: **which channels, if any, may
amend this register.** For a company whose register is kept by an outside authority and
only mirrored here, the honest answer is *none, by anyone's decision*, and a reader
should learn that in the first three seconds rather than by failing to do something.

### People

*What can this person actually do?*

The identities and the authorisation, together, because separately neither answers the
question. One row per person: their id, their name, the document number the notary
recorded if there is one, whether they hold a key, which families of record they may
sign, and which channels they take part in.

Opening a person gives their full standing in one paragraph of plain language, the same
sentence structure the interface used to describe you when you loaded your key. It also
gives their history: when they were registered, when their key was rotated and by which
record, when their right to sign changed.

Two situations get called out rather than being left to inference. A person listed as an
authorised writer who holds no key can sign nothing, so the row says so. A person who
holds a key but is named nowhere else is registered and inert, which is often exactly
right and occasionally a mistake.

### Channels

*Who decides what, and how?*

The competence map, and for many readers the most interesting page in the application.

Each channel shows its name, where its people come from, how it reaches a decision, who
those people currently are with their weights, and **what it may amend**. A channel that
may amend nothing records decisions and changes no part of the company, which is a
legitimate and common configuration and should read as deliberate rather than empty.

Channels that decide by one signature are marked, every time, without apology. One
person deciding alone is a real arrangement that real companies choose, and the point is
that a reader can see it at a glance rather than deducing it.

A channel that does not resolve is shown in full with the reason, because the reason is
usually the interesting part.

The natural question from this view is comparative: *may the board change the register?*
So the view supports reading down a column as well as across a row: for each part of the
company, which channels may amend it. For many companies that column has exactly one
entry, and for the register it may have none.

### History

*Why is the company like this?*

The chain as a narrative, newest first, filterable to one part. Each entry says what
changed, who filed it, who attested it, and which record it replaced.

The entries that changed the company are visually distinct from the entries that
recorded a decision, because that distinction is the heart of the system: a vote counts
ballots and changes nothing; only an amendment changes what the company is.

From any amendment you can walk backwards through the reference chain: this amendment,
authorised by this resolution, resting on this vote, of this item, of this meeting, of
this channel. Each hop is a real reference on the chain rather than an inference, and
the interface should let you follow it without typing anything.

### Record

*What exactly does this record say, and does it hold up?*

One record in full. The document as it was filed, byte for byte, because that is what
was signed and what an auditor will want. The notarisation, prominently: who attested,
when they say it took effect, what external document they pointed at.

Then the verification: every check the system runs, in order, each either holding or
failing, each with a sentence explaining what it means in plain terms. A check that
could not run because an earlier one failed is absent rather than shown as passing,
which the view explains rather than leaving as a gap.

This view is the same for a reader with no key as for the secretary who filed it.

---

## 5. The things you do

Six flows. Each is described as a walk through it, with the moments that cannot be
taken back called out.

### Hold a meeting

The longest flow, and the one the secretary knows best.

**Draft the agenda.** Give the meeting a channel, a title and a date. Then add items.
An item is either something to vote on or something to put before the meeting for the
record. An item to vote on needs the document being decided, and the interface takes
the document itself and derives what it commits to, rather than asking anyone to copy a
digest by hand. This is the single most error-prone step in the command line version
and the one where an interface earns its place immediately.

Nothing is on the chain yet. The draft is a document on your disk and can be abandoned.

**Convene.** The first point of no return. Convening puts the agenda on the chain and
fixes it: from here the meeting decides what it was called to decide and nothing else.
Before committing, the interface shows the agenda as it will be recorded, asks for the
notarisation, and says plainly that items cannot be added afterwards.

**Open.** Each item to be voted on freezes its own picture of the company: who may vote,
what each vote weighs, under what rules. The interface says which height they froze at
and, importantly, that amendments after this moment reach none of them. From here the
meeting is insulated from the world.

**Collect.** The working screen, and where the secretary spends the meeting.

Per item: who has voted and how, who has not, and who cannot because they hold no key.
The tally as it stands. Whether quorum is met yet, and what is still needed.

Ballots arrive as files. The interface accepts one however it arrives and says
immediately whether it was accepted and why not if it was refused. Refusals here are
common and mostly innocent: a ballot for a different vote, a voter already counted, a
signature that does not match the key the company holds for that person.

**Close.** No more ballots. The result is computed from the frozen rules and shown per
item, with the reason: carried, or not carried, and on what basis.

**Finalise.** Each vote goes to the chain as its own record, then the meeting's own
record naming them. After this the meeting is a permanent, independently checkable
fact, and each vote inside it can be verified by someone who knows nothing about the
meeting.

At every step the interface can be closed and the meeting resumed later. A meeting
half-run is a document, not a session.

### Cast a ballot

The member's view, and deliberately the smallest thing in the application. Someone
who votes twice a year should be able to do it without learning anything.

You are handed a request to vote. Opening it shows: which company, which meeting, which
item, the document being decided, and what your vote weighs. If the document itself is
available, you read it here; if only its digest is, the interface says so honestly
rather than implying you have seen something you have not.

You choose. You sign with your own key. You hand the result back the way it came.

Two details matter more than they look. The interface should show **what your vote
weighs** at the moment of voting, because "my 500 shares of 1000" is the fact a
shareholder actually wants. And after the meeting is finalised, the same window should
be able to tell you **that your ballot counted**, verified from the chain, which is the
only reassurance worth giving.

If you hold no key, the window tells you that you are in the electorate, that your
holding counts towards whether the meeting was quorate, and that you cannot cast a
ballot until a key is registered for you.

### Decide alone

For the actor of an individual channel. Short, and heavily guarded.

You choose the channel, and the interface immediately shows two things: that the channel
resolves to exactly you, and **what it may amend**. If you are about to decide something
outside that, you learn it now, before writing anything.

You name what you are deciding and attach the document. You freeze, which fixes the
company you are deciding against. You sign. The record goes to the chain.

A decision on its own changes nothing. The interface says so and offers the next step,
which is turning it into a resolution, so that a user does not believe they have done
something they have not.

### Carry a resolution

Turning something that was decided into something that happened.

**Choose the authority.** Either a meeting item and the vote that answered it, or a
signed decision. The interface only offers real ones from this chain, and it says, for
each, what was decided and whether it carried. A motion that failed is offered with its
refusal attached rather than hidden, because a user looking for it deserves to be told
why it cannot be used.

**Choose what it does.** Either it records a decision and changes nothing, or it amends
one part of the company. If it amends, the interface checks straight away that this
channel may amend that part, and that what you are carrying is exactly what was
approved. Both refusals happen here, before any work.

**Finalise.** The resolution goes to the chain. At this point the company is unchanged
and the chain simply records that this authority approved this thing.

**Execute.** The most consequential screen in the application, and it should feel like
it. Before committing, it shows:

- the company as it is now, and the company as it will be
- exactly which record is being replaced, and that it is still the one the deciders saw
- for a decision taken alone, the self-demotion finding in plain words: what the signer
  keeps, what they give up, and that they gain nothing
- the checks that will hold afterwards, so the user knows what an auditor will see

Then the amendment and its execution record go to the chain together, and the company
is different from the next block onwards.

### File a record directly

The secretary's bare publication: a record put on the chain on the authority the company
gave them, with no channel deciding anything.

This is a real and necessary power. A registrar's filing has to be mirrored; a
misspelled name has to be corrected. It is also the largest piece of trust in the
system, so the interface neither hides it nor makes it feel illicit. It states it:

> No channel decided this. You are filing it because the company authorises you to.
> Anyone reading the chain will see your name against this record and no resolution
> behind it.

Then the ordinary preparation: the document, the notarisation, the before-and-after,
and the commit.

The refusals that guard this are the ones most likely to be met in practice, and each
is shown before anything is written: an amendment that no longer replaces the current
record, a key that may not sign this family of record, and an amendment that would leave
the company with nobody able to amend it ever again.

### Verify

For the reader who has a chain file and one transaction id, and nothing else.

Give it those two things and it says what holds, check by check, in plain language.
Where a check concerns another record, the reader can follow it: the execution rests on
this resolution, which rests on this vote, which was of this meeting.

It also runs in bulk. Point it at a chain and it will check every record on it and
report what it found, which is the audit a lawyer actually wants and the fastest way to
answer *has anything on this chain been tampered with?*

No key is needed. Nothing is uploaded. This should be the easiest thing in the whole
application to do, because it is the promise the system is built on.

---

## 6. How it says no

The refusals are the product. Every one below already exists in the system; this is
what the person on the other side should meet.

The pattern is the same throughout. **What happened, in the company's vocabulary. Why
the rule exists, in one line. What you can do instead, when there is something.**

| When you meet it | What it says | What it offers |
|---|---|---|
| Amending something that has since moved | This part is now provided by a later record than the one you are replacing | Reload and prepare again against what is there now |
| Signing with a key the company does not authorise | This key belongs to a named person, who may not sign this family of record; or it belongs to nobody here | Who may, so the user knows whom to ask |
| A record that would leave nobody able to amend the company | This would leave no authorised writer holding a key | The signers it counted and why each does not qualify |
| A channel amending a part it is not scoped to | This channel may not amend that part, and here is what it may amend | Which channels may, and an offer to put it to one of them |
| One person changing who decides, in their own favour | Naming the seat they would gain, or the seat of theirs that would change | The narrower version they could carry alone |
| One person rewriting another person's key | This would change somebody else's entry, which is signing as them | Their own entry is theirs to change |
| Executing what a channel approved against a company that has since changed | The deciders approved replacing a specific record, and something else provides it now | Decide it again against the company as it now stands |
| Executing twice | Where the first execution is, and at what height | A link to it |
| A resolution naming a vote that answered a different item | Both items, so the mistake is obvious | The vote that did answer it |
| A vote presented as the wrong channel's | Which channel it was really decided through | |
| Carrying a document that is not the one approved | The digest approved and the digest carried | |
| A motion that failed | It was rejected, and on what basis; a rejected motion authorises nothing | |
| A ballot from someone with no registered key | They are in the electorate and count towards quorum, and cannot sign | Registering a key is an identities amendment |
| An individual channel that no longer resolves to one person | How many people it now resolves to and who they are | Amending the channel |
| A vote through a channel that decides alone, or a signature through one that votes | Which the channel is | The right flow for it |
| A chain written around the system | The exact height and transaction where reconstruction stops, and what was wrong | Nothing. It is reported and never repaired, and the interface says so |

Two rules about tone. **Never blame the user for a refusal the system exists to
produce.** Someone meeting the scope rule has not made a mistake; they have met the
company's constitution. And **never offer a way around**. There is no override, no
force, no advanced mode. If the interface cannot do something, the honest response is to
name who can.

---

## 7. Keys, hand-off, and work left half done

### Keys

A key belongs to a person and stays on their machine. The application loads one to act
with and never stores, copies, backs up or transmits it.

Three things follow that the interface must be explicit about.

**It never asks for someone else's key.** There is no screen anywhere on which the
secretary could type a director's key, because there is no arrangement in which that
would be legitimate.

**Losing a key is not losing your standing.** Your id in the company is not your key.
A lost key is replaced by an identities amendment, and everything you decided before
keeps verifying with the key you held then. The interface should say this where someone
would panic, which is when they cannot find their key on the day of a meeting.

**A key alone is not authority.** Holding the key of an authorised writer lets you write;
holding a shareholder's key lets you vote. The interface shows which, plainly, at the
moment the key is loaded, so nobody discovers the difference at the end of a task.

### Hand-off

With one desk holding the chain, the things that move between people are documents,
and the interface treats them as such.

A request to vote goes out and a signed ballot comes back. Both are files. The
application produces them, accepts them however they arrive, and tells the sender
enough to identify what they are looking at. Whether they travel by email, by memory
stick, or across a table on a laptop is not the application's business, and it should
not pretend otherwise by implying delivery it has not made.

What the interface owes the secretary is a clear picture of what is outstanding: for
this item, these people have voted, these have not, and these cannot. Chasing is a
human activity; knowing whom to chase is the application's job.

### Work left half done

Every flow can be abandoned and resumed, because the state of an unfinished meeting,
vote, decision or resolution lives in a document rather than in a session.

So the application needs one more view than the six above: **what is in flight.** Things
prepared and not yet on the chain, and things partly on the chain and not finished. A
meeting convened but never opened is a real and slightly alarming state, and it should
be visible rather than discovered.

Each entry says where it stopped and what the next step is. Nothing expires on its own.

### One desk, and later more than one

Everything here assumes a single machine holds the chain, and one person acts on it at a
time. That assumption is doing a lot of work: it is why hand-off is files, why there is
no presence, and why nothing has to be reconciled.

When several instances need to hold the same company, three things in this design are
where the pressure will land, and they are named here so the later work knows where to
attach: **a meeting being collected in more than one place at once**, **two people
preparing amendments to the same part**, and **what the as-at control means when the
head is moving underneath you**. The first is a coordination problem, the second is
already answered by the rule that refuses a stale amendment, and the third is an
interface problem this document does not solve.

---

## 8. Two languages

The interface speaks English and Polish, and the distinction that matters is not which
words to use but **which words are the interface's to translate at all.**

**The company's own words are never translated.** The registered name, a person's name,
the notary's statement, the channel called `shareholders`, a document number: these are
data on the chain, stored and reproduced and never interpreted. Translating them in the
interface would show the user something the chain does not say. They appear exactly as
filed, in both languages.

**The interface's own vocabulary is translated**, and carefully, because some of it
carries legal weight and some of it deliberately does not.

| The system says | In Polish | Why it needs care |
|---|---|---|
| Company | Spółka | Safe |
| Share register | Rejestr akcjonariuszy | The term already means something precise, including that it is kept by an authorised entity. Using it commits us to that meaning |
| Shareholder | Akcjonariusz | Safe |
| Resolution | Uchwała | Safe, and load-bearing: this is the word a lawyer will look for |
| Meeting | Zgromadzenie | Qualifying it as *walne zgromadzenie* would claim more than a channel is |
| Notary, notarisation | Notariusz, poświadczenie | The notary is the trust boundary, so this must not soften |
| Decision channel | **Do not translate to an organ name** | There is no legal term for this, and reaching for *organ*, *zarząd* or *rada* would tell the reader the software knows something about company law that it does not |
| Scope | **Needs a chosen word, not a literal one** | This is what a channel may amend, which is close to *kompetencje* but is narrower and mechanical |
| Individual, collective | | These describe how a decision is reached, not who reached it, and the Polish must keep that distinction |
| Authorised writer | | No clean equivalent. Whatever is chosen must not imply representation of the company |

The rule behind the table: **where Irena invented a concept, the Polish must sound
invented too.** A channel is not an organ. A scope is not a competence in the legal
sense. If the translation makes the software sound like it understands Polish company
law, the translation is wrong, because the whole design rests on it not understanding
any company law at all.

Dates, numbers and identifiers are shown in the local convention, with one exception:
the notary's own date-time is reproduced exactly as recorded, because it is attested
text rather than a timestamp.

---

## 9. What it deliberately does not do

**It does not edit.** No field on any company part is editable. If the interface ever
gains something that feels like editing, the design has gone wrong.

**It does not repair.** When a chain has been written around, the application reports
exactly where and stops. It never offers to skip a record, rebuild an index, or
reconcile anything, because a reader who trusts a repaired chain has been misled.

**It does not hold keys.** No escrow, no backup, no "remember me", no key generation on
behalf of somebody who is not present.

**It does not do secret ballots.** Ballots are in the record; that is what makes a
meeting checkable from the chain alone. An interface that implied privacy would be
lying.

**It does not give legal advice.** It never says a configuration is valid, compliant, or
sufficient. It says what the company recorded and what follows mechanically from it.

**It does not confirm what the system already refuses.** No "are you sure" in front of
an action that would be refused anyway. Confirmation is reserved for things that are
permitted and permanent, which is a much shorter list and therefore one people will read.

**It does not found companies.** A genesis is prepared with a notary. This application
opens a company that already exists.

---

## 10. Open questions

Things this pass deliberately leaves open.

**The live tally during collection.** Showing the running result lets a chair see how a
vote is going before closing it. Ballots are public in this system anyway, so nothing is
concealed by hiding it, and nothing is leaked by showing it. But it does hand the person
who closes the meeting a small piece of timing power. Worth a decision rather than a
default.

**How much of the reading views a member should see.** A shareholder opening their
ballot could be shown the whole company, or only the item in front of them. The first is
more honest, the second is less overwhelming for someone who does this twice a year.

**Whether the notary needs their own flow at all.** Today the notarisation is captured
as part of filing, by whoever is filing. A notary reviewing and attesting separately,
before the record is committed, is a different and arguably more truthful arrangement,
and it would add a hand-off.

**What the in-flight view does about abandonment.** Nothing expires, which is correct
and also means a list that fills up with things nobody will finish.

**Printing.** A meeting's minutes, a register extract, a verification report as something
that can be handed to somebody who will never open this application. Probably necessary,
entirely undesigned here.

**Where the reader arrives from.** A reader with a transaction id has to have got it
from somewhere. A link, a reference on a filing, a QR code on a printed resolution: the
path into verification from the outside world is not thought through.

---

## Where to look next

- [governance.md](governance.md) for what the company does and why, in the same plain
  language, walked through a real chain
- [IRENA_V1.md](IRENA_V1.md) for the exact rules the interface is dressing
- [docs/irena-cli.md](docs/irena-cli.md) for what every one of these flows looks like
  today at a command line
