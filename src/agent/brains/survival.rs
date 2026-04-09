//! Survival brain: reflexive responses to physical threats and urgent needs.
//!
//! Reads: PhysicalNeeds, CentralNervousSystem (urgencies), ItemSlots, ActiveActions
//! Writes: BrainProposal
//! Upstream: nervous_system::urgency (produces urgency scores), item_slots
//! Downstream: brains::proposal (winner selection)

use super::proposal::{BrainProposal, BrainType};
use crate::agent::actions::{ActionType, ActiveActions};
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::Ontology;
use crate::agent::nervous_system::cns::CentralNervousSystem;
use crate::agent::nervous_system::urgency::UrgencySource;
use crate::constants::brains::survival::WAKE_ENERGY_THRESHOLD;
use bevy::prelude::*;

pub struct SurvivalBrainContext<'a> {
    pub physical: &'a PhysicalNeeds,
    pub cns: &'a CentralNervousSystem,
}

/// Propose a survival action based on the highest urgency drive.
///
/// Hysteresis is handled by the nervous system's momentum bonus — no manual
/// `was_survival` threshold switching needed here.
pub fn survival_brain_propose(
    context: SurvivalBrainContext,
    inventory: &ItemSlots,
    active: &ActiveActions,
    ontology: &Ontology,
    action_registry: &crate::agent::actions::ActionRegistry,
) -> Option<BrainProposal> {
    // Sleep/Wake state machine — not urgency-driven; handles the wake threshold.
    if let Some(proposal) = check_sleep_wake(&context, active, action_registry) {
        return Some(proposal);
    }

    // Find the top survival-relevant urgency (urgencies are sorted highest-first).
    let survival_sources = [
        UrgencySource::Hunger,
        UrgencySource::Thirst,
        UrgencySource::Energy,
        UrgencySource::Pain,
        UrgencySource::Fear,
    ];

    let top = context
        .cns
        .urgencies
        .iter()
        .find(|u| survival_sources.contains(&u.source))?;

    let urgency_score = top.value * 100.0;

    match top.source {
        UrgencySource::Hunger => {
            if inventory.has_edible(ontology)
                && let Some(action) = action_registry.get(ActionType::Eat)
            {
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: action.to_template(None, None),
                    urgency: urgency_score,
                    reasoning: format!("Hunger urgency {:.2} — eating!", top.value),
                });
            }
            // No food in inventory — search for some.
            if let Some(action) = action_registry.get(ActionType::Explore) {
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: action.to_template(None, None),
                    urgency: urgency_score * 0.7,
                    reasoning: format!("Hunger urgency {:.2} — searching for food!", top.value),
                });
            }
        }
        UrgencySource::Thirst => {
            if let Some(action) = action_registry.get(ActionType::Drink) {
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: action.to_template(None, None),
                    urgency: urgency_score,
                    reasoning: format!("Thirst urgency {:.2} — drinking!", top.value),
                });
            }
        }
        UrgencySource::Energy => {
            if let Some(action) = action_registry.get(ActionType::Sleep) {
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: action.to_template(None, None),
                    urgency: urgency_score,
                    reasoning: format!("Fatigue urgency {:.2} — sleeping!", top.value),
                });
            }
        }
        UrgencySource::Pain => {
            if let Some(action) = action_registry.get(ActionType::Idle) {
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: action.to_template(None, None),
                    urgency: urgency_score,
                    reasoning: format!("Pain urgency {:.2} — can't move!", top.value),
                });
            }
        }
        UrgencySource::Fear => {
            if let Some(action) = action_registry.get(ActionType::Flee) {
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: action.to_template(None, None),
                    urgency: urgency_score,
                    reasoning: format!("Fear urgency {:.2} — fleeing!", top.value),
                });
            }
        }
        _ => {}
    }

    None
}

fn check_sleep_wake(
    context: &SurvivalBrainContext,
    active: &ActiveActions,
    action_registry: &crate::agent::actions::ActionRegistry,
) -> Option<BrainProposal> {
    let energy = context.physical.energy;
    let is_sleeping = active.contains(ActionType::Sleep);

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
                reasoning: format!("Rested! Energy {:.0} — waking up", energy),
            });
        } else if let Some(action) = action_registry.get(ActionType::Sleep) {
            return Some(BrainProposal {
                brain: BrainType::Survival,
                action: action.to_template(None, None),
                urgency: 100.0 - energy,
                reasoning: format!("Still tired... {:.0} energy", energy),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mind::knowledge::setup_ontology;
    use crate::agent::nervous_system::urgency::Urgency;

    fn context_with_urgency<'a>(
        physical: &'a PhysicalNeeds,
        cns: &'a CentralNervousSystem,
    ) -> SurvivalBrainContext<'a> {
        SurvivalBrainContext { physical, cns }
    }

    fn cns_with_top(source: UrgencySource, value: f32) -> CentralNervousSystem {
        let mut cns = CentralNervousSystem::default();
        cns.urgencies.push(Urgency::new(source, value));
        cns
    }

    #[test]
    fn high_hunger_urgency_proposes_eat_when_food_available() {
        let ontology = setup_ontology();
        let physical = PhysicalNeeds::default();
        let cns = cns_with_top(UrgencySource::Hunger, 0.9);
        let context = context_with_urgency(&physical, &cns);

        let mut inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        inventory.add(crate::agent::mind::knowledge::Concept::Apple, 1);
        let active = ActiveActions::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register(crate::agent::actions::action::EatAction);

        let proposal = survival_brain_propose(context, &inventory, &active, &ontology, &registry);

        assert!(proposal.is_some());
        assert_eq!(proposal.unwrap().action.name, "Eat");
    }

    #[test]
    fn high_hunger_urgency_proposes_explore_when_no_food() {
        let ontology = setup_ontology();
        let physical = PhysicalNeeds::default();
        let cns = cns_with_top(UrgencySource::Hunger, 0.9);
        let context = context_with_urgency(&physical, &cns);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry(); // empty
        let active = ActiveActions::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register(crate::agent::actions::action::ExploreAction);

        let proposal = survival_brain_propose(context, &inventory, &active, &ontology, &registry);

        assert!(proposal.is_some());
        assert_eq!(proposal.unwrap().action.name, "Explore");
    }

    #[test]
    fn low_urgency_returns_none_when_action_missing_from_registry() {
        let ontology = setup_ontology();
        let physical = PhysicalNeeds::default();
        let cns = cns_with_top(UrgencySource::Hunger, 0.9);
        let context = context_with_urgency(&physical, &cns);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = ActiveActions::default();
        let registry = crate::agent::actions::ActionRegistry::default(); // no actions

        let proposal = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        assert!(proposal.is_none());
    }

    #[test]
    fn no_survival_urgency_returns_none() {
        let ontology = setup_ontology();
        let physical = PhysicalNeeds::default();
        // Only social urgency — not a survival concern
        let cns = cns_with_top(UrgencySource::Social, 0.9);
        let context = context_with_urgency(&physical, &cns);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = ActiveActions::default();
        let registry = crate::agent::actions::ActionRegistry::default();

        let proposal = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        assert!(proposal.is_none());
    }

    #[test]
    fn urgency_score_scales_with_urgency_value() {
        let ontology = setup_ontology();
        let physical = PhysicalNeeds::default();

        let active = ActiveActions::default();
        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register(crate::agent::actions::action::EatAction);

        let mut inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        inventory.add(crate::agent::mind::knowledge::Concept::Apple, 1);

        let cns_high = cns_with_top(UrgencySource::Hunger, 0.9);
        let cns_low = cns_with_top(UrgencySource::Hunger, 0.3);

        let high_proposal = survival_brain_propose(
            context_with_urgency(&physical, &cns_high),
            &inventory,
            &active,
            &ontology,
            &registry,
        )
        .unwrap();
        let low_proposal = survival_brain_propose(
            context_with_urgency(&physical, &cns_low),
            &inventory,
            &active,
            &ontology,
            &registry,
        )
        .unwrap();

        assert!(
            high_proposal.urgency > low_proposal.urgency,
            "higher urgency input should produce higher urgency proposal"
        );
    }
}
