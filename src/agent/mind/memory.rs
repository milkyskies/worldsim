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
    mut agents: Query<(Entity, &mut crate::agent::mind::knowledge::MindGraph)>,
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

        // Snapshot predicate counts for interference (O(P) where P = distinct predicates)
        let pred_counts = mind.predicate_count_map();

        let decayed_count = mind.decay_pass(|triple| {
            let base = decay_config.base_decay(triple.meta.memory_type);
            if base >= 1.0 {
                return true; // Permanent memory type
            }

            // Passive decay: high strength and salience slow the effective rate.
            // At strength 1.0 with no salience the full base rate applies.
            // At strength 10.0 the exponent drops to 0.1, making decay ~10x slower.
            let salience_resist =
                1.0 + triple.meta.salience * decay_config.salience_decay_resistance;
            let effective_rate = base.powf(1.0 / (triple.meta.strength.max(1.0) * salience_resist));
            triple.meta.strength *= effective_rate;

            // Interference: more same-predicate triples → faster loss for weak memories.
            // Strong memories resist interference (vulnerability → 0 as strength grows).
            let pred_count = pred_counts.get(&triple.predicate).copied().unwrap_or(0);
            if pred_count > 1 {
                let pressure =
                    (pred_count as f32 / decay_config.interference_divisor).ln_1p() * 0.01;
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
fn enforce_episodic_capacity(
    mind: &mut crate::agent::mind::knowledge::MindGraph,
    capacity: usize,
) {
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
    /// Strength multiplier per decay pass for Perception triples.
    pub perception_decay: f32,
    /// Strength multiplier per decay pass for Episodic triples.
    pub episodic_decay: f32,
    /// Strength multiplier per decay pass for Semantic triples.
    pub semantic_decay: f32,
    /// How much salience resists decay (multiplier in denominator of exponent).
    pub salience_decay_resistance: f32,
    /// Divisor for interference pressure: higher = less interference.
    pub interference_divisor: f32,
    /// Strength threshold below which a triple is forgotten.
    pub forget_threshold: f32,
    /// Maximum distinct episodic events before weakest are culled.
    pub episodic_capacity: usize,
    /// Ticks between decay passes (staggered per agent).
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
    /// Base decay rate for a memory type. Returns 1.0 (no decay) for permanent types.
    pub fn base_decay(&self, memory_type: MemoryType) -> f32 {
        match memory_type {
            MemoryType::Perception => self.perception_decay,
            MemoryType::Episodic => self.episodic_decay,
            MemoryType::Semantic => self.semantic_decay,
            MemoryType::Intrinsic | MemoryType::Procedural | MemoryType::Cultural => 1.0,
        }
    }
}
