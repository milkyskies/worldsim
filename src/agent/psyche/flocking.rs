//! Flocking / herd cohesion — proximity-based social drive decay.
//!
//! Reads: VisibleObjects, MindGraph (Affection lookups), SpeciesProfile, EntityType
//! Writes: PsychologicalDrives.social
//! Upstream: perception (VisibleObjects), spawner (initialize_relationship_with_affection)
//! Downstream: nervous_system::urgency (reads social drive), brains::emotional (proposes flocking walks)
//!
//! ## The model
//!
//! Every social species (humans, deer, wolves) has a `social` drive that
//! accumulates over time (loneliness / separation stress). The drive is
//! reduced by *proximity to company*, where "company" is species-agnostic at
//! the drive level but per-individual at the satisfaction level:
//!
//! - Deer herd-mates have high Affection toward each other (set at spawn),
//!   so they satisfy each other's drive strongly. Strangers of the same
//!   species still count a little, because "there are more eyes watching
//!   for wolves" is genuinely comforting to a prey animal.
//! - Wolf pack-mates follow the same pattern.
//! - Humans can satisfy their drive weakly by visible strangers and
//!   strongly by visible friends, though their primary satisfier is
//!   still conversation (handled elsewhere).
//!
//! The visual consequence is a *loose, breathing herd*: when the drive is
//! low, animals wander freely; when it drifts out of range of company, the
//! drive climbs and they're pulled back. Herds form and re-form
//! organically without any "stay together" hard rule.
//!
//! ## Why not a separate "herd" drive?
//!
//! The realistic thing about deer separation stress is that it's the same
//! *shape* as human loneliness — an aversive state that motivates rejoining
//! the group — just with different "who counts as company" rules. Folding
//! it into the existing `social` drive lets one decay mechanism serve
//! every social species; the per-species flavour is carried entirely by
//! the relationship graph (Affection values). No species-specific code,
//! no new drive channel, no new urgency source.

use crate::agent::Agent;
use crate::agent::body::needs::PsychologicalDrives;
use crate::agent::body::species::Species;
use crate::agent::body::species::SpeciesProfile;
use crate::agent::inventory::EntityType;
use crate::agent::mind::knowledge::{MindGraph, Node, Predicate};
use crate::agent::mind::perception::VisibleObjects;
use crate::agent::psyche::personality::Personality;
use crate::core::tick::TickCount;
use bevy::prelude::*;

/// Base affection assumed for an unknown conspecific. Tiny — strangers
/// barely satisfy social drive on their own. The point of the baseline is
/// "more eyes watching for wolves" comfort for animals, and "walking past
/// other humans on the street is mildly soothing" for humans. The real
/// signal is supposed to come from kin (Affection > 0.5) introduced at
/// spawn or known friends accumulated over time.
pub const STRANGER_CONSPECIFIC_AFFECTION: f32 = 0.05;

/// Human floor: even maximally introverted humans get a tiny stranger
/// comfort, because being completely alone is universally aversive (not
/// just for extraverts). Smaller than the deer baseline because deer
/// have herd-safety reasons to value any conspecific; humans don't.
pub const HUMAN_INTROVERT_STRANGER_BASELINE: f32 = 0.02;

/// Extra extravert bonus on top of `HUMAN_INTROVERT_STRANGER_BASELINE`.
/// At extraversion 1.0 a Person treats a stranger as if Affection were
/// `0.02 + 0.06 = 0.08`. Kept intentionally small so the proximity decay
/// doesn't drain the social drive below the conversation-initiation
/// threshold (0.55) before agents can walk into range and actually start
/// talking. The personality signal is real ("extraverts are more
/// comforted by crowds") but shallow — the big social satisfier for
/// humans is conversation, not mere proximity.
pub const EXTRAVERT_STRANGER_BONUS: f32 = 0.06;

/// Rate at which companionship drifts down per second when no conspecifics
/// are visible. Modulated by extraversion (extraverts get lonelier faster).
/// At baseline (extraversion 0.5) this drains ~0.006/sec, so a fully
/// satisfied agent takes ~2-3 minutes of solitude to want company again.
pub const LONELINESS_DECAY_PER_SEC: f32 = 0.006;

/// Affection-weighted decay rate applied to the social drive per second.
/// At `affection_sum = 1.0` (e.g. two herd-mates at 0.5 each, or one at 1.0)
/// the social drive drops by this fraction per second. Tuned so a herd of 3
/// deer at 0.8 affection (sum 1.6) satisfies loneliness in a few seconds.
pub const SOCIAL_PROXIMITY_DECAY_PER_SEC: f32 = 0.15;

/// Compute the effective stranger affection for an agent.
///
/// Animals (deer, wolves, etc.) always use the bare baseline — herd safety
/// in numbers is universal for prey, no per-individual variation. Persons
/// scale stranger comfort by extraversion: an extravert is genuinely
/// comforted by being in a crowd of strangers, an introvert isn't.
pub fn stranger_affection_for(
    species: Option<&SpeciesProfile>,
    personality: Option<&Personality>,
) -> f32 {
    let is_person = matches!(species.map(|s| s.species), Some(Species::Human));
    if is_person {
        // Humans get a small extraversion-modulated stranger comfort.
        // Walking through a crowd is mildly soothing (and more so for
        // extraverts) but conversation is the real satisfier. Even a
        // pure introvert gets a tiny floor — being completely alone is
        // universally aversive, not just an extravert thing.
        let extraversion = personality.map(|p| p.traits.extraversion).unwrap_or(0.5);
        return HUMAN_INTROVERT_STRANGER_BASELINE + EXTRAVERT_STRANGER_BONUS * extraversion;
    }
    // Animals: herd safety in numbers is real regardless of personality.
    // Even a random deer standing next to an unfamiliar deer is slightly
    // less anxious than a deer standing alone.
    STRANGER_CONSPECIFIC_AFFECTION
}

/// Decay the `social` drive based on visible conspecifics, weighted by
/// remembered affection. Runs every 10 ticks because relationship lookups
/// are not free and the drive changes slowly anyway.
pub fn decay_social_from_proximity(
    tick: Res<TickCount>,
    mut agents: Query<
        (
            Entity,
            &VisibleObjects,
            &MindGraph,
            &EntityType,
            Option<&SpeciesProfile>,
            Option<&Personality>,
            &mut PsychologicalDrives,
        ),
        With<Agent>,
    >,
    others: Query<&EntityType>,
) {
    const INTERVAL: u64 = 10;

    let dt = tick.dt() * INTERVAL as f32;

    for (self_entity, visible, mind, self_type, species, personality, mut drives) in
        agents.iter_mut()
    {
        if !tick.should_run(self_entity, INTERVAL) {
            continue;
        }

        let stranger_value = stranger_affection_for(species, personality);

        let mut affection_sum = 0.0_f32;
        for &visible_entity in &visible.entities {
            if visible_entity == self_entity {
                continue;
            }
            let Ok(other_type) = others.get(visible_entity) else {
                continue;
            };
            if other_type.0 != self_type.0 {
                continue;
            }

            let affection = query_affection(mind, visible_entity).unwrap_or(stranger_value);
            affection_sum += affection;
        }

        if affection_sum <= 0.0 {
            let extraversion = personality.map(|p| p.traits.extraversion).unwrap_or(0.5);
            let loneliness_rate = LONELINESS_DECAY_PER_SEC * (0.5 + extraversion);
            drives.companionship.drain(loneliness_rate * dt);
            continue;
        }

        let delta = SOCIAL_PROXIMITY_DECAY_PER_SEC * affection_sum * dt;
        drives.companionship.top_up(delta);
    }
}

fn query_affection(mind: &MindGraph, other: Entity) -> Option<f32> {
    mind.get(&Node::Entity(other), Predicate::Affection)?
        .as_quantity()
        .map(|q| q.point_estimate())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mind::recognition::initialize_relationship_with_affection;

    fn make_mind_with_affection(other: Entity, affection: f32) -> MindGraph {
        let mut mind = MindGraph::default();
        initialize_relationship_with_affection(&mut mind, other, "Deer 2", 0, affection);
        mind
    }

    #[test]
    fn query_affection_returns_value_after_initialize() {
        let other = Entity::from_bits(42);
        let mind = make_mind_with_affection(other, 0.8);
        assert_eq!(query_affection(&mind, other), Some(0.8));
    }

    #[test]
    fn query_affection_is_none_for_unknown_entity() {
        let mind = MindGraph::default();
        let stranger = Entity::from_bits(99);
        assert!(query_affection(&mind, stranger).is_none());
    }

    #[test]
    fn extravert_humans_get_more_stranger_comfort_than_introverts() {
        use crate::agent::psyche::personality::{Personality, PersonalityTraits};
        let species = SpeciesProfile::human();
        let extravert = Personality {
            traits: PersonalityTraits {
                extraversion: 1.0,
                ..Default::default()
            },
        };
        let introvert = Personality {
            traits: PersonalityTraits {
                extraversion: 0.0,
                ..Default::default()
            },
        };

        let extra = stranger_affection_for(Some(&species), Some(&extravert));
        let intro = stranger_affection_for(Some(&species), Some(&introvert));

        assert!(intro > 0.0, "introvert floor should still be positive");
        assert!(
            extra > intro,
            "extravert should get more stranger comfort: extra={extra}, intro={intro}"
        );
        assert_eq!(intro, HUMAN_INTROVERT_STRANGER_BASELINE);
        assert_eq!(
            extra,
            HUMAN_INTROVERT_STRANGER_BASELINE + EXTRAVERT_STRANGER_BONUS
        );
    }

    #[test]
    fn animals_get_nonzero_stranger_comfort() {
        let deer = SpeciesProfile::deer();
        let value = stranger_affection_for(Some(&deer), None);
        assert_eq!(
            value, STRANGER_CONSPECIFIC_AFFECTION,
            "herd animals should get the safety-in-numbers baseline from any conspecific"
        );
    }

    #[test]
    fn reasonable_decay_tuning_empties_drive_in_a_few_seconds() {
        // Sanity check the constant: a herd of 3 at 0.8 affection each
        // (sum 1.6) at full loneliness should drain the drive inside ~5
        // simulated seconds. If someone retunes the constant and breaks
        // this, the herd becomes either inert or panicky.
        let mut drive = 1.0_f32;
        let affection_sum = 1.6;
        // 60 ticks/s × 5 s = 300 ticks; the decay system runs every 10 ticks.
        for _ in 0..30 {
            let dt = (1.0 / 60.0) * 10.0;
            let delta = SOCIAL_PROXIMITY_DECAY_PER_SEC * affection_sum * dt;
            drive = (drive - delta).max(0.0);
        }
        assert!(
            drive < 0.1,
            "after 5s with affection_sum 1.6, drive should be nearly empty (got {drive})"
        );
    }
}
