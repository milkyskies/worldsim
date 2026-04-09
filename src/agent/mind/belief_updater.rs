//! Belief updater: processes action outcome events and updates MindGraph triples to reflect results.
//!
//! Reads: ActionOutcomeEvent (success/failure, gained/consumed items, targets), Time
//! Writes: MindGraph (inventory counts, resource depletion, location beliefs)
//! Upstream: agent::events (ActionOutcomeEvent emitted by action execution systems)
//! Downstream: mind::knowledge (MindGraph updated), brains (read updated beliefs next tick)

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
                } => handle_success_outcome(&mut mind, target, gained, consumed, current_time),

                ActionOutcome::Failed { target, reason, .. } => {
                    handle_failure_outcome(&mut mind, target, reason, current_time)
                }
            }
        }
    }
}

fn handle_success_outcome(
    mind: &mut MindGraph,
    target: &Option<Entity>,
    gained: &Option<(Concept, u32)>,
    consumed: &Option<(Concept, u32)>,
    current_time: u64,
) {
    if let Some((concept, qty)) = gained {
        let current = mind.count_of(&Node::Self_, *concept);
        mind.perceive_self(
            Predicate::Contains,
            Value::Item(*concept, current + qty),
            current_time,
        );
    }

    if let Some((concept, qty)) = consumed {
        let current = mind.count_of(&Node::Self_, *concept);
        let new_count = current.saturating_sub(*qty);
        mind.perceive_self(
            Predicate::Contains,
            Value::Item(*concept, new_count),
            current_time,
        );
    }

    // Note that the target had resources (don't assume it's now empty)
    if let (Some(target_entity), Some((concept, _))) = (target, gained) {
        mind.assert(Triple::with_meta(
            Node::Entity(*target_entity),
            Predicate::HasTrait,
            Value::Concept(*concept),
            Metadata::experience(current_time),
        ));
    }
}

fn handle_failure_outcome(
    mind: &mut MindGraph,
    target: &Option<Entity>,
    reason: &FailureReason,
    current_time: u64,
) {
    match reason {
        FailureReason::ResourceDepleted => {
            if let Some(target_entity) = target {
                mind.assert(Triple::with_meta(
                    Node::Entity(*target_entity),
                    Predicate::Contains,
                    Value::Item(Concept::Apple, 0),
                    Metadata::experience(current_time),
                ));
            }
        }
        FailureReason::MissingItem(concept) => {
            mind.perceive_self(Predicate::Contains, Value::Item(*concept, 0), current_time);
        }
        FailureReason::TargetGone => {}
        FailureReason::NoEdibleFood => {
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
