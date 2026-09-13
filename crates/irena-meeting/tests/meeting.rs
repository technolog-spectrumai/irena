//! A meeting from draft to a verified final record, and everything that must be
//! refused along the way.

use bornite_core::VoterIdV1;
use irena_core::{CompanyIdV1, NotarisationV1, NotaryIdV1, NotaryTimeV1, RecordKindV1};
use irena_ledger::{company_now, genesis_with_company, publish};
use irena_meeting::{
    AgendaBodyV1, MeetingCheckNameV1, MeetingError, MeetingFinalRecordV1, MeetingIdV1,
    MeetingStatusV1, MeetingVerificationV1, ShareholderMeetingV1, compose_final,
    read_meeting_record, verify_meeting,
};
use irena_vote::{BallotChoiceV1, SignedBallotV1, VoteError};
use prunella_core::{
    BlockHeight, Hash, Namespace, NetworkId, SchemaVersion, TransactionDraft, TxId,
};
use prunella_crypto::SigningKey;
use prunella_store::LocalChainStore;
use tempfile::TempDir;

const RULES: &str = r#"<voting-rules version="1.0">
  <weight type="electorate"/>
  <exclusions enabled="true"/>
  <quorum type="fraction" numerator="1" denominator="2" basis="total-electorate"/>
  <threshold type="simple-majority" basis="votes-cast"/>
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
        address: Some("12 High Street, London".to_owned()),
        at: NotaryTimeV1::parse(at).expect("time"),
        statement: Some("Minuted".to_owned()),
        source_digest: None,
    }
}

/// alice (500, key 1), bob (300, key 2), carol (200, no key).
fn register() -> String {
    format!(
        r#"<share-structure>
  <holder id="alice" key="{}" shares="500"/>
  <holder id="bob" key="{}" shares="300"/>
  <holder id="carol" shares="200"/>
</share-structure>"#,
        key(1).public_key(),
        key(2).public_key()
    )
}

/// bob's shares move to dave: the amendment used to show a freeze holds.
fn register_amended() -> String {
    format!(
        r#"<share-structure>
  <holder id="alice" key="{}" shares="500"/>
  <holder id="carol" shares="200"/>
  <holder id="dave" key="{}" shares="300"/>
</share-structure>"#,
        key(1).public_key(),
        key(4).public_key()
    )
}

fn genesis_xml() -> String {
    format!(
        "<company-genesis><identity name=\"Acme Industries Ltd\"/>{}<governance>{RULES}</governance></company-genesis>",
        register()
    )
}

struct Chain {
    _dir: TempDir,
    store: LocalChainStore,
}

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

fn metadata() -> irena_meeting::MeetingMetadataV1 {
    irena_meeting::MeetingMetadataV1 {
        title: "Annual General Meeting 2026".to_owned(),
        scheduled_at: "2026-06-01T10:00:00Z".to_owned(),
        notice_digest: Some(Hash::from_bytes([0xa0; 32])),
    }
}

fn digest(byte: u8) -> Hash {
    Hash::from_bytes([byte; 32])
}

/// A meeting with one informational item and two vote items.
fn drafted() -> ShareholderMeetingV1 {
    let mut meeting = ShareholderMeetingV1::draft(metadata());
    meeting
        .add_item(
            "Report of the directors",
            AgendaBodyV1::Informational {
                document_digest: digest(0x11),
            },
        )
        .expect("item 1");
    meeting
        .add_item(
            "Approve the 2026 accounts",
            AgendaBodyV1::Vote {
                proposal_digest: digest(0x22),
            },
        )
        .expect("item 2");
    meeting
        .add_item(
            "Re-appoint the auditor",
            AgendaBodyV1::Vote {
                proposal_digest: digest(0x33),
            },
        )
        .expect("item 3");
    meeting
}

fn ballot(
    meeting: &ShareholderMeetingV1,
    item: u32,
    seed: u8,
    id: &str,
    choice: BallotChoiceV1,
) -> SignedBallotV1 {
    let vote = meeting.vote(item).expect("a vote item");
    SignedBallotV1::sign(&key(seed), vote.id().expect("frozen"), &voter(id), choice)
}

/// Runs the whole meeting: convened, opened, both votes cast, closed, finalised.
fn held(chain: &Chain) -> (ShareholderMeetingV1, irena_meeting::MeetingFinalizedV1) {
    // Block timestamps never go backwards, so a second meeting on the same chain
    // starts after the first one's last block.
    let head = chain.store.head().expect("head");
    let base = chain
        .store
        .get_block(head.height)
        .expect("read")
        .expect("block")
        .header
        .timestamp_millis
        + 1000;
    let mut meeting = drafted();
    meeting
        .convene(&chain.store, &key(9), &notary("2026-05-01T09:00:00Z"), base)
        .expect("convene");
    meeting.open(&chain.store).expect("open");
    // Item 2 passes (alice yes 500, bob no 300); item 3 fails (both no).
    meeting
        .cast(2, ballot(&meeting, 2, 1, "alice", BallotChoiceV1::Yes))
        .expect("alice 2");
    meeting
        .cast(2, ballot(&meeting, 2, 2, "bob", BallotChoiceV1::No))
        .expect("bob 2");
    meeting
        .cast(3, ballot(&meeting, 3, 1, "alice", BallotChoiceV1::No))
        .expect("alice 3");
    meeting
        .cast(3, ballot(&meeting, 3, 2, "bob", BallotChoiceV1::No))
        .expect("bob 3");
    meeting.close(&chain.store).expect("close");
    let finalized = meeting
        .finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-01T12:00:00Z"),
            base + 1000,
        )
        .expect("finalize");
    (meeting, finalized)
}

// ---------------------------------------------------------------------------------
// The lifecycle.
// ---------------------------------------------------------------------------------

#[test]
fn a_meeting_runs_from_draft_to_a_verified_record() {
    let chain = founded();
    let mut meeting = drafted();
    assert_eq!(meeting.status(), MeetingStatusV1::Draft);
    assert_eq!(meeting.items().len(), 3);
    assert!(meeting.id().is_none());
    assert!(meeting.company().is_empty());

    let id = meeting
        .convene(&chain.store, &key(9), &notary("2026-05-01T09:00:00Z"), 1000)
        .expect("convene");
    assert_eq!(meeting.status(), MeetingStatusV1::Convened);
    assert_eq!(
        meeting.company(),
        "acme",
        "the company comes from the chain"
    );
    assert_eq!(meeting.id(), Some(id));
    assert_eq!(meeting.convened().map(|(_, h)| h), Some(BlockHeight(1)));

    let at = meeting.open(&chain.store).expect("open");
    assert_eq!(meeting.status(), MeetingStatusV1::Open);
    assert_eq!(at, BlockHeight(1), "frozen at the head as it then was");
    assert_eq!(meeting.votes().count(), 2, "one vote per vote item");
    // Each vote froze the company independently, with its own id and subject.
    let two = meeting.vote(2).expect("item 2");
    let three = meeting.vote(3).expect("item 3");
    assert_ne!(two.id(), three.id());
    assert_eq!(two.subject(), "item 2: Approve the 2026 accounts");
    assert_eq!(three.subject(), "item 3: Re-appoint the auditor");
    assert_eq!(two.proposal_digest(), digest(0x22));
    assert_eq!(two.snapshot().unwrap().height, at);
    assert_eq!(three.snapshot().unwrap().height, at);
    assert_eq!(two.snapshot().unwrap().electorate.len(), 3);
    assert!(
        meeting.vote(1).is_none(),
        "informational items have no vote"
    );

    meeting
        .cast(2, ballot(&meeting, 2, 1, "alice", BallotChoiceV1::Yes))
        .expect("alice");
    meeting
        .cast(2, ballot(&meeting, 2, 2, "bob", BallotChoiceV1::No))
        .expect("bob");
    meeting
        .cast(3, ballot(&meeting, 3, 1, "alice", BallotChoiceV1::No))
        .expect("alice");
    meeting.close(&chain.store).expect("close");
    assert_eq!(meeting.status(), MeetingStatusV1::Closed);
    assert!(meeting.vote(2).unwrap().evaluation().unwrap().accepted());
    assert!(!meeting.vote(3).unwrap().evaluation().unwrap().accepted());

    let finalized = meeting
        .finalize(&chain.store, &key(9), &notary("2026-06-01T12:00:00Z"), 2000)
        .expect("finalize");
    assert_eq!(meeting.status(), MeetingStatusV1::Finalized);
    // Two vote transactions, then the meeting record.
    assert_eq!(finalized.height, BlockHeight(4));
    assert_eq!(finalized.record.items.len(), 3);
    assert_eq!(finalized.record.items[0].vote_tx_id, None, "informational");
    assert!(finalized.record.items[1].vote_tx_id.is_some());
    assert_eq!(
        finalized.record.items[1].outcome.as_deref(),
        Some("accepted")
    );
    assert_eq!(
        finalized.record.items[2].outcome.as_deref(),
        Some("rejected")
    );
    assert_eq!(finalized.record.opened_at_height, at);
    assert_eq!(finalized.record.meeting_id, id);

    let report = verify_meeting(&chain.store, &finalized.tx_id).expect("verify");
    assert!(report.is_valid(), "{report:#?}");
    assert_eq!(report.checks.len(), 9);
    assert_eq!(report.votes.len(), 2, "every vote verified too");
    assert!(report.votes.iter().all(|(_, v)| v.is_valid()));
}

#[test]
fn every_invalid_transition_is_reported_with_both_ends() {
    let chain = founded();
    let mut meeting = drafted();
    let transition = |result: Result<(), MeetingError>, from: MeetingStatusV1| match result
        .expect_err("invalid transition")
    {
        MeetingError::InvalidTransition { from: f, to } => {
            assert_eq!(f, from);
            assert!(!to.is_empty());
        }
        other => panic!("wrong error: {other}"),
    };
    let info = AgendaBodyV1::Informational {
        document_digest: digest(0xaa),
    };

    // Draft: add items and convene only.
    transition(
        meeting.open(&chain.store).map(|_| ()),
        MeetingStatusV1::Draft,
    );
    transition(meeting.close(&chain.store), MeetingStatusV1::Draft);
    transition(
        meeting
            .finalize(&chain.store, &key(9), &notary("2026-06-01T12:00:00Z"), 1)
            .map(|_| ()),
        MeetingStatusV1::Draft,
    );
    transition(meeting.final_record().map(|_| ()), MeetingStatusV1::Draft);

    meeting
        .convene(&chain.store, &key(9), &notary("2026-05-01T09:00:00Z"), 1000)
        .expect("convene");
    // Convened: open only. The agenda is fixed.
    transition(
        meeting.add_item("Late item", info.clone()).map(|_| ()),
        MeetingStatusV1::Convened,
    );
    transition(
        meeting
            .convene(&chain.store, &key(9), &notary("2026-05-01T09:00:00Z"), 1100)
            .map(|_| ()),
        MeetingStatusV1::Convened,
    );
    transition(meeting.close(&chain.store), MeetingStatusV1::Convened);
    transition(
        meeting.final_record().map(|_| ()),
        MeetingStatusV1::Convened,
    );

    meeting.open(&chain.store).expect("open");
    // Open: cast and close only.
    transition(
        meeting.open(&chain.store).map(|_| ()),
        MeetingStatusV1::Open,
    );
    transition(
        meeting.add_item("Late item", info.clone()).map(|_| ()),
        MeetingStatusV1::Open,
    );
    transition(
        meeting
            .finalize(&chain.store, &key(9), &notary("2026-06-01T12:00:00Z"), 1)
            .map(|_| ()),
        MeetingStatusV1::Open,
    );
    transition(meeting.final_record().map(|_| ()), MeetingStatusV1::Open);

    let late = ballot(&meeting, 2, 1, "alice", BallotChoiceV1::Yes);
    meeting.close(&chain.store).expect("close");
    // Closed: finalize only.
    transition(meeting.cast(2, late.clone()), MeetingStatusV1::Closed);
    transition(meeting.close(&chain.store), MeetingStatusV1::Closed);
    transition(
        meeting.open(&chain.store).map(|_| ()),
        MeetingStatusV1::Closed,
    );
    assert!(meeting.final_record().is_ok());

    meeting
        .finalize(&chain.store, &key(9), &notary("2026-06-01T12:00:00Z"), 2000)
        .expect("finalize");
    // Finalized: nothing.
    transition(
        meeting
            .finalize(&chain.store, &key(9), &notary("2026-06-01T12:00:00Z"), 3000)
            .map(|_| ()),
        MeetingStatusV1::Finalized,
    );
    transition(meeting.cast(2, late), MeetingStatusV1::Finalized);
    transition(meeting.close(&chain.store), MeetingStatusV1::Finalized);
    transition(
        meeting.add_item("Late item", info).map(|_| ()),
        MeetingStatusV1::Finalized,
    );
    assert!(meeting.final_record().is_ok());
    let error = meeting
        .open(&chain.store)
        .map(|_| ())
        .expect_err("held once");
    assert_eq!(error.to_string(), "cannot open a meeting that is finalized");
}

#[test]
fn an_agenda_must_be_an_agenda_and_a_meeting_needs_a_company() {
    let chain = founded();
    // No items.
    let mut empty = ShareholderMeetingV1::draft(metadata());
    assert!(matches!(
        empty.convene(&chain.store, &key(9), &notary("2026-05-01T09:00:00Z"), 1000),
        Err(MeetingError::InvalidAgenda { .. })
    ));
    assert!(matches!(
        empty.add_item(
            "  ",
            AgendaBodyV1::Informational {
                document_digest: digest(1)
            }
        ),
        Err(MeetingError::InvalidAgenda { .. })
    ));
    // Bad metadata.
    let mut bad = ShareholderMeetingV1::draft(irena_meeting::MeetingMetadataV1 {
        title: "AGM".to_owned(),
        scheduled_at: "next Tuesday".to_owned(),
        notice_digest: None,
    });
    bad.add_item(
        "x",
        AgendaBodyV1::Informational {
            document_digest: digest(1),
        },
    )
    .expect("item");
    assert!(matches!(
        bad.convene(&chain.store, &key(9), &notary("2026-05-01T09:00:00Z"), 1000),
        Err(MeetingError::InvalidAgenda { .. })
    ));

    // A chain with no company cannot hold a meeting.
    let dir = TempDir::new().expect("dir");
    let store = LocalChainStore::init_genesis(
        dir.path().join("plain.chain"),
        prunella_core::GenesisSpec::new(NetworkId::new("plain").expect("n")),
    )
    .expect("create");
    let mut meeting = drafted();
    let error = meeting
        .convene(&store, &key(9), &notary("2026-05-01T09:00:00Z"), 1000)
        .expect_err("no company");
    assert!(
        matches!(
            error,
            MeetingError::Ledger(irena_ledger::LedgerError::NoCompany { .. })
        ),
        "{error}"
    );
    assert_eq!(
        meeting.status(),
        MeetingStatusV1::Draft,
        "a failed convening leaves a draft"
    );
}

#[test]
fn ballots_reach_the_right_item_and_nothing_else() {
    let chain = founded();
    let mut meeting = drafted();
    meeting
        .convene(&chain.store, &key(9), &notary("2026-05-01T09:00:00Z"), 1000)
        .expect("convene");
    meeting.open(&chain.store).expect("open");

    // No such item, and an informational item.
    let good = ballot(&meeting, 2, 1, "alice", BallotChoiceV1::Yes);
    assert!(matches!(
        meeting.cast(9, good.clone()),
        Err(MeetingError::NoSuchItem { number: 9 })
    ));
    assert!(matches!(
        meeting.cast(1, good.clone()),
        Err(MeetingError::NotAVoteItem { number: 1 })
    ));

    // A ballot for item 2 is not a ballot for item 3: the subjects differ, so the
    // vote ids differ, and the wrong vote refuses it.
    let error = meeting.cast(3, good.clone()).expect_err("wrong item");
    assert!(
        matches!(
            &error,
            MeetingError::Vote {
                number: 3,
                source: VoteError::Ballot(_)
            }
        ),
        "{error}"
    );
    assert!(error.to_string().contains("item 3"), "{error}");

    meeting.cast(2, good.clone()).expect("alice");
    let again = meeting.cast(2, good).expect_err("twice");
    assert!(
        matches!(&again, MeetingError::Vote { number: 2, .. }),
        "{again}"
    );
    assert!(again.to_string().contains("already cast"), "{again}");

    // carol holds shares but no key.
    let carol = ballot(&meeting, 2, 3, "carol", BallotChoiceV1::Yes);
    let error = meeting.cast(2, carol).expect_err("no key");
    assert!(error.to_string().contains("no signing key"), "{error}");

    assert_eq!(meeting.vote(2).unwrap().ballots().count(), 1);
    assert_eq!(meeting.vote(3).unwrap().ballots().count(), 0);
}

#[test]
fn the_meeting_state_round_trips_through_canonical_bytes_between_steps() {
    use prunella_canonical::Canonical;
    let chain = founded();
    let reload = |m: &ShareholderMeetingV1| {
        ShareholderMeetingV1::from_canonical_bytes(&m.canonical_bytes()).expect("decode")
    };
    let mut meeting = drafted();
    assert_eq!(reload(&meeting), meeting);
    meeting
        .convene(&chain.store, &key(9), &notary("2026-05-01T09:00:00Z"), 1000)
        .expect("convene");
    let mut meeting = reload(&meeting);
    meeting.open(&chain.store).expect("open");
    let mut meeting = reload(&meeting);
    meeting
        .cast(2, ballot(&meeting, 2, 1, "alice", BallotChoiceV1::Yes))
        .expect("alice");
    let mut meeting = reload(&meeting);
    assert_eq!(meeting.vote(2).unwrap().ballots().count(), 1);
    meeting.close(&chain.store).expect("close");
    let mut meeting = reload(&meeting);
    let finalized = meeting
        .finalize(&chain.store, &key(9), &notary("2026-06-01T12:00:00Z"), 2000)
        .expect("finalize");
    assert_eq!(reload(&meeting), meeting);
    assert!(
        verify_meeting(&chain.store, &finalized.tx_id)
            .unwrap()
            .is_valid()
    );
}

// ---------------------------------------------------------------------------------
// The freeze holds.
// ---------------------------------------------------------------------------------

#[test]
fn company_changes_after_the_freeze_reach_no_vote_in_the_meeting() {
    let chain = founded();
    let mut meeting = drafted();
    meeting
        .convene(&chain.store, &key(9), &notary("2026-05-01T09:00:00Z"), 1000)
        .expect("convene");
    let at = meeting.open(&chain.store).expect("open");
    let frozen_two = meeting.vote(2).unwrap().id().unwrap();
    let frozen_three = meeting.vote(3).unwrap().id().unwrap();

    // The register is amended while the meeting is open: bob out, dave in.
    let current = company_now(&chain.store)
        .unwrap()
        .provider_of(RecordKindV1::ShareStructure);
    publish(
        &chain.store,
        &key(9),
        RecordKindV1::ShareStructure,
        &register_amended(),
        Some(current),
        &notary("2026-05-15T10:00:00Z"),
        1500,
    )
    .expect("amend");
    assert_eq!(company_now(&chain.store).unwrap().shares.value.len(), 3);

    // dave cannot vote in this meeting; bob still can, in both items.
    let dave = SignedBallotV1::sign(&key(4), frozen_two, &voter("dave"), BallotChoiceV1::Yes);
    let error = meeting.cast(2, dave).expect_err("dave is not frozen in");
    assert!(
        error.to_string().contains("not in the frozen electorate"),
        "{error}"
    );
    meeting
        .cast(2, ballot(&meeting, 2, 1, "alice", BallotChoiceV1::Yes))
        .expect("alice");
    meeting
        .cast(2, ballot(&meeting, 2, 2, "bob", BallotChoiceV1::No))
        .expect("bob");
    meeting
        .cast(3, ballot(&meeting, 3, 2, "bob", BallotChoiceV1::Yes))
        .expect("bob 3");

    meeting.close(&chain.store).expect("close");
    let two = meeting.vote(2).unwrap().evaluation().unwrap();
    assert_eq!(
        two.total_weight, 1000,
        "the frozen electorate, not the amended one"
    );
    assert!(two.accepted());
    // Item 3: bob alone, 300 of 1000, short of the half quorum.
    assert!(!meeting.vote(3).unwrap().evaluation().unwrap().accepted());
    assert_eq!(meeting.vote(2).unwrap().id(), Some(frozen_two));
    assert_eq!(meeting.vote(3).unwrap().id(), Some(frozen_three));

    let finalized = meeting
        .finalize(&chain.store, &key(9), &notary("2026-06-01T12:00:00Z"), 2000)
        .expect("finalize");
    assert_eq!(finalized.record.opened_at_height, at);
    let report = verify_meeting(&chain.store, &finalized.tx_id).expect("verify");
    assert!(
        report.is_valid(),
        "verification resolves at the frozen height: {report:#?}"
    );

    // And the rules may move too: a later meeting sees the new company.
    let current = company_now(&chain.store)
        .unwrap()
        .provider_of(RecordKindV1::VotingRules);
    publish(
        &chain.store,
        &key(9),
        RecordKindV1::VotingRules,
        &RULES.replace(
            "simple-majority\" basis=\"votes-cast",
            "fraction\" numerator=\"2\" denominator=\"3\" basis=\"votes-cast",
        ),
        Some(current),
        &notary("2026-07-01T10:00:00Z"),
        3000,
    )
    .expect("amend rules");
    let mut later = drafted();
    later
        .convene(&chain.store, &key(9), &notary("2026-08-01T09:00:00Z"), 4000)
        .expect("convene");
    later.open(&chain.store).expect("open");
    assert_ne!(
        later.vote(2).unwrap().id(),
        Some(frozen_two),
        "a new freeze is a new vote"
    );
    assert_eq!(
        later.vote(2).unwrap().snapshot().unwrap().electorate.len(),
        3
    );
    // alice 500 yes, bob is gone; dave 300 no. Two thirds of 800 is not reached.
    later
        .cast(2, ballot(&later, 2, 1, "alice", BallotChoiceV1::Yes))
        .expect("alice");
    let dave = SignedBallotV1::sign(
        &key(4),
        later.vote(2).unwrap().id().unwrap(),
        &voter("dave"),
        BallotChoiceV1::No,
    );
    later.cast(2, dave).expect("dave votes now");
    later.close(&chain.store).expect("close");
    assert!(
        !later.vote(2).unwrap().evaluation().unwrap().accepted(),
        "the new rules decide"
    );
}

#[test]
fn two_meetings_on_one_chain_are_independent() {
    let chain = founded();
    let (_, first) = held(&chain);
    let (_, second) = held(&chain);
    assert_ne!(first.tx_id, second.tx_id);
    assert_ne!(first.record.meeting_id, second.record.meeting_id);
    // Same agenda, different meetings, and every vote belongs to exactly one.
    let a = verify_meeting(&chain.store, &first.tx_id).expect("verify");
    let b = verify_meeting(&chain.store, &second.tx_id).expect("verify");
    assert!(a.is_valid() && b.is_valid());
    let first_votes: Vec<TxId> = first
        .record
        .items
        .iter()
        .filter_map(|i| i.vote_tx_id)
        .collect();
    let second_votes: Vec<TxId> = second
        .record
        .items
        .iter()
        .filter_map(|i| i.vote_tx_id)
        .collect();
    assert_eq!(first_votes.len(), 2);
    assert!(first_votes.iter().all(|tx| !second_votes.contains(tx)));
}

// ---------------------------------------------------------------------------------
// Verification refuses what it should.
// ---------------------------------------------------------------------------------

fn append_raw(store: &LocalChainStore, namespace: &str, payload: Vec<u8>) -> TxId {
    let signer = key(8);
    let head = store.head().unwrap();
    let parent = store.get_block(head.height).unwrap().unwrap();
    let transaction = signer.sign_transaction(TransactionDraft {
        namespace: Namespace::new(namespace).unwrap(),
        schema_version: SchemaVersion(1),
        payload,
        signer: signer.public_key(),
        nonce: head.height.value() + 500,
    });
    let id = transaction.id;
    let block = parent
        .header
        .child_draft(vec![transaction], parent.header.timestamp_millis + 1)
        .unwrap()
        .build()
        .unwrap();
    store.append_block(block).unwrap();
    id
}

/// Re-composes a final record with one field changed and puts it on the chain.
fn republish(chain: &Chain, record: &MeetingFinalRecordV1) -> TxId {
    let payload = compose_final(&acme(), &notary("2026-06-01T12:00:00Z"), record).expect("compose");
    append_raw(&chain.store, "irena.meeting.v1", payload.into_bytes())
}

fn failures(report: &MeetingVerificationV1) -> Vec<MeetingCheckNameV1> {
    report.failures().map(|check| check.name).collect()
}

#[test]
fn a_missing_or_foreign_vote_reference_is_caught() {
    let chain = founded();
    let (_, finalized) = held(&chain);
    let good = finalized.record.clone();

    // A vote transaction that is not on the chain.
    let mut missing = good.clone();
    missing.items[1].vote_tx_id = Some(TxId::from_hash(digest(0x99)));
    let report = verify_meeting(&chain.store, &republish(&chain, &missing)).expect("verify");
    assert!(!report.is_valid());
    assert!(
        failures(&report).contains(&MeetingCheckNameV1::VotesVerify),
        "{report:#?}"
    );

    // The other item's vote, put under this item: a real, valid vote that is not this
    // item's. Only the belongs check can see it.
    let mut swapped = good.clone();
    swapped.items[1].vote_tx_id = good.items[2].vote_tx_id;
    swapped.items[1].outcome = good.items[2].outcome.clone();
    let report = verify_meeting(&chain.store, &republish(&chain, &swapped)).expect("verify");
    assert!(!report.is_valid());
    assert_eq!(
        failures(&report),
        [MeetingCheckNameV1::VotesBelong],
        "{report:#?}"
    );
    assert!(
        report.votes.iter().all(|(_, v)| v.is_valid()),
        "the vote itself is fine"
    );

    // A vote from another meeting entirely, with the same agenda.
    let (_, other) = held(&chain);
    let mut foreign = good.clone();
    foreign.items[1].vote_tx_id = other.record.items[1].vote_tx_id;
    let report = verify_meeting(&chain.store, &republish(&chain, &foreign)).expect("verify");
    assert!(
        failures(&report).contains(&MeetingCheckNameV1::VotesBelong),
        "{report:#?}"
    );

    // A vote item with no vote at all.
    let mut absent = good.clone();
    absent.items[1].vote_tx_id = None;
    absent.items[1].outcome = None;
    let error =
        compose_final(&acme(), &notary("2026-06-01T12:00:00Z"), &absent).expect_err("no vote tx");
    assert!(matches!(error, MeetingError::Record(_)), "{error}");

    // An outcome that disagrees with the vote.
    let mut lying = good;
    lying.items[2].outcome = Some("accepted".to_owned());
    let report = verify_meeting(&chain.store, &republish(&chain, &lying)).expect("verify");
    assert_eq!(
        failures(&report),
        [MeetingCheckNameV1::VotesBelong],
        "{report:#?}"
    );
}

#[test]
fn a_tampered_agenda_or_metadata_is_caught_against_the_convening() {
    let chain = founded();
    let (_, finalized) = held(&chain);
    let good = finalized.record.clone();

    // The agenda in the final record is not the agenda convened.
    let mut retitled = good.clone();
    retitled.items[0].item.title = "Something else entirely".to_owned();
    let report = verify_meeting(&chain.store, &republish(&chain, &retitled)).expect("verify");
    assert!(
        failures(&report).contains(&MeetingCheckNameV1::AgendaMatches),
        "{report:#?}"
    );

    // An item quietly dropped.
    let mut shorter = good.clone();
    shorter.items.pop();
    let report = verify_meeting(&chain.store, &republish(&chain, &shorter)).expect("verify");
    assert!(
        failures(&report).contains(&MeetingCheckNameV1::AgendaMatches),
        "{report:#?}"
    );

    // An informational item turned into a vote item's digest.
    let mut swapped_kind = good.clone();
    swapped_kind.items[0].item.body = AgendaBodyV1::Informational {
        document_digest: digest(0xff),
    };
    let report = verify_meeting(&chain.store, &republish(&chain, &swapped_kind)).expect("verify");
    assert!(
        failures(&report).contains(&MeetingCheckNameV1::AgendaMatches),
        "{report:#?}"
    );

    // The metadata rewritten.
    let mut renamed = good.clone();
    renamed.metadata.title = "Extraordinary General Meeting".to_owned();
    let report = verify_meeting(&chain.store, &republish(&chain, &renamed)).expect("verify");
    assert_eq!(
        failures(&report),
        [MeetingCheckNameV1::MetadataMatches],
        "{report:#?}"
    );
    let mut rescheduled = good.clone();
    rescheduled.metadata.scheduled_at = "2026-06-02T10:00:00Z".to_owned();
    let report = verify_meeting(&chain.store, &republish(&chain, &rescheduled)).expect("verify");
    assert_eq!(
        failures(&report),
        [MeetingCheckNameV1::MetadataMatches],
        "{report:#?}"
    );

    // A claimed opening height that is not where the votes were frozen.
    let mut moved = good;
    moved.opened_at_height = BlockHeight(0);
    let report = verify_meeting(&chain.store, &republish(&chain, &moved)).expect("verify");
    assert!(
        failures(&report).contains(&MeetingCheckNameV1::VotesBelong),
        "{report:#?}"
    );
}

#[test]
fn a_record_that_names_no_real_convening_is_caught() {
    let chain = founded();
    let (_, finalized) = held(&chain);

    // A meeting id pointing at nothing.
    let mut invented = finalized.record.clone();
    invented.meeting_id = MeetingIdV1::from_tx(TxId::from_hash(digest(0x77)));
    let report = verify_meeting(&chain.store, &republish(&chain, &invented)).expect("verify");
    assert_eq!(
        failures(&report),
        [MeetingCheckNameV1::ConveningExists],
        "{report:#?}"
    );

    // A meeting id pointing at a final record rather than a convening.
    let mut circular = finalized.record.clone();
    circular.meeting_id = MeetingIdV1::from_tx(finalized.tx_id);
    let report = verify_meeting(&chain.store, &republish(&chain, &circular)).expect("verify");
    assert_eq!(
        failures(&report),
        [MeetingCheckNameV1::ConveningExists],
        "{report:#?}"
    );

    // A meeting id pointing at something that is not a meeting record at all.
    let mut elsewhere = finalized.record;
    elsewhere.meeting_id = MeetingIdV1::from_tx(company_now(&chain.store).unwrap().genesis_tx_id);
    let report = verify_meeting(&chain.store, &republish(&chain, &elsewhere)).expect("verify");
    assert_eq!(
        failures(&report),
        [MeetingCheckNameV1::ConveningExists],
        "{report:#?}"
    );
}

#[test]
fn a_corrupted_vote_record_fails_both_layers() {
    use prunella_canonical::Canonical;
    let chain = founded();
    let (_, finalized) = held(&chain);
    let vote_tx = finalized.record.items[1].vote_tx_id.expect("a vote");

    // Flip a byte inside a ballot signature and re-publish the vote record.
    let located = chain.store.get_transaction(&vote_tx).unwrap().unwrap();
    let mut vote =
        irena_vote::FinalVoteRecordV1::from_canonical_bytes(&located.transaction.payload).unwrap();
    let mut signature = vote.ballots[0].signature.to_bytes();
    signature[7] ^= 0x01;
    vote.ballots[0].signature = prunella_core::Signature::from_bytes(signature);
    let corrupted = append_raw(&chain.store, "irena.vote.v1", vote.canonical_bytes());

    let mut record = finalized.record;
    record.items[1].vote_tx_id = Some(corrupted);
    let report = verify_meeting(&chain.store, &republish(&chain, &record)).expect("verify");
    assert!(!report.is_valid());
    assert!(
        failures(&report).contains(&MeetingCheckNameV1::VotesVerify),
        "{report:#?}"
    );
    let (_, vote_report) = report.votes.iter().find(|(n, _)| *n == 2).expect("item 2");
    assert!(!vote_report.is_valid(), "the vote's own checks fail too");
    assert!(
        report
            .checks
            .iter()
            .find(|c| c.name == MeetingCheckNameV1::VotesVerify)
            .unwrap()
            .detail
            .contains("BallotsVerify"),
        "{report:#?}"
    );
}

#[test]
fn non_records_are_reported_not_mistaken_for_meetings() {
    let chain = founded();
    let (_, finalized) = held(&chain);

    let garbage = append_raw(&chain.store, "irena.meeting.v1", b"not a meeting".to_vec());
    let report = verify_meeting(&chain.store, &garbage).expect("verify");
    assert_eq!(failures(&report), [MeetingCheckNameV1::Decodes]);
    assert!(report.record.is_none());

    // The convening record is not a final record.
    let convening = finalized.record.meeting_id.tx_id();
    let report = verify_meeting(&chain.store, &convening).expect("verify");
    assert_eq!(failures(&report), [MeetingCheckNameV1::Decodes]);
    assert!(
        report.checks[0].detail.contains("convening record"),
        "{report:#?}"
    );

    // A meeting record filed in another namespace.
    let payload = chain
        .store
        .get_transaction(&finalized.tx_id)
        .unwrap()
        .unwrap()
        .transaction
        .payload;
    let elsewhere = append_raw(&chain.store, "app.other", payload);
    let report = verify_meeting(&chain.store, &elsewhere).expect("verify");
    assert_eq!(failures(&report), [MeetingCheckNameV1::Decodes]);
    assert!(report.checks[0].detail.contains("app.other"));

    let missing = TxId::from_hash(digest(0x99));
    assert!(matches!(
        verify_meeting(&chain.store, &missing),
        Err(MeetingError::NoSuchTransaction { .. })
    ));
}

// ---------------------------------------------------------------------------------
// The records themselves.
// ---------------------------------------------------------------------------------

#[test]
fn both_records_are_nested_readable_xml_on_the_chain() {
    let chain = founded();
    let (meeting, finalized) = held(&chain);
    let convening = meeting.convened().unwrap().0;

    for tx in [convening, finalized.tx_id] {
        let located = chain.store.get_transaction(&tx).unwrap().unwrap();
        assert_eq!(located.transaction.namespace.as_str(), "irena.meeting.v1");
        let text = core::str::from_utf8(&located.transaction.payload).expect("utf-8");
        assert!(text.starts_with("<irena-meeting version=\"1.0\""), "{text}");
        assert!(
            text.contains("<notarisation id=\"notary-07\" name=\"Jane Roe\""),
            "{text}"
        );
        assert!(
            prunella_xml::is_single_element(text),
            "one element, so Prunella nests it"
        );
        read_meeting_record(text).expect("reads back");
    }

    // The export shows both, readable, alongside the company and the votes.
    let document =
        prunella_xml::export(&chain.store, &prunella_xml::ExportRequest::full()).expect("export");
    let xml = prunella_xml::write_document(&document).expect("render");
    assert_eq!(
        xml.matches("<payload encoding=\"xml\"><irena-meeting ")
            .count(),
        2,
        "{xml}"
    );
    assert!(xml.contains("Annual General Meeting 2026"), "{xml}");
    assert!(
        xml.contains("<item number=\"1\" kind=\"informational\""),
        "{xml}"
    );
    assert!(xml.contains("kind=\"vote\""), "{xml}");
    // The vote records are Borsh, so they are the base64 ones.
    assert!(xml.contains("encoding=\"base64\""), "{xml}");

    // And the whole chain re-imports to the same meeting.
    let restored_path = chain._dir.path().join("restored.chain");
    let (restored, _) =
        prunella_xml::restore(&restored_path, &prunella_xml::read_document(&xml).unwrap())
            .expect("restore");
    assert!(
        verify_meeting(&restored, &finalized.tx_id)
            .unwrap()
            .is_valid()
    );
}

#[test]
fn meeting_records_are_read_strictly() {
    let chain = founded();
    let (meeting, finalized) = held(&chain);
    let text = |tx: TxId| {
        let located = chain.store.get_transaction(&tx).unwrap().unwrap();
        String::from_utf8(located.transaction.payload).expect("utf-8")
    };
    let convened = text(meeting.convened().unwrap().0);
    let final_text = text(finalized.tx_id);

    for (label, xml) in [
        (
            "unknown version",
            convened.replace("version=\"1.0\"", "version=\"2.0\""),
        ),
        (
            "unknown kind",
            convened.replace("kind=\"convened\"", "kind=\"adjourned\""),
        ),
        (
            "unknown attribute",
            convened.replace("<meeting ", "<meeting chair=\"alice\" "),
        ),
        (
            "unknown child",
            convened.replace("</meeting>", "<minutes/></meeting>"),
        ),
        (
            "unknown item kind",
            convened.replace("kind=\"informational\"", "kind=\"motion\""),
        ),
        (
            "bad digest",
            convened.replace("document-digest=\"11", "document-digest=\"zz"),
        ),
        (
            "bad time",
            convened.replace(
                "scheduled-at=\"2026-06-01T10:00:00Z\"",
                "scheduled-at=\"soon\"",
            ),
        ),
        (
            "empty title",
            convened.replace("title=\"Annual General Meeting 2026\"", "title=\"\""),
        ),
        (
            "no notarisation",
            convened.replace(
                &convened
                    [convened.find("<notarisation").unwrap()..convened.find("/>").unwrap() + 2],
                "",
            ),
        ),
        (
            "a convening that names a meeting",
            convened.replace("kind=\"convened\"", "kind=\"convened\" meeting=\"aa\""),
        ),
        (
            "a final record with no meeting",
            final_text.replace(&format!(" meeting=\"{}\"", finalized.record.meeting_id), ""),
        ),
        (
            "vote-tx on an informational item",
            convened.replace(
                "kind=\"informational\" title=\"Report of the directors\"",
                "kind=\"informational\" title=\"Report of the directors\" vote-tx=\"00\"",
            ),
        ),
        (
            "opened-at-height on a convening",
            convened.replace("<meeting ", "<meeting opened-at-height=\"1\" "),
        ),
        (
            "misnumbered agenda",
            convened.replace("<item number=\"2\"", "<item number=\"7\""),
        ),
    ] {
        assert!(
            read_meeting_record(&xml).is_err(),
            "{label} should have been refused"
        );
    }
    // A valid record still reads, and its serde form is useful to tooling.
    let record = read_meeting_record(&final_text).expect("valid");
    let json = serde_json::to_value(&record).expect("json");
    assert_eq!(json["company"], "acme");
    assert_eq!(json["notarisation"]["id"], "notary-07");
    assert_eq!(json["body"]["kind"], "final");
}
