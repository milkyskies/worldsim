//! Recognition System - Detect strangers and track who we've met
//!
//! When agents see other agents, this system:
//! 1. Checks if we've met them before (Knows predicate)
//! 2. Marks strangers so the social brain can propose introductions
//! 3. Tracks familiarity levels
//!
//! Emits SimEvent::StrangerDetected on first encounter.

use std::collections::VecDeque;

use crate::agent::Agent;
use crate::agent::events::SimEventKind;
use crate::agent::mind::knowledge::{
    Concept, Metadata, MindGraph, Node, Predicate, Quantity, Triple, Value,
};
use crate::agent::mind::perception::VisibleObjects;
use crate::agent::psyche::relationships::{InteractionRecord, RelationshipHistory};
use crate::core::tick::TickCount;
use bevy::prelude::*;

/// Minimum number of interactions before a relationship can be classified as
/// anything beyond Acquaintance. Prevents a single lucky greeting from
/// producing a "Friend" classification.
pub const MIN_INTERACTIONS_FOR_BOND: usize = 3;

/// Minimum interactions required for Friend classification.
pub const MIN_INTERACTIONS_FOR_FRIEND: usize = 8;

/// Minimum interactions required for Enemy classification.
pub const MIN_INTERACTIONS_FOR_ENEMY: usize = 5;

/// System: Check if visible agents are known or strangers
pub fn check_recognition(
    mut observers: Query<
        (
            Entity,
            &VisibleObjects,
            &mut MindGraph,
            &crate::agent::mind::social_identity::SocialIdentity,
            &RelationshipHistory,
        ),
        With<Agent>,
    >,
    agents: Query<Entity, With<Agent>>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
) {
    let current_time = tick.current;

    for (observer_entity, visible, mut mind, social_identity, history) in observers.iter_mut() {
        let agent_targets: Vec<Entity> = visible
            .iter_by_concept(|c| mind.has_trait(&Node::Concept(c), Concept::Sentient))
            .filter(|e| *e != observer_entity && agents.get(*e).is_ok())
            .collect();

        for visible_entity in agent_targets {
            let target_node = Node::Entity(visible_entity);

            if !social_identity.knows(visible_entity) {
                // Stranger — write the IsA tag once, when it isn't already
                // there, to avoid a per-tick MindGraph round-trip while
                // the stranger remains in view. The social brain proposes
                // introduction; emit the wakeup-style event regardless.
                sim_events.write(crate::agent::events::SimEvent::single(
                    current_time,
                    observer_entity,
                    SimEventKind::StrangerDetected {
                        agent: observer_entity,
                        stranger: visible_entity,
                    },
                ));
                if !mind.has(
                    &target_node,
                    Predicate::IsA,
                    &Value::Concept(Concept::Stranger),
                ) {
                    mind.assert(Triple::with_meta(
                        target_node.clone(),
                        Predicate::IsA,
                        Value::Concept(Concept::Stranger),
                        Metadata::perception(current_time),
                    ));
                    // Drop any stale relationship category from a previous
                    // acquaintance we've since forgotten about.
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
                }
            } else {
                mind.remove(
                    &target_node,
                    Predicate::IsA,
                    &Value::Concept(Concept::Stranger),
                );

                let partner = match target_node {
                    Node::Entity(e) => e,
                    _ => continue,
                };
                let log = history.get(partner);
                update_relationship_category(&mut mind, &target_node, log, current_time);
            }
        }
    }
}

/// Classify a relationship from the interaction log rather than trust/affection
/// thresholds. Requires a minimum history before any classification beyond
/// Acquaintance — two agents who just met cannot be "Friends" regardless of
/// initial trust values.
fn update_relationship_category(
    mind: &mut MindGraph,
    target: &Node,
    log: &VecDeque<InteractionRecord>,
    timestamp: u64,
) {
    let category = classify_from_history(log);

    // Remove old categories.
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

    mind.assert(Triple::with_meta(
        target.clone(),
        Predicate::IsA,
        Value::Concept(category),
        Metadata::semantic(timestamp),
    ));
}

/// Classify a relationship from the interaction log.
///
/// The classification reads the *pattern* of interactions, not summary floats:
///
/// - **Friend**: 8+ interactions, >70% positive, weighted valence > 0.4
/// - **Enemy**: 5+ interactions, >70% negative, weighted valence < -0.3
/// - **Rival**: has at least one strongly negative interaction AND positive ratio > 30%
///   (mixed feelings — conflict, not indifference)
/// - **Acquaintance**: default, including insufficient history
pub(crate) fn classify_from_history(log: &VecDeque<InteractionRecord>) -> Concept {
    let total = log.len();
    if total < MIN_INTERACTIONS_FOR_BOND {
        return Concept::Acquaintance;
    }

    let positive_count = log.iter().filter(|i| i.valence > 0.0).count();
    let negative_count = log.iter().filter(|i| i.valence < 0.0).count();
    let positive_ratio = positive_count as f32 / total as f32;
    let negative_ratio = negative_count as f32 / total as f32;

    let weighted_valence: f32 = log.iter().map(|i| i.valence).sum::<f32>() / total as f32;

    // Friend: many interactions, mostly positive, accumulated valence high.
    if total >= MIN_INTERACTIONS_FOR_FRIEND && positive_ratio > 0.7 && weighted_valence > 0.4 {
        return Concept::Friend;
    }

    // Enemy: enough interactions, mostly negative.
    if total >= MIN_INTERACTIONS_FOR_ENEMY && negative_ratio > 0.7 && weighted_valence < -0.3 {
        return Concept::Enemy;
    }

    // Rival: mixed — some strong positive interactions interleaved with
    // strongly negative ones. Not indifference; active conflict.
    let has_strong_negative = log.iter().any(|i| i.valence < -0.5);
    if has_strong_negative && positive_ratio > 0.3 {
        return Concept::Rival;
    }

    Concept::Acquaintance
}

/// Initialize the epistemic relationship dimensions (Trust / Affection /
/// Respect / PowerBalance) in the agent's MindGraph. The Knows /
/// Introduced / NameOf side of "I've met them" lives in
/// `SocialIdentity` — call `social.introduce(entity, name, tick)`
/// alongside this. Splitting the two halves lets callers grab `&mut` to
/// each component independently without fighting Bevy's borrow rules.
pub fn init_relationship_dimensions(
    mind: &mut MindGraph,
    entity: Entity,
    timestamp: u64,
    baseline_affection: f32,
) {
    let target = Node::Entity(entity);

    let neutral = Value::Quantity(Quantity::Exact(0.5));
    mind.assert(Triple::with_meta(
        target.clone(),
        Predicate::Trust,
        neutral.clone(),
        Metadata::semantic(timestamp),
    ));

    mind.assert(Triple::with_meta(
        target.clone(),
        Predicate::Affection,
        Value::Quantity(Quantity::Exact(baseline_affection.clamp(0.0, 1.0))),
        Metadata::semantic(timestamp),
    ));

    mind.assert(Triple::with_meta(
        target.clone(),
        Predicate::Respect,
        neutral,
        Metadata::semantic(timestamp),
    ));

    mind.assert(Triple::with_meta(
        target.clone(),
        Predicate::PowerBalance,
        Value::Quantity(Quantity::Exact(0.0)), // Equal power
        Metadata::semantic(timestamp),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(valence: f32) -> InteractionRecord {
        InteractionRecord {
            tick: 0,
            topic: None,
            valence,
        }
    }

    fn log_of(records: &[f32]) -> VecDeque<InteractionRecord> {
        records.iter().map(|v| record(*v)).collect()
    }

    #[test]
    fn no_history_is_acquaintance() {
        let log = VecDeque::new();
        assert_eq!(classify_from_history(&log), Concept::Acquaintance);
    }

    #[test]
    fn insufficient_history_is_acquaintance() {
        // Only 2 interactions — below MIN_INTERACTIONS_FOR_BOND (3).
        let log = log_of(&[0.8, 0.9]);
        assert_eq!(classify_from_history(&log), Concept::Acquaintance);
    }

    #[test]
    fn many_positive_interactions_become_friend() {
        // 10 high-valence interactions → Friend.
        let log = log_of(&[0.6, 0.7, 0.8, 0.5, 0.9, 0.7, 0.6, 0.8, 0.5, 0.7]);
        assert_eq!(classify_from_history(&log), Concept::Friend);
    }

    #[test]
    fn many_negative_interactions_become_enemy() {
        // 8 negative-valence interactions → Enemy.
        let log = log_of(&[-0.5, -0.6, -0.7, -0.4, -0.8, -0.5, -0.6, -0.7]);
        assert_eq!(classify_from_history(&log), Concept::Enemy);
    }

    #[test]
    fn mixed_strong_interactions_become_rival() {
        // Positive interactions with at least one strongly negative → Rival.
        let log = log_of(&[0.5, 0.6, -0.8, 0.4, 0.3]);
        assert_eq!(classify_from_history(&log), Concept::Rival);
    }

    #[test]
    fn high_trust_but_no_interactions_is_acquaintance() {
        // Verifies that artificially high trust/affection without a history
        // does NOT produce Friend — the old threshold bug.
        let log = VecDeque::new();
        assert_eq!(
            classify_from_history(&log),
            Concept::Acquaintance,
            "no history → Acquaintance regardless of trust/affection values"
        );
    }

    #[test]
    fn mostly_positive_with_few_negatives_is_friend() {
        // 9 positive + 1 negative → still Friend (positive_ratio > 0.7).
        let log = log_of(&[0.6, 0.7, 0.5, -0.2, 0.8, 0.6, 0.7, 0.5, 0.6, 0.8]);
        assert_eq!(classify_from_history(&log), Concept::Friend);
    }

    #[test]
    fn neutral_interactions_stay_acquaintance() {
        // All zero-valence interactions → Acquaintance (not positive, not negative).
        let log = log_of(&[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(classify_from_history(&log), Concept::Acquaintance);
    }
}
