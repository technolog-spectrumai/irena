//! Authorisation: who may sign record transactions.
//!
//! Every record reaches the ledger as a Prunella transaction signed by some key. The
//! authorisation record says whose key that may be, per **family** of record:
//!
//! | Family | Records |
//! |---|---|
//! | `company` | amendments to any part: identity, share register, channel set, identities, authorisation |
//! | `governance` | meetings, votes, decisions, resolutions and executions |
//!
//! A signer row names a person (from the identities record) and a family. The ledger
//! refuses a company record whose transaction signer is not a `company` signer's
//! current key, and every governance verifier reports the same for its family.
//!
//! **Bare publishing is real power**: a `company` signer may rewrite the register
//! without any channel deciding anything. That is the notary's route by design, and
//! this record is exactly *who* may take it. A company must always have at least one
//! `company` signer holding a key, or nothing could ever be amended again; that is the
//! lockout rule, half of which lives here (a record with no `company` row is refused)
//! and half in the ledger (a `company` row whose person has no key counts for nothing).

use crate::error::{IrenaError, IssueV1};
use bornite_core::VoterIdV1;

/// Which records a signer row covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordFamilyV1 {
    /// Amendments to a company part.
    Company,
    /// Meetings, votes, decisions, resolutions, executions.
    Governance,
}

impl RecordFamilyV1 {
    /// Both families, in a fixed order.
    pub const ALL: [Self; 2] = [Self::Company, Self::Governance];

    /// The attribute text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Company => "company",
            Self::Governance => "governance",
        }
    }

    /// Parses the attribute text.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|family| family.as_str() == text)
    }
}

impl core::fmt::Display for RecordFamilyV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One signer row: a person may sign one family of records.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct SignerV1 {
    /// The person, by identity id.
    pub person: VoterIdV1,
    /// The family they may sign.
    pub family: RecordFamilyV1,
}

/// A validated authorisation record.
///
/// Sorted and free of duplicate rows from the moment it is built, and never without a
/// `company` signer.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct AuthorisationV1 {
    signers: Vec<SignerV1>,
}

impl AuthorisationV1 {
    /// Builds the record, sorting the rows and refusing what cannot be one.
    ///
    /// # Errors
    ///
    /// Returns [`IrenaError::Invalid`] listing every problem at once: a duplicate
    /// row, no `company` signer.
    pub fn new(mut signers: Vec<SignerV1>) -> Result<Self, IrenaError> {
        let mut issues = Vec::new();
        signers.sort();
        let mut previous: Option<&SignerV1> = None;
        for signer in &signers {
            if previous == Some(signer) {
                issues.push(IssueV1::DuplicateSigner {
                    person: signer.person.clone(),
                    family: signer.family,
                });
            }
            previous = Some(signer);
        }
        if !signers
            .iter()
            .any(|signer| signer.family == RecordFamilyV1::Company)
        {
            issues.push(IssueV1::NoCompanySigner);
        }
        if !issues.is_empty() {
            return Err(IrenaError::invalid(issues));
        }
        signers.dedup();
        Ok(Self { signers })
    }

    /// The rows, sorted.
    #[must_use]
    pub fn signers(&self) -> &[SignerV1] {
        &self.signers
    }

    /// The persons who may sign `family`, in id order.
    pub fn persons_for(&self, family: RecordFamilyV1) -> impl Iterator<Item = &VoterIdV1> {
        self.signers
            .iter()
            .filter(move |signer| signer.family == family)
            .map(|signer| &signer.person)
    }

    /// Whether `person` may sign `family`.
    #[must_use]
    pub fn allows(&self, person: &VoterIdV1, family: RecordFamilyV1) -> bool {
        self.signers
            .binary_search(&SignerV1 {
                person: person.clone(),
                family,
            })
            .is_ok()
    }

    /// The families `person` may sign, in order.
    pub fn families_of(&self, person: &VoterIdV1) -> impl Iterator<Item = RecordFamilyV1> + '_ {
        let person = person.clone();
        self.signers
            .iter()
            .filter(move |signer| signer.person == person)
            .map(|signer| signer.family)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(person: &str, family: RecordFamilyV1) -> SignerV1 {
        SignerV1 {
            person: VoterIdV1::new(person).expect("id"),
            family,
        }
    }

    #[test]
    fn rows_are_sorted_and_answer_who_may_sign_what() {
        let authorisation = AuthorisationV1::new(vec![
            row("jane", RecordFamilyV1::Governance),
            row("bob", RecordFamilyV1::Governance),
            row("jane", RecordFamilyV1::Company),
        ])
        .expect("valid");
        let jane = VoterIdV1::new("jane").unwrap();
        let bob = VoterIdV1::new("bob").unwrap();
        assert!(authorisation.allows(&jane, RecordFamilyV1::Company));
        assert!(authorisation.allows(&bob, RecordFamilyV1::Governance));
        assert!(!authorisation.allows(&bob, RecordFamilyV1::Company));
        let company: Vec<&str> = authorisation
            .persons_for(RecordFamilyV1::Company)
            .map(VoterIdV1::as_str)
            .collect();
        assert_eq!(company, ["jane"]);
        let families: Vec<RecordFamilyV1> = authorisation.families_of(&jane).collect();
        assert_eq!(
            families,
            [RecordFamilyV1::Company, RecordFamilyV1::Governance]
        );
        assert_eq!(
            RecordFamilyV1::parse("governance"),
            Some(RecordFamilyV1::Governance)
        );
        assert_eq!(RecordFamilyV1::parse("board"), None);
    }

    #[test]
    fn a_duplicate_row_and_a_missing_company_signer_are_refused() {
        let error = AuthorisationV1::new(vec![
            row("bob", RecordFamilyV1::Governance),
            row("bob", RecordFamilyV1::Governance),
        ])
        .expect_err("problems");
        let issues = error.issues();
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, IssueV1::DuplicateSigner { .. }))
        );
        assert!(issues.iter().any(|i| matches!(i, IssueV1::NoCompanySigner)));
        assert_eq!(issues.len(), 2, "{error}");
    }
}
