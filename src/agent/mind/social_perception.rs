//! Social Perception - Observe other agents' actions, mood, and states
//!
//! When agents see each other, they perceive:
//! - What the other agent is doing (current action)
//! - Their apparent mood (happy, sad, angry, fearful, neutral)
//! - Whether they appear injured
//! - Their movement direction (heading)
//! - Whether they are a stranger or someone we've met

use crate::agent::Agent;
use crate::agent::inventory::EntityType;
use crate::agent::mind::knowledge::{
    AgentName, Metadata, MindGraph, Node, Predicate, Triple, Value,
};
use crate::agent::mind::perception::VisibleObjects;
use crate::core::tick::TickCount;
use bevy::prelude::*;

/// Conversation range in pixels (~2 tiles)
pub const CONVERSATION_RANGE: f32 = 32.0;

/// System: Perceive other agents' observable states
pub fn perceive_other_agents(
    mut observers: Query<(Entity, &Transform, &VisibleObjects, &mut MindGraph), With<Agent>>,
    observable_agents: Query<(Entity, &Name, &Transform, &EntityType), With<Agent>>,
    tick: Res<TickCount>,
) {
    let current_time = tick.current;

    for (observer_entity, observer_transform, visible, mut mind) in observers.iter_mut() {
        let observer_pos = observer_transform.translation.truncate();

        for &visible_entity in &visible.entities {
            // Skip self
            if visible_entity == observer_entity {
                continue;
            }

            // Only perceive other agents
            let Ok((_, name, target_transform, entity_type)) =
                observable_agents.get(visible_entity)
            else {
                continue;
            };

            let target_pos = target_transform.translation.truncate();
            let distance = observer_pos.distance(target_pos);

            // Confidence decreases with distance
            let confidence = (1.0 - (distance / 256.0).min(1.0)).max(0.3);
            let meta = Metadata::perception_with_conf(current_time, confidence);

            let target_node = Node::Entity(visible_entity);

            // 1. Perceive type: what species this entity actually is
            mind.assert(Triple::with_meta(
                target_node.clone(),
                Predicate::IsA,
                Value::Concept(entity_type.0),
                meta.clone(),
            ));

            // (Doing / AppearsMood / AppearsInjured used to be written here as
            // triples every tick. No production behaviour ever queried them —
            // `deliberate_talk` filtered them out of speech, and that was the
            // only consumer. Deleted entirely per #587. If a future feature
            // needs "is the agent observing X is sad", surface it through the
            // event-driven brain wakeup pipeline instead.)

            // (Names used to be written here as `(Entity, NameOf, Text)`
            // triples gated on a `(Entity, Introduced, true)` query. The
            // social ledger now lives in `SocialIdentity`, populated by
            // `recognition::initialize_relationship` on first formal
            // introduction — we don't re-write names from passive sight.)
            let _ = (name, target_node);
        }
    }
}

/// Check if two positions are within conversation range
pub fn within_conversation_range(pos_a: Vec2, pos_b: Vec2) -> bool {
    pos_a.distance(pos_b) <= CONVERSATION_RANGE
}
