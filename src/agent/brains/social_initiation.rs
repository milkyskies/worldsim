//! Per-agent failure cooldowns for the emotional brain's
//! `InitiateConversation` proposer. Breaks the same-target retry storm
//! that would otherwise fire every brain tick after a `PathBlocked` or
//! `ConversationFull` failure.
//!
//! Reads: ActionOutcomeEvent (Failed for InitiateConversation)
//! Writes: SocialInitiationCooldowns

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::agent::events::{ActionOutcome, ActionOutcomeEvent};
use crate::core::tick::TickCount;

/// Ticks an `InitiateConversation` failure against a target gates further
/// attempts at the same target. 600 ticks ≈ 10 game-minutes at 1 tick =
/// 1 game-second — long enough to break a same-tick retry storm without
/// permanently writing off a partner whose path may open back up.
pub const SOCIAL_INITIATION_COOLDOWN_TICKS: u64 = 600;

/// Per-agent record of recent `InitiateConversation` failures, keyed by
/// the partner that the failure targeted. Entries older than
/// [`SOCIAL_INITIATION_COOLDOWN_TICKS`] are pruned lazily on read; the
/// emotional brain calls [`Self::is_on_cooldown`] before proposing.
#[derive(Component, Default, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct SocialInitiationCooldowns {
    entries: HashMap<Entity, u64>,
}

impl SocialInitiationCooldowns {
    pub fn is_on_cooldown(&self, target: Entity, now: u64) -> bool {
        self.entries
            .get(&target)
            .is_some_and(|&t| now.saturating_sub(t) < SOCIAL_INITIATION_COOLDOWN_TICKS)
    }

    pub fn record(&mut self, target: Entity, tick: u64) {
        self.entries
            .retain(|_, &mut t| tick.saturating_sub(t) < SOCIAL_INITIATION_COOLDOWN_TICKS);
        self.entries.insert(target, tick);
    }
}

/// Listens for `InitiateConversation` failures and records a per-target
/// cooldown on the initiating agent. Lazy-inserts the component if the
/// agent has never had one before.
pub fn record_social_initiation_failures(
    mut commands: Commands,
    tick: Res<TickCount>,
    mut outcomes: MessageReader<ActionOutcomeEvent>,
    mut cooldowns: Query<&mut SocialInitiationCooldowns>,
) {
    let now = tick.current;
    for event in outcomes.read() {
        let ActionOutcome::Failed {
            action: crate::agent::actions::ActionType::InitiateConversation,
            target: Some(target),
            ..
        } = event.outcome
        else {
            continue;
        };
        if let Ok(mut existing) = cooldowns.get_mut(event.actor) {
            existing.record(target, now);
        } else {
            let mut fresh = SocialInitiationCooldowns::default();
            fresh.record(target, now);
            commands.entity(event.actor).insert(fresh);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(id: u64) -> Entity {
        Entity::from_bits(id)
    }

    #[test]
    fn fresh_cooldowns_have_no_entries() {
        let cooldowns = SocialInitiationCooldowns::default();
        assert!(!cooldowns.is_on_cooldown(e(1), 0));
    }

    #[test]
    fn recorded_failure_blocks_target_inside_window() {
        let mut cooldowns = SocialInitiationCooldowns::default();
        cooldowns.record(e(7), 100);
        assert!(cooldowns.is_on_cooldown(e(7), 100));
        assert!(cooldowns.is_on_cooldown(e(7), 100 + SOCIAL_INITIATION_COOLDOWN_TICKS - 1));
    }

    #[test]
    fn cooldown_expires_after_window() {
        let mut cooldowns = SocialInitiationCooldowns::default();
        cooldowns.record(e(7), 100);
        assert!(!cooldowns.is_on_cooldown(e(7), 100 + SOCIAL_INITIATION_COOLDOWN_TICKS));
    }

    #[test]
    fn cooldown_is_per_target() {
        let mut cooldowns = SocialInitiationCooldowns::default();
        cooldowns.record(e(7), 100);
        assert!(!cooldowns.is_on_cooldown(e(8), 100));
    }
}
