use bevy::prelude::*;
use std::collections::VecDeque;

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
    use crate::agent::actions::ActionType;
    use crate::agent::mind::knowledge::{Concept, Metadata, Node, Predicate, Triple, Value};

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
                    // Map Action -> Concept
                    let mut concepts = vec![];
                    match action {
                        ActionType::Wave | ActionType::Talk => concepts.push(Concept::SocialAction),
                        ActionType::Attack | ActionType::Flee => {
                            concepts.push(Concept::ViolentAction)
                        }
                        ActionType::Eat | ActionType::Sleep | ActionType::Drink => {
                            concepts.push(Concept::SurvivalAction)
                        }
                        ActionType::Walk | ActionType::Wander => {
                            concepts.push(Concept::MovementAction)
                        }
                        _ => {}
                    }

                    // Determine Emotional Impact
                    // TODO: Query ontology instead of hardcoding (see docs/todo.md)
                    let felt = if concepts.contains(&Concept::ViolentAction) {
                        Some((crate::agent::psyche::emotions::EmotionType::Fear, 0.8)) // emotion + intensity
                    } else if concepts.contains(&Concept::SocialAction) {
                        Some((crate::agent::psyche::emotions::EmotionType::Joy, 0.5))
                    } else {
                        None
                    };

                    // ═══════════════════════════════════════════════════════════
                    // SELECTIVE RECORDING: Only record emotionally significant events
                    // Movement, eating, etc. don't create episodic memories
                    // ═══════════════════════════════════════════════════════════
                    if felt.is_none() {
                        continue; // Skip non-emotional events
                    }

                    let (emotion, intensity) = felt.unwrap();

                    // Calculate Importance based on involvement
                    let is_self = *actor == entity || *target == Some(entity);
                    let importance = if is_self { 1.0 } else { 0.5 };

                    // Salience = emotional intensity * importance
                    // High salience memories decay slower
                    let salience = intensity * importance;

                    // ─── KNOWLEDGE UPDATE (Episodic Event) ───
                    // Unique ID: Timestamp + Actor Index + Target Index
                    let target_idx = target.map_or(0, |t| t.index());
                    let event_id = item.timestamp + (*actor).index() as u64 + target_idx as u64;

                    // Use Episodic memory type with salience for proper decay
                    let meta = Metadata {
                        source: crate::agent::mind::knowledge::Source::Experienced,
                        memory_type: MemoryType::Episodic,
                        timestamp: item.timestamp,
                        confidence: 1.0,
                        informant: None,
                        evidence: Vec::new(),
                        salience,
                    };

                    // (Event, Actor, ActorEntity)
                    mind.assert(Triple::with_meta(
                        Node::Event(event_id),
                        Predicate::Actor,
                        Value::Entity(*actor),
                        meta.clone(),
                    ));

                    // (Event, Action, ActionType)
                    mind.assert(Triple::with_meta(
                        Node::Event(event_id),
                        Predicate::Action,
                        Value::Action(*action),
                        meta.clone(),
                    ));

                    if let Some(t) = target {
                        // (Event, Target, TargetEntity)
                        mind.assert(Triple::with_meta(
                            Node::Event(event_id),
                            Predicate::Target,
                            Value::Entity(*t),
                            meta.clone(),
                        ));
                    }

                    // (Event, Timestamp, Time)
                    mind.assert(Triple::with_meta(
                        Node::Event(event_id),
                        Predicate::Timestamp,
                        Value::Int(item.timestamp as i32),
                        meta.clone(),
                    ));

                    // (Event, FeltEmotion, Emotion)
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

                // Social interactions are emotionally significant and stored
                crate::agent::events::GameEvent::SocialInteraction {
                    actor,
                    target,
                    action,
                    valence,
                    ..
                } => {
                    let is_self = *actor == entity || *target == entity;
                    if !is_self {
                        continue; // Only participants remember social interactions
                    }

                    // Determine emotion based on valence
                    let (emotion, intensity) = if *valence > 0.0 {
                        (crate::agent::psyche::emotions::EmotionType::Joy, *valence)
                    } else {
                        (
                            crate::agent::psyche::emotions::EmotionType::Sadness,
                            valence.abs(),
                        )
                    };

                    let salience = intensity;

                    let target_idx = (*target).index();
                    let event_id = item.timestamp + (*actor).index() as u64 + target_idx as u64;

                    let meta = Metadata {
                        source: crate::agent::mind::knowledge::Source::Experienced,
                        memory_type: MemoryType::Episodic,
                        timestamp: item.timestamp,
                        confidence: 1.0,
                        informant: None,
                        evidence: Vec::new(),
                        salience,
                    };

                    // Store basic event structure
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

                // Knowledge shared via conversation - learn what speaker told us
                crate::agent::events::GameEvent::KnowledgeShared {
                    speaker,
                    listener,
                    content,
                } => {
                    // Only process if we are the listener
                    if entity != *listener {
                        continue;
                    }

                    // Add each shared triple to our mind as hearsay
                    for triple in content {
                        let hearsay_meta = Metadata {
                            source: crate::agent::mind::knowledge::Source::Hearsay,
                            memory_type: MemoryType::Semantic, // Facts learned from others
                            timestamp: item.timestamp,
                            confidence: 0.7, // Not as confident as direct experience
                            informant: Some(*speaker),
                            evidence: Vec::new(),
                            salience: 0.5,
                        };

                        // Clone the triple with new metadata
                        mind.assert(Triple::with_meta(
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
            }
        }
    }
}

/// Decay stale knowledge using exponential decay based on memory type and salience.
///
/// Memory strength decays as: strength = 0.5^(age / half_life)
/// - Perception memories decay fastest (~30s half-life)
/// - Episodic memories decay slower (~5min half-life)
/// - High-salience (emotional) memories decay even slower
/// - Semantic/Cultural memories are essentially permanent
pub fn decay_stale_knowledge(
    mut agents: Query<(Entity, &mut crate::agent::mind::knowledge::MindGraph)>,
    tick: Res<crate::core::TickCount>,
    decay_config: Res<MemoryDecayConfig>,
    mut game_log: ResMut<crate::core::GameLog>,
) {
    let current_time = tick.current;
    let ticks_per_sec = tick.ticks_per_second;

    // Stagger decay across agents to spread load
    for (entity, mut mind) in agents.iter_mut() {
        // Only run decay every N ticks, staggered by entity
        if !(entity.index() as u64 + current_time).is_multiple_of(decay_config.decay_interval) {
            continue;
        }

        let initial_count = mind.triples.len();
        let mut decayed_count = 0;

        mind.triples.retain(|triple| {
            let age_ticks = current_time.saturating_sub(triple.meta.timestamp);
            let age_seconds = age_ticks as f32 / ticks_per_sec;

            // Check if this memory should be forgotten
            if decay_config.should_forget(
                age_seconds,
                triple.meta.memory_type,
                triple.meta.salience,
            ) {
                decayed_count += 1;
                return false; // Forget this triple
            }

            // Special case: Empty containers should decay faster
            // (allows "optimistic fallback" - maybe it grew back?)
            if triple.predicate == crate::agent::mind::knowledge::Predicate::Contains
                && let crate::agent::mind::knowledge::Value::Item(_, 0) = triple.object
            {
                // Empty container beliefs decay after ~12 seconds regardless of memory type
                let empty_decay_threshold = 12.0;
                if age_seconds > empty_decay_threshold {
                    decayed_count += 1;
                    return false;
                }
            }

            true // Keep this triple
        });

        // Rebuild indices if we removed anything
        if decayed_count > 0 {
            mind.rebuild_indices();

            // Log significant decay events
            if decayed_count > 10 {
                game_log.log_debug(format!(
                    "Memory decay: {} forgot {} triples ({} -> {})",
                    entity.index(),
                    decayed_count,
                    initial_count,
                    mind.triples.len()
                ));
            }
        }
    }
}

// =============================================================================
// MEMORY DECAY CONFIG
// =============================================================================

/// Configuration for memory decay half-lives (in seconds)
#[derive(Resource, Debug, Clone, Reflect)]
#[reflect(Resource)]
pub struct MemoryDecayConfig {
    pub perception_half_life: f32,
    pub episodic_half_life: f32,
    pub semantic_half_life: f32,
    pub salience_multiplier: f32,
    pub forget_threshold: f32,
    pub decay_interval: u64,
}

impl Default for MemoryDecayConfig {
    fn default() -> Self {
        Self {
            perception_half_life: 30.0,
            episodic_half_life: 300.0,
            semantic_half_life: 36000.0, // 10 hours
            salience_multiplier: 2.0,
            forget_threshold: 0.1,
            decay_interval: 60,
        }
    }
}

impl MemoryDecayConfig {
    pub fn half_life_for(&self, memory_type: MemoryType) -> f32 {
        match memory_type {
            MemoryType::Perception => self.perception_half_life,
            MemoryType::Episodic => self.episodic_half_life,
            MemoryType::Semantic => self.semantic_half_life,
            MemoryType::Intrinsic => f32::INFINITY,
            MemoryType::Cultural => self.semantic_half_life * 2.0,
            MemoryType::Procedural => f32::INFINITY,
        }
    }

    pub fn should_forget(&self, age_seconds: f32, memory_type: MemoryType, salience: f32) -> bool {
        let base_half_life = self.half_life_for(memory_type);
        if base_half_life.is_infinite() {
            return false;
        }

        let adjusted_half_life = base_half_life * (1.0 + salience * self.salience_multiplier);
        let strength = 0.5_f32.powf(age_seconds / adjusted_half_life);

        strength < self.forget_threshold
    }
}
