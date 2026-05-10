//! Adventure-mode player control: marker + input system that lets one agent run on keyboard input instead of the AI brain stack.
//!
//! Reads: ButtonInput<KeyCode>, Transform, ActiveActions, WorldMap
//! Writes: PlayerControlled (marker), BrainState.chosen_actions (Walk template on directional input)
//! Upstream: ui/menu (inserts the marker on the chosen agent)
//! Downstream: brains::brain_system::arbitrate_every_tick, brains::rational::update_rational_planning (both filter `Without<PlayerControlled>`); nervous_system::execution::start_actions (consumes the Walk template)

use bevy::prelude::*;

use crate::agent::actions::{
    ActionType, ActiveActions,
    motor::{ActionPrimitive, Behavior, IntensityPolicy, Intent, TargetSelector},
};
use crate::agent::brains::proposal::BrainState;
use crate::agent::brains::thinking::ActionTemplate;
use crate::agent::mind::knowledge::{Node as MindNode, Predicate, Triple, Value};
use crate::world::map::{TILE_SIZE, WorldMap};

/// Marker for an agent driven by player input rather than the brain stack.
///
/// `arbitrate_every_tick` and `update_rational_planning` both filter agents
/// carrying this marker. Every other system (perception, biology, action
/// execution, memory, conversations, ...) keeps running normally — the
/// marker only suppresses *decision-making*. Player input writes directly
/// into `BrainState.chosen_actions`, reusing the same execution pipeline
/// the AI brains feed.
#[derive(Component, Reflect, Default, Debug)]
#[reflect(Component)]
pub struct PlayerControlled;

/// Mark `entity` as player-controlled. AI brains will stop choosing
/// actions for it on the next brain tick.
pub fn possess(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).insert(PlayerControlled);
}

/// Drop player control from `entity`. AI brains resume on the next
/// brain tick — wakeup signals continue to fire while the marker is
/// present, so the next arbitration pass already has fresh inputs.
pub fn release(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).remove::<PlayerControlled>();
}

/// Translate held WASD/arrow keys into a Walk template aimed at the
/// adjacent tile in the held direction.
///
/// "Movement-state cooldown": we only queue a fresh walk while no Walk
/// is currently in flight. As soon as the previous step lands and Walk
/// drops out of `ActiveActions`, the next held-key tick re-fires —
/// giving the existing speed model (`calculate_speed` × intensity ×
/// terrain) authority over the per-step pacing.
pub fn player_input(
    keyboard: Option<Res<ButtonInput<KeyCode>>>,
    map: Res<WorldMap>,
    mut query: Query<(&Transform, &mut BrainState, &ActiveActions), With<PlayerControlled>>,
) {
    // Headless and TestWorld runs don't add Bevy's InputPlugin, so the
    // keyboard resource is genuinely absent there. Silently no-op
    // instead of panicking — there's nothing to do without input.
    let Some(keyboard) = keyboard else {
        return;
    };
    let Some(direction) = read_movement_direction(&keyboard) else {
        return;
    };
    // Adventure mode runs with one possessed agent at a time. If somehow
    // multiple are marked we ignore the input entirely rather than
    // double-stepping a stale one.
    let Ok((transform, mut brain_state, active)) = query.single_mut() else {
        return;
    };
    if active.contains(ActionType::Walk) {
        return;
    }
    let current_pos = transform.translation.truncate();
    let target_pos = current_pos + direction * TILE_SIZE;
    if !map.is_walkable(target_pos) {
        return;
    }
    let (tx, ty) = map.world_to_tile(target_pos);
    let snapped = map.tile_to_world(tx as i32, ty as i32);
    brain_state.chosen_actions = vec![build_walk_template(snapped, (tx as i32, ty as i32))];
}

/// 8-direction unit vector from currently-held WASD/arrow keys, or None
/// when no directional key is held.
fn read_movement_direction(keyboard: &ButtonInput<KeyCode>) -> Option<Vec2> {
    let mut dx = 0.0;
    let mut dy = 0.0;
    if keyboard.pressed(KeyCode::KeyA) || keyboard.pressed(KeyCode::ArrowLeft) {
        dx -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyD) || keyboard.pressed(KeyCode::ArrowRight) {
        dx += 1.0;
    }
    if keyboard.pressed(KeyCode::KeyS) || keyboard.pressed(KeyCode::ArrowDown) {
        dy -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyW) || keyboard.pressed(KeyCode::ArrowUp) {
        dy += 1.0;
    }
    if dx == 0.0 && dy == 0.0 {
        return None;
    }
    Some(Vec2::new(dx, dy).normalize())
}

/// Build the same minimal Walk template the regressive planner uses when
/// it inserts an implicit walk step. Mirrors `planner::build_walk_template`
/// — kept separate so `player.rs` doesn't depend on planner internals.
fn build_walk_template(world_pos: Vec2, tile: (i32, i32)) -> ActionTemplate {
    let behavior = Behavior::new(
        ActionPrimitive::Locomote,
        TargetSelector::InPlace,
        IntensityPolicy::Normal,
        Intent::Goal,
    );
    let locomotion_intensity = behavior.intensity.resolve();
    ActionTemplate {
        name: ActionType::Walk.name().to_string(),
        action_type: ActionType::Walk,
        behavior,
        target_entity: None,
        target_position: Some(world_pos),
        preconditions: Vec::new(),
        effects: vec![Triple::new(
            MindNode::Self_,
            Predicate::LocatedAt,
            Value::Tile(tile),
        )],
        consumes: Vec::new(),
        base_cost: 0.0,
        locomotion_intensity,
        estimated_duration_ticks: None,
        search_filter: None,
    }
}

/// Camera-follow lerp for adventure mode — hoisted so it can be unit-tested
/// without spinning up Bevy. Returns the new camera position; alpha is the
/// per-frame catch-up factor in [0, 1] (1.0 = snap, 0.0 = no follow).
pub fn follow_position(camera: Vec2, player: Vec2, alpha: f32) -> Vec2 {
    camera.lerp(player, alpha.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_direction_returns_none_with_no_keys() {
        let keys = ButtonInput::<KeyCode>::default();
        assert!(read_movement_direction(&keys).is_none());
    }

    #[test]
    fn read_direction_normalizes_diagonals() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyW);
        keys.press(KeyCode::KeyD);
        let dir = read_movement_direction(&keys).expect("WD should yield a direction");
        // (1, 1).normalize() = (sqrt(2)/2, sqrt(2)/2) ≈ (0.707, 0.707)
        let expected = Vec2::new(1.0, 1.0).normalize();
        assert!((dir - expected).length() < 1e-5, "got {dir:?}");
    }

    #[test]
    fn opposite_keys_cancel_to_no_input() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyA);
        keys.press(KeyCode::KeyD);
        // dx = -1 + 1 = 0, dy = 0 → no input.
        assert!(read_movement_direction(&keys).is_none());
    }

    #[test]
    fn arrow_keys_alias_to_wasd() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::ArrowUp);
        let dir = read_movement_direction(&keys).expect("ArrowUp should yield a direction");
        assert_eq!(dir, Vec2::new(0.0, 1.0));
    }

    #[test]
    fn follow_lerp_snaps_at_alpha_one() {
        let result = follow_position(Vec2::ZERO, Vec2::new(50.0, -20.0), 1.0);
        assert_eq!(result, Vec2::new(50.0, -20.0));
    }

    #[test]
    fn follow_lerp_holds_at_alpha_zero() {
        let result = follow_position(Vec2::new(10.0, 10.0), Vec2::new(50.0, -20.0), 0.0);
        assert_eq!(result, Vec2::new(10.0, 10.0));
    }

    #[test]
    fn follow_lerp_half_alpha_lands_midpoint() {
        let result = follow_position(Vec2::ZERO, Vec2::new(40.0, 80.0), 0.5);
        assert_eq!(result, Vec2::new(20.0, 40.0));
    }
}
