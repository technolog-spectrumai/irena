# Building the interface, one circle at a time

The design is in [gui.md](gui.md). This is the order to build it in.

It is a **spiral, not a stack**. Each round is a complete, usable thing that handles a
narrower or wider *case*, rather than a horizontal layer that is useless until the next
one lands. Round one already gives somebody a reason to open the application. Every
round after that widens who can use it and what they can do, and every round leaves the
previous rounds working.

## How to use this document

- Boxes start empty. Tick one when the step is done and the thing it describes works.
- **Finish a round before starting the next.** A round is only finished when the "done
  when" line at its end is true for a real person, not when its boxes are ticked.
- Three standing rules apply to every round, and a round is not done if it breaks one:
  1. **Both languages.** Every round ends by translating what it added. The company's
     own words are never translated; see [gui.md](gui.md) §8.
  2. **Every refusal it can produce is worded.** In the company's vocabulary, naming the
     rule and offering the path, as in [gui.md](gui.md) §6.
  3. **Round one's promise still holds.** A stranger with a chain file and no key can
     still read and verify. Nothing added later may require an account, a key, or a
     server to check what happened.
- Rounds are numbered; steps inside them are `round.step`. Order within a round is the
  suggested order, not a hard dependency, except where a step says otherwise.

---

## Round 1 — One company, read once

**The case:** somebody has been handed a chain file and wants to know what it holds and
whether one particular record is genuine.

**Who can use it at the end:** the reader, with no key and no explanation.

This round deliberately starts with the hardest promise the system makes rather than the
easiest screen to build. If verification from the chain alone does not work, nothing
later is worth building.

- [ ] **1.1** Choose the shell and how it reads a chain, and prove it opens one. Wire
      both languages from the very first piece of text, so no round ever has to retrofit
      them. *This step contains the only technology decision in the plan and should be
      made explicitly rather than drifted into.*
- [ ] **1.2** Open a chain file. Refuse a file that is not a chain, and a chain that
      holds no company, each in a sentence rather than an error code.
- [ ] **1.3** The banner: registered name, label on the ledger, height being read, and
      whether the chain reconstructs. Present in every view from here on.
- [ ] **1.4** The **Company** view at the head: the five parts, each with the record that
      provides it and the notary who attested to it.
- [ ] **1.5** The **Record** view: the document exactly as filed, the notarisation, and
      every verification check in plain language, with checks that could not run shown
      as absent rather than passing.
- [ ] **1.6** The health strip: the chain reconstructs, somebody can still write, every
      channel resolves. Each opens onto its explanation. A break names the exact
      transaction and never offers to repair it.
- [ ] **1.7** Both languages for everything this round added.

**Done when:** a stranger can open a chain and answer *what is this company* and *does
this record hold up*, holding nothing but the file.

---

## Round 2 — The same company, through time and across references

**The case:** the reader now wants the whole picture, as it was at any moment, and wants
to follow why the company is the way it is.

**Who can use it at the end:** the auditor, completely. Their job is finished at the end
of this round.

- [ ] **2.1** The **as at** control. Moving it re-reads every view at once. Establish the
      rule now, while there is still nothing to act with: **read at any height, act only
      at the head**, said in a sentence rather than by disabling things silently.
- [ ] **2.2** The **Shares** view: holders, holdings, the weight each produces, and
      whether that person can sign. Lead with which channels, if any, may amend the
      register at all.
- [ ] **2.3** The **People** view: identities and authorisation in one table, one row per
      person, with the plain-language standing paragraph when a row is opened.
- [ ] **2.4** The **Channels** view: actors, mode, scope. Individual channels marked
      every time. A channel that does not resolve shown in full with the reason.
- [ ] **2.5** Read the channels down the column as well as across the row: for each part
      of the company, which channels may amend it.
- [ ] **2.6** The **History** view, filterable by part, with records that changed the
      company visually distinct from records that only decided something.
- [ ] **2.7** Follow the reference chain backwards from an amendment to the execution,
      the resolution, the vote, the item, the meeting, without typing anything.
- [ ] **2.8** Verify in bulk: check every record on a chain and report what was found.
- [ ] **2.9** Both languages for everything this round added.

**Done when:** an auditor can answer *why is the register like this* and *has anything on
this chain been tampered with*, and never needs the command line again.

---

## Round 3 — Know who you are

**The case:** somebody loads their key and wants to know what standing it gives them.

**Who can use it at the end:** everyone, but still only to read. Nothing is written or
signed in this round, which is what makes it safe to get wrong.

- [ ] **3.1** Load a key, and compute standing from the chain rather than from settings:
      which person holds it, which families they may sign, which channels they act in.
- [ ] **3.2** The standing statement, in one short paragraph, for each of the three
      cases: an authorised writer, a member of one or more channels, and a key the
      company does not know.
- [ ] **3.3** Shape the home view by standing: what you may do here, derived and shown
      before you go looking for it.
- [ ] **3.4** Put the key away without closing the chain, and return to reading.
- [ ] **3.5** Both languages for everything this round added.

**Done when:** any key, including a stranger's, produces an honest answer to *what can I
do here*, and the application has still never written anything.

---

## Round 4 — Sign something

**The case:** a member has been asked to vote and wants to do it from their own machine.

**Who can use it at the end:** the member, fully. First round in which a key is used to
sign, and still nothing reaches the chain.

- [ ] **4.1** Open a request to vote: which company, which meeting, which item, the
      document being decided, and what your vote weighs.
- [ ] **4.2** Be honest about the document: show it when it is there, and say plainly
      that only its digest is available when it is not.
- [ ] **4.3** Choose, sign with your own key, and produce the signed ballot to hand back.
- [ ] **4.4** The no-key case: you are in the electorate, your holding counts towards
      quorum, and you cannot cast a ballot until a key is registered for you.
- [ ] **4.5** The refusals a signer can meet: a ballot for another vote, a voter not in
      the frozen electorate, a voter who already cast one, a key that is not the one the
      company holds for that person.
- [ ] **4.6** Both languages for everything this round added.

**Done when:** a shareholder who opens this twice a year can vote without being taught
anything, and can later confirm from the chain that their ballot counted.

---

## Round 5 — Put one record on the chain

**The case:** the actor of an individual channel records a decision they took alone.

**Who can use it at the end:** the sole decider. First round that writes, and the
smallest complete lifecycle in the system: four steps, one record, one signature.

- [ ] **5.1** Choose the channel, and immediately show two things: that it resolves to
      exactly you, and what it may amend. Both are read from the company, not asserted.
- [ ] **5.2** Name what is being decided, attach the document, and freeze. Say what the
      freeze fixed and at which height, so it is clear what later amendments cannot
      reach.
- [ ] **5.3** Sign, and put the record on the chain.
- [ ] **5.4** Say plainly that a decision on its own changes nothing, and offer the next
      step rather than letting somebody believe they have finished.
- [ ] **5.5** The refusals: a channel that decides by vote, a channel resolving to more
      than one person, a sole actor with no registered key, and a decision outside what
      the channel may amend, shown while choosing rather than after signing.
- [ ] **5.6** The record written here verifies in the Record view from round 1, with no
      special handling.
- [ ] **5.7** Both languages for everything this round added.

**Done when:** a decision taken alone can be recorded end to end, and a reader who was
not there can check it from the chain.

---

## Round 6 — Run a meeting

**The case:** the secretary holds a meeting of a channel, with several people voting on
several items.

**Who can use it at the end:** the secretary, for everything except changing the company.
The longest flow in the system and the one where an interface earns its place over the
command line.

- [ ] **6.1** Draft the agenda: channel, title, date, items. For an item to be voted on,
      take the **document** and derive what it commits to, rather than asking anybody to
      copy a digest by hand. This single step removes the most error-prone act in the
      command line version.
- [ ] **6.2** Convene. The first point of no return: show the agenda as it will be
      recorded, capture the notarisation, and say that items cannot be added afterwards.
- [ ] **6.3** Open. Say what froze, at which height, and that amendments after this
      moment reach none of it.
- [ ] **6.4** The collection screen: per item, who has voted and how, who has not, who
      cannot, and where quorum stands. Accept a ballot however it arrives and say at once
      whether it was taken.
- [ ] **6.5** Close, and show each item's result with the reason it came out that way.
- [ ] **6.6** Finalise: each vote to the chain as its own record, then the meeting record
      naming them. Each vote must verify alone, without the meeting.
- [ ] **6.7** Resume a meeting left half-run, including one convened and never opened.
- [ ] **6.8** Both languages for everything this round added.

**Done when:** a real meeting can be run from agenda to final record, and a shareholder
who voted in it can verify their own ballot without being shown the meeting.

---

## Round 7 — Change the company

**The case:** something that was decided becomes something that happened.

**Who can use it at the end:** the secretary, completely, for the whole loop in
[governance.md](governance.md).

- [ ] **7.1** Choose the authority from real records on this chain: a meeting item with
      the vote that answered it, or a signed decision. Show what each decided. Offer a
      failed motion with its refusal attached rather than hiding it.
- [ ] **7.2** Choose what the resolution does: record a decision, or amend one part. Fire
      the scope check and the approved-document check here, before any work is done.
- [ ] **7.3** Finalise the resolution, and be clear that the company is still unchanged.
- [ ] **7.4** The execution preview, the most consequential screen in the application:
      the company now and the company afterwards, exactly which record is replaced, the
      self-demotion finding in words when one person decided alone, and the checks that
      will hold once it is done.
- [ ] **7.5** Execute: the amendment and its execution record together.
- [ ] **7.6** The refusals: a base that has moved since the vote, a resolution already
      executed, a vote that answered another item, a vote from another channel, and a
      document that is not the one approved.
- [ ] **7.7** Both languages for everything this round added.

**Done when:** the whole walk-through in [governance.md](governance.md) can be performed
in the interface, and the resulting chain verifies exactly as the drill's does.

---

## Round 8 — The secretary's own hand, and everything that guards it

**The case:** a record has to go on the chain without any channel deciding it, and every
remaining guard has to be met properly.

**Who can use it at the end:** the secretary and the notary.

- [ ] **8.1** File a record directly, with the honesty screen: no channel decided this,
      you are filing it because the company authorises you to, and anybody reading will
      see that.
- [ ] **8.2** Notarisation as a deliberate act rather than a form to hurry through.
- [ ] **8.3** Decide whether the notary reviews and attests as a separate step before the
      record is committed, which would add a hand-off. Listed as an open question in
      [gui.md](gui.md) §10 and settled here.
- [ ] **8.4** Complete the refusal catalogue: an unauthorised signer, an amendment that
      would leave nobody able to amend the company, an amendment against a record that
      has moved, and a chain that has been written around.
- [ ] **8.5** The **in flight** view: what is prepared and not yet on the chain, and what
      is part-done, each saying where it stopped and what comes next.
- [ ] **8.6** Both languages for everything this round added.

**Done when:** every refusal listed in [gui.md](gui.md) §6 has been met by a real person
in the interface and worded so they knew what to do next.

---

## Round 9 — Hand it to somebody who will never open it

**The case:** the output has to leave the application, for a lawyer, a registrar, a
court, or a shareholder with a printer.

- [ ] **9.1** Print or export a meeting's minutes, a register extract, and a verification
      report, each readable by somebody with no access to the chain.
- [ ] **9.2** The path in from outside: how a reader comes to hold a transaction id at
      all, and how they get from that to the verification view.
- [ ] **9.3** Review the whole vocabulary with somebody who practises law in Polish,
      against the rule that where Irena invented a concept the Polish must sound invented
      too.
- [ ] **9.4** Both languages for everything this round added.

**Done when:** somebody who has never heard of any of this can be handed a piece of paper
and check the thing it describes.

---

## Beyond the spiral

Not scheduled, and named so the spiral has an end rather than a fade.

- [ ] **More than one desk.** Everything above assumes one machine holds the chain and
      one person acts at a time. The three places that assumption will break are named in
      [gui.md](gui.md) §7: a meeting collected in two places at once, two people
      preparing amendments to the same part, and what the as-at control means while the
      head is moving underneath you.
- [ ] **Founding a company.** Deliberately left with the notary and the command line.
- [ ] **The live tally question.** Whether the chair sees the running result during
      collection. Worth deciding before round 6 is built, and recorded here because it is
      a policy decision rather than a piece of work.

---

## Why this order

The spiral is arranged so that each round widens the *case*, and so that risk is met
before investment.

| Round | Widening from | Widening to |
|---|---|---|
| 1 | nothing | one company, one record, one reader |
| 2 | one reading | every reading, at any height, with the references followed |
| 3 | anonymous | named, with standing computed from the chain |
| 4 | reading | signing, without touching the chain |
| 5 | signing | writing, one record, one person |
| 6 | one person | many people, many items, one meeting |
| 7 | deciding | changing the company |
| 8 | the channels' route | the secretary's own hand, and every guard |
| 9 | inside the application | out of it |

The two hardest promises are met first and last. **Verification from the chain alone** is
round one, because everything else is worthless if a reader cannot check it. **Leaving
the application** is round nine, because a system nobody outside can read is a system
that has not finished.
