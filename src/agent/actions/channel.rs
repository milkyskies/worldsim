//! Body channel system - parallel action execution via body part resources.
//!
//! Actions consume body channels (Legs, Hands, Mouth, FullBody, Mind) at given
//! intensities. Multiple actions run in parallel when their channel requirements
//! don't conflict. Conflicts are resolved by saturation thresholds:
//!
//! - **Soft** (1.0..=1.4): both actions degrade in quality/speed.
//! - **Hard** (>1.4): the lower-urgency action is preempted.
//!
//! Reads: Body (for max channel capacity, hooked for #49)
//! Writes: nothing - this is a pure helper module
//! Upstream: actions::registry (Action trait body_channels()), biology::body
//! Downstream: nervous_system::execution, brains::arbitration

use crate::agent::biology::body::Body;
use bevy::prelude::*;
use std::collections::HashMap;

// ============================================================================
// THRESHOLDS
// ============================================================================

/// Saturation at which a channel begins degrading actions but still permits parallel use.
pub const SOFT_CONFLICT_THRESHOLD: f32 = 1.0;

/// Saturation above which actions hard-conflict and the lowest urgency must be preempted.
pub const HARD_CONFLICT_THRESHOLD: f32 = 1.4;

// ============================================================================
// BODY CHANNEL ENUM
// ============================================================================

/// A logical body resource that actions occupy.
///
/// Channels are categories of body usage rather than specific anatomical parts -
/// `Hands` aggregates both arms, `Legs` aggregates both legs, etc. Biology
/// integration (#49) maps anatomical body parts onto channel capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum BodyChannel {
    /// Locomotion - walking, running, fleeing
    Legs,
    /// Manipulation - eating, harvesting, attacking, holding
    Hands,
    /// Vocalization and consumption - talking, drinking, eating
    Mouth,
    /// Whole-body engagement - sleep, flee posture, falling
    FullBody,
    /// Cognition - never occupied, planning always runs
    Mind,
}

impl BodyChannel {
    /// All channels in iteration order.
    pub fn all() -> &'static [BodyChannel] {
        &[
            BodyChannel::Legs,
            BodyChannel::Hands,
            BodyChannel::Mouth,
            BodyChannel::FullBody,
            BodyChannel::Mind,
        ]
    }

    /// Maximum intensity available for this channel given the current body state.
    ///
    /// Defaults to `1.0` for every channel. Issue #49 wires this to the actual
    /// `BodyPart::function_rate` values so injuries reduce channel capacity.
    pub fn max_capacity(&self, _body: Option<&Body>) -> f32 {
        // TODO(#49): wire to body part function_rate
        // - Legs    -> avg(left_leg.function_rate, right_leg.function_rate)
        // - Hands   -> max(left_arm.function_rate, right_arm.function_rate)
        // - Mouth   -> head.function_rate
        // - FullBody -> min(torso.function_rate, head.function_rate)
        // - Mind    -> always 1.0
        1.0
    }
}

// ============================================================================
// CHANNEL USAGE
// ============================================================================

/// How much of a single body channel an action requires.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelUsage {
    pub channel: BodyChannel,
    /// 0.0 - 1.0, how demanding this action is on the channel.
    pub intensity: f32,
}

impl ChannelUsage {
    pub const fn new(channel: BodyChannel, intensity: f32) -> Self {
        Self { channel, intensity }
    }
}

// ============================================================================
// CHANNEL LOAD - aggregate state across running actions
// ============================================================================

/// Saturation of every channel summed across a set of actions.
///
/// Built once per tick from the running [`ActiveActions`](super::registry::ActiveActions)
/// and consumed by execution + arbitration to detect conflicts.
#[derive(Debug, Clone, Default)]
pub struct ChannelLoad {
    usage: HashMap<BodyChannel, f32>,
}

impl ChannelLoad {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a single action's usage to the load.
    pub fn add(&mut self, requirements: &[ChannelUsage]) {
        for usage in requirements {
            *self.usage.entry(usage.channel).or_insert(0.0) += usage.intensity;
        }
    }

    /// Remove a single action's usage from the load (used when previewing preemption).
    pub fn remove(&mut self, requirements: &[ChannelUsage]) {
        for usage in requirements {
            if let Some(load) = self.usage.get_mut(&usage.channel) {
                *load = (*load - usage.intensity).max(0.0);
            }
        }
    }

    /// Total intensity currently committed to a channel.
    pub fn saturation(&self, channel: BodyChannel) -> f32 {
        self.usage.get(&channel).copied().unwrap_or(0.0)
    }

    /// Adding `requirements` would push some channel above the hard threshold,
    /// after accounting for the body's max capacity per channel.
    pub fn would_hard_conflict(&self, requirements: &[ChannelUsage], body: Option<&Body>) -> bool {
        for usage in requirements {
            let cap = usage.channel.max_capacity(body);
            let projected = self.saturation(usage.channel) + usage.intensity;
            // Effective threshold scales down with reduced capacity, so a
            // half-functioning leg hard-conflicts at 0.7 instead of 1.4.
            if projected > HARD_CONFLICT_THRESHOLD * cap {
                return true;
            }
        }
        false
    }

    /// Adding `requirements` would push some channel into the soft band but
    /// not over the hard threshold.
    pub fn would_soft_conflict(&self, requirements: &[ChannelUsage], body: Option<&Body>) -> bool {
        if self.would_hard_conflict(requirements, body) {
            return false;
        }
        for usage in requirements {
            let cap = usage.channel.max_capacity(body);
            let projected = self.saturation(usage.channel) + usage.intensity;
            if projected > SOFT_CONFLICT_THRESHOLD * cap {
                return true;
            }
        }
        false
    }

    /// The strain factor for an action that requires `requirements`, given the
    /// current load. `1.0` means no degradation; values <1.0 mean tick rate /
    /// effects should be scaled down.
    ///
    /// Computed as `min(1.0 / saturation_of_primary_channel)` where saturation
    /// counts the action's own contribution. An action only running on its own
    /// always returns `1.0`.
    pub fn degradation_factor(&self, requirements: &[ChannelUsage], body: Option<&Body>) -> f32 {
        let mut min_factor: f32 = 1.0;
        for usage in requirements {
            let cap = usage.channel.max_capacity(body).max(0.001);
            let saturation = self.saturation(usage.channel) / cap;
            if saturation > 1.0 {
                let factor = 1.0 / saturation;
                if factor < min_factor {
                    min_factor = factor;
                }
            }
        }
        min_factor
    }
}

// ============================================================================
// CONFLICT
// ============================================================================

/// The result of testing whether an action can join the running set.
#[derive(Debug, Clone, PartialEq)]
pub enum ConflictKind {
    /// No channel saturation - action runs at full quality.
    None,
    /// Some channel is in the 1.0..=1.4 band - both contributing actions degrade.
    Soft,
    /// Some channel is over 1.4 - one of the conflicting actions must be preempted.
    Hard,
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn req(c: BodyChannel, i: f32) -> ChannelUsage {
        ChannelUsage::new(c, i)
    }

    #[test]
    fn empty_load_has_zero_saturation() {
        let load = ChannelLoad::new();
        for ch in BodyChannel::all() {
            assert_eq!(load.saturation(*ch), 0.0);
        }
    }

    #[test]
    fn adding_action_increases_saturation() {
        let mut load = ChannelLoad::new();
        load.add(&[req(BodyChannel::Hands, 0.5), req(BodyChannel::Mouth, 0.7)]);
        assert!((load.saturation(BodyChannel::Hands) - 0.5).abs() < 1e-6);
        assert!((load.saturation(BodyChannel::Mouth) - 0.7).abs() < 1e-6);
        assert_eq!(load.saturation(BodyChannel::Legs), 0.0);
    }

    #[test]
    fn walk_and_eat_have_no_conflict() {
        let mut load = ChannelLoad::new();
        // Walk: Legs 0.4
        load.add(&[req(BodyChannel::Legs, 0.4)]);
        // Eat: Hands 0.5, Mouth 0.7
        let eat = [req(BodyChannel::Hands, 0.5), req(BodyChannel::Mouth, 0.7)];
        assert!(!load.would_hard_conflict(&eat, None));
        assert!(!load.would_soft_conflict(&eat, None));
    }

    #[test]
    fn eat_plus_talk_is_soft_conflict() {
        let mut load = ChannelLoad::new();
        // Eat: Mouth 0.7
        load.add(&[req(BodyChannel::Hands, 0.5), req(BodyChannel::Mouth, 0.7)]);
        // Talk: Mouth 0.6 -> Mouth total 1.3 -> soft band
        let talk = [req(BodyChannel::Mouth, 0.6)];
        assert!(load.would_soft_conflict(&talk, None));
        assert!(!load.would_hard_conflict(&talk, None));
    }

    #[test]
    fn flee_plus_walk_is_hard_conflict() {
        let mut load = ChannelLoad::new();
        // Walk: Legs 0.4
        load.add(&[req(BodyChannel::Legs, 0.4)]);
        // Flee: Legs 1.0, FullBody 0.5 -> Legs total 1.4 (boundary). Use >1.4 to trip hard.
        let flee = [req(BodyChannel::Legs, 1.0), req(BodyChannel::FullBody, 0.5)];
        // 0.4 + 1.0 = 1.4, exactly at the boundary. Per spec: > 1.4 is hard.
        assert!(!load.would_hard_conflict(&flee, None));
        // But adding even slight extra walking pushes it over.
        load.add(&[req(BodyChannel::Legs, 0.05)]);
        assert!(load.would_hard_conflict(&flee, None));
    }

    #[test]
    fn sleep_full_body_blocks_other_full_body_actions() {
        let mut load = ChannelLoad::new();
        load.add(&[req(BodyChannel::FullBody, 1.0)]);
        // Adding another FullBody 1.0 -> 2.0, hard conflict.
        let other = [req(BodyChannel::FullBody, 1.0)];
        assert!(load.would_hard_conflict(&other, None));
    }

    #[test]
    fn degradation_factor_reduces_with_overload() {
        let mut load = ChannelLoad::new();
        // Eat (0.7) + Talk (0.6) on Mouth -> 1.3 saturation
        load.add(&[req(BodyChannel::Mouth, 0.7), req(BodyChannel::Mouth, 0.6)]);
        let eat = [req(BodyChannel::Mouth, 0.7), req(BodyChannel::Hands, 0.5)];
        let factor = load.degradation_factor(&eat, None);
        // 1.0 / 1.3 ≈ 0.77
        assert!(factor < 1.0);
        assert!(factor > 0.7);
    }

    #[test]
    fn degradation_factor_is_one_when_no_overload() {
        let mut load = ChannelLoad::new();
        load.add(&[req(BodyChannel::Mouth, 0.7)]);
        let eat = [req(BodyChannel::Mouth, 0.7)];
        // Saturation is exactly 0.7, no overload.
        assert_eq!(load.degradation_factor(&eat, None), 1.0);
    }

    #[test]
    fn body_max_capacity_defaults_to_one() {
        for ch in BodyChannel::all() {
            assert_eq!(ch.max_capacity(None), 1.0);
        }
    }

    #[test]
    fn remove_undoes_add() {
        let mut load = ChannelLoad::new();
        let req_walk = [req(BodyChannel::Legs, 0.4)];
        load.add(&req_walk);
        load.remove(&req_walk);
        assert_eq!(load.saturation(BodyChannel::Legs), 0.0);
    }
}
