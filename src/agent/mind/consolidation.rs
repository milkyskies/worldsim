use crate::agent::psyche::emotions::EmotionType;
use crate::agent::mind::knowledge::{
    MemoryType, Metadata, MindGraph, Node, Predicate, Source, Triple, Value,
};
use bevy::prelude::*;
use std::collections::HashMap;

/// System to periodically scan Episodic memories and form Semantic beliefs.
/// This mimics "sleep" or offline processing.
pub fn consolidate_knowledge(
    tick: Res<crate::core::tick::TickCount>,
    mut agents: Query<(Entity, &mut MindGraph), With<crate::agent::Agent>>,
) {
    // Pause is handled by run_if(not_paused) at the plugin level
    let current_time = tick.current;

    for (entity, mut mind) in agents.iter_mut() {
        // Staggered: Run every 30 ticks, offset by entity ID
        if !tick.should_run(entity, 30) {
            continue;
        }
        // We want to find patterns like: "Person X has attacked me N times" -> Hostile

        // Scan all Event triples
        // This is expensive O(N). In the future, use MindGraphIndex::by_memory_type(Episodic).

        let mut social_events: HashMap<Entity, Vec<(u64, f32)>> = HashMap::new(); // Actor -> [(Time, Valence)]

        // Reconstruct events roughly
        // We look for (EventID, Actor, Other) and (EventID, FeltEmotion, E)

        let mut event_actors: HashMap<u64, Entity> = HashMap::new();
        let mut event_valences: HashMap<u64, f32> = HashMap::new();

        for triple in &mind.triples {
            if let Node::Event(eid) = triple.subject {
                match triple.predicate {
                    Predicate::Actor => {
                        if let Value::Entity(actor) = triple.object
                            && actor != entity {
                                // Don't judge self yet
                                event_actors.insert(eid, actor);
                            }
                    }
                    Predicate::FeltEmotion => {
                        if let Value::Emotion(emph, _intensity) = triple.object {
                            let valence = match emph {
                                EmotionType::Joy => 1.0,
                                EmotionType::Surprise => 0.2, // Neutral-ish
                                EmotionType::Sadness => -0.5,
                                EmotionType::Fear => -1.0,
                                EmotionType::Anger => -0.8,
                                EmotionType::Disgust => -0.7,
                            };
                            event_valences.insert(eid, valence);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Correlate Actor and Valence
        for (eid, actor) in event_actors {
            if let Some(&valence) = event_valences.get(&eid) {
                social_events.entry(actor).or_default().push((eid, valence));
            }
        }

        // 2. Form Beliefs from Patterns
        for (subject, events) in social_events {
            // Formula: weight = (0.2 + intensity * 0.8) * (0.3 + recency * 0.7)
            // Simplified here: Valence IS intensity*sign.

            let mut weighted_sum = 0.0;
            let mut total_weight = 0.0;

            // Time half-life for recency (e.g., 5 minutes worth of ticks)
            let half_life = 300.0 * tick.ticks_per_second;

            for (timestamp, valence) in &events {
                let age = (current_time.saturating_sub(*timestamp)) as f32;
                let recency = 0.5f32.powf(age / half_life);

                let intensity = valence.abs();
                let weight = (0.2 + intensity * 0.8) * (0.3 + recency * 0.7);

                weighted_sum += valence * weight;
                total_weight += weight;
            }

            if total_weight > 0.0 {
                let aggregate_valence = weighted_sum / total_weight; // -1.0 to 1.0 relative to weight

                // Confidence increases with Total Weight (more evidence = higher confidence)
                // e.g. 1 event = ~0.5 weight -> low confidence
                // 3 events = ~1.5 weight -> high confidence
                let confidence = (total_weight / 2.0).clamp(0.0, 1.0);

                // Thresholds for belief formation
                if confidence > 0.4 {
                    if aggregate_valence < -0.3 {
                        // Form Hostile Belief
                        mind.assert(Triple::with_meta(
                            Node::Entity(subject),
                            Predicate::HasTrait,
                            Value::Concept(crate::agent::mind::knowledge::Concept::Hostile),
                            Metadata {
                                source: Source::Inferred,
                                memory_type: MemoryType::Semantic,
                                timestamp: current_time,
                                confidence,
                                informant: None,
                                evidence: events.iter().map(|(id, _)| *id).collect(),
                                salience: confidence, // High confidence beliefs are salient
                            },
                        ));
                    } else if aggregate_valence > 0.3 {
                        // Form Friendly Belief
                        mind.assert(Triple::with_meta(
                            Node::Entity(subject),
                            Predicate::HasTrait,
                            Value::Concept(crate::agent::mind::knowledge::Concept::Friendly),
                            Metadata {
                                source: Source::Inferred,
                                memory_type: MemoryType::Semantic,
                                timestamp: current_time,
                                confidence,
                                informant: None,
                                evidence: events.iter().map(|(id, _)| *id).collect(),
                                salience: confidence,
                            },
                        ));
                    }
                }
            }
        }
    }
}
