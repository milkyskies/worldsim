//! Food-security tick. Drains baseline always; recovers when the agent
//! carries surplus food OR is near a known `StorageChest`. Same proximity-
//! only recovery idiom as `tick_warmth` and `tick_rest_quality`.

use bevy::prelude::*;

use crate::agent::Agent;
use crate::agent::body::need::crossed_threshold;
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::body::species::{Species, SpeciesProfile};
use crate::agent::events::{SimEvent, SimEventKind};
use crate::agent::inventory::EntityType;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::{Concept, Ontology};
use crate::constants::brains::food_security::{
    BASELINE_DRAIN_PER_SEC, COMFORT_THRESHOLD, CRITICAL_THRESHOLD, STOCKED_CHEST_RECOVERY_PER_SEC,
    SURPLUS_RECOVERY_PER_SEC, SURPLUS_THRESHOLD, URGENT_THRESHOLD,
};
use crate::core::tick::TickCount;
use crate::world::map::TILE_SIZE;

/// Range within which a known `StorageChest` confers food-security
/// recovery. Matches the shelter / heat-emitter proximity radius.
const CHEST_RANGE: f32 = TILE_SIZE * 3.0;

pub fn tick_food_security(
    tick: Res<TickCount>,
    ontology: Res<Ontology>,
    chests: Query<(&Transform, &EntityType, &ItemSlots)>,
    mut agents: Query<
        (
            Entity,
            &Transform,
            &mut PhysicalNeeds,
            &ItemSlots,
            Option<&SpeciesProfile>,
        ),
        With<Agent>,
    >,
    mut sim_events: MessageWriter<SimEvent>,
) {
    let dt = tick.dt();
    let current_tick = tick.current;

    for (agent_entity, agent_transform, mut physical, inventory, species) in agents.iter_mut() {
        if !matches!(species.map(|s| s.species), Some(Species::Human)) {
            continue;
        }

        let agent_pos = agent_transform.translation.truncate();
        let near_stocked_chest = chests.iter().any(|(transform, entity_type, slots)| {
            entity_type.0 == Concept::StorageChest
                && transform.translation.truncate().distance(agent_pos) <= CHEST_RANGE
                && slots.all_items().next().is_some()
        });
        let surplus = surplus_food_count(inventory, &ontology) >= SURPLUS_THRESHOLD;

        let rate_per_sec = compute_food_security_rate(near_stocked_chest, surplus);

        let old = physical.food_security.value;
        physical.food_security.apply_delta(rate_per_sec * dt);
        let new = physical.food_security.value;

        if crossed_threshold(old, new, NAMED_THRESHOLDS) {
            sim_events.write(SimEvent::single(
                current_tick,
                agent_entity,
                SimEventKind::FoodSecurityChanged {
                    agent: agent_entity,
                    old_value: old,
                    new_value: new,
                },
            ));
        }
    }
}

fn surplus_food_count(inventory: &ItemSlots, ontology: &Ontology) -> u32 {
    inventory
        .all_items()
        .filter(|thing| ontology.has_trait(thing.concept, Concept::Edible))
        .count() as u32
}

fn compute_food_security_rate(near_stocked_chest: bool, surplus: bool) -> f32 {
    let recovery = match (near_stocked_chest, surplus) {
        (true, _) => STOCKED_CHEST_RECOVERY_PER_SEC,
        (false, true) => SURPLUS_RECOVERY_PER_SEC,
        (false, false) => 0.0,
    };
    -BASELINE_DRAIN_PER_SEC + recovery
}

const NAMED_THRESHOLDS: &[f32] = &[COMFORT_THRESHOLD, URGENT_THRESHOLD, CRITICAL_THRESHOLD];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_stocked_chest_no_surplus_drains_baseline() {
        let rate = compute_food_security_rate(false, false);
        assert!((rate - (-BASELINE_DRAIN_PER_SEC)).abs() < 1e-6);
    }

    #[test]
    fn surplus_alone_recovers_slowly() {
        let rate = compute_food_security_rate(false, true);
        let expected = -BASELINE_DRAIN_PER_SEC + SURPLUS_RECOVERY_PER_SEC;
        assert!((rate - expected).abs() < 1e-6);
        assert!(rate > 0.0);
    }

    #[test]
    fn stocked_chest_recovers_faster_than_surplus() {
        let chest = compute_food_security_rate(true, false);
        let surplus = compute_food_security_rate(false, true);
        assert!(chest > surplus);
    }

    #[test]
    fn stocked_chest_dominates_surplus_when_both_present() {
        let both = compute_food_security_rate(true, true);
        let chest_only = compute_food_security_rate(true, false);
        // Stocked-chest recovery is the ceiling — surplus doesn't add on top.
        assert!((both - chest_only).abs() < 1e-6);
    }

    #[test]
    fn crossing_comfort_threshold_fires() {
        assert!(crossed_threshold(0.65, 0.55, NAMED_THRESHOLDS));
        assert!(crossed_threshold(0.55, 0.65, NAMED_THRESHOLDS));
    }
}
