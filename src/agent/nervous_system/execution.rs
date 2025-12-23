//! Unified execution system - handles all action types based on declarative Action data.
//!
//! The system reads Action::kind() and Action::runtime_effects() to handle execution.
//! Actions define their own can_start() check - execution is generic.

use crate::agent::TargetPosition;
use crate::agent::actions::ActionType;
use crate::agent::actions::registry::{ActionContext, ActionKind, ActionRegistry, ActionState};
use crate::agent::body::needs::{Consciousness, PhysicalNeeds};
use crate::agent::brains::proposal::BrainState;
use crate::agent::events::{ActionOutcome, ActionOutcomeEvent};
use crate::agent::inventory::Inventory;
use crate::agent::mind::knowledge::{MindGraph, Node, Predicate, Value}; // Used by find_explore_target
use crate::agent::movement::{ARRIVAL_THRESHOLD, MoveResult, calculate_speed, move_toward};
use crate::core::tick::TickCount;
use crate::ui::hud::GameLog;
use crate::world::map::{CHUNK_SIZE, TILE_SIZE, WorldMap};
use bevy::prelude::*;
use rand::Rng;

/// System to start new actions based on brain decisions
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
        &mut ActionState,
        &BrainState,
        &MindGraph,
        &Inventory,
    )>,
    entity_transforms: Query<&GlobalTransform>,
    mut outcome_events: MessageWriter<ActionOutcomeEvent>,
) {
    for (_entity, name, transform, mut target, mut action_state, brain_state, mind, inventory) in
        agents.iter_mut()
    {
        // Check if brain wants a different action
        let Some(action_template) = &brain_state.chosen_action else {
            continue;
        };

        let wanted_action = action_template.action_type;

        // Only switch if it's a different action type
        if action_state.action_type == wanted_action {
            continue;
        }

        // Get action definition first
        let Some(action_def) = registry.get(wanted_action) else {
            continue;
        };

        // Create context for can_start check
        let ctx = ActionContext {
            inventory,
            mind,
            target_entity: action_template.target_entity,
            target_position: action_template.target_position,
            agent_position: transform.translation.truncate(),
        };

        // GENERIC: Use action's can_start method
        if let Err(reason) = action_def.can_start(&ctx) {
            game_log.log_debug(format!(
                "{} cannot start {:?}: {:?}",
                name.as_str(),
                wanted_action,
                reason
            ));

            // Emit failure event so beliefs get updated
            outcome_events.write(ActionOutcomeEvent {
                actor: _entity,
                outcome: ActionOutcome::Failed {
                    action: wanted_action,
                    target: action_template.target_entity,
                    reason,
                },
            });
            continue;
        }

        // Create new action state
        let mut new_state = ActionState::new(wanted_action, tick.current);

        // Set target from template
        if let Some(te) = action_template.target_entity {
            new_state = new_state.with_target_entity(te);
        }
        if let Some(tp) = action_template.target_position {
            new_state = new_state.with_target_position(tp);
        }
        if let Some(topic) = action_template.topic {
            new_state.topic = Some(topic);
        }
        new_state.content = action_template.content.clone();

        // Set duration for timed actions
        if let ActionKind::Timed { duration_ticks } = action_def.kind() {
            new_state = new_state.with_duration(duration_ticks);
        }

        // Find target for movement actions
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
                            // Threat entity gone, pick random direction
                            pick_random_walkable_target(pos, &world_map, 30.0..60.0)
                        }
                    } else {
                        // No specific threat, flee in random direction
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

        *action_state = new_state;

        // Log start message
        if let Some(msg) = action_def.start_log() {
            game_log.action(name.as_str(), msg, None, Some(_entity));
        }
    }
}

/// System to tick active actions
pub fn tick_actions(
    registry: Res<ActionRegistry>,
    tick: Res<TickCount>,
    world_map: Res<WorldMap>,
    mut game_log: ResMut<GameLog>,
    mut event_writer: MessageWriter<crate::agent::events::GameEvent>,
    mut conversation_manager: Option<ResMut<crate::agent::mind::conversation::ConversationManager>>,
    mut agents: Query<(
        Entity,
        &Name,
        &mut Transform,
        &mut TargetPosition,
        &mut ActionState,
        &mut PhysicalNeeds,
        &mut Inventory,
        Option<&mut crate::agent::body::needs::PsychologicalDrives>,
    )>,
    mut target_inventories: Query<&mut Inventory, Without<PhysicalNeeds>>,
) {
    let current_tick = tick.current;

    for (
        entity,
        name,
        mut transform,
        mut target_pos,
        mut action_state,
        mut physical,
        mut inventory,
        mut drives,
    ) in agents.iter_mut()
    {
        let action_type = action_state.action_type;
        let Some(action_def) = registry.get(action_type) else {
            continue;
        };

        let completed = match action_def.kind() {
            ActionKind::Instant => true,

            ActionKind::Timed { duration_ticks: _ } => {
                if action_state.ticks_remaining > 0 {
                    action_state.ticks_remaining = action_state.ticks_remaining.saturating_sub(1);
                }
                action_state.ticks_remaining == 0 && action_state.ticks_remaining != u32::MAX
            }

            ActionKind::Movement => {
                if let Some(target_position) = action_state.target_position {
                    let current_pos = transform.translation.truncate();

                    if current_pos.distance(target_position) < ARRIVAL_THRESHOLD {
                        true
                    } else {
                        // Move toward target
                        let ticks = current_tick.saturating_sub(action_state.last_movement_tick);
                        if ticks > 0 {
                            action_state.last_movement_tick = current_tick;
                            let mut speed = calculate_speed(physical.energy, None);

                            // Adrenaline boost when fleeing!
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
                                    // Path blocked - fail action
                                    game_log.log_debug(format!("{} path blocked", name.as_str()));
                                    true
                                }
                            }
                        } else {
                            false
                        }
                    }
                } else {
                    true // No target = done
                }
            }
        };

        if completed {
            // Get target's inventory if action has a target entity
            let mut target_inv = action_state
                .target_entity
                .and_then(|e| target_inventories.get_mut(e).ok());

            // Build completion context and let action handle its own effects
            // Prepare completion context
            // We need to handle the Option<ResMut> for conversation manager
            let mut conv_mgr_ptr = None;
            if let Some(ref mut mgr) = conversation_manager {
                conv_mgr_ptr = Some(mgr.as_mut());
            }
            let target_inv_ptr = target_inv.as_deref_mut();

            let mut ctx = crate::agent::actions::registry::CompletionContext {
                physical: &mut physical,
                inventory: &mut inventory,
                drives: drives.as_deref_mut(),
                target_inventory: target_inv_ptr,
                conversation_manager: conv_mgr_ptr,
                topic: action_state.topic,
                target_entity: action_state.target_entity,
                actor: entity,
                content: action_state.content.clone(),
                tick: current_tick,
            };

            // Declarative: action applies its own effects!
            action_def.on_complete(&mut ctx);

            // Emit social interaction event for social actions
            if (action_type == ActionType::Introduce || action_type == ActionType::Talk)
                && let Some(target_entity) = action_state.target_entity
            {
                event_writer.write(crate::agent::events::GameEvent::SocialInteraction {
                    actor: entity,
                    target: target_entity,
                    action: action_type,
                    topic: action_state.topic.map(|t| match t {
                        crate::agent::mind::conversation::Topic::General => {
                            crate::agent::events::ConversationTopic::Greetings
                        }
                        crate::agent::mind::conversation::Topic::Location(_) => {
                            crate::agent::events::ConversationTopic::Request
                        }
                        crate::agent::mind::conversation::Topic::State(_) => {
                            crate::agent::events::ConversationTopic::Knowledge
                        }
                        crate::agent::mind::conversation::Topic::Person(_) => {
                            crate::agent::events::ConversationTopic::Gossip
                        }
                        crate::agent::mind::conversation::Topic::Help => {
                            crate::agent::events::ConversationTopic::Request
                        }
                    }),
                    valence: 0.5, // Positive interaction
                });

                // Also emit KnowledgeShared if there's content to share
                if action_type == ActionType::Talk && !action_state.content.is_empty() {
                    event_writer.write(crate::agent::events::GameEvent::KnowledgeShared {
                        speaker: entity,
                        listener: target_entity,
                        content: action_state.content.clone(),
                    });
                }
            }

            // Log completion
            if let Some(msg) = action_def.complete_log() {
                game_log.action(name.as_str(), msg, None, Some(entity));
            }

            // Reset to idle
            *action_state = ActionState::new(ActionType::Idle, current_tick);
            target_pos.0 = None;
        }
    }
}

/// System to apply per-tick action effects
pub fn apply_action_effects(
    registry: Res<ActionRegistry>,
    tick: Res<TickCount>,
    mut agents: Query<(&ActionState, &mut PhysicalNeeds, &mut Consciousness)>,
) {
    let dt = tick.ticks_per_second / 3600.0;

    for (action_state, mut physical, mut consciousness) in agents.iter_mut() {
        if let Some(action_def) = registry.get(action_state.action_type) {
            let effects = action_def.runtime_effects();

            physical.energy = (physical.energy + effects.energy_per_sec * dt).clamp(0.0, 100.0);
            physical.hunger = (physical.hunger + effects.hunger_per_sec * dt).clamp(0.0, 100.0);
            consciousness.alertness =
                (consciousness.alertness + effects.alertness_per_sec * dt * 0.01).clamp(0.0, 1.0);
        }
    }
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
