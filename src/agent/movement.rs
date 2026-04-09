//! Movement utilities: tick-based position stepping toward a target with speed modifiers for fatigue and injury.
//!
//! Reads: MovementState (last_tick), TickCount, PhysicalNeeds (energy for speed penalty), Body (injury mobility), WorldMap (walkability)
//! Writes: Transform (position), MovementState (last_tick updated), MoveResult (Arrived/Moving/Blocked)
//! Upstream: constants::movement (speed/threshold values), world::map (walkability checks), body::needs (fatigue)
//! Downstream: action execution systems (call move_toward each tick), nervous_system (movement completes actions)

use crate::constants::movement::{
    BASE_SPEED_PER_TICK, EXHAUSTED_ENERGY_THRESHOLD, EXHAUSTED_SPEED_MULTIPLIER,
    INJURY_MOBILITY_RANGE, MIN_INJURY_MOBILITY, TIRED_ENERGY_THRESHOLD, TIRED_SPEED_MULTIPLIER,
};
use bevy::prelude::*;

/// Tracks movement timing for tick-based movement
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct MovementState {
    pub last_tick: u64,
}

/// Consistent arrival threshold for all movement types
pub const ARRIVAL_THRESHOLD: f32 = 2.0;

/// Calculate ticks elapsed since last movement, handling first-tick initialization.
/// Returns None if no ticks have passed (skip this frame).
pub fn calculate_ticks_elapsed(current_tick: u64, movement: &mut MovementState) -> Option<u64> {
    if movement.last_tick == 0 {
        // First tick - initialize and process one tick
        movement.last_tick = current_tick.saturating_sub(1);
        Some(1)
    } else {
        let elapsed = current_tick.saturating_sub(movement.last_tick);
        if elapsed == 0 {
            None // No ticks passed, skip
        } else {
            Some(elapsed)
        }
    }
}

/// Move toward a target position. Updates transform and returns the result.
pub fn move_toward(
    current_pos: Vec2,
    target_pos: Vec2,
    speed: f32,
    ticks: u64,
    map: &crate::world::map::WorldMap,
    transform: &mut Transform,
) -> MoveResult {
    let direction = target_pos - current_pos;
    let distance = direction.length();

    if distance < ARRIVAL_THRESHOLD {
        // Already at destination — snap to exact position so the perceived tile
        // matches the Walk effect's tile and is_step_complete returns true.
        transform.translation.x = target_pos.x;
        transform.translation.y = target_pos.y;
        return MoveResult::Arrived;
    }

    // Terrain at the agent's current tile slows movement (forest, sand, rock, etc.).
    // If the agent is off-map or somehow stuck on an impassable tile (multiplier 0),
    // fall back to full speed so they can escape rather than freeze in place.
    let terrain_mult = map
        .tile_at(current_pos)
        .map(|t| t.speed_multiplier())
        .filter(|m| *m > 0.0)
        .unwrap_or(1.0);
    let move_dist = speed * ticks as f32 * terrain_mult;
    let new_pos = if move_dist >= distance {
        target_pos
    } else {
        current_pos + direction.normalize() * move_dist
    };

    if map.is_walkable(new_pos) {
        let arrived = new_pos.distance(target_pos) < ARRIVAL_THRESHOLD;
        // Snap to exact target on arrival so the perceived tile always matches
        // the Walk effect's tile (prevents is_step_complete from staying false).
        let set_pos = if arrived { target_pos } else { new_pos };
        transform.translation.x = set_pos.x;
        transform.translation.y = set_pos.y;

        if arrived {
            MoveResult::Arrived
        } else {
            MoveResult::Moving
        }
    } else {
        MoveResult::Blocked
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveResult {
    Moving,
    Arrived,
    Blocked,
}

/// Calculates movement speed based on energy levels and body condition.
/// Returns pixels per tick (assuming 60 ticks/sec equivalent).
pub fn calculate_speed(energy: f32, body: Option<&crate::agent::biology::body::Body>) -> f32 {
    // Speed = pixels per tick (at 60 ticks/sec, 1.0 = 60 px/sec equivalent)
    // FATIGUE PENALTY
    let mut speed_modifier = 1.0;
    if energy < TIRED_ENERGY_THRESHOLD {
        speed_modifier = TIRED_SPEED_MULTIPLIER;
    }
    if energy < EXHAUSTED_ENERGY_THRESHOLD {
        speed_modifier = EXHAUSTED_SPEED_MULTIPLIER;
    }

    // INJURY PENALTY
    let mut injury_modifier = 1.0;
    if let Some(body) = body {
        // Legs determine movement speed.
        // Average function of both legs.
        let legs_function = (body.left_leg.function_rate + body.right_leg.function_rate) / 2.0;
        // Map 0.0-1.0 to MIN_INJURY_MOBILITY..1.0 (Can always crawl a bit)
        injury_modifier = MIN_INJURY_MOBILITY + (legs_function * INJURY_MOBILITY_RANGE);
    }

    BASE_SPEED_PER_TICK * speed_modifier * injury_modifier
}
