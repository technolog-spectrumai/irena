//! The strict element parsers.
//!
//! Each parser consumes exactly one element from a [`quick_xml::Reader`] positioned just
//! after that element's start tag. They are public so that another document format can
//! embed a `<voting-rules>` or `<electorate>` element and parse it with exactly this
//! code — the same bytes always mean the same rules, wherever they appear.
//!
//! Strictness: an unknown element or attribute is an error, never skipped. Attribute
//! content problems are collected across the element and reported together.

use crate::error::{XmlError, XmlIssueV1};
use bornite_core::{
    BallotSetV1, BallotV1, ChoiceV1, CoreError, ElectorateV1, FractionV1, VoterIdV1, VoterV1,
    WeightV1,
};
use bornite_rules::{
    AbstentionTreatmentV1, QuorumBasisV1, QuorumRuleV1, ThresholdBasisV1, ThresholdRuleV1,
    TieTreatmentV1, VotingRulesV1, WeightRuleV1,
};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

/// The only version string V1 accepts.
pub const VERSION: &str = "1.0";

/// A reader over in-memory bytes.
pub type XmlReader<'a> = Reader<&'a [u8]>;

/// Attributes of one element, with the element name for diagnostics.
pub struct Attributes {
    element: &'static str,
    pairs: Vec<(String, String)>,
    used: Vec<bool>,
}

impl Attributes {
    /// Collects an element's attributes.
    ///
    /// # Errors
    ///
    /// Returns [`XmlError::Malformed`] for an attribute that cannot be decoded.
    pub fn of(
        reader: &XmlReader<'_>,
        element: &'static str,
        start: &BytesStart<'_>,
    ) -> Result<Self, XmlError> {
        let mut pairs = Vec::new();
        for attribute in start.attributes() {
            let attribute =
                attribute.map_err(|error| malformed(reader, format!("bad attribute: {error}")))?;
            let key = attribute.key.as_ref().to_owned();
            let value = attribute
                .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                .map_err(|error| malformed(reader, format!("bad attribute value: {error}")))?
                .into_owned();
            pairs.push((key, value));
        }
        let used = vec![false; pairs.len()];
        Ok(Self {
            element,
            pairs,
            used,
        })
    }

    /// Takes an attribute's value, marking it used.
    pub fn take(&mut self, name: &'static str) -> Option<String> {
        let index = self.pairs.iter().position(|(key, _)| key == name)?;
        self.used[index] = true;
        Some(self.pairs[index].1.clone())
    }

    /// Takes a required attribute, recording an issue if absent.
    pub fn require(&mut self, name: &'static str, issues: &mut Vec<XmlIssueV1>) -> Option<String> {
        let value = self.take(name);
        if value.is_none() {
            issues.push(XmlIssueV1::MissingAttribute {
                element: self.element,
                attribute: name,
            });
        }
        value
    }

    /// Records an issue for any attribute nobody took.
    ///
    /// # Errors
    ///
    /// Returns [`XmlError::Malformed`] naming the first unknown attribute: unknown
    /// attributes are structural, because the reader cannot know what they meant.
    pub fn finish(self, reader: &XmlReader<'_>) -> Result<(), XmlError> {
        for (index, (key, _)) in self.pairs.iter().enumerate() {
            if !self.used[index] {
                return Err(malformed(
                    reader,
                    format!("<{}> has an unknown attribute {key:?}", self.element),
                ));
            }
        }
        Ok(())
    }

    /// Records an issue if `name` is present, because it is meaningless here.
    pub fn forbid(
        &mut self,
        name: &'static str,
        reason: &'static str,
        issues: &mut Vec<XmlIssueV1>,
    ) {
        if self.take(name).is_some() {
            issues.push(XmlIssueV1::UnusedAttribute {
                element: self.element,
                attribute: name,
                reason,
            });
        }
    }
}

/// Reports a structural problem at the reader's position.
#[must_use]
pub fn malformed(reader: &XmlReader<'_>, detail: impl Into<String>) -> XmlError {
    XmlError::Malformed {
        position: reader.buffer_position(),
        detail: detail.into(),
    }
}

/// Reads the next event, treating reader errors as malformed input.
///
/// # Errors
///
/// Returns [`XmlError::Malformed`] when the input is not well-formed.
pub fn next_event<'a>(reader: &mut XmlReader<'a>) -> Result<Event<'a>, XmlError> {
    let position = reader.buffer_position();
    reader.read_event().map_err(|error| XmlError::Malformed {
        position,
        detail: error.to_string(),
    })
}

/// Consumes events until the matching end tag of an element that must be empty.
///
/// # Errors
///
/// Returns [`XmlError::Malformed`] if the element has children.
pub fn expect_empty(reader: &mut XmlReader<'_>, element: &str) -> Result<(), XmlError> {
    loop {
        match next_event(reader)? {
            Event::Text(_) | Event::Comment(_) => {}
            Event::End(_) => return Ok(()),
            other => {
                return Err(malformed(
                    reader,
                    format!("<{element}> must be empty, found {}", describe(&other)),
                ));
            }
        }
    }
}

/// Describes an event for an error message.
#[must_use]
pub fn describe(event: &Event<'_>) -> String {
    match event {
        Event::Start(e) | Event::Empty(e) => format!("<{}>", e.name().as_ref()),
        Event::End(e) => format!("</{}>", e.name().as_ref()),
        Event::Text(_) => "text".to_owned(),
        Event::CData(_) => "a CDATA section".to_owned(),
        Event::Comment(_) => "a comment".to_owned(),
        Event::Decl(_) => "an XML declaration".to_owned(),
        Event::PI(_) => "a processing instruction".to_owned(),
        Event::DocType(_) => "a doctype".to_owned(),
        Event::GeneralRef(_) => "an entity reference".to_owned(),
        Event::Eof => "the end of the document".to_owned(),
    }
}

/// Checks a `version` attribute value.
///
/// # Errors
///
/// Returns [`XmlError::UnsupportedVersion`] unless it is exactly `1.0`.
pub fn check_version(value: &str) -> Result<(), XmlError> {
    if value == VERSION {
        Ok(())
    } else {
        Err(XmlError::UnsupportedVersion {
            found: value.to_owned(),
        })
    }
}

fn enum_value<T: Copy>(
    element: &'static str,
    attribute: &'static str,
    value: &str,
    options: &[(&str, T)],
    issues: &mut Vec<XmlIssueV1>,
) -> Option<T> {
    if let Some((_, parsed)) = options.iter().find(|(name, _)| *name == value) {
        return Some(*parsed);
    }
    let allowed: Vec<&str> = options.iter().map(|(name, _)| *name).collect();
    issues.push(XmlIssueV1::InvalidValue {
        element,
        attribute,
        value: value.to_owned(),
        reason: format!("expected one of {}", allowed.join(", ")),
    });
    None
}

fn bool_value(
    element: &'static str,
    attribute: &'static str,
    value: &str,
    issues: &mut Vec<XmlIssueV1>,
) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        other => {
            issues.push(XmlIssueV1::InvalidValue {
                element,
                attribute,
                value: other.to_owned(),
                reason: "expected true or false".to_owned(),
            });
            None
        }
    }
}

fn u64_value(
    element: &'static str,
    attribute: &'static str,
    value: &str,
    issues: &mut Vec<XmlIssueV1>,
) -> Option<u64> {
    match value.parse::<u64>() {
        Ok(number) if !value.starts_with('+') => Some(number),
        _ => {
            issues.push(XmlIssueV1::InvalidValue {
                element,
                attribute,
                value: value.to_owned(),
                reason: "expected a non-negative integer".to_owned(),
            });
            None
        }
    }
}

fn core_issue(element: &'static str, error: CoreError) -> XmlIssueV1 {
    XmlIssueV1::Core { element, error }
}

/// Parses the body of a `<voting-rules>` element whose start tag has been read.
///
/// `start` is that start tag. On success the reader is positioned just after the
/// matching end tag.
///
/// # Errors
///
/// Returns [`XmlError::Malformed`] for structural problems, [`XmlError::UnsupportedVersion`]
/// for any version but `1.0`, and [`XmlError::Invalid`] carrying every content issue.
pub fn parse_voting_rules(
    reader: &mut XmlReader<'_>,
    start: &BytesStart<'_>,
) -> Result<VotingRulesV1, XmlError> {
    let mut issues = Vec::new();
    let mut attributes = Attributes::of(reader, "voting-rules", start)?;
    let version = attributes.require("version", &mut issues);
    attributes.finish(reader)?;
    if let Some(version) = version {
        check_version(&version)?;
    }

    let mut weight = None;
    let mut exclusions = None;
    let mut quorum = None;
    let mut threshold = None;
    let mut abstentions = None;
    let mut tie = None;

    loop {
        let event = next_event(reader)?;
        let (child, is_start) = match event {
            Event::Text(_) | Event::Comment(_) => continue,
            Event::Empty(child) => (child, false),
            Event::Start(child) => (child, true),
            Event::End(_) => break,
            other => {
                return Err(malformed(
                    reader,
                    format!("unexpected {} in <voting-rules>", describe(&other)),
                ));
            }
        };
        let name = child.name().as_ref().to_owned();
        match name.as_str() {
            "weight" => set_once(
                &mut weight,
                "weight",
                parse_weight(reader, &child, &mut issues)?,
                &mut issues,
            ),
            "exclusions" => set_once(
                &mut exclusions,
                "exclusions",
                parse_exclusions(reader, &child, &mut issues)?,
                &mut issues,
            ),
            "quorum" => set_once(
                &mut quorum,
                "quorum",
                parse_quorum(reader, &child, &mut issues)?,
                &mut issues,
            ),
            "threshold" => set_once(
                &mut threshold,
                "threshold",
                parse_threshold(reader, &child, &mut issues)?,
                &mut issues,
            ),
            "abstentions" => set_once(
                &mut abstentions,
                "abstentions",
                parse_abstentions(reader, &child, &mut issues)?,
                &mut issues,
            ),
            "tie" => set_once(
                &mut tie,
                "tie",
                parse_tie(reader, &child, &mut issues)?,
                &mut issues,
            ),
            other => {
                return Err(malformed(
                    reader,
                    format!("<voting-rules> has an unknown child <{other}>"),
                ));
            }
        }
        // A leaf written as <x></x> still has an end tag to consume; <x/> does not.
        if is_start {
            expect_empty(reader, &name)?;
        }
    }

    for (present, name) in [
        (weight.is_some(), "weight"),
        (exclusions.is_some(), "exclusions"),
        (quorum.is_some(), "quorum"),
        (threshold.is_some(), "threshold"),
        (abstentions.is_some(), "abstentions"),
        (tie.is_some(), "tie"),
    ] {
        if !present {
            issues.push(XmlIssueV1::MissingElement {
                parent: "voting-rules",
                element: name,
            });
        }
    }

    finish_issues(issues)?;
    Ok(VotingRulesV1 {
        weight: weight.flatten().expect("checked"),
        exclusions_enabled: exclusions.flatten().expect("checked"),
        quorum: quorum.flatten().expect("checked"),
        threshold: threshold.flatten().expect("checked"),
        abstentions: abstentions.flatten().expect("checked"),
        tie: tie.flatten().expect("checked"),
    })
}

fn set_once<T>(
    slot: &mut Option<Option<T>>,
    element: &'static str,
    value: Option<T>,
    issues: &mut Vec<XmlIssueV1>,
) {
    if slot.is_some() {
        issues.push(XmlIssueV1::RepeatedElement {
            parent: "voting-rules",
            element,
        });
    } else {
        *slot = Some(value);
    }
}

fn parse_weight(
    reader: &mut XmlReader<'_>,
    child: &BytesStart<'_>,
    issues: &mut Vec<XmlIssueV1>,
) -> Result<Option<WeightRuleV1>, XmlError> {
    let mut attributes = Attributes::of(reader, "weight", child)?;
    let value = attributes.require("type", issues).and_then(|v| {
        enum_value(
            "weight",
            "type",
            &v,
            &[
                ("equal", WeightRuleV1::Equal),
                ("electorate", WeightRuleV1::Electorate),
            ],
            issues,
        )
    });
    attributes.finish(reader)?;
    Ok(value)
}

fn parse_exclusions(
    reader: &mut XmlReader<'_>,
    child: &BytesStart<'_>,
    issues: &mut Vec<XmlIssueV1>,
) -> Result<Option<bool>, XmlError> {
    let mut attributes = Attributes::of(reader, "exclusions", child)?;
    let value = attributes
        .require("enabled", issues)
        .and_then(|v| bool_value("exclusions", "enabled", &v, issues));
    attributes.finish(reader)?;
    Ok(value)
}

fn parse_quorum(
    reader: &mut XmlReader<'_>,
    child: &BytesStart<'_>,
    issues: &mut Vec<XmlIssueV1>,
) -> Result<Option<QuorumRuleV1>, XmlError> {
    let mut attributes = Attributes::of(reader, "quorum", child)?;
    let kind = attributes.require("type", issues);
    let value = match kind.as_deref() {
        Some("none") => {
            attributes.forbid("weight", "type is none", issues);
            attributes.forbid("numerator", "type is none", issues);
            attributes.forbid("denominator", "type is none", issues);
            attributes.forbid("basis", "type is none", issues);
            Some(QuorumRuleV1::None)
        }
        Some("absolute") => {
            attributes.forbid("numerator", "type is absolute", issues);
            attributes.forbid("denominator", "type is absolute", issues);
            attributes.forbid("basis", "type is absolute", issues);
            attributes
                .require("weight", issues)
                .and_then(|v| u64_value("quorum", "weight", &v, issues))
                .and_then(|w| {
                    WeightV1::new(w)
                        .map_err(|e| issues.push(core_issue("quorum", e)))
                        .ok()
                })
                .map(|weight| QuorumRuleV1::Absolute { weight })
        }
        Some("fraction") => {
            attributes.forbid("weight", "type is fraction", issues);
            let numerator = attributes
                .require("numerator", issues)
                .and_then(|v| u64_value("quorum", "numerator", &v, issues));
            let denominator = attributes
                .require("denominator", issues)
                .and_then(|v| u64_value("quorum", "denominator", &v, issues));
            let basis = attributes.require("basis", issues).and_then(|v| {
                enum_value(
                    "quorum",
                    "basis",
                    &v,
                    &[
                        ("total-electorate", QuorumBasisV1::TotalElectorate),
                        ("effective-electorate", QuorumBasisV1::EffectiveElectorate),
                    ],
                    issues,
                )
            });
            match (numerator, denominator, basis) {
                (Some(n), Some(d), Some(basis)) => FractionV1::proportion(n, d)
                    .map_err(|e| issues.push(core_issue("quorum", e)))
                    .ok()
                    .map(|fraction| QuorumRuleV1::Fraction { fraction, basis }),
                _ => None,
            }
        }
        Some(other) => {
            issues.push(XmlIssueV1::InvalidValue {
                element: "quorum",
                attribute: "type",
                value: other.to_owned(),
                reason: "expected one of none, absolute, fraction".to_owned(),
            });
            // Drain the remaining attributes so they are not also reported as unknown.
            for name in ["weight", "numerator", "denominator", "basis"] {
                attributes.take(name);
            }
            None
        }
        None => {
            for name in ["weight", "numerator", "denominator", "basis"] {
                attributes.take(name);
            }
            None
        }
    };
    attributes.finish(reader)?;
    Ok(value)
}

fn parse_threshold(
    reader: &mut XmlReader<'_>,
    child: &BytesStart<'_>,
    issues: &mut Vec<XmlIssueV1>,
) -> Result<Option<ThresholdRuleV1>, XmlError> {
    let mut attributes = Attributes::of(reader, "threshold", child)?;
    let kind = attributes.require("type", issues);
    let basis = attributes.require("basis", issues).and_then(|v| {
        enum_value(
            "threshold",
            "basis",
            &v,
            &[
                ("votes-cast", ThresholdBasisV1::VotesCast),
                (
                    "effective-electorate",
                    ThresholdBasisV1::EffectiveElectorate,
                ),
                ("total-electorate", ThresholdBasisV1::TotalElectorate),
            ],
            issues,
        )
    });
    let value = match kind.as_deref() {
        Some("simple-majority") => {
            attributes.forbid("numerator", "type is simple-majority", issues);
            attributes.forbid("denominator", "type is simple-majority", issues);
            basis.map(|basis| ThresholdRuleV1::SimpleMajority { basis })
        }
        Some("fraction") => {
            let numerator = attributes
                .require("numerator", issues)
                .and_then(|v| u64_value("threshold", "numerator", &v, issues));
            let denominator = attributes
                .require("denominator", issues)
                .and_then(|v| u64_value("threshold", "denominator", &v, issues));
            match (numerator, denominator, basis) {
                (Some(n), Some(d), Some(basis)) => FractionV1::proportion(n, d)
                    .map_err(|e| issues.push(core_issue("threshold", e)))
                    .ok()
                    .map(|fraction| ThresholdRuleV1::Fraction { fraction, basis }),
                _ => None,
            }
        }
        Some(other) => {
            issues.push(XmlIssueV1::InvalidValue {
                element: "threshold",
                attribute: "type",
                value: other.to_owned(),
                reason: "expected one of simple-majority, fraction".to_owned(),
            });
            attributes.take("numerator");
            attributes.take("denominator");
            None
        }
        None => {
            attributes.take("numerator");
            attributes.take("denominator");
            None
        }
    };
    attributes.finish(reader)?;
    Ok(value)
}

fn parse_abstentions(
    reader: &mut XmlReader<'_>,
    child: &BytesStart<'_>,
    issues: &mut Vec<XmlIssueV1>,
) -> Result<Option<AbstentionTreatmentV1>, XmlError> {
    let mut attributes = Attributes::of(reader, "abstentions", child)?;
    let value = attributes.require("treatment", issues).and_then(|v| {
        enum_value(
            "abstentions",
            "treatment",
            &v,
            &[
                ("exclude", AbstentionTreatmentV1::Exclude),
                ("include", AbstentionTreatmentV1::Include),
            ],
            issues,
        )
    });
    attributes.finish(reader)?;
    Ok(value)
}

fn parse_tie(
    reader: &mut XmlReader<'_>,
    child: &BytesStart<'_>,
    issues: &mut Vec<XmlIssueV1>,
) -> Result<Option<TieTreatmentV1>, XmlError> {
    let mut attributes = Attributes::of(reader, "tie", child)?;
    let value = attributes.require("treatment", issues).and_then(|v| {
        enum_value(
            "tie",
            "treatment",
            &v,
            &[
                ("reject", TieTreatmentV1::Reject),
                ("accept", TieTreatmentV1::Accept),
            ],
            issues,
        )
    });
    attributes.finish(reader)?;
    Ok(value)
}

fn finish_issues(mut issues: Vec<XmlIssueV1>) -> Result<(), XmlError> {
    if issues.is_empty() {
        Ok(())
    } else {
        issues.sort();
        issues.dedup();
        Err(XmlError::Invalid { issues })
    }
}

/// Parses an `<electorate>` element whose start tag has been read.
///
/// `is_empty` says whether the tag was `<electorate/>`; `<electorate></electorate>` and
/// `<electorate>…</electorate>` pass `false`.
///
/// # Errors
///
/// As [`parse_voting_rules`].
pub fn parse_electorate(
    reader: &mut XmlReader<'_>,
    start: &BytesStart<'_>,
    is_empty: bool,
) -> Result<ElectorateV1, XmlError> {
    let mut issues = Vec::new();
    Attributes::of(reader, "electorate", start)?.finish(reader)?;
    let mut voters = Vec::new();
    // `<electorate/>` has no end tag to consume and no children.
    if is_empty {
        return ElectorateV1::new(voters).map_err(|error| XmlError::Invalid {
            issues: vec![core_issue("electorate", error)],
        });
    }
    loop {
        match next_event(reader)? {
            Event::Text(_) | Event::Comment(_) => {}
            Event::Empty(child) => voters.extend(parse_voter(reader, &child, &mut issues)?),
            Event::Start(child) => {
                let voter = parse_voter(reader, &child, &mut issues)?;
                expect_empty(reader, "voter")?;
                voters.extend(voter);
            }
            Event::End(_) => break,
            other => {
                return Err(malformed(
                    reader,
                    format!("unexpected {} in <electorate>", describe(&other)),
                ));
            }
        }
    }
    finish_issues(issues)?;
    ElectorateV1::new(voters).map_err(|error| XmlError::Invalid {
        issues: vec![core_issue("electorate", error)],
    })
}

fn parse_voter(
    reader: &mut XmlReader<'_>,
    child: &BytesStart<'_>,
    issues: &mut Vec<XmlIssueV1>,
) -> Result<Option<VoterV1>, XmlError> {
    if child.name().as_ref() != "voter" {
        return Err(malformed(
            reader,
            format!(
                "<electorate> has an unknown child <{}>",
                child.name().as_ref()
            ),
        ));
    }
    let mut attributes = Attributes::of(reader, "voter", child)?;
    let id = attributes.require("id", issues).and_then(|v| {
        VoterIdV1::new(v)
            .map_err(|e| issues.push(core_issue("voter", e)))
            .ok()
    });
    let weight = match attributes.take("weight") {
        None => Some(WeightV1::ONE),
        Some(v) => u64_value("voter", "weight", &v, issues).and_then(|w| {
            WeightV1::new(w)
                .map_err(|e| issues.push(core_issue("voter", e)))
                .ok()
        }),
    };
    let excluded = match attributes.take("excluded") {
        None => Some(false),
        Some(v) => bool_value("voter", "excluded", &v, issues),
    };
    attributes.finish(reader)?;
    Ok(match (id, weight, excluded) {
        (Some(id), Some(weight), Some(excluded)) => Some(VoterV1 {
            id,
            weight,
            excluded,
        }),
        _ => None,
    })
}

/// Parses a `<ballots>` element whose start tag has been read.
///
/// `is_empty` as for [`parse_electorate`].
///
/// # Errors
///
/// As [`parse_voting_rules`].
pub fn parse_ballots(
    reader: &mut XmlReader<'_>,
    start: &BytesStart<'_>,
    is_empty: bool,
) -> Result<BallotSetV1, XmlError> {
    let mut issues = Vec::new();
    Attributes::of(reader, "ballots", start)?.finish(reader)?;
    let mut ballots = Vec::new();
    if is_empty {
        return Ok(BallotSetV1::empty());
    }
    loop {
        match next_event(reader)? {
            Event::Text(_) | Event::Comment(_) => {}
            Event::Empty(child) => ballots.extend(parse_ballot(reader, &child, &mut issues)?),
            Event::Start(child) => {
                let ballot = parse_ballot(reader, &child, &mut issues)?;
                expect_empty(reader, "ballot")?;
                ballots.extend(ballot);
            }
            Event::End(_) => break,
            other => {
                return Err(malformed(
                    reader,
                    format!("unexpected {} in <ballots>", describe(&other)),
                ));
            }
        }
    }
    finish_issues(issues)?;
    BallotSetV1::new(ballots).map_err(|error| XmlError::Invalid {
        issues: vec![core_issue("ballots", error)],
    })
}

fn parse_ballot(
    reader: &mut XmlReader<'_>,
    child: &BytesStart<'_>,
    issues: &mut Vec<XmlIssueV1>,
) -> Result<Option<BallotV1>, XmlError> {
    if child.name().as_ref() != "ballot" {
        return Err(malformed(
            reader,
            format!("<ballots> has an unknown child <{}>", child.name().as_ref()),
        ));
    }
    let mut attributes = Attributes::of(reader, "ballot", child)?;
    let voter = attributes.require("voter", issues).and_then(|v| {
        VoterIdV1::new(v)
            .map_err(|e| issues.push(core_issue("ballot", e)))
            .ok()
    });
    let choice = attributes.require("choice", issues).and_then(|v| {
        enum_value(
            "ballot",
            "choice",
            &v,
            &[
                ("yes", ChoiceV1::Yes),
                ("no", ChoiceV1::No),
                ("abstain", ChoiceV1::Abstain),
            ],
            issues,
        )
    });
    attributes.finish(reader)?;
    Ok(match (voter, choice) {
        (Some(voter), Some(choice)) => Some(BallotV1 { voter, choice }),
        _ => None,
    })
}
