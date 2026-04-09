//! Survival brain: reflexive responses to physical threats and urgent needs.
//!
//! Reads: PhysicalNeeds, Consciousness, EmotionalState, Body, Inventory, VisibleObjects
//! Writes: BrainProposal
//! Upstream: body (needs, body), psyche (emotions), inventory
//! Downstream: brains::proposal (winner selection)

use super::proposal::{BrainProposal, BrainType};
use crate::agent::actions::{ActionState, ActionType};
use crate::agent::biology::body::Body;
use crate::agent::body::needs::{Consciousness, PhysicalNeeds};
use crate::agent::inventory::Inventory;
use crate::agent::mind::knowledge::Ontology;
use crate::agent::mind::perception::VisibleObjects;
use crate::agent::psyche::emotions::{EmotionType, EmotionalState};
use crate::constants::brains::survival::{
    EXHAUSTION_RELEASE, EXHAUSTION_TRIGGER, FEAR_HIGH, FEAR_LOW, HUNGER_HIGH, HUNGER_LOW,
    PAIN_HIGH, PAIN_LOW, SNAP_EXHAUSTION_ENERGY_THRESHOLD, SNAP_HUNGER_THRESHOLD,
    SNAP_SEARCH_HUNGER_THRESHOLD, STRESS_SNAP_HIGH, STRESS_SNAP_LOW, WAKE_ENERGY_THRESHOLD,
};
use bevy::prelude::*;

// ============================================================================
// SURVIVAL BRAIN
// ============================================================================

pub struct SurvivalBrainContext<'a> {
    pub physical: &'a PhysicalNeeds,
    pub consciousness: &'a Consciousness,
    pub emotions: &'a EmotionalState,
    pub body: Option<&'a Body>,
}

pub fn survival_brain_propose(
    context: SurvivalBrainContext,
    inventory: &Inventory,
    _visible: &VisibleObjects,
    previous_winner: Option<BrainType>,
    activity: &ActionState,
    ontology: &Ontology,
    action_registry: &crate::agent::actions::ActionRegistry,
) -> Option<BrainProposal> {
    let was_survival = previous_winner == Some(BrainType::Survival);

    // Sleep/Wake Check (Separate logic)
    if let Some(proposal) = check_sleep_wake(&context, activity, action_registry) {
        return Some(proposal);
    }

    // --- THE SNAP (Extreme Stress) ---
    // Threshold hysteresis: Needs STRESS_SNAP_HIGH to start, drops to STRESS_SNAP_LOW to stop
    let stress = context.emotions.stress_level;
    let snap_threshold = if was_survival {
        STRESS_SNAP_LOW
    } else {
        STRESS_SNAP_HIGH
    };

    if stress > snap_threshold {
        // 1. Extreme Hunger Snap
        if context.physical.hunger > SNAP_HUNGER_THRESHOLD
            && inventory.has_edible(ontology)
            && let Some(action) = action_registry.get(ActionType::Eat)
        {
            return Some(BrainProposal {
                brain: BrainType::Survival,
                action: action.to_template(None, None),
                urgency: 100.0,
                reasoning: format!("THE SNAP! Stress {:.0} - desperately eating!", stress),
            });
        }

        // 2. Extreme Hunger Search Snap
        if context.physical.hunger > SNAP_SEARCH_HUNGER_THRESHOLD
            && let Some(action) = action_registry.get(ActionType::Explore)
        {
            return Some(BrainProposal {
                brain: BrainType::Survival,
                action: action.to_template(None, None),
                urgency: 95.0,
                reasoning: format!("THE SNAP! Stress {:.0} - desperately seeking food!", stress),
            });
        }

        // 3. Exhaustion Snap
        if context.physical.energy < SNAP_EXHAUSTION_ENERGY_THRESHOLD
            && let Some(action) = action_registry.get(ActionType::Sleep)
        {
            return Some(BrainProposal {
                brain: BrainType::Survival,
                action: action.to_template(None, None),
                urgency: 100.0,
                reasoning: format!(
                    "THE SNAP! Stress {:.0} - collapsing from exhaustion!",
                    stress
                ),
            });
        }

        // 4. Panic Hide Snap (Default if others don't fire)
        // Seek safety usually implies Walk to safety or Flee
        // Using WalkAction for now as "Seek Safety" creates variable destination
        // But for now, let's use Flee with no target (run away randomly?) or fallback
        if let Some(action) = action_registry.get(ActionType::Flee) {
            return Some(BrainProposal {
                brain: BrainType::Survival,
                action: action.to_template(None, None),
                urgency: 90.0,
                reasoning: format!("THE SNAP! Stress {:.0} - hiding!", stress),
            });
        }
    }

    // --- STANDARD SURVIVAL REFLEXES ---

    // 1. Pain Response
    let pain = context.body.map(|b| b.total_pain()).unwrap_or(0.0);
    let pain_threshold = if was_survival { PAIN_LOW } else { PAIN_HIGH };
    if pain > pain_threshold {
        // Idle/CurlUp
        if let Some(action) = action_registry.get(ActionType::Idle) {
            return Some(BrainProposal {
                brain: BrainType::Survival,
                action: action.to_template(None, None),
                urgency: pain, // Urgency scales with pain
                reasoning: format!("PAIN! {:.0} - can't move!", pain),
            });
        }
    }

    // 2. Starvation Response
    let hunger_threshold = if was_survival {
        HUNGER_LOW
    } else {
        HUNGER_HIGH
    };
    if context.physical.hunger > hunger_threshold
        && inventory.has_edible(ontology)
        && let Some(action) = action_registry.get(ActionType::Eat)
    {
        return Some(BrainProposal {
            brain: BrainType::Survival,
            action: action.to_template(None, None), // Target will be found by execution or planner?
            // Wait, EatAction to_template might need a target if we want specific target.
            // But standard Survival Eat response was "Eat Nearest".
            // Generic EatAction usually implies finding food.
            // Let's assume generic template is fine or check if EatAction supports None.
            // EatAction::to_template implementation (checked earlier) supports None for "find something".
            urgency: context.physical.hunger,
            reasoning: format!("STARVING! {:.0} - must eat!", context.physical.hunger),
        });
    }
    // If no food, survival brain might panic search?
    // For now, let Rational handle searching unless it's a "Snap".

    // 3. Exhaustion Response
    // Low energy triggers sleep. Hysteresis: sleep until EXHAUSTION_RELEASE, triggered below EXHAUSTION_TRIGGER.
    let exhaustion_threshold = if was_survival {
        EXHAUSTION_RELEASE
    } else {
        EXHAUSTION_TRIGGER
    };
    if context.physical.energy < exhaustion_threshold
        && let Some(action) = action_registry.get(ActionType::Sleep)
    {
        return Some(BrainProposal {
            brain: BrainType::Survival,
            action: action.to_template(None, None), // Sleep here
            urgency: 100.0 - context.physical.energy,
            reasoning: format!(
                "EXHAUSTED! {:.0} energy - collapsing!",
                context.physical.energy
            ),
        });
    }

    // 4. Fear Response
    let fear = context.emotions.get_emotion_intensity(EmotionType::Fear);
    let fear_threshold = if was_survival { FEAR_LOW } else { FEAR_HIGH };
    if fear > fear_threshold
        && let Some(action) = action_registry.get(ActionType::Flee)
    {
        return Some(BrainProposal {
            brain: BrainType::Survival,
            action: action.to_template(None, None),
            urgency: fear * 100.0,
            reasoning: format!("TERROR! {:.2} - must hide!", fear),
        });
    }

    None
}

fn check_sleep_wake(
    context: &SurvivalBrainContext,
    activity: &ActionState,
    action_registry: &crate::agent::actions::ActionRegistry,
) -> Option<BrainProposal> {
    let energy = context.physical.energy;
    let is_sleeping = activity.action_type == ActionType::Sleep;

    if is_sleeping {
        if energy >= WAKE_ENERGY_THRESHOLD {
            let wake_action = action_registry
                .get(ActionType::WakeUp)
                .map(|a| a.to_template(None, None))
                .expect("WakeUp action must be registered");
            return Some(BrainProposal {
                brain: BrainType::Survival,
                action: wake_action,
                urgency: 50.0,
                reasoning: format!("Rested! Energy {:.0} - waking up", energy),
            });
        } else {
            // Stay asleep
            // Survival brain keeps proposing "Sleep Here" essentially to maintain state?
            // Or if we return Some, we override rational.
            // If we are already sleeping, Rational might not propose anything else, so maybe we don't need to force it.
            // But if we want to ensure we don't wake up until rested:
            if let Some(action) = action_registry.get(ActionType::Sleep) {
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: action.to_template(None, None),
                    urgency: 100.0 - energy,
                    reasoning: format!("Still tired... {:.0} energy", energy),
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mind::knowledge::Concept;
    use crate::agent::mind::knowledge::setup_ontology;

    fn mock_context<'a>(
        physical: &'a PhysicalNeeds,
        consciousness: &'a Consciousness,
        emotions: &'a EmotionalState,
    ) -> SurvivalBrainContext<'a> {
        SurvivalBrainContext {
            physical,
            consciousness,
            emotions,
            body: None, // Body difficult to mock easily here without complex setup
        }
    }

    #[test]
    fn test_survival_hunger_response() {
        let ontology = setup_ontology();
        let mut physical = PhysicalNeeds::default();
        physical.hunger = 90.0;
        let consciousness = Consciousness::default();
        let emotions = EmotionalState::default();

        let context = mock_context(&physical, &consciousness, &emotions);
        let mut inventory = Inventory::default();
        inventory.add(Concept::Apple, 1);
        let visible = VisibleObjects::default();
        let activity = ActionState::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register(crate::agent::actions::action::EatAction);

        let proposal = survival_brain_propose(
            context, &inventory, &visible, None, &activity, &ontology, &registry,
        );

        assert!(proposal.is_some());
        assert_eq!(proposal.unwrap().action.name, "Eat");
    }
}
