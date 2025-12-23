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
        // Already at destination
        return MoveResult::Arrived;
    }

    let move_dist = speed * ticks as f32;
    let new_pos = if move_dist >= distance {
        target_pos
    } else {
        current_pos + direction.normalize() * move_dist
    };

    if map.is_walkable(new_pos) {
        transform.translation.x = new_pos.x;
        transform.translation.y = new_pos.y;

        if new_pos.distance(target_pos) < ARRIVAL_THRESHOLD {
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
    let base_speed_per_tick = 0.8; // pixels per tick

    // FATIGUE PENALTY
    let mut speed_modifier = 1.0;
    if energy < 20.0 {
        speed_modifier = 0.5; // Tired: 50% speed
    }
    if energy < 5.0 {
        speed_modifier = 0.2; // Exhausted: 20% speed
    }

    // INJURY PENALTY
    let mut injury_modifier = 1.0;
    if let Some(body) = body {
        // Legs determine movement speed.
        // Average function of both legs.
        let legs_function = (body.left_leg.function_rate + body.right_leg.function_rate) / 2.0;
        // Map 0.0-1.0 to 0.1-1.0 (Can always crawl a bit)
        injury_modifier = 0.1 + (legs_function * 0.9);
    }

    base_speed_per_tick * speed_modifier * injury_modifier
}
