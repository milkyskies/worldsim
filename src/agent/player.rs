//! Adventure-mode player control: marker + input system that lets one agent run on keyboard input instead of the AI brain stack.
//!
//! Reads: ButtonInput<KeyCode>, Transform, ActiveActions, WorldMap
//! Writes: PlayerControlled (marker), BrainState.chosen_actions (Walk template on directional input)
//! Upstream: ui/menu (inserts the marker on the chosen agent)
//! Downstream: brains::brain_system::arbitrate_every_tick, brains::rational::update_rational_planning (both filter `Without<PlayerControlled>`); nervous_system::execution::start_actions (consumes the Walk template)

use bevy::prelude::*;

use crate::agent::TargetPosition;
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

/// How far ahead of the agent the smooth-walk lookahead point sits
/// while a directional key is held. Long enough that the agent never
/// "arrives" mid-step (Walk would otherwise drop out of ActiveActions
/// and re-admit on the next tick, producing the step / pause / step
/// rhythm). Short enough that the brief overshoot when the player
/// releases the key is barely perceptible.
const SMOOTH_WALK_LOOKAHEAD: f32 = TILE_SIZE * 0.6;

/// Locomotion intensity for a normal walk. `intensity_speed_multiplier`
/// maps 0.5 → 1.2× base speed, matching the rational brain's default
/// Walk so the player's cruising speed is identical to AI agents.
const WALK_INTENSITY: f32 = 0.5;

/// Locomotion intensity while Shift is held. Maps to 2.0× base speed
/// and burns anaerobic stamina — same model the Flee action uses, so
/// sprinting tires the player just like it would tire any other agent.
const SPRINT_INTENSITY: f32 = 1.0;

/// Translate held WASD/arrow keys into smooth movement by keeping the
/// running Walk's target position pinned a short distance ahead of the
/// player every frame.
///
/// Why not "queue one Walk per tile"? That's what the first version did,
/// and it produced a step / pause / step cadence — the agent walked one
/// tile, the Walk action ended (`MoveResult::Arrived`), the next held-key
/// tick had to spawn a fresh Walk, etc. The pause is the unsmooth.
///
/// Instead: write the Walk template once, then each frame mutate the
/// active Walk's `target_position` to a moving point ahead of the agent.
/// `move_toward` never sees `Arrived` while the key is held, so movement
/// stays continuous at the existing speed-model rate (`calculate_speed`
/// × intensity × terrain). When the key is released, the target stays
/// where it last was and the agent walks to it and stops naturally —
/// no abrupt halt mid-step.
pub fn player_input(
    keyboard: Option<Res<ButtonInput<KeyCode>>>,
    map: Res<WorldMap>,
    mut query: Query<
        (
            &Transform,
            &mut BrainState,
            &mut ActiveActions,
            &mut TargetPosition,
        ),
        With<PlayerControlled>,
    >,
) {
    // Headless and TestWorld runs don't add Bevy's InputPlugin, so the
    // keyboard resource is genuinely absent there. Silently no-op
    // instead of panicking — there's nothing to do without input.
    let Some(keyboard) = keyboard else {
        return;
    };
    // Adventure mode runs with one possessed agent at a time. If somehow
    // multiple are marked we ignore the input entirely rather than
    // double-stepping a stale one.
    let Ok((transform, mut brain_state, mut active, mut target_position)) = query.single_mut()
    else {
        return;
    };
    let current_pos = transform.translation.truncate();

    // No directional key held → stop walking. Pin the active Walk's
    // target to the current position so the next movement tick observes
    // distance < ARRIVAL_THRESHOLD and completes the action cleanly
    // instead of carrying the player to a stale lookahead point.
    //
    // CRITICAL: only touch Walk. The context menu writes other actions
    // (Harvest, Eat, Fish, Build, …) into `chosen_actions`, and an
    // earlier version of this branch cleared the whole vec on every
    // tick a movement key wasn't held — which clobbered the menu's
    // intent before `start_actions` could admit it. Filter to Walk only.
    let Some(direction) = read_movement_direction(&keyboard) else {
        if let Some(state) = active.get_mut(ActionType::Walk) {
            state.target_position = Some(current_pos);
        }
        brain_state
            .chosen_actions
            .retain(|a| a.action_type != ActionType::Walk);
        // Don't blank the `TargetPosition` component — Harvest and
        // friends route through it for proximity navigation. Walk's
        // arrival will leave it pointing at current_pos, which is a
        // valid no-op for the next admission.
        return;
    };
    let sprinting = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let intensity = if sprinting {
        SPRINT_INTENSITY
    } else {
        WALK_INTENSITY
    };
    let lookahead = current_pos + direction * SMOOTH_WALK_LOOKAHEAD;
    // If the lookahead is into a wall, clamp to the agent's current tile
    // edge in that direction so we don't ask the movement system to walk
    // into rock. The `Gate::TileReachable` check inside Walk's admission
    // path also catches this, but clamping here keeps behavior stable
    // when the player is held against a wall.
    let target_pos = if map.is_walkable(lookahead) {
        lookahead
    } else {
        current_pos + direction * (TILE_SIZE * 0.4)
    };
    let (tx, ty) = map.world_to_tile(target_pos);

    // If a Walk is already running, just refresh its target_position
    // and intensity — start_actions would otherwise skip the new
    // template and the old target keeps pulling the agent toward the
    // original tile. Refreshing intensity each frame is what lets the
    // player tap-and-hold Shift to switch between walk and sprint
    // mid-step without re-admitting Walk.
    if let Some(state) = active.get_mut(ActionType::Walk) {
        state.target_position = Some(target_pos);
        state.locomotion_intensity = intensity;
        target_position.0 = Some(target_pos);
        // Keep chosen_actions in sync so the brain log and any debug
        // overlays see the live target — `start_actions` will see Walk
        // already running and short-circuit, so this is purely
        // observability, not an admission path.
        brain_state.chosen_actions = vec![build_walk_template(
            target_pos,
            (tx as i32, ty as i32),
            intensity,
        )];
        return;
    }

    // First step of a new walk burst: write the template so start_actions
    // admits it. Subsequent frames take the in-flight branch above.
    brain_state.chosen_actions = vec![build_walk_template(
        target_pos,
        (tx as i32, ty as i32),
        intensity,
    )];
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
/// `intensity` lets the caller switch between walk (`WALK_INTENSITY`)
/// and sprint (`SPRINT_INTENSITY`) without rebuilding the rest of the
/// template.
fn build_walk_template(world_pos: Vec2, tile: (i32, i32), intensity: f32) -> ActionTemplate {
    let behavior = Behavior::new(
        ActionPrimitive::Locomote,
        TargetSelector::InPlace,
        IntensityPolicy::Normal,
        Intent::Goal,
    );
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
        locomotion_intensity: intensity,
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
