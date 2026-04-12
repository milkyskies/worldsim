//! Survival brain: reflexive responses to physical threats and urgent needs.
//!
//! Reads: PhysicalNeeds, CentralNervousSystem (urgencies), ItemSlots, ActiveActions
//! Writes: BrainProposal
//! Upstream: nervous_system::urgency (produces urgency scores), item_slots
//! Downstream: brains::proposal (winner selection)

use super::proposal::{BrainProposal, BrainType, Intent};
use crate::agent::actions::{ActionType, ActiveActions};
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::Ontology;
use crate::agent::nervous_system::cns::CentralNervousSystem;
use crate::agent::nervous_system::urgency::UrgencySource;
use crate::constants::brains::survival::{SLEEPINESS_SLEEP_THRESHOLD, WAKE_WAKEFULNESS_THRESHOLD};
use bevy::prelude::*;

pub struct SurvivalBrainContext<'a> {
    pub physical: &'a PhysicalNeeds,
    pub cns: &'a CentralNervousSystem,
    /// The visible entity the agent fears most, if any.
    pub most_feared_entity: Option<Entity>,
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
    // While sleeping, the sleep/wake gate owns the decision — either stay
    // asleep or transition through WakeUp. The urgency ladder below runs
    // normally once awake.
    if let Some(proposal) = check_sleep_wake(&context, active, action_registry) {
        return Some(proposal);
    }

    // Find the top survival-relevant urgency (urgencies are sorted highest-first).
    let top = context
        .cns
        .urgencies
        .iter()
        .find(|u| u.source.is_survival())?;

    let urgency_score = top.value * 100.0;
    let intent = Intent::from_urgency_source(top.source);

    match top.source {
        UrgencySource::Hunger => {
            // Direct reflex: eat if we have something edible in hand.
            if inventory.has_edible(ontology)
                && let Some(action) = action_registry.get(ActionType::Eat)
            {
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: action.to_template(None),
                    urgency: urgency_score,
                    intent,
                    reasoning: format!("Hunger urgency {:.2} — eating!", top.value),
                });
            }
            // No food in hand — defer to Rational. The planner can find a known
            // food source or fall back to its own Explore. Survival proposing
            // Explore here would duplicate Rational's job and outscore the
            // planner's actual plan inside intent dedup, blocking it.
        }
        UrgencySource::Thirst => {
            if let Some(action) = action_registry.get(ActionType::Drink) {
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: action.to_template(None),
                    urgency: urgency_score,
                    intent,
                    reasoning: format!("Thirst urgency {:.2} — drinking!", top.value),
                });
            }
        }
        UrgencySource::Stamina => {
            // Stamina fatigue only proposes Rest (sit-and-recover). Full
            // Sleep is now driven exclusively by the Sleepiness urgency
            // from wakefulness decay (#462). This decouples physical
            // exhaustion from sleepiness — a tired agent rests, a drowsy
            // agent sleeps.
            if let Some(action) = action_registry.get(ActionType::Rest) {
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: action.to_template(None),
                    urgency: urgency_score,
                    intent,
                    reasoning: format!("Fatigue urgency {:.2} — resting.", top.value),
                });
            }
        }
        UrgencySource::Pain => {
            if let Some(action) = action_registry.get(ActionType::Idle) {
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: action.to_template(None),
                    urgency: urgency_score,
                    intent,
                    reasoning: format!("Pain urgency {:.2} — can't move!", top.value),
                });
            }
        }
        UrgencySource::Fear => {
            if let Some(action) = action_registry.get(ActionType::Flee) {
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: action.to_template(context.most_feared_entity),
                    urgency: urgency_score,
                    intent,
                    reasoning: format!("Fear urgency {:.2} — fleeing!", top.value),
                });
            }
        }
        UrgencySource::Sleepiness => {
            let action_type = if top.value >= SLEEPINESS_SLEEP_THRESHOLD {
                ActionType::Sleep
            } else {
                ActionType::Rest
            };
            if let Some(action) = action_registry.get(action_type) {
                let reasoning = match action_type {
                    ActionType::Sleep => {
                        format!("Sleepiness urgency {:.2} — sleeping!", top.value)
                    }
                    _ => format!("Sleepiness urgency {:.2} — resting.", top.value),
                };
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: action.to_template(None),
                    urgency: urgency_score,
                    intent: Intent::SatisfySleepiness,
                    reasoning,
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
    if !active.contains(ActionType::Sleep) {
        return None;
    }

    let aerobic = context.physical.stamina.aerobic;
    let wakefulness = context.physical.wakefulness;

    let wake_proposal = |urgency: f32, reasoning: String| BrainProposal {
        brain: BrainType::Survival,
        action: action_registry
            .get(ActionType::WakeUp)
            .map(|a| a.to_template(None))
            .expect("WakeUp action must be registered"),
        urgency,
        intent: Intent::SatisfySleepiness,
        reasoning,
    };

    // Rested wake: wakefulness is the primary gate. Once sleep pressure is
    // cleared, the agent wakes regardless of stamina — any remaining
    // physical fatigue triggers Rest (not Sleep) once awake. Using AND
    // here would trap agents in Sleep with high wakefulness but slow
    // stamina recovery.
    if wakefulness >= WAKE_WAKEFULNESS_THRESHOLD {
        return Some(wake_proposal(
            50.0,
            format!("Rested! Wakefulness {wakefulness:.2} — waking up"),
        ));
    }

    // Emergency wake: the urgency layer raised a flag because some drive's
    // raw input crossed its `sleep_wake_threshold`. Which drive and what
    // threshold is pure config — the brain just obeys the verdict.
    if let Some(source) = context.cns.sleep_wake_trigger {
        return Some(wake_proposal(90.0, format!("Emergency wake: {source:?}")));
    }

    // Still tired, nothing urgent — stay asleep.
    let sleep_urgency = (1.0 - wakefulness) * 100.0;
    action_registry
        .get(ActionType::Sleep)
        .map(|action| BrainProposal {
            brain: BrainType::Survival,
            action: action.to_template(None),
            urgency: sleep_urgency,
            intent: Intent::SatisfySleepiness,
            reasoning: format!("Still tired... wakefulness {wakefulness:.2}, aerobic {aerobic:.0}"),
        })
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
        SurvivalBrainContext {
            physical,
            cns,
            most_feared_entity: None,
        }
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

    /// Survival is for direct reflexive actions only — eating food in hand,
    /// drinking, sleeping, fleeing, curling up in pain. Random exploration to
    /// FIND food is a planning concern; Rational owns it (planner +
    /// rational.rs's own Explore fallback). Survival proposing Explore would
    /// duplicate Rational's job and outscore Rational's actual plan inside
    /// intent dedup, blocking the planner from ever executing.
    #[test]
    fn hunger_with_no_food_returns_none_so_rational_can_plan() {
        let ontology = setup_ontology();
        let physical = PhysicalNeeds::default();
        let cns = cns_with_top(UrgencySource::Hunger, 0.9);
        let context = context_with_urgency(&physical, &cns);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry(); // empty
        let active = ActiveActions::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register(crate::agent::actions::action::ExploreAction);
        registry.register(crate::agent::actions::action::EatAction);

        let proposal = survival_brain_propose(context, &inventory, &active, &ontology, &registry);

        assert!(
            proposal.is_none(),
            "Survival must defer to Rational when starving but empty-handed; \
             got proposal: {proposal:?}"
        );
    }

    #[test]
    fn stamina_fatigue_always_proposes_rest() {
        // #462: Stamina urgency always routes to Rest, never Sleep.
        // Sleep is now driven exclusively by the Sleepiness urgency
        // from wakefulness decay, decoupling physical exhaustion from
        // sleepiness.
        let ontology = setup_ontology();
        let physical = PhysicalNeeds::default();
        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = ActiveActions::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register(crate::agent::actions::action::RestAction);
        registry.register(crate::agent::actions::action::SleepAction);

        // Mild fatigue → Rest
        let cns = cns_with_top(UrgencySource::Stamina, 0.4);
        let context = context_with_urgency(&physical, &cns);
        let proposal = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        let proposal = proposal.expect("mild fatigue must produce a proposal");
        assert_eq!(proposal.action.action_type, ActionType::Rest);

        // Severe fatigue → still Rest (not Sleep)
        let cns = cns_with_top(UrgencySource::Stamina, 0.9);
        let context = context_with_urgency(&physical, &cns);
        let proposal = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        let proposal = proposal.expect("severe fatigue must produce a proposal");
        assert_eq!(
            proposal.action.action_type,
            ActionType::Rest,
            "stamina fatigue must never propose Sleep; got {:?}",
            proposal.action.name,
        );
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

    // ── Sleep/wake behaviour ────────────────────────────────────────────────
    //
    // These unit-test the `check_sleep_wake` gate in isolation. The wake
    // thresholds themselves live in `NervousSystemConfig` and are applied
    // in `urgency::generate_urgency`; the brain just observes the verdict
    // via `cns.sleep_wake_trigger`. So these tests seed the trigger directly
    // rather than running the full urgency pipeline — that integration is
    // covered by the scenario tests in `tests/test_sleep_wake_cycle.rs`.

    fn sleeping_agent_registry() -> crate::agent::actions::ActionRegistry {
        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register(crate::agent::actions::action::SleepAction);
        registry.register(crate::agent::actions::action::WakeUpAction);
        registry
    }

    fn active_sleep() -> ActiveActions {
        let mut active = ActiveActions::empty();
        active.insert(crate::agent::actions::ActionState::new(
            ActionType::Sleep,
            0,
        ));
        active
    }

    fn tired_needs() -> PhysicalNeeds {
        let mut needs = PhysicalNeeds::default();
        needs.stamina.aerobic = 10.0;
        needs.wakefulness = 0.2; // well below WAKE_WAKEFULNESS_THRESHOLD
        needs
    }

    #[test]
    fn sleep_wake_trigger_rouses_sleeping_agent() {
        let ontology = setup_ontology();
        let physical = tired_needs();
        let cns = CentralNervousSystem {
            sleep_wake_trigger: Some(UrgencySource::Fear),
            ..Default::default()
        };
        let context = context_with_urgency(&physical, &cns);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = active_sleep();
        let registry = sleeping_agent_registry();

        let proposal = survival_brain_propose(context, &inventory, &active, &ontology, &registry)
            .expect("sleeping agent with wake trigger should propose WakeUp");
        assert_eq!(
            proposal.action.name, "Wake Up",
            "got {:?}",
            proposal.action.name
        );
    }

    #[test]
    fn no_sleep_wake_trigger_keeps_sleeping_agent_asleep() {
        let ontology = setup_ontology();
        let physical = tired_needs();
        // No trigger, no urgency — the gate should keep the agent asleep.
        let cns = CentralNervousSystem::default();
        let context = context_with_urgency(&physical, &cns);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = active_sleep();
        let registry = sleeping_agent_registry();

        let proposal = survival_brain_propose(context, &inventory, &active, &ontology, &registry)
            .expect("sleeping agent should keep sleeping");
        assert_eq!(
            proposal.action.name, "Sleep",
            "expected Sleep, got {:?}",
            proposal.action.name
        );
    }

    #[test]
    fn rested_sleeping_agent_wakes_even_without_trigger() {
        let ontology = setup_ontology();
        // Aerobic at the wake threshold — normal homeostatic wake.
        let physical = PhysicalNeeds::default(); // aerobic = 100
        let cns = CentralNervousSystem::default();
        let context = context_with_urgency(&physical, &cns);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = active_sleep();
        let registry = sleeping_agent_registry();

        let proposal = survival_brain_propose(context, &inventory, &active, &ontology, &registry)
            .expect("rested sleeping agent should wake");
        assert_eq!(proposal.action.name, "Wake Up");
    }

    #[test]
    fn high_wakefulness_wakes_agent_even_with_low_stamina() {
        // #462: the rested-wake gate uses wakefulness only. An agent with
        // high wakefulness but low stamina must wake up — any remaining
        // physical fatigue triggers Rest once awake, not trapped Sleep.
        let ontology = setup_ontology();
        let mut physical = PhysicalNeeds::default();
        physical.wakefulness = 0.95; // above WAKE_WAKEFULNESS_THRESHOLD (0.9)
        physical.stamina.aerobic = 30.0; // low stamina
        let cns = CentralNervousSystem::default();
        let context = context_with_urgency(&physical, &cns);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = active_sleep();
        let registry = sleeping_agent_registry();

        let proposal = survival_brain_propose(context, &inventory, &active, &ontology, &registry)
            .expect("high-wakefulness agent should wake even with low stamina");
        assert_eq!(
            proposal.action.name, "Wake Up",
            "wakefulness 0.95 should trigger wake regardless of stamina; got {:?}",
            proposal.action.name,
        );
    }

    #[test]
    fn sleep_wake_trigger_ignored_when_awake() {
        // A trigger set on an awake agent should NOT propose WakeUp through
        // the sleep gate — check_sleep_wake bails out immediately and the
        // normal urgency ladder runs. With only a trigger (no urgencies),
        // the ladder proposes nothing.
        let ontology = setup_ontology();
        let physical = PhysicalNeeds::default();
        let cns = CentralNervousSystem {
            sleep_wake_trigger: Some(UrgencySource::Fear),
            ..Default::default()
        };
        let context = context_with_urgency(&physical, &cns);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = ActiveActions::default(); // not sleeping
        let registry = sleeping_agent_registry();

        let proposal = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        assert!(
            proposal.is_none(),
            "awake agents route through the urgency ladder, not the sleep gate; got {proposal:?}"
        );
    }
}
