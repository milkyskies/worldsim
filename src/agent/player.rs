//! Adventure-mode player control: marker that disables AI brain decisions for one agent so input drives it instead.
//!
//! Reads: nothing (leaf marker module)
//! Writes: PlayerControlled (marker component)
//! Upstream: ui/menu (inserts the marker on the chosen agent)
//! Downstream: brains::brain_system::arbitrate_every_tick, brains::rational::update_rational_planning (both filter `Without<PlayerControlled>`)

use bevy::prelude::*;

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
