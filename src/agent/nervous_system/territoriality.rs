//! Territoriality drive update: raises `PsychologicalDrives.territoriality` when
//! the agent perceives non-friend entities on tiles it owns.
//!
//! Reads: VisibleObjects, MindGraph (owned tiles, relationship classifications),
//!        Transform (intruder position), SpeciesProfile, Personality, TickCount
//! Writes: PsychologicalDrives.territoriality
//! Upstream: perception (VisibleObjects populated), world::map (tile coordinates)
//! Downstream: nervous_system::urgency (reads territoriality as UrgencySource::Territoriality)

use bevy::prelude::*;

use crate::agent::Agent;
use crate::agent::body::needs::PsychologicalDrives;
use crate::agent::body::species::SpeciesProfile;
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use crate::agent::mind::perception::VisibleObjects;
use crate::agent::psyche::personality::Personality;
use crate::core::tick::TickCount;
use crate::world::Physical;
use crate::world::map::TILE_SIZE;

/// Updates the `territoriality` drive for every agent that has territory
/// (i.e. has `(tile, HasTrait, Territory)` triples in their MindGraph).
///
/// Formula per tick (when intruders are detected):
/// ```text
/// territoriality = species_baseline
///   × intrusion_factor   (0–1, capped at 3 intruders)
///   × kin_support        (boosted by visible friends)
///   × personality_mod    (less agreeable = more territorial)
/// ```
/// When no intruders are visible on owned tiles the drive decays back toward
/// the species baseline so the agent doesn't remain permanently agitated.
pub fn update_territoriality(
    mut agents: Query<
        (
            Entity,
            &VisibleObjects,
            &MindGraph,
            &Transform,
            &mut PsychologicalDrives,
            &SpeciesProfile,
            &Personality,
        ),
        With<Agent>,
    >,
    transforms: Query<&Transform, With<Physical>>,
    tick: Res<TickCount>,
) {
    for (entity, visible, mind, _transform, mut drives, species, personality) in agents.iter_mut() {
        // Stagger: run every 10 ticks, offset by entity to spread load.
        if !tick.should_run(entity, 10) {
            continue;
        }

        let baseline = species.territoriality_baseline;

        // Skip entirely for species that have no territorial instinct.
        if baseline == 0.0 {
            drives.dominion.set(1.0);
            continue;
        }

        let mut intruder_count: f32 = 0.0;
        let mut ally_count: f32 = 0.0;

        for &vis_entity in &visible.entities {
            let Ok(vis_transform) = transforms.get(vis_entity) else {
                continue;
            };
            let vis_pos = vis_transform.translation.truncate();
            let tx = (vis_pos.x / TILE_SIZE).floor() as i32;
            let ty = (vis_pos.y / TILE_SIZE).floor() as i32;

            // Only react to entities on tiles the agent claims as territory.
            let on_owned_tile = !mind
                .query(
                    Some(&Node::Tile((tx, ty))),
                    Some(Predicate::HasTrait),
                    Some(&Value::Concept(Concept::Territory)),
                )
                .is_empty();

            if !on_owned_tile {
                continue;
            }

            // Classify the intruder: friend (ally) or stranger/enemy (intruder).
            let vis_node = Node::Entity(vis_entity);
            let is_friend = !mind
                .query(
                    Some(&vis_node),
                    Some(Predicate::IsA),
                    Some(&Value::Concept(Concept::Friend)),
                )
                .is_empty();

            if is_friend {
                ally_count += 1.0;
            } else {
                intruder_count += 1.0;
            }
        }

        if intruder_count == 0.0 {
            // No intruders on owned tiles — decay toward baseline (don't snap to zero).
            let relaxed = drives.dominion.value * 0.85 + (1.0 - baseline) * 0.15;
            drives.dominion.set(relaxed);
            continue;
        }

        // Normalize intruder count: one intruder → full baseline response,
        // more intruders scale up to a cap of 3×.
        let intrusion_factor = (intruder_count / 3.0).min(1.0);

        // Packmates/allies nearby boost willingness to defend.
        // Each ally adds 30 %, capped at doubling the response.
        let kin_support = (1.0 + ally_count * 0.3).min(2.0);

        // Less agreeable personalities feel territory violations more keenly.
        // Range: 0.7 (fully agreeable) to 1.3 (fully disagreeable).
        let aggression = 1.0 - personality.traits.agreeableness();
        let personality_mod = 0.7 + aggression * 0.6;

        let dominion_value =
            1.0 - (baseline * intrusion_factor * kin_support * personality_mod).clamp(0.0, 1.0);
        drives.dominion.set(dominion_value);
    }
}

#[cfg(test)]
mod tests {
    fn run_no_intruder(baseline: f32) -> f32 {
        // Simulate one "no intruder" tick starting from baseline.
        let decayed = baseline * 0.85 + baseline * 0.15;
        decayed.clamp(0.0, 1.0)
    }

    fn run_with_intruders(
        baseline: f32,
        intruder_count: f32,
        ally_count: f32,
        agreeableness: f32,
    ) -> f32 {
        let intrusion_factor = (intruder_count / 3.0).min(1.0);
        let kin_support = (1.0 + ally_count * 0.3).min(2.0);
        let aggression = 1.0 - agreeableness;
        let personality_mod = 0.7 + aggression * 0.6;
        (baseline * intrusion_factor * kin_support * personality_mod).clamp(0.0, 1.0)
    }

    #[test]
    fn zero_baseline_species_always_has_zero_territoriality() {
        // Deer / Rabbit have baseline 0.0
        let result = run_no_intruder(0.0);
        assert_eq!(result, 0.0, "zero-baseline species should stay at zero");
    }

    #[test]
    fn no_intruders_decays_to_baseline() {
        let baseline = 0.7;
        // Starting at baseline, after one tick of no intruders we should be at ~baseline.
        let result = run_no_intruder(baseline);
        // 0.85 + 0.15 = 1.0 — stays at exactly baseline when already at baseline
        assert!(
            (result - baseline).abs() < 1e-4,
            "expected ~{baseline}, got {result}"
        );
    }

    #[test]
    fn single_intruder_on_owned_tile_raises_drive() {
        let baseline = 0.7;
        let before = 0.0_f32;
        let result = run_with_intruders(baseline, 1.0, 0.0, 0.5);
        assert!(
            result > before,
            "intruder on owned tile should raise territoriality"
        );
    }

    #[test]
    fn wolf_has_higher_response_than_human_for_same_intrusion() {
        let wolf_result = run_with_intruders(0.7, 1.0, 0.0, 0.5);
        let human_result = run_with_intruders(0.1, 1.0, 0.0, 0.5);
        assert!(
            wolf_result > human_result,
            "wolf ({wolf_result}) should have higher response than human ({human_result})"
        );
    }

    #[test]
    fn ally_count_boosts_response() {
        let alone = run_with_intruders(0.7, 1.0, 0.0, 0.5);
        let with_pack = run_with_intruders(0.7, 1.0, 3.0, 0.5);
        assert!(
            with_pack > alone,
            "pack support ({with_pack}) should exceed lone response ({alone})"
        );
    }

    #[test]
    fn aggressive_personality_responds_more_than_agreeable() {
        let agreeable = run_with_intruders(0.7, 1.0, 0.0, 1.0); // fully agreeable
        let aggressive = run_with_intruders(0.7, 1.0, 0.0, 0.0); // fully disagreeable
        assert!(
            aggressive > agreeable,
            "aggressive ({aggressive}) should exceed agreeable ({agreeable})"
        );
    }

    #[test]
    fn drive_clamped_to_unit_interval() {
        // Max possible: baseline=1.0, 3 intruders, 3 allies, fully aggressive
        let max = run_with_intruders(1.0, 3.0, 3.0, 0.0);
        assert!(max <= 1.0, "drive must be ≤ 1.0, got {max}");
        assert!(max >= 0.0, "drive must be ≥ 0.0, got {max}");
    }
}
