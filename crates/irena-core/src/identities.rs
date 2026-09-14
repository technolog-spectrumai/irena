//! Identities: the one key table.
//!
//! A **person** has a stable id — the actor id grammar, so a person is a holder, a
//! roster member or a signer with no translation — a name, and at most one current
//! Ed25519 key. Holders and members carry an id only; whatever they sign is checked
//! against the key their identity holds in the identities record in force at the
//! frozen height. Rotating a key is an identities amendment: one record, and every
//! channel the person sits on sees the new key from that height on, while anything
//! frozen earlier keeps the key it froze.
//!
//! A person without a key is registered but cannot sign: they count towards quorum
//! where they hold shares or a seat, and no ballot, decision or record transaction can
//! be theirs.

use crate::error::{IrenaError, IssueV1};
use bornite_core::VoterIdV1;
use prunella_core::PublicKey;
use std::collections::BTreeMap;

/// Largest number of persons an identities record may list.
pub const MAX_PERSONS: usize = 1_000_000;

/// One person.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PersonV1 {
    /// The person's id, which is also their id as a holder, member or signer.
    pub id: VoterIdV1,
    /// A display name. Opaque.
    pub name: Option<String>,
    /// An external identity document number — a national id, a passport — in
    /// whatever form the notary uses. Opaque: stored, reproduced, never interpreted.
    pub document_id: Option<String>,
    /// The key the person currently signs with, if they have registered one.
    pub key: Option<PublicKey>,
}

/// A validated identities record.
///
/// Sorted by id and free of duplicates from the moment it is built; no key appears on
/// two persons, so a signature names exactly one of them.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct IdentitiesV1 {
    persons: Vec<PersonV1>,
}

impl IdentitiesV1 {
    /// Builds the record, sorting by id and refusing what cannot be one.
    ///
    /// # Errors
    ///
    /// Returns [`IrenaError::Invalid`] listing every problem at once: duplicate ids,
    /// one key on two persons, too many persons.
    pub fn new(mut persons: Vec<PersonV1>) -> Result<Self, IrenaError> {
        let mut issues = Vec::new();
        if persons.len() > MAX_PERSONS {
            return Err(IrenaError::invalid(vec![IssueV1::TooManyPersons {
                limit: MAX_PERSONS,
            }]));
        }
        persons.sort_by(|left, right| left.id.cmp(&right.id));
        let mut previous: Option<&VoterIdV1> = None;
        let mut keys: BTreeMap<&PublicKey, &VoterIdV1> = BTreeMap::new();
        for person in &persons {
            if previous == Some(&person.id) {
                issues.push(IssueV1::DuplicatePerson {
                    id: person.id.clone(),
                });
            }
            previous = Some(&person.id);
            if let Some(key) = &person.key
                && let Some(first) = keys.insert(key, &person.id)
                && first != &person.id
            {
                issues.push(IssueV1::DuplicateKey {
                    first: first.clone(),
                    second: person.id.clone(),
                });
            }
        }
        if !issues.is_empty() {
            return Err(IrenaError::invalid(issues));
        }
        Ok(Self { persons })
    }

    /// The persons, in id order.
    #[must_use]
    pub fn persons(&self) -> &[PersonV1] {
        &self.persons
    }

    /// Finds a person by id.
    #[must_use]
    pub fn get(&self, id: &VoterIdV1) -> Option<&PersonV1> {
        self.persons
            .binary_search_by(|person| person.id.cmp(id))
            .ok()
            .map(|index| &self.persons[index])
    }

    /// The current key of a person, if they are listed and have one.
    #[must_use]
    pub fn key_of(&self, id: &VoterIdV1) -> Option<PublicKey> {
        self.get(id).and_then(|person| person.key)
    }

    /// The person who currently holds `key`, if any.
    #[must_use]
    pub fn holder_of(&self, key: &PublicKey) -> Option<&PersonV1> {
        self.persons
            .iter()
            .find(|person| person.key.as_ref() == Some(key))
    }

    /// Number of persons.
    #[must_use]
    pub fn len(&self) -> usize {
        self.persons.len()
    }

    /// Whether nobody is listed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.persons.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(id: &str, key: Option<u8>) -> PersonV1 {
        PersonV1 {
            id: VoterIdV1::new(id).expect("id"),
            name: None,
            document_id: None,
            key: key.map(|byte| PublicKey::from_bytes([byte; 32])),
        }
    }

    #[test]
    fn identities_are_sorted_and_looked_up_both_ways() {
        let identities = IdentitiesV1::new(vec![
            person("carol", None),
            person("alice", Some(1)),
            person("bob", Some(2)),
        ])
        .expect("valid");
        let ids: Vec<&str> = identities.persons().iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["alice", "bob", "carol"]);
        let alice = VoterIdV1::new("alice").unwrap();
        assert_eq!(
            identities.key_of(&alice),
            Some(PublicKey::from_bytes([1; 32]))
        );
        assert_eq!(identities.key_of(&VoterIdV1::new("carol").unwrap()), None);
        assert_eq!(identities.key_of(&VoterIdV1::new("dave").unwrap()), None);
        assert_eq!(
            identities
                .holder_of(&PublicKey::from_bytes([2; 32]))
                .map(|p| p.id.as_str()),
            Some("bob")
        );
        assert!(
            identities
                .holder_of(&PublicKey::from_bytes([9; 32]))
                .is_none()
        );
    }

    #[test]
    fn every_problem_is_reported_together() {
        let error = IdentitiesV1::new(vec![
            person("a", Some(1)),
            person("a", None),
            person("b", Some(1)),
        ])
        .expect_err("problems");
        let issues = error.issues();
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, IssueV1::DuplicatePerson { .. }))
        );
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, IssueV1::DuplicateKey { .. }))
        );
        assert_eq!(issues.len(), 2, "{error}");
        assert!(
            IdentitiesV1::new(Vec::new())
                .expect("empty is a document")
                .is_empty()
        );
    }
}
