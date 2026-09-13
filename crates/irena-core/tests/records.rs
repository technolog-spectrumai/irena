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
    <holder id="alice" key="4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c" name="Alice Smith" shares="500"/>
    <holder id="bob" key="7b217b217b217b217b217b217b217b217b217b217b217b217b217b217b217b21" shares="300"/>
    <holder id="carol" shares="200"/>
  </share-structure>
  <governance>
    <voting-rules version="1.0">
      <weight type="electorate"/>
      <exclusions enabled="true"/>
      <quorum type="fraction" numerator="1" denominator="2" basis="total-electorate"/>
      <threshold type="simple-majority" basis="votes-cast"/>
      <abstentions treatment="exclude"/>
      <tie treatment="reject"/>
    </voting-rules>
  </governance>
</company-genesis>"#;

const IDENTITY: &str =
    r#"<identity name="Acme Industries plc" jurisdiction="gb" registered-number="01234567"/>"#;

const SHARES: &str = r#"<share-structure>
  <holder id="alice" key="4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c" name="Alice Smith" shares="500"/>
  <holder id="bob" key="7b217b217b217b217b217b217b217b217b217b217b217b217b217b217b217b21" shares="300"/>
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

fn bodies() -> [(RecordKindV1, &'static str); 4] {
    [
        (RecordKindV1::CompanyGenesis, GENESIS),
        (RecordKindV1::Identity, IDENTITY),
        (RecordKindV1::ShareStructure, SHARES),
        (RecordKindV1::VotingRules, RULES),
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
fn the_share_structure_is_read_into_a_sorted_register_with_keys() {
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
    let summary: Vec<(&str, u64, bool, Option<&str>)> = register
        .holders()
        .iter()
        .map(|h| (h.id.as_str(), h.shares, h.key.is_some(), h.name.as_deref()))
        .collect();
    assert_eq!(
        summary,
        [
            ("alice", 500, true, Some("Alice Smith")),
            ("bob", 300, true, None),
            ("carol", 200, false, None),
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
    assert_eq!(genesis.rules, standalone);

    let minimal = read_company_genesis_document(
        r#"<company-genesis><identity name="X"/><share-structure/><governance><voting-rules version="1.0"><weight type="equal"/><exclusions enabled="false"/><quorum type="none"/><threshold type="simple-majority" basis="votes-cast"/><abstentions treatment="exclude"/><tie treatment="reject"/></voting-rules></governance></company-genesis>"#,
    )
    .expect("minimal");
    assert_eq!(minimal.incorporation_digest, None);
    assert!(minimal.shares.is_empty());

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
        RecordKindV1::VotingRules,
        &company(),
        None,
        &notarisation,
        RULES,
    )
    .expect("compose");
    assert_eq!(read_record(&xml).expect("read").notarisation, notarisation);
}

#[test]
fn a_notarisation_is_required_and_every_field_is_checked() {
    let good = compose_record(
        RecordKindV1::VotingRules,
        &company(),
        None,
        &notary(),
        RULES,
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
  <holder id="alice" key="4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c" shares="0"/>
  <holder id="alice" shares="1"/>
  <holder id="bob" key="4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c4e9c" shares="18446744073709551615"/>
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
            .any(|i| matches!(i, IssueV1::DuplicateKey { .. })),
        "{error}"
    );
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, IssueV1::TotalSharesOverflow { .. })),
        "{error}"
    );

    // Attribute-level problems are collected too, before the register is built.
    let attributes = r#"<share-structure>
  <holder id="al ice" shares="5"/>
  <holder id="bob" key="short" shares="5"/>
  <holder id="carol" shares="-1"/>
  <holder id="dave" shares="+5"/>
  <holder id="erin"/>
</share-structure>"#;
    let error = read_share_structure_document(attributes).expect_err("bad attributes");
    let issues = error.issues();
    assert_eq!(issues.len(), 5, "{error}");
    for (attribute, count) in [("id", 1), ("key", 1), ("shares", 3)] {
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
    let governance = GENESIS
        [GENESIS.find("<governance>").unwrap()..GENESIS.find("</company-genesis>").unwrap()]
        .to_owned();
    let shares = GENESIS[GENESIS.find("<share-structure>").unwrap()
        ..GENESIS.find("</share-structure>").unwrap() + "</share-structure>".len()]
        .to_owned();
    let with = |middle: &str| format!("<company-genesis>{middle}</company-genesis>");
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
            "empty governance",
            with(&format!(
                "<identity name=\"a\"/>{shares}<governance></governance>"
            )),
            IssueV1::MissingElement {
                parent: "governance",
                element: "voting-rules",
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
            .expect_err("three problems");
    assert_eq!(error.issues().len(), 3, "{error}");
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
        RecordKindV1::VotingRules,
        &company(),
        None,
        &notary(),
        RULES,
    )
    .expect("compose");

    let mismatched = good.replace("kind=\"voting-rules\"", "kind=\"share-structure\"");
    assert!(matches!(
        read_record(&mismatched),
        Err(IrenaError::KindMismatch {
            declared: RecordKindV1::ShareStructure,
            carried: RecordKindV1::VotingRules
        })
    ));

    assert!(matches!(
        read_record(&good.replace("version=\"1.0\"", "version=\"2.0\"")),
        Err(IrenaError::UnsupportedVersion { .. })
    ));
    for (label, xml) in [
        (
            "unknown kind",
            good.replace("kind=\"voting-rules\"", "kind=\"roll\""),
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
            good.replace("</irena-record>", &format!("{RULES}</irena-record>")),
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
        "kind=\"voting-rules\"",
        "kind=\"voting-rules\" supersedes=\"nope\"",
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
            RULES
        )
        .is_err()
    );
    assert!(
        compose_record(
            RecordKindV1::VotingRules,
            &company(),
            None,
            &notary(),
            SHARES
        )
        .is_err()
    );
    let mut blank = notary();
    blank.name = String::new();
    let error = compose_record(RecordKindV1::VotingRules, &company(), None, &blank, RULES)
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
    let declared = format!("<?xml version=\"1.0\"?>\n{RULES}\n");
    let xml = compose_record(
        RecordKindV1::VotingRules,
        &company(),
        None,
        &notary(),
        &declared,
    )
    .expect("ok");
    assert!(!xml.contains("<?xml"));
    assert!(xml.contains(RULES));
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

    // A minimal notarisation, without the optional attributes, also validates.
    let minimal = NotarisationV1 {
        id: NotaryIdV1::new("n").expect("id"),
        name: "N".to_owned(),
        address: None,
        at: NotaryTimeV1::parse("2026-01-01T00:00:00Z").expect("time"),
        statement: None,
        source_digest: None,
    };
    let xml = compose_record(RecordKindV1::VotingRules, &company(), None, &minimal, RULES)
        .expect("compose");
    check("minimal-notarisation", "irena-record-v1.xsd", &xml);
}
