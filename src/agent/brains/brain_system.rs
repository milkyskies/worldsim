use super::arbitration::{arbitrate, calculate_brain_powers};
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
                &crate::agent::inventory::Inventory,
            ),
            // Context
            (
                &Transform,
                &VisibleObjects,
                &crate::agent::mind::knowledge::MindGraph,
                &crate::agent::actions::ActionState,
                Option<&crate::agent::mind::conversation::InConversation>,
            ),
        ),
        With<crate::agent::Agent>,
    >,
    world_map: Res<WorldMap>,
    action_registry: Res<crate::agent::actions::ActionRegistry>,
    mut affordances: Query<(
        &GlobalTransform,
        Option<&crate::agent::affordance::Affordance>,
    )>,
    mut game_log: ResMut<crate::core::GameLog>,
    ontology: Res<crate::agent::mind::knowledge::Ontology>,
) {
    for (
        entity,
        name,
        mut brain_state,
        (rational_brain, cns),
        (physical, consciousness, _drives),
        (emotions, body, personality, inventory),
        (transform, visible, mind, activity, in_conversation),
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
            activity,
            &ontology,
            &action_registry,
        );

        let emotional_proposal = emotional_brain_propose(
            emotions,
            mind,
            visible,
            _drives,
            &action_registry,
            in_conversation,
        );

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

        // 3. Arbitrate
        let proposals = [survival_proposal, emotional_proposal, rational_proposal];
        let winner = arbitrate(&proposals, &powers);

        // 4. Store for debugging/UI and execution
        brain_state.proposals = proposals.into_iter().flatten().collect();
        brain_state.powers = powers;

        if let Some((winner_brain, proposal)) = winner {
            brain_state.winner = Some(winner_brain);
            brain_state.chosen_action = Some(proposal.action.clone());

            // Log brain decision for debugging
            let brain_name = match winner_brain {
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
        } else {
            brain_state.winner = None;
            brain_state.chosen_action = None;
        }
    }
}
