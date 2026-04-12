//! Working memory buffer and strength-based knowledge decay.
//!
//! Reads: GameEvent messages, TickCount, MemoryDecayConfig, MindGraph
//! Writes: WorkingMemory (event buffer), MindGraph (episodic triples, decay/forget)
//! Upstream: perception (GameEvents), action execution (GameEvents), conversation (KnowledgeShared)
//! Downstream: consolidation (reads episodic triples), brain systems (reads MindGraph)

use bevy::prelude::*;
use std::collections::VecDeque;

use crate::agent::actions::ActionType;
use crate::agent::mind::knowledge::MemoryType;

// WorkingMemory structs
#[derive(Component, Debug, Clone, Reflect, Default)]
#[reflect(Component)]
pub struct WorkingMemory {
    pub buffer: VecDeque<WorkingMemoryItem>,
}

#[derive(Debug, Clone, Reflect)]
pub struct WorkingMemoryItem {
    pub event: crate::agent::events::GameEvent,
    pub timestamp: u64,
    pub processed: bool,
}

pub fn process_perception(
    mut events: MessageReader<crate::agent::events::GameEvent>,
    mut agents: Query<(
        Entity,
        &Transform,
        &crate::agent::mind::perception::Vision,
        &mut WorkingMemory,
    )>,
    transforms: Query<&Transform>, // To look up actor positions
    current_tick: Res<crate::core::TickCount>,
) {
    for event in events.read() {
        match event {
            crate::agent::events::GameEvent::Interaction { actor, target, .. } => {
                if let Ok(actor_transform) = transforms.get(*actor) {
                    let actor_pos = actor_transform.translation.truncate();

                    for (observer_entity, observer_transform, vision, mut wm) in agents.iter_mut() {
                        let observer_pos = observer_transform.translation.truncate();
                        let distance = observer_pos.distance(actor_pos);

                        let is_actor = observer_entity == *actor;
                        let is_target = target.is_some_and(|t| t == observer_entity);
                        let is_witness = distance <= vision.range;

                        if is_actor || is_target || is_witness {
                            // Add to Working Memory
                            wm.buffer.push_back(WorkingMemoryItem {
                                event: event.clone(),
                                timestamp: current_tick.current,
                                processed: false,
                            });

                            // Limit WM size (keep recent 20, even if processed, for UI)
                            if wm.buffer.len() > 20 {
                                wm.buffer.pop_front();
                            }
                        }
                    }
                }
            }

            // Social interactions are also stored in memory
            crate::agent::events::GameEvent::SocialInteraction { actor, target, .. } => {
                // Both participants remember the interaction
                for (observer_entity, _observer_transform, _vision, mut wm) in agents.iter_mut() {
                    if observer_entity == *actor || observer_entity == *target {
                        wm.buffer.push_back(WorkingMemoryItem {
                            event: event.clone(),
                            timestamp: current_tick.current,
                            processed: false,
                        });

                        if wm.buffer.len() > 20 {
                            wm.buffer.pop_front();
                        }
                    }
                }
            }

            // Knowledge sharing - listener receives triples
            crate::agent::events::GameEvent::KnowledgeShared { listener, .. } => {
                for (observer_entity, _observer_transform, _vision, mut wm) in agents.iter_mut() {
                    if observer_entity == *listener {
                        wm.buffer.push_back(WorkingMemoryItem {
                            event: event.clone(),
                            timestamp: current_tick.current,
                            processed: false,
                        });

                        if wm.buffer.len() > 20 {
                            wm.buffer.pop_front();
                        }
                    }
                }
            }
        }
    }
}

pub fn process_working_memory(
    mut query: Query<
        (
            Entity,
            &mut WorkingMemory,
            &mut crate::agent::mind::knowledge::MindGraph,
        ),
        With<crate::agent::Agent>,
    >,
    mut game_log: ResMut<crate::core::GameLog>,
) {
    for (entity, mut wm, mut mind) in query.iter_mut() {
        for item in wm.buffer.iter_mut() {
            if item.processed {
                continue;
            }
            item.processed = true;

            match &item.event {
                crate::agent::events::GameEvent::Interaction {
                    actor,
                    action,
                    target,
                    ..
                } => {
                    record_interaction_event(
                        entity,
                        item,
                        actor,
                        action,
                        target,
                        &mut mind,
                        &mut game_log,
                    );
                }

                crate::agent::events::GameEvent::SocialInteraction {
                    actor,
                    target,
                    action,
                    valence,
                    ..
                } => {
                    record_social_interaction(
                        entity, item, actor, target, action, *valence, &mut mind,
                    );
                }

                crate::agent::events::GameEvent::KnowledgeShared {
                    speaker,
                    listener,
                    content,
                } => {
                    record_knowledge_shared(
                        entity,
                        item,
                        speaker,
                        listener,
                        content,
                        &mut mind,
                        &mut game_log,
                    );
                }
            }
        }
    }
}

fn record_interaction_event(
    entity: Entity,
    item: &WorkingMemoryItem,
    actor: &Entity,
    action: &ActionType,
    target: &Option<Entity>,
    mind: &mut crate::agent::mind::knowledge::MindGraph,
    game_log: &mut crate::core::GameLog,
) {
    use crate::agent::mind::knowledge::{Concept, Metadata, Node, Predicate, Triple, Value};

    let mut concepts = vec![];
    match action {
        ActionType::Wave | ActionType::Converse => concepts.push(Concept::SocialAction),
        ActionType::Attack | ActionType::Flee => concepts.push(Concept::ViolentAction),
        ActionType::Eat | ActionType::Sleep | ActionType::Drink => {
            concepts.push(Concept::SurvivalAction)
        }
        ActionType::Walk | ActionType::Wander => concepts.push(Concept::MovementAction),
        _ => {}
    }

    // Only record emotionally significant events — movement, eating, etc. don't create episodic memories
    // TODO: Query ontology instead of hardcoding (see docs/todo.md)
    let (emotion, intensity) = if concepts.contains(&Concept::ViolentAction) {
        (crate::agent::psyche::emotions::EmotionType::Fear, 0.8)
    } else if concepts.contains(&Concept::SocialAction) {
        (crate::agent::psyche::emotions::EmotionType::Joy, 0.5)
    } else {
        return;
    };

    let is_self = *actor == entity || *target == Some(entity);
    let importance = if is_self { 1.0 } else { 0.5 };
    let salience = intensity * importance;

    let target_idx = target.map_or(0, |t| t.index_u32());
    let event_id = item.timestamp + (*actor).index_u32() as u64 + target_idx as u64;

    let meta = Metadata {
        source: crate::agent::mind::knowledge::Source::Experienced,
        memory_type: MemoryType::Episodic,
        timestamp: item.timestamp,
        confidence: 1.0,
        informant: None,
        evidence: Vec::new(),
        salience,
        source_sense: None,
        strength: 1.0,
    };

    mind.assert(Triple::with_meta(
        Node::Event(event_id),
        Predicate::Actor,
        Value::Entity(*actor),
        meta.clone(),
    ));
    mind.assert(Triple::with_meta(
        Node::Event(event_id),
        Predicate::Action,
        Value::Action(*action),
        meta.clone(),
    ));

    if let Some(t) = target {
        mind.assert(Triple::with_meta(
            Node::Event(event_id),
            Predicate::Target,
            Value::Entity(*t),
            meta.clone(),
        ));
    }

    mind.assert(Triple::with_meta(
        Node::Event(event_id),
        Predicate::Timestamp,
        Value::Int(item.timestamp as i32),
        meta.clone(),
    ));
    mind.assert(Triple::with_meta(
        Node::Event(event_id),
        Predicate::FeltEmotion,
        Value::Emotion(emotion, intensity),
        meta.clone(),
    ));

    if is_self {
        game_log.perception(
            &format!("{:?}", entity),
            &format!("observed: {}", action),
            Some(entity),
        );
    }
}

fn record_social_interaction(
    entity: Entity,
    item: &WorkingMemoryItem,
    actor: &Entity,
    target: &Entity,
    action: &ActionType,
    valence: f32,
    mind: &mut crate::agent::mind::knowledge::MindGraph,
) {
    use crate::agent::mind::knowledge::{Metadata, Node, Predicate, Triple, Value};

    if *actor != entity && *target != entity {
        return;
    }

    let (emotion, intensity) = if valence > 0.0 {
        (crate::agent::psyche::emotions::EmotionType::Joy, valence)
    } else {
        (
            crate::agent::psyche::emotions::EmotionType::Sadness,
            valence.abs(),
        )
    };

    let event_id = item.timestamp + (*actor).index_u32() as u64 + (*target).index_u32() as u64;

    let meta = Metadata {
        source: crate::agent::mind::knowledge::Source::Experienced,
        memory_type: MemoryType::Episodic,
        timestamp: item.timestamp,
        confidence: 1.0,
        informant: None,
        evidence: Vec::new(),
        salience: intensity,
        source_sense: None,
        strength: 1.0,
    };

    mind.assert(Triple::with_meta(
        Node::Event(event_id),
        Predicate::Actor,
        Value::Entity(*actor),
        meta.clone(),
    ));
    mind.assert(Triple::with_meta(
        Node::Event(event_id),
        Predicate::Action,
        Value::Action(*action),
        meta.clone(),
    ));
    mind.assert(Triple::with_meta(
        Node::Event(event_id),
        Predicate::Target,
        Value::Entity(*target),
        meta.clone(),
    ));
    mind.assert(Triple::with_meta(
        Node::Event(event_id),
        Predicate::FeltEmotion,
        Value::Emotion(emotion, intensity),
        meta.clone(),
    ));
}

fn record_knowledge_shared(
    entity: Entity,
    item: &WorkingMemoryItem,
    speaker: &Entity,
    listener: &Entity,
    content: &[crate::agent::mind::knowledge::Triple],
    mind: &mut crate::agent::mind::knowledge::MindGraph,
    game_log: &mut crate::core::GameLog,
) {
    use crate::agent::mind::knowledge::Metadata;

    if entity != *listener {
        return;
    }

    for triple in content {
        let hearsay_meta = Metadata {
            source: crate::agent::mind::knowledge::Source::Hearsay,
            memory_type: MemoryType::Semantic,
            timestamp: item.timestamp,
            confidence: 0.7,
            informant: Some(*speaker),
            evidence: Vec::new(),
            salience: 0.5,
            source_sense: None,
            strength: 0.7,
        };

        mind.assert(crate::agent::mind::knowledge::Triple::with_meta(
            triple.subject.clone(),
            triple.predicate,
            triple.object.clone(),
            hearsay_meta,
        ));
    }

    game_log.log_debug(format!(
        "{:?} learned {} facts from {:?}",
        listener,
        content.len(),
        speaker
    ));
}

/// Strength-based memory decay with reinforcement and interference.
///
/// Each triple carries a `strength` float that is:
/// - **Reinforced** by repeated perception/assertion (in `MindGraph::assert`)
/// - **Passively decayed** each tick: `strength *= base^(1 / (strength * salience_resist))`
/// - **Interfered** by competing same-predicate triples: weak memories in crowded
///   categories decay faster
/// - **Forgotten** when strength drops below `forget_threshold`
///
/// Intrinsic, Cultural, and Procedural memories never decay.
pub fn decay_stale_knowledge(
    mut agents: Query<
        (Entity, &mut crate::agent::mind::knowledge::MindGraph),
        With<crate::agent::Alive>,
    >,
    tick: Res<crate::core::TickCount>,
    decay_config: Res<MemoryDecayConfig>,
    mut game_log: ResMut<crate::core::GameLog>,
) {
    let current_time = tick.current;

    for (entity, mut mind) in agents.iter_mut() {
        if !(entity.index_u32() as u64 + current_time).is_multiple_of(decay_config.decay_interval) {
            continue;
        }

        let initial_count = mind.len();

        // Precompute per-predicate interference pressure and per-type ln(base)
        // so the inner loop avoids redundant ln_1p and powf calls.
        let pred_pressure = decay_config.precompute_interference(&mind.predicate_count_map());
        let ln_perception = decay_config.perception_decay.ln();
        let ln_episodic = decay_config.episodic_decay.ln();
        let ln_semantic = decay_config.semantic_decay.ln();

        let decayed_count = mind.decay_pass(|triple| {
            let ln_base = match triple.meta.memory_type {
                MemoryType::Perception => ln_perception,
                MemoryType::Episodic => ln_episodic,
                MemoryType::Semantic => ln_semantic,
                _ => return true, // Permanent memory type
            };

            // Passive decay: exp(ln_base / (strength * salience_resist))
            // High strength and salience slow the effective rate.
            let salience_resist =
                1.0 + triple.meta.salience * decay_config.salience_decay_resistance;
            let effective_rate =
                (ln_base / (triple.meta.strength.max(1.0) * salience_resist)).exp();
            triple.meta.strength *= effective_rate;

            // Interference: more same-predicate triples → faster loss for weak memories.
            if let Some(&pressure) = pred_pressure.get(&triple.predicate) {
                let vulnerability = 1.0 / (1.0 + triple.meta.strength * 2.0);
                triple.meta.strength -= pressure * vulnerability;
            }

            triple.meta.strength = triple.meta.strength.max(0.0);
            triple.meta.strength >= decay_config.forget_threshold
        });

        // Episodic capacity cap: cull weakest events when over limit
        if decay_config.episodic_capacity > 0 {
            enforce_episodic_capacity(&mut mind, decay_config.episodic_capacity);
        }

        if decayed_count > 0 {
            if mind.tombstone_count() * 2 > mind.total_slots() {
                mind.compact();
            }

            if decayed_count > 10 {
                game_log.log_debug(format!(
                    "Memory decay: {} forgot {} triples ({} -> {})",
                    entity.index(),
                    decayed_count,
                    initial_count,
                    mind.len()
                ));
            }
        }
    }
}

/// Remove the weakest episodic events when the total event count exceeds capacity.
/// An "event" is a group of triples sharing the same `Node::Event(eid)` subject.
fn enforce_episodic_capacity(mind: &mut crate::agent::mind::knowledge::MindGraph, capacity: usize) {
    use crate::agent::mind::knowledge::Node;
    use std::collections::{HashMap, HashSet};

    let mut event_strengths: HashMap<u64, f32> = HashMap::new();
    for triple in mind.iter() {
        if let Node::Event(eid) = triple.subject {
            let entry = event_strengths.entry(eid).or_insert(0.0_f32);
            *entry = entry.max(triple.meta.strength);
        }
    }

    if event_strengths.len() <= capacity {
        return;
    }

    let mut events: Vec<(u64, f32)> = event_strengths.into_iter().collect();
    events.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let to_remove = events.len() - capacity;
    let eids_to_remove: HashSet<u64> = events[..to_remove].iter().map(|(eid, _)| *eid).collect();

    mind.retain(|triple| {
        if let Node::Event(eid) = triple.subject {
            !eids_to_remove.contains(&eid)
        } else {
            true
        }
    });
}

// =============================================================================
// MEMORY DECAY CONFIG
// =============================================================================

/// Configuration for the reinforcement + interference memory model.
///
/// Decay rates are per-pass multipliers (one pass per `decay_interval` ticks).
/// Values closer to 1.0 mean slower decay.
#[derive(Resource, Debug, Clone, Reflect)]
#[reflect(Resource)]
pub struct MemoryDecayConfig {
    pub perception_decay: f32,
    pub episodic_decay: f32,
    pub semantic_decay: f32,
    /// Multiplier in the exponent denominator: higher salience → slower decay.
    pub salience_decay_resistance: f32,
    /// Denominator for interference pressure: higher → less interference.
    pub interference_divisor: f32,
    pub forget_threshold: f32,
    pub episodic_capacity: usize,
    pub decay_interval: u64,
}

impl Default for MemoryDecayConfig {
    fn default() -> Self {
        Self {
            perception_decay: 0.95,
            episodic_decay: 0.997,
            semantic_decay: 0.9998,
            salience_decay_resistance: 2.0,
            interference_divisor: 30.0,
            forget_threshold: 0.05,
            episodic_capacity: 200,
            decay_interval: 60,
        }
    }
}

impl MemoryDecayConfig {
    pub fn base_decay(&self, memory_type: MemoryType) -> f32 {
        match memory_type {
            MemoryType::Perception => self.perception_decay,
            MemoryType::Episodic => self.episodic_decay,
            MemoryType::Semantic => self.semantic_decay,
            MemoryType::Intrinsic | MemoryType::Procedural | MemoryType::Cultural => 1.0,
        }
    }

    /// Precompute interference pressure per predicate from a count snapshot.
    /// Only includes predicates with count > 1 (no interference from a lone triple).
    pub fn precompute_interference(
        &self,
        pred_counts: &std::collections::HashMap<crate::agent::mind::knowledge::Predicate, usize>,
    ) -> std::collections::HashMap<crate::agent::mind::knowledge::Predicate, f32> {
        pred_counts
            .iter()
            .filter(|(_, count)| **count > 1)
            .map(|(&pred, &count)| {
                let pressure = (count as f32 / self.interference_divisor).ln_1p() * 0.01;
                (pred, pressure)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mind::knowledge::{
        Concept, Metadata, MindGraph, Node, Predicate, Triple, Value,
    };

    fn perception_triple(entity_id: u64, tile: (i32, i32), strength: f32) -> Triple {
        let mut meta = Metadata::perception(0);
        meta.strength = strength;
        Triple::with_meta(
            Node::Entity(bevy::prelude::Entity::from_bits(entity_id)),
            Predicate::LocatedAt,
            Value::Tile(tile),
            meta,
        )
    }

    fn episodic_event_triples(eid: u64, strength: f32) -> Vec<Triple> {
        let meta = Metadata {
            source: crate::agent::mind::knowledge::Source::Experienced,
            memory_type: MemoryType::Episodic,
            timestamp: eid,
            confidence: 1.0,
            informant: None,
            evidence: Vec::new(),
            salience: 0.5,
            source_sense: None,
            strength,
        };
        vec![
            Triple::with_meta(
                Node::Event(eid),
                Predicate::Actor,
                Value::Entity(bevy::prelude::Entity::from_bits(1)),
                meta.clone(),
            ),
            Triple::with_meta(
                Node::Event(eid),
                Predicate::Action,
                Value::Action(crate::agent::actions::ActionType::Attack),
                meta.clone(),
            ),
            Triple::with_meta(
                Node::Event(eid),
                Predicate::FeltEmotion,
                Value::Emotion(crate::agent::psyche::emotions::EmotionType::Fear, 0.8),
                meta,
            ),
        ]
    }

    /// Run the same decay formula used by `decay_stale_knowledge`, minus ECS wiring.
    fn run_decay_pass(mind: &mut MindGraph, config: &MemoryDecayConfig) -> usize {
        let pred_pressure = config.precompute_interference(&mind.predicate_count_map());
        let ln_perception = config.perception_decay.ln();
        let ln_episodic = config.episodic_decay.ln();
        let ln_semantic = config.semantic_decay.ln();

        mind.decay_pass(|triple| {
            let ln_base = match triple.meta.memory_type {
                MemoryType::Perception => ln_perception,
                MemoryType::Episodic => ln_episodic,
                MemoryType::Semantic => ln_semantic,
                _ => return true,
            };
            let resist = 1.0 + triple.meta.salience * config.salience_decay_resistance;
            let rate = (ln_base / (triple.meta.strength.max(1.0) * resist)).exp();
            triple.meta.strength *= rate;

            if let Some(&pressure) = pred_pressure.get(&triple.predicate) {
                let vulnerability = 1.0 / (1.0 + triple.meta.strength * 2.0);
                triple.meta.strength -= pressure * vulnerability;
            }
            triple.meta.strength = triple.meta.strength.max(0.0);
            triple.meta.strength >= config.forget_threshold
        })
    }

    #[test]
    fn decay_pass_reduces_perception_strength() {
        let config = MemoryDecayConfig::default();
        let mut mind = MindGraph::default();
        mind.add(perception_triple(1, (5, 5), 1.0));

        run_decay_pass(&mut mind, &config);

        let s = mind.iter().next().unwrap().meta.strength;
        assert!(
            (s - 0.95).abs() < 0.01,
            "expected ~0.95 after one pass, got {s}"
        );
    }

    #[test]
    fn high_strength_resists_decay() {
        let config = MemoryDecayConfig::default();
        let mut mind = MindGraph::default();
        mind.add(perception_triple(1, (5, 5), 5.0));

        run_decay_pass(&mut mind, &config);

        let s = mind.iter().next().unwrap().meta.strength;
        assert!(s > 4.9, "high-strength triple should barely decay, got {s}");
    }

    #[test]
    fn intrinsic_does_not_decay() {
        let config = MemoryDecayConfig::default();
        let mut mind = MindGraph::default();
        mind.add(Triple::new(
            Node::Concept(Concept::Apple),
            Predicate::IsA,
            Value::Concept(Concept::Food),
        ));

        let removed = run_decay_pass(&mut mind, &config);

        assert_eq!(removed, 0);
        assert!(
            (mind.iter().next().unwrap().meta.strength - 1.0).abs() < f32::EPSILON,
            "intrinsic strength must not change"
        );
    }

    #[test]
    fn cultural_does_not_decay() {
        let config = MemoryDecayConfig::default();
        let mut mind = MindGraph::default();
        let meta = Metadata {
            memory_type: MemoryType::Cultural,
            source: crate::agent::mind::knowledge::Source::Cultural,
            ..Default::default()
        };
        mind.add(Triple::with_meta(
            Node::Concept(Concept::Apple),
            Predicate::IsA,
            Value::Concept(Concept::Food),
            meta,
        ));

        let removed = run_decay_pass(&mut mind, &config);

        assert_eq!(removed, 0);
        assert!(
            (mind.iter().next().unwrap().meta.strength - 1.0).abs() < f32::EPSILON,
            "cultural strength must not change"
        );
    }

    #[test]
    fn interference_penalizes_crowded_predicates() {
        let config = MemoryDecayConfig::default();
        let mut mind_crowded = MindGraph::default();
        let mut mind_sparse = MindGraph::default();

        for i in 1..=50 {
            mind_crowded.add(perception_triple(i, (i as i32, 0), 1.0));
        }
        for i in 1..=2 {
            mind_sparse.add(perception_triple(i, (i as i32, 0), 1.0));
        }

        run_decay_pass(&mut mind_crowded, &config);
        run_decay_pass(&mut mind_sparse, &config);

        let crowded_strength = mind_crowded.iter().next().unwrap().meta.strength;
        let sparse_strength = mind_sparse.iter().next().unwrap().meta.strength;

        assert!(
            crowded_strength < sparse_strength,
            "crowded predicate should decay faster: crowded={crowded_strength} vs sparse={sparse_strength}"
        );
    }

    #[test]
    fn episodic_cap_removes_weakest_events() {
        let mut mind = MindGraph::default();

        for eid in 0..200 {
            for triple in episodic_event_triples(eid, 0.5) {
                mind.add(triple);
            }
        }
        for eid in 200..205 {
            for triple in episodic_event_triples(eid, 5.0) {
                mind.add(triple);
            }
        }

        enforce_episodic_capacity(&mut mind, 200);

        let mut event_ids: std::collections::HashSet<u64> = std::collections::HashSet::new();
        for triple in mind.iter() {
            if let Node::Event(eid) = triple.subject {
                event_ids.insert(eid);
            }
        }

        assert_eq!(event_ids.len(), 200, "should cull down to capacity");
        for eid in 200..205 {
            assert!(
                event_ids.contains(&eid),
                "strong event {eid} should survive cull"
            );
        }
    }

    #[test]
    fn perception_eventually_forgotten_without_reinforcement() {
        let config = MemoryDecayConfig::default();
        let mut mind = MindGraph::default();
        mind.add(perception_triple(1, (5, 5), 1.0));

        for _ in 0..200 {
            run_decay_pass(&mut mind, &config);
        }

        assert!(
            mind.is_empty(),
            "unreinforced perception should be forgotten after enough passes"
        );
    }
}
