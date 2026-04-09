//! Body channel system - parallel action execution via body part resources.
//!
//! Actions consume body channels (Legs, Hands, Mouth, FullBody, Mind) at given
//! intensities. Multiple actions run in parallel when their channel requirements
//! don't conflict. Conflicts are resolved by saturation thresholds:
//!
//! - **Soft** (1.0..=1.4): both actions degrade in quality/speed.
//! - **Hard** (>1.4): the lower-urgency action is preempted.
//!
//! Reads: Body (for max channel capacity, hooked for biology integration)
//! Writes: nothing - this is a pure helper module
//! Upstream: actions::registry (Action trait body_channels()), biology::body
//! Downstream: nervous_system::execution, brains::arbitration

use crate::agent::biology::body::Body;
use bevy::prelude::*;

/// Saturation at which a channel begins degrading actions but still permits parallel use.
pub const SOFT_CONFLICT_THRESHOLD: f32 = 1.0;

/// Saturation above which actions hard-conflict and the lowest urgency must be preempted.
pub const HARD_CONFLICT_THRESHOLD: f32 = 1.4;

/// Number of distinct body channels - used for fixed-size load arrays.
pub const CHANNEL_COUNT: usize = 5;

/// A logical body resource that actions occupy.
///
/// Channels are categories of body usage rather than specific anatomical parts -
/// `Hands` aggregates both arms, `Legs` aggregates both legs, etc. Biology
/// integration maps anatomical body parts onto channel capacity.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum BodyChannel {
    /// Locomotion - walking, running, fleeing
    Legs = 0,
    /// Manipulation - eating, harvesting, attacking, holding
    Hands = 1,
    /// Vocalization and consumption - talking, drinking, eating
    Mouth = 2,
    /// Whole-body engagement - sleep, flee posture, falling
    FullBody = 3,
    /// Cognition - never occupied, planning always runs
    Mind = 4,
}

impl BodyChannel {
    /// All channels in iteration order.
    pub const ALL: [BodyChannel; CHANNEL_COUNT] = [
        BodyChannel::Legs,
        BodyChannel::Hands,
        BodyChannel::Mouth,
        BodyChannel::FullBody,
        BodyChannel::Mind,
    ];

    #[inline]
    pub const fn idx(self) -> usize {
        self as usize
    }

    /// Maximum intensity available for this channel given the current body state.
    ///
    /// Returns `1.0` for every channel until the biology integration wires this
    /// to anatomical part `function_rate`. The intended mapping mirrors the
    /// existing aggregators on `Body`:
    /// - `Legs` -> `Body::mobility()` (avg of leg function_rates)
    /// - `Hands` -> `Body::manipulation()` (max of arm function_rates)
    /// - `Mouth` -> head function_rate
    /// - `FullBody` -> min(torso, head)
    /// - `Mind` -> always 1.0
    pub fn max_capacity(&self, _body: Option<&Body>) -> f32 {
        // TODO: wire to Body::mobility() / Body::manipulation() / head.function_rate
        1.0
    }
}

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

/// Saturation of every channel summed across a set of actions.
///
/// Backed by a fixed-size array indexed by `BodyChannel as usize` so the hot
/// `tick_actions` and `apply_action_effects` loops never hash or allocate.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChannelLoad {
    usage: [f32; CHANNEL_COUNT],
}

impl ChannelLoad {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a single action's usage to the load.
    pub fn add(&mut self, requirements: &[ChannelUsage]) {
        for usage in requirements {
            self.usage[usage.channel.idx()] += usage.intensity;
        }
    }

    /// Remove a single action's usage from the load (used when previewing preemption).
    pub fn remove(&mut self, requirements: &[ChannelUsage]) {
        for usage in requirements {
            let slot = &mut self.usage[usage.channel.idx()];
            *slot = (*slot - usage.intensity).max(0.0);
        }
    }

    /// Total intensity currently committed to a channel.
    #[inline]
    pub fn saturation(&self, channel: BodyChannel) -> f32 {
        self.usage[channel.idx()]
    }

    /// Adding `requirements` would push some channel above the hard threshold,
    /// after accounting for the body's max capacity per channel.
    pub fn would_hard_conflict(&self, requirements: &[ChannelUsage], body: Option<&Body>) -> bool {
        for usage in requirements {
            let cap = usage.channel.max_capacity(body);
            let projected = self.saturation(usage.channel) + usage.intensity;
            // Effective threshold scales with capacity, so a half-functioning
            // leg hard-conflicts at 0.7 instead of 1.4.
            if projected > HARD_CONFLICT_THRESHOLD * cap {
                return true;
            }
        }
        false
    }

    /// Adding `requirements` would push some channel into the soft band but
    /// not over the hard threshold.
    pub fn would_soft_conflict(&self, requirements: &[ChannelUsage], body: Option<&Body>) -> bool {
        let mut soft = false;
        for usage in requirements {
            let cap = usage.channel.max_capacity(body);
            let projected = self.saturation(usage.channel) + usage.intensity;
            if projected > HARD_CONFLICT_THRESHOLD * cap {
                return false;
            }
            if projected > SOFT_CONFLICT_THRESHOLD * cap {
                soft = true;
            }
        }
        soft
    }

    /// The strain factor for an action that requires `requirements`, given the
    /// current load. `1.0` means no degradation; values <1.0 mean tick rate /
    /// effects should be scaled down.
    pub fn degradation_factor(&self, requirements: &[ChannelUsage], body: Option<&Body>) -> f32 {
        let mut min_factor: f32 = 1.0;
        for usage in requirements {
            let cap = usage.channel.max_capacity(body).max(0.001);
            let saturation = self.saturation(usage.channel) / cap;
            if saturation > SOFT_CONFLICT_THRESHOLD {
                let factor = 1.0 / saturation;
                if factor < min_factor {
                    min_factor = factor;
                }
            }
        }
        min_factor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(c: BodyChannel, i: f32) -> ChannelUsage {
        ChannelUsage::new(c, i)
    }

    #[test]
    fn empty_load_has_zero_saturation() {
        let load = ChannelLoad::new();
        for ch in BodyChannel::ALL {
            assert_eq!(load.saturation(ch), 0.0);
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
        load.add(&[req(BodyChannel::Legs, 0.4)]);
        let eat = [req(BodyChannel::Hands, 0.5), req(BodyChannel::Mouth, 0.7)];
        assert!(!load.would_hard_conflict(&eat, None));
        assert!(!load.would_soft_conflict(&eat, None));
    }

    #[test]
    fn eat_plus_talk_is_soft_conflict() {
        let mut load = ChannelLoad::new();
        load.add(&[req(BodyChannel::Hands, 0.5), req(BodyChannel::Mouth, 0.7)]);
        let talk = [req(BodyChannel::Mouth, 0.6)];
        assert!(load.would_soft_conflict(&talk, None));
        assert!(!load.would_hard_conflict(&talk, None));
    }

    #[test]
    fn flee_plus_walk_crosses_hard_threshold_when_pushed_over() {
        let mut load = ChannelLoad::new();
        load.add(&[req(BodyChannel::Legs, 0.4)]);
        let flee = [req(BodyChannel::Legs, 1.0), req(BodyChannel::FullBody, 0.5)];
        // 0.4 + 1.0 = 1.4, exactly at HARD_CONFLICT_THRESHOLD - not over.
        assert!(!load.would_hard_conflict(&flee, None));
        // Anything past the threshold trips hard.
        load.add(&[req(BodyChannel::Legs, 0.05)]);
        assert!(load.would_hard_conflict(&flee, None));
    }

    #[test]
    fn sleep_full_body_blocks_other_full_body_actions() {
        let mut load = ChannelLoad::new();
        load.add(&[req(BodyChannel::FullBody, 1.0)]);
        let other = [req(BodyChannel::FullBody, 1.0)];
        assert!(load.would_hard_conflict(&other, None));
    }

    #[test]
    fn degradation_factor_reduces_with_overload() {
        let mut load = ChannelLoad::new();
        load.add(&[req(BodyChannel::Mouth, 0.7), req(BodyChannel::Mouth, 0.6)]);
        let eat = [req(BodyChannel::Mouth, 0.7), req(BodyChannel::Hands, 0.5)];
        let factor = load.degradation_factor(&eat, None);
        let expected = 1.0 / 1.3;
        assert!((factor - expected).abs() < 1e-4);
    }

    #[test]
    fn degradation_factor_is_one_when_no_overload() {
        let mut load = ChannelLoad::new();
        load.add(&[req(BodyChannel::Mouth, 0.7)]);
        let eat = [req(BodyChannel::Mouth, 0.7)];
        assert_eq!(load.degradation_factor(&eat, None), 1.0);
    }

    #[test]
    fn body_max_capacity_defaults_to_one() {
        for ch in BodyChannel::ALL {
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
