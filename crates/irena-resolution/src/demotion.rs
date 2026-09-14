//! The self-demotion rules: an individual channel may amend who decides, who holds a
//! key and who may publish only if its own actor's reach does not grow.
//!
//! Irena enforces mechanism, not legitimacy, so a channel-set amendment carried by a
//! meeting is unrestricted. But one person rewriting who decides is the one place a
//! mechanism can quietly become a takeover, so an amendment executed on an
//! **individual** decision is held to two rules, with `signer` the channel's sole
//! actor, `old` the channel set the amendment supersedes and `new` its body:
//!
//! * **R1** — the set of channel ids the signer is an actor of must not grow.
//! * **R2** — every channel the signer remains an actor of must be identical in `old`
//!   and `new`: same mode, same actors, same rules.
//!
//! So a sole director may abolish their own channel, hand the company to a collective
//! they are not sole in, or leave themselves alone — and may not add themselves
//! anywhere, widen their own channel, thin out a collective they sit on, or change its
//! rules. Two set comparisons; no scoring, no ordering, no expressions.
//!
//! Two parts beside the channel set can hand one person the same power by another
//! route, so each has its own rule, applied the same way and reported the same way:
//!
//! * **Identities** ([`identity_demotion`]) — a person's key *is* their voice in every
//!   channel they sit on, so on an individual decision only the signer's own entry may
//!   change. Persons may be added; nobody else's entry may be changed or removed. A
//!   sole director rotates their own key alone and never anyone else's.
//! * **Authorisation** ([`authorisation_demotion`]) — publishing bare records is real
//!   power, so the signer's own rows in the new record must be a subset of their rows
//!   in the old one: you may drop your own publishing right, never grant yourself one.
//!
//! **What they do not do**, stated plainly: they bound the signer's *own* reach. They
//! do not stop an individual channel from rewriting a channel its actor is not part
//! of, registering a new person, or authorising somebody else. That is a configuration
//! hazard the notarisation on the record attests to, and `irena channels` marks every
//! individual channel so a reader knows to look.

use crate::error::ResolutionError;
use bornite_core::VoterIdV1;
use irena_core::{
    AuthorisationV1, ChannelIdV1, DecisionChannelsV1, IdentitiesV1, ShareStructureV1,
};
use irena_decision::actors_of;
use std::collections::BTreeSet;

/// How an amendment fared under the rule.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SelfDemotionV1 {
    /// Whether both rules held.
    pub holds: bool,
    /// What was found, for a reader.
    pub detail: String,
}

/// The ids of every channel `signer` is an actor of, with sources resolved against
/// `register` and `identities`.
fn seats(
    set: &DecisionChannelsV1,
    signer: &VoterIdV1,
    register: &ShareStructureV1,
    identities: &IdentitiesV1,
) -> Result<BTreeSet<ChannelIdV1>, ResolutionError> {
    let mut seats = BTreeSet::new();
    for channel in set.channels() {
        if actors_of(&channel.actors, register, identities)?
            .get(signer)
            .is_some()
        {
            seats.insert(channel.id.clone());
        }
    }
    Ok(seats)
}

fn list(ids: &BTreeSet<ChannelIdV1>) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Applies the rule to an amendment replacing `old` with `new`, signed by `signer`.
///
/// `register` is the share register the sources resolve against — the one in force
/// where the amendment lands, which is also the one the signer's channel resolved
/// against.
///
/// # Errors
///
/// [`ResolutionError::Decision`] if a source cannot resolve, which validated documents
/// cannot cause.
pub fn self_demotion(
    signer: &VoterIdV1,
    old: &DecisionChannelsV1,
    new: &DecisionChannelsV1,
    register: &ShareStructureV1,
    identities: &IdentitiesV1,
) -> Result<SelfDemotionV1, ResolutionError> {
    let before = seats(old, signer, register, identities)?;
    let after = seats(new, signer, register, identities)?;
    let gained: BTreeSet<ChannelIdV1> = after.difference(&before).cloned().collect();
    let changed: BTreeSet<ChannelIdV1> = after
        .intersection(&before)
        .filter(|id| old.get(id) != new.get(id))
        .cloned()
        .collect();
    let given_up: BTreeSet<ChannelIdV1> = before.difference(&after).cloned().collect();

    let holds = gained.is_empty() && changed.is_empty();
    let mut parts = Vec::new();
    if !gained.is_empty() {
        parts.push(format!("{signer} would gain a seat on {}", list(&gained)));
    }
    if !changed.is_empty() {
        parts.push(format!(
            "{} — a channel {signer} sits on — would change",
            list(&changed)
        ));
    }
    if holds {
        let kept: BTreeSet<ChannelIdV1> = after.clone();
        parts.push(format!(
            "{signer} keeps {} seat(s) unchanged{} and gains none",
            kept.len(),
            if kept.is_empty() {
                String::new()
            } else {
                format!(" ({})", list(&kept))
            }
        ));
        if !given_up.is_empty() {
            parts.push(format!("gives up {}", list(&given_up)));
        }
    }
    Ok(SelfDemotionV1 {
        holds,
        detail: parts.join("; "),
    })
}

/// Applies the identities rule to an amendment replacing `old` with `new`, signed by
/// `signer`: only the signer's own entry may change, and persons may be added.
#[must_use]
pub fn identity_demotion(
    signer: &VoterIdV1,
    old: &IdentitiesV1,
    new: &IdentitiesV1,
) -> SelfDemotionV1 {
    let mut changed = Vec::new();
    let mut removed = Vec::new();
    for person in old.persons() {
        if &person.id == signer {
            continue;
        }
        match new.get(&person.id) {
            None => removed.push(person.id.to_string()),
            Some(found) if found != person => changed.push(person.id.to_string()),
            Some(_) => {}
        }
    }
    let added: Vec<String> = new
        .persons()
        .iter()
        .filter(|person| old.get(&person.id).is_none())
        .map(|person| person.id.to_string())
        .collect();
    let holds = changed.is_empty() && removed.is_empty();
    let mut parts = Vec::new();
    if !changed.is_empty() {
        parts.push(format!("{signer} would change {}", changed.join(", ")));
    }
    if !removed.is_empty() {
        parts.push(format!("{signer} would remove {}", removed.join(", ")));
    }
    if holds {
        let own = match (old.get(signer), new.get(signer)) {
            (Some(before), Some(after)) if before == after => "own entry unchanged".to_owned(),
            (Some(_), Some(_)) => format!("{signer} changed their own entry"),
            (Some(_), None) => format!("{signer} removed their own entry"),
            (None, _) => format!("{signer} is not listed"),
        };
        parts.push(format!("{own}; nobody else's entry changed"));
        if !added.is_empty() {
            parts.push(format!("registered {}", added.join(", ")));
        }
    }
    SelfDemotionV1 {
        holds,
        detail: parts.join("; "),
    }
}

/// Applies the authorisation rule to an amendment replacing `old` with `new`, signed
/// by `signer`: the signer's own rows may only shrink.
#[must_use]
pub fn authorisation_demotion(
    signer: &VoterIdV1,
    old: &AuthorisationV1,
    new: &AuthorisationV1,
) -> SelfDemotionV1 {
    let before: BTreeSet<_> = old.families_of(signer).collect();
    let after: BTreeSet<_> = new.families_of(signer).collect();
    let gained: Vec<String> = after.difference(&before).map(ToString::to_string).collect();
    let given_up: Vec<String> = before.difference(&after).map(ToString::to_string).collect();
    let holds = gained.is_empty();
    let mut parts = Vec::new();
    if holds {
        parts.push(format!(
            "{signer} may sign {} and gains nothing",
            if after.is_empty() {
                "nothing".to_owned()
            } else {
                after
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ));
        if !given_up.is_empty() {
            parts.push(format!("gives up {}", given_up.join(", ")));
        }
    } else {
        parts.push(format!(
            "{signer} would gain the right to sign {}",
            gained.join(", ")
        ));
    }
    SelfDemotionV1 {
        holds,
        detail: parts.join("; "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use irena_core::{
        ActorSourceV1, ChannelModeV1, DecisionChannelV1, HolderV1, MemberV1, PersonV1, RosterV1,
        SignerV1,
    };
    use prunella_core::PublicKey;

    fn id(text: &str) -> ChannelIdV1 {
        ChannelIdV1::new(text).unwrap()
    }

    fn voter(text: &str) -> VoterIdV1 {
        VoterIdV1::new(text).unwrap()
    }

    fn member(text: &str, weight: u64) -> MemberV1 {
        MemberV1 {
            id: voter(text),
            name: None,
            weight,
        }
    }

    fn person(id: &str, key: Option<u8>) -> PersonV1 {
        PersonV1 {
            id: voter(id),
            name: None,
            document_id: None,
            key: key.map(|seed| PublicKey::from_bytes([seed; 32])),
        }
    }

    fn identities(people: Vec<PersonV1>) -> IdentitiesV1 {
        IdentitiesV1::new(people).unwrap()
    }

    fn keys() -> IdentitiesV1 {
        identities(vec![
            person("chen", Some(1)),
            person("okafor", Some(2)),
            person("vance", None),
        ])
    }

    fn authorisation(rows: &[(&str, irena_core::RecordFamilyV1)]) -> AuthorisationV1 {
        AuthorisationV1::new(
            rows.iter()
                .map(|(person, family)| SignerV1 {
                    person: voter(person),
                    family: *family,
                })
                .collect(),
        )
        .unwrap()
    }

    fn rules(tie_accept: bool) -> irena_core::VotingRulesV1 {
        bornite_xml::read_rules_document(&format!(
            r#"<voting-rules version="1.0"><weight type="electorate"/><exclusions enabled="false"/><quorum type="none"/><threshold type="simple-majority" basis="votes-cast"/><abstentions treatment="exclude"/><tie treatment="{}"/></voting-rules>"#,
            if tie_accept { "accept" } else { "reject" }
        ))
        .unwrap()
    }

    fn roster(channel: &str, members: &[(&str, u64)]) -> ActorSourceV1 {
        ActorSourceV1::Roster(
            RosterV1::new(
                &id(channel),
                members.iter().map(|(m, w)| member(m, *w)).collect(),
            )
            .unwrap(),
        )
    }

    fn collective(channel: &str, actors: ActorSourceV1, tie_accept: bool) -> DecisionChannelV1 {
        DecisionChannelV1 {
            id: id(channel),
            actors,
            mode: ChannelModeV1::Collective {
                rules: rules(tie_accept),
            },
        }
    }

    fn individual(channel: &str, actor: &str) -> DecisionChannelV1 {
        DecisionChannelV1 {
            id: id(channel),
            actors: roster(channel, &[(actor, 1)]),
            mode: ChannelModeV1::Individual,
        }
    }

    fn set(channels: Vec<DecisionChannelV1>) -> DecisionChannelsV1 {
        DecisionChannelsV1::new(channels).unwrap()
    }

    fn register() -> ShareStructureV1 {
        ShareStructureV1::new(vec![HolderV1 {
            id: voter("chen"),
            name: None,
            shares: 10,
        }])
        .unwrap()
    }

    /// shareholders (register: chen), board (chen 2, okafor 1, vance 1), ceo (chen).
    fn old() -> DecisionChannelsV1 {
        set(vec![
            collective("shareholders", ActorSourceV1::ShareRegister, false),
            collective(
                "board",
                roster("board", &[("chen", 2), ("okafor", 1), ("vance", 1)]),
                false,
            ),
            individual("ceo", "chen"),
        ])
    }

    fn check(new: DecisionChannelsV1) -> SelfDemotionV1 {
        self_demotion(&voter("chen"), &old(), &new, &register(), &keys()).unwrap()
    }

    #[test]
    fn abolishing_oneself_or_changing_nothing_holds() {
        let unchanged = check(old());
        assert!(unchanged.holds, "{}", unchanged.detail);
        let abolished = check(set(vec![
            collective("shareholders", ActorSourceV1::ShareRegister, false),
            collective(
                "board",
                roster("board", &[("chen", 2), ("okafor", 1), ("vance", 1)]),
                false,
            ),
        ]));
        assert!(abolished.holds, "{}", abolished.detail);
        assert!(
            abolished.detail.contains("gives up ceo"),
            "{}",
            abolished.detail
        );
    }

    #[test]
    fn becoming_one_of_several_holds() {
        // The ceo channel becomes a collective of two: chen still sits on it, but the
        // channel changed, so R2 refuses — unless the old seat is given up and a new
        // channel is created for the pair instead.
        let widened = check(set(vec![
            collective("shareholders", ActorSourceV1::ShareRegister, false),
            collective(
                "board",
                roster("board", &[("chen", 2), ("okafor", 1), ("vance", 1)]),
                false,
            ),
            collective("ceo", roster("ceo", &[("chen", 1), ("okafor", 1)]), false),
        ]));
        assert!(!widened.holds, "{}", widened.detail);
        assert!(widened.detail.contains("ceo"), "{}", widened.detail);
    }

    #[test]
    fn gaining_a_seat_is_refused() {
        let verdict = check(set(vec![
            collective("shareholders", ActorSourceV1::ShareRegister, false),
            collective(
                "board",
                roster("board", &[("chen", 2), ("okafor", 1), ("vance", 1)]),
                false,
            ),
            individual("ceo", "chen"),
            individual("treasury", "chen"),
        ]));
        assert!(!verdict.holds);
        assert!(
            verdict.detail.contains("gain a seat on treasury"),
            "{}",
            verdict.detail
        );
    }

    #[test]
    fn changing_a_channel_one_sits_on_is_refused() {
        // Thinning the board.
        let thinned = check(set(vec![
            collective("shareholders", ActorSourceV1::ShareRegister, false),
            collective("board", roster("board", &[("chen", 2)]), false),
            individual("ceo", "chen"),
        ]));
        assert!(!thinned.holds);
        assert!(thinned.detail.contains("board"), "{}", thinned.detail);
        // Changing the board's rules, members untouched.
        let rewritten = check(set(vec![
            collective("shareholders", ActorSourceV1::ShareRegister, false),
            collective(
                "board",
                roster("board", &[("chen", 2), ("okafor", 1), ("vance", 1)]),
                true,
            ),
            individual("ceo", "chen"),
        ]));
        assert!(!rewritten.holds);
        assert!(rewritten.detail.contains("board"), "{}", rewritten.detail);
        // The shareholders' rules, which chen the holder sits on through the register.
        let shareholders = check(set(vec![
            collective("shareholders", ActorSourceV1::ShareRegister, true),
            collective(
                "board",
                roster("board", &[("chen", 2), ("okafor", 1), ("vance", 1)]),
                false,
            ),
            individual("ceo", "chen"),
        ]));
        assert!(!shareholders.holds);
        assert!(
            shareholders.detail.contains("shareholders"),
            "{}",
            shareholders.detail
        );
    }

    #[test]
    fn a_channel_the_signer_is_not_on_may_change() {
        // The documented gap: the audit committee is rewritten and the rule is silent.
        let with_committee = set(vec![
            collective("shareholders", ActorSourceV1::ShareRegister, false),
            collective(
                "board",
                roster("board", &[("chen", 2), ("okafor", 1), ("vance", 1)]),
                false,
            ),
            individual("ceo", "chen"),
            collective("audit", roster("audit", &[("okafor", 1)]), false),
        ]);
        let verdict = self_demotion(
            &voter("chen"),
            &with_committee,
            &{
                set(vec![
                    collective("shareholders", ActorSourceV1::ShareRegister, false),
                    collective(
                        "board",
                        roster("board", &[("chen", 2), ("okafor", 1), ("vance", 1)]),
                        false,
                    ),
                    individual("ceo", "chen"),
                    collective("audit", roster("audit", &[("vance", 1)]), false),
                ])
            },
            &register(),
            &keys(),
        )
        .unwrap();
        assert!(verdict.holds, "{}", verdict.detail);
    }

    #[test]
    fn only_the_signers_own_identity_entry_may_change() {
        let chen = voter("chen");
        let old = keys();
        // Rotating one's own key, alone.
        let rotated = identities(vec![
            person("chen", Some(9)),
            person("okafor", Some(2)),
            person("vance", None),
        ]);
        let verdict = identity_demotion(&chen, &old, &rotated);
        assert!(verdict.holds, "{}", verdict.detail);
        assert!(
            verdict.detail.contains("changed their own entry"),
            "{}",
            verdict.detail
        );
        // Registering somebody new alongside.
        let added = identities(vec![
            person("chen", Some(1)),
            person("okafor", Some(2)),
            person("quinn", Some(8)),
            person("vance", None),
        ]);
        let verdict = identity_demotion(&chen, &old, &added);
        assert!(verdict.holds, "{}", verdict.detail);
        assert!(
            verdict.detail.contains("registered quinn"),
            "{}",
            verdict.detail
        );
        // Taking over somebody else's key: the takeover the rule exists for.
        let stolen = identities(vec![
            person("chen", Some(1)),
            person("okafor", Some(7)),
            person("vance", None),
        ]);
        let verdict = identity_demotion(&chen, &old, &stolen);
        assert!(!verdict.holds);
        assert!(
            verdict.detail.contains("change okafor"),
            "{}",
            verdict.detail
        );
        // Removing somebody, which silences them everywhere.
        let dropped = identities(vec![person("chen", Some(1)), person("okafor", Some(2))]);
        let verdict = identity_demotion(&chen, &old, &dropped);
        assert!(!verdict.holds);
        assert!(
            verdict.detail.contains("remove vance"),
            "{}",
            verdict.detail
        );
        // Standing down oneself.
        let gone = identities(vec![person("okafor", Some(2)), person("vance", None)]);
        let verdict = identity_demotion(&chen, &old, &gone);
        assert!(verdict.holds, "{}", verdict.detail);
    }

    #[test]
    fn a_signer_may_drop_their_own_publishing_right_and_never_grant_one() {
        use irena_core::RecordFamilyV1::{Company, Governance};
        let chen = voter("chen");
        let old = authorisation(&[("jane", Company), ("chen", Governance)]);
        // Giving up one's own row.
        let dropped = authorisation(&[("jane", Company)]);
        let verdict = authorisation_demotion(&chen, &old, &dropped);
        assert!(verdict.holds, "{}", verdict.detail);
        assert!(
            verdict.detail.contains("gives up governance"),
            "{}",
            verdict.detail
        );
        // Granting oneself the company family.
        let grabbed = authorisation(&[("jane", Company), ("chen", Governance), ("chen", Company)]);
        let verdict = authorisation_demotion(&chen, &old, &grabbed);
        assert!(!verdict.holds);
        assert!(
            verdict.detail.contains("gain the right to sign company"),
            "{}",
            verdict.detail
        );
        // Somebody else's rows are not this rule's business.
        let others = authorisation(&[("jane", Company), ("chen", Governance), ("okafor", Company)]);
        assert!(authorisation_demotion(&chen, &old, &others).holds);
    }
}
