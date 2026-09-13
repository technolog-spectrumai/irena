//! Channels resolved against a real company, and an individual decision from draft to
//! a verified record on the chain — with everything that must be refused on the way.

use irena_core::{
    ChannelIdV1, CompanyIdV1, NotarisationV1, NotaryIdV1, NotaryTimeV1, RecordKindV1,
};
use irena_decision::{
    DECISION_NAMESPACE, DecisionCheckNameV1, DecisionError, DecisionStatusV1, DecisionV1,
    FinalDecisionRecordV1, resolve_channel, verify_decision,
};
use irena_ledger::{company_now, genesis_with_company, publish, reconstruct};
use prunella_canonical::Canonical;
use prunella_core::{
    BlockHeight, Hash, Namespace, NetworkId, SchemaVersion, TransactionDraft, TxId,
};
use prunella_crypto::SigningKey;
use prunella_store::LocalChainStore;
use std::path::Path;
use tempfile::TempDir;

fn key(seed: u8) -> SigningKey {
    SigningKey::from_seed([seed; 32])
}

fn channel(id: &str) -> ChannelIdV1 {
    ChannelIdV1::new(id).expect("channel id")
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

fn example(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

struct Chain {
    _dir: TempDir,
    store: LocalChainStore,
}

/// A company founded from one of the example genesis documents.
fn founded(genesis: &str) -> Chain {
    let dir = TempDir::new().expect("temp dir");
    let spec = genesis_with_company(
        NetworkId::new("acme-net").expect("n"),
        &key(9),
        &CompanyIdV1::new("acme").expect("company"),
        &example(genesis),
        &notary("2026-01-10T09:00:00Z"),
        0,
    )
    .expect("spec");
    let store = LocalChainStore::init_genesis(dir.path().join("acme.chain"), spec).expect("create");
    Chain { _dir: dir, store }
}

/// The three-channel company: shareholders, board, ceo.
fn acme() -> Chain {
    founded("genesis-three-channels.xml")
}

fn next_timestamp(store: &LocalChainStore) -> u64 {
    let head = store.head().expect("head");
    let block = store
        .get_block(head.height)
        .expect("block")
        .expect("stored");
    block.header.timestamp_millis + 1000
}

/// Publishes an amendment superseding whatever currently provides the part.
fn amend(chain: &Chain, kind: RecordKindV1, body: &str) -> TxId {
    let current = company_now(&chain.store).expect("state").provider_of(kind);
    let ts = next_timestamp(&chain.store);
    publish(
        &chain.store,
        &key(9),
        kind,
        body,
        Some(current),
        &notary("2026-02-01T09:00:00Z"),
        ts,
    )
    .expect("publish")
    .tx_id
}

fn proposal() -> Hash {
    Hash::from_bytes([0x77; 32])
}

/// A frozen, signed decision through the ceo channel at the chain head.
fn signed(chain: &Chain) -> DecisionV1 {
    let mut decision = DecisionV1::draft("appoint auditors", proposal());
    let head = chain.store.head().unwrap().height;
    decision
        .freeze(&chain.store, head, &channel("ceo"))
        .expect("freeze");
    decision.sign(&key(4)).expect("sign");
    decision
}

fn append_raw(store: &LocalChainStore, namespace: &str, payload: Vec<u8>) -> TxId {
    let head = store.head().unwrap();
    let parent = store.get_block(head.height).unwrap().unwrap();
    let height = head.height.next().unwrap();
    let signer = key(9);
    let transaction = signer.sign_transaction(TransactionDraft {
        namespace: Namespace::new(namespace).unwrap(),
        schema_version: SchemaVersion(1),
        payload,
        signer: signer.public_key(),
        nonce: height.value(),
    });
    let tx_id = transaction.id;
    let block = parent
        .header
        .child_draft(vec![transaction], parent.header.timestamp_millis + 1000)
        .unwrap()
        .build()
        .unwrap();
    store.append_block(block).unwrap();
    tx_id
}

#[test]
fn three_configurations_resolve_through_one_function() {
    let chain = acme();
    let state = company_now(&chain.store).unwrap();

    let shareholders = resolve_channel(&state, &channel("shareholders")).expect("shareholders");
    let weights: Vec<(&str, u64, bool)> = shareholders
        .actors
        .actors
        .iter()
        .map(|a| (a.id.as_str(), a.weight.value(), a.can_sign))
        .collect();
    assert_eq!(
        weights,
        [
            ("alice", 500, true),
            ("bob", 300, true),
            ("carol", 200, false)
        ],
        "share register: weight = shares, a keyless holder counts but cannot sign"
    );
    assert!(shareholders.rules().is_some());
    assert!(shareholders.sole_actor().is_none());
    assert_eq!(shareholders.channels_tx_id, state.genesis_tx_id);
    assert_eq!(shareholders.shares_tx_id, state.genesis_tx_id);

    let board = resolve_channel(&state, &channel("board")).expect("board");
    let weights: Vec<(&str, u64, bool)> = board
        .actors
        .actors
        .iter()
        .map(|a| (a.id.as_str(), a.weight.value(), a.can_sign))
        .collect();
    assert_eq!(
        weights,
        [("chen", 2, true), ("okafor", 1, true), ("vance", 1, false)],
        "roster: the declared weight, default 1"
    );
    assert_eq!(board.actors.total_weight.value(), 4);
    assert!(board.rules().is_some());
    assert_ne!(
        board.rules(),
        shareholders.rules(),
        "each channel has its own rules"
    );

    let ceo = resolve_channel(&state, &channel("ceo")).expect("ceo");
    let actor = ceo.sole_actor().expect("individual");
    assert_eq!(actor.id.as_str(), "chen");
    assert_eq!(actor.key, Some(key(4).public_key()));
    assert!(ceo.rules().is_none());

    let error = resolve_channel(&state, &channel("treasury")).expect_err("no such channel");
    assert!(
        matches!(error, DecisionError::NoSuchChannel { ref channel, height } if channel.as_str() == "treasury" && height == BlockHeight::GENESIS),
        "{error}"
    );
}

#[test]
fn a_decision_runs_from_draft_to_a_verified_record() {
    let chain = acme();
    let mut decision = DecisionV1::draft("appoint auditors", proposal());
    assert_eq!(decision.status(), DecisionStatusV1::Draft);
    assert!(decision.id().is_none());

    let snapshot = decision
        .freeze(&chain.store, BlockHeight::GENESIS, &channel("ceo"))
        .expect("freeze")
        .clone();
    assert_eq!(decision.status(), DecisionStatusV1::Frozen);
    assert_eq!(snapshot.company, "acme");
    assert_eq!(snapshot.channel, "ceo");
    assert_eq!(snapshot.actor, "chen");
    assert_eq!(snapshot.key, key(4).public_key());
    assert_eq!(snapshot.proposal_digest, proposal());
    let genesis = company_now(&chain.store).unwrap().genesis_tx_id;
    assert_eq!(snapshot.genesis_tx_id, genesis);
    assert_eq!(snapshot.shares_tx_id, genesis);
    assert_eq!(snapshot.channels_tx_id, genesis);
    assert_eq!(decision.id(), Some(snapshot.id()));

    decision.sign(&key(4)).expect("sign");
    assert_eq!(decision.status(), DecisionStatusV1::Signed);
    let record = decision.final_record().expect("record");
    record.check_signature().expect("signature verifies");
    assert_eq!(record.decision_id, snapshot.id());

    let finalized = decision
        .finalize(&chain.store, &key(9), 5000)
        .expect("finalize");
    assert_eq!(decision.status(), DecisionStatusV1::Finalized);
    assert_eq!(finalized.height, BlockHeight(1));
    assert_eq!(finalized.record, record);
    assert_eq!(
        decision.finalized(),
        Some((finalized.tx_id, BlockHeight(1)))
    );

    // On the chain, under the decision namespace, byte for byte.
    let stored = chain
        .store
        .get_transaction(&finalized.tx_id)
        .unwrap()
        .expect("stored");
    assert_eq!(stored.transaction.namespace.as_str(), DECISION_NAMESPACE);
    assert_eq!(stored.transaction.payload, record.canonical_bytes());

    // Verified from nothing but the chain and the transaction id.
    let report = verify_decision(&chain.store, &finalized.tx_id).expect("verify");
    assert!(report.is_valid(), "{report:#?}");
    let names: Vec<DecisionCheckNameV1> = report.checks.iter().map(|c| c.name).collect();
    assert_eq!(
        names,
        [
            DecisionCheckNameV1::Decodes,
            DecisionCheckNameV1::DecisionIdDerives,
            DecisionCheckNameV1::SnapshotPrecedesRecord,
            DecisionCheckNameV1::RecordsResolve,
            DecisionCheckNameV1::ChannelIsIndividual,
            DecisionCheckNameV1::ActorResolves,
            DecisionCheckNameV1::SignatureVerifies,
        ]
    );
    assert_eq!(report.record, Some(record));

    // The company itself is untouched: a decision decides, it does not change.
    let state = company_now(&chain.store).unwrap();
    assert_eq!(
        state.applied.len(),
        1,
        "only the genesis is a company record"
    );
    assert_eq!(state.channels.tx_id, genesis);
}

#[test]
fn a_decision_needs_an_individual_channel_with_a_signing_actor() {
    let chain = acme();
    let at = BlockHeight::GENESIS;

    let error = DecisionV1::draft("x", proposal())
        .freeze(&chain.store, at, &channel("board"))
        .expect_err("collective");
    assert!(
        matches!(error, DecisionError::NotIndividual { .. }),
        "{error}"
    );

    let error = DecisionV1::draft("x", proposal())
        .freeze(&chain.store, at, &channel("shareholders"))
        .expect_err("collective");
    assert!(
        matches!(error, DecisionError::NotIndividual { .. }),
        "{error}"
    );

    let error = DecisionV1::draft("x", proposal())
        .freeze(&chain.store, at, &channel("nobody"))
        .expect_err("unknown");
    assert!(
        matches!(error, DecisionError::NoSuchChannel { .. }),
        "{error}"
    );

    // Two members in an individual channel: refused at resolution, not at parse.
    let two = example("genesis-three-channels.xml");
    let channels = two[two.find("<decision-channels>").unwrap()
        ..two.find("</decision-channels>").unwrap() + "</decision-channels>".len()]
        .replace(
            r#"<member id="chen" key="ca93ac1705187071d67b83c7ff0efe8108e8ec4530575d7726879333dbdabe7c" name="M. Chen"/>"#,
            r#"<member id="chen" key="ca93ac1705187071d67b83c7ff0efe8108e8ec4530575d7726879333dbdabe7c" name="M. Chen"/><member id="vance"/>"#,
        );
    assert!(
        channels.contains(
            r#"<member id="vance"/>
      </actors>
    </channel>"#
        ) || channels.matches("vance").count() == 2,
        "fixture edited: {channels}"
    );
    amend(&chain, RecordKindV1::DecisionChannels, &channels);
    let head = chain.store.head().unwrap().height;
    let error = DecisionV1::draft("x", proposal())
        .freeze(&chain.store, head, &channel("ceo"))
        .expect_err("two actors");
    assert!(
        matches!(error, DecisionError::NotSingleActor { found: 2, .. }),
        "{error}"
    );
    // The same channel at the earlier height is still fine: the past does not move.
    DecisionV1::draft("x", proposal())
        .freeze(&chain.store, at, &channel("ceo"))
        .expect("one actor at genesis");

    // A sole actor with no key cannot sign, and freeze says so.
    let keyless = channels.replace(
        r#"<member id="chen" key="ca93ac1705187071d67b83c7ff0efe8108e8ec4530575d7726879333dbdabe7c" name="M. Chen"/><member id="vance"/>"#,
        r#"<member id="vance"/>"#,
    );
    amend(&chain, RecordKindV1::DecisionChannels, &keyless);
    let head = chain.store.head().unwrap().height;
    let error = DecisionV1::draft("x", proposal())
        .freeze(&chain.store, head, &channel("ceo"))
        .expect_err("no key");
    assert!(
        matches!(error, DecisionError::NoKey { ref actor, .. } if actor == "vance"),
        "{error}"
    );
}

#[test]
fn a_single_member_company_decides_through_its_register() {
    let chain = founded("genesis-single-member.xml");
    let owner = channel("owner");
    let state = company_now(&chain.store).unwrap();
    let resolved = resolve_channel(&state, &owner).expect("one holder, one actor");
    assert_eq!(resolved.sole_actor().unwrap().id.as_str(), "ada");

    let mut decision = DecisionV1::draft("open a bank account", proposal());
    decision
        .freeze(&chain.store, BlockHeight::GENESIS, &owner)
        .expect("freeze");
    decision.sign(&key(7)).expect("ada signs");
    let finalized = decision
        .finalize(&chain.store, &key(9), 5000)
        .expect("finalize");
    assert!(
        verify_decision(&chain.store, &finalized.tx_id)
            .unwrap()
            .is_valid()
    );

    // A second holder is admitted: the same channel no longer resolves to one actor.
    let register = format!(
        r#"<share-structure>
  <holder id="ada" key="{}" shares="1"/>
  <holder id="bea" key="{}" shares="1"/>
</share-structure>"#,
        key(7).public_key(),
        key(8).public_key()
    );
    amend(&chain, RecordKindV1::ShareStructure, &register);
    let head = chain.store.head().unwrap().height;
    let state = reconstruct(&chain.store, head).unwrap();
    let error = resolve_channel(&state, &owner).expect_err("two holders");
    assert!(
        matches!(error, DecisionError::NotSingleActor { found: 2, .. }),
        "{error}"
    );
    // The decision already on the chain still verifies: it pinned the earlier register.
    assert!(
        verify_decision(&chain.store, &finalized.tx_id)
            .unwrap()
            .is_valid()
    );
}

#[test]
fn only_the_frozen_actor_can_sign() {
    let chain = acme();
    let mut decision = DecisionV1::draft("x", proposal());
    decision
        .freeze(&chain.store, BlockHeight::GENESIS, &channel("ceo"))
        .unwrap();
    for wrong in [1, 2, 5, 9] {
        let error = decision.sign(&key(wrong)).expect_err("wrong key");
        assert!(
            matches!(error, DecisionError::WrongKey { ref actor } if actor == "chen"),
            "seed {wrong}: {error}"
        );
        assert_eq!(decision.status(), DecisionStatusV1::Frozen);
    }
    decision.sign(&key(4)).expect("the actor's key");
    assert_eq!(decision.status(), DecisionStatusV1::Signed);
}

#[test]
fn every_invalid_transition_is_reported_with_both_ends() {
    let chain = acme();
    let mut decision = DecisionV1::draft("x", proposal());
    let at = BlockHeight::GENESIS;

    let error = decision.sign(&key(4)).expect_err("sign a draft");
    assert!(
        matches!(
            error,
            DecisionError::InvalidTransition {
                from: DecisionStatusV1::Draft,
                to: "sign"
            }
        ),
        "{error}"
    );
    let error = decision
        .finalize(&chain.store, &key(9), 1)
        .expect_err("finalize a draft");
    assert!(matches!(
        error,
        DecisionError::InvalidTransition {
            from: DecisionStatusV1::Draft,
            to: "finalize"
        }
    ));
    assert!(decision.final_record().is_err());

    decision.freeze(&chain.store, at, &channel("ceo")).unwrap();
    let error = decision
        .freeze(&chain.store, at, &channel("ceo"))
        .expect_err("freeze twice");
    assert!(matches!(
        error,
        DecisionError::InvalidTransition {
            from: DecisionStatusV1::Frozen,
            to: "freeze"
        }
    ));
    let error = decision
        .finalize(&chain.store, &key(9), 1)
        .expect_err("finalize unsigned");
    assert!(matches!(
        error,
        DecisionError::InvalidTransition {
            from: DecisionStatusV1::Frozen,
            to: "finalize"
        }
    ));

    decision.sign(&key(4)).unwrap();
    let error = decision.sign(&key(4)).expect_err("sign twice");
    assert!(matches!(
        error,
        DecisionError::InvalidTransition {
            from: DecisionStatusV1::Signed,
            to: "sign"
        }
    ));
    decision.finalize(&chain.store, &key(9), 5000).unwrap();
    let error = decision
        .finalize(&chain.store, &key(9), 6000)
        .expect_err("finalize twice");
    assert!(matches!(
        error,
        DecisionError::InvalidTransition {
            from: DecisionStatusV1::Finalized,
            to: "finalize"
        }
    ));
    assert_eq!(
        chain.store.head().unwrap().height,
        BlockHeight(1),
        "exactly one record reached the chain"
    );
}

#[test]
fn amendments_after_freezing_change_nothing() {
    let chain = acme();
    let decision = signed(&chain);
    // The ceo channel is abolished after the freeze...
    amend(
        &chain,
        RecordKindV1::DecisionChannels,
        &example("channels-ceo-abolished.xml"),
    );
    let head = chain.store.head().unwrap().height;
    assert!(
        resolve_channel(&reconstruct(&chain.store, head).unwrap(), &channel("ceo")).is_err(),
        "the channel is gone at the head"
    );
    // ...and the decision, frozen before, still finalises and verifies: it pinned the
    // channel set it was taken under, and that record has not moved.
    let mut decision = decision;
    let finalized = decision
        .finalize(&chain.store, &key(9), next_timestamp(&chain.store))
        .expect("finalize against the frozen height");
    let report = verify_decision(&chain.store, &finalized.tx_id).unwrap();
    assert!(report.is_valid(), "{report:#?}");
    // A new decision through the abolished channel at the head is refused.
    let error = DecisionV1::draft("x", proposal())
        .freeze(&chain.store, head, &channel("ceo"))
        .expect_err("abolished");
    assert!(matches!(error, DecisionError::NoSuchChannel { .. }));
}

#[test]
fn the_decision_state_round_trips_through_canonical_bytes_between_steps() {
    let chain = acme();
    let mut decision = DecisionV1::draft("x", proposal());
    let bytes = decision.canonical_bytes();
    let mut restored = DecisionV1::from_canonical_bytes(&bytes).expect("decode");
    assert_eq!(restored, decision);
    restored
        .freeze(&chain.store, BlockHeight::GENESIS, &channel("ceo"))
        .unwrap();
    decision
        .freeze(&chain.store, BlockHeight::GENESIS, &channel("ceo"))
        .unwrap();
    assert_eq!(
        restored, decision,
        "the same inputs freeze to the same decision"
    );
    let mut restored = DecisionV1::from_canonical_bytes(&restored.canonical_bytes()).unwrap();
    restored.sign(&key(4)).unwrap();
    decision.sign(&key(4)).unwrap();
    assert_eq!(
        restored, decision,
        "Ed25519 is deterministic: the same signature on another machine"
    );
}

#[test]
fn every_tampered_field_is_caught_by_a_named_check() {
    let chain = acme();
    let mut decision = signed(&chain);
    let genuine = decision
        .finalize(&chain.store, &key(9), 5000)
        .unwrap()
        .record;

    let plant = |record: &FinalDecisionRecordV1| {
        append_raw(&chain.store, DECISION_NAMESPACE, record.canonical_bytes())
    };
    let failing = |tx: &TxId| -> Vec<DecisionCheckNameV1> {
        verify_decision(&chain.store, tx)
            .unwrap()
            .failures()
            .map(|c| c.name)
            .collect()
    };

    // A stored id that is not the snapshot's digest.
    let mut tampered = genuine.clone();
    tampered.decision_id = irena_decision::DecisionIdV1::from_hash(Hash::from_bytes([0; 32]));
    assert_eq!(
        failing(&plant(&tampered)),
        [DecisionCheckNameV1::DecisionIdDerives]
    );

    // A different actor claimed: the id is recomputed so only the substance fails.
    let mut snapshot = genuine.snapshot.clone();
    snapshot.actor = "okafor".to_owned();
    let tampered = FinalDecisionRecordV1::assemble(snapshot, genuine.signature);
    assert_eq!(
        failing(&plant(&tampered)),
        [
            DecisionCheckNameV1::ActorResolves,
            DecisionCheckNameV1::SignatureVerifies
        ],
        "the channel does not resolve to okafor, and chen's signature no longer covers the snapshot"
    );

    // A different key claimed, with a signature that matches it: the chain says whose
    // key the channel holds.
    let mut snapshot = genuine.snapshot.clone();
    snapshot.key = key(5).public_key();
    let signature = key(5).sign(&snapshot.signing_message());
    let tampered = FinalDecisionRecordV1::assemble(snapshot, signature);
    assert_eq!(
        failing(&plant(&tampered)),
        [DecisionCheckNameV1::ActorResolves]
    );

    // A collective channel claimed.
    let mut snapshot = genuine.snapshot.clone();
    snapshot.channel = "board".to_owned();
    let signature = key(4).sign(&snapshot.signing_message());
    let tampered = FinalDecisionRecordV1::assemble(snapshot, signature);
    assert_eq!(
        failing(&plant(&tampered)),
        [DecisionCheckNameV1::ChannelIsIndividual]
    );

    // A pinned record that was never in force at that height.
    let mut snapshot = genuine.snapshot.clone();
    snapshot.channels_tx_id = TxId::from_hash(Hash::from_bytes([0xab; 32]));
    let signature = key(4).sign(&snapshot.signing_message());
    let tampered = FinalDecisionRecordV1::assemble(snapshot, signature);
    assert_eq!(
        failing(&plant(&tampered)),
        [DecisionCheckNameV1::RecordsResolve]
    );

    // A snapshot from the future.
    let mut snapshot = genuine.snapshot.clone();
    snapshot.height = BlockHeight(1_000);
    let signature = key(4).sign(&snapshot.signing_message());
    let tampered = FinalDecisionRecordV1::assemble(snapshot, signature);
    let failed = failing(&plant(&tampered));
    assert!(
        failed.contains(&DecisionCheckNameV1::SnapshotPrecedesRecord),
        "{failed:?}"
    );

    // A signature by someone else over the genuine snapshot.
    let tampered = FinalDecisionRecordV1::assemble(
        genuine.snapshot.clone(),
        key(5).sign(&genuine.snapshot.signing_message()),
    );
    assert_eq!(
        failing(&plant(&tampered)),
        [DecisionCheckNameV1::SignatureVerifies]
    );

    // One flipped byte in the stored record.
    let mut bytes = genuine.canonical_bytes();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    let tx = append_raw(&chain.store, DECISION_NAMESPACE, bytes);
    let report = verify_decision(&chain.store, &tx).unwrap();
    assert!(!report.is_valid());
    assert!(report.checks.iter().any(|c| !c.passed), "{report:#?}");

    // Not a record at all, and a record in the wrong namespace.
    let tx = append_raw(&chain.store, DECISION_NAMESPACE, b"not a record".to_vec());
    assert_eq!(failing(&tx), [DecisionCheckNameV1::Decodes]);
    let tx = append_raw(&chain.store, "irena.vote.v1", genuine.canonical_bytes());
    assert_eq!(failing(&tx), [DecisionCheckNameV1::Decodes]);
    let error = verify_decision(&chain.store, &TxId::from_hash(Hash::from_bytes([1; 32])))
        .expect_err("no such transaction");
    assert!(matches!(error, DecisionError::NoSuchTransaction { .. }));
}
