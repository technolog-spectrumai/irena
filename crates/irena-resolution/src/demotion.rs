//! The self-demotion rule: an individual channel may amend the channel set only if
//! its own actor's reach does not grow.
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
//! **What it does not do**, stated plainly: it bounds the signer's *own* reach. It does
//! not stop an individual channel from rewriting a channel its actor is not part of.
//! That is a configuration hazard the notarisation on the channel-set record attests
//! to, and `irena channels` marks every individual channel so a reader knows to look.

use crate::error::ResolutionError;
use bornite_core::VoterIdV1;
use irena_core::{ChannelIdV1, DecisionChannelsV1, ShareStructureV1};
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
/// `register`.
fn seats(
    set: &DecisionChannelsV1,
    signer: &VoterIdV1,
    register: &ShareStructureV1,
) -> Result<BTreeSet<ChannelIdV1>, ResolutionError> {
    let mut seats = BTreeSet::new();
    for channel in set.channels() {
        if actors_of(&channel.actors, register)?.get(signer).is_some() {
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
) -> Result<SelfDemotionV1, ResolutionError> {
    let before = seats(old, signer, register)?;
    let after = seats(new, signer, register)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use irena_core::{
        ActorSourceV1, ChannelModeV1, DecisionChannelV1, HolderV1, MemberV1, RosterV1,
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
            key: Some(PublicKey::from_bytes([text.len() as u8; 32])),
            name: None,
            weight,
        }
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
            key: None,
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
        self_demotion(&voter("chen"), &old(), &new, &register()).unwrap()
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
        )
        .unwrap();
        assert!(verdict.holds, "{}", verdict.detail);
    }
}
