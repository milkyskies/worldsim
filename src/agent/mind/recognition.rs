//! Recognition System - Detect strangers and track who we've met
//!
//! When agents see other agents, this system:
//! 1. Checks if we've met them before (Knows predicate)
//! 2. Marks strangers so the social brain can propose introductions
//! 3. Tracks familiarity levels

use crate::agent::Agent;
use crate::agent::mind::knowledge::{Concept, Metadata, MindGraph, Node, Predicate, Triple, Value};
use crate::agent::mind::perception::VisibleObjects;
use crate::core::tick::TickCount;
use bevy::prelude::*;

/// System: Check if visible agents are known or strangers
pub fn check_recognition(
    mut observers: Query<(Entity, &VisibleObjects, &mut MindGraph), With<Agent>>,
    agents: Query<Entity, With<Agent>>,
    tick: Res<TickCount>,
) {
    let current_time = tick.current;

    for (observer_entity, visible, mut mind) in observers.iter_mut() {
        for &visible_entity in &visible.entities {
            // Skip self
            if visible_entity == observer_entity {
                continue;
            }

            // Only process other agents
            if agents.get(visible_entity).is_err() {
                continue;
            }

            let target_node = Node::Entity(visible_entity);

            // Check: Do I know this entity?
            let knows_triples = mind.query(
                Some(&target_node),
                Some(Predicate::Knows),
                Some(&Value::Boolean(true)),
            );

            if knows_triples.is_empty() {
                // This is a stranger! Mark them as such
                // The social brain will see this and propose introduction
                mind.assert(Triple::with_meta(
                    target_node.clone(),
                    Predicate::IsA,
                    Value::Concept(Concept::Stranger),
                    Metadata::perception(current_time),
                ));

                // Remove any stale "known" relationship type
                // (they might have been Friend/Acquaintance but we forgot)
                mind.remove(
                    &target_node,
                    Predicate::IsA,
                    &Value::Concept(Concept::Friend),
                );
                mind.remove(
                    &target_node,
                    Predicate::IsA,
                    &Value::Concept(Concept::Acquaintance),
                );
            } else {
                // We know them - remove stranger tag if present
                mind.remove(
                    &target_node,
                    Predicate::IsA,
                    &Value::Concept(Concept::Stranger),
                );

                // Update relationship category based on trust/affection
                update_relationship_category(&mut mind, &target_node, current_time);
            }
        }
    }
}

/// Update the relationship category (Friend, Acquaintance, etc.) based on dimensions
fn update_relationship_category(mind: &mut MindGraph, target: &Node, timestamp: u64) {
    // Get current trust and affection
    let trust = mind
        .get(target, Predicate::Trust)
        .and_then(|v| match v {
            Value::Float(f) => Some(*f),
            _ => None,
        })
        .unwrap_or(0.5); // Default neutral

    let affection = mind
        .get(target, Predicate::Affection)
        .and_then(|v| match v {
            Value::Float(f) => Some(*f),
            _ => None,
        })
        .unwrap_or(0.5);

    // Determine category
    let category = if trust > 0.7 && affection > 0.7 {
        Concept::Friend
    } else if trust < 0.3 && affection < 0.3 {
        Concept::Enemy
    } else if trust < 0.5 && affection > 0.5 {
        Concept::Rival // Like them but don't trust them
    } else {
        Concept::Acquaintance
    };

    // Remove old categories
    for old_cat in [
        Concept::Friend,
        Concept::Acquaintance,
        Concept::Rival,
        Concept::Enemy,
    ] {
        if old_cat != category {
            mind.remove(target, Predicate::IsA, &Value::Concept(old_cat));
        }
    }

    // Set new category
    mind.assert(Triple::with_meta(
        target.clone(),
        Predicate::IsA,
        Value::Concept(category),
        Metadata::semantic(timestamp),
    ));
}

/// Initialize relationship when meeting someone for the first time
pub fn initialize_relationship(mind: &mut MindGraph, entity: Entity, name: &str, timestamp: u64) {
    let target = Node::Entity(entity);

    // Mark as known
    mind.assert(Triple::with_meta(
        target.clone(),
        Predicate::Knows,
        Value::Boolean(true),
        Metadata::semantic(timestamp),
    ));

    // Mark as introduced
    mind.assert(Triple::with_meta(
        target.clone(),
        Predicate::Introduced,
        Value::Boolean(true),
        Metadata::semantic(timestamp),
    ));

    // Store their name
    mind.assert(Triple::with_meta(
        target.clone(),
        Predicate::NameOf,
        Value::Text(name.to_string()),
        Metadata::semantic(timestamp),
    ));

    // Initialize neutral relationship dimensions
    mind.assert(Triple::with_meta(
        target.clone(),
        Predicate::Trust,
        Value::Float(0.5),
        Metadata::semantic(timestamp),
    ));

    mind.assert(Triple::with_meta(
        target.clone(),
        Predicate::Affection,
        Value::Float(0.5),
        Metadata::semantic(timestamp),
    ));

    mind.assert(Triple::with_meta(
        target.clone(),
        Predicate::Respect,
        Value::Float(0.5),
        Metadata::semantic(timestamp),
    ));

    mind.assert(Triple::with_meta(
        target.clone(),
        Predicate::PowerBalance,
        Value::Float(0.0), // Equal power
        Metadata::semantic(timestamp),
    ));
}
