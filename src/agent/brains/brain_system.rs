//! Three-brains orchestration: runs all brain systems and arbitrates between their proposals each tick.
//!
//! Reads: PhysicalNeeds, Consciousness, PsychologicalDrives, EmotionalState, Body, Personality, ItemSlots, VisibleObjects, MindGraph, ActiveActions, WorldMap, BrainHistory, PlanMemory
//! Writes: BrainState (chosen action, winner, proposals, powers), BrainHistory (active attributions), SimEvent::Decision
//! Upstream: survival/emotional/rational brain modules, arbitration, perception, knowledge
//! Downstream: nervous_system::cns (executes the chosen action), SimEvent consumers

use super::arbitration::{arbitrate_parallel, calculate_brain_powers};
use super::emotional::{emotional_brain_propose, find_most_feared_visible_entity};
use super::history::BrainHistory;
use super::plan_memory::{PlanMemory, PlanState};
use super::proposal::{BrainPowers, BrainState, BrainType};
use super::rational::rational_brain_propose;
use super::survival::{SurvivalBrainContext, survival_brain_propose};
use crate::agent::biology::body::Body;
use crate::agent::body::needs::{Consciousness, PhysicalNeeds, PsychologicalDrives};
use crate::agent::mind::perception::VisibleObjects;
use crate::agent::nervous_system::cns::CentralNervousSystem;
use crate::agent::psyche::emotions::EmotionalState;
use crate::agent::psyche::personality::Personality;
use crate::world::map::WorldMap;
use bevy::prelude::*;

/// Collect proposals from all three brains and pick the admitted
/// action set for every agent, every tick. Proposal generation,
/// arbitration, and BrainState writeback are all cheap enough to run
/// per-tick so the recorded winner always agrees with what the body
/// is running. Expensive GOAP re-planning lives in
/// `rational::update_rational_planning` and fires only when the
/// current goal has no concrete plan.
pub fn arbitrate_every_tick(
    tick: Res<crate::core::tick::TickCount>,
    mut query: Query<
        (
            Entity,
            &Name,
            &mut BrainState,
            // Brains
            (&mut PlanMemory, &CentralNervousSystem),
            // Needs
            (
                &PhysicalNeeds,
                &mut Consciousness,
                Option<&PsychologicalDrives>,
            ),
            // Body & Self
            (
                &EmotionalState,
                Option<&Body>,
                &Personality,
                &crate::agent::item_slots::ItemSlots,
            ),
            // Context
            (
                &Transform,
                &VisibleObjects,
                &crate::agent::mind::knowledge::MindGraph,
                &crate::agent::actions::ActiveActions,
                Option<&crate::agent::mind::conversation::InConversation>,
                Option<&crate::agent::inventory::EntityType>,
            ),
        ),
        (
            With<crate::agent::Agent>,
            With<super::rational::RationalBrain>,
        ),
    >,
    _world_map: Res<WorldMap>,
    action_registry: Res<crate::agent::actions::ActionRegistry>,
    _affordances: Query<(
        &GlobalTransform,
        Option<&crate::agent::affordance::Affordance>,
    )>,
    mut game_log: ResMut<crate::core::GameLog>,
    ontology: Res<crate::agent::mind::knowledge::Ontology>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
    mut brain_histories: Query<&mut BrainHistory>,
) {
    for (
        entity,
        name,
        mut brain_state,
        (mut plan_memory, cns),
        (physical, mut consciousness, drives),
        (emotions, body, personality, inventory),
        (_transform, visible, mind, active_actions, in_conversation, self_entity_type),
    ) in query.iter_mut()
    {
        // Cognitive tick cost: every arbitration burns a sliver of alertness.
        // Conscientious agents tolerate brain work better — they're wired
        // for it. The constant is calibrated per wallclock-second at the
        // 60 tps base rate, so divide by 60 to spread the cost across
        // every-tick arbitration.
        let tick_relief = personality.traits.conscientiousness
            * crate::constants::brains::cognition::CONSCIENTIOUSNESS_TICK_RELIEF;
        let tick_drain = crate::constants::brains::rational::COGNITIVE_TICK_ALERTNESS_DRAIN
            * (1.0 - tick_relief)
            / 60.0;
        consciousness.alertness = (consciousness.alertness - tick_drain).max(0.0);

        // 1. Gather proposals from all three brains

        let survival_context = SurvivalBrainContext {
            physical,
            cns,
            most_feared_entity: find_most_feared_visible_entity(visible, mind),
        };

        let survival_proposals = survival_brain_propose(
            survival_context,
            inventory,
            active_actions,
            &ontology,
            &action_registry,
        );

        let emotional_proposal = emotional_brain_propose(
            emotions,
            mind,
            visible,
            drives,
            in_conversation,
            self_entity_type.map(|t| t.0),
            cns,
            &action_registry,
        );

        // Rational brain now surfaces one proposal per Executing plan in
        // `PlanMemory`, so the output is variable-length and joins the
        let rational_proposals = rational_brain_propose(&plan_memory, cns, mind, &action_registry);

        // 2. Calculate brain powers, then apply history-based multiplier
        let base_powers = calculate_brain_powers(cns, &consciousness, emotions, personality);
        let powers = if let Ok(history) = brain_histories.get(entity) {
            BrainPowers {
                survival: base_powers.survival * history.power_multiplier(BrainType::Survival),
                emotional: base_powers.emotional * history.power_multiplier(BrainType::Emotional),
                rational: base_powers.rational * history.power_multiplier(BrainType::Rational),
            }
        } else {
            base_powers
        };

        // 3. Arbitrate - greedy multi-action admission across body channels.
        let mut proposals: Vec<Option<super::proposal::BrainProposal>> = Vec::new();
        proposals.extend(survival_proposals.into_iter().map(Some));
        proposals.push(emotional_proposal);
        proposals.extend(rational_proposals.into_iter().map(Some));
        let capacities = crate::agent::actions::ChannelCapacities::compute(
            body,
            Some(physical),
            Some(&*consciousness),
        );

        let result = arbitrate_parallel(&proposals, &powers, &capacities, &action_registry);
        let admitted = result.admitted;
        let rejected = result.rejected;

        // 3a. Channel-conflict losers from the rational brain demote
        // their backing Executing plan to Suspended (#338). The plan
        // sticks around in PlanMemory; its commitment decays each tick
        // until it drops back to Background, at which point the
        // commitment ladder can re-promote it once channels free up.
        for losing in &rejected {
            if losing.brain != BrainType::Rational {
                continue;
            }
            let action_type = losing.action.action_type;
            let mut to_suspend = Vec::new();
            for plan in plan_memory.in_state(PlanState::Executing) {
                if plan
                    .current()
                    .map(|a| a.action_type == action_type)
                    .unwrap_or(false)
                {
                    to_suspend.push(plan.id);
                }
            }
            for id in to_suspend {
                if let Some(plan) = plan_memory.get_mut(id) {
                    plan.state = PlanState::Suspended;
                    plan.last_touched = tick.current;
                }
            }
        }

        // 4. Update attribution map so outcome events can credit the right brain
        if let Ok(mut history) = brain_histories.get_mut(entity) {
            history.active.retain(|at, _| active_actions.contains(*at));
            for proposal in &admitted {
                history
                    .active
                    .insert(proposal.action.action_type, proposal.brain);
            }
        }

        // 5. Store for debugging/UI and execution
        brain_state.proposals = proposals.into_iter().flatten().collect();
        brain_state.powers = powers;

        if let Some(top) = admitted.first() {
            brain_state.winner = Some(top.brain);
            brain_state.chosen_actions = admitted
                .iter()
                .map(|p| {
                    // Carry urgency-modulated locomotion intensity (#339)
                    // from the proposal into the chosen template. Proposal
                    // urgency is on the 0-100 arbitration scale; the
                    // intensity formula expects a 0-1 "normalized drive"
                    // input so we divide before clamping.
                    let mut action = p.action.clone();
                    let urgency_unit = (p.urgency / 100.0).clamp(0.0, 1.0);
                    action.locomotion_intensity =
                        action.behavior.intensity.resolve_with_urgency(urgency_unit);
                    action
                })
                .collect();

            // Log every admitted action so multi-channel decisions are visible.
            for proposal in &admitted {
                let brain_name = match proposal.brain {
                    super::proposal::BrainType::Survival => "SURVIVAL",
                    super::proposal::BrainType::Emotional => "EMOTIONAL",
                    super::proposal::BrainType::Rational => "RATIONAL",
                };

                game_log.brain(
                    name.as_str(),
                    brain_name,
                    &proposal.action.name,
                    &proposal.reasoning,
                    Some(entity),
                );
            }
        } else {
            brain_state.winner = None;
            brain_state.chosen_actions.clear();
        }

        sim_events.write(crate::agent::events::SimEvent::Decision {
            agent: entity,
            tick: tick.current,
            winner: brain_state.winner,
            chosen_actions: brain_state
                .chosen_actions
                .iter()
                .map(|a| a.action_type)
                .collect(),
            powers,
            proposals: std::sync::Arc::new(brain_state.proposals.clone()),
        });
    }
}
