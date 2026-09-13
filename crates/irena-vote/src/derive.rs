//! Turning a share register into a Bornite electorate.
//!
//! This is the company/mathematics boundary in one function. Bornite receives voter
//! ids and integer weights and never learns that a share exists; Irena decides here
//! what a share is worth. With **flat shares** the answer is one vote each, so
//! `weight = shares`. Share classes, when they arrive, change this line and nothing
//! else.

use crate::error::VoteError;
use bornite_core::{ElectorateV1, VoterV1, WeightTotalV1, WeightV1};
use irena_core::ShareStructureV1;
use prunella_core::PublicKey;

/// One holder as the derivation saw them.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DerivedHolderV1 {
    /// The holder, who is the voter.
    pub id: bornite_core::VoterIdV1,
    /// Shares held.
    pub shares: u64,
    /// Voting weight derived from them.
    pub weight: WeightV1,
    /// The key the holder votes with, if registered.
    pub key: Option<PublicKey>,
    /// Whether a ballot from this holder could ever be accepted.
    pub can_sign: bool,
}

/// The electorate and how it was arrived at.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ElectorateDerivationV1 {
    /// What Bornite receives.
    pub electorate: ElectorateV1,
    /// Every holder, in id order, with their weight and signing ability.
    pub holders: Vec<DerivedHolderV1>,
    /// The sum of every weight.
    pub total_weight: WeightTotalV1,
    /// How many holders can sign a ballot.
    pub signing_holders: usize,
}

impl ElectorateDerivationV1 {
    /// The registered key of a holder, if they are in the electorate and have one.
    #[must_use]
    pub fn key_of(&self, id: &bornite_core::VoterIdV1) -> Option<&PublicKey> {
        self.holders
            .binary_search_by(|holder| holder.id.cmp(id))
            .ok()
            .and_then(|index| self.holders[index].key.as_ref())
    }
}

/// Derives the electorate for a share register.
///
/// Flat shares: every holder's weight is their share count. Every holder has at least
/// one share by validation, so every holder enters the electorate; a holder without a
/// signing key is still a voter (they own the shares and count towards quorum) but can
/// never cast a valid ballot, and the derivation says so. Nobody is marked excluded:
/// the register has no notion of exclusion, and the rules' exclusion setting therefore
/// removes no one.
///
/// # Errors
///
/// Returns [`VoteError::Derivation`] if Bornite refuses the weights, which a validated
/// register cannot cause; the check exists so nothing is ever `expect`ed about money.
pub fn derive_electorate(register: &ShareStructureV1) -> Result<ElectorateDerivationV1, VoteError> {
    let mut holders = Vec::with_capacity(register.len());
    let mut voters = Vec::with_capacity(register.len());
    for holder in register.holders() {
        let weight = WeightV1::new(holder.shares).map_err(VoteError::Derivation)?;
        holders.push(DerivedHolderV1 {
            id: holder.id.clone(),
            shares: holder.shares,
            weight,
            key: holder.key,
            can_sign: holder.key.is_some(),
        });
        voters.push(VoterV1 {
            id: holder.id.clone(),
            weight,
            excluded: false,
        });
    }
    let total_weight =
        WeightTotalV1::sum(holders.iter().map(|h| h.weight)).map_err(VoteError::Derivation)?;
    let signing_holders = holders.iter().filter(|h| h.can_sign).count();
    Ok(ElectorateDerivationV1 {
        electorate: ElectorateV1::new(voters).map_err(VoteError::Derivation)?,
        holders,
        total_weight,
        signing_holders,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bornite_core::VoterIdV1;
    use irena_core::HolderV1;

    fn holder(id: &str, shares: u64, key: Option<u8>) -> HolderV1 {
        HolderV1 {
            id: VoterIdV1::new(id).expect("id"),
            key: key.map(|b| PublicKey::from_bytes([b; 32])),
            name: None,
            shares,
        }
    }

    #[test]
    fn weight_is_shares_and_keyless_holders_stay_in() {
        let register = ShareStructureV1::new(vec![
            holder("carol", 200, None),
            holder("alice", 500, Some(1)),
            holder("bob", 300, Some(2)),
        ])
        .expect("register");
        let derived = derive_electorate(&register).expect("derive");
        let ids: Vec<&str> = derived
            .electorate
            .voters()
            .iter()
            .map(|v| v.id.as_str())
            .collect();
        assert_eq!(ids, ["alice", "bob", "carol"]);
        let weights: Vec<u64> = derived
            .electorate
            .voters()
            .iter()
            .map(|v| v.weight.value())
            .collect();
        assert_eq!(weights, [500, 300, 200]);
        assert!(derived.electorate.voters().iter().all(|v| !v.excluded));
        assert_eq!(derived.total_weight.value(), 1000);
        assert_eq!(derived.signing_holders, 2);
        assert!(!derived.holders[2].can_sign);
        assert!(derived.key_of(&VoterIdV1::new("alice").unwrap()).is_some());
        assert!(derived.key_of(&VoterIdV1::new("carol").unwrap()).is_none());
        assert!(derived.key_of(&VoterIdV1::new("dave").unwrap()).is_none());
    }

    #[test]
    fn derivation_does_not_depend_on_register_order() {
        let a = derive_electorate(
            &ShareStructureV1::new(vec![holder("b", 1, None), holder("a", 2, Some(1))]).unwrap(),
        )
        .unwrap();
        let b = derive_electorate(
            &ShareStructureV1::new(vec![holder("a", 2, Some(1)), holder("b", 1, None)]).unwrap(),
        )
        .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn an_empty_register_derives_an_empty_electorate() {
        let derived = derive_electorate(&ShareStructureV1::new(Vec::new()).unwrap()).unwrap();
        assert!(derived.electorate.is_empty());
        assert_eq!(derived.total_weight.value(), 0);
    }
}
