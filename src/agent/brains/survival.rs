//! Survival brain: reflexive responses to physical threats and urgent needs.
//!
//! Reads: PhysicalNeeds, CentralNervousSystem (urgencies), ItemSlots, ActiveActions, WorldMap, Transform
//! Writes: BrainProposal
//! Upstream: nervous_system::urgency (produces urgency scores), item_slots
//! Downstream: brains::proposal (winner selection)

use super::proposal::{BrainProposal, BrainType, Intent};
use crate::agent::actions::action::drink::is_adjacent_to_water;
use crate::agent::actions::{ActionType, ActiveActions};
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::Ontology;
use crate::agent::nervous_system::cns::CentralNervousSystem;
use crate::agent::nervous_system::urgency::UrgencySource;
use crate::constants::brains::survival::{WAKE_STAMINA_FRACTION, WAKE_WAKEFULNESS_THRESHOLD};
use crate::world::map::WorldMap;
use bevy::prelude::*;

pub struct SurvivalBrainContext<'a> {
    pub physical: &'a PhysicalNeeds,
    pub cns: &'a CentralNervousSystem,
    /// The visible entity the agent fears most, if any.
    pub most_feared_entity: Option<Entity>,
    pub pos: Vec2,
    pub world_map: &'a WorldMap,
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
) -> Vec<BrainProposal> {
    // While sleeping, the sleep/wake gate owns the decision — either stay
    // asleep or transition through WakeUp. No other proposals are generated.
    // The Sleep engagement (#746) keeps the Sleep beat in `active` for the
    // whole duration; the only valid survival proposal during it is WakeUp
    // when an emergency trigger fires or wakefulness has restored.
    if active.contains(ActionType::Sleep) {
        return check_sleep_wake(&context, active, action_registry)
            .map(|p| vec![p])
            .unwrap_or_default();
    }

    // One proposal per active survival urgency. Arbitration picks the winner
    // via score (urgency * survival_power). No priority gates needed — if
    // Sleepiness is higher than Stamina, Sleep naturally outscores Rest.
    let mut proposals = Vec::new();
    for u in context
        .cns
        .urgencies
        .iter()
        .filter(|u| u.source.is_survival())
    {
        if let Some(proposal) = propose_for_source(
            u.source,
            u.value,
            &context,
            inventory,
            ontology,
            action_registry,
        ) {
            proposals.push(proposal);
        }
    }
    proposals
}

fn propose_for_source(
    source: UrgencySource,
    value: f32,
    context: &SurvivalBrainContext,
    inventory: &ItemSlots,
    ontology: &Ontology,
    action_registry: &crate::agent::actions::ActionRegistry,
) -> Option<BrainProposal> {
    let urgency_score = value * 100.0;
    let intent = Intent::from_urgency_source(source);

    let escalated = |action: &dyn crate::agent::actions::registry::Action,
                     target: Option<bevy::prelude::Entity>|
     -> super::thinking::ActionTemplate {
        let mut t = action.to_template(target);
        t.escalate_intensity(value);
        t
    };

    match source {
        UrgencySource::Hunger => {
            if inventory.has_edible(ontology)
                && let Some(action) = action_registry.get(ActionType::Eat)
                && action.is_plan_time_viable(Some(context.physical), Some(inventory))
            {
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: escalated(action, None),
                    urgency: urgency_score,
                    intent,
                    reasoning: format!("Hunger urgency {:.2} — eating!", value),
                });
            }
        }
        UrgencySource::Thirst => {
            if is_adjacent_to_water(context.pos, context.world_map)
                && let Some(action) = action_registry.get(ActionType::Drink)
                && action.is_plan_time_viable(Some(context.physical), Some(inventory))
            {
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: escalated(action, None),
                    urgency: urgency_score,
                    intent,
                    reasoning: format!("Thirst urgency {:.2} — drinking!", value),
                });
            }
        }
        UrgencySource::Stamina => {
            if let Some(action) = action_registry.get(ActionType::Rest)
                && action.is_plan_time_viable(Some(context.physical), Some(inventory))
            {
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: escalated(action, None),
                    urgency: urgency_score,
                    intent,
                    reasoning: format!("Fatigue urgency {:.2} — resting.", value),
                });
            }
        }
        UrgencySource::Pain => {
            if let Some(action) = action_registry.get(ActionType::Idle) {
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: escalated(action, None),
                    urgency: urgency_score,
                    intent,
                    reasoning: format!("Pain urgency {:.2} — can't move!", value),
                });
            }
        }
        UrgencySource::Fear => {
            if let Some(action) = action_registry.get(ActionType::InitiateFlee) {
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: escalated(action, context.most_feared_entity),
                    urgency: urgency_score,
                    intent,
                    reasoning: format!("Fear urgency {:.2} — fleeing!", value),
                });
            }
        }
        UrgencySource::Sleepiness => {
            if let Some(action) = action_registry.get(ActionType::InitiateSleep)
                && action.is_plan_time_viable(Some(context.physical), Some(inventory))
            {
                return Some(BrainProposal {
                    brain: BrainType::Survival,
                    action: action.to_template(None),
                    urgency: urgency_score,
                    intent: Intent::SatisfySleepiness,
                    reasoning: format!("Sleepiness urgency {:.2} — sleeping!", value),
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
    let aerobic_fraction = context.physical.stamina.aerobic_fraction();
    let wakefulness = context.physical.wakefulness.value;

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

    // Rested wake: both wakefulness and stamina are recovered enough.
    // Compare aerobic as a fraction of max, not raw value — genetic
    // `aerobic_capacity` can set `aerobic_max` below the legacy 100,
    // which would otherwise keep the agent stuck asleep forever (their
    // real ceiling is below the absolute threshold).
    if wakefulness >= WAKE_WAKEFULNESS_THRESHOLD && aerobic_fraction >= WAKE_STAMINA_FRACTION {
        return Some(wake_proposal(
            50.0,
            format!(
                "Rested! Wakefulness {wakefulness:.2}, aerobic {aerobic:.0} ({:.0}% of max) — waking up",
                aerobic_fraction * 100.0
            ),
        ));
    }

    // Emergency wake: the urgency layer raised a flag because some drive's
    // raw input crossed its `sleep_wake_threshold`. Which drive and what
    // threshold is pure config — the brain just obeys the verdict.
    if let Some(source) = context.cns.sleep_wake_trigger {
        return Some(wake_proposal(90.0, format!("Emergency wake: {source:?}")));
    }

    // Still tired, nothing urgent — let the Sleep engagement keep running.
    // Pre-migration this re-proposed `Sleep` every tick to keep the
    // single-action slot occupied; with the engagement primitive the
    // SleepPlugin owns continuation, and `Sleep` is beat-only.
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mind::knowledge::setup_ontology;
    use crate::agent::nervous_system::urgency::Urgency;
    use crate::world::map::{CHUNK_SIZE, TileType};
    use bevy::math::IVec2;

    fn no_water_map() -> WorldMap {
        WorldMap::new(0, 0)
    }

    fn water_adjacent_map() -> WorldMap {
        use crate::world::map::Chunk;
        let mut map = WorldMap::new(16, 16);
        let chunks_x = 16u32.div_ceil(CHUNK_SIZE);
        let chunks_y = 16u32.div_ceil(CHUNK_SIZE);
        for cy in 0..chunks_y as i32 {
            for cx in 0..chunks_x as i32 {
                map.chunks.insert(IVec2::new(cx, cy), Chunk::new(cx, cy));
            }
        }
        map.set_tile(1, 0, TileType::ShallowWater);
        map
    }

    fn context_with_urgency<'a>(
        physical: &'a PhysicalNeeds,
        cns: &'a CentralNervousSystem,
        pos: Vec2,
        world_map: &'a WorldMap,
    ) -> SurvivalBrainContext<'a> {
        SurvivalBrainContext {
            physical,
            cns,
            most_feared_entity: None,
            pos,
            world_map,
        }
    }

    fn cns_with_top(source: UrgencySource, value: f32) -> CentralNervousSystem {
        let mut cns = CentralNervousSystem::default();
        cns.urgencies.push(Urgency::new(source, value));
        cns
    }

    /// Build a `PhysicalNeeds` whose target need is depleted below its
    /// satiation threshold — i.e. the corresponding satisfier action
    /// (Eat / Drink / Rest / Sleep) is viable. Use this in any test that
    /// wants to assert "a hungry agent proposes Eat" etc.; the bare
    /// `PhysicalNeeds::default()` is fully satisfied on every need and
    /// now correctly suppresses proposals for satiation-gated actions.
    fn needy_for(source: UrgencySource) -> PhysicalNeeds {
        use crate::agent::body::needs::Stamina;
        match source {
            UrgencySource::Hunger => PhysicalNeeds::full()
                .with_metabolism(crate::agent::body::metabolism::Metabolism::at_urgency(0.9)),
            UrgencySource::Thirst => PhysicalNeeds::full().with_hydration(0.1),
            UrgencySource::Stamina => PhysicalNeeds::full().with_stamina(Stamina {
                aerobic: 20.0,
                ..Default::default()
            }),
            UrgencySource::Sleepiness => PhysicalNeeds::full().with_wakefulness(0.1),
            _ => PhysicalNeeds::full(),
        }
    }

    /// Helper: find a proposal by action type from the multi-proposal Vec.
    fn find_proposal(
        proposals: &[BrainProposal],
        action_type: ActionType,
    ) -> Option<&BrainProposal> {
        proposals
            .iter()
            .find(|p| p.action.action_type == action_type)
    }

    #[test]
    fn high_hunger_urgency_proposes_eat_when_food_available() {
        let ontology = setup_ontology();
        let physical = needy_for(UrgencySource::Hunger);
        let cns = cns_with_top(UrgencySource::Hunger, 0.9);
        let map = no_water_map();
        let context = context_with_urgency(&physical, &cns, Vec2::ZERO, &map);

        let mut inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        inventory.add(crate::agent::mind::knowledge::Concept::Apple, 1);
        let active = ActiveActions::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register_def(&crate::agent::actions::action::EAT_DEF);

        let proposals = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        assert!(find_proposal(&proposals, ActionType::Eat).is_some());
    }

    #[test]
    fn hunger_with_full_stomach_does_not_propose_eat() {
        // Full stomach but low glucose/reserves: the urgency signal is
        // legitimately high (low blood sugar) but eating physically can't
        // satisfy it until digestion clears the stomach. The brain must
        // NOT propose Eat — otherwise it wins arbitration every tick and
        // the agent stands still mashing the fork against a closed mouth.
        let ontology = setup_ontology();
        let physical = PhysicalNeeds {
            metabolism: crate::agent::body::metabolism::Metabolism {
                // stomach_fraction = 0.90 → above the 0.80 Hunger satiation gate
                stomach_carbs: 54.0,
                stomach_fat: 36.0,
                // glucose + reserves below their caps to keep hunger_urgency high
                glucose: 5.0,
                reserves: 10.0,
            },
            ..Default::default()
        };
        let cns = cns_with_top(UrgencySource::Hunger, 0.9);
        let map = no_water_map();
        let context = context_with_urgency(&physical, &cns, Vec2::ZERO, &map);

        let mut inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        inventory.add(crate::agent::mind::knowledge::Concept::Apple, 1);
        let active = ActiveActions::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register_def(&crate::agent::actions::action::EAT_DEF);

        let proposals = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        assert!(
            find_proposal(&proposals, ActionType::Eat).is_none(),
            "full stomach must suppress Eat even when hunger urgency is high; got {proposals:?}"
        );
    }

    #[test]
    fn thirst_with_full_hydration_does_not_propose_drink() {
        // Hydration at 100% — the thirst gate is already closed but we
        // synthesise a high thirst urgency in the CNS to simulate a stale
        // urgency carried over from the previous tick.
        let ontology = setup_ontology();
        let physical = PhysicalNeeds {
            hydration: crate::agent::body::need::Need::full(),
            ..Default::default()
        };
        let cns = cns_with_top(UrgencySource::Thirst, 0.9);
        let map = water_adjacent_map();
        let context = context_with_urgency(&physical, &cns, Vec2::ZERO, &map);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = ActiveActions::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register_def(&crate::agent::actions::action::DRINK_DEF);

        let proposals = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        assert!(
            find_proposal(&proposals, ActionType::Drink).is_none(),
            "full hydration must suppress Drink; got {proposals:?}"
        );
    }

    #[test]
    fn stamina_fatigue_with_full_aerobic_does_not_propose_rest() {
        let ontology = setup_ontology();
        // Default physical already has aerobic_fraction = 1.0, above the
        // 0.95 Stamina satiation gate — exactly the scenario we need.
        let physical = PhysicalNeeds::default();
        let cns = cns_with_top(UrgencySource::Stamina, 0.9);
        let map = no_water_map();
        let context = context_with_urgency(&physical, &cns, Vec2::ZERO, &map);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = ActiveActions::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register_def(&crate::agent::actions::action::REST_DEF);

        let proposals = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        assert!(
            find_proposal(&proposals, ActionType::Rest).is_none(),
            "full aerobic must suppress Rest; got {proposals:?}"
        );
    }

    #[test]
    fn hunger_with_no_food_produces_no_eat_proposal() {
        let ontology = setup_ontology();
        let physical = PhysicalNeeds::default();
        let cns = cns_with_top(UrgencySource::Hunger, 0.9);
        let map = no_water_map();
        let context = context_with_urgency(&physical, &cns, Vec2::ZERO, &map);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry(); // empty
        let active = ActiveActions::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register_def(&crate::agent::actions::action::EXPLORE_DEF);
        registry.register_def(&crate::agent::actions::action::EAT_DEF);

        let proposals = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        assert!(
            find_proposal(&proposals, ActionType::Eat).is_none(),
            "Survival must defer to Rational when starving but empty-handed; \
             got proposals: {proposals:?}"
        );
    }

    #[test]
    fn stamina_fatigue_always_proposes_rest() {
        let ontology = setup_ontology();
        let physical = needy_for(UrgencySource::Stamina);
        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = ActiveActions::default();
        let map = no_water_map();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register_def(&crate::agent::actions::action::REST_DEF);
        registry.register_def(&crate::agent::actions::action::SLEEP_DEF);

        // Mild fatigue -> Rest
        let cns = cns_with_top(UrgencySource::Stamina, 0.4);
        let context = context_with_urgency(&physical, &cns, Vec2::ZERO, &map);
        let proposals = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        assert!(find_proposal(&proposals, ActionType::Rest).is_some());
        assert!(find_proposal(&proposals, ActionType::Sleep).is_none());

        // Severe fatigue -> still Rest (not Sleep)
        let cns = cns_with_top(UrgencySource::Stamina, 0.9);
        let context = context_with_urgency(&physical, &cns, Vec2::ZERO, &map);
        let proposals = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        assert!(find_proposal(&proposals, ActionType::Rest).is_some());
        assert!(
            find_proposal(&proposals, ActionType::Sleep).is_none(),
            "stamina fatigue must never propose Sleep"
        );
    }

    #[test]
    fn low_urgency_returns_empty_when_action_missing_from_registry() {
        let ontology = setup_ontology();
        let physical = PhysicalNeeds::default();
        let cns = cns_with_top(UrgencySource::Hunger, 0.9);
        let map = no_water_map();
        let context = context_with_urgency(&physical, &cns, Vec2::ZERO, &map);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = ActiveActions::default();
        let registry = crate::agent::actions::ActionRegistry::default(); // no actions

        let proposals = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        assert!(proposals.is_empty());
    }

    #[test]
    fn no_survival_urgency_returns_empty() {
        let ontology = setup_ontology();
        let physical = PhysicalNeeds::default();
        let cns = cns_with_top(UrgencySource::Social, 0.9);
        let map = no_water_map();
        let context = context_with_urgency(&physical, &cns, Vec2::ZERO, &map);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = ActiveActions::default();
        let registry = crate::agent::actions::ActionRegistry::default();

        let proposals = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        assert!(proposals.is_empty());
    }

    #[test]
    fn urgency_score_scales_with_urgency_value() {
        let ontology = setup_ontology();
        let physical = needy_for(UrgencySource::Hunger);

        let active = ActiveActions::default();
        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register_def(&crate::agent::actions::action::EAT_DEF);

        let mut inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        inventory.add(crate::agent::mind::knowledge::Concept::Apple, 1);

        let map = no_water_map();
        let cns_high = cns_with_top(UrgencySource::Hunger, 0.9);
        let cns_low = cns_with_top(UrgencySource::Hunger, 0.3);

        let high = survival_brain_propose(
            context_with_urgency(&physical, &cns_high, Vec2::ZERO, &map),
            &inventory,
            &active,
            &ontology,
            &registry,
        );
        let low = survival_brain_propose(
            context_with_urgency(&physical, &cns_low, Vec2::ZERO, &map),
            &inventory,
            &active,
            &ontology,
            &registry,
        );

        let high_eat = find_proposal(&high, ActionType::Eat).expect("high urgency should propose");
        let low_eat = find_proposal(&low, ActionType::Eat).expect("low urgency should propose");
        assert!(
            high_eat.urgency > low_eat.urgency,
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
        registry.register_def(&crate::agent::actions::action::SLEEP_DEF);
        registry.register_def(&crate::agent::actions::action::WAKE_UP_DEF);
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
        needs.stamina.aerobic = 10.0; // well below WAKE_STAMINA_THRESHOLD
        needs.wakefulness.set(0.2); // well below WAKE_WAKEFULNESS_THRESHOLD
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
        let map = no_water_map();
        let context = context_with_urgency(&physical, &cns, Vec2::ZERO, &map);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = active_sleep();
        let registry = sleeping_agent_registry();

        let proposals = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        assert!(
            find_proposal(&proposals, ActionType::WakeUp).is_some(),
            "sleeping agent with wake trigger should propose WakeUp; got {proposals:?}"
        );
    }

    #[test]
    fn no_sleep_wake_trigger_keeps_sleeping_agent_asleep() {
        let ontology = setup_ontology();
        let physical = tired_needs();
        let cns = CentralNervousSystem::default();
        let map = no_water_map();
        let context = context_with_urgency(&physical, &cns, Vec2::ZERO, &map);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = active_sleep();
        let registry = sleeping_agent_registry();

        let proposals = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        // Post-#746 migration: the SleepPlugin owns continuation, so the
        // survival brain stops re-proposing Sleep tick after tick. The
        // Sleep beat already in `active_actions` keeps the agent down.
        assert!(
            find_proposal(&proposals, ActionType::Sleep).is_none(),
            "Sleep beat is plugin-owned now; brain should not re-propose it: {proposals:?}"
        );
        assert!(find_proposal(&proposals, ActionType::WakeUp).is_none());
    }

    #[test]
    fn rested_sleeping_agent_wakes_even_without_trigger() {
        let ontology = setup_ontology();
        let physical = PhysicalNeeds::default(); // aerobic = 100, wakefulness = 1.0
        let cns = CentralNervousSystem::default();
        let map = no_water_map();
        let context = context_with_urgency(&physical, &cns, Vec2::ZERO, &map);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = active_sleep();
        let registry = sleeping_agent_registry();

        let proposals = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        assert!(find_proposal(&proposals, ActionType::WakeUp).is_some());
    }

    #[test]
    fn sleep_wake_trigger_ignored_when_awake() {
        let ontology = setup_ontology();
        let physical = PhysicalNeeds::default();
        let cns = CentralNervousSystem {
            sleep_wake_trigger: Some(UrgencySource::Fear),
            ..Default::default()
        };
        let map = no_water_map();
        let context = context_with_urgency(&physical, &cns, Vec2::ZERO, &map);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = ActiveActions::default(); // not sleeping
        let registry = sleeping_agent_registry();

        let proposals = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        assert!(
            proposals.is_empty(),
            "awake agents route through the urgency ladder, not the sleep gate; got {proposals:?}"
        );
    }

    #[test]
    fn survival_brain_proposes_behavior_not_action_type() {
        use crate::agent::actions::motor::{ActionPrimitive, Intent as MotorIntent};

        let ontology = setup_ontology();
        let physical = needy_for(UrgencySource::Hunger);
        let cns = cns_with_top(UrgencySource::Hunger, 0.9);
        let map = no_water_map();
        let context = context_with_urgency(&physical, &cns, Vec2::ZERO, &map);

        let mut inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        inventory.add(crate::agent::mind::knowledge::Concept::Apple, 1);
        let active = ActiveActions::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register_def(&crate::agent::actions::action::EAT_DEF);

        let proposals = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        let proposal = find_proposal(&proposals, ActionType::Eat).expect("should propose Eat");

        let behavior = &proposal.action.behavior;
        assert_eq!(
            behavior.primitive,
            ActionPrimitive::Ingest,
            "Eat should use the Ingest primitive"
        );
        assert_eq!(
            behavior.intent,
            MotorIntent::Hunger,
            "Eat should carry Hunger intent"
        );
    }

    #[test]
    fn high_urgency_escalates_flee_intensity() {
        use crate::agent::actions::motor::IntensityPolicy;

        let ontology = setup_ontology();
        let physical = PhysicalNeeds::default();
        // Fear urgency 0.9 → should escalate (though Flee is already Maximal)
        let cns = cns_with_top(UrgencySource::Fear, 0.9);
        let feared = bevy::prelude::Entity::from_bits(99);
        let map = no_water_map();
        let context = SurvivalBrainContext {
            physical: &physical,
            cns: &cns,
            most_feared_entity: Some(feared),
            pos: Vec2::ZERO,
            world_map: &map,
        };

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = ActiveActions::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register_def(&crate::agent::actions::action::INITIATE_FLEE_DEF);

        let proposals = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        let proposal = find_proposal(&proposals, ActionType::InitiateFlee)
            .expect("should propose InitiateFlee");

        assert!(
            matches!(proposal.action.behavior.intensity, IntensityPolicy::Maximal),
            "Flee at high urgency should be Maximal"
        );
    }

    #[test]
    fn moderate_thirst_keeps_normal_intensity() {
        use crate::agent::actions::motor::IntensityPolicy;

        let ontology = setup_ontology();
        let physical = needy_for(UrgencySource::Thirst);
        let cns = cns_with_top(UrgencySource::Thirst, 0.3);
        let map = water_adjacent_map();
        let pos = map.tile_to_world(0, 0);
        let context = context_with_urgency(&physical, &cns, pos, &map);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = ActiveActions::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register_def(&crate::agent::actions::action::DRINK_DEF);

        let proposals = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        let proposal = find_proposal(&proposals, ActionType::Drink)
            .expect("should propose Drink when adjacent to water");

        assert!(
            matches!(
                proposal.action.behavior.intensity,
                IntensityPolicy::Fixed(_)
            ),
            "Drink at moderate urgency should keep Fixed intensity"
        );
    }

    #[test]
    fn survival_does_not_propose_drink_when_no_water_adjacent() {
        let ontology = setup_ontology();
        let physical = PhysicalNeeds::default();
        let cns = cns_with_top(UrgencySource::Thirst, 0.9);
        let map = no_water_map();
        let context = context_with_urgency(&physical, &cns, Vec2::ZERO, &map);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = ActiveActions::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register_def(&crate::agent::actions::action::DRINK_DEF);

        let proposals = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        assert!(
            find_proposal(&proposals, ActionType::Drink).is_none(),
            "Survival must not propose Drink when no water is adjacent; got {proposals:?}"
        );
    }

    #[test]
    fn survival_proposes_drink_when_water_adjacent_and_thirsty() {
        let ontology = setup_ontology();
        let physical = needy_for(UrgencySource::Thirst);
        let cns = cns_with_top(UrgencySource::Thirst, 0.9);
        let map = water_adjacent_map();
        let pos = map.tile_to_world(0, 0);
        let context = context_with_urgency(&physical, &cns, pos, &map);

        let inventory = crate::agent::item_slots::ItemSlots::agent_carry();
        let active = ActiveActions::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register_def(&crate::agent::actions::action::DRINK_DEF);

        let proposals = survival_brain_propose(context, &inventory, &active, &ontology, &registry);
        assert!(
            find_proposal(&proposals, ActionType::Drink).is_some(),
            "Survival must propose Drink when thirsty and adjacent to water; got {proposals:?}"
        );
    }
}
