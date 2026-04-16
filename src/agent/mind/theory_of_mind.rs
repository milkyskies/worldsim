//! Theory of Mind: agents model what other agents know, believe, and feel.
//!
//! Reads: MindGraph (own knowledge), Conversation/Turn (what was shared)
//! Writes: TheoryOfMind (updated belief models about other agents)
//! Upstream: communication (shared triples update models), perception (shared experience)
//! Downstream: communication (novelty_score, has_danger_to_warn use belief models)
//!
//! Each agent maintains a lightweight model of what they *think* other agents know.
//! This model is imperfect — agents can be wrong about what others know. The model
//! is built from:
//! - Direct communication: "I told Alice X" → Alice probably knows X
//! - Shared experience: "Alice was nearby when Y happened" → Alice probably saw Y
//!
//! The model is used to replace direct mind queries in content selection: instead of
//! looking at what the listener *actually* knows, the speaker checks what they
//! *believe* the listener knows.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use super::knowledge::{Node, Predicate, Triple, Value};

/// Maximum number of triples stored per modeled agent.
/// Keeps memory bounded — oldest entries are evicted when full.
const MAX_TRIPLES_PER_AGENT: usize = 64;

/// Confidence assigned to beliefs formed via communication ("I told them").
pub const COMMUNICATED_BELIEF_CONFIDENCE: f32 = 0.8;

/// Confidence assigned to beliefs formed via shared experience ("they were there").
pub const SHARED_EXPERIENCE_CONFIDENCE: f32 = 0.7;

/// An agent's model of what other agents know.
///
/// Each entry maps a known entity to a set of triples the agent *believes*
/// that entity knows. This is a first-order theory of mind — "I think Alice
/// knows X" — not recursive ("I think Alice thinks Bob knows X").
#[derive(Component, Clone, Debug, Default, Reflect)]
#[reflect(Component)]
pub struct TheoryOfMind {
    /// Per-agent belief models. Key = the entity being modeled.
    #[reflect(ignore)]
    models: HashMap<Entity, Vec<BeliefEntry>>,
}

/// A single belief about what another agent knows.
#[derive(Clone, Debug)]
pub struct BeliefEntry {
    pub subject: Node,
    pub predicate: Predicate,
    pub object: Value,
    /// How confident the modeler is that the target knows this (0.0–1.0).
    pub confidence: f32,
    /// When this belief was formed (game tick).
    pub timestamp: u64,
}

impl TheoryOfMind {
    /// Record that we believe `target` knows a specific fact.
    ///
    /// If the target already has a matching belief (same subject+predicate+object),
    /// the confidence and timestamp are updated to the higher/newer values.
    pub fn record_belief(
        &mut self,
        target: Entity,
        subject: Node,
        predicate: Predicate,
        object: Value,
        confidence: f32,
        timestamp: u64,
    ) {
        let entries = self.models.entry(target).or_default();

        if let Some(existing) = entries
            .iter_mut()
            .find(|e| e.subject == subject && e.predicate == predicate && e.object == object)
        {
            existing.confidence = existing.confidence.max(confidence);
            existing.timestamp = existing.timestamp.max(timestamp);
            return;
        }

        if entries.len() >= MAX_TRIPLES_PER_AGENT
            && let Some(oldest_idx) = entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.timestamp)
                .map(|(i, _)| i)
        {
            entries.swap_remove(oldest_idx);
        }

        entries.push(BeliefEntry {
            subject,
            predicate,
            object,
            confidence,
            timestamp,
        });
    }

    /// Record that we believe `target` knows about a set of triples
    /// (e.g., because we just told them).
    pub fn record_shared_triples(
        &mut self,
        target: Entity,
        triples: &[Triple],
        confidence: f32,
        timestamp: u64,
    ) {
        for triple in triples {
            self.record_belief(
                target,
                triple.subject.clone(),
                triple.predicate,
                triple.object.clone(),
                confidence,
                timestamp,
            );
        }
    }

    /// Query how confident we are that `target` knows a specific fact.
    ///
    /// Returns 0.0 if we have no model for the target or no matching belief.
    pub fn believed_confidence(
        &self,
        target: Entity,
        subject: &Node,
        predicate: Predicate,
        object: &Value,
    ) -> f32 {
        self.models
            .get(&target)
            .and_then(|entries| {
                entries
                    .iter()
                    .filter(|e| {
                        e.subject == *subject && e.predicate == predicate && e.object == *object
                    })
                    .map(|e| e.confidence)
                    .reduce(f32::max)
            })
            .unwrap_or(0.0)
    }

    /// Iterate over all beliefs we hold about what `target` knows.
    ///
    /// Returns an empty iterator if we have no model for the target.
    pub fn beliefs_about(&self, target: Entity) -> impl Iterator<Item = &BeliefEntry> {
        self.models
            .get(&target)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
            .iter()
    }

    /// Check if `target` probably knows about a danger (HasTrait Dangerous)
    /// for a given subject, at or above the given confidence threshold.
    pub fn believes_target_knows_danger(
        &self,
        target: Entity,
        danger_subject: &Node,
        min_confidence: f32,
    ) -> bool {
        self.believed_confidence(
            target,
            danger_subject,
            Predicate::HasTrait,
            &Value::Concept(super::knowledge::Concept::Dangerous),
        ) >= min_confidence
    }

    /// Number of agents being modeled.
    pub fn modeled_agent_count(&self) -> usize {
        self.models.len()
    }

    /// Number of beliefs held about a specific agent.
    pub fn belief_count_for(&self, target: Entity) -> usize {
        self.models.get(&target).map_or(0, |v| v.len())
    }
}

// ============================================================================
// Shared novelty scoring
// ============================================================================

/// Score how novel a triple is to a listener based on the speaker's theory of mind.
///
/// Returns 1.0 if the speaker has no model for the listener (stranger model) or
/// believes the listener doesn't know this fact. Scales toward 0.0 as the
/// speaker's belief that the listener knows it grows.
///
/// Shared by both deliberate_talk and small_talk content selection.
pub fn tom_novelty_score(
    triple: &super::knowledge::Triple,
    speaker_tom: Option<&TheoryOfMind>,
    listener: Entity,
) -> f32 {
    let Some(tom) = speaker_tom else {
        return 1.0;
    };

    let known =
        tom.believed_confidence(listener, &triple.subject, triple.predicate, &triple.object);
    1.0 - known
}

// ============================================================================
// Shared-experience system
// ============================================================================

/// Minimum salience for a triple to be included in shared-experience ToM updates.
/// Only noteworthy observations are worth modeling — mundane perception is ignored.
const SHARED_EXPERIENCE_MIN_SALIENCE: f32 = 0.5;

/// When two agents are in conversation (co-located), each agent infers that the
/// other probably also observed any high-salience entities they can both see.
///
/// For each conversation pair, we check which entities both agents can see.
/// For each shared visible entity, we look at the observer's high-salience
/// triples about that entity and record in their ToM that the partner
/// probably knows about it too.
pub fn update_shared_experience_tom(
    manager: Res<crate::agent::mind::conversation::ConversationManager>,
    visible_query: Query<&super::perception::VisibleObjects>,
    minds: Query<&super::knowledge::MindGraph>,
    mut toms: Query<&mut TheoryOfMind>,
    tick: Res<crate::core::tick::TickCount>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
) {
    for conv in manager.conversations.values() {
        if conv.state == crate::agent::mind::conversation::ConversationState::Ended {
            continue;
        }
        if conv.participants.len() < 2 {
            continue;
        }

        // Precompute each participant's visible-entity set once so the
        // inner N×N loop is a pure lookup instead of rebuilding a HashSet
        // per (observer, partner) pair.
        let vis_sets: Vec<(Entity, HashSet<Entity>)> = conv
            .participants
            .iter()
            .copied()
            .filter_map(|p| {
                visible_query
                    .get(p)
                    .ok()
                    .map(|v| (p, v.entities.iter().copied().collect()))
            })
            .collect();

        for (observer, observer_set) in &vis_sets {
            let Ok(mind) = minds.get(*observer) else {
                continue;
            };
            let Ok(mut tom) = toms.get_mut(*observer) else {
                continue;
            };

            for (partner, partner_set) in &vis_sets {
                if partner == observer {
                    continue;
                }
                let mut count = 0usize;
                for visible_entity in observer_set.iter().filter(|e| partner_set.contains(e)) {
                    let node = Node::Entity(*visible_entity);
                    for triple in mind.query(Some(&node), None, None) {
                        if triple.meta.salience >= SHARED_EXPERIENCE_MIN_SALIENCE {
                            tom.record_belief(
                                *partner,
                                triple.subject.clone(),
                                triple.predicate,
                                triple.object.clone(),
                                SHARED_EXPERIENCE_CONFIDENCE,
                                tick.current,
                            );
                            count += 1;
                        }
                    }
                }

                if count > 0 {
                    sim_events.write(crate::agent::events::SimEvent::TheoryOfMindUpdated {
                        agent: *observer,
                        about: *partner,
                        tick: tick.current,
                        source: crate::agent::events::TheoryOfMindSource::SharedExperience,
                        belief_count: count,
                    });
                }
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mind::knowledge::{Concept, Node, Predicate, Quantity, Value};
    use bevy::prelude::Entity;

    fn test_entity(id: u32) -> Entity {
        Entity::from_bits(id as u64)
    }

    #[test]
    fn record_and_query_belief() {
        let mut tom = TheoryOfMind::default();
        let alice = test_entity(1);

        tom.record_belief(
            alice,
            Node::Concept(Concept::Wolf),
            Predicate::HasTrait,
            Value::Concept(Concept::Dangerous),
            0.8,
            100,
        );

        let confidence = tom.believed_confidence(
            alice,
            &Node::Concept(Concept::Wolf),
            Predicate::HasTrait,
            &Value::Concept(Concept::Dangerous),
        );
        assert!((confidence - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn unknown_agent_returns_zero_confidence() {
        let tom = TheoryOfMind::default();
        let alice = test_entity(1);

        let confidence = tom.believed_confidence(
            alice,
            &Node::Concept(Concept::Wolf),
            Predicate::HasTrait,
            &Value::Concept(Concept::Dangerous),
        );
        assert!((confidence - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn duplicate_belief_updates_confidence_upward() {
        let mut tom = TheoryOfMind::default();
        let alice = test_entity(1);

        tom.record_belief(
            alice,
            Node::Concept(Concept::Wolf),
            Predicate::HasTrait,
            Value::Concept(Concept::Dangerous),
            0.5,
            100,
        );
        tom.record_belief(
            alice,
            Node::Concept(Concept::Wolf),
            Predicate::HasTrait,
            Value::Concept(Concept::Dangerous),
            0.9,
            200,
        );

        let confidence = tom.believed_confidence(
            alice,
            &Node::Concept(Concept::Wolf),
            Predicate::HasTrait,
            &Value::Concept(Concept::Dangerous),
        );
        assert!((confidence - 0.9).abs() < f32::EPSILON);
        assert_eq!(tom.belief_count_for(alice), 1);
    }

    #[test]
    fn evicts_oldest_when_at_capacity() {
        let mut tom = TheoryOfMind::default();
        let alice = test_entity(1);

        // Fill to capacity
        for i in 0..MAX_TRIPLES_PER_AGENT {
            tom.record_belief(
                alice,
                Node::Tile((i as i32, 0)),
                Predicate::Explored,
                Value::Quantity(Quantity::Exact(1.0)),
                0.5,
                i as u64,
            );
        }
        assert_eq!(tom.belief_count_for(alice), MAX_TRIPLES_PER_AGENT);

        // Add one more — should evict the oldest (timestamp=0)
        tom.record_belief(
            alice,
            Node::Tile((999, 999)),
            Predicate::Explored,
            Value::Quantity(Quantity::Exact(1.0)),
            0.5,
            999,
        );
        assert_eq!(tom.belief_count_for(alice), MAX_TRIPLES_PER_AGENT);

        // The oldest (timestamp=0, tile (0,0)) should be gone
        let confidence = tom.believed_confidence(
            alice,
            &Node::Tile((0, 0)),
            Predicate::Explored,
            &Value::Quantity(Quantity::Exact(1.0)),
        );
        assert!((confidence - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn believes_target_knows_danger() {
        let mut tom = TheoryOfMind::default();
        let alice = test_entity(1);

        tom.record_belief(
            alice,
            Node::Concept(Concept::Wolf),
            Predicate::HasTrait,
            Value::Concept(Concept::Dangerous),
            0.8,
            100,
        );

        assert!(tom.believes_target_knows_danger(alice, &Node::Concept(Concept::Wolf), 0.5));
        assert!(!tom.believes_target_knows_danger(alice, &Node::Concept(Concept::Wolf), 0.9));
    }

    #[test]
    fn record_shared_triples_batch() {
        use crate::agent::mind::knowledge::Metadata;

        let mut tom = TheoryOfMind::default();
        let alice = test_entity(1);

        let triples = vec![
            Triple::with_meta(
                Node::Concept(Concept::Wolf),
                Predicate::HasTrait,
                Value::Concept(Concept::Dangerous),
                Metadata::experience(100),
            ),
            Triple::with_meta(
                Node::Concept(Concept::Apple),
                Predicate::IsA,
                Value::Concept(Concept::Food),
                Metadata::experience(100),
            ),
        ];

        tom.record_shared_triples(alice, &triples, 0.8, 100);
        assert_eq!(tom.belief_count_for(alice), 2);
    }
}
