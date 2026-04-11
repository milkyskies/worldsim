//! Belief updater: updates MindGraph from action outcomes; generates need-satisfaction emotions.
//!
//! Reads: ActionOutcomeEvent (success/failure, need satisfaction, items, targets), Time, PhysicalNeeds
//! Writes: MindGraph (inventory counts, resource depletion), EmotionalState (joy/frustration), SimEvent
//! Upstream: agent::events (ActionOutcomeEvent emitted by execution systems)
//! Downstream: mind::knowledge (MindGraph updated), psyche::emotions (EmotionalState updated)

use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::events::{ActionOutcome, ActionOutcomeEvent, FailureReason};
use crate::agent::mind::knowledge::{Concept, Metadata, MindGraph, Node, Predicate, Triple, Value};
use crate::agent::psyche::emotions::{
    Emotion, EmotionType, EmotionalState, add_emotion_with_event,
};
use bevy::prelude::*;

pub fn process_action_outcomes(
    mut agents: Query<
        (&mut MindGraph, &mut EmotionalState, Option<&PhysicalNeeds>),
        With<crate::agent::Agent>,
    >,
    mut outcome_events: MessageReader<ActionOutcomeEvent>,
    tick: Res<crate::core::tick::TickCount>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
) {
    let current_time = tick.current;

    for event in outcome_events.read() {
        if let Ok((mut mind, mut emotional_state, physical)) = agents.get_mut(event.actor) {
            match &event.outcome {
                ActionOutcome::Success {
                    target,
                    gained,
                    consumed,
                    need_satisfaction,
                    ..
                } => {
                    handle_success_outcome(&mut mind, target, gained, consumed, current_time);
                    if let Some(sat) = need_satisfaction {
                        generate_satisfaction_joy(
                            sat,
                            &mut emotional_state,
                            event.actor,
                            tick.current,
                            &mut sim_events,
                        );
                    }
                }

                ActionOutcome::Failed { target, reason, .. } => {
                    handle_failure_outcome(&mut mind, target, reason, current_time);
                    if let Some(needs) = physical {
                        generate_failure_frustration(
                            reason,
                            needs,
                            &mut emotional_state,
                            event.actor,
                            tick.current,
                            &mut sim_events,
                        );
                    }
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
        FailureReason::PathBlocked { target_tile } => {
            // Record the blocked target so the planner stops picking it.
            // TTL-checked on read in `generate_implicit_walk` via the
            // triple's metadata timestamp — no explicit decay needed.
            mind.assert(Triple::with_meta(
                Node::Tile(*target_tile),
                Predicate::HasTrait,
                Value::Concept(Concept::Unreachable),
                Metadata::experience(current_time),
            ));
        }
        _ => {}
    }
}

/// Joy proportional to need relief, scaled by how urgent the need was.
/// Eating when starving (hunger=90) produces much more joy than eating when barely hungry (hunger=10).
fn generate_satisfaction_joy(
    sat: &crate::agent::events::NeedSatisfaction,
    state: &mut EmotionalState,
    agent: Entity,
    tick: u64,
    sim_events: &mut MessageWriter<crate::agent::events::SimEvent>,
) {
    let hunger_joy = (sat.hunger_reduced / 100.0) * (sat.pre_hunger / 100.0);
    let thirst_joy = (sat.thirst_reduced / 100.0) * (sat.pre_thirst / 100.0);
    let joy_intensity = (hunger_joy + thirst_joy).clamp(0.0, 1.0);

    if joy_intensity > 0.01 {
        add_emotion_with_event(
            state,
            sim_events,
            agent,
            tick,
            Emotion::new(EmotionType::Joy, joy_intensity),
        );
    }
}

/// Frustration when a goal fails, proportional to how urgently that need was felt.
fn generate_failure_frustration(
    reason: &FailureReason,
    needs: &PhysicalNeeds,
    state: &mut EmotionalState,
    agent: Entity,
    tick: u64,
    sim_events: &mut MessageWriter<crate::agent::events::SimEvent>,
) {
    let urgency = match reason {
        FailureReason::NoEdibleFood | FailureReason::MissingItem(_) => needs.hunger_urgency(),
        FailureReason::NoWaterNearby => needs.thirst / 100.0,
        _ => 0.0,
    };

    if urgency > 0.1 {
        add_emotion_with_event(
            state,
            sim_events,
            agent,
            tick,
            Emotion::new(EmotionType::Anger, urgency * 0.6),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::events::NeedSatisfaction;

    #[test]
    fn starving_agent_gets_high_joy_from_eating() {
        let sat = NeedSatisfaction {
            hunger_reduced: 50.0,
            pre_hunger: 90.0,
            ..Default::default()
        };
        let joy = (sat.hunger_reduced / 100.0) * (sat.pre_hunger / 100.0);
        assert!(
            joy > 0.3,
            "starving agent should get high joy from eating (got {joy})"
        );
    }

    #[test]
    fn barely_hungry_agent_gets_low_joy_from_eating() {
        let sat = NeedSatisfaction {
            hunger_reduced: 50.0,
            pre_hunger: 15.0,
            ..Default::default()
        };
        let joy = (sat.hunger_reduced / 100.0) * (sat.pre_hunger / 100.0);
        assert!(
            joy < 0.1,
            "barely hungry agent should get low joy from eating (got {joy})"
        );
    }

    #[test]
    fn starving_joy_exceeds_barely_hungry_joy() {
        let starving = NeedSatisfaction {
            hunger_reduced: 50.0,
            pre_hunger: 90.0,
            ..Default::default()
        };
        let barely = NeedSatisfaction {
            hunger_reduced: 50.0,
            pre_hunger: 15.0,
            ..Default::default()
        };
        let starving_joy = (starving.hunger_reduced / 100.0) * (starving.pre_hunger / 100.0);
        let barely_joy = (barely.hunger_reduced / 100.0) * (barely.pre_hunger / 100.0);
        assert!(
            starving_joy > barely_joy * 4.0,
            "starving joy ({starving_joy}) should be much greater than barely-hungry joy ({barely_joy})"
        );
    }

    #[test]
    fn high_urgency_failure_generates_frustration() {
        let needs = PhysicalNeeds {
            metabolism: crate::agent::body::metabolism::Metabolism::at_urgency(0.85),
            ..Default::default()
        };
        let urgency = needs.hunger_urgency();
        let frustration = urgency * 0.6;
        assert!(
            frustration > 0.4,
            "high hunger failure should produce significant frustration (got {frustration})"
        );
    }

    #[test]
    fn low_urgency_failure_produces_no_frustration() {
        let urgency = 8.0_f32 / 100.0; // hunger=8, below 0.1 threshold
        assert!(
            urgency <= 0.1,
            "low urgency ({urgency}) should not trigger frustration"
        );
    }
}
