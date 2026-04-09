//! Three-brains orchestration: runs all brain systems and arbitrates between their proposals each tick.
//!
//! Reads: PhysicalNeeds, Consciousness, PsychologicalDrives, EmotionalState, Body, Personality, ItemSlots, VisibleObjects, MindGraph, ActiveActions, WorldMap
//! Writes: BrainState (chosen action, winner, proposals, powers), SimEvent::Decision
//! Upstream: survival/emotional/rational brain modules, arbitration, perception, knowledge
//! Downstream: nervous_system::cns (executes the chosen action), SimEvent consumers

use super::arbitration::{arbitrate_parallel, calculate_brain_powers};
use super::emotional::emotional_brain_propose;
use super::proposal::BrainState;
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
) {
    for (
        entity,
        name,
        mut brain_state,
        (rational_brain, cns),
        (physical, consciousness, _drives),
        (emotions, body, personality, inventory),
        (transform, visible, mind, active_actions),
    ) in query.iter_mut()
    {
        // Staggered: heavy thinking runs every N ticks, offset by entity ID
        if !tick.should_run(entity, ns_config.thinking_interval) {
            continue;
        }

        // 1. Gather proposals from all three brains

        let survival_context = SurvivalBrainContext {
            physical,
            consciousness,
            emotions,
            body,
        };

        let survival_proposal = survival_brain_propose(
            survival_context,
            inventory,
            visible,
            brain_state.winner,
            active_actions,
            &ontology,
            &action_registry,
        );

        let emotional_proposal = emotional_brain_propose(emotions, mind, visible, &action_registry);

        // Pass None for state since RationalBrain doesn't use it anymore
        // or update rational_brain_propose to strictly take only what it needs
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
        );

        // 2. Calculate brain powers
        let powers = calculate_brain_powers(physical, consciousness, body, emotions, personality);

        // 3. Arbitrate - greedy multi-action admission across body channels
        let proposals = [survival_proposal, emotional_proposal, rational_proposal];
        let capacities = crate::agent::actions::ChannelCapacities::compute(body, Some(physical));
        let admitted = arbitrate_parallel(&proposals, &powers, &capacities, &action_registry);

        // 4. Store for debugging/UI and execution
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
