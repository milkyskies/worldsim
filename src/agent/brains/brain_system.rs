//! Three-brains orchestration: runs all brain systems and arbitrates between their proposals each tick.
//!
//! Reads: PhysicalNeeds, Consciousness, PsychologicalDrives, EmotionalState, Body, Personality, ItemSlots, VisibleObjects, MindGraph, ActiveActions, WorldMap, BrainHistory, ActivePlans
//! Writes: BrainState (chosen action, winner, proposals, powers), BrainHistory (active attributions), ActivePlans (commitment tracking), SimEvent::Decision, SimEvent::PlanAbandoned
//! Upstream: survival/emotional/rational brain modules, arbitration, perception, knowledge
//! Downstream: nervous_system::cns (executes the chosen action), SimEvent consumers

use super::active_plan::{ActivePlans, PlanOwner};
use super::arbitration::{
    CommitmentContext, arbitrate_parallel_with_commitment, calculate_brain_powers,
};
use super::emotional::{emotional_brain_propose, find_most_feared_visible_entity};
use super::history::BrainHistory;
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

/// The Three Brains System
///
/// Orchestrates the three brains (Survival, Emotional, Rational) and arbitrates
/// between their proposals to determine what action the agent takes.
pub fn three_brains_system(
    tick: Res<crate::core::tick::TickCount>,
    ns_config: Res<crate::agent::nervous_system::config::NervousSystemConfig>,
    mut query: Query<
        (
            Entity,
            &Name,
            &mut BrainState,
            // Brains
            (&super::rational::RationalBrain, &CentralNervousSystem),
            // Needs
            (&PhysicalNeeds, &Consciousness, Option<&PsychologicalDrives>),
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
        With<crate::agent::Agent>,
    >,
    world_map: Res<WorldMap>,
    action_registry: Res<crate::agent::actions::ActionRegistry>,
    affordances: Query<(
        &GlobalTransform,
        Option<&crate::agent::affordance::Affordance>,
    )>,
    mut game_log: ResMut<crate::core::GameLog>,
    ontology: Res<crate::agent::mind::knowledge::Ontology>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
    mut brain_histories: Query<&mut BrainHistory>,
    mut active_plans_query: Query<&mut ActivePlans>,
) {
    for (
        entity,
        name,
        mut brain_state,
        (rational_brain, cns),
        (physical, consciousness, drives),
        (emotions, body, personality, inventory),
        (transform, visible, mind, active_actions, in_conversation, self_entity_type),
    ) in query.iter_mut()
    {
        // Staggered: heavy thinking runs every N ticks, offset by entity ID
        if !tick.should_run(entity, ns_config.thinking_interval) {
            continue;
        }

        // 1. Gather proposals from all three brains

        let survival_context = SurvivalBrainContext {
            physical,
            cns,
            most_feared_entity: find_most_feared_visible_entity(visible, mind),
        };

        let survival_proposal = survival_brain_propose(
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
            &action_registry,
        );

        let capacities = crate::agent::actions::ChannelCapacities::compute(body, Some(physical));
        let rational_proposal = rational_brain_propose(
            rational_brain,
            cns,
            inventory,
            transform,
            mind,
            visible,
            &world_map,
            &action_registry,
            &affordances,
            &capacities,
        );

        // 2. Calculate brain powers, then apply history-based multiplier
        let base_powers = calculate_brain_powers(cns, consciousness, emotions, personality);
        let powers = if let Ok(history) = brain_histories.get(entity) {
            BrainPowers {
                survival: base_powers.survival * history.power_multiplier(BrainType::Survival),
                emotional: base_powers.emotional * history.power_multiplier(BrainType::Emotional),
                rational: base_powers.rational * history.power_multiplier(BrainType::Rational),
            }
        } else {
            base_powers
        };

        // 3. Arbitrate - greedy multi-action admission across body channels
        //    with plan commitment inertia (#166)
        let proposals = [survival_proposal, emotional_proposal, rational_proposal];
        let capacities = crate::agent::actions::ChannelCapacities::compute(body, Some(physical));

        let commitment_ctx = active_plans_query
            .get(entity)
            .ok()
            .map(|plans| CommitmentContext {
                active_plans: plans,
                conscientiousness: personality.traits.conscientiousness,
                current_tick: tick.current,
            });
        let admitted = arbitrate_parallel_with_commitment(
            &proposals,
            &powers,
            &capacities,
            &action_registry,
            commitment_ctx.as_ref(),
        );

        // 4. Update active plans: activate winning proposals, decay stalled ones
        if let Ok(mut plans) = active_plans_query.get_mut(entity) {
            let admitted_actions: Vec<_> = admitted.iter().map(|p| p.action.action_type).collect();

            // Activate newly admitted proposals
            for proposal in &admitted {
                if proposal.intent != super::proposal::Intent::None {
                    plans.activate(
                        PlanOwner::Brain(proposal.brain),
                        proposal.intent,
                        proposal.action.action_type,
                        tick.current,
                    );
                }
            }

            // Decay stalled plans and emit PlanAbandoned events
            let abandoned = plans.decay_stalled_plans(&admitted_actions, tick.current);
            for (intent, action) in abandoned {
                sim_events.write(crate::agent::events::SimEvent::PlanAbandoned {
                    agent: entity,
                    tick: tick.current,
                    action,
                    intent,
                });
            }
        }

        // 5. Update attribution map so outcome events can credit the right brain
        if let Ok(mut history) = brain_histories.get_mut(entity) {
            history.active.retain(|at, _| active_actions.contains(*at));
            for proposal in &admitted {
                history
                    .active
                    .insert(proposal.action.action_type, proposal.brain);
            }
        }

        // 6. Store for debugging/UI and execution
        brain_state.proposals = proposals.into_iter().flatten().collect();
        brain_state.powers = powers;

        if let Some(top) = admitted.first() {
            brain_state.winner = Some(top.brain);
            brain_state.chosen_actions = admitted.iter().map(|p| p.action.clone()).collect();

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
