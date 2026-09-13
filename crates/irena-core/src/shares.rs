//! The share register: who holds how many shares, and the key each votes with.
//!
//! **Flat shares.** Every share is one vote, so a holder is a holding and one element
//! carries both. There are no share classes; when they arrive they are a new version of
//! this body, not an attribute added to it, so a flat register stays readable forever
//! and the derivation `weight = shares` changes in exactly one place.
//!
//! The register carries the holders' **signing keys**. That is what lets a later vote
//! check that a ballot came from a registered holder without any key table of its own:
//! keys are company data, on the chain, amended through the same chain as everything
//! else. A holder without a key is a shareholder who cannot cast a ballot but still
//! owns their shares — they are in the electorate and count towards quorum.

use crate::error::{IrenaError, IssueV1};
use bornite_core::VoterIdV1;
use prunella_core::PublicKey;
use std::collections::BTreeMap;

/// Largest number of holders a share structure may list.
///
/// A document is untrusted input; this bounds what a reader allocates for it.
pub const MAX_HOLDERS: usize = 1_000_000;

/// One shareholder.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HolderV1 {
    /// The holder's id, which is also their voter id: a holder becomes a Bornite voter
    /// with no translation.
    pub id: VoterIdV1,
    /// The key the holder signs ballots with, if they have registered one.
    pub key: Option<PublicKey>,
    /// A display name. Opaque.
    pub name: Option<String>,
    /// How many shares. At least one.
    pub shares: u64,
}

/// A validated share register.
///
/// Sorted by holder id and free of duplicates from the moment it is built, so nothing
/// downstream can observe an order that depends on how the document listed them.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct ShareStructureV1 {
    holders: Vec<HolderV1>,
}

impl ShareStructureV1 {
    /// Builds a register, sorting by id and refusing what cannot be a register.
    ///
    /// # Errors
    ///
    /// Returns [`IrenaError::Invalid`] listing every problem at once: duplicate ids,
    /// duplicate keys, zero holdings, a total past `u64::MAX`, too many holders.
    pub fn new(mut holders: Vec<HolderV1>) -> Result<Self, IrenaError> {
        let mut issues = Vec::new();
        if holders.len() > MAX_HOLDERS {
            return Err(IrenaError::invalid(vec![IssueV1::TooManyHolders {
                limit: MAX_HOLDERS,
            }]));
        }
        holders.sort_by(|left, right| left.id.cmp(&right.id));

        let mut previous: Option<&VoterIdV1> = None;
        let mut keys: BTreeMap<&PublicKey, &VoterIdV1> = BTreeMap::new();
        let mut total: u64 = 0;
        let mut overflowed = false;
        for holder in &holders {
            if previous == Some(&holder.id) {
                issues.push(IssueV1::DuplicateHolder {
                    id: holder.id.clone(),
                });
            }
            previous = Some(&holder.id);
            if holder.shares == 0 {
                issues.push(IssueV1::ZeroShares {
                    id: holder.id.clone(),
                });
            }
            if let Some(key) = &holder.key
                && let Some(first) = keys.insert(key, &holder.id)
                && first != &holder.id
            {
                issues.push(IssueV1::DuplicateKey {
                    first: first.clone(),
                    second: holder.id.clone(),
                });
            }
            match total.checked_add(holder.shares) {
                Some(sum) => total = sum,
                None => overflowed = true,
            }
        }
        if overflowed {
            issues.push(IssueV1::TotalSharesOverflow { max: u64::MAX });
        }
        if !issues.is_empty() {
            return Err(IrenaError::invalid(issues));
        }
        Ok(Self { holders })
    }

    /// The holders, in id order.
    #[must_use]
    pub fn holders(&self) -> &[HolderV1] {
        &self.holders
    }

    /// Finds a holder by id.
    #[must_use]
    pub fn get(&self, id: &VoterIdV1) -> Option<&HolderV1> {
        self.holders
            .binary_search_by(|holder| holder.id.cmp(id))
            .ok()
            .map(|index| &self.holders[index])
    }

    /// Number of holders.
    #[must_use]
    pub fn len(&self) -> usize {
        self.holders.len()
    }

    /// Whether the register is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.holders.is_empty()
    }

    /// Total shares in issue. Cannot overflow: `new` refused that.
    #[must_use]
    pub fn total_shares(&self) -> u64 {
        self.holders.iter().map(|holder| holder.shares).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holder(id: &str, shares: u64, key: Option<u8>) -> HolderV1 {
        HolderV1 {
            id: VoterIdV1::new(id).expect("id"),
            key: key.map(|byte| PublicKey::from_bytes([byte; 32])),
            name: None,
            shares,
        }
    }

    #[test]
    fn a_register_is_sorted_and_totalled() {
        let register = ShareStructureV1::new(vec![
            holder("carol", 200, None),
            holder("alice", 500, Some(1)),
            holder("bob", 300, Some(2)),
        ])
        .expect("valid");
        let ids: Vec<&str> = register.holders().iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["alice", "bob", "carol"]);
        assert_eq!(register.total_shares(), 1000);
        assert_eq!(
            register
                .get(&VoterIdV1::new("bob").unwrap())
                .unwrap()
                .shares,
            300
        );
        assert!(register.get(&VoterIdV1::new("dave").unwrap()).is_none());
    }

    #[test]
    fn every_problem_is_reported_together() {
        let error = ShareStructureV1::new(vec![
            holder("alice", 0, Some(1)),
            holder("alice", 5, None),
            holder("bob", u64::MAX, Some(1)),
            holder("carol", 1, None),
        ])
        .expect_err("many problems");
        let issues = error.issues();
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, IssueV1::DuplicateHolder { .. }))
        );
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, IssueV1::ZeroShares { .. }))
        );
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, IssueV1::DuplicateKey { .. }))
        );
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, IssueV1::TotalSharesOverflow { .. }))
        );
        assert_eq!(issues.len(), 4, "{error}");
    }

    #[test]
    fn issue_order_does_not_depend_on_input_order() {
        let a = ShareStructureV1::new(vec![holder("b", 0, None), holder("a", 0, None)])
            .expect_err("zero");
        let b = ShareStructureV1::new(vec![holder("a", 0, None), holder("b", 0, None)])
            .expect_err("zero");
        assert_eq!(a, b);
    }

    #[test]
    fn an_empty_register_is_allowed_by_the_type() {
        // Whether an empty register makes sense is the ledger's call, not the type's.
        assert!(ShareStructureV1::new(Vec::new()).expect("ok").is_empty());
    }
}
