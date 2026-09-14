//! Resolving a channel against the company into the people who may decide.
//!
//! This is the company/mathematics boundary in one `match`. A channel names an actor
//! source; resolution turns that source, as it stands at one height, into ids and
//! integer weights. Bornite receives an electorate and never learns whether a weight
//! came from a shareholding or a seat. With **flat shares** the register gives
//! `weight = shares`; a roster gives the weight each member was listed with. Keys come
//! from neither: an actor's key is the one their identity holds in the identities in
//! force at the same height, the one key table.

use crate::error::DecisionError;
use bornite_core::{ElectorateV1, VoterIdV1, VoterV1, WeightTotalV1, WeightV1};
use bornite_rules::VotingRulesV1;
use irena_core::{
    ActorSourceV1, ChannelIdV1, ChannelModeV1, DecisionChannelV1, IdentitiesV1, RosterV1,
    ShareStructureV1,
};
use irena_ledger::CompanyStateV1;
use prunella_core::{BlockHeight, PublicKey, TxId};

/// One person a channel resolved to.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ActorV1 {
    /// The actor, who is the voter.
    pub id: VoterIdV1,
    /// Their weight in this channel.
    pub weight: WeightV1,
    /// The key their identity currently holds, if any.
    pub key: Option<PublicKey>,
    /// Whether a signature from this actor could ever be accepted.
    pub can_sign: bool,
}

/// The actors of a channel and how they were arrived at.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ActorSetV1 {
    /// What Bornite receives, for a collective channel.
    pub electorate: ElectorateV1,
    /// Every actor, in id order, with their weight and signing ability.
    pub actors: Vec<ActorV1>,
    /// The sum of every weight.
    pub total_weight: WeightTotalV1,
    /// How many actors can sign.
    pub signing_actors: usize,
}

impl ActorSetV1 {
    fn build(actors: Vec<ActorV1>) -> Result<Self, DecisionError> {
        let voters = actors
            .iter()
            .map(|actor| VoterV1 {
                id: actor.id.clone(),
                weight: actor.weight,
                excluded: false,
            })
            .collect();
        let total_weight = WeightTotalV1::sum(actors.iter().map(|a| a.weight))
            .map_err(DecisionError::Derivation)?;
        let signing_actors = actors.iter().filter(|a| a.can_sign).count();
        Ok(Self {
            electorate: ElectorateV1::new(voters).map_err(DecisionError::Derivation)?,
            actors,
            total_weight,
            signing_actors,
        })
    }

    /// Finds an actor by id.
    #[must_use]
    pub fn get(&self, id: &VoterIdV1) -> Option<&ActorV1> {
        self.actors
            .binary_search_by(|actor| actor.id.cmp(id))
            .ok()
            .map(|index| &self.actors[index])
    }

    /// The registered key of an actor, if they are in the set and have one.
    #[must_use]
    pub fn key_of(&self, id: &VoterIdV1) -> Option<&PublicKey> {
        self.get(id).and_then(|actor| actor.key.as_ref())
    }

    /// Number of actors.
    #[must_use]
    pub fn len(&self) -> usize {
        self.actors.len()
    }

    /// Whether nobody resolved.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.actors.is_empty()
    }
}

/// The actors of the share register: every holder, weight equal to their holding.
///
/// Every holder has at least one share by validation, so every holder is an actor; a
/// holder whose identity holds no key — or who has no identity at all — is still one
/// (they own the shares and count towards quorum) but can never sign, and the set says
/// so. Nobody is marked excluded: the register has no notion of exclusion, so a
/// channel's exclusion rule removes no one.
///
/// # Errors
///
/// Returns [`DecisionError::Derivation`] if Bornite refuses the weights, which a
/// validated register cannot cause; the check exists so nothing is ever `expect`ed
/// about money.
pub fn actors_of_register(
    register: &ShareStructureV1,
    identities: &IdentitiesV1,
) -> Result<ActorSetV1, DecisionError> {
    let actors = register
        .holders()
        .iter()
        .map(|holder| actor(&holder.id, holder.shares, identities))
        .collect::<Result<Vec<_>, DecisionError>>()?;
    ActorSetV1::build(actors)
}

/// The actors of a roster: every member, at the weight they were listed with.
///
/// # Errors
///
/// As [`actors_of_register`].
pub fn actors_of_roster(
    roster: &RosterV1,
    identities: &IdentitiesV1,
) -> Result<ActorSetV1, DecisionError> {
    let actors = roster
        .members()
        .iter()
        .map(|member| actor(&member.id, member.weight, identities))
        .collect::<Result<Vec<_>, DecisionError>>()?;
    ActorSetV1::build(actors)
}

fn actor(id: &VoterIdV1, weight: u64, identities: &IdentitiesV1) -> Result<ActorV1, DecisionError> {
    let key = identities.key_of(id);
    Ok(ActorV1 {
        id: id.clone(),
        weight: WeightV1::new(weight).map_err(DecisionError::Derivation)?,
        key,
        can_sign: key.is_some(),
    })
}

/// The actors of a source, as the company stands.
///
/// # Errors
///
/// As [`actors_of_register`].
pub fn actors_of(
    source: &ActorSourceV1,
    register: &ShareStructureV1,
    identities: &IdentitiesV1,
) -> Result<ActorSetV1, DecisionError> {
    match source {
        ActorSourceV1::ShareRegister => actors_of_register(register, identities),
        ActorSourceV1::Roster(roster) => actors_of_roster(roster, identities),
    }
}

/// A channel resolved against the company at one height.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ResolvedChannelV1 {
    /// The channel as configured.
    pub channel: DecisionChannelV1,
    /// The channel-set record it came from.
    pub channels_tx_id: TxId,
    /// The register record its actors came from (the register is consulted whatever
    /// the source, so the pin is always meaningful).
    pub shares_tx_id: TxId,
    /// The identities record its actors' keys came from.
    pub identities_tx_id: TxId,
    /// The height the company was resolved at.
    pub height: BlockHeight,
    /// Who may decide.
    pub actors: ActorSetV1,
}

impl ResolvedChannelV1 {
    /// The channel's id.
    #[must_use]
    pub fn id(&self) -> &ChannelIdV1 {
        &self.channel.id
    }

    /// The channel's mode.
    #[must_use]
    pub fn mode(&self) -> &ChannelModeV1 {
        &self.channel.mode
    }

    /// The one actor, for an individual channel. `None` for a collective one.
    #[must_use]
    pub fn sole_actor(&self) -> Option<&ActorV1> {
        match self.channel.mode {
            ChannelModeV1::Individual => self.actors.actors.first(),
            ChannelModeV1::Collective { .. } => None,
        }
    }

    /// The rules, for a collective channel. `None` for an individual one.
    #[must_use]
    pub fn rules(&self) -> Option<&VotingRulesV1> {
        self.channel.mode.rules()
    }
}

/// Resolves channel `id` against a reconstructed company.
///
/// Finds the channel in the set in force, resolves its actors from its source as the
/// company stands at that height, and — for an individual channel — checks that
/// exactly one actor resolved. That last check can only be made here: a
/// `share-register` source in individual mode is one actor in a single-member company
/// and two the day a second holder is admitted.
///
/// # Errors
///
/// [`DecisionError::NoSuchChannel`], [`DecisionError::NotSingleActor`], or
/// [`DecisionError::Derivation`].
pub fn resolve_channel(
    state: &CompanyStateV1,
    id: &ChannelIdV1,
) -> Result<ResolvedChannelV1, DecisionError> {
    let channel = state
        .channels
        .value
        .get(id)
        .ok_or_else(|| DecisionError::NoSuchChannel {
            channel: id.clone(),
            height: state.at,
        })?;
    let actors = actors_of(
        &channel.actors,
        &state.shares.value,
        &state.identities.value,
    )?;
    if channel.mode.is_individual() && actors.len() != 1 {
        return Err(DecisionError::NotSingleActor {
            channel: id.clone(),
            found: actors.len(),
        });
    }
    Ok(ResolvedChannelV1 {
        channel: channel.clone(),
        channels_tx_id: state.channels.tx_id,
        shares_tx_id: state.shares.tx_id,
        identities_tx_id: state.identities.tx_id,
        height: state.at,
        actors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use irena_core::{HolderV1, MemberV1, PersonV1};

    fn holder(id: &str, shares: u64) -> HolderV1 {
        HolderV1 {
            id: VoterIdV1::new(id).expect("id"),
            name: None,
            shares,
        }
    }

    fn member(id: &str, weight: u64) -> MemberV1 {
        MemberV1 {
            id: VoterIdV1::new(id).expect("id"),
            name: None,
            weight,
        }
    }

    /// The one key table: `(id, key seed)` for everyone who has a key.
    fn identities(keyed: &[(&str, u8)]) -> IdentitiesV1 {
        IdentitiesV1::new(
            keyed
                .iter()
                .map(|(id, seed)| PersonV1 {
                    id: VoterIdV1::new(*id).expect("id"),
                    name: None,
                    document_id: None,
                    key: Some(PublicKey::from_bytes([*seed; 32])),
                })
                .collect(),
        )
        .expect("identities")
    }

    #[test]
    fn register_weight_is_shares_and_keyless_holders_stay_in() {
        let register = ShareStructureV1::new(vec![
            holder("carol", 200),
            holder("alice", 500),
            holder("bob", 300),
        ])
        .expect("register");
        // Carol is registered with no key; a person the identities never list is the
        // same to the channel.
        let set = actors_of_register(&register, &identities(&[("alice", 1), ("bob", 2)]))
            .expect("actors");
        let ids: Vec<&str> = set
            .electorate
            .voters()
            .iter()
            .map(|v| v.id.as_str())
            .collect();
        assert_eq!(ids, ["alice", "bob", "carol"]);
        let weights: Vec<u64> = set
            .electorate
            .voters()
            .iter()
            .map(|v| v.weight.value())
            .collect();
        assert_eq!(weights, [500, 300, 200]);
        assert!(set.electorate.voters().iter().all(|v| !v.excluded));
        assert_eq!(set.total_weight.value(), 1000);
        assert_eq!(set.signing_actors, 2);
        assert!(!set.actors[2].can_sign);
        assert!(set.key_of(&VoterIdV1::new("alice").unwrap()).is_some());
        assert!(set.key_of(&VoterIdV1::new("carol").unwrap()).is_none());
        assert!(set.key_of(&VoterIdV1::new("dave").unwrap()).is_none());
    }

    #[test]
    fn roster_weight_is_the_declared_weight() {
        let channel = ChannelIdV1::new("board").unwrap();
        let roster = RosterV1::new(
            &channel,
            vec![member("vance", 1), member("chen", 2), member("okafor", 1)],
        )
        .expect("roster");
        let set =
            actors_of_roster(&roster, &identities(&[("chen", 4), ("okafor", 5)])).expect("actors");
        let weights: Vec<(&str, u64)> = set
            .actors
            .iter()
            .map(|a| (a.id.as_str(), a.weight.value()))
            .collect();
        assert_eq!(weights, [("chen", 2), ("okafor", 1), ("vance", 1)]);
        assert_eq!(set.total_weight.value(), 4);
        assert_eq!(set.signing_actors, 2);
    }

    #[test]
    fn resolution_does_not_depend_on_listing_order() {
        let keys = identities(&[("a", 1)]);
        let a = actors_of_register(
            &ShareStructureV1::new(vec![holder("b", 1), holder("a", 2)]).unwrap(),
            &keys,
        )
        .unwrap();
        let b = actors_of_register(
            &ShareStructureV1::new(vec![holder("a", 2), holder("b", 1)]).unwrap(),
            &keys,
        )
        .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn an_empty_register_resolves_to_nobody() {
        let set = actors_of_register(
            &ShareStructureV1::new(Vec::new()).unwrap(),
            &identities(&[("a", 1)]),
        )
        .unwrap();
        assert!(set.is_empty());
        assert_eq!(set.total_weight.value(), 0);
    }
}
