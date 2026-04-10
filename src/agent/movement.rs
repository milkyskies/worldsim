//! Movement utilities: tick-based position stepping toward a target with speed modifiers for fatigue and injury.
//!
//! Reads: MovementState (last_tick), TickCount, PhysicalNeeds (stamina for speed penalty), Body (injury mobility), WorldMap (walkability)
//! Writes: Transform (position), MovementState (last_tick updated), MoveResult (Arrived/Moving/Blocked)
//! Upstream: constants::movement (speed/threshold values), world::map (walkability checks), body::needs (fatigue)
//! Downstream: action execution systems (call move_toward each tick), nervous_system (movement completes actions)

use crate::constants::movement::{
    BASE_SPEED_PER_TICK, EXHAUSTED_SPEED_MULTIPLIER, EXHAUSTED_STAMINA_THRESHOLD,
    INJURY_MOBILITY_RANGE, MIN_INJURY_MOBILITY, TIRED_SPEED_MULTIPLIER, TIRED_STAMINA_THRESHOLD,
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

/// Calculates movement speed based on stamina levels and body condition.
/// Returns pixels per tick (assuming 60 ticks/sec equivalent).
pub fn calculate_speed(stamina: f32, body: Option<&crate::agent::biology::body::Body>) -> f32 {
    // Speed = pixels per tick (at 60 ticks/sec, 1.0 = 60 px/sec equivalent)
    // FATIGUE PENALTY
    let mut speed_modifier = 1.0;
    if stamina < TIRED_STAMINA_THRESHOLD {
        speed_modifier = TIRED_SPEED_MULTIPLIER;
    }
    if stamina < EXHAUSTED_STAMINA_THRESHOLD {
        speed_modifier = EXHAUSTED_SPEED_MULTIPLIER;
    }

    // INJURY PENALTY
    let mut injury_modifier = 1.0;
    if let Some(body) = body {
        // Capability-level locomotion is the species-agnostic equivalent of
        // "how well the legs work" — works for quadrupeds, bipeds, and
        // whatever wing / tentacle anatomy shows up later.
        use crate::agent::actions::channel::Channel;
        let locomotion = body.channel_capacity(Channel::Locomotion);
        // Map 0.0-1.0 to MIN_INJURY_MOBILITY..1.0 (can always crawl a bit).
        // Wolves and deer have total Locomotion ~1.2 from four legs; clamp
        // to 1.0 so quadrupeds don't get a silent speed bonus from this
        // injury multiplier (they already get it from base_speed).
        let clamped = locomotion.min(1.0);
        injury_modifier = MIN_INJURY_MOBILITY + (clamped * INJURY_MOBILITY_RANGE);
    }

    BASE_SPEED_PER_TICK * speed_modifier * injury_modifier
}

/// Maps a locomotion intensity in [0, 1] to a speed multiplier applied on
/// top of [`calculate_speed`]. Calibrated so Walk's default (0.5) produces
/// 1.2x base and Flee's default (1.0) produces 2.0x base.
///
/// At intensity 0.0 the agent is still, not crawling — callers should
/// usually skip movement entirely rather than call this with 0.
pub fn intensity_speed_multiplier(intensity: f32) -> f32 {
    let i = intensity.clamp(0.0, 1.0);
    0.4 + i * 1.6
}

/// Graceful-degradation cap on desired locomotion intensity: if the body
/// can't deliver the requested intensity because stamina reserves are
/// depleted, return the highest intensity it actually *can* sustain. The
/// caller's desired intensity is preserved elsewhere (on `ActionState`) so
/// the agent's *intent* stays visible.
///
/// - Sprint (`> 0.7`) requires anaerobic reserve. With anaerobic empty,
///   caps at `0.5` (jog).
/// - Sustained (`> 0.3`) requires aerobic reserve. With aerobic also low,
///   caps at `0.3` (walk).
/// - Walk-or-slower always allowed.
pub fn effective_intensity(desired: f32, stamina: &crate::agent::body::needs::Stamina) -> f32 {
    let d = desired.clamp(0.0, 1.0);
    if d > 0.7 && stamina.anaerobic < 5.0 {
        return if stamina.aerobic < 10.0 { 0.3 } else { 0.5 };
    }
    if d > 0.3 && stamina.aerobic < 10.0 {
        return 0.3;
    }
    d
}

#[cfg(test)]
mod intensity_tests {
    use super::*;
    use crate::agent::body::needs::Stamina;

    #[test]
    fn walk_default_intensity_yields_1_2x_multiplier() {
        // Walk's default intensity is 0.5 → 0.4 + 0.8 = 1.2
        let m = intensity_speed_multiplier(0.5);
        assert!((m - 1.2).abs() < 1e-5, "expected 1.2x, got {m}");
    }

    #[test]
    fn flee_default_intensity_yields_2_0x_multiplier() {
        // Flee's default intensity is 1.0 → 0.4 + 1.6 = 2.0
        let m = intensity_speed_multiplier(1.0);
        assert!((m - 2.0).abs() < 1e-5, "expected 2.0x, got {m}");
    }

    #[test]
    fn effective_intensity_passes_through_when_rested() {
        let s = Stamina::default();
        assert_eq!(effective_intensity(1.0, &s), 1.0);
        assert_eq!(effective_intensity(0.5, &s), 0.5);
        assert_eq!(effective_intensity(0.2, &s), 0.2);
    }

    #[test]
    fn effective_intensity_caps_sprint_when_anaerobic_empty() {
        let s = Stamina {
            anaerobic: 0.0,
            aerobic: 80.0,
            ..Default::default()
        };
        // Desired sprint downgrades to jog (0.5) when anaerobic is empty
        // but aerobic remains.
        assert_eq!(effective_intensity(1.0, &s), 0.5);
    }

    #[test]
    fn effective_intensity_caps_to_walk_when_both_low() {
        let s = Stamina {
            anaerobic: 0.0,
            aerobic: 5.0,
            ..Default::default()
        };
        // Both pools empty: even sustained effort degrades to walk.
        assert_eq!(effective_intensity(1.0, &s), 0.3);
        assert_eq!(effective_intensity(0.5, &s), 0.3);
    }

    #[test]
    fn effective_intensity_does_not_upgrade() {
        // A walk-intensity desired never gets boosted.
        let s = Stamina::default();
        assert_eq!(effective_intensity(0.25, &s), 0.25);
    }
}
