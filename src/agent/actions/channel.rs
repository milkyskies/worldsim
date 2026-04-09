//! Body channel system - parallel action execution via body part resources.
//!
//! Actions consume body channels (Legs, Hands, Mouth, FullBody, Mind) at given
//! intensities. Multiple actions run in parallel when their channel requirements
//! don't conflict. Conflicts are resolved by saturation thresholds:
//!
//! - **Soft** (1.0..=1.4): both actions degrade in quality/speed.
//! - **Hard** (>1.4): the lower-urgency action is preempted.
//!
//! Reads: Body (function_rate per part), PhysicalNeeds (energy for exhaustion)
//! Writes: nothing - this is a pure helper module
//! Upstream: actions::registry (Action trait body_channels()), biology::body, body::needs
//! Downstream: nervous_system::execution, brains::arbitration

use crate::agent::biology::body::Body;
use crate::agent::body::needs::PhysicalNeeds;
use crate::constants::movement::{TIRED_ENERGY_THRESHOLD, TIRED_SPEED_MULTIPLIER};
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

    /// Maximum intensity available for this channel given the current body and
    /// physical-needs state.
    ///
    /// Mapping (for non-incapacitated agents):
    /// - `Legs`     -> `Body::mobility()` (avg of leg function_rates)
    /// - `Hands`    -> `Body::manipulation()` (best arm function_rate)
    /// - `Mouth`    -> head function_rate
    /// - `FullBody` -> min(torso, head)
    /// - `Mind`     -> always 1.0
    ///
    /// Modifiers:
    /// - **Incapacitation** (`Body::is_incapacitated()`): Legs/Hands/Mouth lock
    ///   to 0.0. The brain falls through to Idle (no channel requirements);
    ///   passive healing still ticks regardless of action state.
    /// - **Exhaustion** (`PhysicalNeeds::energy < TIRED_ENERGY_THRESHOLD`):
    ///   scales the active channels (Legs/Hands/Mouth) by
    ///   `TIRED_SPEED_MULTIPLIER..=1.0`. FullBody and Mind are exempt.
    ///
    /// Per-tick callers should use [`ChannelCapacities::compute`] to evaluate
    /// every channel once and reuse the array. This direct method is for
    /// one-shot lookups (tests, debug UI).
    pub fn max_capacity(&self, body: Option<&Body>, physical: Option<&PhysicalNeeds>) -> f32 {
        if matches!(self, BodyChannel::Mind) {
            return 1.0;
        }

        let Some(body) = body else {
            return 1.0;
        };

        if body.is_incapacitated() {
            return match self {
                BodyChannel::Legs | BodyChannel::Hands | BodyChannel::Mouth => 0.0,
                BodyChannel::FullBody | BodyChannel::Mind => 1.0,
            };
        }

        let base = match self {
            BodyChannel::Legs => body.mobility(),
            BodyChannel::Hands => body.manipulation(),
            BodyChannel::Mouth => body.head.function_rate,
            BodyChannel::FullBody => body.torso.function_rate.min(body.head.function_rate),
            BodyChannel::Mind => 1.0,
        };

        // FullBody is exempt from exhaustion so Sleep stays accessible at zero energy.
        let exhaustion = if matches!(self, BodyChannel::FullBody) {
            1.0
        } else {
            exhaustion_factor(physical)
        };

        base * exhaustion
    }
}

/// Computes the exhaustion multiplier from physical needs energy.
/// Returns 1.0 above the threshold; scales to `TIRED_SPEED_MULTIPLIER` at zero energy.
/// Reuses the same threshold and floor as `movement::calculate_speed` so the
/// "exhausted" curve is consistent across the codebase.
fn exhaustion_factor(physical: Option<&PhysicalNeeds>) -> f32 {
    let Some(p) = physical else {
        return 1.0;
    };
    if p.energy >= TIRED_ENERGY_THRESHOLD {
        return 1.0;
    }
    let energy_fraction = (p.energy / TIRED_ENERGY_THRESHOLD).clamp(0.0, 1.0);
    TIRED_SPEED_MULTIPLIER + energy_fraction * (1.0 - TIRED_SPEED_MULTIPLIER)
}

/// Per-channel capacity snapshot, computed once per agent per tick.
///
/// Built once via [`ChannelCapacities::compute`] from `Body` + `PhysicalNeeds`,
/// then passed by reference into [`ChannelLoad`] methods. Avoids recomputing
/// `max_capacity` (which dispatches on incapacitation, exhaustion, and body
/// part aggregators) for every conflict check or degradation lookup.
#[derive(Debug, Clone, Copy)]
pub struct ChannelCapacities([f32; CHANNEL_COUNT]);

impl Default for ChannelCapacities {
    fn default() -> Self {
        Self::full()
    }
}

impl ChannelCapacities {
    /// All channels at 1.0 - the default for entities without a body or
    /// for tests that don't care about biology.
    pub const fn full() -> Self {
        Self([1.0; CHANNEL_COUNT])
    }

    /// Compute the per-channel capacity snapshot for an agent's current state.
    pub fn compute(body: Option<&Body>, physical: Option<&PhysicalNeeds>) -> Self {
        let mut caps = [1.0; CHANNEL_COUNT];
        for ch in BodyChannel::ALL {
            caps[ch.idx()] = ch.max_capacity(body, physical);
        }
        Self(caps)
    }

    #[inline]
    pub fn get(&self, channel: BodyChannel) -> f32 {
        self.0[channel.idx()]
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

    /// Adding `requirements` would push some channel to or above the hard
    /// threshold, after accounting for the body's max capacity per channel.
    /// The spec uses inclusive bounds: `Flee(Legs 1.0) + Walk(Legs 0.4) = 1.4`
    /// is a hard conflict, not a soft one.
    pub fn would_hard_conflict(
        &self,
        requirements: &[ChannelUsage],
        capacities: &ChannelCapacities,
    ) -> bool {
        for usage in requirements {
            let cap = capacities.get(usage.channel);
            let projected = self.saturation(usage.channel) + usage.intensity;
            // Effective threshold scales with capacity, so a half-functioning
            // leg hard-conflicts at 0.7 instead of 1.4.
            if projected >= HARD_CONFLICT_THRESHOLD * cap {
                return true;
            }
        }
        false
    }

    /// Adding `requirements` would push some channel into the soft band but
    /// not into the hard band.
    pub fn would_soft_conflict(
        &self,
        requirements: &[ChannelUsage],
        capacities: &ChannelCapacities,
    ) -> bool {
        let mut soft = false;
        for usage in requirements {
            let cap = capacities.get(usage.channel);
            let projected = self.saturation(usage.channel) + usage.intensity;
            if projected >= HARD_CONFLICT_THRESHOLD * cap {
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
    pub fn degradation_factor(
        &self,
        requirements: &[ChannelUsage],
        capacities: &ChannelCapacities,
    ) -> f32 {
        let mut min_factor: f32 = 1.0;
        for usage in requirements {
            let cap = capacities.get(usage.channel).max(0.001);
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
    use crate::agent::biology::body::{Injury, InjuryType};

    fn req(c: BodyChannel, i: f32) -> ChannelUsage {
        ChannelUsage::new(c, i)
    }

    fn injure(part: &mut crate::agent::biology::body::BodyPart, severity: f32) {
        part.add_injury(Injury {
            injury_type: InjuryType::Fracture,
            severity,
            pain: 5.0,
            healed_amount: 0.0,
        });
    }

    /// Capacities snapshot used by tests that don't care about biology.
    fn full_caps() -> ChannelCapacities {
        ChannelCapacities::full()
    }

    fn caps_for(body: &Body, physical: Option<&PhysicalNeeds>) -> ChannelCapacities {
        ChannelCapacities::compute(Some(body), physical)
    }

    /// Push two severe head injuries to cross the 0.2 incapacitation threshold.
    fn incapacitate(body: &mut Body) {
        injure(&mut body.head, 1.0);
        injure(&mut body.head, 1.0);
        assert!(body.is_incapacitated());
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
        let caps = full_caps();
        assert!(!load.would_hard_conflict(&eat, &caps));
        assert!(!load.would_soft_conflict(&eat, &caps));
    }

    #[test]
    fn eat_plus_talk_is_soft_conflict() {
        let mut load = ChannelLoad::new();
        load.add(&[req(BodyChannel::Hands, 0.5), req(BodyChannel::Mouth, 0.7)]);
        let talk = [req(BodyChannel::Mouth, 0.6)];
        let caps = full_caps();
        assert!(load.would_soft_conflict(&talk, &caps));
        assert!(!load.would_hard_conflict(&talk, &caps));
    }

    #[test]
    fn flee_plus_walk_hard_conflicts_at_threshold() {
        let mut load = ChannelLoad::new();
        load.add(&[req(BodyChannel::Legs, 0.4)]);
        let flee = [req(BodyChannel::Legs, 1.0), req(BodyChannel::FullBody, 0.5)];
        // 0.4 + 1.0 = 1.4 lands exactly at HARD_CONFLICT_THRESHOLD - the spec
        // example treats this as a hard conflict (Walk gets preempted).
        assert!(load.would_hard_conflict(&flee, &full_caps()));
    }

    #[test]
    fn sleep_full_body_blocks_other_full_body_actions() {
        let mut load = ChannelLoad::new();
        load.add(&[req(BodyChannel::FullBody, 1.0)]);
        let other = [req(BodyChannel::FullBody, 1.0)];
        assert!(load.would_hard_conflict(&other, &full_caps()));
    }

    #[test]
    fn degradation_factor_reduces_with_overload() {
        let mut load = ChannelLoad::new();
        load.add(&[req(BodyChannel::Mouth, 0.7), req(BodyChannel::Mouth, 0.6)]);
        let eat = [req(BodyChannel::Mouth, 0.7), req(BodyChannel::Hands, 0.5)];
        let factor = load.degradation_factor(&eat, &full_caps());
        let expected = 1.0 / 1.3;
        assert!((factor - expected).abs() < 1e-4);
    }

    #[test]
    fn degradation_factor_is_one_when_no_overload() {
        let mut load = ChannelLoad::new();
        load.add(&[req(BodyChannel::Mouth, 0.7)]);
        let eat = [req(BodyChannel::Mouth, 0.7)];
        assert_eq!(load.degradation_factor(&eat, &full_caps()), 1.0);
    }

    #[test]
    fn body_max_capacity_defaults_to_one_when_no_body() {
        for ch in BodyChannel::ALL {
            assert_eq!(ch.max_capacity(None, None), 1.0);
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

    // ----- Biology integration -----

    #[test]
    fn healthy_body_has_full_capacity() {
        let body = Body::default();
        for ch in BodyChannel::ALL {
            assert_eq!(ch.max_capacity(Some(&body), None), 1.0);
        }
    }

    #[test]
    fn broken_leg_reduces_legs_capacity() {
        let mut body = Body::default();
        injure(&mut body.left_leg, 1.0);
        // Severity 1.0 fracture takes left leg function_rate to 0; right leg
        // is unhurt, so mobility() = (0 + 1) / 2 = 0.5.
        let cap = BodyChannel::Legs.max_capacity(Some(&body), None);
        assert!((cap - 0.5).abs() < 1e-4, "expected 0.5, got {cap}");
    }

    #[test]
    fn broken_arm_reduces_hands_capacity() {
        let mut body = Body::default();
        injure(&mut body.right_arm, 1.0);
        // manipulation() takes the BEST arm, so left arm still works.
        assert_eq!(BodyChannel::Hands.max_capacity(Some(&body), None), 1.0);

        injure(&mut body.left_arm, 1.0);
        let cap_both = BodyChannel::Hands.max_capacity(Some(&body), None);
        assert!(cap_both < 1.0);
    }

    #[test]
    fn incapacitated_body_locks_active_channels_but_keeps_full_body_open() {
        let mut body = Body::default();
        incapacitate(&mut body);

        assert_eq!(BodyChannel::Legs.max_capacity(Some(&body), None), 0.0);
        assert_eq!(BodyChannel::Hands.max_capacity(Some(&body), None), 0.0);
        assert_eq!(BodyChannel::Mouth.max_capacity(Some(&body), None), 0.0);
        assert_eq!(BodyChannel::FullBody.max_capacity(Some(&body), None), 1.0);
        assert_eq!(BodyChannel::Mind.max_capacity(Some(&body), None), 1.0);
    }

    #[test]
    fn incapacitated_agent_cannot_start_walk_or_harvest() {
        let mut body = Body::default();
        incapacitate(&mut body);
        let caps = caps_for(&body, None);
        let load = ChannelLoad::new();
        let walk = [req(BodyChannel::Legs, 0.4)];
        assert!(load.would_hard_conflict(&walk, &caps));
        let harvest = [req(BodyChannel::Hands, 0.9), req(BodyChannel::Legs, 0.2)];
        assert!(load.would_hard_conflict(&harvest, &caps));
    }

    #[test]
    fn incapacitated_agent_falls_through_to_idle() {
        // With Sleep declaring all four active channels at 1.0, an
        // incapacitated agent (Legs/Hands/Mouth locked to 0) cannot start
        // Sleep either - the brain falls through to Idle, which has no
        // channel requirements. Passive healing still ticks.
        let mut body = Body::default();
        incapacitate(&mut body);
        let caps = caps_for(&body, None);
        let load = ChannelLoad::new();
        let sleep = [
            req(BodyChannel::Legs, 1.0),
            req(BodyChannel::Hands, 1.0),
            req(BodyChannel::Mouth, 1.0),
            req(BodyChannel::FullBody, 1.0),
        ];
        assert!(load.would_hard_conflict(&sleep, &caps));
        // Idle has no channels, so no conflict regardless of capacities.
        assert!(!load.would_hard_conflict(&[], &caps));
    }

    #[test]
    fn exhaustion_scales_active_channels_only() {
        let body = Body::default();
        let exhausted = PhysicalNeeds {
            energy: 0.0,
            ..Default::default()
        };
        // Active channels collapse to TIRED_SPEED_MULTIPLIER at zero energy.
        let legs = BodyChannel::Legs.max_capacity(Some(&body), Some(&exhausted));
        assert!((legs - TIRED_SPEED_MULTIPLIER).abs() < 1e-4);
        let hands = BodyChannel::Hands.max_capacity(Some(&body), Some(&exhausted));
        assert!((hands - TIRED_SPEED_MULTIPLIER).abs() < 1e-4);
        // FullBody and Mind are exempt so Sleep is always reachable.
        assert_eq!(
            BodyChannel::FullBody.max_capacity(Some(&body), Some(&exhausted)),
            1.0
        );
        assert_eq!(
            BodyChannel::Mind.max_capacity(Some(&body), Some(&exhausted)),
            1.0
        );
    }

    #[test]
    fn exhaustion_at_threshold_is_full_capacity() {
        let body = Body::default();
        let rested = PhysicalNeeds {
            energy: TIRED_ENERGY_THRESHOLD,
            ..Default::default()
        };
        for ch in BodyChannel::ALL {
            assert_eq!(ch.max_capacity(Some(&body), Some(&rested)), 1.0);
        }
    }

    #[test]
    fn exhaustion_at_midpoint_is_linear() {
        // At half the threshold, the multiplier is halfway between the floor
        // and 1.0 - i.e. the linear ramp is honored.
        let body = Body::default();
        let half = PhysicalNeeds {
            energy: TIRED_ENERGY_THRESHOLD / 2.0,
            ..Default::default()
        };
        let expected = TIRED_SPEED_MULTIPLIER + 0.5 * (1.0 - TIRED_SPEED_MULTIPLIER);
        let cap = BodyChannel::Legs.max_capacity(Some(&body), Some(&half));
        assert!(
            (cap - expected).abs() < 1e-4,
            "expected {expected}, got {cap}"
        );
    }

    #[test]
    fn exhausted_agent_cannot_flee_but_can_walk() {
        let body = Body::default();
        let exhausted = PhysicalNeeds {
            energy: 0.0,
            ..Default::default()
        };
        let caps = caps_for(&body, Some(&exhausted));
        let load = ChannelLoad::new();
        let walk = [req(BodyChannel::Legs, 0.4)];
        assert!(!load.would_hard_conflict(&walk, &caps));
        let flee = [req(BodyChannel::Legs, 1.0), req(BodyChannel::FullBody, 0.5)];
        assert!(load.would_hard_conflict(&flee, &caps));
    }

    #[test]
    fn channel_capacities_compute_matches_per_channel_max_capacity() {
        let mut body = Body::default();
        injure(&mut body.left_leg, 0.5);
        let physical = PhysicalNeeds {
            energy: 10.0,
            ..Default::default()
        };
        let caps = ChannelCapacities::compute(Some(&body), Some(&physical));
        for ch in BodyChannel::ALL {
            assert_eq!(caps.get(ch), ch.max_capacity(Some(&body), Some(&physical)));
        }
    }
}
