//! A vote from draft to a verified record on the chain, and everything that must be
//! refused along the way.

use bornite_core::{BallotSetV1, BallotV1, VoterIdV1};
use irena_core::{
    ChannelIdV1, CompanyIdV1, NotarisationV1, NotaryIdV1, NotaryTimeV1, RecordKindV1,
};
use irena_ledger::{company_now, genesis_with_company, publish, reconstruct};
use irena_vote::{
    BallotChoiceV1, BallotRejectionV1, COMMITMENT_TAGS, CheckNameV1, FinalVoteRecordV1,
    SignedBallotV1, VoteError, VoteStatusV1, VoteV1, actors_of_register, ballot_commitment, verify,
};
use prunella_canonical::Canonical;
use prunella_core::merkle::{InclusionProof, TreeTags};
use prunella_core::{BlockHeight, Hash, Namespace, NetworkId, SchemaVersion, TransactionDraft};
use prunella_crypto::SigningKey;
use prunella_store::LocalChainStore;
use tempfile::TempDir;

/// The whole company: alice (500 shares), bob (300), carol (200, no key), three
/// channels — the shareholders under RULES, a board, a ceo — the one key table, and
/// jane as the only person who may sign records.
fn genesis_xml() -> String {
    format!(
        "<company-genesis><identity name=\"Acme Industries Ltd\"/>{}{}<governance>{}</governance>{AUTHORISATION}</company-genesis>",
        register(),
        identities(),
        channels(RULES)
    )
}

/// The one key table: alice 1, bob 2, the three directors 5/6/7, jane 9. Carol is
/// listed with no key; dave is not listed at all.
fn identities() -> String {
    format!(
        r#"<identities>
  <person id="alice" key="{}"/>
  <person id="bob" key="{}"/>
  <person id="carol" name="Carol White"/>
  <person id="chair" key="{}"/>
  <person id="dir-a" key="{}"/>
  <person id="dir-b" key="{}"/>
  <person id="jane" name="Jane Roe" key="{}"/>
</identities>"#,
        key(1).public_key(),
        key(2).public_key(),
        key(5).public_key(),
        key(6).public_key(),
        key(7).public_key(),
        key(9).public_key()
    )
}

/// Carol registers key 3 and dave, the new holder, registers key 4.
fn identities_amended() -> String {
    format!(
        r#"<identities>
  <person id="alice" key="{}"/>
  <person id="bob" key="{}"/>
  <person id="carol" name="Carol White" key="{}"/>
  <person id="chair" key="{}"/>
  <person id="dave" key="{}"/>
  <person id="dir-a" key="{}"/>
  <person id="dir-b" key="{}"/>
  <person id="jane" name="Jane Roe" key="{}"/>
</identities>"#,
        key(1).public_key(),
        key(2).public_key(),
        key(3).public_key(),
        key(5).public_key(),
        key(4).public_key(),
        key(6).public_key(),
        key(7).public_key(),
        key(9).public_key()
    )
}

/// Jane, the company secretary, is the only signer: company and governance alike.
const AUTHORISATION: &str = r#"<authorisation>
  <signer person="jane" records="company"/>
  <signer person="jane" records="governance"/>
</authorisation>"#;

/// The channel set: shareholders (share register, collective, `rules`); board (chair
/// weight 2, dir-a, dir-b; collective, simple majority, no quorum); ceo (chair,
/// individual). Keys are nowhere here: they live in the identities.
fn channels(rules: &str) -> String {
    format!(
        r#"<decision-channels>
  <channel id="shareholders" mode="collective">
    <actors source="share-register"/>
    <scope>
      <amend part="identity"/>
      <amend part="share-structure"/>
      <amend part="decision-channels"/>
      <amend part="identities"/>
      <amend part="authorisation"/>
    </scope>
    {rules}
  </channel>
  <channel id="board" mode="collective">
    <actors source="roster">
      <member id="chair" weight="2"/>
      <member id="dir-a"/>
      <member id="dir-b"/>
    </actors>
    <scope>
      <amend part="identity"/>
      <amend part="share-structure"/>
      <amend part="decision-channels"/>
      <amend part="identities"/>
      <amend part="authorisation"/>
    </scope>
    <voting-rules version="1.0">
      <weight type="electorate"/>
      <exclusions enabled="false"/>
      <quorum type="none"/>
      <threshold type="simple-majority" basis="votes-cast"/>
      <abstentions treatment="exclude"/>
      <tie treatment="reject"/>
    </voting-rules>
  </channel>
  <channel id="ceo" mode="individual">
    <actors source="roster">
      <member id="chair"/>
    </actors>
    <scope>
      <amend part="identity"/>
      <amend part="share-structure"/>
      <amend part="decision-channels"/>
      <amend part="identities"/>
      <amend part="authorisation"/>
    </scope>
  </channel>
</decision-channels>"#
    )
}

fn channel(id: &str) -> ChannelIdV1 {
    ChannelIdV1::new(id).expect("channel id")
}

fn shareholders() -> ChannelIdV1 {
    channel("shareholders")
}

const RULES: &str = r#"<voting-rules version="1.0">
  <weight type="electorate"/>
  <exclusions enabled="true"/>
  <quorum type="fraction" numerator="1" denominator="2" basis="total-electorate"/>
  <threshold type="simple-majority" basis="votes-cast"/>
  <abstentions treatment="exclude"/>
  <tie treatment="reject"/>
</voting-rules>"#;

/// Two-thirds supermajority: the amendment used to show a freeze holds.
const RULES_STRICT: &str = r#"<voting-rules version="1.0">
  <weight type="electorate"/>
  <exclusions enabled="true"/>
  <quorum type="none"/>
  <threshold type="fraction" numerator="2" denominator="3" basis="votes-cast"/>
  <abstentions treatment="exclude"/>
  <tie treatment="reject"/>
</voting-rules>"#;

fn key(seed: u8) -> SigningKey {
    SigningKey::from_seed([seed; 32])
}

fn acme() -> CompanyIdV1 {
    CompanyIdV1::new("acme").expect("company")
}

fn voter(id: &str) -> VoterIdV1 {
    VoterIdV1::new(id).expect("voter")
}

fn notary(at: &str) -> NotarisationV1 {
    NotarisationV1 {
        id: NotaryIdV1::new("notary-07").expect("id"),
        name: "Jane Roe".to_owned(),
        address: None,
        at: NotaryTimeV1::parse(at).expect("time"),
        statement: None,
        source_digest: None,
    }
}

/// alice 500, bob 300, carol 200.
fn register() -> String {
    r#"<share-structure>
  <holder id="alice" shares="500"/>
  <holder id="bob" shares="300"/>
  <holder id="carol" shares="200"/>
</share-structure>"#
        .to_owned()
}

/// bob sold half to dave.
fn register_amended() -> String {
    r#"<share-structure>
  <holder id="alice" shares="500"/>
  <holder id="bob" shares="150"/>
  <holder id="carol" shares="200"/>
  <holder id="dave" shares="150"/>
</share-structure>"#
        .to_owned()
}

struct Chain {
    _dir: TempDir,
    store: LocalChainStore,
}

/// The company founded at height 0 with its register and rules.
fn founded() -> Chain {
    let dir = TempDir::new().expect("temp dir");
    let spec = genesis_with_company(
        NetworkId::new("acme-net").expect("n"),
        &key(9),
        &acme(),
        &genesis_xml(),
        &notary("2026-01-10T09:00:00Z"),
        0,
    )
    .expect("spec");
    let store = LocalChainStore::init_genesis(dir.path().join("acme.chain"), spec).expect("create");
    Chain { _dir: dir, store }
}

/// Publishes an amendment superseding whatever currently provides the part.
fn amend(chain: &Chain, kind: RecordKindV1, body: &str, at: &str, ts: u64) {
    let current = company_now(&chain.store).expect("state").provider_of(kind);
    publish(
        &chain.store,
        &key(9),
        kind,
        body,
        Some(current),
        &notary(at),
        ts,
    )
    .expect("publish");
}

/// The height every test freezes at: the founding block.
const FOUNDED: BlockHeight = BlockHeight::GENESIS;

fn proposal() -> Hash {
    Hash::from_bytes([0xd0; 32])
}

/// A vote frozen at the founding height and open.
fn open_vote(chain: &Chain) -> VoteV1 {
    let mut vote = VoteV1::draft("Approve the 2026 accounts", proposal());
    vote.freeze(&chain.store, FOUNDED, &shareholders())
        .expect("freeze");
    vote.open().expect("open");
    vote
}

fn ballot(vote: &VoteV1, seed: u8, id: &str, choice: BallotChoiceV1) -> SignedBallotV1 {
    SignedBallotV1::sign(&key(seed), vote.id().expect("frozen"), &voter(id), choice)
}

// ---------------------------------------------------------------------------------
// Lifecycle.
// ---------------------------------------------------------------------------------

#[test]
fn a_vote_runs_from_draft_to_a_verified_record() {
    let chain = founded();
    let mut vote = VoteV1::draft("Approve the 2026 accounts", proposal());
    assert_eq!(vote.status(), VoteStatusV1::Draft);
    assert!(vote.id().is_none());

    let snapshot = vote
        .freeze(&chain.store, FOUNDED, &shareholders())
        .expect("freeze")
        .clone();
    assert_eq!(vote.status(), VoteStatusV1::Frozen);
    assert_eq!(snapshot.height, FOUNDED);
    assert_eq!(snapshot.electorate.len(), 3);
    assert_eq!(snapshot.electorate[0].id, "alice");
    assert_eq!(snapshot.electorate[0].weight, 500);
    assert_eq!(snapshot.electorate[2].key, None);
    assert_eq!(
        snapshot.shares_tx_id,
        reconstruct(&chain.store, FOUNDED).unwrap().shares.tx_id
    );
    assert_eq!(
        snapshot.channels_tx_id,
        reconstruct(&chain.store, FOUNDED).unwrap().channels.tx_id
    );
    assert_eq!(snapshot.channel, "shareholders");

    vote.open().expect("open");
    vote.cast(ballot(&vote, 1, "alice", BallotChoiceV1::Yes))
        .expect("alice");
    vote.cast(ballot(&vote, 2, "bob", BallotChoiceV1::No))
        .expect("bob");
    vote.close().expect("close");
    assert_eq!(vote.status(), VoteStatusV1::Closed);

    let evaluation = vote.evaluate(&chain.store).expect("evaluate");
    assert!(evaluation.accepted(), "{evaluation:?}");
    assert_eq!(evaluation.tally.yes_weight.value(), 500);
    assert_eq!(evaluation.tally.no_weight.value(), 300);
    assert_eq!(vote.status(), VoteStatusV1::Evaluated);
    assert!(vote.evaluation().unwrap().accepted());

    let finalized = vote
        .finalize(&chain.store, &key(9), 3000)
        .expect("finalize");
    assert_eq!(vote.status(), VoteStatusV1::Finalized);
    assert_eq!(finalized.height, BlockHeight(1));
    assert_eq!(finalized.record.ballots.len(), 2);
    assert_eq!(vote.finalized(), Some((finalized.tx_id, BlockHeight(1))));

    // The record is on the chain under the vote namespace, as canonical bytes.
    let committed = chain
        .store
        .get_transaction(&finalized.tx_id)
        .unwrap()
        .unwrap();
    assert_eq!(committed.transaction.namespace.as_str(), "irena.vote.v1");
    assert_eq!(
        committed.transaction.payload,
        finalized.record.canonical_bytes()
    );

    // And it verifies from nothing but the chain and the id.
    let report = verify(&chain.store, &finalized.tx_id).expect("verify");
    assert!(report.is_valid(), "{report:#?}");
    assert_eq!(report.checks.len(), 11);
    assert_eq!(report.record.as_ref().unwrap(), &finalized.record);
}

#[test]
fn every_invalid_transition_is_reported_with_both_ends() {
    let chain = founded();
    let mut vote = VoteV1::draft("x", proposal());
    let transition = |result: Result<(), VoteError>, from: VoteStatusV1| match result
        .expect_err("invalid transition")
    {
        VoteError::InvalidTransition { from: f, to } => {
            assert_eq!(f, from);
            assert!(!to.is_empty());
        }
        other => panic!("wrong error: {other}"),
    };

    // Draft: only freeze.
    transition(vote.open(), VoteStatusV1::Draft);
    transition(vote.close(), VoteStatusV1::Draft);
    transition(vote.evaluate(&chain.store).map(|_| ()), VoteStatusV1::Draft);
    transition(
        vote.finalize(&chain.store, &key(9), 1).map(|_| ()),
        VoteStatusV1::Draft,
    );
    transition(vote.final_record().map(|_| ()), VoteStatusV1::Draft);

    vote.freeze(&chain.store, FOUNDED, &shareholders())
        .expect("freeze");
    // Frozen: only open.
    transition(
        vote.freeze(&chain.store, FOUNDED, &shareholders())
            .map(|_| ()),
        VoteStatusV1::Frozen,
    );
    transition(
        vote.cast(ballot(&vote, 1, "alice", BallotChoiceV1::Yes)),
        VoteStatusV1::Frozen,
    );
    transition(vote.close(), VoteStatusV1::Frozen);
    transition(
        vote.evaluate(&chain.store).map(|_| ()),
        VoteStatusV1::Frozen,
    );

    vote.open().expect("open");
    // Open: cast or close.
    transition(vote.open(), VoteStatusV1::Open);
    transition(vote.evaluate(&chain.store).map(|_| ()), VoteStatusV1::Open);
    transition(
        vote.finalize(&chain.store, &key(9), 1).map(|_| ()),
        VoteStatusV1::Open,
    );

    vote.close().expect("close");
    // Closed: only evaluate.
    transition(
        vote.cast(ballot(&vote, 1, "alice", BallotChoiceV1::Yes)),
        VoteStatusV1::Closed,
    );
    transition(vote.open(), VoteStatusV1::Closed);
    transition(
        vote.finalize(&chain.store, &key(9), 1).map(|_| ()),
        VoteStatusV1::Closed,
    );

    vote.evaluate(&chain.store).expect("evaluate");
    // Evaluated: only finalize.
    transition(
        vote.evaluate(&chain.store).map(|_| ()),
        VoteStatusV1::Evaluated,
    );
    transition(
        vote.cast(ballot(&vote, 1, "alice", BallotChoiceV1::Yes)),
        VoteStatusV1::Evaluated,
    );
    assert!(vote.final_record().is_ok());

    vote.finalize(&chain.store, &key(9), 3000)
        .expect("finalize");
    // Finalized: nothing.
    transition(
        vote.finalize(&chain.store, &key(9), 1).map(|_| ()),
        VoteStatusV1::Finalized,
    );
    transition(
        vote.evaluate(&chain.store).map(|_| ()),
        VoteStatusV1::Finalized,
    );
    transition(vote.open(), VoteStatusV1::Finalized);
    assert!(vote.final_record().is_ok());
    let error = vote
        .freeze(&chain.store, FOUNDED, &shareholders())
        .map(|_| ())
        .expect_err("frozen for ever");
    assert_eq!(error.to_string(), "cannot freeze a vote that is finalized");
}

#[test]
fn a_vote_needs_a_company_on_the_chain() {
    let dir = TempDir::new().expect("dir");
    let store = LocalChainStore::init_genesis(
        dir.path().join("plain.chain"),
        prunella_core::GenesisSpec::new(NetworkId::new("plain").expect("n")),
    )
    .expect("create");
    let mut vote = VoteV1::draft("x", proposal());
    let error = vote
        .freeze(&store, BlockHeight::GENESIS, &shareholders())
        .map(|_| ())
        .expect_err("no company");
    assert!(
        matches!(
            error,
            VoteError::Ledger(irena_ledger::LedgerError::NoCompany { .. })
        ),
        "{error}"
    );
    assert_eq!(
        vote.status(),
        VoteStatusV1::Draft,
        "a failed freeze leaves a draft"
    );
    assert!(vote.company().is_empty());

    // Once frozen, the vote knows its company from the chain.
    let chain = founded();
    let frozen = open_vote(&chain);
    assert_eq!(frozen.company(), "acme");
    assert_eq!(frozen.snapshot().unwrap().company, "acme");
}

#[test]
fn the_vote_state_round_trips_through_canonical_bytes_between_steps() {
    let chain = founded();
    let mut vote = VoteV1::draft("x", proposal());
    let reload =
        |vote: &VoteV1| VoteV1::from_canonical_bytes(&vote.canonical_bytes()).expect("decode");
    assert_eq!(reload(&vote), vote);
    vote.freeze(&chain.store, FOUNDED, &shareholders())
        .expect("freeze");
    let mut vote = reload(&vote);
    vote.open().expect("open");
    vote.cast(ballot(&vote, 1, "alice", BallotChoiceV1::Yes))
        .expect("alice");
    let mut vote = reload(&vote);
    assert_eq!(vote.ballots().count(), 1);
    vote.close().expect("close");
    vote.evaluate(&chain.store).expect("evaluate");
    let mut vote = reload(&vote);
    let finalized = vote
        .finalize(&chain.store, &key(9), 3000)
        .expect("finalize");
    assert_eq!(reload(&vote), vote);
    assert!(verify(&chain.store, &finalized.tx_id).unwrap().is_valid());
}

// ---------------------------------------------------------------------------------
// The freeze holds.
// ---------------------------------------------------------------------------------

#[test]
fn amendments_after_freezing_change_nothing() {
    let chain = founded();
    let mut vote = open_vote(&chain);
    let frozen_id = vote.id().unwrap();

    // The register and the rules both change mid-vote.
    amend(
        &chain,
        RecordKindV1::ShareStructure,
        &register_amended(),
        "2026-02-01T10:00:00Z",
        1000,
    );
    amend(
        &chain,
        RecordKindV1::DecisionChannels,
        &channels(RULES_STRICT),
        "2026-02-01T10:05:00Z",
        2000,
    );
    amend(
        &chain,
        RecordKindV1::Identities,
        &identities_amended(),
        "2026-02-01T10:10:00Z",
        3000,
    );

    // dave, now a holder, is not in this vote; carol, now with a key, still cannot vote.
    let dave = SignedBallotV1::sign(&key(4), frozen_id, &voter("dave"), BallotChoiceV1::Yes);
    assert!(matches!(
        vote.cast(dave),
        Err(VoteError::Ballot(BallotRejectionV1::NotInElectorate { .. }))
    ));
    let carol = SignedBallotV1::sign(&key(3), frozen_id, &voter("carol"), BallotChoiceV1::Yes);
    assert!(matches!(
        vote.cast(carol),
        Err(VoteError::Ballot(BallotRejectionV1::NoKey { .. }))
    ));

    // alice yes 500, bob no 300: a simple majority passes; two thirds would not.
    vote.cast(ballot(&vote, 1, "alice", BallotChoiceV1::Yes))
        .expect("alice");
    vote.cast(ballot(&vote, 2, "bob", BallotChoiceV1::No))
        .expect("bob");
    vote.close().expect("close");
    let evaluation = vote.evaluate(&chain.store).expect("evaluate");
    assert!(
        evaluation.accepted(),
        "the frozen rules decide: {evaluation:?}"
    );
    assert_eq!(evaluation.electorate.total_weight.value(), 1000);
    assert_eq!(vote.id(), Some(frozen_id));

    let finalized = vote
        .finalize(&chain.store, &key(9), 5000)
        .expect("finalize");
    assert_eq!(finalized.record.snapshot.height, FOUNDED);
    let report = verify(&chain.store, &finalized.tx_id).expect("verify");
    assert!(
        report.is_valid(),
        "verification re-resolves at the snapshot height: {report:#?}"
    );

    // A vote frozen now sees the amended company and the stricter rules.
    let mut later = VoteV1::draft("x", proposal());
    later
        .freeze(&chain.store, BlockHeight(4), &shareholders())
        .expect("freeze");
    assert_ne!(later.id(), Some(frozen_id));
    later.open().unwrap();
    later
        .cast(ballot(&later, 1, "alice", BallotChoiceV1::Yes))
        .unwrap();
    later
        .cast(ballot(&later, 2, "bob", BallotChoiceV1::No))
        .unwrap();
    later
        .cast(ballot(&later, 3, "carol", BallotChoiceV1::No))
        .unwrap();
    later.close().unwrap();
    assert!(
        !later.evaluate(&chain.store).unwrap().accepted(),
        "500 of 1000 is not two thirds"
    );
}

#[test]
fn the_same_inputs_freeze_to_the_same_vote_on_another_machine() {
    let a = founded();
    let b = founded();
    let va = open_vote(&a);
    let vb = open_vote(&b);
    assert_eq!(va.id(), vb.id());
    assert_eq!(va.snapshot(), vb.snapshot());
    // A different proposal is a different vote.
    let mut other = VoteV1::draft("Approve the 2026 accounts", Hash::from_bytes([0xd1; 32]));
    other.freeze(&a.store, FOUNDED, &shareholders()).unwrap();
    assert_ne!(other.id(), va.id());
}

// ---------------------------------------------------------------------------------
// Ballots.
// ---------------------------------------------------------------------------------

#[test]
fn each_ballot_check_is_named() {
    let chain = founded();
    let mut vote = open_vote(&chain);
    let id = vote.id().unwrap();
    let rejection = |error: VoteError| match error {
        VoteError::Ballot(rejection) => rejection,
        other => panic!("{other}"),
    };

    // Wrong vote.
    let other_id = irena_vote::VoteIdV1::from_hash(Hash::from_bytes([0xee; 32]));
    let wrong = SignedBallotV1::sign(&key(1), other_id, &voter("alice"), BallotChoiceV1::Yes);
    assert!(matches!(
        rejection(vote.cast(wrong).unwrap_err()),
        BallotRejectionV1::WrongVote { .. }
    ));

    // Not in the electorate.
    let stranger = SignedBallotV1::sign(&key(5), id, &voter("eve"), BallotChoiceV1::Yes);
    assert!(matches!(
        rejection(vote.cast(stranger).unwrap_err()),
        BallotRejectionV1::NotInElectorate { .. }
    ));

    // In the electorate, no key.
    let keyless = SignedBallotV1::sign(&key(3), id, &voter("carol"), BallotChoiceV1::Yes);
    assert!(matches!(
        rejection(vote.cast(keyless).unwrap_err()),
        BallotRejectionV1::NoKey { .. }
    ));

    // Wrong key for the voter.
    let forged = SignedBallotV1::sign(&key(2), id, &voter("alice"), BallotChoiceV1::Yes);
    assert!(matches!(
        rejection(vote.cast(forged).unwrap_err()),
        BallotRejectionV1::BadSignature { .. }
    ));

    // Right key, tampered choice.
    let mut tampered = ballot(&vote, 1, "alice", BallotChoiceV1::Yes);
    tampered.body.choice = BallotChoiceV1::No;
    assert!(matches!(
        rejection(vote.cast(tampered).unwrap_err()),
        BallotRejectionV1::BadSignature { .. }
    ));

    // Malformed voter id inside a ballot body.
    let mut malformed = ballot(&vote, 1, "alice", BallotChoiceV1::Yes);
    malformed.body.voter = "al ice".to_owned();
    assert!(matches!(
        rejection(vote.cast(malformed).unwrap_err()),
        BallotRejectionV1::InvalidVoter { .. }
    ));

    // The good one, then again.
    vote.cast(ballot(&vote, 1, "alice", BallotChoiceV1::Yes))
        .expect("alice");
    let again = ballot(&vote, 1, "alice", BallotChoiceV1::No);
    assert!(matches!(
        rejection(vote.cast(again).unwrap_err()),
        BallotRejectionV1::AlreadyVoted { .. }
    ));
    assert_eq!(vote.ballots().count(), 1);
    assert_eq!(
        vote.ballots().next().unwrap().body.choice,
        BallotChoiceV1::Yes,
        "the first ballot stands"
    );
}

#[test]
fn a_ballot_from_one_vote_cannot_be_replayed_in_another() {
    let chain = founded();
    let first = open_vote(&chain);
    let mut second = VoteV1::draft("Something else", Hash::from_bytes([0x02; 32]));
    second
        .freeze(&chain.store, FOUNDED, &shareholders())
        .unwrap();
    second.open().unwrap();
    let replay = ballot(&first, 1, "alice", BallotChoiceV1::Yes);
    assert!(matches!(
        second.cast(replay),
        Err(VoteError::Ballot(BallotRejectionV1::WrongVote { .. }))
    ));
}

#[test]
fn the_commitment_is_order_independent_and_proves_inclusion() {
    let chain = founded();
    let mut forward = open_vote(&chain);
    let mut backward = open_vote(&chain);
    let alice = ballot(&forward, 1, "alice", BallotChoiceV1::Yes);
    let bob = ballot(&forward, 2, "bob", BallotChoiceV1::Abstain);
    forward.cast(alice.clone()).unwrap();
    forward.cast(bob.clone()).unwrap();
    backward.cast(bob.clone()).unwrap();
    backward.cast(alice.clone()).unwrap();
    for vote in [&mut forward, &mut backward] {
        vote.close().unwrap();
        vote.evaluate(&chain.store).unwrap();
    }
    let a = forward.final_record().unwrap();
    let b = backward.final_record().unwrap();
    assert_eq!(a, b);
    assert_eq!(a.canonical_bytes(), b.canonical_bytes());
    assert_eq!(
        a.ballot_commitment,
        ballot_commitment(&[alice.clone(), bob.clone()])
    );
    assert_ne!(
        a.ballot_commitment,
        ballot_commitment(&[bob.clone(), alice.clone()]),
        "order is part of the tree"
    );
    assert_ne!(
        ballot_commitment(&[]),
        ballot_commitment(std::slice::from_ref(&alice))
    );

    // bob can prove his ballot is in the record with Prunella's own proof machinery.
    let leaves: Vec<Hash> = a.ballots.iter().map(SignedBallotV1::digest).collect();
    let proof = InclusionProof::generate_with(COMMITMENT_TAGS, &leaves, 1).expect("proof");
    assert!(
        proof
            .verify_with(COMMITMENT_TAGS, &bob.digest(), 2, &a.ballot_commitment)
            .is_ok()
    );
    assert!(
        proof
            .verify_with(COMMITMENT_TAGS, &alice.digest(), 2, &a.ballot_commitment)
            .is_err()
    );
    // And a ballot tree can never pass as a block tree.
    assert!(
        proof
            .verify_with(
                TreeTags::PRUNELLA_V1,
                &bob.digest(),
                2,
                &a.ballot_commitment
            )
            .is_err()
    );
    assert_ne!(
        prunella_core::merkle::root(TreeTags::PRUNELLA_V1, &leaves),
        a.ballot_commitment
    );
}

#[test]
fn evaluation_matches_bornite_run_on_the_same_inputs_directly() {
    let chain = founded();
    let mut vote = open_vote(&chain);
    vote.cast(ballot(&vote, 1, "alice", BallotChoiceV1::Abstain))
        .unwrap();
    vote.cast(ballot(&vote, 2, "bob", BallotChoiceV1::Yes))
        .unwrap();
    vote.close().unwrap();
    let via_vote = vote.evaluate(&chain.store).unwrap();

    let rules = bornite_xml::read_rules_document(RULES).unwrap();
    let register = irena_core::read_share_structure_document(&register()).unwrap();
    let identities = irena_core::read_identities_document(&identities()).unwrap();
    let electorate = actors_of_register(&register, &identities)
        .unwrap()
        .electorate;
    let ballots = BallotSetV1::new(vec![
        BallotV1 {
            voter: voter("bob"),
            choice: bornite_core::ChoiceV1::Yes,
        },
        BallotV1 {
            voter: voter("alice"),
            choice: bornite_core::ChoiceV1::Abstain,
        },
    ])
    .unwrap();
    let direct = bornite_eval::evaluate(&rules, &electorate, &ballots).unwrap();
    assert_eq!(via_vote, direct);
    assert_eq!(
        irena_vote::EvaluationSummaryV1::of(&direct),
        *vote.evaluation().unwrap()
    );
    // Quorum: 800 of 1000 participated against a 1/2 requirement; bob's 300 is all the
    // votes cast that count, so it passes.
    assert!(direct.accepted());
    assert_eq!(vote.evaluation().unwrap().quorum_required_weight, Some(500));
}

#[test]
fn a_vote_with_no_ballots_is_still_a_vote() {
    let chain = founded();
    let mut vote = open_vote(&chain);
    vote.close().unwrap();
    let evaluation = vote.evaluate(&chain.store).unwrap();
    assert!(!evaluation.accepted());
    assert_eq!(evaluation.reason, bornite_eval::ReasonCodeV1::QuorumNotMet);
    let finalized = vote.finalize(&chain.store, &key(9), 3000).unwrap();
    assert!(finalized.record.ballots.is_empty());
    assert!(verify(&chain.store, &finalized.tx_id).unwrap().is_valid());
}

// ---------------------------------------------------------------------------------
// Tampering with the record.
// ---------------------------------------------------------------------------------

/// Appends a block holding one transaction, bypassing every Irena check. Signed by
/// jane's key, so a planted vote record is judged on its content, not its signer.
fn append_raw(store: &LocalChainStore, namespace: &str, payload: Vec<u8>) -> prunella_core::TxId {
    append_raw_signed(store, 9, namespace, payload)
}

/// As [`append_raw`], signed by the key of the given seed.
fn append_raw_signed(
    store: &LocalChainStore,
    seed: u8,
    namespace: &str,
    payload: Vec<u8>,
) -> prunella_core::TxId {
    let signer = key(seed);
    let head = store.head().unwrap();
    let parent = store.get_block(head.height).unwrap().unwrap();
    let transaction = signer.sign_transaction(TransactionDraft {
        namespace: Namespace::new(namespace).unwrap(),
        schema_version: SchemaVersion(1),
        payload,
        signer: signer.public_key(),
        nonce: head.height.value() + 100,
    });
    let id = transaction.id;
    let block = parent
        .header
        .child_draft(vec![transaction], 9999)
        .unwrap()
        .build()
        .unwrap();
    store.append_block(block).unwrap();
    id
}

/// A finalised record, ready to be tampered with.
fn finalized_record(chain: &Chain) -> FinalVoteRecordV1 {
    let mut vote = open_vote(chain);
    vote.cast(ballot(&vote, 1, "alice", BallotChoiceV1::Yes))
        .unwrap();
    vote.cast(ballot(&vote, 2, "bob", BallotChoiceV1::No))
        .unwrap();
    vote.close().unwrap();
    vote.evaluate(&chain.store).unwrap();
    vote.final_record().unwrap()
}

#[test]
fn every_tampered_field_is_caught_by_a_named_check() {
    let chain = founded();
    let good = finalized_record(&chain);
    let key_id = |record: &FinalVoteRecordV1| record.canonical_bytes();

    type Edit = fn(&mut FinalVoteRecordV1);
    let edits: Vec<(&str, Edit, CheckNameV1)> = vec![
        (
            "vote id",
            |r| r.vote_id = irena_vote::VoteIdV1::from_hash(Hash::from_bytes([1; 32])),
            CheckNameV1::VoteIdDerives,
        ),
        (
            "snapshot subject",
            |r| {
                r.snapshot.subject.push('!');
            },
            CheckNameV1::VoteIdDerives,
        ),
        (
            "snapshot height",
            |r| {
                r.snapshot.height = BlockHeight(1);
            },
            CheckNameV1::VoteIdDerives,
        ),
        (
            "pinned register",
            |r| {
                r.snapshot.shares_tx_id = prunella_core::TxId::from_hash(Hash::from_bytes([2; 32]));
            },
            CheckNameV1::VoteIdDerives,
        ),
        (
            "electorate weight",
            |r| {
                r.snapshot.electorate[0].weight = 5000;
            },
            CheckNameV1::VoteIdDerives,
        ),
        (
            "ballot choice",
            |r| {
                r.ballots[0].body.choice = BallotChoiceV1::No;
            },
            CheckNameV1::BallotsVerify,
        ),
        (
            "ballot signature",
            |r| {
                r.ballots[0].signature = prunella_core::Signature::from_bytes([7; 64]);
            },
            CheckNameV1::BallotsVerify,
        ),
        (
            "ballot order",
            |r| {
                r.ballots.swap(0, 1);
            },
            CheckNameV1::BallotsOrdered,
        ),
        (
            "duplicated ballot",
            |r| {
                let b = r.ballots[0].clone();
                r.ballots.push(b);
            },
            CheckNameV1::BallotsOrdered,
        ),
        (
            "dropped ballot",
            |r| {
                r.ballots.pop();
            },
            CheckNameV1::CommitmentDerives,
        ),
        (
            "commitment",
            |r| {
                r.ballot_commitment = Hash::from_bytes([3; 32]);
            },
            CheckNameV1::CommitmentDerives,
        ),
        (
            "outcome",
            |r| {
                r.evaluation.outcome = irena_vote::EvaluationSummaryV1::of(
                    &bornite_eval::evaluate(
                        &bornite_xml::read_rules_document(RULES).unwrap(),
                        &actors_of_register(
                            &irena_core::read_share_structure_document(&register()).unwrap(),
                            &irena_core::read_identities_document(&identities()).unwrap(),
                        )
                        .unwrap()
                        .electorate,
                        &BallotSetV1::new(vec![]).unwrap(),
                    )
                    .unwrap(),
                )
                .outcome;
            },
            CheckNameV1::ResultReproduces,
        ),
        (
            "yes weight",
            |r| {
                r.evaluation.yes_weight += 1;
            },
            CheckNameV1::ResultReproduces,
        ),
        (
            "reason",
            |r| {
                r.evaluation.reason = "tie_accepted".to_owned();
            },
            CheckNameV1::ResultReproduces,
        ),
    ];

    for (label, edit, expected) in edits {
        let mut tampered = good.clone();
        edit(&mut tampered);
        assert_ne!(
            key_id(&tampered),
            key_id(&good),
            "{label}: the edit must change the bytes"
        );
        let tx_id = append_raw(&chain.store, "irena.vote.v1", tampered.canonical_bytes());
        let report = verify(&chain.store, &tx_id).expect("verify runs");
        assert!(!report.is_valid(), "{label} must fail");
        let failed: Vec<CheckNameV1> = report.failures().map(|c| c.name).collect();
        assert!(
            failed.contains(&expected),
            "{label}: expected {expected:?} among {failed:?}\n{report:#?}"
        );
        assert!(
            report
                .checks
                .iter()
                .any(|c| c.name == CheckNameV1::Decodes && c.passed),
            "{label}: still decodes"
        );
    }

    // The untampered record, appended the same way, verifies.
    let tx_id = append_raw(&chain.store, "irena.vote.v1", good.canonical_bytes());
    assert!(verify(&chain.store, &tx_id).unwrap().is_valid());
}

#[test]
fn a_record_that_never_matched_the_chain_fails_however_well_formed() {
    let chain = founded();
    let mut good = finalized_record(&chain);
    // Claim the register in force at height 2 was some other transaction, and make the
    // vote id agree, so only the chain can tell.
    good.snapshot.shares_tx_id = prunella_core::TxId::from_hash(Hash::from_bytes([0x42; 32]));
    good.vote_id = good.snapshot.id();
    // Ballots were signed for the old id, so re-sign them for the new one.
    let alice = SignedBallotV1::sign(&key(1), good.vote_id, &voter("alice"), BallotChoiceV1::Yes);
    let bob = SignedBallotV1::sign(&key(2), good.vote_id, &voter("bob"), BallotChoiceV1::No);
    good.ballots = vec![alice, bob];
    good.ballot_commitment = ballot_commitment(&good.ballots);

    let tx_id = append_raw(&chain.store, "irena.vote.v1", good.canonical_bytes());
    let report = verify(&chain.store, &tx_id).unwrap();
    assert!(!report.is_valid());
    let failed: Vec<CheckNameV1> = report.failures().map(|c| c.name).collect();
    assert_eq!(failed, [CheckNameV1::RecordsResolve], "{report:#?}");
    assert!(
        report
            .checks
            .iter()
            .any(|c| c.name == CheckNameV1::VoteIdDerives && c.passed)
    );
}

#[test]
fn non_records_are_reported_not_mistaken_for_votes() {
    let chain = founded();
    let garbage = append_raw(&chain.store, "irena.vote.v1", b"not a record".to_vec());
    let report = verify(&chain.store, &garbage).unwrap();
    assert!(!report.is_valid());
    assert_eq!(report.checks.len(), 1);
    assert_eq!(report.checks[0].name, CheckNameV1::Decodes);
    assert!(report.record.is_none());

    let elsewhere = append_raw(
        &chain.store,
        "app.other",
        finalized_record(&chain).canonical_bytes(),
    );
    let report = verify(&chain.store, &elsewhere).unwrap();
    assert_eq!(report.checks[0].name, CheckNameV1::Decodes);
    assert!(!report.checks[0].passed);
    assert!(report.checks[0].detail.contains("app.other"));

    let missing = prunella_core::TxId::from_hash(Hash::from_bytes([0x99; 32]));
    assert!(matches!(
        verify(&chain.store, &missing),
        Err(VoteError::NoSuchTransaction { .. })
    ));

    // A record with trailing bytes is not canonical.
    let mut bytes = finalized_record(&chain).canonical_bytes();
    bytes.push(0);
    let trailing = append_raw(&chain.store, "irena.vote.v1", bytes);
    let report = verify(&chain.store, &trailing).unwrap();
    assert!(!report.checks[0].passed, "{report:#?}");
}

#[test]
fn a_snapshot_from_the_future_is_caught() {
    let chain = founded();
    let good = finalized_record(&chain);
    // Append the record, then a record claiming a snapshot height at or past itself.
    let mut future = good.clone();
    future.snapshot.height = BlockHeight(3);
    future.vote_id = future.snapshot.id();
    let alice = SignedBallotV1::sign(
        &key(1),
        future.vote_id,
        &voter("alice"),
        BallotChoiceV1::Yes,
    );
    let bob = SignedBallotV1::sign(&key(2), future.vote_id, &voter("bob"), BallotChoiceV1::No);
    future.ballots = vec![alice, bob];
    future.ballot_commitment = ballot_commitment(&future.ballots);
    let tx_id = append_raw(&chain.store, "irena.vote.v1", future.canonical_bytes());
    let report = verify(&chain.store, &tx_id).unwrap();
    let failed: Vec<CheckNameV1> = report.failures().map(|c| c.name).collect();
    assert!(
        failed.contains(&CheckNameV1::SnapshotPrecedesRecord),
        "{report:#?}"
    );
}

#[test]
fn only_a_governance_signer_may_put_a_vote_on_the_chain() {
    let chain = founded();
    let mut vote = open_vote(&chain);
    vote.cast(ballot(&vote, 1, "alice", BallotChoiceV1::Yes))
        .unwrap();
    vote.close().unwrap();
    vote.evaluate(&chain.store).unwrap();
    // Alice voted and holds half the company; writing the record is another matter.
    let error = vote
        .finalize(&chain.store, &key(1), 3000)
        .expect_err("alice writes");
    assert!(
        matches!(
            error,
            VoteError::Ledger(irena_ledger::LedgerError::UnauthorisedSigner { .. })
        ),
        "{error}"
    );
    assert_eq!(chain.store.head().unwrap().height, BlockHeight::GENESIS);
    let finalized = vote.finalize(&chain.store, &key(9), 3000).expect("jane");
    assert!(verify(&chain.store, &finalized.tx_id).unwrap().is_valid());

    // The same record planted by an unauthorised key fails one named check.
    let planted = append_raw_signed(
        &chain.store,
        1,
        "irena.vote.v1",
        finalized.record.canonical_bytes(),
    );
    let failed: Vec<CheckNameV1> = verify(&chain.store, &planted)
        .unwrap()
        .failures()
        .map(|c| c.name)
        .collect();
    assert_eq!(failed, [CheckNameV1::SignerAuthorised]);
}

#[test]
fn a_broken_chain_stops_evaluation_rather_than_guessing() {
    let chain = founded();
    let mut vote = open_vote(&chain);
    vote.cast(ballot(&vote, 1, "alice", BallotChoiceV1::Yes))
        .unwrap();
    vote.close().unwrap();
    // Write a channel-set record around Irena that supersedes nothing; the snapshot
    // is at the founding height, so the pinned channel set still resolves — the freeze
    // holds even against a later break.
    let payload = irena_core::compose_record(
        RecordKindV1::DecisionChannels,
        &acme(),
        None,
        &notary("2026-05-01T00:00:00Z"),
        &channels(RULES_STRICT),
    )
    .unwrap();
    append_raw(&chain.store, "irena.channels.v1", payload.into_bytes());
    assert!(
        vote.evaluate(&chain.store).is_ok(),
        "evaluation re-resolves at the snapshot height, which the break is after"
    );
    // Writing, though, needs the company at the head: who may sign a record is the
    // authorisation in force now, and on a broken chain there is no now.
    let error = vote
        .finalize(&chain.store, &key(9), 10_000)
        .expect_err("broken head");
    assert!(
        matches!(
            error,
            VoteError::Ledger(irena_ledger::LedgerError::BrokenAmendmentChain { .. })
        ),
        "{error}"
    );

    // And a vote frozen after the break cannot exist: the company does not resolve.
    let mut later = VoteV1::draft("x", proposal());
    assert!(matches!(
        later.freeze(&chain.store, BlockHeight(2), &shareholders()),
        Err(VoteError::Ledger(
            irena_ledger::LedgerError::BrokenAmendmentChain { .. }
        ))
    ));
}

// ---------------------------------------------------------------------------------
// Channels: the same vote through a roster, and not through an individual.
// ---------------------------------------------------------------------------------

#[test]
fn a_board_vote_is_the_same_vote_with_a_roster() {
    let chain = founded();
    let mut vote = VoteV1::draft("Approve the budget", proposal());
    let snapshot = vote
        .freeze(&chain.store, FOUNDED, &channel("board"))
        .expect("freeze")
        .clone();
    assert_eq!(snapshot.channel, "board");
    let weights: Vec<(&str, u64)> = snapshot
        .electorate
        .iter()
        .map(|e| (e.id.as_str(), e.weight))
        .collect();
    assert_eq!(
        weights,
        [("chair", 2), ("dir-a", 1), ("dir-b", 1)],
        "the roster's declared weights, nothing about shares"
    );

    vote.open().unwrap();
    // A shareholder is not on the board.
    let alice = SignedBallotV1::sign(
        &key(1),
        vote.id().unwrap(),
        &voter("alice"),
        BallotChoiceV1::Yes,
    );
    assert!(matches!(
        vote.cast(alice),
        Err(VoteError::Ballot(BallotRejectionV1::NotInElectorate { .. }))
    ));
    vote.cast(ballot(&vote, 5, "chair", BallotChoiceV1::Yes))
        .expect("chair");
    vote.cast(ballot(&vote, 6, "dir-a", BallotChoiceV1::No))
        .expect("dir-a");
    vote.cast(ballot(&vote, 7, "dir-b", BallotChoiceV1::No))
        .expect("dir-b");
    vote.close().unwrap();
    let evaluation = vote.evaluate(&chain.store).expect("evaluate");
    assert!(
        !evaluation.accepted(),
        "2 yes against 2 no is a tie, and the board's rules reject ties: {evaluation:?}"
    );
    assert_eq!(evaluation.tally.yes_weight.value(), 2);
    assert_eq!(evaluation.tally.no_weight.value(), 2);

    let finalized = vote
        .finalize(&chain.store, &key(9), 3000)
        .expect("finalize");
    let report = verify(&chain.store, &finalized.tx_id).expect("verify");
    assert!(report.is_valid(), "{report:#?}");
    assert!(
        report
            .checks
            .iter()
            .any(|c| c.name == CheckNameV1::ChannelIsCollective && c.passed)
    );

    // The chair's weight carries a 2–1 division.
    let mut second = VoteV1::draft("Approve the revised budget", proposal());
    second
        .freeze(&chain.store, FOUNDED, &channel("board"))
        .unwrap();
    second.open().unwrap();
    second
        .cast(ballot(&second, 5, "chair", BallotChoiceV1::Yes))
        .unwrap();
    second
        .cast(ballot(&second, 6, "dir-a", BallotChoiceV1::No))
        .unwrap();
    second.close().unwrap();
    assert!(second.evaluate(&chain.store).unwrap().accepted());
}

#[test]
fn a_vote_needs_a_collective_channel() {
    let chain = founded();
    let error = VoteV1::draft("x", proposal())
        .freeze(&chain.store, FOUNDED, &channel("ceo"))
        .expect_err("individual");
    assert!(
        matches!(error, VoteError::NotCollective { ref channel } if channel == "ceo"),
        "{error}"
    );
    let error = VoteV1::draft("x", proposal())
        .freeze(&chain.store, FOUNDED, &channel("treasury"))
        .expect_err("unknown");
    assert!(
        matches!(
            error,
            VoteError::Decision(ref inner)
                if matches!(**inner, irena_decision::DecisionError::NoSuchChannel { .. })
        ),
        "{error}"
    );
}

#[test]
fn a_record_claiming_another_channel_fails_where_the_channels_differ() {
    let chain = founded();
    let genuine = finalized_record(&chain);

    // The shareholders' record relabelled as the board's, id recomputed: the board
    // resolves to different people, so the electorate no longer derives.
    let mut snapshot = genuine.snapshot.clone();
    snapshot.channel = "board".to_owned();
    let tampered = FinalVoteRecordV1::assemble(
        snapshot,
        genuine.ballots.clone(),
        genuine.evaluation.clone(),
    );
    let tx = append_raw(&chain.store, "irena.vote.v1", tampered.canonical_bytes());
    let report = verify(&chain.store, &tx).unwrap();
    assert!(!report.is_valid());
    let failed: Vec<CheckNameV1> = report.failures().map(|c| c.name).collect();
    assert!(
        failed.contains(&CheckNameV1::ElectorateDerives),
        "{failed:?}"
    );
    assert!(
        report
            .checks
            .iter()
            .any(|c| c.name == CheckNameV1::ChannelIsCollective && c.passed),
        "the board is a real collective channel; what fails is who it resolves to"
    );

    // Relabelled as the ceo's: not a collective channel at all, and no rules to rerun.
    let mut snapshot = genuine.snapshot.clone();
    snapshot.channel = "ceo".to_owned();
    let tampered = FinalVoteRecordV1::assemble(
        snapshot,
        genuine.ballots.clone(),
        genuine.evaluation.clone(),
    );
    let tx = append_raw(&chain.store, "irena.vote.v1", tampered.canonical_bytes());
    let report = verify(&chain.store, &tx).unwrap();
    let failed: Vec<CheckNameV1> = report.failures().map(|c| c.name).collect();
    assert!(
        failed.contains(&CheckNameV1::ChannelIsCollective),
        "{failed:?}"
    );
    assert!(
        !report
            .checks
            .iter()
            .any(|c| c.name == CheckNameV1::ResultReproduces),
        "no rules, so the rerun is absent rather than reported: {report:#?}"
    );
}
