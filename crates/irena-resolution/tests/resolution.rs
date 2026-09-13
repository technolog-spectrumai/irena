//! The governance-closing loop: a passed vote becomes a resolution, and an amendment
//! resolution changes the company — and everything that must be refused.

use bornite_core::VoterIdV1;
use irena_core::{CompanyIdV1, NotarisationV1, NotaryIdV1, NotaryTimeV1, RecordKindV1};
use irena_ledger::{company_now, genesis_with_company, publish, reconstruct};
use irena_meeting::{AgendaBodyV1, MeetingMetadataV1, MeetingV1};
use irena_resolution::{
    AmendmentTargetV1, AuthorityV1, ExecutionVerificationV1, ResolutionError, ResolutionIdV1,
    ResolutionKindV1, ResolutionStatusV1, ResolutionV1, ResolutionVerificationV1,
    compose_execution, compose_resolution, proposal_digest, read_execution_record,
    read_resolution_record, verify_execution, verify_resolution,
};
use irena_vote::{BallotChoiceV1, SignedBallotV1};
use prunella_core::{Hash, Namespace, NetworkId, SchemaVersion, TransactionDraft, TxId};
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

/// A two-thirds supermajority: the rules an amendment resolution installs.
const RULES_V2: &str = r#"<voting-rules version="1.0">
  <weight type="electorate"/>
  <exclusions enabled="true"/>
  <quorum type="none"/>
  <threshold type="fraction" numerator="2" denominator="3" basis="votes-cast"/>
  <abstentions treatment="exclude"/>
  <tie treatment="accept"/>
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

/// carol is bought out; her shares go to alice.
fn register_v2() -> String {
    format!(
        r#"<share-structure>
  <holder id="alice" key="{}" shares="700"/>
  <holder id="bob" key="{}" shares="300"/>
</share-structure>"#,
        key(1).public_key(),
        key(2).public_key()
    )
}

fn genesis_xml() -> String {
    format!(
        "<company-genesis><identity name=\"Acme Industries Ltd\"/>{}<governance>{}</governance></company-genesis>",
        register(),
        channels(RULES)
    )
}

/// The channel set: shareholders (share register, collective, `rules`); board (chair
/// key 5 weight 2, dir-a key 6, dir-b key 7; collective, simple majority, no quorum);
/// ceo (chair, individual).
fn channels(rules: &str) -> String {
    format!(
        r#"<decision-channels>
  <channel id="shareholders" mode="collective">
    <actors source="share-register"/>
    {rules}
  </channel>
  <channel id="board" mode="collective">
    <actors source="roster">
      <member id="chair" key="{}" weight="2"/>
      <member id="dir-a" key="{}"/>
      <member id="dir-b" key="{}"/>
    </actors>
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
      <member id="chair" key="{}"/>
    </actors>
  </channel>
</decision-channels>"#,
        key(5).public_key(),
        key(6).public_key(),
        key(7).public_key(),
        key(5).public_key()
    )
}

/// The same channels with the shareholders under RULES_V2: the channel-set amendment
/// a resolution installs.
fn channels_v2() -> String {
    channels(RULES_V2)
}

/// The shareholders' rules of a channel set in force.
fn rules_of(channels: &irena_core::DecisionChannelsV1) -> &irena_core::VotingRulesV1 {
    channels
        .get(&irena_core::ChannelIdV1::new("shareholders").unwrap())
        .expect("shareholders channel")
        .mode
        .rules()
        .expect("collective")
}

struct Chain {
    _dir: TempDir,
    store: LocalChainStore,
}

impl Chain {
    /// A timestamp after the head's, so blocks never regress.
    fn next_timestamp(&self) -> u64 {
        let head = self.store.head().expect("head");
        self.store
            .get_block(head.height)
            .expect("read")
            .expect("block")
            .header
            .timestamp_millis
            + 1000
    }
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

/// What a meeting decided: the meeting's final record transaction, and per item the
/// vote transaction.
struct Held {
    meeting_tx: TxId,
}

/// Holds a meeting whose items are exactly `items` (title, digest, pass), and returns
/// where it landed. Item numbers are 1-based in the order given.
fn hold(chain: &Chain, items: &[(&str, Hash, bool)]) -> Held {
    let mut meeting = MeetingV1::draft(MeetingMetadataV1 {
        channel: "shareholders".to_owned(),
        title: "Annual General Meeting 2026".to_owned(),
        scheduled_at: "2026-06-01T10:00:00Z".to_owned(),
        notice_digest: None,
    });
    for (title, digest, _) in items {
        meeting
            .add_item(
                *title,
                AgendaBodyV1::Vote {
                    proposal_digest: *digest,
                },
            )
            .expect("item");
    }
    let base = chain.next_timestamp();
    meeting
        .convene(&chain.store, &key(9), &notary("2026-05-01T09:00:00Z"), base)
        .expect("convene");
    meeting.open(&chain.store).expect("open");
    for (index, (_, _, pass)) in items.iter().enumerate() {
        let number = index as u32 + 1;
        let vote_id = meeting.vote(number).expect("vote").id().expect("frozen");
        // alice 500 and bob 300 both vote the same way: 800 of 1000 is a quorum, and
        // the motion carries or falls unanimously.
        let choice = if *pass {
            BallotChoiceV1::Yes
        } else {
            BallotChoiceV1::No
        };
        for (seed, id) in [(1u8, "alice"), (2, "bob")] {
            meeting
                .cast(
                    number,
                    SignedBallotV1::sign(&key(seed), vote_id, &voter(id), choice),
                )
                .expect("ballot");
        }
    }
    meeting.close(&chain.store).expect("close");
    let finalized = meeting
        .finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-01T12:00:00Z"),
            base + 100,
        )
        .expect("finalize");
    Held {
        meeting_tx: finalized.tx_id,
    }
}

/// The vote transaction that answered one item of a held meeting.
fn vote_of(chain: &Chain, held: &Held, item: u32) -> TxId {
    let report = irena_meeting::verify_meeting(&chain.store, &held.meeting_tx).expect("verify");
    report
        .record
        .expect("final record")
        .items
        .iter()
        .find(|entry| entry.item.number == item)
        .expect("item")
        .vote_tx_id
        .expect("a vote")
}

fn authority(chain: &Chain, held: &Held, item: u32) -> AuthorityV1 {
    AuthorityV1::Collective {
        channel: "shareholders".to_owned(),
        meeting_tx: held.meeting_tx,
        item_number: item,
        vote_tx: vote_of(chain, held, item),
    }
}

/// Drafts, finalises and executes an amendment resolution end to end.
fn resolve_amendment(
    chain: &Chain,
    held: &Held,
    item: u32,
    target: AmendmentTargetV1,
    body: &str,
    title: &str,
) -> ResolutionV1 {
    let mut resolution = ResolutionV1::draft(
        title,
        authority(chain, held, item),
        ResolutionKindV1::Amendment {
            target,
            body: body.to_owned(),
        },
    );
    resolution
        .finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-02T09:00:00Z"),
            chain.next_timestamp(),
        )
        .expect("finalize");
    resolution
        .execute(
            &chain.store,
            &key(9),
            &notary("2026-06-02T10:00:00Z"),
            chain.next_timestamp(),
        )
        .expect("execute");
    resolution
}

// ---------------------------------------------------------------------------------
// The loop closes.
// ---------------------------------------------------------------------------------

#[test]
fn a_passed_vote_becomes_a_resolution_that_replaces_the_share_register() {
    let chain = founded();
    let before = company_now(&chain.store).expect("state");
    assert_eq!(before.shares.value.len(), 3);
    let approved_base = before.shares.tx_id;

    // The proposal the shareholders vote on IS the new register.
    let digest = proposal_digest(&register_v2());
    let held = hold(&chain, &[("Buy out carol", digest, true)]);

    let mut resolution = ResolutionV1::draft(
        "Resolution 1: buy out carol",
        authority(&chain, &held, 1),
        ResolutionKindV1::Amendment {
            target: AmendmentTargetV1::ShareStructure,
            body: register_v2(),
        },
    );
    assert_eq!(resolution.status(), ResolutionStatusV1::Draft);
    assert!(resolution.id().is_none());

    let id = resolution
        .finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-02T09:00:00Z"),
            chain.next_timestamp(),
        )
        .expect("finalize");
    assert_eq!(resolution.status(), ResolutionStatusV1::Finalized);
    assert_eq!(resolution.company(), "acme");
    assert_eq!(resolution.id(), Some(id));
    // Recording the resolution changed nothing about the company.
    assert_eq!(
        company_now(&chain.store).unwrap().shares.tx_id,
        approved_base
    );

    let executed = resolution
        .execute(
            &chain.store,
            &key(9),
            &notary("2026-06-02T10:00:00Z"),
            chain.next_timestamp(),
        )
        .expect("execute");
    assert_eq!(resolution.status(), ResolutionStatusV1::Executed);
    assert_eq!(executed.replaced_tx, approved_base);
    assert!(executed.amendment_height < executed.execution_height);

    // Now, and only now, the company has changed — through an ordinary amendment.
    let after = company_now(&chain.store).expect("state");
    assert_eq!(after.shares.tx_id, executed.amendment_tx);
    assert_eq!(after.shares.supersedes, Some(approved_base));
    assert_eq!(after.shares.value.len(), 2);
    assert_eq!(after.shares.value.total_shares(), 1000);
    assert_eq!(
        after.shares.value.holders()[0].shares,
        700,
        "alice bought carol out"
    );
    // The amendment is a plain company record: reconstruction needed no resolution.
    let amendment = chain
        .store
        .get_transaction(&executed.amendment_tx)
        .unwrap()
        .unwrap();
    assert_eq!(amendment.transaction.namespace.as_str(), "irena.shares.v1");

    // And both records verify from the chain alone.
    let report = verify_resolution(&chain.store, &id.tx_id()).expect("verify");
    assert!(report.is_valid(), "{report:#?}");
    assert_eq!(report.checks.len(), 10);
    let report = verify_execution(&chain.store, &executed.execution_tx).expect("verify");
    assert!(report.is_valid(), "{report:#?}");
    assert_eq!(report.checks.len(), 10);
    assert!(report.resolution.as_ref().unwrap().is_valid());
}

#[test]
fn a_resolution_can_replace_the_channel_set_and_the_next_vote_uses_it() {
    let chain = founded();
    let digest = proposal_digest(&channels_v2());
    let held = hold(&chain, &[("Adopt a two-thirds majority", digest, true)]);
    let before = company_now(&chain.store).unwrap().channels.tx_id;

    let resolution = resolve_amendment(
        &chain,
        &held,
        1,
        AmendmentTargetV1::DecisionChannels,
        &channels_v2(),
        "Resolution 1: two-thirds majority",
    );
    let executed = resolution.executed().expect("executed");
    let after = company_now(&chain.store).expect("state");
    assert_eq!(after.channels.tx_id, executed.amendment_tx);
    assert_ne!(after.channels.tx_id, before);
    assert_eq!(
        *rules_of(&after.channels.value),
        bornite_xml::read_rules_document(RULES_V2).unwrap()
    );
    assert_eq!(after.shares.tx_id, before, "the register is untouched");

    // A meeting held now is decided under the new rules: alice 500 yes and bob 300 no
    // would carry a simple majority, but not two thirds.
    let later = hold(
        &chain,
        &[("A further motion", Hash::from_bytes([9; 32]), true)],
    );
    let vote = irena_vote::verify(&chain.store, &vote_of(&chain, &later, 1)).unwrap();
    let record = vote.record.expect("record");
    assert_eq!(record.evaluation.threshold_numerator, 2);
    assert_eq!(record.evaluation.threshold_denominator, 3);
    assert!(
        verify_execution(&chain.store, &executed.execution_tx)
            .unwrap()
            .is_valid()
    );
}

#[test]
fn a_declarative_resolution_records_a_decision_and_changes_nothing() {
    let chain = founded();
    let document = Hash::from_bytes([0xdd; 32]);
    let held = hold(&chain, &[("Receive the directors' report", document, true)]);
    let before = company_now(&chain.store).expect("state");

    let mut resolution = ResolutionV1::draft(
        "Resolution 1: the report is received",
        authority(&chain, &held, 1),
        ResolutionKindV1::Declarative {
            document_digest: document,
        },
    );
    let id = resolution
        .finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-02T09:00:00Z"),
            chain.next_timestamp(),
        )
        .expect("finalize");
    assert_eq!(resolution.status(), ResolutionStatusV1::Finalized);

    // Finalized is the end: there is nothing to execute.
    let error = resolution
        .execute(
            &chain.store,
            &key(9),
            &notary("2026-06-02T10:00:00Z"),
            chain.next_timestamp(),
        )
        .expect_err("declarative");
    assert!(
        matches!(error, ResolutionError::NothingToExecute),
        "{error}"
    );
    assert_eq!(resolution.status(), ResolutionStatusV1::Finalized);

    // The company is exactly as it was.
    let after = company_now(&chain.store).expect("state");
    assert_eq!(after.identity.tx_id, before.identity.tx_id);
    assert_eq!(after.shares.tx_id, before.shares.tx_id);
    assert_eq!(after.channels.tx_id, before.channels.tx_id);
    assert_eq!(after.applied.len(), before.applied.len());

    let report = verify_resolution(&chain.store, &id.tx_id()).expect("verify");
    assert!(report.is_valid(), "{report:#?}");
}

#[test]
fn several_resolutions_from_one_meeting_are_independent() {
    let chain = founded();
    let shares_digest = proposal_digest(&register_v2());
    let rules_digest = proposal_digest(&channels_v2());
    let document = Hash::from_bytes([0xdd; 32]);
    let held = hold(
        &chain,
        &[
            ("Receive the report", document, true),
            ("Buy out carol", shares_digest, true),
            ("Adopt a two-thirds majority", rules_digest, true),
        ],
    );

    let mut report_resolution = ResolutionV1::draft(
        "Resolution 1",
        authority(&chain, &held, 1),
        ResolutionKindV1::Declarative {
            document_digest: document,
        },
    );
    report_resolution
        .finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-02T09:00:00Z"),
            chain.next_timestamp(),
        )
        .expect("finalize");
    let shares = resolve_amendment(
        &chain,
        &held,
        2,
        AmendmentTargetV1::ShareStructure,
        &register_v2(),
        "Resolution 2",
    );
    let rules = resolve_amendment(
        &chain,
        &held,
        3,
        AmendmentTargetV1::DecisionChannels,
        &channels_v2(),
        "Resolution 3",
    );

    // Both amendments landed, each on its own part.
    let state = company_now(&chain.store).expect("state");
    assert_eq!(state.shares.tx_id, shares.executed().unwrap().amendment_tx);
    assert_eq!(state.channels.tx_id, rules.executed().unwrap().amendment_tx);
    assert_eq!(state.shares.value.len(), 2);
    // Three resolutions, two executions, every one verifying.
    for id in [
        report_resolution.id().unwrap(),
        shares.id().unwrap(),
        rules.id().unwrap(),
    ] {
        assert!(
            verify_resolution(&chain.store, &id.tx_id())
                .unwrap()
                .is_valid()
        );
    }
    for executed in [shares.executed().unwrap(), rules.executed().unwrap()] {
        assert!(
            verify_execution(&chain.store, &executed.execution_tx)
                .unwrap()
                .is_valid()
        );
    }
}

// ---------------------------------------------------------------------------------
// What must be refused.
// ---------------------------------------------------------------------------------

#[test]
fn a_rejected_vote_authorises_nothing() {
    let chain = founded();
    let digest = proposal_digest(&register_v2());
    let held = hold(&chain, &[("Buy out carol", digest, false)]);

    let mut resolution = ResolutionV1::draft(
        "Resolution 1",
        authority(&chain, &held, 1),
        ResolutionKindV1::Amendment {
            target: AmendmentTargetV1::ShareStructure,
            body: register_v2(),
        },
    );
    let head = chain.store.head().expect("head");
    let error = resolution
        .finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-02T09:00:00Z"),
            chain.next_timestamp(),
        )
        .expect_err("rejected");
    assert!(
        matches!(&error, ResolutionError::VoteRejected { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("authorises nothing"), "{error}");
    assert_eq!(resolution.status(), ResolutionStatusV1::Draft);
    assert_eq!(chain.store.head().expect("head"), head, "nothing written");
}

#[test]
fn a_resolution_must_name_the_vote_that_answered_its_item() {
    let chain = founded();
    let first = proposal_digest(&register_v2());
    let second = proposal_digest(&channels_v2());
    let held = hold(
        &chain,
        &[
            ("Buy out carol", first, true),
            ("Adopt two thirds", second, true),
        ],
    );
    let head = chain.store.head().expect("head");

    // Item 1's resolution, pointing at item 2's vote.
    let mut crossed = ResolutionV1::draft(
        "Resolution 1",
        AuthorityV1::Collective {
            channel: "shareholders".to_owned(),
            meeting_tx: held.meeting_tx,
            item_number: 1,
            vote_tx: vote_of(&chain, &held, 2),
        },
        ResolutionKindV1::Amendment {
            target: AmendmentTargetV1::ShareStructure,
            body: register_v2(),
        },
    );
    let error = crossed
        .finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-02T09:00:00Z"),
            chain.next_timestamp(),
        )
        .expect_err("wrong vote");
    assert!(
        matches!(&error, ResolutionError::WrongVote { item_number: 1, .. }),
        "{error}"
    );

    assert_eq!(
        chain.store.head().expect("head"),
        head,
        "a refused resolution writes nothing"
    );

    // A vote from another meeting entirely.
    let other = hold(&chain, &[("Buy out carol", first, true)]);
    let mut foreign = ResolutionV1::draft(
        "Resolution 1",
        AuthorityV1::Collective {
            channel: "shareholders".to_owned(),
            meeting_tx: held.meeting_tx,
            item_number: 1,
            vote_tx: vote_of(&chain, &other, 1),
        },
        ResolutionKindV1::Amendment {
            target: AmendmentTargetV1::ShareStructure,
            body: register_v2(),
        },
    );
    assert!(matches!(
        foreign.finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-02T09:00:00Z"),
            chain.next_timestamp()
        ),
        Err(ResolutionError::WrongVote { .. })
    ));

    // An item that does not exist, and a meeting that is not a meeting. Neither of
    // these reaches the chain either.
    let head = chain.store.head().expect("head");
    let mut absent = ResolutionV1::draft(
        "Resolution 9",
        AuthorityV1::Collective {
            channel: "shareholders".to_owned(),
            meeting_tx: held.meeting_tx,
            item_number: 9,
            vote_tx: vote_of(&chain, &held, 1),
        },
        ResolutionKindV1::Declarative {
            document_digest: first,
        },
    );
    assert!(matches!(
        absent.finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-02T09:00:00Z"),
            chain.next_timestamp()
        ),
        Err(ResolutionError::NoSuchVoteItem { item_number: 9, .. })
    ));
    let mut nowhere = ResolutionV1::draft(
        "Resolution 1",
        AuthorityV1::Collective {
            channel: "shareholders".to_owned(),
            meeting_tx: TxId::from_hash(Hash::from_bytes([0x77; 32])),
            item_number: 1,
            vote_tx: vote_of(&chain, &held, 1),
        },
        ResolutionKindV1::Declarative {
            document_digest: first,
        },
    );
    assert!(
        nowhere
            .finalize(
                &chain.store,
                &key(9),
                &notary("2026-06-02T09:00:00Z"),
                chain.next_timestamp()
            )
            .is_err()
    );
    assert_eq!(chain.store.head().expect("head"), head, "nothing written");
}

#[test]
fn a_resolution_must_carry_what_the_shareholders_approved() {
    let chain = founded();
    let held = hold(
        &chain,
        &[("Buy out carol", proposal_digest(&register_v2()), true)],
    );
    let head = chain.store.head().expect("head");

    // A different register than the one voted on.
    let mut swapped = ResolutionV1::draft(
        "Resolution 1",
        authority(&chain, &held, 1),
        ResolutionKindV1::Amendment {
            target: AmendmentTargetV1::ShareStructure,
            body: register(),
        },
    );
    let error = swapped
        .finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-02T09:00:00Z"),
            chain.next_timestamp(),
        )
        .expect_err("not approved");
    assert!(
        matches!(&error, ResolutionError::ProposalMismatch { .. }),
        "{error}"
    );

    // One byte different is a different proposal.
    let mut edited = ResolutionV1::draft(
        "Resolution 1",
        authority(&chain, &held, 1),
        ResolutionKindV1::Amendment {
            target: AmendmentTargetV1::ShareStructure,
            body: register_v2().replace("shares=\"700\"", "shares=\"701\""),
        },
    );
    assert!(matches!(
        edited.finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-02T09:00:00Z"),
            chain.next_timestamp()
        ),
        Err(ResolutionError::ProposalMismatch { .. })
    ));

    // A declarative resolution naming another document.
    let mut wrong_document = ResolutionV1::draft(
        "Resolution 1",
        authority(&chain, &held, 1),
        ResolutionKindV1::Declarative {
            document_digest: Hash::from_bytes([0xee; 32]),
        },
    );
    assert!(matches!(
        wrong_document.finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-02T09:00:00Z"),
            chain.next_timestamp()
        ),
        Err(ResolutionError::ProposalMismatch { .. })
    ));

    // But the approved register, with a declaration and whitespace around it, is the
    // same proposal: normalisation matches what the ledger stores.
    let mut reformatted = ResolutionV1::draft(
        "Resolution 1",
        authority(&chain, &held, 1),
        ResolutionKindV1::Amendment {
            target: AmendmentTargetV1::ShareStructure,
            body: format!("<?xml version=\"1.0\"?>\n{}\n", register_v2()),
        },
    );
    assert_eq!(
        chain.store.head().expect("head"),
        head,
        "nothing written yet"
    );
    reformatted
        .finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-02T09:00:00Z"),
            chain.next_timestamp(),
        )
        .expect("the same proposal");
}

#[test]
fn a_resolution_cannot_be_executed_twice() {
    let chain = founded();
    let held = hold(
        &chain,
        &[("Buy out carol", proposal_digest(&register_v2()), true)],
    );
    let mut resolution = ResolutionV1::draft(
        "Resolution 1",
        authority(&chain, &held, 1),
        ResolutionKindV1::Amendment {
            target: AmendmentTargetV1::ShareStructure,
            body: register_v2(),
        },
    );
    resolution
        .finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-02T09:00:00Z"),
            chain.next_timestamp(),
        )
        .expect("finalize");
    let executed = resolution
        .execute(
            &chain.store,
            &key(9),
            &notary("2026-06-02T10:00:00Z"),
            chain.next_timestamp(),
        )
        .expect("execute");

    // The same resolution again: the state machine refuses first.
    let error = resolution
        .execute(
            &chain.store,
            &key(9),
            &notary("2026-06-02T11:00:00Z"),
            chain.next_timestamp(),
        )
        .expect_err("twice");
    assert!(
        matches!(
            &error,
            ResolutionError::InvalidTransition {
                from: ResolutionStatusV1::Executed,
                ..
            }
        ),
        "{error}"
    );

    // A copy of the resolution that does not know it was executed: the chain refuses.
    let copy = {
        use prunella_canonical::Canonical;
        let mut fresh = ResolutionV1::draft(
            "Resolution 1",
            authority(&chain, &held, 1),
            ResolutionKindV1::Amendment {
                target: AmendmentTargetV1::ShareStructure,
                body: register_v2(),
            },
        );
        // Rewind to just after finalisation by re-reading the state as it then was.
        let bytes = resolution.canonical_bytes();
        let mut decoded = ResolutionV1::from_canonical_bytes(&bytes).expect("decode");
        core::mem::swap(&mut fresh, &mut decoded);
        fresh
    };
    // The decoded copy is Executed too, so drive a genuinely independent attempt: a
    // second resolution on the same passed vote.
    assert_eq!(copy.status(), ResolutionStatusV1::Executed);
    let mut second = ResolutionV1::draft(
        "Resolution 1 again",
        authority(&chain, &held, 1),
        ResolutionKindV1::Amendment {
            target: AmendmentTargetV1::ShareStructure,
            body: register_v2(),
        },
    );
    second
        .finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-03T09:00:00Z"),
            chain.next_timestamp(),
        )
        .expect("a second resolution may be recorded");
    let head = chain.store.head().expect("head");
    let error = second
        .execute(
            &chain.store,
            &key(9),
            &notary("2026-06-03T10:00:00Z"),
            chain.next_timestamp(),
        )
        .expect_err("the base moved");
    // The register it approved for replacement is no longer the one in force.
    assert!(
        matches!(&error, ResolutionError::StaleBase { .. }),
        "{error}"
    );
    assert_eq!(chain.store.head().expect("head"), head, "nothing written");
    assert_eq!(
        company_now(&chain.store).unwrap().shares.tx_id,
        executed.amendment_tx
    );

    // And the one execution that exists still verifies.
    assert!(
        verify_execution(&chain.store, &executed.execution_tx)
            .unwrap()
            .is_valid()
    );
}

#[test]
fn a_resolution_passed_against_a_company_that_has_since_changed_is_refused() {
    let chain = founded();
    let held = hold(
        &chain,
        &[("Buy out carol", proposal_digest(&register_v2()), true)],
    );
    let mut resolution = ResolutionV1::draft(
        "Resolution 1",
        authority(&chain, &held, 1),
        ResolutionKindV1::Amendment {
            target: AmendmentTargetV1::ShareStructure,
            body: register_v2(),
        },
    );
    resolution
        .finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-02T09:00:00Z"),
            chain.next_timestamp(),
        )
        .expect("finalize");

    // Someone amends the register directly between the vote and the execution.
    let current = company_now(&chain.store)
        .unwrap()
        .provider_of(RecordKindV1::ShareStructure);
    publish(
        &chain.store,
        &key(9),
        RecordKindV1::ShareStructure,
        &register().replace("shares=\"200\"", "shares=\"201\""),
        Some(current),
        &notary("2026-06-02T09:30:00Z"),
        chain.next_timestamp(),
    )
    .expect("direct amendment");

    let head = chain.store.head().expect("head");
    let error = resolution
        .execute(
            &chain.store,
            &key(9),
            &notary("2026-06-02T10:00:00Z"),
            chain.next_timestamp(),
        )
        .expect_err("stale base");
    assert!(
        matches!(
            &error,
            ResolutionError::StaleBase {
                target: AmendmentTargetV1::ShareStructure,
                ..
            }
        ),
        "{error}"
    );
    assert!(error.to_string().contains("has since changed"), "{error}");
    assert_eq!(chain.store.head().expect("head"), head, "nothing written");
    assert_eq!(resolution.status(), ResolutionStatusV1::Finalized);

    // A rules resolution is just as strict about its own part.
    let rules_held = hold(
        &chain,
        &[("Adopt two thirds", proposal_digest(&channels_v2()), true)],
    );
    let mut rules = ResolutionV1::draft(
        "Resolution 1",
        authority(&chain, &rules_held, 1),
        ResolutionKindV1::Amendment {
            target: AmendmentTargetV1::DecisionChannels,
            body: channels_v2(),
        },
    );
    rules
        .finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-04T09:00:00Z"),
            chain.next_timestamp(),
        )
        .expect("finalize");
    // The register moved again, but the rules did not: this resolution still executes.
    let current = company_now(&chain.store)
        .unwrap()
        .provider_of(RecordKindV1::ShareStructure);
    publish(
        &chain.store,
        &key(9),
        RecordKindV1::ShareStructure,
        &register().replace("shares=\"200\"", "shares=\"202\""),
        Some(current),
        &notary("2026-06-04T09:30:00Z"),
        chain.next_timestamp(),
    )
    .expect("another direct amendment");
    rules
        .execute(
            &chain.store,
            &key(9),
            &notary("2026-06-04T10:00:00Z"),
            chain.next_timestamp(),
        )
        .expect("the rules were untouched");
}

#[test]
fn every_invalid_transition_is_reported_with_both_ends() {
    let chain = founded();
    let held = hold(
        &chain,
        &[("Buy out carol", proposal_digest(&register_v2()), true)],
    );
    let mut resolution = ResolutionV1::draft(
        "Resolution 1",
        authority(&chain, &held, 1),
        ResolutionKindV1::Amendment {
            target: AmendmentTargetV1::ShareStructure,
            body: register_v2(),
        },
    );
    let transition = |result: Result<(), ResolutionError>, from: ResolutionStatusV1| match result
        .expect_err("invalid transition")
    {
        ResolutionError::InvalidTransition { from: f, to } => {
            assert_eq!(f, from);
            assert!(!to.is_empty());
        }
        other => panic!("wrong error: {other}"),
    };

    // Draft: finalize only.
    transition(
        resolution
            .execute(
                &chain.store,
                &key(9),
                &notary("2026-06-02T10:00:00Z"),
                chain.next_timestamp(),
            )
            .map(|_| ()),
        ResolutionStatusV1::Draft,
    );
    resolution
        .finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-02T09:00:00Z"),
            chain.next_timestamp(),
        )
        .expect("finalize");
    // Finalized: execute only.
    transition(
        resolution
            .finalize(
                &chain.store,
                &key(9),
                &notary("2026-06-02T09:00:00Z"),
                chain.next_timestamp(),
            )
            .map(|_| ()),
        ResolutionStatusV1::Finalized,
    );
    resolution
        .execute(
            &chain.store,
            &key(9),
            &notary("2026-06-02T10:00:00Z"),
            chain.next_timestamp(),
        )
        .expect("execute");
    // Executed: nothing.
    transition(
        resolution
            .finalize(
                &chain.store,
                &key(9),
                &notary("2026-06-02T09:00:00Z"),
                chain.next_timestamp(),
            )
            .map(|_| ()),
        ResolutionStatusV1::Executed,
    );
    transition(
        resolution
            .execute(
                &chain.store,
                &key(9),
                &notary("2026-06-02T10:00:00Z"),
                chain.next_timestamp(),
            )
            .map(|_| ()),
        ResolutionStatusV1::Executed,
    );
    assert_eq!(
        resolution
            .execute(&chain.store, &key(9), &notary("2026-06-02T10:00:00Z"), 1)
            .unwrap_err()
            .to_string(),
        "cannot execute a resolution that is executed"
    );
}

#[test]
fn the_resolution_state_round_trips_through_canonical_bytes() {
    use prunella_canonical::Canonical;
    let chain = founded();
    let held = hold(
        &chain,
        &[("Buy out carol", proposal_digest(&register_v2()), true)],
    );
    let reload = |r: &ResolutionV1| {
        ResolutionV1::from_canonical_bytes(&r.canonical_bytes()).expect("decode")
    };
    let mut resolution = ResolutionV1::draft(
        "Resolution 1",
        authority(&chain, &held, 1),
        ResolutionKindV1::Amendment {
            target: AmendmentTargetV1::ShareStructure,
            body: register_v2(),
        },
    );
    assert_eq!(reload(&resolution), resolution);
    resolution
        .finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-02T09:00:00Z"),
            chain.next_timestamp(),
        )
        .expect("finalize");
    let mut resolution = reload(&resolution);
    let executed = resolution
        .execute(
            &chain.store,
            &key(9),
            &notary("2026-06-02T10:00:00Z"),
            chain.next_timestamp(),
        )
        .expect("execute");
    assert_eq!(reload(&resolution), resolution);
    assert!(
        verify_execution(&chain.store, &executed.execution_tx)
            .unwrap()
            .is_valid()
    );
}

// ---------------------------------------------------------------------------------
// Tampering, seen from the chain.
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
        nonce: head.height.value() + 900,
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

fn resolution_failures(report: &ResolutionVerificationV1) -> Vec<String> {
    report.failures().map(|check| check.name.clone()).collect()
}

fn execution_failures(report: &ExecutionVerificationV1) -> Vec<String> {
    report.failures().map(|check| check.name.clone()).collect()
}

#[test]
fn a_forged_resolution_is_caught_by_the_chain() {
    let chain = founded();
    let digest = proposal_digest(&register_v2());
    let held = hold(&chain, &[("Buy out carol", digest, false)]);
    let vote_tx = vote_of(&chain, &held, 1);

    // A resolution composed by hand on a REJECTED vote, published directly.
    let payload = compose_resolution(
        &acme(),
        &notary("2026-06-02T09:00:00Z"),
        "Resolution 1",
        &AuthorityV1::Collective {
            channel: "shareholders".to_owned(),
            meeting_tx: held.meeting_tx,
            item_number: 1,
            vote_tx,
        },
        &ResolutionKindV1::Amendment {
            target: AmendmentTargetV1::ShareStructure,
            body: register_v2(),
        },
    )
    .expect("compose");
    let forged = append_raw(&chain.store, "irena.resolution.v1", payload.into_bytes());
    let report = verify_resolution(&chain.store, &forged).expect("verify");
    assert!(!report.is_valid());
    assert_eq!(resolution_failures(&report), ["VotePassed"], "{report:#?}");

    // One claiming a proposal the vote never approved.
    let passed = hold(&chain, &[("Buy out carol", digest, true)]);
    let payload = compose_resolution(
        &acme(),
        &notary("2026-06-02T09:00:00Z"),
        "Resolution 1",
        &AuthorityV1::Collective {
            channel: "shareholders".to_owned(),
            meeting_tx: passed.meeting_tx,
            item_number: 1,
            vote_tx: vote_of(&chain, &passed, 1),
        },
        &ResolutionKindV1::Amendment {
            target: AmendmentTargetV1::ShareStructure,
            body: register(),
        },
    )
    .expect("compose");
    let mismatched = append_raw(&chain.store, "irena.resolution.v1", payload.into_bytes());
    let report = verify_resolution(&chain.store, &mismatched).expect("verify");
    assert_eq!(
        resolution_failures(&report),
        ["ProposalMatches"],
        "{report:#?}"
    );

    // One pointing at another item's vote.
    let two = hold(
        &chain,
        &[
            ("Buy out carol", digest, true),
            ("Adopt two thirds", proposal_digest(&channels_v2()), true),
        ],
    );
    let payload = compose_resolution(
        &acme(),
        &notary("2026-06-02T09:00:00Z"),
        "Resolution 1",
        &AuthorityV1::Collective {
            channel: "shareholders".to_owned(),
            meeting_tx: two.meeting_tx,
            item_number: 1,
            vote_tx: vote_of(&chain, &two, 2),
        },
        &ResolutionKindV1::Amendment {
            target: AmendmentTargetV1::ShareStructure,
            body: register_v2(),
        },
    )
    .expect("compose");
    let crossed = append_raw(&chain.store, "irena.resolution.v1", payload.into_bytes());
    let report = verify_resolution(&chain.store, &crossed).expect("verify");
    assert_eq!(
        resolution_failures(&report),
        ["VoteAnsweredTheItem"],
        "{report:#?}"
    );

    // And things that are not resolutions at all.
    let garbage = append_raw(
        &chain.store,
        "irena.resolution.v1",
        b"not a resolution".to_vec(),
    );
    let report = verify_resolution(&chain.store, &garbage).expect("verify");
    assert_eq!(resolution_failures(&report), ["Decodes"]);
    assert!(report.record.is_none());
    let elsewhere = append_raw(&chain.store, "app.other", b"anything".to_vec());
    let report = verify_resolution(&chain.store, &elsewhere).expect("verify");
    assert_eq!(resolution_failures(&report), ["Decodes"]);
    assert!(matches!(
        verify_resolution(&chain.store, &TxId::from_hash(Hash::from_bytes([0x99; 32]))),
        Err(ResolutionError::NoSuchTransaction { .. })
    ));
}

#[test]
fn a_forged_execution_is_caught_by_the_chain() {
    let chain = founded();
    let digest = proposal_digest(&register_v2());
    let held = hold(&chain, &[("Buy out carol", digest, true)]);
    let rules_held = hold(
        &chain,
        &[("Adopt two thirds", proposal_digest(&channels_v2()), true)],
    );
    let resolution = resolve_amendment(
        &chain,
        &held,
        1,
        AmendmentTargetV1::ShareStructure,
        &register_v2(),
        "Resolution 1",
    );
    let good = resolution.executed().expect("executed");
    let genuine = read_execution_record(
        core::str::from_utf8(
            &chain
                .store
                .get_transaction(&good.execution_tx)
                .unwrap()
                .unwrap()
                .transaction
                .payload,
        )
        .unwrap(),
    )
    .expect("read")
    .execution;

    let republish = |execution: &irena_resolution::ResolutionExecutionV1| {
        let payload = compose_execution(&acme(), &notary("2026-06-05T09:00:00Z"), execution)
            .expect("compose");
        append_raw(&chain.store, "irena.execution.v1", payload.into_bytes())
    };

    // A second execution record for a resolution already executed.
    let duplicate = republish(&genuine);
    let report = verify_execution(&chain.store, &duplicate).expect("verify");
    assert!(!report.is_valid());
    assert_eq!(execution_failures(&report), ["ExecutedOnce"], "{report:#?}");

    // An execution naming an amendment that is not the resolution's body.
    let mut other_body = genuine.clone();
    other_body.amendment_tx = company_now(&chain.store).unwrap().genesis_tx_id;
    let report = verify_execution(&chain.store, &republish(&other_body)).expect("verify");
    assert!(
        execution_failures(&report).contains(&"AmendmentExists".to_owned()),
        "{report:#?}"
    );

    // An execution claiming a digest the vote never approved.
    let mut wrong_digest = genuine.clone();
    wrong_digest.body_digest = Hash::from_bytes([0xab; 32]);
    let report = verify_execution(&chain.store, &republish(&wrong_digest)).expect("verify");
    assert!(
        execution_failures(&report).contains(&"AmendmentMatchesResolution".to_owned()),
        "{report:#?}"
    );

    // An execution claiming it replaced something else.
    let mut wrong_base = genuine.clone();
    wrong_base.replaced_tx = TxId::from_hash(Hash::from_bytes([0x33; 32]));
    let report = verify_execution(&chain.store, &republish(&wrong_base)).expect("verify");
    assert!(
        execution_failures(&report).contains(&"AmendmentReplacedApprovedBase".to_owned()),
        "{report:#?}"
    );

    // An execution whose target is not what the resolution authorises.
    let mut wrong_target = genuine.clone();
    wrong_target.target = AmendmentTargetV1::DecisionChannels;
    let report = verify_execution(&chain.store, &republish(&wrong_target)).expect("verify");
    assert!(
        execution_failures(&report).contains(&"ResolutionAuthorisesThis".to_owned()),
        "{report:#?}"
    );

    // An execution resting on a declarative resolution, which authorises nothing.
    let document = Hash::from_bytes([0xdd; 32]);
    let declarative_meeting = hold(&chain, &[("Receive the report", document, true)]);
    let mut declarative = ResolutionV1::draft(
        "Resolution 1",
        authority(&chain, &declarative_meeting, 1),
        ResolutionKindV1::Declarative {
            document_digest: document,
        },
    );
    let declarative_id = declarative
        .finalize(
            &chain.store,
            &key(9),
            &notary("2026-06-06T09:00:00Z"),
            chain.next_timestamp(),
        )
        .expect("finalize");
    let mut on_declarative = genuine.clone();
    on_declarative.resolution_id = declarative_id;
    let report = verify_execution(&chain.store, &republish(&on_declarative)).expect("verify");
    assert!(
        execution_failures(&report).contains(&"ResolutionAuthorisesThis".to_owned()),
        "{report:#?}"
    );
    assert!(
        report.resolution.as_ref().unwrap().is_valid(),
        "the resolution itself is fine"
    );

    // An execution resting on a resolution that never existed.
    let mut nowhere = genuine;
    nowhere.resolution_id = ResolutionIdV1::from_tx(TxId::from_hash(Hash::from_bytes([0x55; 32])));
    let report = verify_execution(&chain.store, &republish(&nowhere)).expect("verify");
    assert_eq!(
        execution_failures(&report),
        ["ResolutionVerifies"],
        "{report:#?}"
    );

    // Non-records.
    let garbage = append_raw(
        &chain.store,
        "irena.execution.v1",
        b"not an execution".to_vec(),
    );
    assert_eq!(
        execution_failures(&verify_execution(&chain.store, &garbage).unwrap()),
        ["Decodes"]
    );
    // The rules meeting was held but never resolved: nothing was executed from it.
    assert!(
        irena_meeting::verify_meeting(&chain.store, &rules_held.meeting_tx)
            .unwrap()
            .is_valid()
    );
}

#[test]
fn an_amendment_published_without_a_resolution_still_reconstructs_but_has_no_authority() {
    let chain = founded();
    // Nothing in the company layer requires a resolution: a notarised amendment is
    // still a valid amendment. What a resolution adds is the proof of authority.
    let current = company_now(&chain.store)
        .unwrap()
        .provider_of(RecordKindV1::ShareStructure);
    let direct = publish(
        &chain.store,
        &key(9),
        RecordKindV1::ShareStructure,
        &register_v2(),
        Some(current),
        &notary("2026-06-02T09:00:00Z"),
        chain.next_timestamp(),
    )
    .expect("direct amendment");
    let state = company_now(&chain.store).expect("state");
    assert_eq!(state.shares.tx_id, direct.tx_id);
    // No execution record points at it, so nothing claims it was authorised.
    assert!(
        irena_resolution::verify_execution(&chain.store, &direct.tx_id)
            .unwrap()
            .failures()
            .any(|check| check.name == "Decodes")
    );
}

// ---------------------------------------------------------------------------------
// The records themselves.
// ---------------------------------------------------------------------------------

#[test]
fn both_records_are_nested_readable_xml_on_the_chain() {
    let chain = founded();
    let held = hold(
        &chain,
        &[("Buy out carol", proposal_digest(&register_v2()), true)],
    );
    let resolution = resolve_amendment(
        &chain,
        &held,
        1,
        AmendmentTargetV1::ShareStructure,
        &register_v2(),
        "Resolution 1: buy out carol",
    );
    let id = resolution.id().unwrap();
    let executed = resolution.executed().unwrap();

    let text = |tx: TxId| {
        String::from_utf8(
            chain
                .store
                .get_transaction(&tx)
                .unwrap()
                .unwrap()
                .transaction
                .payload,
        )
        .expect("utf-8")
    };
    let resolution_text = text(id.tx_id());
    assert!(
        resolution_text.starts_with("<irena-resolution version=\"1.0\""),
        "{resolution_text}"
    );
    assert!(
        resolution_text.contains(&register_v2()),
        "the body, verbatim"
    );
    assert!(prunella_xml::is_single_element(&resolution_text));
    let execution_text = text(executed.execution_tx);
    assert!(
        execution_text.starts_with("<irena-execution version=\"1.0\""),
        "{execution_text}"
    );
    assert!(prunella_xml::is_single_element(&execution_text));

    // Read back: the body recovered from the record is byte for byte the one approved.
    let record = read_resolution_record(&resolution_text).expect("read");
    assert_eq!(record.kind.body(), Some(register_v2().as_str()));
    assert_eq!(
        record.kind.approved_digest(),
        proposal_digest(&register_v2())
    );
    assert_eq!(record.title, "Resolution 1: buy out carol");
    assert!(
        matches!(
            record.authority,
            AuthorityV1::Collective { item_number: 1, ref channel, .. } if channel == "shareholders"
        ),
        "{:?}",
        record.authority
    );

    // The export shows both, readable, and re-imports to the same verdict.
    let document =
        prunella_xml::export(&chain.store, &prunella_xml::ExportRequest::full()).expect("export");
    let xml = prunella_xml::write_document(&document).expect("render");
    assert_eq!(
        xml.matches("<payload encoding=\"xml\"><irena-resolution ")
            .count(),
        1,
        "{xml}"
    );
    assert_eq!(
        xml.matches("<payload encoding=\"xml\"><irena-execution ")
            .count(),
        1,
        "{xml}"
    );
    let restored_path = chain._dir.path().join("restored.chain");
    let (restored, _) =
        prunella_xml::restore(&restored_path, &prunella_xml::read_document(&xml).unwrap())
            .expect("restore");
    assert!(
        verify_execution(&restored, &executed.execution_tx)
            .unwrap()
            .is_valid()
    );
    assert_eq!(
        reconstruct(&restored, restored.head().unwrap().height)
            .unwrap()
            .shares
            .value
            .len(),
        2
    );
}

#[test]
fn resolution_records_are_read_strictly() {
    let chain = founded();
    let held = hold(
        &chain,
        &[("Buy out carol", proposal_digest(&register_v2()), true)],
    );
    let good = compose_resolution(
        &acme(),
        &notary("2026-06-02T09:00:00Z"),
        "Resolution 1",
        &authority(&chain, &held, 1),
        &ResolutionKindV1::Amendment {
            target: AmendmentTargetV1::ShareStructure,
            body: register_v2(),
        },
    )
    .expect("compose");

    for (label, xml) in [
        (
            "unknown version",
            good.replace("version=\"1.0\"", "version=\"2.0\""),
        ),
        (
            "unknown kind",
            good.replace("kind=\"amendment\"", "kind=\"advisory\""),
        ),
        (
            "unknown target",
            good.replace("target=\"share-structure\"", "target=\"identity\""),
        ),
        (
            "unknown attribute",
            good.replace("<irena-resolution ", "<irena-resolution chair=\"x\" "),
        ),
        (
            "unknown child",
            good.replace("</irena-resolution>", "<minutes/></irena-resolution>"),
        ),
        (
            "empty title",
            good.replace("title=\"Resolution 1\"", "title=\"\""),
        ),
        ("bad item", good.replace("item=\"1\"", "item=\"one\"")),
        ("bad vote id", good.replacen("vote=\"", "vote=\"zz", 1)),
        ("no notarisation", {
            let start = good.find("<notarisation").unwrap();
            let end = good[start..].find("/>").unwrap() + start + 2;
            format!("{}{}", &good[..start], &good[end..])
        }),
        (
            "an amendment with a document digest",
            good.replace(
                "target=\"share-structure\"",
                "target=\"share-structure\" document-digest=\"aa\"",
            ),
        ),
        (
            "two bodies",
            good.replace(
                "</amendment>",
                "</amendment><amendment><share-structure/></amendment>",
            ),
        ),
        (
            "an invalid body",
            good.replace("shares=\"700\"", "shares=\"0\""),
        ),
        (
            "a body of the wrong kind",
            good.replace(&register_v2(), &channels_v2()),
        ),
    ] {
        assert!(
            read_resolution_record(&xml).is_err(),
            "{label} should have been refused"
        );
    }

    // A declarative record, and what it may not carry.
    let declarative = compose_resolution(
        &acme(),
        &notary("2026-06-02T09:00:00Z"),
        "Resolution 1",
        &authority(&chain, &held, 1),
        &ResolutionKindV1::Declarative {
            document_digest: Hash::from_bytes([0xdd; 32]),
        },
    )
    .expect("compose");
    assert!(read_resolution_record(&declarative).is_ok());
    for (label, xml) in [
        (
            "a declarative with a target",
            declarative.replace(
                "document-digest=",
                "target=\"share-structure\" document-digest=",
            ),
        ),
        (
            "a declarative with no digest",
            declarative.replace(
                &format!(" document-digest=\"{}\"", Hash::from_bytes([0xdd; 32])),
                "",
            ),
        ),
    ] {
        assert!(
            read_resolution_record(&xml).is_err(),
            "{label} should have been refused"
        );
    }

    // Composition refuses what reading would.
    assert!(
        compose_resolution(
            &acme(),
            &notary("2026-06-02T09:00:00Z"),
            "   ",
            &authority(&chain, &held, 1),
            &ResolutionKindV1::Declarative {
                document_digest: Hash::from_bytes([1; 32])
            },
        )
        .is_err()
    );
    assert!(
        compose_resolution(
            &acme(),
            &notary("2026-06-02T09:00:00Z"),
            "Resolution 1",
            &authority(&chain, &held, 1),
            &ResolutionKindV1::Amendment {
                target: AmendmentTargetV1::DecisionChannels,
                body: register_v2(),
            },
        )
        .is_err(),
        "a register is not a rules document"
    );

    // The serde form is useful to tooling.
    let record = read_resolution_record(&good).expect("read");
    let json = serde_json::to_value(&record).expect("json");
    assert_eq!(json["company"], "acme");
    assert_eq!(json["kind"]["kind"], "amendment");
    assert_eq!(json["kind"]["target"], "share-structure");
    assert_eq!(json["notarisation"]["id"], "notary-07");
}
