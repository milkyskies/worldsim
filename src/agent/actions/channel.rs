//! Capability channels — parallel action execution via capability resources.
//!
//! Actions declare which [`Channel`] capabilities they occupy at what
//! intensity, and multiple actions run in parallel when their requirements
//! don't conflict. Conflicts are resolved by saturation thresholds:
//!
//! - **Soft** (1.0..=1.4): both actions degrade in quality/speed.
//! - **Hard** (>1.4): the lower-urgency action is preempted.
//!
//! Capabilities are decoupled from anatomy: a wolf's jaws and a human's hands
//! both provide `Manipulation`, at different intensities. The `Body` module
//! owns which parts offer which channels; this module just consumes the
//! aggregate `channel_capacity`.
//!
//! Reads: Body (per-channel capacity after injury), PhysicalNeeds (stamina for exhaustion), Consciousness (alertness for cognitive channels)
//! Writes: nothing - this is a pure helper module
//! Upstream: actions::registry (Action trait body_channels()), biology::body, body::needs
//! Downstream: nervous_system::execution, brains::arbitration

use crate::agent::biology::body::{Body, TagChannelMapping};
use crate::agent::body::needs::Consciousness;
use crate::agent::body::needs::PhysicalNeeds;
use crate::constants::movement::{TIRED_SPEED_MULTIPLIER, TIRED_STAMINA_THRESHOLD};
use bevy::prelude::*;

/// Saturation at which a channel begins degrading actions but still permits parallel use.
pub const SOFT_CONFLICT_THRESHOLD: f32 = 1.0;

/// Saturation above which actions hard-conflict and the lowest urgency must be preempted.
pub const HARD_CONFLICT_THRESHOLD: f32 = 1.4;

/// Number of distinct capability channels - used for fixed-size load arrays.
pub const CHANNEL_COUNT: usize = 9;

/// A capability an action occupies. Channels describe *what the body or
/// mind is doing*, not *which part is doing it* — a wolf and a human can
/// both satisfy `Manipulation`, but via different anatomy.
///
/// Physical channels (0-6) represent anatomical capabilities. Cognitive
/// channels (7-8) represent mental capacity — Focus for deliberate
/// processing and Awareness for environmental monitoring. Cognitive
/// channels are exempt from physical exhaustion but scale with alertness.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum Channel {
    /// Moving through space — walking, running, fleeing, swimming.
    Locomotion = 0,
    /// Grasping, holding, manipulating objects — harvest, attack, craft.
    Manipulation = 1,
    /// Eating, drinking, swallowing — takes the mouth/jaws/beak.
    Consumption = 2,
    /// Talking, howling, alarm-calling, barking.
    Vocalization = 3,
    /// Jaws-as-weapon — wolf attack, snake constrict, crocodile bite.
    Bite = 4,
    /// Holding an item while doing something else (free-hand carry).
    Carry = 5,
    /// Whole-body engagement — sleep, flee posture, falling. Abstract: not
    /// an anatomical part, but a "the whole animal is committed" gate.
    FullBody = 6,
    /// Deliberate processing — planning, deep conversation, complex
    /// problem-solving. Limited capacity: two Focus-heavy activities
    /// conflict. Capacity scales with `Consciousness.alertness`.
    Focus = 7,
    /// Environmental monitoring — threat detection, noticing nearby
    /// movement, ambient awareness. Deep focus reduces it (tunnel vision);
    /// fear demands it (hypervigilance). Capacity scales with alertness.
    Awareness = 8,
}

impl Channel {
    /// All channels in iteration order.
    pub const ALL: [Channel; CHANNEL_COUNT] = [
        Channel::Locomotion,
        Channel::Manipulation,
        Channel::Consumption,
        Channel::Vocalization,
        Channel::Bite,
        Channel::Carry,
        Channel::FullBody,
        Channel::Focus,
        Channel::Awareness,
    ];

    #[inline]
    pub const fn idx(self) -> usize {
        self as usize
    }

    /// Is this channel subject to physical exhaustion scaling? Abstract and
    /// cognitive channels are exempt so Sleep and planning stay reachable at
    /// zero stamina. Focus and Awareness scale with alertness instead (see
    /// `max_capacity`).
    #[inline]
    fn exhausts(self) -> bool {
        !matches!(
            self,
            Channel::FullBody | Channel::Focus | Channel::Awareness
        )
    }

    /// Is this a cognitive channel whose capacity scales with alertness
    /// rather than physical stamina?
    #[inline]
    pub fn is_cognitive(self) -> bool {
        matches!(self, Channel::Focus | Channel::Awareness)
    }

    /// Maximum intensity available for this channel given the current body,
    /// physical-needs, and consciousness state.
    ///
    /// Physical channels get their base capacity from [`Body::channel_capacity`]
    /// scaled by physical exhaustion. Cognitive channels (Focus, Awareness)
    /// get their base from the body and scale with `Consciousness.alertness`
    /// instead — a tired agent has reduced cognitive capacity.
    ///
    /// Per-tick callers should use [`ChannelCapacities::compute`] to evaluate
    /// every channel once and reuse the array.
    pub fn max_capacity(
        &self,
        body: Option<&Body>,
        physical: Option<&PhysicalNeeds>,
        consciousness: Option<&Consciousness>,
        mapping: &TagChannelMapping,
    ) -> f32 {
        if self.is_cognitive() {
            let base = body
                .map(|b| b.channel_capacity(*self, mapping))
                .unwrap_or(1.0);
            let alertness = consciousness.map(|c| c.alertness).unwrap_or(1.0);
            return base * alertness.clamp(0.0, 1.0);
        }

        let Some(body) = body else {
            return 1.0;
        };

        if body.is_incapacitated() {
            return match self {
                Channel::FullBody => 1.0,
                ch if ch.is_cognitive() => 1.0,
                _ => 0.0,
            };
        }

        let base = body.channel_capacity(*self, mapping);

        let exhaustion = if self.exhausts() {
            exhaustion_factor(physical)
        } else {
            1.0
        };

        base * exhaustion
    }
}

/// Computes the exhaustion multiplier from physical needs stamina.
/// Returns 1.0 above the threshold; scales to `TIRED_SPEED_MULTIPLIER` at zero stamina.
/// Reuses the same threshold and floor as `movement::calculate_speed` so the
/// "exhausted" curve is consistent across the codebase.
///
/// Gated on **aerobic** stamina — the sustained pool. Anaerobic is for sprint
/// bursts and recovers in seconds, so it doesn't represent durable fatigue.
fn exhaustion_factor(physical: Option<&PhysicalNeeds>) -> f32 {
    let Some(p) = physical else {
        return 1.0;
    };
    let aerobic = p.stamina.aerobic;
    if aerobic >= TIRED_STAMINA_THRESHOLD {
        return 1.0;
    }
    let aerobic_fraction = (aerobic / TIRED_STAMINA_THRESHOLD).clamp(0.0, 1.0);
    TIRED_SPEED_MULTIPLIER + aerobic_fraction * (1.0 - TIRED_SPEED_MULTIPLIER)
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
    pub fn compute(
        body: Option<&Body>,
        physical: Option<&PhysicalNeeds>,
        consciousness: Option<&Consciousness>,
        mapping: &TagChannelMapping,
    ) -> Self {
        let mut caps = [1.0; CHANNEL_COUNT];
        for ch in Channel::ALL {
            caps[ch.idx()] = ch.max_capacity(body, physical, consciousness, mapping);
        }
        Self(caps)
    }

    #[inline]
    pub fn get(&self, channel: Channel) -> f32 {
        self.0[channel.idx()]
    }
}

/// How much of a single capability channel an action requires.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelUsage {
    pub channel: Channel,
    /// 0.0 - 1.0, how demanding this action is on the channel.
    pub intensity: f32,
}

impl ChannelUsage {
    pub const fn new(channel: Channel, intensity: f32) -> Self {
        Self { channel, intensity }
    }
}

/// Prebuilt channel slices for actions that legitimately claim no body
/// channel at all — an explicit empty slice so the intent is visible in
/// the diff rather than hiding behind a default. `body_channels()` is
/// required on every `Action`, so this is the one named way to say
/// "this action doesn't touch any capability channel."
pub struct ChannelSlices;

impl ChannelSlices {
    pub const NONE: &'static [ChannelUsage] = &[];
}

/// How an agent's whole body is positioned. Orthogonal to body-part
/// channels: a `Stationary` agent can still use Manipulation, Consumption,
/// Focus, Awareness, Vocalization in parallel; they just can't also be `Moving`.
///
/// Starts as a binary enum. Extend to Sitting, Lying, Crouching, etc.
/// only when a feature actually needs the distinction — don't add
/// speculative granularity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Posture {
    /// Legs planted, body committed in one spot. Rest, Idle, Sleep, Eat,
    /// Harvest, Build — anything the agent does from a fixed position.
    Stationary,
    /// The agent's whole body is moving through space. Walk, Wander,
    /// Flee, Graze, Explore — any action whose core purpose is getting
    /// somewhere else.
    Moving,
}

/// Does `incoming` mutex with `running` at the posture layer?
///
/// `None` on either side means posture-agnostic and always compatible —
/// a charging wolf biting its prey, a runner shouting a greeting. Only
/// two declared-but-different postures conflict.
#[inline]
pub fn posture_conflict(incoming: Option<Posture>, running: Option<Posture>) -> bool {
    matches!((incoming, running), (Some(a), Some(b)) if a != b)
}

/// Saturation of every channel summed across a set of actions.
///
/// Backed by a fixed-size array indexed by `Channel as usize` so the hot
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
    pub fn saturation(&self, channel: Channel) -> f32 {
        self.usage[channel.idx()]
    }

    /// Adding `requirements` would push some channel to or above the hard
    /// threshold, after accounting for the body's max capacity per channel.
    /// The spec uses inclusive bounds: `Flee(Locomotion 1.0) +
    /// Walk(Locomotion 0.4) = 1.4` is a hard conflict, not a soft one.
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
    use crate::agent::biology::body::{BodyNodeKind, Injury, InjuryType};

    fn req(c: Channel, i: f32) -> ChannelUsage {
        ChannelUsage::new(c, i)
    }

    fn injure(part: &mut crate::agent::biology::body::BodyNode, severity: f32) {
        part.add_injury(Injury {
            injury_type: InjuryType::Fracture,
            severity,
            pain: 5.0,
            healed_amount: 0.0,
            bleed_rate: 0.0,
        });
    }

    /// Capacities snapshot used by tests that don't care about biology.
    fn full_caps() -> ChannelCapacities {
        ChannelCapacities::full()
    }

    fn caps_for(body: &Body, physical: Option<&PhysicalNeeds>) -> ChannelCapacities {
        ChannelCapacities::compute(Some(body), physical, None)
    }

    /// Smash the head hard enough to incapacitate and damage the brain inside.
    fn incapacitate(body: &mut Body) {
        let head = body
            .part_mut(BodyNodeKind::Head)
            .expect("human body has head");
        injure(head, 1.0);
        let head = body
            .part_mut(BodyNodeKind::Head)
            .expect("human body has head");
        injure(head, 1.0);
        assert!(body.is_incapacitated());
        // Severe head trauma damages everything inside.
        for kind in [
            BodyNodeKind::Brain,
            BodyNodeKind::LeftEye,
            BodyNodeKind::RightEye,
            BodyNodeKind::LeftEar,
            BodyNodeKind::RightEar,
            BodyNodeKind::Jaw,
        ] {
            let node = body.node_mut(kind).unwrap();
            injure(node, 1.0);
            let node = body.node_mut(kind).unwrap();
            injure(node, 1.0);
        }
    }

    #[test]
    fn empty_load_has_zero_saturation() {
        let load = ChannelLoad::new();
        for ch in Channel::ALL {
            assert_eq!(load.saturation(ch), 0.0);
        }
    }

    #[test]
    fn adding_action_increases_saturation() {
        let mut load = ChannelLoad::new();
        load.add(&[
            req(Channel::Manipulation, 0.5),
            req(Channel::Consumption, 0.7),
        ]);
        assert!((load.saturation(Channel::Manipulation) - 0.5).abs() < 1e-6);
        assert!((load.saturation(Channel::Consumption) - 0.7).abs() < 1e-6);
        assert_eq!(load.saturation(Channel::Locomotion), 0.0);
    }

    #[test]
    fn walk_and_eat_have_no_conflict() {
        let mut load = ChannelLoad::new();
        load.add(&[req(Channel::Locomotion, 0.4)]);
        let eat = [req(Channel::Consumption, 0.8)];
        let caps = full_caps();
        assert!(!load.would_hard_conflict(&eat, &caps));
        assert!(!load.would_soft_conflict(&eat, &caps));
    }

    #[test]
    fn eat_plus_talk_is_soft_conflict() {
        let mut load = ChannelLoad::new();
        load.add(&[req(Channel::Consumption, 0.8)]);
        let talk = [req(Channel::Vocalization, 0.6)];
        let caps = full_caps();
        // Talking and eating share the mouth anatomy on humans, but the
        // channel system treats them as independent capabilities now — a
        // human can (clumsily) do both. No conflict at the capability
        // layer; any "mouth full" quirk would have to be modeled as a body
        // part that provides both and gets saturated by part-level rules
        // (not this PR).
        assert!(!load.would_soft_conflict(&talk, &caps));
        assert!(!load.would_hard_conflict(&talk, &caps));
    }

    #[test]
    fn flee_plus_walk_hard_conflicts_at_threshold() {
        let mut load = ChannelLoad::new();
        load.add(&[req(Channel::Locomotion, 0.4)]);
        let flee = [req(Channel::Locomotion, 1.0), req(Channel::FullBody, 0.5)];
        // 0.4 + 1.0 = 1.4 lands exactly at HARD_CONFLICT_THRESHOLD - the spec
        // example treats this as a hard conflict (Walk gets preempted).
        assert!(load.would_hard_conflict(&flee, &full_caps()));
    }

    #[test]
    fn sleep_full_body_blocks_other_full_body_actions() {
        let mut load = ChannelLoad::new();
        load.add(&[req(Channel::FullBody, 1.0)]);
        let other = [req(Channel::FullBody, 1.0)];
        assert!(load.would_hard_conflict(&other, &full_caps()));
    }

    #[test]
    fn degradation_factor_reduces_with_overload() {
        let mut load = ChannelLoad::new();
        load.add(&[
            req(Channel::Consumption, 0.7),
            req(Channel::Consumption, 0.6),
        ]);
        let eat = [req(Channel::Consumption, 0.7)];
        let factor = load.degradation_factor(&eat, &full_caps());
        let expected = 1.0 / 1.3;
        assert!((factor - expected).abs() < 1e-4);
    }

    #[test]
    fn degradation_factor_is_one_when_no_overload() {
        let mut load = ChannelLoad::new();
        load.add(&[req(Channel::Consumption, 0.7)]);
        let eat = [req(Channel::Consumption, 0.7)];
        assert_eq!(load.degradation_factor(&eat, &full_caps()), 1.0);
    }

    #[test]
    fn body_max_capacity_defaults_to_one_when_no_body() {
        for ch in Channel::ALL {
            assert_eq!(ch.max_capacity(None, None, None), 1.0);
        }
    }

    #[test]
    fn remove_undoes_add() {
        let mut load = ChannelLoad::new();
        let req_walk = [req(Channel::Locomotion, 0.4)];
        load.add(&req_walk);
        load.remove(&req_walk);
        assert_eq!(load.saturation(Channel::Locomotion), 0.0);
    }

    // ----- Biology integration -----

    #[test]
    fn healthy_human_body_has_full_capacity_on_common_channels() {
        let body = Body::human();
        for ch in [
            Channel::Locomotion,
            Channel::Manipulation,
            Channel::Consumption,
            Channel::Vocalization,
            Channel::FullBody,
            Channel::Focus,
        ] {
            assert_eq!(
                ch.max_capacity(Some(&body), None, None),
                1.0,
                "{ch:?} should be 1.0 on a healthy human body"
            );
        }
    }

    #[test]
    fn healthy_human_has_weak_bite() {
        let body = Body::human();
        let bite = Channel::Bite.max_capacity(Some(&body), None, None);
        assert!(
            (bite - 0.1).abs() < 1e-6,
            "humans have weak Bite 0.1 from jaw, got {bite}"
        );
    }

    #[test]
    fn wolf_has_bite_but_limited_manipulation() {
        let body = Body::wolf();
        let bite = Channel::Bite.max_capacity(Some(&body), None, None);
        let manip = Channel::Manipulation.max_capacity(Some(&body), None, None);
        assert!(bite >= 1.0, "wolf jaws should provide Bite 1.0, got {bite}");
        assert!(
            (manip - 0.4).abs() < 1e-4,
            "wolf jaws should cap Manipulation at 0.4, got {manip}"
        );
    }

    #[test]
    fn deer_has_no_manipulation_or_bite() {
        let body = Body::deer();
        assert_eq!(
            Channel::Manipulation.max_capacity(Some(&body), None, None),
            0.0
        );
        assert_eq!(Channel::Bite.max_capacity(Some(&body), None, None), 0.0);
    }

    #[test]
    fn broken_foot_reduces_locomotion_capacity() {
        let mut body = Body::human();
        let foot = body
            .node_mut(BodyNodeKind::LeftFoot)
            .expect("human body has left foot");
        injure(foot, 1.0);
        let cap = Channel::Locomotion.max_capacity(Some(&body), None, None);
        assert!((cap - 0.5).abs() < 1e-4, "expected 0.5, got {cap}");
    }

    #[test]
    fn broken_hand_reduces_manipulation_capacity() {
        let mut body = Body::human();
        let hand = body
            .node_mut(BodyNodeKind::RightHand)
            .expect("human body has right hand");
        injure(hand, 1.0);
        let one_hand = Channel::Manipulation.max_capacity(Some(&body), None, None);
        assert!(
            (one_hand - 0.5).abs() < 1e-4,
            "expected 0.5 after one broken hand, got {one_hand}"
        );

        let hand = body
            .node_mut(BodyNodeKind::LeftHand)
            .expect("human body has left hand");
        injure(hand, 1.0);
        let cap_both = Channel::Manipulation.max_capacity(Some(&body), None, None);
        assert!(
            cap_both < 1e-4,
            "both hands broken should zero Manipulation, got {cap_both}"
        );
    }

    #[test]
    fn incapacitated_body_locks_active_channels_but_keeps_full_body_open() {
        let mut body = Body::human();
        incapacitate(&mut body);

        assert_eq!(
            Channel::Locomotion.max_capacity(Some(&body), None, None),
            0.0
        );
        assert_eq!(
            Channel::Manipulation.max_capacity(Some(&body), None, None),
            0.0
        );
        assert_eq!(
            Channel::Consumption.max_capacity(Some(&body), None, None),
            0.0
        );
        assert_eq!(Channel::FullBody.max_capacity(Some(&body), None, None), 1.0);
        // Cognitive channels collapse too — a smashed head can't think.
        assert!(Channel::Focus.max_capacity(Some(&body), None, None) < 0.3);
        assert!(Channel::Awareness.max_capacity(Some(&body), None, None) < 0.3);
    }

    #[test]
    fn incapacitated_agent_cannot_start_walk_or_harvest() {
        let mut body = Body::human();
        incapacitate(&mut body);
        let caps = caps_for(&body, None);
        let load = ChannelLoad::new();
        let walk = [req(Channel::Locomotion, 0.4)];
        assert!(load.would_hard_conflict(&walk, &caps));
        let harvest = [
            req(Channel::Manipulation, 0.9),
            req(Channel::Locomotion, 0.2),
        ];
        assert!(load.would_hard_conflict(&harvest, &caps));
    }

    #[test]
    fn incapacitated_agent_falls_through_to_idle() {
        // An incapacitated agent cannot start Sleep either because Sleep
        // still requires a live brain/body, and active channels are locked
        // to 0. The brain falls through to Idle, which has no channel
        // requirements. Passive healing still ticks.
        let mut body = Body::human();
        incapacitate(&mut body);
        let caps = caps_for(&body, None);
        let load = ChannelLoad::new();
        let sleep = [
            req(Channel::Locomotion, 1.0),
            req(Channel::Manipulation, 1.0),
            req(Channel::Consumption, 1.0),
            req(Channel::FullBody, 1.0),
        ];
        assert!(load.would_hard_conflict(&sleep, &caps));
        // Idle has no channels, so no conflict regardless of capacities.
        assert!(!load.would_hard_conflict(&[], &caps));
    }

    #[test]
    fn exhaustion_scales_active_channels_only() {
        use crate::agent::body::needs::Stamina;
        let body = Body::human();
        let exhausted = PhysicalNeeds {
            stamina: Stamina {
                aerobic: 0.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // Active channels collapse to TIRED_SPEED_MULTIPLIER at zero stamina.
        let legs = Channel::Locomotion.max_capacity(Some(&body), Some(&exhausted), None);
        assert!((legs - TIRED_SPEED_MULTIPLIER).abs() < 1e-4);
        let hands = Channel::Manipulation.max_capacity(Some(&body), Some(&exhausted), None);
        assert!((hands - TIRED_SPEED_MULTIPLIER).abs() < 1e-4);
        // FullBody is exempt from exhaustion so Sleep is always reachable.
        assert_eq!(
            Channel::FullBody.max_capacity(Some(&body), Some(&exhausted), None),
            1.0
        );
        // Cognitive channels are exempt from physical exhaustion (they
        // scale with alertness, not stamina). With no Consciousness
        // provided, alertness defaults to 1.0.
        assert_eq!(
            Channel::Focus.max_capacity(Some(&body), Some(&exhausted), None),
            1.0
        );
        assert_eq!(
            Channel::Awareness.max_capacity(Some(&body), Some(&exhausted), None),
            1.0
        );
    }

    #[test]
    fn exhaustion_at_threshold_is_full_capacity() {
        use crate::agent::body::needs::Stamina;
        let body = Body::human();
        let rested = PhysicalNeeds {
            stamina: Stamina {
                aerobic: TIRED_STAMINA_THRESHOLD,
                ..Default::default()
            },
            ..Default::default()
        };
        for ch in [
            Channel::Locomotion,
            Channel::Manipulation,
            Channel::Consumption,
            Channel::FullBody,
            Channel::Focus,
        ] {
            assert_eq!(ch.max_capacity(Some(&body), Some(&rested), None), 1.0);
        }
    }

    #[test]
    fn exhaustion_at_midpoint_is_linear() {
        use crate::agent::body::needs::Stamina;
        // At half the threshold, the multiplier is halfway between the floor
        // and 1.0 - i.e. the linear ramp is honored.
        let body = Body::human();
        let half = PhysicalNeeds {
            stamina: Stamina {
                aerobic: TIRED_STAMINA_THRESHOLD / 2.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let expected = TIRED_SPEED_MULTIPLIER + 0.5 * (1.0 - TIRED_SPEED_MULTIPLIER);
        let cap = Channel::Locomotion.max_capacity(Some(&body), Some(&half), None);
        assert!(
            (cap - expected).abs() < 1e-4,
            "expected {expected}, got {cap}"
        );
    }

    #[test]
    fn exhausted_agent_cannot_flee_but_can_walk() {
        use crate::agent::body::needs::Stamina;
        let body = Body::human();
        let exhausted = PhysicalNeeds {
            stamina: Stamina {
                aerobic: 0.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let caps = caps_for(&body, Some(&exhausted));
        let load = ChannelLoad::new();
        let walk = [req(Channel::Locomotion, 0.4)];
        assert!(!load.would_hard_conflict(&walk, &caps));
        let flee = [req(Channel::Locomotion, 1.0), req(Channel::FullBody, 0.5)];
        assert!(load.would_hard_conflict(&flee, &caps));
    }

    #[test]
    fn posture_conflict_rejects_opposed_postures() {
        assert!(posture_conflict(
            Some(Posture::Stationary),
            Some(Posture::Moving)
        ));
        assert!(posture_conflict(
            Some(Posture::Moving),
            Some(Posture::Stationary)
        ));
    }

    #[test]
    fn posture_conflict_accepts_identical_postures() {
        assert!(!posture_conflict(
            Some(Posture::Stationary),
            Some(Posture::Stationary)
        ));
        assert!(!posture_conflict(
            Some(Posture::Moving),
            Some(Posture::Moving)
        ));
    }

    #[test]
    fn posture_conflict_treats_none_as_compatible_with_anything() {
        assert!(!posture_conflict(None, Some(Posture::Stationary)));
        assert!(!posture_conflict(None, Some(Posture::Moving)));
        assert!(!posture_conflict(Some(Posture::Stationary), None));
        assert!(!posture_conflict(Some(Posture::Moving), None));
        assert!(!posture_conflict(None, None));
    }

    #[test]
    fn channel_capacities_compute_matches_per_channel_max_capacity() {
        use crate::agent::body::needs::Stamina;
        let mut body = Body::human();
        let foot = body.node_mut(BodyNodeKind::LeftFoot).unwrap();
        injure(foot, 0.5);
        let physical = PhysicalNeeds {
            stamina: Stamina {
                aerobic: 10.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let caps = ChannelCapacities::compute(Some(&body), Some(&physical), None);
        for ch in Channel::ALL {
            assert_eq!(
                caps.get(ch),
                ch.max_capacity(Some(&body), Some(&physical), None)
            );
        }
    }

    #[test]
    fn cognitive_channels_scale_with_alertness() {
        let body = Body::human();
        let tired = Consciousness { alertness: 0.5 };
        let focus = Channel::Focus.max_capacity(Some(&body), None, Some(&tired));
        let awareness = Channel::Awareness.max_capacity(Some(&body), None, Some(&tired));
        assert!(
            (focus - 0.5).abs() < 1e-4,
            "Focus should scale to 0.5 at half alertness, got {focus}"
        );
        assert!(
            (awareness - 0.5).abs() < 1e-4,
            "Awareness should scale to 0.5 at half alertness, got {awareness}"
        );

        let rested = Consciousness { alertness: 1.0 };
        assert_eq!(
            Channel::Focus.max_capacity(Some(&body), None, Some(&rested)),
            1.0
        );
        assert_eq!(
            Channel::Awareness.max_capacity(Some(&body), None, Some(&rested)),
            1.0
        );
    }

    #[test]
    fn cognitive_channels_not_affected_by_physical_exhaustion() {
        use crate::agent::body::needs::Stamina;
        let body = Body::human();
        let exhausted = PhysicalNeeds {
            stamina: Stamina {
                aerobic: 0.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let focus = Channel::Focus.max_capacity(Some(&body), Some(&exhausted), None);
        let awareness = Channel::Awareness.max_capacity(Some(&body), Some(&exhausted), None);
        assert_eq!(
            focus, 1.0,
            "Focus should not be reduced by physical exhaustion"
        );
        assert_eq!(
            awareness, 1.0,
            "Awareness should not be reduced by physical exhaustion"
        );

        let legs = Channel::Locomotion.max_capacity(Some(&body), Some(&exhausted), None);
        assert!(legs < 1.0, "Locomotion should be reduced by exhaustion");
    }

    #[test]
    fn converse_and_harvest_have_no_hard_focus_conflict() {
        let mut load = ChannelLoad::new();
        let converse = [
            req(Channel::Vocalization, 0.6),
            req(Channel::Focus, 0.6),
            req(Channel::Awareness, 0.3),
        ];
        load.add(&converse);

        let harvest = [req(Channel::Manipulation, 0.9), req(Channel::Focus, 0.1)];
        assert!(
            !load.would_hard_conflict(&harvest, &full_caps()),
            "Converse (Focus 0.6) + Harvest (Focus 0.1) = 0.7, no hard conflict"
        );
    }

    #[test]
    fn converse_and_observe_fit_at_full_alertness() {
        let mut load = ChannelLoad::new();
        let converse = [
            req(Channel::Vocalization, 0.6),
            req(Channel::Focus, 0.6),
            req(Channel::Awareness, 0.3),
        ];
        load.add(&converse);

        let observe = [req(Channel::Focus, 0.3), req(Channel::Awareness, 0.6)];
        assert!(
            !load.would_hard_conflict(&observe, &full_caps()),
            "Converse + Observe should fit at full alertness (Focus 0.9, Awareness 0.9)"
        );

        // But at half alertness, cognitive capacity is 0.5 — Focus 0.9 > 0.7
        // (= 0.5 * 1.4 hard threshold) means hard conflict.
        let tired = Consciousness { alertness: 0.5 };
        let tired_caps = ChannelCapacities::compute(Some(&Body::human()), None, Some(&tired));
        assert!(
            load.would_hard_conflict(&observe, &tired_caps),
            "Converse + Observe should hard-conflict when tired (Focus 0.9 > capacity 0.5)"
        );
    }
}
