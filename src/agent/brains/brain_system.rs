//! Three-brains orchestration: runs all brain systems and arbitrates between their proposals each tick.
//!
//! Reads: PhysicalNeeds, Consciousness, PsychologicalDrives, EmotionalState, Body, Personality, ItemSlots, VisibleObjects, MindGraph, ActiveActions, WorldMap, BrainHistory, PlanMemory
//! Writes: BrainState (chosen action, winner, proposals, powers), BrainHistory (active attributions), SimEvent::Decision
//! Upstream: survival/emotional/rational brain modules, arbitration, perception, knowledge
//! Downstream: nervous_system::cns (executes the chosen action), SimEvent consumers

use super::arbitration::{arbitrate_parallel, calculate_brain_powers};
use super::emotional::emotional_brain_propose;
use super::history::BrainHistory;
use super::plan_memory::{PlanMemory, PlanState};
use super::proposal::{BrainPowers, BrainState, BrainType};
use super::rational::rational_brain_propose;
use super::social_initiation::SocialInitiationCooldowns;
use super::survival::{SurvivalBrainContext, survival_brain_propose};
use crate::agent::biology::body::{Body, TagChannelMapping};
use crate::agent::body::needs::{Consciousness, PhysicalNeeds, PsychologicalDrives};
use crate::agent::events::SimEventKind;
use crate::agent::mind::knowledge::Concept;
use crate::agent::mind::perception::VisibleObjects;
use crate::agent::nervous_system::cns::CentralNervousSystem;
use crate::agent::psyche::emotions::EmotionalState;
use crate::agent::psyche::personality::Personality;
use crate::world::map::WorldMap;
use bevy::prelude::*;

/// Per-tick alertness drain from cognitive load. Runs every tick for
/// every agent regardless of whether arbitration fired, so agents that
/// don't get woken still pay the steady metabolic cost of being awake.
pub fn tick_cognitive_drain(
    mut query: Query<
        (&mut Consciousness, &Personality),
        (
            With<crate::agent::Agent>,
            With<super::rational::RationalBrain>,
        ),
    >,
) {
    for (mut consciousness, personality) in query.iter_mut() {
        let tick_relief = personality.traits.conscientiousness
            * crate::constants::brains::cognition::CONSCIENTIOUSNESS_TICK_RELIEF;
        let tick_drain = crate::constants::brains::rational::COGNITIVE_TICK_ALERTNESS_DRAIN
            * (1.0 - tick_relief)
            / 60.0;
        consciousness.alertness = (consciousness.alertness - tick_drain).max(0.0);
    }
}

/// Emit `AgentStateHash` every tick for every agent, regardless of
/// whether arbitration fired. This is a determinism-debugging signal,
/// not a decision signal, so it can't depend on the wakeup gate.
pub fn emit_agent_state_hash(
    tick: Res<crate::core::tick::TickCount>,
    query: Query<
        (Entity, &Transform, &CentralNervousSystem, &PlanMemory),
        (
            With<crate::agent::Agent>,
            With<super::rational::RationalBrain>,
        ),
    >,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
) {
    for (entity, transform, cns, plan_memory) in query.iter() {
        let hash =
            compute_agent_state_hash(entity, transform.translation.truncate(), cns, plan_memory);
        sim_events.write(crate::agent::events::SimEvent::single(
            tick.current,
            entity,
            SimEventKind::AgentStateHash {
                agent: entity,
                hash,
            },
        ));
    }
}

/// Collect proposals from all three brains and pick the admitted action
/// set for agents whose situation changed since the last brain run
/// (signalled via `PendingBrainWakeups`). Agents with no pending wakeup
/// keep the BrainState from their previous arbitration — the body
/// executes the same winner until something interesting happens to fire
/// a fresh wakeup. Drains the pending set so the next 10 Hz brain run
/// sees only newly-buffered wakeups.
pub fn arbitrate_every_tick(
    tick: Res<crate::core::tick::TickCount>,
    mut pending: ResMut<super::wakeup::PendingBrainWakeups>,
    mut query: Query<
        (
            Entity,
            &Name,
            &mut BrainState,
            // Brains
            (&mut PlanMemory, &CentralNervousSystem),
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
                Option<&crate::agent::engagement::Engaged>,
                Option<&crate::agent::inventory::EntityType>,
            ),
        ),
        (
            With<crate::agent::Agent>,
            With<super::rational::RationalBrain>,
        ),
    >,
    world_map: Res<WorldMap>,
    action_registry: Res<crate::agent::actions::ActionRegistry>,
    _affordances: Query<(
        &GlobalTransform,
        Option<&crate::agent::affordance::Affordance>,
        Option<&crate::agent::Dead>,
    )>,
    mut game_log: ResMut<crate::core::GameLog>,
    ontology: Res<crate::agent::mind::knowledge::Ontology>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
    mut brain_histories: Query<&mut BrainHistory>,
    mapping: Res<TagChannelMapping>,
    fields: Res<crate::world::field_grid_plugin::FieldGrids>,
    all_transforms: Query<(&Transform, Option<&crate::agent::inventory::EntityType>)>,
    all_bodies: Query<&Body>,
    // Bundled into one slot — Bevy's SystemParam tuple impl caps the
    // function at 16 parameters and adding the Engaged / cooldowns
    // queries individually would push us over.
    side_queries: (
        Query<&crate::agent::Cornered>,
        Query<&crate::agent::Dazed>,
        Query<&crate::agent::engagement::Engaged>,
        Query<&SocialInitiationCooldowns>,
    ),
) {
    let (cornered_query, dazed_query, engaged_query, social_cooldowns_query) = side_queries;
    let woken = pending.drain();

    for (
        entity,
        name,
        mut brain_state,
        (mut plan_memory, cns),
        (physical, consciousness, drives),
        (emotions, body, personality, inventory),
        (transform, visible, mind, active_actions, engaged, self_entity_type),
    ) in query.iter_mut()
    {
        // Skip agents whose situation didn't change this tick — their
        // existing BrainState is still the correct decision.
        if !woken.contains(&entity) {
            continue;
        }

        // Dazed agents skip arbitration entirely — heavy head trauma
        // suppresses the next decision cycle. The component clears on
        // its own when `until_tick` passes.
        if dazed_query.contains(entity) {
            continue;
        }

        // 1. Gather proposals from all three brains

        let agent_pos = transform.translation.truncate();
        // Compute once per agent — survival context and threat appraisal
        // both need the closest visible Dangerous entity.
        let closest_dangerous =
            super::emotional::find_closest_dangerous(visible, mind, &all_transforms, agent_pos);

        let survival_context = SurvivalBrainContext {
            physical,
            cns,
            most_feared_entity: closest_dangerous.map(|(e, _)| e),
            pos: agent_pos,
            world_map: &world_map,
        };

        let survival_proposals = survival_brain_propose(
            survival_context,
            inventory,
            active_actions,
            &ontology,
            &action_registry,
        );

        // Pre-resolve visible-entity positions, concept types, and
        // Engaged status in one pass over the Query, parallel-
        // indexed so brain proposers can iterate them together without
        // re-querying ECS or the MindGraph ontology per visible entity.
        let mut visible_positions: Vec<(Entity, Vec2)> = Vec::with_capacity(visible.entities.len());
        let mut visible_types: Vec<Option<Concept>> = Vec::with_capacity(visible.entities.len());
        let mut visible_engaged_converse: Vec<bool> = Vec::with_capacity(visible.entities.len());
        for &e in &visible.entities {
            if let Ok((t, et)) = all_transforms.get(e) {
                visible_positions.push((e, t.translation.truncate()));
                visible_types.push(et.map(|et| et.0));
                visible_engaged_converse.push(
                    engaged_query
                        .get(e)
                        .map(|eng| eng.kind == crate::agent::engagement::EngagementKind::Converse)
                        .unwrap_or(false),
                );
            }
        }

        let social_cooldowns = social_cooldowns_query.get(entity).ok();

        let closest_threat = closest_dangerous.map(|(e, pos)| super::emotional::ClosestThreat {
            entity: e,
            pos,
            type_concept: all_transforms
                .get(e)
                .ok()
                .and_then(|(_, et)| et)
                .map(|et| et.0),
            body: all_bodies.get(e).ok(),
        });
        let cornered = cornered_query.contains(entity);

        let emotional_inputs = super::emotional::EmotionalInputs {
            emotions,
            mind,
            visible,
            visible_positions: &visible_positions,
            visible_types: &visible_types,
            physical,
            drives,
            engaged,
            self_concept: self_entity_type.map(|t| t.0),
            agent_pos,
            fields: &fields,
            cns,
            action_registry: &action_registry,
            personality: Some(&personality.traits),
            body,
            cornered,
            closest_threat,
            visible_engaged_converse: &visible_engaged_converse,
            social_cooldowns,
            current_tick: tick.current,
        };
        let emotional_proposal = emotional_brain_propose(&emotional_inputs);

        // Rational brain now surfaces one proposal per Executing plan in
        // `PlanMemory`, so the output is variable-length and joins the
        let rational_proposals = rational_brain_propose(&plan_memory, cns, mind, &action_registry);

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

        // 3. Arbitrate - greedy multi-action admission across body channels.
        let mut proposals: Vec<Option<super::proposal::BrainProposal>> = Vec::new();
        proposals.extend(survival_proposals.into_iter().map(Some));
        proposals.push(emotional_proposal);
        proposals.extend(rational_proposals.into_iter().map(Some));
        let capacities = crate::agent::actions::ChannelCapacities::compute(
            body,
            Some(physical),
            Some(consciousness),
            &mapping,
        );

        let result = arbitrate_parallel(&proposals, &powers, &capacities, &action_registry);
        let rejected = result.rejected;

        // Action-prep pass: for each admitted proposal whose action has
        // a `location_preference` hook, sample the local tile
        // neighborhood — if a meaningfully-better tile exists, swap the
        // proposal for a Walk toward it so the action fires in a good
        // spot next cycle.
        let pref_ctx = emotional_inputs.preference_context();
        let admitted: Vec<super::proposal::BrainProposal> = result
            .admitted
            .into_iter()
            .map(|p| super::drift::apply_location_preference(p, &pref_ctx, &action_registry))
            .collect();

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

            // Fingerprint of this tick's admitted (brain, action) set. Log
            // only when it differs from the previous tick — otherwise the
            // per-tick "Still tired..." line floods the log while an agent
            // continues the same Sleep/Rest/Converse for thousands of ticks.
            let current_fingerprint: Vec<(super::proposal::BrainType, String)> = admitted
                .iter()
                .map(|p| (p.brain, p.action.name.clone()))
                .collect();
            let changed = brain_state.last_logged.as_ref() != Some(&current_fingerprint);
            if changed {
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
                brain_state.last_logged = Some(current_fingerprint);
            }
        } else {
            brain_state.winner = None;
            brain_state.chosen_actions.clear();
            brain_state.last_logged = None;
        }

        let urgencies_snapshot: Vec<crate::agent::nervous_system::urgency::Urgency> =
            cns.urgencies.clone();

        sim_events.write(crate::agent::events::SimEvent::single(
            tick.current,
            entity,
            SimEventKind::Decision {
                agent: entity,
                winner: brain_state.winner,
                chosen_actions: brain_state
                    .chosen_actions
                    .iter()
                    .map(|a| a.action_type)
                    .collect(),
                powers,
                proposals: std::sync::Arc::new(brain_state.proposals.clone()),
                urgencies: urgencies_snapshot,
            },
        ));
    }
}

/// FxHash of (tile_x, tile_y, urgency_sources_sorted, plan_ids_sorted).
/// Used to diff two runs and find the tick of first non-determinism divergence.
fn compute_agent_state_hash(
    _entity: Entity,
    pos: Vec2,
    cns: &crate::agent::nervous_system::cns::CentralNervousSystem,
    plan_memory: &super::plan_memory::PlanMemory,
) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let tile_x = (pos.x / crate::world::map::TILE_SIZE).floor() as i32;
    let tile_y = (pos.y / crate::world::map::TILE_SIZE).floor() as i32;

    let mut urgency_sources: Vec<u8> = cns.urgencies.iter().map(|u| u.source as u8).collect();
    urgency_sources.sort_unstable();

    let mut plan_ids: Vec<u64> = plan_memory.plans.iter().map(|p| p.id.0).collect();
    plan_ids.sort_unstable();

    let mut h = DefaultHasher::new();
    tile_x.hash(&mut h);
    tile_y.hash(&mut h);
    urgency_sources.hash(&mut h);
    plan_ids.hash(&mut h);
    h.finish()
}
