//! Strict parsing of both documents: what is accepted, what is refused, and that every
//! content issue is reported together.

use bornite_core::{ChoiceV1, CoreError, FractionV1, WeightV1};
use bornite_rules::{
    AbstentionTreatmentV1, QuorumBasisV1, QuorumRuleV1, ThresholdBasisV1, ThresholdRuleV1,
    TieTreatmentV1, WeightRuleV1,
};
use bornite_xml::{
    XmlError, XmlIssueV1, read_rules_document, read_rules_document_with_limit, read_vote_document,
};
use std::path::Path;

const RULES: &str = r#"<voting-rules version="1.0">
  <weight type="electorate"/>
  <exclusions enabled="true"/>
  <quorum type="fraction" numerator="1" denominator="2" basis="effective-electorate"/>
  <threshold type="simple-majority" basis="votes-cast"/>
  <abstentions treatment="exclude"/>
  <tie treatment="reject"/>
</voting-rules>
"#;

const VOTE: &str = r#"<vote version="1.0">
  <electorate>
    <voter id="drone-01" weight="3"/>
    <voter id="drone-02"/>
    <voter id="drone-03" weight="2" excluded="true"/>
  </electorate>
  <ballots>
    <ballot voter="drone-01" choice="yes"/>
    <ballot voter="drone-02" choice="abstain"/>
  </ballots>
</vote>
"#;

fn issues(error: XmlError) -> Vec<XmlIssueV1> {
    match error {
        XmlError::Invalid { issues } => issues,
        other => panic!("expected content issues, got {other}"),
    }
}

#[test]
fn the_example_rules_document_reads_into_typed_rules() {
    let rules = read_rules_document(RULES).expect("valid");
    assert_eq!(rules.weight, WeightRuleV1::Electorate);
    assert!(rules.exclusions_enabled);
    assert_eq!(
        rules.quorum,
        QuorumRuleV1::Fraction {
            fraction: FractionV1::proportion(1, 2).expect("f"),
            basis: QuorumBasisV1::EffectiveElectorate
        }
    );
    assert_eq!(
        rules.threshold,
        ThresholdRuleV1::SimpleMajority {
            basis: ThresholdBasisV1::VotesCast
        }
    );
    assert_eq!(rules.abstentions, AbstentionTreatmentV1::Exclude);
    assert_eq!(rules.tie, TieTreatmentV1::Reject);
}

#[test]
fn every_rule_variant_reads() {
    let xml = r#"<?xml version="1.0"?>
<!-- comments and a declaration are fine -->
<voting-rules version="1.0">
  <weight type="equal"></weight>
  <exclusions enabled="false"/>
  <quorum type="absolute" weight="7"/>
  <threshold type="fraction" numerator="2" denominator="3" basis="total-electorate"/>
  <abstentions treatment="include"/>
  <tie treatment="accept"/>
</voting-rules>"#;
    let rules = read_rules_document(xml).expect("valid");
    assert_eq!(rules.weight, WeightRuleV1::Equal);
    assert!(!rules.exclusions_enabled);
    assert_eq!(
        rules.quorum,
        QuorumRuleV1::Absolute {
            weight: WeightV1::new(7).expect("w")
        }
    );
    assert_eq!(
        rules.threshold,
        ThresholdRuleV1::Fraction {
            fraction: FractionV1::proportion(2, 3).expect("f"),
            basis: ThresholdBasisV1::TotalElectorate
        }
    );
    assert_eq!(rules.abstentions, AbstentionTreatmentV1::Include);
    assert_eq!(rules.tie, TieTreatmentV1::Accept);

    let none = read_rules_document(&RULES.replace(
        r#"<quorum type="fraction" numerator="1" denominator="2" basis="effective-electorate"/>"#,
        r#"<quorum type="none"/>"#,
    ))
    .expect("valid");
    assert_eq!(none.quorum, QuorumRuleV1::None);
}

#[test]
fn the_example_vote_document_reads_with_defaults_applied() {
    let vote = read_vote_document(VOTE).expect("valid");
    let voters = vote.electorate.voters();
    assert_eq!(voters.len(), 3);
    assert_eq!(voters[0].id.as_str(), "drone-01");
    assert_eq!(voters[0].weight.value(), 3);
    assert_eq!(voters[1].weight, WeightV1::ONE, "weight defaults to 1");
    assert!(!voters[1].excluded, "excluded defaults to false");
    assert!(voters[2].excluded);
    assert_eq!(vote.ballots.len(), 2);
    assert_eq!(vote.ballots.ballots()[1].choice, ChoiceV1::Abstain);
}

#[test]
fn an_empty_electorate_and_no_ballots_are_valid_documents() {
    let vote =
        read_vote_document(r#"<vote version="1.0"><electorate/><ballots/></vote>"#).expect("valid");
    assert!(vote.electorate.is_empty());
    assert!(vote.ballots.is_empty());
}

#[test]
fn only_version_one_point_zero_is_read() {
    for bad in ["1", "1.1", "2.0", "v1.0", ""] {
        let error =
            read_rules_document(&RULES.replace(r#"version="1.0""#, &format!(r#"version="{bad}""#)))
                .expect_err("wrong version");
        assert!(
            matches!(error, XmlError::UnsupportedVersion { .. }),
            "{bad:?}: {error}"
        );
    }
    let missing =
        read_rules_document(&RULES.replace(r#" version="1.0""#, "")).expect_err("no version");
    assert_eq!(
        issues(missing),
        vec![XmlIssueV1::MissingAttribute {
            element: "voting-rules",
            attribute: "version"
        }]
    );
}

#[test]
fn unknown_elements_and_attributes_are_refused_not_skipped() {
    let extra_element =
        read_rules_document(&RULES.replace("</voting-rules>", "<delegation/></voting-rules>"));
    assert!(
        matches!(extra_element, Err(XmlError::Malformed { .. })),
        "{extra_element:?}"
    );

    let extra_attribute = read_rules_document(&RULES.replace(
        r#"<tie treatment="reject"/>"#,
        r#"<tie treatment="reject" casting="chair"/>"#,
    ));
    assert!(
        matches!(extra_attribute, Err(XmlError::Malformed { .. })),
        "{extra_attribute:?}"
    );

    let extra_root_attribute = read_vote_document(&VOTE.replace(
        r#"<vote version="1.0">"#,
        r#"<vote version="1.0" company="acme">"#,
    ));
    assert!(
        matches!(extra_root_attribute, Err(XmlError::Malformed { .. })),
        "{extra_root_attribute:?}"
    );
}

#[test]
fn attributes_a_rule_type_does_not_use_are_refused() {
    // A weight on a type="none" quorum is a mistake about what the rule does, not
    // something to ignore.
    let error = read_rules_document(&RULES.replace(
        r#"<quorum type="fraction" numerator="1" denominator="2" basis="effective-electorate"/>"#,
        r#"<quorum type="none" weight="5"/>"#,
    ))
    .expect_err("unused attribute");
    assert_eq!(
        issues(error),
        vec![XmlIssueV1::UnusedAttribute {
            element: "quorum",
            attribute: "weight",
            reason: "type is none"
        }]
    );

    let error = read_rules_document(&RULES.replace(
        r#"<threshold type="simple-majority" basis="votes-cast"/>"#,
        r#"<threshold type="simple-majority" numerator="2" denominator="3" basis="votes-cast"/>"#,
    ))
    .expect_err("unused attributes");
    assert_eq!(issues(error).len(), 2);
}

#[test]
fn a_fraction_type_needs_its_fraction() {
    let error = read_rules_document(&RULES.replace(
        r#"<threshold type="simple-majority" basis="votes-cast"/>"#,
        r#"<threshold type="fraction" basis="votes-cast"/>"#,
    ))
    .expect_err("missing fraction");
    assert_eq!(
        issues(error),
        vec![
            XmlIssueV1::MissingAttribute {
                element: "threshold",
                attribute: "denominator"
            },
            XmlIssueV1::MissingAttribute {
                element: "threshold",
                attribute: "numerator"
            },
        ]
    );
}

#[test]
fn invalid_fractions_are_refused() {
    let zero = read_rules_document(&RULES.replace(
        r#"numerator="1" denominator="2""#,
        r#"numerator="1" denominator="0""#,
    ))
    .expect_err("zero denominator");
    assert_eq!(
        issues(zero),
        vec![XmlIssueV1::Core {
            element: "quorum",
            error: CoreError::ZeroDenominator
        }]
    );

    let improper = read_rules_document(&RULES.replace(
        r#"numerator="1" denominator="2""#,
        r#"numerator="3" denominator="2""#,
    ))
    .expect_err("improper");
    assert_eq!(
        issues(improper),
        vec![XmlIssueV1::Core {
            element: "quorum",
            error: CoreError::ImproperFraction {
                numerator: 3,
                denominator: 2
            }
        }]
    );

    let negative = read_rules_document(&RULES.replace(r#"numerator="1""#, r#"numerator="-1""#))
        .expect_err("negative");
    assert!(matches!(
        issues(negative)[0],
        XmlIssueV1::InvalidValue {
            attribute: "numerator",
            ..
        }
    ));
}

#[test]
fn invalid_weights_and_voter_ids_are_refused() {
    let zero = read_vote_document(&VOTE.replace(r#"weight="3""#, r#"weight="0""#))
        .expect_err("zero weight");
    assert_eq!(
        issues(zero),
        vec![XmlIssueV1::Core {
            element: "voter",
            error: CoreError::ZeroWeight
        }]
    );

    let bad_id = read_vote_document(&VOTE.replace(r#"id="drone-02""#, r#"id="drone 02""#))
        .expect_err("bad id");
    assert!(matches!(
        issues(bad_id)[0],
        XmlIssueV1::Core {
            element: "voter",
            error: CoreError::InvalidVoterId { .. }
        }
    ));

    let bad_choice =
        read_vote_document(&VOTE.replace(r#"choice="yes""#, r#"choice="YES""#)).expect_err("case");
    assert!(matches!(
        issues(bad_choice)[0],
        XmlIssueV1::InvalidValue {
            element: "ballot",
            attribute: "choice",
            ..
        }
    ));
}

#[test]
fn duplicate_voters_and_ballots_are_refused() {
    let dup_voter =
        read_vote_document(&VOTE.replace(r#"<voter id="drone-02"/>"#, r#"<voter id="drone-01"/>"#))
            .expect_err("dup");
    assert!(matches!(
        issues(dup_voter)[0],
        XmlIssueV1::Core {
            element: "electorate",
            error: CoreError::DuplicateVoter { .. }
        }
    ));

    let dup_ballot = read_vote_document(&VOTE.replace(
        r#"<ballot voter="drone-02" choice="abstain"/>"#,
        r#"<ballot voter="drone-01" choice="no"/>"#,
    ))
    .expect_err("dup");
    assert!(matches!(
        issues(dup_ballot)[0],
        XmlIssueV1::Core {
            element: "ballots",
            error: CoreError::DuplicateBallot { .. }
        }
    ));
}

#[test]
fn every_content_issue_is_reported_together() {
    let xml = r#"<voting-rules version="1.0">
  <weight type="shares"/>
  <exclusions enabled="maybe"/>
  <quorum type="none" weight="1"/>
  <threshold type="fraction" numerator="9" denominator="2" basis="votes-cast"/>
  <abstentions treatment="exclude"/>
</voting-rules>"#;
    let found = issues(read_rules_document(xml).expect_err("many issues"));
    assert_eq!(found.len(), 5, "{found:?}");
    assert!(
        found
            .iter()
            .any(|i| matches!(i, XmlIssueV1::MissingElement { element: "tie", .. }))
    );
    assert!(found.iter().any(|i| matches!(
        i,
        XmlIssueV1::InvalidValue {
            element: "weight",
            ..
        }
    )));
    assert!(found.iter().any(|i| matches!(
        i,
        XmlIssueV1::InvalidValue {
            element: "exclusions",
            ..
        }
    )));
    assert!(found.iter().any(|i| matches!(
        i,
        XmlIssueV1::UnusedAttribute {
            element: "quorum",
            ..
        }
    )));
    assert!(found.iter().any(|i| matches!(
        i,
        XmlIssueV1::Core {
            element: "threshold",
            ..
        }
    )));
}

#[test]
fn repeated_and_missing_rule_elements_are_reported() {
    let repeated = read_rules_document(&RULES.replace(
        r#"<tie treatment="reject"/>"#,
        r#"<tie treatment="reject"/><tie treatment="accept"/>"#,
    ))
    .expect_err("repeated");
    assert_eq!(
        issues(repeated),
        vec![XmlIssueV1::RepeatedElement {
            parent: "voting-rules",
            element: "tie"
        }]
    );

    let missing = read_rules_document(&RULES.replace(r#"<abstentions treatment="exclude"/>"#, ""))
        .expect_err("missing");
    assert_eq!(
        issues(missing),
        vec![XmlIssueV1::MissingElement {
            parent: "voting-rules",
            element: "abstentions"
        }]
    );
}

#[test]
fn malformed_input_is_refused_with_a_position() {
    for bad in [
        "",
        "<",
        "<voting-rules version=\"1.0\">",
        "<vote/>",
        "<rules/>",
        &RULES.replace("</voting-rules>", "</voting-rules><extra/>"),
    ] {
        match read_rules_document(bad) {
            Err(XmlError::Malformed { .. }) => {}
            other => panic!("{bad:?}: expected malformed, got {other:?}"),
        }
    }
}

#[test]
fn a_wrong_root_is_refused() {
    assert!(matches!(
        read_rules_document(VOTE),
        Err(XmlError::Malformed { .. })
    ));
    assert!(matches!(
        read_vote_document(RULES),
        Err(XmlError::Malformed { .. })
    ));
}

#[test]
fn the_size_limit_is_checked_first() {
    let error = read_rules_document_with_limit(RULES, 16).expect_err("too large");
    assert!(matches!(error, XmlError::TooLarge { limit: 16, .. }));
}

#[test]
fn documents_are_read_the_same_whatever_their_layout() {
    let compact = RULES.replace('\n', "").replace("  ", "");
    assert_eq!(
        read_rules_document(&compact).expect("compact"),
        read_rules_document(RULES).expect("pretty")
    );
    let reordered = VOTE
        .replace(r#"<voter id="drone-01" weight="3"/>"#, "")
        .replace(
            "</electorate>",
            r#"<voter id="drone-01" weight="3"/></electorate>"#,
        );
    assert_eq!(
        read_vote_document(&reordered).expect("reordered"),
        read_vote_document(VOTE).expect("original")
    );
}

#[test]
fn example_documents_validate_against_the_published_schemas() {
    if std::process::Command::new("xmllint")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping: xmllint is not installed");
        return;
    }
    let schemas = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas")
        .canonicalize()
        .expect("schemas");
    let dir = tempfile::TempDir::new().expect("temp dir");
    for (name, schema, xml) in [
        ("rules", "bornite-voting-rules-v1.xsd", RULES),
        ("vote", "bornite-vote-v1.xsd", VOTE),
    ] {
        let file = dir.path().join(format!("{name}.xml"));
        std::fs::write(&file, xml).expect("write");
        let output = std::process::Command::new("xmllint")
            .args(["--noout", "--schema"])
            .arg(schemas.join(schema))
            .arg(&file)
            .output()
            .expect("run xmllint");
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
