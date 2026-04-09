//! Parallel action execution - ticks every running action independently.
//!
//! Reads: BrainState (chosen actions), PhysicalNeeds, Inventory, WorldMap, Body
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
use crate::agent::body::needs::{Consciousness, PhysicalNeeds};
use crate::agent::brains::proposal::BrainState;
use crate::agent::events::{ActionOutcome, ActionOutcomeEvent, NeedSatisfaction};
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::{MindGraph, Node, Predicate, Value};
use crate::agent::movement::{ARRIVAL_THRESHOLD, MoveResult, calculate_speed, move_toward};
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

            let Some(action_def) = registry.get(wanted_action) else {
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

            if matches!(action_def.kind(), ActionKind::Movement) {
                let pos = transform.translation.truncate();
                let new_target = match wanted_action {
                    ActionType::Explore => find_explore_target(pos, mind, &world_map, tick.current),
                    ActionType::Wander => pick_random_walkable_target(pos, &world_map, 10.0..30.0),
                    ActionType::Flee => {
                        if let Some(threat) = action_template.target_entity {
                            if let Ok(threat_t) = entity_transforms.get(threat) {
                                let threat_pos = threat_t.translation().truncate();
                                let away = (pos - threat_pos).normalize_or_zero();
                                Some(pos + away * 50.0)
                            } else {
                                pick_random_walkable_target(pos, &world_map, 30.0..60.0)
                            }
                        } else {
                            pick_random_walkable_target(pos, &world_map, 30.0..60.0)
                        }
                    }
                    ActionType::Walk => action_template.target_position,
                    _ => None,
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
    ) in agents.iter_mut()
    {
        // Snapshot the load and capacities at the start of the tick. Capacities
        // freeze the start-of-tick energy so degradation doesn't compound as
        // physical needs are mutated by per-action effects.
        let load = active.channel_load(&registry);
        let capacities = ChannelCapacities::compute(body, Some(&*physical));

        let mut completed_types: Vec<ActionType> = Vec::new();
        let mut target_gone_types: Vec<ActionType> = Vec::new();

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
                            true
                        } else {
                            let ticks =
                                current_tick.saturating_sub(action_state.last_movement_tick);
                            if ticks > 0 {
                                action_state.last_movement_tick = current_tick;
                                let mut speed =
                                    calculate_speed(physical.energy, None) * degradation;

                                if action_type == ActionType::Flee {
                                    speed *= 1.5;
                                }

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

            if completed {
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
            let pre_hunger = physical.hunger;
            let pre_thirst = physical.thirst;
            let pre_energy = physical.energy;

            let mut ctx = crate::agent::actions::registry::CompletionContext {
                physical: &mut physical,
                inventory: &mut inventory,
                drives: drives.as_deref_mut(),
                target_inventory: target_inv_ptr,
                target_entity: snapshot.target_entity,
                tick: current_tick,
            };

            action_def.on_complete(&mut ctx);

            // Only emit a success outcome when something observable changed.
            // Walk/Idle/Wander complete with no effects — skip the allocation.
            let hunger_reduced = pre_hunger - physical.hunger;
            let thirst_reduced = pre_thirst - physical.thirst;
            let energy_gained = physical.energy - pre_energy;
            if hunger_reduced > 0.0 || thirst_reduced > 0.0 || energy_gained > 0.0 {
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
                            energy_gained,
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
    let dt = tick.ticks_per_second / 3600.0;

    for (active, mut physical, mut consciousness, body) in agents.iter_mut() {
        let load = active.channel_load(&registry);
        // Capacities freeze the start-of-tick energy so degradation doesn't
        // compound as the loop mutates physical.energy mid-iteration.
        let capacities = ChannelCapacities::compute(body, Some(&*physical));

        for action_state in active.iter() {
            let Some(action_def) = registry.get(action_state.action_type) else {
                continue;
            };
            let effects = action_def.runtime_effects();
            let degradation = load.degradation_factor(action_def.body_channels(), &capacities);

            physical.energy =
                (physical.energy + effects.energy_per_sec * dt * degradation).clamp(0.0, 100.0);
            physical.hunger =
                (physical.hunger + effects.hunger_per_sec * dt * degradation).clamp(0.0, 100.0);
            consciousness.alertness = (consciousness.alertness
                + effects.alertness_per_sec * dt * 0.01 * degradation)
                .clamp(0.0, 1.0);
        }
    }
}

// ============================================================================
// Preemption helpers
// ============================================================================

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
) -> Option<Vec2> {
    let mut best_target: Option<Vec2> = None;
    let mut best_score = f32::MAX;
    let mut rng = rand::rng();
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
) -> Option<Vec2> {
    let mut rng = rand::rng();
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

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::actions::ActionType;

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
    fn sleep_preempts_other_actions() {
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

        assert!(admitted, "Sleep should clear interruptible slots");
        assert!(!active.contains(ActionType::Walk));
        assert!(!active.contains(ActionType::Eat));
    }

    #[test]
    fn eat_plus_converse_creates_soft_conflict_with_degradation() {
        let registry = build_registry();
        let mut active = ActiveActions::empty();
        active.insert(ActionState::new(ActionType::Eat, 0).with_duration(20));
        active.insert(ActionState::new(ActionType::Converse, 0));

        let load = active.channel_load(&registry);
        let eat_channels = registry.get(ActionType::Eat).unwrap().body_channels();
        let factor = load.degradation_factor(eat_channels, &ChannelCapacities::full());
        let expected = 1.0 / 1.3;
        assert!((factor - expected).abs() < 1e-4);
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
}
