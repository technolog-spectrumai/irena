//! Record composition, reading, validation and the published schemas.

use irena_core::{
    CompanyIdV1, IrenaError, IssueV1, NotarisationV1, NotaryIdV1, NotaryTimeV1, RecordBodyV1,
    RecordKindV1, compose_record, read_company_genesis_document, read_record,
    read_share_structure_document,
};
use prunella_core::{Hash, TxId};
use std::path::Path;

const GENESIS: &str = r#"<company-genesis>
  <identity name="Acme Industries Ltd" jurisdiction="gb" registered-number="01234567"/>
  <incorporation document-digest="9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f9a3f"/>
  <share-structure>
    <holder id="alice" name="Alice Smith" shares="500"/>
    <holder id="bob" shares="300"/>
    <holder id="carol" shares="200"/>
  </share-structure>
  <identities>
    <person id="alice" name="Alice Smith" document-id="GB-P-123456789" key="4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c"/>
    <person id="bob" key="7b217b217b217b217b217b217b217b217b217b217b217b217b217b217b217b21"/>
    <person id="carol" name="Carol White"/>
    <person id="chen" name="M. Chen" key="c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1"/>
    <person id="okafor" key="c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2"/>
    <person id="jane" name="Jane Roe" key="9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e"/>
  </identities>
  <governance>
    <decision-channels>
      <channel id="shareholders" mode="collective">
        <actors source="share-register"/>
        <scope>
          <amend part="share-structure"/>
          <amend part="decision-channels"/>
          <amend part="identities"/>
          <amend part="authorisation"/>
        </scope>
        <voting-rules version="1.0">
          <weight type="electorate"/>
          <exclusions enabled="true"/>
          <quorum type="fraction" numerator="1" denominator="2" basis="total-electorate"/>
          <threshold type="simple-majority" basis="votes-cast"/>
          <abstentions treatment="exclude"/>
          <tie treatment="reject"/>
        </voting-rules>
      </channel>
      <channel id="board" mode="collective">
        <actors source="roster">
          <member id="chen" name="M. Chen" weight="2"/>
          <member id="okafor"/>
          <member id="vance"/>
        </actors>
        <scope>
          <amend part="decision-channels"/>
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
          <member id="chen"/>
        </actors>
      </channel>
    </decision-channels>
  </governance>
  <authorisation>
    <signer person="jane" records="company"/>
    <signer person="jane" records="governance"/>
  </authorisation>
</company-genesis>"#;

const IDENTITY: &str =
    r#"<identity name="Acme Industries plc" jurisdiction="gb" registered-number="01234567"/>"#;

const SHARES: &str = r#"<share-structure>
  <holder id="alice" name="Alice Smith" shares="500"/>
  <holder id="bob" shares="300"/>
  <holder id="carol" shares="200"/>
</share-structure>"#;

const RULES: &str = r#"<voting-rules version="1.0">
  <weight type="electorate"/>
  <exclusions enabled="true"/>
  <quorum type="fraction" numerator="1" denominator="2" basis="total-electorate"/>
  <threshold type="simple-majority" basis="votes-cast"/>
  <abstentions treatment="exclude"/>
  <tie treatment="reject"/>
</voting-rules>"#;

const CHANNELS: &str = r#"<decision-channels>
  <channel id="shareholders" mode="collective">
    <actors source="share-register"/>
    <scope>
      <amend part="share-structure"/>
    </scope>
    <voting-rules version="1.0">
      <weight type="electorate"/>
      <exclusions enabled="true"/>
      <quorum type="fraction" numerator="1" denominator="2" basis="total-electorate"/>
      <threshold type="simple-majority" basis="votes-cast"/>
      <abstentions treatment="exclude"/>
      <tie treatment="reject"/>
    </voting-rules>
  </channel>
  <channel id="ceo" mode="individual">
    <actors source="roster">
      <member id="chen" name="M. Chen"/>
    </actors>
  </channel>
</decision-channels>"#;

const IDENTITIES: &str = r#"<identities>
  <person id="alice" name="Alice Smith" document-id="GB-P-123456789" key="4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c"/>
  <person id="carol" name="Carol White"/>
  <person id="jane" key="9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e"/>
</identities>"#;

const AUTHORISATION: &str = r#"<authorisation>
  <signer person="jane" records="governance"/>
  <signer person="jane" records="company"/>
  <signer person="alice" records="governance"/>
</authorisation>"#;

fn shareholders() -> irena_core::ChannelIdV1 {
    irena_core::ChannelIdV1::new("shareholders").expect("channel id")
}

fn company() -> CompanyIdV1 {
    CompanyIdV1::new("acme").expect("company")
}

fn notary() -> NotarisationV1 {
    NotarisationV1 {
        id: NotaryIdV1::new("notary-07").expect("id"),
        name: "Jane Roe".to_owned(),
        address: Some("12 High Street, London".to_owned()),
        at: NotaryTimeV1::parse("2026-03-01T09:30:00Z").expect("time"),
        statement: Some("Filed at Companies House".to_owned()),
        source_digest: Some(Hash::from_bytes([0xc4; 32])),
    }
}

fn bodies() -> [(RecordKindV1, &'static str); 6] {
    [
        (RecordKindV1::CompanyGenesis, GENESIS),
        (RecordKindV1::Identity, IDENTITY),
        (RecordKindV1::ShareStructure, SHARES),
        (RecordKindV1::DecisionChannels, CHANNELS),
        (RecordKindV1::Identities, IDENTITIES),
        (RecordKindV1::Authorisation, AUTHORISATION),
    ]
}

#[test]
fn every_kind_composes_and_reads_back_with_the_body_verbatim() {
    for (kind, body) in bodies() {
        let xml = compose_record(kind, &company(), None, &notary(), body)
            .unwrap_or_else(|e| panic!("{kind}: {e}"));
        assert!(
            xml.contains(body),
            "{kind}: body must be embedded verbatim\n{xml}"
        );
        assert!(xml.starts_with("<irena-record version=\"1.0\""), "{xml}");
        assert!(xml.ends_with("</irena-record>"), "{xml}");
        assert!(
            prunella_xml::is_single_element(&xml),
            "{kind}: a composed record must be exactly one element so Prunella nests it"
        );

        let record = read_record(&xml).expect("reads back");
        assert_eq!(record.kind(), kind);
        assert_eq!(record.company, company());
        assert_eq!(record.supersedes, None);
        assert_eq!(record.notarisation, notary());
    }
}

#[test]
fn a_supersedes_id_survives_the_trip() {
    let id = TxId::from_hash(Hash::from_bytes([0x8f; 32]));
    let xml = compose_record(
        RecordKindV1::ShareStructure,
        &company(),
        Some(id),
        &notary(),
        SHARES,
    )
    .expect("compose");
    assert!(xml.contains(&format!("supersedes=\"{id}\"")));
    assert_eq!(read_record(&xml).expect("read").supersedes, Some(id));
}

#[test]
fn the_share_structure_is_read_into_a_sorted_register() {
    let xml = compose_record(
        RecordKindV1::ShareStructure,
        &company(),
        None,
        &notary(),
        SHARES,
    )
    .expect("compose");
    let RecordBodyV1::ShareStructure(register) = read_record(&xml).expect("read").body else {
        panic!("wrong body");
    };
    let summary: Vec<(&str, u64, Option<&str>)> = register
        .holders()
        .iter()
        .map(|h| (h.id.as_str(), h.shares, h.name.as_deref()))
        .collect();
    assert_eq!(
        summary,
        [
            ("alice", 500, Some("Alice Smith")),
            ("bob", 300, None),
            ("carol", 200, None),
        ]
    );
    assert_eq!(register.total_shares(), 1000);
}

#[test]
fn the_genesis_is_the_whole_company() {
    let genesis = read_company_genesis_document(GENESIS).expect("read");
    assert_eq!(genesis.identity.name, "Acme Industries Ltd");
    assert_eq!(genesis.identity.jurisdiction.as_deref(), Some("gb"));
    assert_eq!(
        genesis.identity.registered_number.as_deref(),
        Some("01234567")
    );
    assert_eq!(
        genesis.incorporation_digest,
        Some(Hash::from_bytes(
            [0x9a, 0x3f].repeat(16).try_into().unwrap()
        ))
    );
    assert_eq!(genesis.shares.len(), 3);
    assert_eq!(genesis.shares.total_shares(), 1000);
    // The nested rules are exactly what Bornite reads from a standalone file.
    let standalone = bornite_xml::read_rules_document(RULES).expect("rules");
    assert_eq!(genesis.channels.len(), 3);
    let shareholders = genesis.channels.get(&shareholders()).expect("channel");
    assert_eq!(
        shareholders.actors,
        irena_core::ActorSourceV1::ShareRegister
    );
    assert_eq!(shareholders.mode.rules(), Some(&standalone));
    let board = genesis
        .channels
        .get(&irena_core::ChannelIdV1::new("board").unwrap())
        .expect("board");
    let irena_core::ActorSourceV1::Roster(roster) = &board.actors else {
        panic!("board is a roster");
    };
    let weights: Vec<(&str, u64)> = roster
        .members()
        .iter()
        .map(|m| (m.id.as_str(), m.weight))
        .collect();
    assert_eq!(
        weights,
        [("chen", 2), ("okafor", 1), ("vance", 1)],
        "weight defaults to 1"
    );
    // Keys live in the identities part, once per person, wherever they sit.
    let alice = bornite_core::VoterIdV1::new("alice").unwrap();
    let alice_key =
        prunella_core::PublicKey::from_bytes([0x4e, 0x9c].repeat(16).try_into().unwrap());
    assert_eq!(genesis.identities.key_of(&alice), Some(alice_key));
    assert_eq!(
        genesis
            .identities
            .get(&alice)
            .unwrap()
            .document_id
            .as_deref(),
        Some("GB-P-123456789")
    );
    let carol = bornite_core::VoterIdV1::new("carol").unwrap();
    assert_eq!(
        genesis.identities.key_of(&carol),
        None,
        "listed, cannot sign"
    );
    assert_eq!(genesis.identities.get(&carol).unwrap().document_id, None);
    assert_eq!(genesis.identities.len(), 6);
    let jane = bornite_core::VoterIdV1::new("jane").unwrap();
    assert!(
        genesis
            .authorisation
            .allows(&jane, irena_core::RecordFamilyV1::Company)
    );
    assert!(
        genesis
            .authorisation
            .allows(&jane, irena_core::RecordFamilyV1::Governance)
    );
    assert!(
        !genesis
            .authorisation
            .allows(&alice, irena_core::RecordFamilyV1::Company)
    );
    let ceo = genesis
        .channels
        .get(&irena_core::ChannelIdV1::new("ceo").unwrap())
        .expect("ceo");
    assert!(ceo.mode.is_individual());
    assert!(ceo.mode.rules().is_none());
    // Scope is read alongside the mode, and silence denies.
    assert_eq!(
        shareholders.scope.as_ref().expect("scoped").parts(),
        [
            RecordKindV1::ShareStructure,
            RecordKindV1::DecisionChannels,
            RecordKindV1::Identities,
            RecordKindV1::Authorisation,
        ],
        "sorted by the kind order, whatever order the document listed"
    );
    assert!(shareholders.may_amend(RecordKindV1::Authorisation));
    assert!(board.may_amend(RecordKindV1::DecisionChannels));
    assert!(!board.may_amend(RecordKindV1::ShareStructure));
    assert!(ceo.scope.is_none());
    assert!(!ceo.may_amend(RecordKindV1::DecisionChannels));

    let minimal = read_company_genesis_document(
        r#"<company-genesis><identity name="X"/><share-structure/><identities/><governance><decision-channels><channel id="all" mode="collective"><actors source="share-register"/><voting-rules version="1.0"><weight type="equal"/><exclusions enabled="false"/><quorum type="none"/><threshold type="simple-majority" basis="votes-cast"/><abstentions treatment="exclude"/><tie treatment="reject"/></voting-rules></channel></decision-channels></governance><authorisation><signer person="x" records="company"/></authorisation></company-genesis>"#,
    )
    .expect("minimal");
    assert_eq!(minimal.incorporation_digest, None);
    assert!(minimal.shares.is_empty());
    assert!(minimal.identities.is_empty());

    let identity = irena_core::read_identity_document(IDENTITY).expect("identity");
    assert_eq!(identity.name, "Acme Industries plc");
}
#[test]
fn notarisation_special_characters_survive() {
    let mut notarisation = notary();
    notarisation.name = "O'Brien & \"Sons\" <Ltd>".to_owned();
    notarisation.address = Some("1 <Main> & \"Side\"".to_owned());
    notarisation.statement = Some("said \"yes\" & <no>".to_owned());
    let xml = compose_record(
        RecordKindV1::DecisionChannels,
        &company(),
        None,
        &notarisation,
        CHANNELS,
    )
    .expect("compose");
    assert_eq!(read_record(&xml).expect("read").notarisation, notarisation);
}

#[test]
fn a_notarisation_is_required_and_every_field_is_checked() {
    let good = compose_record(
        RecordKindV1::DecisionChannels,
        &company(),
        None,
        &notary(),
        CHANNELS,
    )
    .expect("compose");

    let without = {
        let start = good.find("<notarisation").unwrap();
        let end = good[start..].find("/>").unwrap() + start + 2;
        format!("{}{}", &good[..start], &good[end..])
    };
    let error = read_record(&without).expect_err("no notarisation");
    assert!(
        matches!(
            error.issues(),
            [IssueV1::MissingElement {
                parent: "irena-record",
                element: "notarisation"
            }]
        ),
        "{error}"
    );

    type Matches = fn(&IssueV1) -> bool;
    let cases: Vec<(&str, String, Matches)> = vec![
        ("missing id", good.replace(" id=\"notary-07\"", ""), |i| {
            matches!(
                i,
                IssueV1::Xml(bornite_xml::XmlIssueV1::MissingAttribute {
                    attribute: "id",
                    ..
                })
            )
        }),
        (
            "missing name",
            good.replace(" name=\"Jane Roe\"", ""),
            |i| {
                matches!(
                    i,
                    IssueV1::Xml(bornite_xml::XmlIssueV1::MissingAttribute {
                        attribute: "name",
                        ..
                    })
                )
            },
        ),
        (
            "empty name",
            good.replace("name=\"Jane Roe\"", "name=\"  \""),
            |i| {
                matches!(
                    i,
                    IssueV1::InvalidValue {
                        element: "notarisation",
                        attribute: "name",
                        ..
                    }
                )
            },
        ),
        (
            "missing at",
            good.replace(" at=\"2026-03-01T09:30:00Z\"", ""),
            |i| {
                matches!(
                    i,
                    IssueV1::Xml(bornite_xml::XmlIssueV1::MissingAttribute {
                        attribute: "at",
                        ..
                    })
                )
            },
        ),
        (
            "offset time",
            good.replace("2026-03-01T09:30:00Z", "2026-03-01T09:30:00+01:00"),
            |i| {
                matches!(
                    i,
                    IssueV1::InvalidValue {
                        element: "notarisation",
                        attribute: "at",
                        ..
                    }
                )
            },
        ),
        (
            "fractional seconds",
            good.replace("2026-03-01T09:30:00Z", "2026-03-01T09:30:00.5Z"),
            |i| {
                matches!(
                    i,
                    IssueV1::InvalidValue {
                        element: "notarisation",
                        attribute: "at",
                        ..
                    }
                )
            },
        ),
        (
            "impossible date",
            good.replace("2026-03-01T09:30:00Z", "2026-02-30T09:30:00Z"),
            |i| {
                matches!(
                    i,
                    IssueV1::InvalidValue {
                        element: "notarisation",
                        attribute: "at",
                        ..
                    }
                )
            },
        ),
        (
            "bad notary id",
            good.replace("id=\"notary-07\"", "id=\"notary 07\""),
            |i| {
                matches!(
                    i,
                    IssueV1::InvalidValue {
                        element: "notarisation",
                        attribute: "id",
                        ..
                    }
                )
            },
        ),
        (
            "bad digest",
            good.replace("source-digest=\"c4c4", "source-digest=\"zzc4"),
            |i| {
                matches!(
                    i,
                    IssueV1::InvalidValue {
                        element: "notarisation",
                        attribute: "source-digest",
                        ..
                    }
                )
            },
        ),
    ];
    for (label, xml, matches) in cases {
        let error = read_record(&xml).expect_err(label);
        assert!(
            error.issues().iter().any(matches),
            "{label}: expected a specific issue, got {error}"
        );
    }

    // Several problems at once are all reported, in one sorted list.
    let many = good
        .replace(" name=\"Jane Roe\"", "")
        .replace("2026-03-01T09:30:00Z", "never")
        .replace("id=\"notary-07\"", "id=\"\"");
    let error = read_record(&many).expect_err("three problems");
    assert_eq!(error.issues().len(), 3, "{error}");

    // An unknown attribute is structural.
    let unknown = good.replace("<notarisation ", "<notarisation licence=\"1\" ");
    assert!(matches!(
        read_record(&unknown),
        Err(IrenaError::Malformed { .. })
    ));
}

#[test]
fn share_structure_problems_are_collected_together() {
    let bad = r#"<share-structure>
  <holder id="alice" shares="0"/>
  <holder id="alice" shares="1"/>
  <holder id="bob" shares="18446744073709551615"/>
  <holder id="carol" shares="1"/>
</share-structure>"#;
    let error = read_share_structure_document(bad).expect_err("bad register");
    let issues = error.issues();
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, IssueV1::DuplicateHolder { .. })),
        "{error}"
    );
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, IssueV1::ZeroShares { .. })),
        "{error}"
    );
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, IssueV1::TotalSharesOverflow { .. })),
        "{error}"
    );
    assert_eq!(issues.len(), 3, "{error}");

    // Attribute-level problems are collected too, before the register is built.
    let attributes = r#"<share-structure>
  <holder id="al ice" shares="5"/>
  <holder id="carol" shares="-1"/>
  <holder id="dave" shares="+5"/>
  <holder id="erin"/>
</share-structure>"#;
    let error = read_share_structure_document(attributes).expect_err("bad attributes");
    let issues = error.issues();
    assert_eq!(issues.len(), 4, "{error}");
    for (attribute, count) in [("id", 1), ("shares", 3)] {
        let found = issues
            .iter()
            .filter(|i| match i {
                IssueV1::InvalidValue {
                    element: "holder",
                    attribute: a,
                    ..
                } => *a == attribute,
                IssueV1::Xml(bornite_xml::XmlIssueV1::MissingAttribute {
                    element: "holder",
                    attribute: a,
                }) => *a == attribute,
                _ => false,
            })
            .count();
        assert_eq!(found, count, "{attribute}: {error}");
    }

    // Structural problems stop parsing.
    for (label, xml) in [
        (
            "unknown child",
            "<share-structure><class id=\"a\"/></share-structure>",
        ),
        (
            "unknown attribute",
            "<share-structure><holder id=\"a\" shares=\"1\" class=\"x\"/></share-structure>",
        ),
        (
            "element in holder",
            "<share-structure><holder id=\"a\" shares=\"1\"><x/></holder></share-structure>",
        ),
        ("wrong root", "<electorate/>"),
        ("not xml", "<<<"),
    ] {
        assert!(
            matches!(
                read_share_structure_document(xml),
                Err(IrenaError::Malformed { .. })
            ),
            "{label}"
        );
    }
    assert!(
        read_share_structure_document("<share-structure/>")
            .expect("empty")
            .is_empty()
    );
}

#[test]
fn genesis_problems_are_reported_together() {
    let governance = GENESIS[GENESIS.find("<governance>").unwrap()
        ..GENESIS.find("</governance>").unwrap() + "</governance>".len()]
        .to_owned();
    let shares = GENESIS[GENESIS.find("<share-structure>").unwrap()
        ..GENESIS.find("</share-structure>").unwrap() + "</share-structure>".len()]
        .to_owned();
    let identities = GENESIS[GENESIS.find("<identities>").unwrap()
        ..GENESIS.find("</identities>").unwrap() + "</identities>".len()]
        .to_owned();
    let authorisation = GENESIS[GENESIS.find("<authorisation>").unwrap()
        ..GENESIS.find("</authorisation>").unwrap() + "</authorisation>".len()]
        .to_owned();
    let with = |middle: &str| {
        format!("<company-genesis>{identities}{middle}{authorisation}</company-genesis>")
    };
    for (label, xml, expect) in [
        (
            "no identity",
            with(&format!("<incorporation/>{shares}{governance}")),
            IssueV1::MissingElement {
                parent: "company-genesis",
                element: "identity",
            },
        ),
        (
            "no register",
            with(&format!("<identity name=\"a\"/>{governance}")),
            IssueV1::MissingElement {
                parent: "company-genesis",
                element: "share-structure",
            },
        ),
        (
            "no governance",
            with(&format!("<identity name=\"a\"/>{shares}")),
            IssueV1::MissingElement {
                parent: "company-genesis",
                element: "governance",
            },
        ),
        (
            "no authorisation",
            format!(
                "<company-genesis><identity name=\"a\"/>{shares}{identities}{governance}</company-genesis>"
            ),
            IssueV1::MissingElement {
                parent: "company-genesis",
                element: "authorisation",
            },
        ),
        (
            "no identities",
            format!(
                "<company-genesis><identity name=\"a\"/>{shares}{governance}{authorisation}</company-genesis>"
            ),
            IssueV1::MissingElement {
                parent: "company-genesis",
                element: "identities",
            },
        ),
        (
            "empty governance",
            with(&format!(
                "<identity name=\"a\"/>{shares}<governance></governance>"
            )),
            IssueV1::MissingElement {
                parent: "governance",
                element: "decision-channels",
            },
        ),
        (
            "bad authorisation inside the genesis",
            format!(
                "<company-genesis><identity name=\"a\"/>{shares}{identities}{governance}<authorisation/></company-genesis>"
            ),
            IssueV1::NoCompanySigner,
        ),
        (
            "bad identities inside the genesis",
            format!(
                "<company-genesis><identity name=\"a\"/>{shares}<identities><person id=\"x\"/><person id=\"x\"/></identities>{governance}{authorisation}</company-genesis>"
            ),
            IssueV1::DuplicatePerson {
                id: bornite_core::VoterIdV1::new("x").unwrap(),
            },
        ),
        (
            "two identities",
            with(&format!(
                "<identity name=\"a\"/><identity name=\"b\"/>{shares}{governance}"
            )),
            IssueV1::RepeatedElement {
                parent: "company-genesis",
                element: "identity",
            },
        ),
        (
            "empty name",
            with(&format!("<identity name=\"\"/>{shares}{governance}")),
            IssueV1::InvalidValue {
                element: "identity",
                attribute: "name",
                value: String::new(),
                reason: "must not be empty".to_owned(),
            },
        ),
        (
            "bad register inside the genesis",
            with(&format!(
                "<identity name=\"a\"/><share-structure><holder id=\"x\" shares=\"0\"/></share-structure>{governance}"
            )),
            IssueV1::ZeroShares {
                id: bornite_core::VoterIdV1::new("x").unwrap(),
            },
        ),
    ] {
        let error = read_company_genesis_document(&xml).expect_err(label);
        assert!(error.issues().contains(&expect), "{label}: {error}");
    }
    // Everything wrong at once is reported at once.
    let error =
        read_company_genesis_document("<company-genesis><identity name=\"\"/></company-genesis>")
            .expect_err("five problems");
    assert_eq!(error.issues().len(), 5, "{error}");
    // Bornite's own issues inside the nested rules surface as Irena issues.
    let bad_rules = GENESIS.replace("treatment=\"reject\"", "treatment=\"maybe\"");
    let error = read_company_genesis_document(&bad_rules).expect_err("bad rules");
    assert!(matches!(error.issues(), [IssueV1::Xml(_)]), "{error}");
    for (label, xml) in [
        (
            "unknown attribute",
            "<company-genesis><identity name=\"a\" founder=\"x\"/></company-genesis>",
        ),
        (
            "unknown child",
            "<company-genesis><board/></company-genesis>",
        ),
        (
            "stray element in governance",
            "<company-genesis><governance><quorum/></governance></company-genesis>",
        ),
    ] {
        assert!(
            matches!(
                read_company_genesis_document(xml),
                Err(IrenaError::Malformed { .. })
            ),
            "{label}"
        );
    }
}
#[test]
fn the_kind_must_match_the_body_and_the_envelope_is_strict() {
    let good = compose_record(
        RecordKindV1::DecisionChannels,
        &company(),
        None,
        &notary(),
        CHANNELS,
    )
    .expect("compose");

    let mismatched = good.replace("kind=\"decision-channels\"", "kind=\"share-structure\"");
    assert!(matches!(
        read_record(&mismatched),
        Err(IrenaError::KindMismatch {
            declared: RecordKindV1::ShareStructure,
            carried: RecordKindV1::DecisionChannels
        })
    ));

    assert!(matches!(
        read_record(&good.replace("version=\"1.0\"", "version=\"2.0\"")),
        Err(IrenaError::UnsupportedVersion { .. })
    ));
    for (label, xml) in [
        (
            "unknown kind",
            good.replace("kind=\"decision-channels\"", "kind=\"roll\""),
        ),
        (
            "unknown attribute",
            good.replace("company=\"acme\"", "company=\"acme\" x=\"1\""),
        ),
        (
            "unknown child",
            good.replace("</irena-record>", "<extra/></irena-record>"),
        ),
        (
            "two bodies",
            good.replace("</irena-record>", &format!("{CHANNELS}</irena-record>")),
        ),
        (
            "wrong root",
            good.replace("irena-record", "governance-record"),
        ),
        ("notarisation after body", {
            let start = good.find("<notarisation").unwrap();
            let end = good[start..].find("/>").unwrap() + start + 2;
            let element = good[start..end].to_owned();
            format!(
                "{}{}{element}</irena-record>",
                &good[..start],
                &good[end..good.len() - "</irena-record>".len()]
            )
        }),
        ("trailing content", format!("{good}<x/>")),
    ] {
        assert!(
            matches!(read_record(&xml), Err(IrenaError::Malformed { .. })),
            "{label}: {:?}",
            read_record(&xml)
        );
    }
    let error = read_record(&good.replace("company=\"acme\"", "company=\"Acme Ltd\""))
        .expect_err("bad company");
    assert!(
        matches!(
            error.issues(),
            [IssueV1::InvalidValue {
                element: "irena-record",
                attribute: "company",
                ..
            }]
        ),
        "{error}"
    );
    let error = read_record(&good.replace(
        "kind=\"decision-channels\"",
        "kind=\"decision-channels\" supersedes=\"nope\"",
    ))
    .expect_err("bad supersedes");
    assert!(
        matches!(
            error.issues(),
            [IssueV1::InvalidValue {
                element: "irena-record",
                attribute: "supersedes",
                ..
            }]
        ),
        "{error}"
    );
}

#[test]
fn composing_refuses_a_body_of_the_wrong_kind_or_an_invalid_notarisation() {
    assert!(
        compose_record(
            RecordKindV1::ShareStructure,
            &company(),
            None,
            &notary(),
            CHANNELS
        )
        .is_err()
    );
    assert!(
        compose_record(
            RecordKindV1::DecisionChannels,
            &company(),
            None,
            &notary(),
            SHARES
        )
        .is_err()
    );
    let mut blank = notary();
    blank.name = String::new();
    let error = compose_record(
        RecordKindV1::DecisionChannels,
        &company(),
        None,
        &blank,
        CHANNELS,
    )
    .expect_err("blank notary name");
    assert!(
        matches!(
            error.issues(),
            [IssueV1::InvalidValue {
                attribute: "name",
                ..
            }]
        ),
        "{error}"
    );
    // A declaration on the body is stripped; the element is embedded.
    let declared = format!("<?xml version=\"1.0\"?>\n{CHANNELS}\n");
    let xml = compose_record(
        RecordKindV1::DecisionChannels,
        &company(),
        None,
        &notary(),
        &declared,
    )
    .expect("ok");
    assert!(!xml.contains("<?xml"));
    assert!(xml.contains(CHANNELS));
    // The wrong body for a kind, in both directions.
    assert!(compose_record(RecordKindV1::Identity, &company(), None, &notary(), GENESIS).is_err());
    assert!(
        compose_record(
            RecordKindV1::CompanyGenesis,
            &company(),
            None,
            &notary(),
            IDENTITY
        )
        .is_err()
    );
}

#[test]
fn a_record_serialises_to_json_for_tooling() {
    let xml = compose_record(
        RecordKindV1::ShareStructure,
        &company(),
        None,
        &notary(),
        SHARES,
    )
    .expect("compose");
    let record = read_record(&xml).expect("read");
    let json = serde_json::to_value(&record).expect("json");
    assert_eq!(json["company"], "acme");
    assert_eq!(json["notarisation"]["id"], "notary-07");
    assert_eq!(json["notarisation"]["at"], "2026-03-01T09:30:00Z");
    assert_eq!(json["body"]["kind"], "share-structure");
    assert_eq!(json["body"]["value"][0]["id"], "alice");
    assert_eq!(json["body"]["value"][0]["shares"], 500);
}

#[test]
fn composed_records_and_bodies_validate_against_the_published_schemas() {
    if std::process::Command::new("xmllint")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping: xmllint is not installed");
        return;
    }
    let schemas = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let dir = tempfile::TempDir::new().expect("temp dir");
    let check = |label: &str, schema: &str, xml: &str| {
        let file = dir.path().join(format!("{label}.xml"));
        std::fs::write(&file, xml).expect("write");
        let output = std::process::Command::new("xmllint")
            .args(["--noout", "--schema"])
            .arg(schemas.join(schema).canonicalize().expect("schema"))
            .arg(&file)
            .output()
            .expect("run xmllint");
        assert!(
            output.status.success(),
            "{label} failed {schema}:\n{}\n{xml}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    let id = TxId::from_hash(Hash::from_bytes([0x11; 32]));
    for (kind, body) in bodies() {
        let first = compose_record(kind, &company(), None, &notary(), body).expect("compose");
        check(&format!("{kind}-first"), "irena-record-v1.xsd", &first);
        let amendment =
            compose_record(kind, &company(), Some(id), &notary(), body).expect("compose");
        check(
            &format!("{kind}-amendment"),
            "irena-record-v1.xsd",
            &amendment,
        );
    }
    check("genesis-body", "irena-company-v1.xsd", GENESIS);
    check("identity-body", "irena-company-v1.xsd", IDENTITY);
    check("shares-body", "irena-company-v1.xsd", SHARES);
    check(
        "empty-shares-body",
        "irena-company-v1.xsd",
        "<share-structure/>",
    );
    check("rules-body", "bornite-voting-rules-v1.xsd", RULES);
    check("channels-body", "irena-company-v1.xsd", CHANNELS);
    check("identities-body", "irena-company-v1.xsd", IDENTITIES);
    check(
        "empty-identities-body",
        "irena-company-v1.xsd",
        "<identities/>",
    );
    check("authorisation-body", "irena-company-v1.xsd", AUTHORISATION);
    for example in examples() {
        let xml = std::fs::read_to_string(&example).expect("read example");
        let schema = if xml.contains("<irena-record") {
            "irena-record-v1.xsd"
        } else {
            "irena-company-v1.xsd"
        };
        check(
            &example.file_name().unwrap().to_string_lossy(),
            schema,
            &xml,
        );
    }

    // A minimal notarisation, without the optional attributes, also validates.
    let minimal = NotarisationV1 {
        id: NotaryIdV1::new("n").expect("id"),
        name: "N".to_owned(),
        address: None,
        at: NotaryTimeV1::parse("2026-01-01T00:00:00Z").expect("time"),
        statement: None,
        source_digest: None,
    };
    let xml = compose_record(
        RecordKindV1::DecisionChannels,
        &company(),
        None,
        &minimal,
        CHANNELS,
    )
    .expect("compose");
    check("minimal-notarisation", "irena-record-v1.xsd", &xml);
}

/// Every XML file under `examples/` at the repository root.
fn examples() -> Vec<std::path::PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "xml"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no examples found");
    files
}

#[test]
fn every_example_document_parses() {
    for example in examples() {
        let xml = std::fs::read_to_string(&example).expect("read example");
        let name = example.file_name().unwrap().to_string_lossy().into_owned();
        let result = if xml.contains("<irena-record") {
            read_record(&xml).map(|_| ())
        } else if xml.contains("<company-genesis") {
            read_company_genesis_document(&xml).map(|_| ())
        } else if xml.contains("<decision-channels") {
            irena_core::read_decision_channels_document(&xml).map(|_| ())
        } else if xml.contains("<share-structure") {
            read_share_structure_document(&xml).map(|_| ())
        } else if xml.contains("<identities") {
            irena_core::read_identities_document(&xml).map(|_| ())
        } else if xml.contains("<authorisation") {
            irena_core::read_authorisation_document(&xml).map(|_| ())
        } else {
            panic!("{name}: unknown example document");
        };
        result.unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn channel_problems_are_reported_together() {
    let rules = RULES;
    let wrap = |channels: &str| format!("<decision-channels>{channels}</decision-channels>");
    let read = irena_core::read_decision_channels_document;
    let chen = irena_core::ChannelIdV1::new("c").unwrap();
    for (label, xml, expect) in [
        (
            "individual with rules",
            wrap(&format!(
                r#"<channel id="c" mode="individual"><actors source="roster"><member id="x"/></actors>{rules}</channel>"#
            )),
            IssueV1::UnexpectedElement {
                channel: chen.clone(),
                mode: "individual",
                element: "voting-rules",
            },
        ),
        (
            "collective without rules",
            wrap(
                r#"<channel id="c" mode="collective"><actors source="share-register"/></channel>"#,
            ),
            IssueV1::MissingElement {
                parent: "channel",
                element: "voting-rules",
            },
        ),
        (
            "no actors",
            wrap(&format!(
                r#"<channel id="c" mode="collective">{rules}</channel>"#
            )),
            IssueV1::MissingElement {
                parent: "channel",
                element: "actors",
            },
        ),
        (
            "empty channel",
            wrap(r#"<channel id="c" mode="individual"/>"#),
            IssueV1::MissingElement {
                parent: "channel",
                element: "actors",
            },
        ),
        (
            "unknown mode",
            wrap(r#"<channel id="c" mode="consensus"><actors source="share-register"/></channel>"#),
            IssueV1::InvalidValue {
                element: "channel",
                attribute: "mode",
                value: "consensus".to_owned(),
                reason: "must be individual or collective".to_owned(),
            },
        ),
        (
            "unknown source",
            wrap(r#"<channel id="c" mode="individual"><actors source="registry"/></channel>"#),
            IssueV1::InvalidValue {
                element: "actors",
                attribute: "source",
                value: "registry".to_owned(),
                reason: "must be share-register or roster".to_owned(),
            },
        ),
        (
            "members under the share register",
            wrap(
                r#"<channel id="c" mode="individual"><actors source="share-register"><member id="x"/></actors></channel>"#,
            ),
            IssueV1::UnexpectedElement {
                channel: chen.clone(),
                mode: "share-register",
                element: "member",
            },
        ),
        (
            "empty roster",
            wrap(r#"<channel id="c" mode="individual"><actors source="roster"/></channel>"#),
            IssueV1::EmptyRoster {
                channel: chen.clone(),
            },
        ),
        (
            "zero weight",
            wrap(
                r#"<channel id="c" mode="individual"><actors source="roster"><member id="x" weight="0"/></actors></channel>"#,
            ),
            IssueV1::ZeroWeight {
                channel: chen.clone(),
                id: bornite_core::VoterIdV1::new("x").unwrap(),
            },
        ),
        (
            "duplicate member",
            wrap(
                r#"<channel id="c" mode="individual"><actors source="roster"><member id="x"/><member id="x"/></actors></channel>"#,
            ),
            IssueV1::DuplicateMember {
                channel: chen.clone(),
                id: bornite_core::VoterIdV1::new("x").unwrap(),
            },
        ),
        (
            "duplicate channel",
            wrap(
                r#"<channel id="c" mode="individual"><actors source="roster"><member id="x"/></actors></channel><channel id="c" mode="individual"><actors source="roster"><member id="y"/></actors></channel>"#,
            ),
            IssueV1::DuplicateChannel { id: chen.clone() },
        ),
        (
            "bad channel id",
            wrap(
                r#"<channel id="Board" mode="individual"><actors source="roster"><member id="x"/></actors></channel>"#,
            ),
            IssueV1::InvalidValue {
                element: "channel",
                attribute: "id",
                value: "Board".to_owned(),
                reason: "must start with a lowercase letter or digit".to_owned(),
            },
        ),
        ("no channels", wrap(""), IssueV1::NoChannels),
        (
            "no channels, empty element",
            "<decision-channels/>".to_owned(),
            IssueV1::NoChannels,
        ),
    ] {
        let error = read(&xml).expect_err(label);
        assert!(error.issues().contains(&expect), "{label}: {error}");
    }
    // Two channels each wrong: both reported.
    let error = read(&wrap(
        r#"<channel id="a" mode="individual"><actors source="roster"/></channel><channel id="b" mode="collective"><actors source="share-register"/></channel>"#,
    ))
    .expect_err("two problems");
    assert_eq!(error.issues().len(), 2, "{error}");
    // Structure is refused where it is found.
    // Scope issues are content, collected like the rest.
    for (label, xml, expect) in [
        (
            "an empty scope",
            r#"<channel id="c" mode="individual"><actors source="share-register"/><scope/></channel>"#,
            IssueV1::EmptyScope {
                channel: irena_core::ChannelIdV1::new("c").unwrap(),
            },
        ),
        (
            "the genesis in a scope",
            r#"<channel id="c" mode="individual"><actors source="share-register"/><scope><amend part="company-genesis"/></scope></channel>"#,
            IssueV1::ScopeNotAnAmendment {
                channel: irena_core::ChannelIdV1::new("c").unwrap(),
                part: RecordKindV1::CompanyGenesis,
            },
        ),
        (
            "a part named twice",
            r#"<channel id="c" mode="individual"><actors source="share-register"/><scope><amend part="identity"/><amend part="identity"/></scope></channel>"#,
            IssueV1::DuplicateScopePart {
                channel: irena_core::ChannelIdV1::new("c").unwrap(),
                part: RecordKindV1::Identity,
            },
        ),
        (
            "a part that is not a kind",
            r#"<channel id="c" mode="individual"><actors source="share-register"/><scope><amend part="everything"/></scope></channel>"#,
            IssueV1::InvalidValue {
                element: "amend",
                attribute: "part",
                value: "everything".to_owned(),
                reason: "must be identity, share-structure, decision-channels, identities or authorisation".to_owned(),
            },
        ),
    ] {
        let error = read(&wrap(xml)).expect_err(label);
        assert!(error.issues().contains(&expect), "{label}: {error}");
    }

    for (label, xml) in [
        (
            "unknown child of channel",
            wrap(
                r#"<channel id="c" mode="individual"><actors source="share-register"/><quorum/></channel>"#,
            ),
        ),
        (
            "unknown child of actors",
            wrap(
                r#"<channel id="c" mode="individual"><actors source="roster"><holder id="x"/></actors></channel>"#,
            ),
        ),
        ("unknown child of the set", wrap("<rule/>")),
        (
            "unknown child of a scope",
            wrap(
                r#"<channel id="c" mode="individual"><actors source="share-register"/><scope><part name="identity"/></scope></channel>"#,
            ),
        ),
        (
            "unknown attribute on an amend",
            wrap(
                r#"<channel id="c" mode="individual"><actors source="share-register"/><scope><amend part="identity" until="2030"/></scope></channel>"#,
            ),
        ),
        (
            "unknown attribute",
            wrap(
                r#"<channel id="c" mode="individual" legal="director"><actors source="share-register"/></channel>"#,
            ),
        ),
    ] {
        assert!(
            matches!(read(&xml), Err(IrenaError::Malformed { .. })),
            "{label}"
        );
    }
}

#[test]
fn identities_problems_are_reported_together() {
    use irena_core::read_identities_document;
    let bad = r#"<identities>
  <person id="alice" key="4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c"/>
  <person id="alice"/>
  <person id="bob" key="4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c"/>
</identities>"#;
    let error = read_identities_document(bad).expect_err("bad identities");
    let issues = error.issues();
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, IssueV1::DuplicatePerson { .. })),
        "{error}"
    );
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, IssueV1::DuplicateKey { .. })),
        "{error}"
    );
    assert_eq!(issues.len(), 2, "{error}");

    // Attribute-level problems are collected before the record is built.
    let attributes = r#"<identities>
  <person id="al ice"/>
  <person id="bob" key="short"/>
  <person/>
</identities>"#;
    let error = read_identities_document(attributes).expect_err("bad attributes");
    let issues = error.issues();
    assert_eq!(issues.len(), 3, "{error}");
    assert!(
        issues.iter().any(|i| matches!(
            i,
            IssueV1::InvalidValue {
                element: "person",
                attribute: "key",
                ..
            }
        )),
        "{error}"
    );

    // The document id is opaque and comes back exactly as written.
    let read = read_identities_document(IDENTITIES).expect("read");
    let alice = bornite_core::VoterIdV1::new("alice").unwrap();
    assert_eq!(
        read.get(&alice).unwrap().document_id.as_deref(),
        Some("GB-P-123456789")
    );
    assert_eq!(
        read.get(&alice).unwrap().name.as_deref(),
        Some("Alice Smith")
    );
    let carol = bornite_core::VoterIdV1::new("carol").unwrap();
    assert_eq!(read.get(&carol).unwrap().key, None);
    assert!(
        read_identities_document("<identities/>")
            .expect("empty")
            .is_empty()
    );

    // Structure is strict: unknown attributes and children stop the reader.
    for (label, xml) in [
        (
            "unknown attribute",
            "<identities><person id=\"a\" email=\"x\"/></identities>",
        ),
        (
            "unknown child",
            "<identities><signer person=\"a\"/></identities>",
        ),
        (
            "nested child",
            "<identities><person id=\"a\"><key/></person></identities>",
        ),
    ] {
        assert!(
            matches!(
                read_identities_document(xml),
                Err(IrenaError::Malformed { .. })
            ),
            "{label}"
        );
    }
}

#[test]
fn authorisation_problems_are_reported_together() {
    use irena_core::read_authorisation_document;
    let bad = r#"<authorisation>
  <signer person="bob" records="governance"/>
  <signer person="bob" records="governance"/>
</authorisation>"#;
    let error = read_authorisation_document(bad).expect_err("bad authorisation");
    let issues = error.issues();
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, IssueV1::DuplicateSigner { .. })),
        "{error}"
    );
    assert!(
        issues.iter().any(|i| matches!(i, IssueV1::NoCompanySigner)),
        "{error}"
    );
    assert_eq!(issues.len(), 2, "{error}");
    // Nobody may sign anything: the company could never be amended.
    let error = read_authorisation_document("<authorisation/>").expect_err("locked out");
    assert_eq!(error.issues(), [IssueV1::NoCompanySigner], "{error}");

    let attributes = r#"<authorisation>
  <signer person="ja ne" records="company"/>
  <signer person="jane" records="board"/>
  <signer person="jane"/>
  <signer records="company"/>
</authorisation>"#;
    let error = read_authorisation_document(attributes).expect_err("bad attributes");
    let issues = error.issues();
    assert_eq!(issues.len(), 4, "{error}");
    assert!(
        issues.iter().any(|i| matches!(
            i,
            IssueV1::InvalidValue {
                element: "signer",
                attribute: "records",
                ..
            }
        )),
        "{error}"
    );

    let read = read_authorisation_document(AUTHORISATION).expect("read");
    let rows: Vec<(&str, irena_core::RecordFamilyV1)> = read
        .signers()
        .iter()
        .map(|s| (s.person.as_str(), s.family))
        .collect();
    assert_eq!(
        rows,
        [
            ("alice", irena_core::RecordFamilyV1::Governance),
            ("jane", irena_core::RecordFamilyV1::Company),
            ("jane", irena_core::RecordFamilyV1::Governance),
        ],
        "rows are sorted by person, then family"
    );
    for (label, xml) in [
        (
            "unknown attribute",
            "<authorisation><signer person=\"a\" records=\"company\" since=\"x\"/></authorisation>",
        ),
        (
            "unknown child",
            "<authorisation><person id=\"a\"/></authorisation>",
        ),
    ] {
        assert!(
            matches!(
                read_authorisation_document(xml),
                Err(IrenaError::Malformed { .. })
            ),
            "{label}"
        );
    }
}

#[test]
fn holders_and_members_no_longer_carry_keys() {
    // A key on a holder or a member is an unknown attribute: the one key table is the
    // identities part, and a document written for the older shape is refused outright.
    let holder = SHARES.replace(
        "<holder id=\"bob\"",
        "<holder id=\"bob\" key=\"7b217b217b217b217b217b217b217b217b217b217b217b217b217b217b217b21\"",
    );
    assert!(
        matches!(
            read_share_structure_document(&holder),
            Err(IrenaError::Malformed { .. })
        ),
        "holder key"
    );
    let member = CHANNELS.replace(
        "<member id=\"chen\"",
        "<member id=\"chen\" key=\"c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1\"",
    );
    assert!(
        matches!(
            irena_core::read_decision_channels_document(&member),
            Err(IrenaError::Malformed { .. })
        ),
        "member key"
    );
}
