//! Social Perception - what species visible agents are.
//!
//! Reads: VisibleObjects, EntityType
//! Writes: MindGraph (Entity, IsA, Concept) — observed species.
//! Upstream: perception (VisibleObjects)
//! Downstream: brain target enumeration, react_to_danger.

use crate::agent::Agent;
use crate::agent::inventory::EntityType;
use crate::agent::mind::knowledge::{Concept, Metadata, MindGraph, Node, Predicate, Triple, Value};
use crate::agent::mind::perception::VisibleObjects;
use crate::core::tick::TickCount;
use bevy::prelude::*;

/// Conversation range in pixels (~2 tiles)
pub const CONVERSATION_RANGE: f32 = 32.0;

/// Per-tick: write `(visible_entity, IsA, Species)` for every other agent
/// in view. Distance-weighted confidence so far-away observations decay
/// faster.
pub fn perceive_other_agents(
    mut observers: Query<(Entity, &Transform, &VisibleObjects, &mut MindGraph), With<Agent>>,
    observable_agents: Query<(Entity, &Transform, &EntityType), With<Agent>>,
    tick: Res<TickCount>,
) {
    let current_time = tick.current;

    for (observer_entity, observer_transform, visible, mut mind) in observers.iter_mut() {
        let observer_pos = observer_transform.translation.truncate();

        let agent_targets: Vec<Entity> = visible
            .iter_by_concept(|c| mind.has_trait(&Node::Concept(c), Concept::Sentient))
            .filter(|e| *e != observer_entity)
            .collect();

        for visible_entity in agent_targets {
            let Ok((_, target_transform, entity_type)) = observable_agents.get(visible_entity)
            else {
                continue;
            };
            let distance = observer_pos.distance(target_transform.translation.truncate());
            let confidence = (1.0 - (distance / 256.0).min(1.0)).max(0.3);
            mind.assert(Triple::with_meta(
                Node::Entity(visible_entity),
                Predicate::IsA,
                Value::Concept(entity_type.0),
                Metadata::perception_with_conf(current_time, confidence),
            ));
        }
    }
}

/// Check if two positions are within conversation range
pub fn within_conversation_range(pos_a: Vec2, pos_b: Vec2) -> bool {
    pos_a.distance(pos_b) <= CONVERSATION_RANGE
}
