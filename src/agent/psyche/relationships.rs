//! Relationship dynamics - updates relationships based on social interactions.
//!
//! When social interactions occur:
//! - Trust, affection, and respect are updated
//! - Positive interactions increase values (small amounts)
//! - Negative interactions decrease values (larger amounts - negativity bias!)
//! - Relationships decay slowly without contact

use crate::agent::Agent;
use crate::agent::events::{ConversationTopic, GameEvent};
use crate::agent::mind::knowledge::{Metadata, MindGraph, Node, Predicate, Triple, Value};
use crate::agent::mind::recognition::initialize_relationship;
use crate::agent::psyche::personality::Personality;
use crate::core::tick::TickCount;
use bevy::prelude::*;

/// Configuration for relationship dynamics
#[derive(Resource, Reflect)]
#[reflect(Resource)]
pub struct RelationshipConfig {
    /// Trust gain per positive interaction
    pub positive_trust_gain: f32,
    /// Affection gain per positive interaction  
    pub positive_affection_gain: f32,
    /// Trust loss per negative interaction (larger = more impactful)
    pub negative_trust_loss: f32,
    /// Affection loss per negative interaction
    pub negative_affection_loss: f32,
    /// Respect gain when witnessing competence
    pub competence_respect_gain: f32,
    /// How much relationships decay per day without contact
    pub decay_rate_per_day: f32,
}

impl Default for RelationshipConfig {
    fn default() -> Self {
        Self {
            positive_trust_gain: 0.05,
            positive_affection_gain: 0.03,
            negative_trust_loss: 0.15, // 3x larger than positive - negativity bias!
            negative_affection_loss: 0.10,
            competence_respect_gain: 0.02,
            decay_rate_per_day: 0.01,
        }
    }
}

/// System: Update relationships based on social interaction events
pub fn update_relationships(
    mut events: MessageReader<GameEvent>,
    mut agents: Query<(Entity, &Name, &mut MindGraph, &Personality), With<Agent>>,
    config: Res<RelationshipConfig>,
    tick: Res<TickCount>,
) {
    let current_time = tick.current;

    for event in events.read() {
        if let GameEvent::SocialInteraction {
            actor,
            target,
            action: _,
            topic,
            valence,
        } = event
        {
            // Update target's feelings about actor (the one who did the action)
            if let Ok((_, actor_name, _, _)) = agents.get(*actor) {
                let actor_name_str = actor_name.to_string();

                if let Ok((_, _, mut target_mind, personality)) = agents.get_mut(*target) {
                    let actor_node = Node::Entity(*actor);

                    // Check if we know this person, if not initialize
                    let knows = target_mind.query(
                        Some(&actor_node),
                        Some(Predicate::Knows),
                        Some(&Value::Boolean(true)),
                    );

                    if knows.is_empty() {
                        // First meeting! Initialize relationship
                        initialize_relationship(
                            &mut target_mind,
                            *actor,
                            &actor_name_str,
                            current_time,
                        );
                    }

                    // Get current values
                    let current_trust = target_mind
                        .get(&actor_node, Predicate::Trust)
                        .and_then(|v| {
                            if let Value::Float(f) = v {
                                Some(*f)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0.5);

                    let current_affection = target_mind
                        .get(&actor_node, Predicate::Affection)
                        .and_then(|v| {
                            if let Value::Float(f) = v {
                                Some(*f)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0.5);

                    // Calculate changes based on valence
                    let (trust_delta, affection_delta) = if *valence > 0.0 {
                        // Positive interaction
                        let trust_gain = config.positive_trust_gain * valence;
                        let affection_gain = config.positive_affection_gain * valence;

                        // Topic modifiers
                        let (t_mod, a_mod) = match topic {
                            Some(ConversationTopic::Feelings) => (0.5, 1.5), // Feelings build affection
                            Some(ConversationTopic::Knowledge) => (1.2, 0.8), // Knowledge builds trust
                            Some(ConversationTopic::Gossip) => (0.8, 1.0),
                            _ => (1.0, 1.0),
                        };

                        (trust_gain * t_mod, affection_gain * a_mod)
                    } else {
                        // Negative interaction - larger impact!
                        let trust_loss = config.negative_trust_loss * valence.abs();
                        let affection_loss = config.negative_affection_loss * valence.abs();
                        (-trust_loss, -affection_loss)
                    };

                    // Apply personality modifiers
                    // High agreeableness = bigger trust gains, smaller losses
                    let agreeableness = personality.traits.agreeableness;
                    let trust_mod = if trust_delta > 0.0 {
                        0.5 + agreeableness
                    } else {
                        1.5 - agreeableness
                    };

                    // High neuroticism = bigger negativity impact
                    let neuroticism = personality.traits.neuroticism;
                    let negative_mod = if trust_delta < 0.0 {
                        1.0 + neuroticism * 0.5
                    } else {
                        1.0
                    };

                    // Calculate new values
                    let new_trust =
                        (current_trust + trust_delta * trust_mod * negative_mod).clamp(0.0, 1.0);
                    let new_affection =
                        (current_affection + affection_delta * negative_mod).clamp(0.0, 1.0);

                    // Update MindGraph
                    target_mind.assert(Triple::with_meta(
                        actor_node.clone(),
                        Predicate::Trust,
                        Value::Float(new_trust),
                        Metadata::semantic(current_time),
                    ));

                    target_mind.assert(Triple::with_meta(
                        actor_node,
                        Predicate::Affection,
                        Value::Float(new_affection),
                        Metadata::semantic(current_time),
                    ));
                }
            }
        }
    }
}

/// System: Decay relationships over time without contact
/// Runs periodically (every game day)
pub fn decay_relationships(
    mut agents: Query<&mut MindGraph, With<Agent>>,
    tick: Res<TickCount>,
    config: Res<RelationshipConfig>,
) {
    // Only run once per game day (1440 ticks at 60 ticks/second = 24 minutes real time)
    // For now, run every ~5 minutes real time (300 ticks)
    if !tick.current.is_multiple_of(300) {
        return;
    }

    let decay = config.decay_rate_per_day;
    let current_time = tick.current;

    for mut mind in agents.iter_mut() {
        // Find all relationship entries
        let trust_entries: Vec<_> = mind
            .query(None, Some(Predicate::Trust), None)
            .into_iter()
            .filter_map(|t| {
                if let Node::Entity(e) = &t.subject
                    && let Value::Float(f) = &t.object {
                        return Some((*e, *f));
                    }
                None
            })
            .collect();

        // Decay toward neutral (0.5)
        for (entity, current) in trust_entries {
            let target = 0.5; // Neutral
            let new_value = current + (target - current) * decay;

            mind.assert(Triple::with_meta(
                Node::Entity(entity),
                Predicate::Trust,
                Value::Float(new_value.clamp(0.0, 1.0)),
                Metadata::semantic(current_time),
            ));
        }

        // Same for affection
        let affection_entries: Vec<_> = mind
            .query(None, Some(Predicate::Affection), None)
            .into_iter()
            .filter_map(|t| {
                if let Node::Entity(e) = &t.subject
                    && let Value::Float(f) = &t.object {
                        return Some((*e, *f));
                    }
                None
            })
            .collect();

        for (entity, current) in affection_entries {
            let target = 0.5;
            let new_value = current + (target - current) * decay;

            mind.assert(Triple::with_meta(
                Node::Entity(entity),
                Predicate::Affection,
                Value::Float(new_value.clamp(0.0, 1.0)),
                Metadata::semantic(current_time),
            ));
        }
    }
}
