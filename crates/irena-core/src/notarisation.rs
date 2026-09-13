//! Who attested to a record, and when.
//!
//! Every record on the ledger is put there by hand, by someone with the authority and
//! the documents to say the company is now like this. The notarisation records who
//! that was, when they say the change happened, and optionally which external document
//! backs it. Irena stores and reproduces all of it and interprets none of it: the
//! ledger's own order is the only order, and a notary's date-time is attested
//! metadata, never used to resolve or sequence anything.

use crate::error::{IrenaError, IssueV1};
use prunella_core::Hash;

/// Maximum length of a notary id, in bytes.
pub const MAX_NOTARY_ID_LEN: usize = 128;

/// Identifies a notary.
///
/// 1 to 128 bytes of ASCII from `A-Z a-z 0-9 . _ : + @ -`, compared byte for byte —
/// the same grammar as a voter id, so a registrar's reference, an email-like handle or
/// a licence number all fit.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct NotaryIdV1(String);

impl NotaryIdV1 {
    /// Validates and wraps a notary id.
    ///
    /// # Errors
    ///
    /// Returns [`IrenaError::Invalid`] with one [`IssueV1::InvalidValue`].
    pub fn new(value: impl Into<String>) -> Result<Self, IrenaError> {
        let value = value.into();
        let reject = |reason: &str| {
            IrenaError::invalid(vec![IssueV1::InvalidValue {
                element: "notarisation",
                attribute: "id",
                value: value.clone(),
                reason: reason.to_owned(),
            }])
        };
        if value.is_empty() {
            return Err(reject("must not be empty"));
        }
        if value.len() > MAX_NOTARY_ID_LEN {
            return Err(reject("must be at most 128 bytes"));
        }
        if !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'+' | b'@' | b'-')
        }) {
            return Err(reject(
                "may only contain ASCII letters, digits, '.', '_', ':', '+', '@' and '-'",
            ));
        }
        Ok(Self(value))
    }

    /// The id text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for NotaryIdV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::fmt::Debug for NotaryIdV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// A date-time as a notary states it: RFC 3339, UTC, seconds precision.
///
/// Exactly `YYYY-MM-DDTHH:MM:SSZ`, nothing else: no offsets, no fractional seconds, no
/// lowercase `t` or `z`, no leap second. One instant has one spelling, so two notaries
/// writing the same moment write the same bytes, and the string's byte order is its
/// chronological order.
///
/// It is **attested metadata**. Irena never orders, resolves or expires anything by
/// it; the ledger's order is the only order. It is here so a reader of the chain can
/// see when the notary says the change took effect.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct NotaryTimeV1(String);

impl NotaryTimeV1 {
    /// Parses the canonical form.
    ///
    /// # Errors
    ///
    /// Returns [`IrenaError::Invalid`] with one [`IssueV1::InvalidValue`] saying what is
    /// wrong: the shape, or a field out of range (a 31 April, a 25th hour).
    pub fn parse(text: &str) -> Result<Self, IrenaError> {
        let reject = |reason: &str| {
            IrenaError::invalid(vec![IssueV1::InvalidValue {
                element: "notarisation",
                attribute: "at",
                value: text.to_owned(),
                reason: reason.to_owned(),
            }])
        };
        let bytes = text.as_bytes();
        if bytes.len() != 20 {
            return Err(reject("must be exactly YYYY-MM-DDTHH:MM:SSZ"));
        }
        for (index, byte) in bytes.iter().enumerate() {
            let expected_digit = !matches!(index, 4 | 7 | 10 | 13 | 16 | 19);
            let ok = match index {
                4 | 7 => *byte == b'-',
                10 => *byte == b'T',
                13 | 16 => *byte == b':',
                19 => *byte == b'Z',
                _ => byte.is_ascii_digit(),
            };
            if !ok {
                return Err(reject(if expected_digit {
                    "must be exactly YYYY-MM-DDTHH:MM:SSZ with ASCII digits"
                } else {
                    "must use '-', 'T', ':' and a trailing 'Z' exactly as in YYYY-MM-DDTHH:MM:SSZ"
                }));
            }
        }
        let field =
            |from: usize, to: usize| -> u32 { text[from..to].parse().expect("checked digits") };
        let (year, month, day) = (field(0, 4), field(5, 7), field(8, 10));
        let (hour, minute, second) = (field(11, 13), field(14, 16), field(17, 19));
        if !(1..=12).contains(&month) {
            return Err(reject("month must be 01 to 12"));
        }
        let days = days_in_month(year, month);
        if day == 0 || day > days {
            return Err(reject("day is outside the month"));
        }
        if hour > 23 {
            return Err(reject("hour must be 00 to 23"));
        }
        if minute > 59 {
            return Err(reject("minute must be 00 to 59"));
        }
        if second > 59 {
            return Err(reject(
                "second must be 00 to 59; leap seconds are not accepted",
            ));
        }
        Ok(Self(text.to_owned()))
    }

    /// The canonical text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for NotaryTimeV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::fmt::Debug for NotaryTimeV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl core::str::FromStr for NotaryTimeV1 {
    type Err = IrenaError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            let leap =
                year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
            if leap { 29 } else { 28 }
        }
    }
}

/// Who attests to a record, when, and on what basis.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct NotarisationV1 {
    /// The notary's stable identifier.
    pub id: NotaryIdV1,
    /// The notary's name. Non-empty free text.
    pub name: String,
    /// The notary's address. Free text.
    pub address: Option<String>,
    /// When the notary says this change took effect.
    pub at: NotaryTimeV1,
    /// Free text.
    pub statement: Option<String>,
    /// Digest of an external document the notary attests to — a certificate, a signed
    /// register, minutes. Opaque.
    pub source_digest: Option<Hash>,
}

impl NotarisationV1 {
    /// Validates the free-text fields: the name must be non-empty.
    ///
    /// # Errors
    ///
    /// Returns [`IrenaError::Invalid`] listing every problem.
    pub fn validate(&self) -> Result<(), IrenaError> {
        let mut issues = Vec::new();
        if self.name.trim().is_empty() {
            issues.push(IssueV1::InvalidValue {
                element: "notarisation",
                attribute: "name",
                value: self.name.clone(),
                reason: "must not be empty".to_owned(),
            });
        }
        if issues.is_empty() {
            Ok(())
        } else {
            Err(IrenaError::invalid(issues))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_times_parse_and_others_do_not() {
        for good in [
            "2026-03-01T09:30:00Z",
            "2024-02-29T23:59:59Z",
            "0001-01-01T00:00:00Z",
            "9999-12-31T23:59:59Z",
            "2000-02-29T12:00:00Z",
        ] {
            let time = NotaryTimeV1::parse(good).unwrap_or_else(|e| panic!("{good}: {e}"));
            assert_eq!(time.as_str(), good);
        }
        for bad in [
            "",
            "2026-03-01",
            "2026-03-01T09:30:00",
            "2026-03-01T09:30:00+00:00",
            "2026-03-01T09:30:00.000Z",
            "2026-03-01t09:30:00z",
            "2026-03-01 09:30:00Z",
            "2026-13-01T09:30:00Z",
            "2026-00-01T09:30:00Z",
            "2026-04-31T09:30:00Z",
            "2026-02-29T09:30:00Z",
            "1900-02-29T09:30:00Z",
            "2026-03-00T09:30:00Z",
            "2026-03-01T24:00:00Z",
            "2026-03-01T09:60:00Z",
            "2026-03-01T09:30:60Z",
            "２026-03-01T09:30:00Z",
        ] {
            let error = NotaryTimeV1::parse(bad).expect_err(bad);
            assert!(
                matches!(
                    error.issues(),
                    [IssueV1::InvalidValue {
                        element: "notarisation",
                        attribute: "at",
                        ..
                    }]
                ),
                "{bad}: {error}"
            );
        }
    }

    #[test]
    fn byte_order_is_chronological_order() {
        let earlier = NotaryTimeV1::parse("2026-03-01T09:30:00Z").expect("ok");
        let later = NotaryTimeV1::parse("2026-03-01T09:30:01Z").expect("ok");
        assert!(earlier < later);
    }

    #[test]
    fn notary_ids_follow_the_voter_id_grammar() {
        assert!(NotaryIdV1::new("notary-07").is_ok());
        assert!(NotaryIdV1::new("jane.roe@registrar.example").is_ok());
        assert!(NotaryIdV1::new("").is_err());
        assert!(NotaryIdV1::new("jane roe").is_err());
        assert!(NotaryIdV1::new("x".repeat(129)).is_err());
    }
}
