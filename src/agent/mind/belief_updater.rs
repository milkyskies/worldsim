use crate::agent::events::{ActionOutcome, ActionOutcomeEvent, FailureReason};
use crate::agent::mind::knowledge::{Concept, Metadata, MindGraph, Node, Predicate, Triple, Value};
use bevy::prelude::*;

/// Processes action outcomes and updates agent beliefs accordingly
pub fn process_action_outcomes(
    mut agents: Query<&mut MindGraph, With<crate::agent::Agent>>,
    mut outcome_events: MessageReader<ActionOutcomeEvent>,
    time: Res<Time>,
) {
    let current_time = time.elapsed().as_millis() as u64;

    for event in outcome_events.read() {
        if let Ok(mut mind) = agents.get_mut(event.actor) {
            match &event.outcome {
                ActionOutcome::Success {
                    target,
                    gained,
                    consumed,
                    ..
                } => {
                    // Update belief about what we gained
                    if let Some((concept, qty)) = gained {
                        // Get current count and add
                        let current = mind.count_of(&Node::Self_, *concept);
                        mind.perceive_self(
                            Predicate::Contains,
                            Value::Item(*concept, current + qty),
                            current_time,
                        );
                    }

                    // Update belief about what we consumed
                    if let Some((concept, qty)) = consumed {
                        let current = mind.count_of(&Node::Self_, *concept);
                        let new_count = current.saturating_sub(*qty);
                        mind.perceive_self(
                            Predicate::Contains,
                            Value::Item(*concept, new_count),
                            current_time,
                        );
                    }

                    // If we took from a target, we know it had resources (but not exact count)
                    if let (Some(target_entity), Some((concept, _))) = (target, gained) {
                        // Don't assume it's empty - just note we took from it
                        // Recoding "HasTrait" is a bit vague, but matches original logic.
                        mind.assert(Triple::with_meta(
                            Node::Entity(*target_entity),
                            Predicate::HasTrait,
                            Value::Concept(*concept), // "This thing has apples (or had)"
                            Metadata::experience(current_time),
                        ));
                    }
                }

                ActionOutcome::Failed { target, reason, .. } => {
                    match reason {
                        FailureReason::ResourceDepleted => {
                            if let Some(target_entity) = target {
                                // Mark it as empty
                                mind.assert(Triple::with_meta(
                                    Node::Entity(*target_entity),
                                    Predicate::Contains,
                                    Value::Item(Concept::Apple, 0),
                                    Metadata::experience(current_time),
                                ));
                            }
                        }
                        FailureReason::MissingItem(concept) => {
                            // We don't have this
                            mind.perceive_self(
                                Predicate::Contains,
                                Value::Item(*concept, 0),
                                current_time,
                            );
                        }
                        FailureReason::TargetGone => {
                            // Target doesn't exist - beliefs will decay
                        }
                        FailureReason::NoEdibleFood => {
                            // We have no food! Update beliefs so planner knows
                            // Assert that we have 0 of the common food types
                            for food_concept in [Concept::Apple, Concept::Berry] {
                                mind.perceive_self(
                                    Predicate::Contains,
                                    Value::Item(food_concept, 0),
                                    current_time,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}
