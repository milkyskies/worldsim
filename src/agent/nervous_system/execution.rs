//! Parallel action execution - ticks every running action independently.
//!
//! Reads: BrainState (chosen actions), PhysicalNeeds, Inventory, WorldMap, Body, Skills, Phenotype
//! Writes: ActiveActions, PhysicalNeeds, Inventory, TargetPosition, ActionOutcomeEvent, SimEvent
//! Upstream: brains::arbitration (BrainState), actions::registry (Action definitions)
//! Downstream: mind::belief_updater (ActionOutcomeEvent), ui (GameLog), SimEvent consumers

use crate::agent::TargetPosition;
use crate::agent::actions::ActionType;
use crate::agent::actions::channel::ChannelCapacities;
use crate::agent::actions::registry::{
    ActionContext, ActionKind, ActionRegistry, ActionState, ActiveActions,
};
use crate::agent::biology::body::Body;
use crate::agent::body::genetics::phenotype::Phenotype;
use crate::agent::body::needs::{Consciousness, PhysicalNeeds};
use crate::agent::body::species::SpeciesProfile;
use crate::agent::brains::proposal::BrainState;
use crate::agent::events::{ActionOutcome, ActionOutcomeEvent, NeedSatisfaction};
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::{MindGraph, Node, Predicate, Value};
use crate::agent::movement::{
    ARRIVAL_THRESHOLD, MoveResult, calculate_speed, effective_intensity,
    intensity_speed_multiplier, move_toward,
};
use crate::core::SimRng;
use crate::core::tick::TickCount;
use crate::ui::hud::GameLog;
use crate::world::map::{CHUNK_SIZE, TILE_SIZE, WorldMap};
use bevy::prelude::*;
use rand::Rng;

/// Admit brain-chosen actions into the parallel running set.
///
/// For each chosen action:
/// 1. If already running with the same target, leave it alone.
/// 2. If it would hard-conflict with the current load, attempt to preempt
///    lower-intensity interruptible actions until it fits, otherwise drop it.
/// 3. Otherwise insert it - soft conflicts are allowed and degrade later.
///
/// Sleep is special: because Sleep occupies `FullBody` at intensity 1.0, every
/// other body-using action hard-conflicts with it. The preemption pass clears
/// them out, satisfying the "Sleep clears all slots" acceptance criterion.
pub fn start_actions(
    registry: Res<ActionRegistry>,
    tick: Res<TickCount>,
    world_map: Res<WorldMap>,
    mut sim_rng: ResMut<SimRng>,
    mut game_log: ResMut<GameLog>,
    mut agents: Query<(
        Entity,
        &Name,
        &Transform,
        &mut TargetPosition,
        &mut ActiveActions,
        &BrainState,
        &MindGraph,
        &ItemSlots,
        Option<&Body>,
        Option<&PhysicalNeeds>,
    )>,
    entity_transforms: Query<&GlobalTransform>,
    mut outcome_events: MessageWriter<ActionOutcomeEvent>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
) {
    for (
        entity,
        name,
        transform,
        mut target,
        mut active,
        brain_state,
        mind,
        inventory,
        body,
        physical,
    ) in agents.iter_mut()
    {
        // Snapshot capacities once per agent so the channel methods don't
        // recompute incapacitation/exhaustion math per requirement check.
        let capacities = ChannelCapacities::compute(body, physical);

        for action_template in &brain_state.chosen_actions {
            let wanted_action = action_template.action_type;

            // Already running this action - leave it alone (no restart).
            if active.contains(wanted_action) {
                continue;
            }

            // Sleep locks the whole agent — short-circuit everything except
            // WakeUp while it's active. We can't enforce this through the
            // channel system alone: capability-per-species means a "1.0 on
            // every active channel" Sleep declaration would refuse to start
            // on bodies whose per-channel capacity doesn't match the human
            // default (a wolf's 0.4 Manipulation can never satisfy
            // Manipulation 1.0 through the admission math). Sleep declares
            // FullBody 1.0 to gate vs. other whole-body actions, and this
            // branch gates it vs. the rest of the catalog.
            if active.contains(ActionType::Sleep) && wanted_action != ActionType::WakeUp {
                continue;
            }

            let Some(action_def) = registry.get(wanted_action) else {
                warn!(
                    "Agent {:?} ({}) wanted action {:?} which is not in the registry",
                    entity, name, wanted_action
                );
                continue;
            };

            // Runtime can-start check.
            let ctx = ActionContext {
                inventory,
                mind,
                world_map: &world_map,
                target_entity: action_template.target_entity,
                target_position: action_template.target_position,
                agent_position: transform.translation.truncate(),
            };

            if let Err(reason) = action_def.can_start(&ctx) {
                game_log.log_debug(format!(
                    "{} cannot start {:?}: {:?}",
                    name.as_str(),
                    wanted_action,
                    reason
                ));
                sim_events.write(crate::agent::events::SimEvent::ActionFailed {
                    agent: entity,
                    tick: tick.current,
                    action: wanted_action,
                    reason: reason.clone(),
                });
                outcome_events.write(ActionOutcomeEvent {
                    actor: entity,
                    outcome: ActionOutcome::Failed {
                        action: wanted_action,
                        target: action_template.target_entity,
                        reason,
                    },
                });
                continue;
            }

            // Resolve hard conflicts by preempting interruptible actions.
            let requirements = action_def.body_channels();
            let before_preempt: Vec<ActionType> = active.iter().map(|a| a.action_type).collect();

            // #223: Two ActionKind::Movement actions cannot coexist (only one
            // transform). Displace any existing interruptible Movement before
            // the channel-based preemption pass runs. Most-recent wins. The
            // event emission loop below catches the removal via the
            // `before_preempt` snapshot.
            preempt_existing_movement(&mut active, &registry, &mut target, action_def.kind());

            if !preempt_to_make_room(
                &mut active,
                &registry,
                requirements,
                &capacities,
                &mut target,
            ) {
                game_log.log_debug(format!(
                    "{} could not start {:?}: hard conflict with uninterruptible actions",
                    name.as_str(),
                    wanted_action
                ));
                continue;
            }

            // Emit preemption events for any actions that were removed.
            for preempted in &before_preempt {
                if !active.contains(*preempted) {
                    sim_events.write(crate::agent::events::SimEvent::ActionPreempted {
                        agent: entity,
                        tick: tick.current,
                        preempted_action: *preempted,
                    });
                }
            }

            // Build the new ActionState for this slot.
            let mut new_state = ActionState::new(wanted_action, tick.current);

            if let Some(te) = action_template.target_entity {
                new_state = new_state.with_target_entity(te);
            }
            if let Some(tp) = action_template.target_position {
                new_state = new_state.with_target_position(tp);
            }

            if let ActionKind::Timed { duration_ticks } = action_def.kind() {
                new_state = new_state.with_duration(duration_ticks);
            }

            // #339: propagate the brain's urgency-modulated intensity into
            // the runtime action state. Non-movement actions carry 0.0 and
            // are unaffected.
            if action_template.locomotion_intensity > 0.0 {
                new_state =
                    new_state.with_locomotion_intensity(action_template.locomotion_intensity);
            }

            if matches!(action_def.kind(), ActionKind::Movement) {
                let pos = transform.translation.truncate();
                let rng = sim_rng.inner_mut();
                let new_target = match wanted_action {
                    ActionType::Explore => {
                        find_explore_target(pos, mind, &world_map, tick.current, rng)
                    }
                    ActionType::Wander => {
                        pick_random_walkable_target(pos, &world_map, 10.0..30.0, rng)
                    }
                    ActionType::Graze => pick_random_grass_target(
                        pos,
                        &world_map,
                        crate::constants::actions::graze::DRIFT_RANGE_MIN
                            ..crate::constants::actions::graze::DRIFT_RANGE_MAX,
                        rng,
                    )
                    .or(action_template.target_position),
                    ActionType::Flee => {
                        if let Some(threat) = action_template.target_entity
                            && let Ok(threat_t) = entity_transforms.get(threat)
                        {
                            let threat_pos = threat_t.translation().truncate();
                            let away = (pos - threat_pos).normalize_or_zero();
                            pick_flee_target(pos, away, &world_map, rng)
                        } else {
                            pick_random_walkable_target(pos, &world_map, 30.0..60.0, rng)
                        }
                    }
                    ActionType::Walk => action_template.target_position.or_else(|| {
                        // Fall back to the target entity's current position so
                        // brains can propose Walk { target_entity, target_position: None }
                        // for "approach this thing" behaviour (#260 flock
                        // seeking is the first user). Same lookup pattern as
                        // InitiateConversation below.
                        action_template
                            .target_entity
                            .and_then(|partner| entity_transforms.get(partner).ok())
                            .map(|t| t.translation().truncate())
                    }),
                    ActionType::InitiateConversation => {
                        // Walk toward the partner's current position. The
                        // CommunicationPlugin intercepts arrival at
                        // CONVERSATION_RANGE before the standard 2px arrival
                        // check fires.
                        action_template.target_entity.and_then(|partner| {
                            entity_transforms
                                .get(partner)
                                .ok()
                                .map(|t| t.translation().truncate())
                        })
                    }
                    // For any other movement action, honour the brain's target rather
                    // than silently discarding it. If the brain left the position
                    // unspecified, this returns None and the action completes
                    // immediately (which is the correct degenerate behaviour).
                    _ => action_template.target_position,
                };

                if let Some(tp) = new_target {
                    new_state = new_state.with_target_position(tp);
                    target.0 = Some(tp);
                }
            }

            sim_events.write(crate::agent::events::SimEvent::ActionStarted {
                agent: entity,
                tick: tick.current,
                action: wanted_action,
                target: action_template.target_entity,
            });

            active.insert(new_state);

            if let Some(msg) = action_def.start_log() {
                game_log.action(name.as_str(), msg, None, Some(entity));
            }
        }
    }
}

/// Tick every running action independently.
pub fn tick_actions(
    mut commands: Commands,
    registry: Res<ActionRegistry>,
    tick: Res<TickCount>,
    world_map: Res<WorldMap>,
    mut game_log: ResMut<GameLog>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
    mut outcome_events: MessageWriter<ActionOutcomeEvent>,
    mut agents: Query<(
        Entity,
        &Name,
        &mut Transform,
        &mut TargetPosition,
        &mut ActiveActions,
        &mut PhysicalNeeds,
        &mut ItemSlots,
        Option<&mut crate::agent::body::needs::PsychologicalDrives>,
        Option<&Body>,
        &crate::agent::mind::knowledge::MindGraph,
        Option<&crate::agent::skills::Skills>,
        Option<&SpeciesProfile>,
        Option<&Phenotype>,
    )>,
    mut target_inventories: Query<&mut ItemSlots, Without<PhysicalNeeds>>,
    living_entities: Query<()>,
) {
    let current_tick = tick.current;

    for (
        entity,
        name,
        mut transform,
        mut target_pos,
        mut active,
        mut physical,
        mut inventory,
        mut drives,
        body,
        mind,
        skills,
        species,
        phenotype,
    ) in agents.iter_mut()
    {
        // Snapshot the load and capacities at the start of the tick. Capacities
        // freeze the start-of-tick stamina so degradation doesn't compound as
        // physical needs are mutated by per-action effects.
        let load = active.channel_load(&registry);
        let capacities = ChannelCapacities::compute(body, Some(&*physical));

        let mut completed_types: Vec<ActionType> = Vec::new();
        let mut target_gone_types: Vec<ActionType> = Vec::new();
        // Movement actions whose straight-line step hit a non-walkable tile
        // this frame. They're still removed from the active set (like any
        // completed action) but processed as failures instead of successes:
        // on_complete is skipped and an ActionOutcome::Failed is emitted
        // carrying the target tile so the belief updater can mark it
        // Unreachable.
        let mut path_blocked_types: Vec<(ActionType, (i32, i32))> = Vec::new();

        for action_state in active.iter_mut() {
            let action_type = action_state.action_type;

            // If this action targets an entity that has since been despawned, cancel it
            // immediately rather than ticking it to completion (where on_complete would
            // silently do nothing or — without this guard — potentially panic).
            if let Some(target) = action_state.target_entity
                && living_entities.get(target).is_err()
            {
                target_gone_types.push(action_type);
                continue;
            }

            let Some(action_def) = registry.get(action_type) else {
                continue;
            };

            let channels = action_def.body_channels();
            let degradation = load.degradation_factor(channels, &capacities);

            let completed = match action_def.kind() {
                ActionKind::Instant => true,
                ActionKind::Timed { duration_ticks } => {
                    if duration_ticks == u32::MAX || action_state.ticks_remaining == u32::MAX {
                        // Indefinite (Sleep, Idle) - never autocompletes here.
                        false
                    } else {
                        // Deterministic fractional progress: each tick contributes
                        // `degradation` units, and `ticks_remaining` decrements
                        // every time the accumulator crosses 1.0. Replay-safe.
                        action_state.progress_accumulator += degradation;
                        while action_state.progress_accumulator >= 1.0
                            && action_state.ticks_remaining > 0
                        {
                            action_state.progress_accumulator -= 1.0;
                            action_state.ticks_remaining -= 1;
                        }
                        action_state.ticks_remaining == 0
                    }
                }
                ActionKind::Movement => match action_state.target_position {
                    None => true,
                    Some(target_position) => {
                        let current_pos = transform.translation.truncate();
                        if current_pos.distance(target_position) < ARRIVAL_THRESHOLD {
                            // Snap to exact target so perceived tile matches Walk effect.
                            transform.translation.x = target_position.x;
                            transform.translation.y = target_position.y;
                            true
                        } else {
                            let ticks =
                                current_tick.saturating_sub(action_state.last_movement_tick);
                            if ticks > 0 {
                                action_state.last_movement_tick = current_tick;

                                // Intensity-based locomotion (#339). Brain sets a
                                // desired intensity on the ActionState; the body
                                // caps it if stamina reserves are empty. Speed and
                                // stamina drain both scale from the *effective*
                                // intensity so a sprinted-out agent slows down and
                                // a brisk walk costs more than a stroll.
                                let desired = if action_state.locomotion_intensity > 0.0 {
                                    action_state.locomotion_intensity
                                } else {
                                    action_type.default_locomotion_intensity()
                                };
                                let effective = effective_intensity(desired, &physical.stamina);
                                let intensity_mult = intensity_speed_multiplier(effective);

                                // Apply species base speed and individual genetic multiplier.
                                // Phenotype.speed is 1.0 for an average individual; faster
                                // or slower individuals deviate from the species baseline.
                                let species_speed = species.map(|s| s.base_speed).unwrap_or(1.0);
                                let genetic_speed = phenotype.map(|p| p.speed).unwrap_or(1.0);
                                let speed = calculate_speed(physical.stamina.aerobic, None)
                                    * species_speed
                                    * genetic_speed
                                    * degradation
                                    * intensity_mult;

                                // Stamina drain routes through the intensity-aware
                                // drain formula from #331. Non-movement activities
                                // still drain via activity_effects.
                                let dt_sec = ticks as f32 * tick.dt();
                                physical.stamina.drain(effective, dt_sec);

                                match move_toward(
                                    current_pos,
                                    target_position,
                                    speed,
                                    ticks,
                                    &world_map,
                                    &mut transform,
                                ) {
                                    MoveResult::Moving => false,
                                    MoveResult::Arrived => true,
                                    MoveResult::Blocked => {
                                        game_log
                                            .log_debug(format!("{} path blocked", name.as_str()));
                                        let tile = (
                                            (target_position.x / TILE_SIZE).floor() as i32,
                                            (target_position.y / TILE_SIZE).floor() as i32,
                                        );
                                        path_blocked_types.push((action_type, tile));
                                        true
                                    }
                                }
                            } else {
                                false
                            }
                        }
                    }
                },
            };

            // A path-blocked Movement is "complete" in the sense that it's
            // removed from the active set this tick, but it's a failure, not
            // a success: handled by the path_blocked_types loop below.
            if completed && path_blocked_types.iter().all(|(t, _)| *t != action_type) {
                completed_types.push(action_type);
            }
        }

        // Process completions: run on_complete.
        for action_type in &completed_types {
            let Some(snapshot) = active.get(*action_type).cloned() else {
                continue;
            };
            let Some(action_def) = registry.get(*action_type) else {
                continue;
            };

            let mut target_inv = snapshot
                .target_entity
                .and_then(|e| target_inventories.get_mut(e).ok());
            let target_inv_ptr = target_inv.as_deref_mut();

            // Snapshot needs before on_complete so we can compute the delta.
            // Hunger is derived from the metabolism pools rather than a raw
            // field, so we snapshot the urgency (0..1) as "pre_hunger" on a
            // 0..100 scale to preserve the outcome event semantics.
            let pre_hunger = physical.metabolism.hunger_urgency() * 100.0;
            let pre_thirst = physical.thirst;
            let pre_aerobic = physical.stamina.aerobic;

            let agent_position = transform.translation.truncate();
            let mut spawn_requests = Vec::new();

            let mut ctx = crate::agent::actions::registry::CompletionContext {
                physical: &mut physical,
                inventory: &mut inventory,
                drives: drives.as_deref_mut(),
                mind,
                skills,
                target_inventory: target_inv_ptr,
                target_entity: snapshot.target_entity,
                tick: current_tick,
                agent_position,
                spawn_requests: &mut spawn_requests,
            };

            action_def.on_complete(&mut ctx);

            // Process any entity spawn requests from the action.
            for req in spawn_requests {
                use crate::agent::actions::registry::SpawnRequest;
                match req {
                    SpawnRequest::Entity { concept, position } => {
                        if crate::world::spawn::spawn_concept_entity(
                            &mut commands,
                            concept,
                            position,
                            tick.current,
                        )
                        .is_none()
                        {
                            game_log.log_debug(format!(
                                "Unhandled spawn request for concept {concept:?}"
                            ));
                        }
                    }
                    SpawnRequest::Site {
                        target,
                        position,
                        requirements,
                        initial_items,
                        labor_required,
                    } => {
                        crate::world::construction_site::spawn_construction_site_headless(
                            &mut commands,
                            target,
                            position,
                            &requirements,
                            &initial_items,
                            labor_required,
                            current_tick,
                            Some(entity),
                        );
                    }
                    SpawnRequest::BecomesAttach {
                        entity,
                        target,
                        mode,
                    } => {
                        let mut becomes = crate::world::becomes::Becomes::new(
                            target,
                            crate::world::becomes::BecomesTrigger::AfterTicks(0),
                            current_tick,
                        );
                        if matches!(mode, crate::world::becomes::BecomesMode::InPlace) {
                            becomes = becomes.in_place();
                        }
                        commands.entity(entity).insert(becomes);
                    }
                }
            }

            // Only emit a success outcome when something observable changed.
            // Walk/Idle/Wander complete with no effects — skip the allocation.
            let post_hunger = physical.metabolism.hunger_urgency() * 100.0;
            let hunger_reduced = pre_hunger - post_hunger;
            let thirst_reduced = pre_thirst - physical.thirst;
            let stamina_gained = physical.stamina.aerobic - pre_aerobic;
            if hunger_reduced > 0.0 || thirst_reduced > 0.0 || stamina_gained > 0.0 {
                outcome_events.write(ActionOutcomeEvent {
                    actor: entity,
                    outcome: ActionOutcome::Success {
                        action: *action_type,
                        target: snapshot.target_entity,
                        gained: None,
                        consumed: None,
                        need_satisfaction: Some(NeedSatisfaction {
                            hunger_reduced,
                            thirst_reduced,
                            stamina_gained,
                            pre_hunger,
                            pre_thirst,
                        }),
                    },
                });
            }

            sim_events.write(crate::agent::events::SimEvent::ActionCompleted {
                agent: entity,
                tick: current_tick,
                action: *action_type,
            });

            if let Some(msg) = action_def.complete_log() {
                game_log.action(name.as_str(), msg, None, Some(entity));
            }
        }

        // Cancel actions whose target entity was despawned mid-execution.
        for action_type in &target_gone_types {
            let snapshot = active.get(*action_type).cloned();
            active.remove(*action_type);
            let target = snapshot.and_then(|s| s.target_entity);
            sim_events.write(crate::agent::events::SimEvent::ActionFailed {
                agent: entity,
                tick: current_tick,
                action: *action_type,
                reason: crate::agent::events::FailureReason::TargetGone,
            });
            outcome_events.write(ActionOutcomeEvent {
                actor: entity,
                outcome: ActionOutcome::Failed {
                    action: *action_type,
                    target,
                    reason: crate::agent::events::FailureReason::TargetGone,
                },
            });
        }

        // Fail path-blocked Movement actions. Keeps the active set clean
        // (like completion), but emits a failure outcome so the belief
        // updater can record the target tile as Unreachable and the
        // planner stops re-picking it on the next replan.
        for (action_type, target_tile) in &path_blocked_types {
            let snapshot = active.get(*action_type).cloned();
            active.remove(*action_type);
            let target = snapshot.and_then(|s| s.target_entity);
            let reason = crate::agent::events::FailureReason::PathBlocked {
                target_tile: *target_tile,
            };
            sim_events.write(crate::agent::events::SimEvent::ActionFailed {
                agent: entity,
                tick: current_tick,
                action: *action_type,
                reason: reason.clone(),
            });
            outcome_events.write(ActionOutcomeEvent {
                actor: entity,
                outcome: ActionOutcome::Failed {
                    action: *action_type,
                    target,
                    reason,
                },
            });
        }

        // Remove completed actions from the running set.
        for action_type in &completed_types {
            active.remove(*action_type);
        }

        // If everything cleared out, drop back to Idle so legacy queries
        // (UI, perception) always see something.
        if active.is_empty() {
            active.reset_to_idle(current_tick);
        }

        // Clear the movement target if no movement action remains.
        let any_movement = active.iter().any(|a| {
            matches!(
                registry.get(a.action_type).map(|d| d.kind()),
                Some(ActionKind::Movement)
            )
        });
        if !any_movement {
            target_pos.0 = None;
        }
    }
}

/// Per-tick stat drain summed across every running action.
pub fn apply_action_effects(
    registry: Res<ActionRegistry>,
    tick: Res<TickCount>,
    mut agents: Query<(
        &ActiveActions,
        &mut PhysicalNeeds,
        &mut Consciousness,
        Option<&Body>,
    )>,
) {
    let dt = tick.dt();

    for (active, mut physical, mut consciousness, body) in agents.iter_mut() {
        let load = active.channel_load(&registry);
        // Capacities freeze the start-of-tick stamina so degradation doesn't
        // compound as the loop mutates physical.stamina mid-iteration.
        let capacities = ChannelCapacities::compute(body, Some(&*physical));

        for action_state in active.iter() {
            let Some(action_def) = registry.get(action_state.action_type) else {
                continue;
            };
            let effects = action_def.runtime_effects();
            let degradation = load.degradation_factor(action_def.body_channels(), &capacities);
            let is_movement = matches!(action_def.kind(), ActionKind::Movement);

            // Runtime stamina effects route through aerobic for non-movement
            // actions. Movement actions (#339) drain stamina through the
            // intensity-aware path in `tick_actions` instead, so skipping
            // here avoids double-dipping.
            if !is_movement {
                physical
                    .stamina
                    .adjust_aerobic(effects.stamina_per_sec * dt * degradation);
            }

            // Action-level glucose drain stacks on top of the activity-level
            // BMR drain handled by `apply_activity_effects`. Each drain pushes
            // through `Metabolism::tick`-equivalent accounting: we deduct the
            // glucose directly because the overflow/mobilize pass already ran
            // this tick via the activity system. Stomach fill (grazing, etc.)
            // adds raw carbs that `activity_effects.tick` will digest next tick.
            let drain = effects.glucose_drain_per_sec * dt * degradation;
            if drain != 0.0 {
                physical.metabolism.glucose = (physical.metabolism.glucose - drain)
                    .clamp(0.0, crate::agent::body::metabolism::GLUCOSE_MAX);
            }
            let carbs_fill = effects.stomach_carbs_per_sec * dt * degradation;
            if carbs_fill > 0.0 {
                physical
                    .metabolism
                    .eat(crate::agent::body::metabolism::FoodMacros::new(
                        carbs_fill, 0.0,
                    ));
            }

            consciousness.alertness = (consciousness.alertness
                + effects.alertness_per_sec * dt * 0.01 * degradation)
                .clamp(0.0, 1.0);
        }
    }
}

// ============================================================================
// Preemption helpers
// ============================================================================

/// Enforce "at most one `ActionKind::Movement` action active at a time."
///
/// The channel system models *body parts* (Legs, Mouth, Hands, …). Two
/// Movement actions sharing Legs at intensity 0.4 each don't channel-conflict
/// (load 0.8 < hard threshold 1.4) and would otherwise be admitted in
/// parallel. But there is exactly one `transform.translation` — two
/// simultaneous moves toward different targets are physically incoherent.
/// They tick in parallel, both call `move_toward`, both mutate the transform,
/// and the agent bounces between targets forever (see #223).
///
/// When a new Movement action is about to be admitted, this function removes
/// any existing *interruptible* Movement action so the new one can take over
/// the transform exclusively. The most-recent Movement always wins — the
/// brain re-thinks every 60 ticks, so this converges to the right answer
/// within one cycle.
///
/// Returns the displaced action type if one was preempted, `None` otherwise.
/// The caller is responsible for emitting `SimEvent::ActionPreempted` (the
/// existing snapshot-and-diff loop in `start_actions` handles this).
fn preempt_existing_movement(
    active: &mut ActiveActions,
    registry: &ActionRegistry,
    target: &mut TargetPosition,
    incoming_kind: ActionKind,
) -> Option<ActionType> {
    if !matches!(incoming_kind, ActionKind::Movement) {
        return None;
    }
    let existing = active.iter().find_map(|s| {
        let def = registry.get(s.action_type)?;
        if matches!(def.kind(), ActionKind::Movement) && def.interruptible() {
            Some(s.action_type)
        } else {
            None
        }
    })?;
    active.remove(existing);
    target.0 = None;
    Some(existing)
}

/// Try to admit `requirements` into `active`, preempting interruptible actions
/// until there's no hard conflict. Returns false if preemption can't make room
/// (e.g. an uninterruptible action holds a conflicting channel).
///
/// Victim selection only considers actions that contribute to a *saturated*
/// channel - removing a Walk wouldn't help relieve a Mouth conflict.
fn preempt_to_make_room(
    active: &mut ActiveActions,
    registry: &ActionRegistry,
    requirements: &[crate::agent::actions::channel::ChannelUsage],
    capacities: &ChannelCapacities,
    target: &mut TargetPosition,
) -> bool {
    // Transactional: snapshot the active set and target before mutating, so
    // a failed search (e.g. an uninterruptible action blocking the path)
    // does not leave half-removed victims behind. Without this, an incoming
    // action that ultimately gets rejected can still drop unrelated
    // interruptible neighbours as collateral while searching for a
    // (non-existent) victim path.
    let active_snapshot = active.clone();
    let target_snapshot = target.0;

    let mut load = active.channel_load(registry);

    while load.would_hard_conflict(requirements, capacities) {
        // Which channels are at or over the hard threshold given the new requirements?
        let saturated: [bool; crate::agent::actions::channel::CHANNEL_COUNT] = {
            let mut s = [false; crate::agent::actions::channel::CHANNEL_COUNT];
            for usage in requirements {
                let cap = capacities.get(usage.channel);
                let projected = load.saturation(usage.channel) + usage.intensity;
                if projected >= crate::agent::actions::channel::HARD_CONFLICT_THRESHOLD * cap {
                    s[usage.channel.idx()] = true;
                }
            }
            s
        };

        // Pick the smallest interruptible action that touches a saturated channel.
        let preempt_target = active
            .iter()
            .filter_map(|s| {
                let def = registry.get(s.action_type)?;
                if !def.interruptible() {
                    return None;
                }
                let channels = def.body_channels();
                if !channels.iter().any(|c| saturated[c.channel.idx()]) {
                    return None;
                }
                let total_intensity: f32 = channels.iter().map(|c| c.intensity).sum();
                Some((s.action_type, total_intensity, channels))
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let Some((victim_type, _intensity, victim_channels)) = preempt_target else {
            // No further victims available — roll back any preemptions we
            // made during this search so callers don't observe collateral
            // damage from a rejected preempt.
            *active = active_snapshot;
            target.0 = target_snapshot;
            return false;
        };

        load.remove(victim_channels);
        active.remove(victim_type);

        if matches!(
            registry.get(victim_type).map(|d| d.kind()),
            Some(ActionKind::Movement)
        ) {
            target.0 = None;
        }
    }

    true
}

// ============================================================================
// Target Finding Helpers
// ============================================================================

fn find_explore_target(
    current_pos: Vec2,
    mind: &MindGraph,
    world_map: &WorldMap,
    current_tick: u64,
    rng: &mut impl Rng,
) -> Option<Vec2> {
    let mut best_target: Option<Vec2> = None;
    let mut best_score = f32::MAX;
    let (map_w, map_h) = world_map.pixel_bounds();

    for _ in 0..10 {
        let test_pos = Vec2::new(rng.random_range(0.0..map_w), rng.random_range(0.0..map_h));

        if world_map.is_walkable(test_pos) {
            let chunk_x = (test_pos.x / (CHUNK_SIZE as f32 * TILE_SIZE)).floor() as i32;
            let chunk_y = (test_pos.y / (CHUNK_SIZE as f32 * TILE_SIZE)).floor() as i32;

            let mut score = 0.0;
            let triples = mind.query(
                Some(&Node::Chunk((chunk_x, chunk_y))),
                Some(Predicate::Explored),
                None,
            );

            if let Some(triple) = triples.first()
                && let Value::Boolean(true) = triple.object
            {
                let age = (current_tick as i32 - triple.meta.timestamp as i32).max(0) as f32;
                score = 1000.0 / (age + 1.0);
            }

            score += current_pos.distance(test_pos) / 5000.0;

            if score < best_score {
                best_score = score;
                best_target = Some(test_pos);
            }
        }
    }
    best_target
}

fn pick_random_walkable_target(
    pos: Vec2,
    world_map: &WorldMap,
    dist_range: std::ops::Range<f32>,
    rng: &mut impl Rng,
) -> Option<Vec2> {
    let base_angle: f32 = rng.random_range(0.0..std::f32::consts::TAU);
    let dist: f32 = rng.random_range(dist_range);

    for i in 0..8 {
        let angle = base_angle + (i as f32 * std::f32::consts::TAU / 8.0);
        let test_pos = pos + Vec2::new(angle.cos(), angle.sin()) * dist;
        if world_map.in_bounds(test_pos) && world_map.is_walkable(test_pos) {
            return Some(test_pos);
        }
    }
    None
}

/// Sample the straight line from `from` to `to` at tile-sized steps and
/// return `false` if any sampled point is non-walkable. Shallow
/// line-of-sight check — good enough for the walker which itself moves in
/// a straight line, so a destination that's walkable but behind a wall
/// doesn't count as reachable.
fn straight_line_is_clear(from: Vec2, to: Vec2, world_map: &WorldMap) -> bool {
    let delta = to - from;
    let distance = delta.length();
    if distance < 1e-3 {
        return world_map.in_bounds(from) && world_map.is_walkable(from);
    }
    let steps = (distance / TILE_SIZE).ceil() as i32;
    if steps <= 0 {
        return world_map.in_bounds(to) && world_map.is_walkable(to);
    }
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let sample = from + delta * t;
        if !world_map.in_bounds(sample) || !world_map.is_walkable(sample) {
            return false;
        }
    }
    true
}

/// Pick a flee target along an away-vector cone, preferring directly-away
/// at maximum distance and widening the angle and shortening the distance
/// until a walkable tile *with a clear straight-line path* is found.
/// Without this sampling the straight `pos + away * 50` from Flee would
/// drive agents into water or off-map whenever the direct retreat path
/// was blocked — every tick, repeatedly, because Flee is urgency-driven
/// and regenerates on its own (#376).
///
/// Destination walkability alone is not enough: the walker moves in a
/// straight line with no pathfinding, so a candidate whose destination is
/// walkable but whose path crosses water will still fail as `PathBlocked`
/// next tick. `straight_line_is_clear` samples the path at tile steps to
/// reject those cases up front.
///
/// `away` is expected to be a unit vector pointing away from the threat.
/// If the caller supplies a zero vector (agent standing on the threat,
/// `normalize_or_zero` degenerate case) this function falls back to a
/// random walkable target so the agent at least tries to move.
fn pick_flee_target(
    pos: Vec2,
    away: Vec2,
    world_map: &WorldMap,
    rng: &mut impl Rng,
) -> Option<Vec2> {
    if away == Vec2::ZERO {
        return pick_random_walkable_target(pos, world_map, 30.0..60.0, rng);
    }

    // Distance candidates, longest first — a far flee is more useful than
    // a short one, so we accept the first distance that works.
    const DIST_STEPS: [f32; 3] = [50.0, 35.0, 20.0];
    // Angular offsets from the pure-away direction. Zero first so the
    // agent flees directly back when possible; widen to ±30° and ±60°
    // only when the direct line is blocked.
    const ANGLE_OFFSETS_RAD: [f32; 5] = [
        0.0,
        std::f32::consts::FRAC_PI_6,  //  +30°
        -std::f32::consts::FRAC_PI_6, //  -30°
        std::f32::consts::FRAC_PI_3,  //  +60°
        -std::f32::consts::FRAC_PI_3, //  -60°
    ];

    for dist in DIST_STEPS {
        for offset in ANGLE_OFFSETS_RAD {
            let (sin, cos) = offset.sin_cos();
            let rotated = Vec2::new(away.x * cos - away.y * sin, away.x * sin + away.y * cos);
            let test_pos = pos + rotated * dist;
            if world_map.in_bounds(test_pos)
                && world_map.is_walkable(test_pos)
                && straight_line_is_clear(pos, test_pos, world_map)
            {
                return Some(test_pos);
            }
        }
    }

    // Cornered against an obstacle in every direction we checked. Try a
    // fully-random walkable tile as a last-resort so the agent doesn't
    // thrash repeatedly against the same blocked cone.
    pick_random_walkable_target(pos, world_map, 30.0..60.0, rng)
}

/// Pick a nearby grass tile as a drift target. Grazing only happens on grass,
/// so this refuses to return non-grass positions rather than silently letting
/// the grazer wander onto sand, forest, or rock.
fn pick_random_grass_target(
    pos: Vec2,
    world_map: &WorldMap,
    dist_range: std::ops::Range<f32>,
    rng: &mut impl Rng,
) -> Option<Vec2> {
    use crate::world::map::TileType;

    let base_angle: f32 = rng.random_range(0.0..std::f32::consts::TAU);
    let dist: f32 = rng.random_range(dist_range);

    for i in 0..8 {
        let angle = base_angle + (i as f32 * std::f32::consts::TAU / 8.0);
        let test_pos = pos + Vec2::new(angle.cos(), angle.sin()) * dist;
        if !world_map.in_bounds(test_pos) {
            continue;
        }
        if matches!(world_map.tile_at(test_pos), Some(TileType::Grass)) {
            return Some(test_pos);
        }
    }
    None
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::actions::ActionType;
    use crate::agent::actions::channel::{ChannelLoad, ChannelUsage};

    fn build_registry() -> ActionRegistry {
        ActionRegistry::new()
    }

    #[test]
    fn walk_and_eat_run_in_parallel() {
        let registry = build_registry();
        let mut active = ActiveActions::empty();
        active.insert(ActionState::new(ActionType::Walk, 0));
        active.insert(ActionState::new(ActionType::Eat, 0).with_duration(20));

        let load = active.channel_load(&registry);
        // Walk(Legs 0.4) + Eat(Hands 0.5, Mouth 0.7) - no overlap
        assert!(!load.would_hard_conflict(&[], &ChannelCapacities::full()));
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn flee_replaces_walk_via_preemption() {
        let registry = build_registry();
        let mut active = ActiveActions::empty();
        active.insert(ActionState::new(ActionType::Walk, 0));

        let flee_def = registry.get(ActionType::Flee).unwrap();
        let mut target = TargetPosition::default();
        let admitted = preempt_to_make_room(
            &mut active,
            &registry,
            flee_def.body_channels(),
            &ChannelCapacities::full(),
            &mut target,
        );

        assert!(admitted, "Flee should preempt Walk to make room");
        assert!(!active.contains(ActionType::Walk));
    }

    #[test]
    fn sleep_channel_alone_does_not_preempt_everything() {
        // After #291, Sleep declares only FullBody(1.0). Walk/Eat don't
        // touch FullBody, so `preempt_to_make_room` alone can't clear them
        // — that responsibility moved to the `start_actions` short-circuit
        // (tested below by `sleep_active_blocks_other_action_admission`).
        // This test documents the new boundary: channel-based preemption
        // is orthogonal to the sleeping gate.
        let registry = build_registry();
        let mut active = ActiveActions::empty();
        active.insert(ActionState::new(ActionType::Walk, 0));
        active.insert(ActionState::new(ActionType::Eat, 0).with_duration(20));

        let sleep_def = registry.get(ActionType::Sleep).unwrap();
        let mut target = TargetPosition::default();
        let admitted = preempt_to_make_room(
            &mut active,
            &registry,
            sleep_def.body_channels(),
            &ChannelCapacities::full(),
            &mut target,
        );

        // Sleep fits (FullBody is free), and nothing else needed to be
        // preempted via the channel system.
        assert!(admitted);
        assert!(active.contains(ActionType::Walk));
        assert!(active.contains(ActionType::Eat));
    }

    #[test]
    fn flee_preempts_sleep_via_full_body_channel() {
        // Sleep is interruptible, so Flee's FullBody(0.5) collides with
        // Sleep's FullBody(1.0) and evicts it through normal preemption.
        // Casual admission of other actions while asleep is still blocked
        // one layer up by the `start_actions` short-circuit — this test
        // only exercises the raw channel-admission behaviour.
        use crate::agent::actions::channel::Channel;
        let registry = build_registry();
        let mut active = ActiveActions::empty();
        active.insert(ActionState::new(ActionType::Sleep, 0));

        let flee_channels = &[
            ChannelUsage::new(Channel::Locomotion, 1.0),
            ChannelUsage::new(Channel::FullBody, 0.5),
        ];
        let mut target = TargetPosition::default();
        let admitted = preempt_to_make_room(
            &mut active,
            &registry,
            flee_channels,
            &ChannelCapacities::full(),
            &mut target,
        );

        assert!(admitted, "Flee should evict Sleep at the channel layer");
        assert!(
            !active.contains(ActionType::Sleep),
            "Sleep should have been preempted to make room for Flee"
        );
    }

    #[test]
    fn wake_up_preempts_sleep_via_full_body_channel() {
        // Regression for #352: when Sleep was marked uninterruptible,
        // WakeUp could never free the FullBody channel and agents slept
        // forever. WakeUp and Sleep both touch FullBody, so admitting
        // WakeUp must preempt Sleep through the normal channel pass.
        let registry = build_registry();
        let mut active = ActiveActions::empty();
        active.insert(ActionState::new(ActionType::Sleep, 0));

        let wake_def = registry.get(ActionType::WakeUp).unwrap();
        let mut target = TargetPosition::default();
        let admitted = preempt_to_make_room(
            &mut active,
            &registry,
            wake_def.body_channels(),
            &ChannelCapacities::full(),
            &mut target,
        );

        assert!(admitted, "WakeUp must be admissible while Sleep is active");
        assert!(
            !active.contains(ActionType::Sleep),
            "Sleep should be preempted when WakeUp takes its FullBody slot"
        );
    }

    #[test]
    fn oversaturated_consumption_degrades_eat() {
        // After #291, Eat and Converse no longer share a channel — Eat uses
        // Consumption and Converse uses Vocalization. This test now just
        // verifies the degradation math still kicks in when *anything*
        // oversaturates Consumption. Two pretend load sources sum beyond
        // the soft threshold and Eat's computed factor drops below 1.0.
        use crate::agent::actions::channel::Channel;
        let registry = build_registry();

        let mut load = ChannelLoad::new();
        load.add(&[ChannelUsage::new(Channel::Consumption, 0.7)]);
        load.add(&[ChannelUsage::new(Channel::Consumption, 0.6)]);

        let eat_channels = registry.get(ActionType::Eat).unwrap().body_channels();
        let factor = load.degradation_factor(eat_channels, &ChannelCapacities::full());
        let expected = 1.0 / 1.3;
        assert!(
            (factor - expected).abs() < 1e-4,
            "expected {expected}, got {factor}"
        );
    }

    #[test]
    fn preemption_only_removes_actions_touching_saturated_channels() {
        // Walk(Legs 0.4) and Converse(Mouth 0.6) both running. Admitting
        // Flee(Legs 1.0, FullBody 0.5) saturates Legs at 1.4 - exactly the
        // hard threshold. The preemption pass should drop Walk (Legs is
        // saturated) and leave Converse alone (Mouth is not).
        let registry = build_registry();
        let mut active = ActiveActions::empty();
        active.insert(ActionState::new(ActionType::Walk, 0));
        active.insert(ActionState::new(ActionType::Converse, 0));

        let flee_def = registry.get(ActionType::Flee).unwrap();
        let mut target = TargetPosition::default();
        let admitted = preempt_to_make_room(
            &mut active,
            &registry,
            flee_def.body_channels(),
            &ChannelCapacities::full(),
            &mut target,
        );

        assert!(admitted);
        assert!(!active.contains(ActionType::Walk));
        assert!(active.contains(ActionType::Converse));
    }

    // ─────────────────────────────────────────────────────────────────────
    // #109: Action::interruptible() — Build refuses casual preemption
    // ─────────────────────────────────────────────────────────────────────
    //
    // Build overrides `interruptible() -> false` so a half-built campfire
    // is not dropped when a new urgency edges in. These tests cover the
    // uninterruptible-victim filter inside `preempt_to_make_room` and the
    // documented exit transition (timed countdown → on_complete).

    #[test]
    fn build_blocks_preemption_attempt_from_harvest() {
        // Build is running. Harvest tries to start; both want Hands at 0.9
        // (combined 1.8, well over the 1.4 hard threshold). Because Build
        // is uninterruptible, no victim can be selected and Harvest is
        // rejected. Build keeps going.
        let registry = build_registry();
        let mut active = ActiveActions::empty();
        active.insert(ActionState::new(ActionType::Build, 0));

        let harvest_def = registry.get(ActionType::Harvest).unwrap();
        let mut target = TargetPosition::default();
        let admitted = preempt_to_make_room(
            &mut active,
            &registry,
            harvest_def.body_channels(),
            &ChannelCapacities::full(),
            &mut target,
        );

        assert!(!admitted, "Harvest must NOT be allowed to preempt Build");
        assert!(
            active.contains(ActionType::Build),
            "Build must remain running after a rejected preempt"
        );
    }

    #[test]
    fn build_blocks_preemption_from_custom_high_manipulation_interrupter() {
        // After #291, Sleep no longer floods every active channel (it only
        // declares FullBody 1.0), so the old "Sleep preempts Build" test
        // case became mechanically impossible — Sleep doesn't touch
        // Build's Manipulation channel. This test replaces it with a
        // custom interrupter that actually collides with Build's channels:
        // a hypothetical Manipulation-heavy action that would need Build
        // out of the way. Build's uninterruptible flag must still hold.
        use crate::agent::actions::channel::Channel;
        let registry = build_registry();
        let mut active = ActiveActions::empty();
        active.insert(ActionState::new(ActionType::Build, 0));

        let interrupter = &[
            ChannelUsage::new(Channel::Manipulation, 0.9),
            ChannelUsage::new(Channel::Locomotion, 0.2),
        ];
        let mut target = TargetPosition::default();
        let admitted = preempt_to_make_room(
            &mut active,
            &registry,
            interrupter,
            &ChannelCapacities::full(),
            &mut target,
        );

        assert!(
            !admitted,
            "Manipulation-heavy interrupter must NOT be allowed to preempt Build"
        );
        assert!(active.contains(ActionType::Build));
    }

    #[test]
    fn rejected_preempt_rolls_back_collateral_damage() {
        // Build (Manipulation 0.9, Locomotion 0.2) and Walk (Locomotion
        // 0.4) are both running. A custom interrupter needs both channels
        // saturated (Locomotion 1.0, Manipulation 0.9). The search:
        //   - drops Walk first (smallest interruptible touching Locomotion)
        //   - then can't find another victim because Build is
        //     uninterruptible and still saturates Manipulation
        //   - returns false → rolls back all mutations
        //
        // Without the rollback, Walk would be destroyed for nothing. With
        // it, the active set and target are exactly what they were before
        // the failed attempt.
        use crate::agent::actions::channel::Channel;
        let registry = build_registry();
        let mut active = ActiveActions::empty();
        active.insert(ActionState::new(ActionType::Build, 0));
        active.insert(ActionState::new(ActionType::Walk, 0));

        let interrupter = &[
            ChannelUsage::new(Channel::Locomotion, 1.0),
            ChannelUsage::new(Channel::Manipulation, 0.9),
        ];
        let mut target = TargetPosition(Some(Vec2::new(99.0, 99.0)));
        let target_before = target.0;

        let admitted = preempt_to_make_room(
            &mut active,
            &registry,
            interrupter,
            &ChannelCapacities::full(),
            &mut target,
        );

        assert!(
            !admitted,
            "interrupter must be rejected because of uninterruptible Build"
        );
        assert!(
            active.contains(ActionType::Build),
            "Build must remain after a rejected preempt"
        );
        assert!(
            active.contains(ActionType::Walk),
            "Walk must NOT be collateral damage from a rejected preempt"
        );
        assert_eq!(
            target.0, target_before,
            "TargetPosition must be untouched on rejected preempt"
        );
    }

    #[test]
    fn build_exit_transition_is_timed_completion() {
        // Document the documented exit transition for the uninterruptible
        // Build action: it is `ActionKind::Timed` with a finite duration,
        // so the standard `tick_actions` countdown drives it to completion.
        // No force-clear path is needed.
        let registry = build_registry();
        let build_def = registry.get(ActionType::Build).unwrap();
        match build_def.kind() {
            ActionKind::Timed { duration_ticks } => {
                assert!(
                    duration_ticks > 0 && duration_ticks < u32::MAX,
                    "Build duration must be finite so the timed countdown is the exit path"
                );
            }
            other => panic!("Build must be ActionKind::Timed, got {other:?}"),
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // #223: ActionKind::Movement mutual exclusion
    // ─────────────────────────────────────────────────────────────────────
    //
    // The channel system models body parts (Legs, Mouth, Hands…). Two
    // Movement actions sharing Legs at intensity 0.4 each don't channel-
    // conflict (load 0.8 < hard threshold 1.4) and would otherwise be
    // admitted in parallel. But there's only one `transform.translation` —
    // two simultaneous moves toward different targets are physically
    // incoherent. The agent can't walk toward two places at once.
    //
    // `preempt_existing_movement` enforces "at most one Movement action
    // active at a time" by removing any existing interruptible Movement
    // when a new Movement is about to be admitted. The new movement
    // overrides the old (most-recent wins).

    #[test]
    fn admitting_walk_preempts_existing_wander() {
        let registry = build_registry();
        let mut active = ActiveActions::empty();
        active.insert(ActionState::new(ActionType::Wander, 0));
        let mut target = TargetPosition(Some(Vec2::new(10.0, 10.0)));

        let preempted =
            preempt_existing_movement(&mut active, &registry, &mut target, ActionKind::Movement);

        assert_eq!(
            preempted,
            Some(ActionType::Wander),
            "An incoming Movement must displace the existing Wander Movement"
        );
        assert!(!active.contains(ActionType::Wander));
        assert_eq!(
            target.0, None,
            "TargetPosition must clear with the old movement"
        );
    }

    #[test]
    fn admitting_walk_preempts_existing_explore() {
        let registry = build_registry();
        let mut active = ActiveActions::empty();
        active.insert(ActionState::new(ActionType::Explore, 0));
        let mut target = TargetPosition(Some(Vec2::new(50.0, 50.0)));

        let preempted =
            preempt_existing_movement(&mut active, &registry, &mut target, ActionKind::Movement);

        assert_eq!(preempted, Some(ActionType::Explore));
        assert!(!active.contains(ActionType::Explore));
    }

    #[test]
    fn non_movement_action_does_not_preempt_movement() {
        // Eat is not Movement — it shouldn't touch a running Walk.
        let registry = build_registry();
        let mut active = ActiveActions::empty();
        active.insert(ActionState::new(ActionType::Walk, 0));
        let mut target = TargetPosition(Some(Vec2::new(99.0, 99.0)));
        let target_before = target.0;

        let eat_kind = registry.get(ActionType::Eat).unwrap().kind();
        let preempted = preempt_existing_movement(&mut active, &registry, &mut target, eat_kind);

        assert_eq!(
            preempted, None,
            "Non-Movement admission must not preempt Movement"
        );
        assert!(active.contains(ActionType::Walk));
        assert_eq!(target.0, target_before, "TargetPosition must be untouched");
    }

    #[test]
    fn movement_admission_with_no_existing_movement_does_nothing() {
        let registry = build_registry();
        let mut active = ActiveActions::empty();
        active.insert(ActionState::new(ActionType::Eat, 0).with_duration(20));
        let mut target = TargetPosition::default();

        let preempted =
            preempt_existing_movement(&mut active, &registry, &mut target, ActionKind::Movement);

        assert_eq!(preempted, None);
        assert!(active.contains(ActionType::Eat));
    }

    // ─── Flee target sampling (#376) ───────────────────────────────────────

    use crate::world::map::{CHUNK_SIZE as MAP_CHUNK_SIZE, TILE_SIZE as MAP_TILE_SIZE, TileType};
    use bevy::math::IVec2;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    /// Build a flat-grass WorldMap large enough to cover the flee search
    /// cone. Tests then paint specific tiles with `map.set_tile(x, y, …)`.
    fn walkable_test_map() -> WorldMap {
        let size = MAP_CHUNK_SIZE * 4;
        let mut map = WorldMap::new(size, size);
        // Seed every chunk the search cone can touch so set_tile and
        // is_walkable don't hit missing-chunk fallbacks.
        for cx in 0..4i32 {
            for cy in 0..4i32 {
                map.chunks
                    .entry(IVec2::new(cx, cy))
                    .or_insert_with(|| crate::world::map::Chunk::new(cx, cy));
            }
        }
        for x in 0..size {
            for y in 0..size {
                map.set_tile(x, y, TileType::Grass);
            }
        }
        map
    }

    fn fill_rect(map: &mut WorldMap, x: u32, y: u32, w: u32, h: u32, tile: TileType) {
        for ty in y..y + h {
            for tx in x..x + w {
                map.set_tile(tx, ty, tile);
            }
        }
    }

    #[test]
    fn flee_picks_walkable_straight_away_when_path_is_clear() {
        // Agent at (50, 50), threat east. Away vector points -x. No
        // obstacles — the function must return a tile directly west, at
        // the longest candidate distance.
        let map = walkable_test_map();
        let mut rng = StdRng::seed_from_u64(0);
        let pos = Vec2::new(50.0, 50.0);
        let away = Vec2::new(-1.0, 0.0);

        let target = pick_flee_target(pos, away, &map, &mut rng)
            .expect("clear map must yield a walkable flee target");

        // Directly west at the maximum distance step (50 px) — the
        // cone-sampler should hit this on its very first candidate.
        assert!(
            (target.x - 0.0).abs() < 1e-3 && (target.y - 50.0).abs() < 1e-3,
            "expected direct-away at 50 px (≈ (0, 50)); got {target:?}",
        );
    }

    #[test]
    fn flee_widens_angle_when_straight_retreat_is_blocked() {
        // Agent at (100, 100), threat east (away = -x). Paint a vertical
        // water column immediately west of the agent so the 0° candidates
        // at all three distances hit water. The sampler should deflect to
        // ±30° (or further) and return a walkable tile.
        //
        // Water column at tile x=2 (world x range 32-48). Agent at tile
        // (6, 6). Direct-away (0° offset) at 50/35/20 px lands at
        // world x ≈ 50, 65, 80 — *east* of the water wall (walkable).
        //
        // To force deflection, place the threat to the WEST so away = +x,
        // and put the water wall east of the agent.
        let mut map = walkable_test_map();
        // Water wall east of the agent, blocking the direct-east retreat
        // across all three distance steps. Tile x=7 is world range
        // 112-128; the agent at (100, 100) tries (150, 100), (135, 100),
        // (120, 100). All three land on tile x≥7 which is water.
        fill_rect(&mut map, 7, 0, 3, MAP_CHUNK_SIZE * 4, TileType::Water);
        let mut rng = StdRng::seed_from_u64(0);
        let pos = Vec2::new(100.0, 100.0);
        let away = Vec2::new(1.0, 0.0); // flee east

        let target = pick_flee_target(pos, away, &map, &mut rng)
            .expect("angular offsets should open a viable flee route");

        // Not in the water column. Must be on a walkable tile.
        assert!(
            map.is_walkable(target),
            "flee target {target:?} must be walkable",
        );
        // Must still be moving away from the threat (x >= pos.x), even
        // after angular deflection — the sampler starts with 0° so it
        // should never pick something actively moving *toward* the threat.
        assert!(
            target.x >= pos.x - 1e-3,
            "flee target should still be east-ish (not moving back \
             toward the threat at x < {}); got {target:?}",
            pos.x,
        );
    }

    #[test]
    fn flee_falls_back_to_random_when_entire_cone_is_blocked() {
        // Surround the agent on all sides by water, leaving only the tile
        // they stand on walkable. Every cone candidate fails, so the
        // sampler must fall through to `pick_random_walkable_target` and
        // return either the agent's own tile or None (since no other
        // walkable tile exists, None is also acceptable).
        let mut map = walkable_test_map();
        let agent_tx = 6u32;
        let agent_ty = 6u32;
        for ty in 0..(MAP_CHUNK_SIZE * 4) {
            for tx in 0..(MAP_CHUNK_SIZE * 4) {
                if tx != agent_tx || ty != agent_ty {
                    map.set_tile(tx, ty, TileType::Water);
                }
            }
        }
        let mut rng = StdRng::seed_from_u64(0);
        let pos = Vec2::new(
            agent_tx as f32 * MAP_TILE_SIZE + MAP_TILE_SIZE / 2.0,
            agent_ty as f32 * MAP_TILE_SIZE + MAP_TILE_SIZE / 2.0,
        );
        let away = Vec2::new(1.0, 0.0);

        // The desperate fallback may return None (no walkable neighbour
        // exists). What we're asserting is that it does *not* return a
        // bogus water tile — i.e. any Some(target) must be walkable.
        if let Some(target) = pick_flee_target(pos, away, &map, &mut rng) {
            assert!(
                map.is_walkable(target),
                "fallback target {target:?} must still be walkable when the \
                 cone is fully blocked",
            );
        }
    }

    #[test]
    fn flee_rejects_candidates_whose_straight_line_crosses_water() {
        // The destination tile is walkable, but the path from the agent
        // to it crosses a water column. A flee picker that only checks
        // `is_walkable(test_pos)` returns that target; a straight-line
        // check correctly rejects it. This is the concrete regression
        // from the post-fix 40k headless run where 3v0 kept targeting a
        // walkable tile behind a water wall and accumulating 329
        // PathBlocked failures on the same (68, 36) coordinate.
        let mut map = walkable_test_map();
        // Vertical water column at tile x=7 (world 112-128). Agent at
        // world (100, 100), fleeing east (away = +x). Cone candidates:
        //   50 px: (150, 100) — walkable destination, but path crosses
        //          water at (112-128, 100).
        //   35 px: (135, 100) — same.
        //   20 px: (120, 100) — also in water.
        // With angular offsets, ±30°/±60° at each distance land on y
        // values that still cross the water column.
        //
        // Expect pick_flee_target to reject every east-going candidate
        // and fall back to a random walkable tile (which won't be east
        // of the wall either, since no path goes through water — but
        // the fallback picks angles from the agent's position and the
        // walkable area directly around them).
        fill_rect(&mut map, 7, 0, 1, MAP_CHUNK_SIZE * 4, TileType::Water);
        let mut rng = StdRng::seed_from_u64(0);
        let pos = Vec2::new(100.0, 100.0);
        let away = Vec2::new(1.0, 0.0);

        let target = pick_flee_target(pos, away, &map, &mut rng);

        if let Some(target) = target {
            // Whatever we return must actually be reachable in a straight
            // line from the agent's position.
            assert!(
                straight_line_is_clear(pos, target, &map),
                "flee target {target:?} must have a clear straight line \
                 from agent at {pos:?}",
            );
            assert!(
                map.is_walkable(target),
                "flee target {target:?} must also be walkable",
            );
        }
    }

    #[test]
    fn flee_with_zero_away_vector_falls_back_to_random_walkable() {
        // Agent standing on the threat: (pos - threat_pos) normalises to
        // zero. The function must not return pos verbatim; it should fall
        // back to pick_random_walkable_target and actually try to move.
        let map = walkable_test_map();
        let mut rng = StdRng::seed_from_u64(0);
        let pos = Vec2::new(100.0, 100.0);
        let away = Vec2::ZERO;

        let target = pick_flee_target(pos, away, &map, &mut rng)
            .expect("open map must yield a fallback walkable target");

        assert!(
            (target - pos).length() > 1e-3,
            "zero-away fallback must actually move; got same position {target:?}",
        );
        assert!(map.is_walkable(target));
    }
}
