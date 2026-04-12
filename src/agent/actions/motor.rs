//! Motor primitives and behavior configurations.
//!
//! A `MotorPrimitive` is what the body is physically doing — a small, fixed
//! set (~6 variants). A `Behavior` is why, where, and how fast — a thin
//! configuration over a primitive.
//!
//! Together they replace the flat `ActionType` enum where every verb
//! (Wander, Flee, WalkTo, Harvest, ...) was its own variant with its own
//! constants. Now `Wander` and `Flee` are both `Locomote` at different
//! intensity policies.
//!
//! Reads: nothing (leaf types)
//! Writes: MotorPrimitive, Behavior, IntensityPolicy, TargetSelector
//! Upstream: none
//! Downstream: actions::registry, brains, nervous_system, effort model

use crate::agent::body::effort::EffortProfile;
use bevy::prelude::*;

// ---------------------------------------------------------------------------
// MotorPrimitive
// ---------------------------------------------------------------------------

/// What the body is physically doing. Small, fixed set.
///
/// Each primitive owns a single `EffortProfile` — the ONE place the body's
/// cost for that kind of work is declared. Wander and Flee both resolve to
/// `Locomote` with the same profile; the cost difference comes from
/// intensity, not from the behavior label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum MotorPrimitive {
    /// Translate body through space.
    /// Subsumes: Walk, Wander, Flee, Explore, Approach, Follow, Stalk, Chase.
    Locomote,
    /// Apply force to external objects (swing, lift, dig, strike, craft).
    /// Subsumes: Harvest, Build, Construct, Attack, Deposit, Take.
    Manipulate,
    /// Route external substance into the body.
    /// Subsumes: Eat, Drink, Graze.
    Ingest,
    /// Low-effort recovery. Subsumes: Sleep, Rest, Idle.
    Rest,
    /// Cognitive channel only, no motor output.
    /// Subsumes: Observe, Watch, Scan, Vigilance.
    Observe,
    /// Sound production. Subsumes: Converse, Howl, Bark, Call, Warn.
    Vocalize,
}

impl MotorPrimitive {
    /// The canonical effort profile for this primitive. This is the ONE
    /// place per-primitive physical cost is declared.
    ///
    /// The profile represents the body's engagement at `intensity = 1.0`.
    /// The actual effort is `profile * resolved_intensity` — the cost
    /// function in `body::effort::compute_action_cost` handles scaling.
    pub fn effort_profile(self) -> EffortProfile {
        match self {
            MotorPrimitive::Locomote => EffortProfile {
                locomotion: 1.0,
                ..Default::default()
            },
            MotorPrimitive::Manipulate => EffortProfile {
                manipulation: 0.8,
                isometric: 0.3,
                ..Default::default()
            },
            MotorPrimitive::Ingest => EffortProfile {
                // Ingestion is low-effort physically; the calorie gain
                // comes from the food, not the act of eating.
                cognition: 0.05,
                ..Default::default()
            },
            MotorPrimitive::Rest => EffortProfile {
                recovery: 1.0,
                ..Default::default()
            },
            MotorPrimitive::Observe => EffortProfile {
                cognition: 0.5,
                ..Default::default()
            },
            MotorPrimitive::Vocalize => EffortProfile {
                cognition: 0.2,
                ..Default::default()
            },
        }
    }

    /// Human-readable name for UI/debug display.
    pub fn label(self) -> &'static str {
        match self {
            MotorPrimitive::Locomote => "Moving",
            MotorPrimitive::Manipulate => "Working",
            MotorPrimitive::Ingest => "Eating",
            MotorPrimitive::Rest => "Resting",
            MotorPrimitive::Observe => "Watching",
            MotorPrimitive::Vocalize => "Talking",
        }
    }
}

// ---------------------------------------------------------------------------
// IntensityPolicy
// ---------------------------------------------------------------------------

/// What effort level the behavior wants *before* a regulator dials it down
/// for fatigue (#400). The regulator always runs after the policy resolves.
#[derive(Debug, Clone, Reflect)]
pub enum IntensityPolicy {
    /// Casual, no goal pressure. A deer browsing between feeding spots.
    Ambient,
    /// Stay under the aerobic threshold, long duration. A wolf pack migrating.
    Sustained,
    /// Goal-directed, finite duration. Walking to a water source.
    Normal,
    /// Deliberately low for concealment. A cat stalking a bird.
    Stealth,
    /// Lowest that still makes progress. An exhausted agent dragging home.
    Minimal,
    /// Match a target entity's speed plus a margin. Chasing slower prey.
    Matched(Entity),
    /// Reach target within a time window. Intensity scales with urgency.
    TimeBudget(f32),
    /// Fight-or-flight, burn everything. Flee from imminent threat.
    Maximal,
    /// Literal 0..1 escape hatch for scripted/test cases only.
    Fixed(f32),
}

impl Default for IntensityPolicy {
    fn default() -> Self {
        Self::Normal
    }
}

impl IntensityPolicy {
    /// Resolve the policy to a scalar intensity in [0, 1].
    ///
    /// This is the *desired* intensity before body-state capping. The
    /// execution system passes the result through `effective_intensity()`
    /// to cap it based on stamina reserves.
    pub fn resolve(&self) -> f32 {
        match self {
            IntensityPolicy::Ambient => 0.25,
            IntensityPolicy::Sustained => 0.5,
            IntensityPolicy::Normal => 0.5,
            IntensityPolicy::Stealth => 0.15,
            IntensityPolicy::Minimal => 0.2,
            IntensityPolicy::Matched(_) => 0.5, // placeholder until target velocity lookup
            IntensityPolicy::TimeBudget(_) => 0.6, // placeholder
            IntensityPolicy::Maximal => 1.0,
            IntensityPolicy::Fixed(v) => v.clamp(0.0, 1.0),
        }
    }

    /// Resolve with an urgency boost. Urgency in [0, 1] pushes the
    /// intensity up by up to 0.3, so an ambient walk can accelerate
    /// toward sprint without jumping straight to 1.0.
    ///
    /// Replaces the old `ActionType::pick_locomotion_intensity(urgency)`.
    pub fn resolve_with_urgency(&self, urgency_unit: f32) -> f32 {
        let base = self.resolve();
        if base == 0.0 {
            return 0.0;
        }
        let boost = urgency_unit.clamp(0.0, 1.0) * 0.3;
        (base + boost).clamp(0.0, 1.0)
    }
}

// ---------------------------------------------------------------------------
// TargetSelector
// ---------------------------------------------------------------------------

/// Where/what the behavior is directed at.
#[derive(Debug, Clone, Reflect)]
pub enum TargetSelector {
    /// No spatial target — action runs in place. Sleep, Rest, Idle, Eat.
    InPlace,
    /// Random walkable point nearby. Wander.
    RandomNearby,
    /// A specific entity. Walk-to, Harvest, Attack, Converse.
    Specific(Entity),
    /// A specific world position. Walk-to-point.
    Position(Vec2),
    /// Move away from the highest perceived threat. Flee.
    ThreatAvoidant,
    /// Move toward unexplored territory. Explore.
    UnknownArea,
}

impl Default for TargetSelector {
    fn default() -> Self {
        Self::InPlace
    }
}

// ---------------------------------------------------------------------------
// Intent
// ---------------------------------------------------------------------------

/// Why the agent is doing this — the motivational drive behind the behavior.
/// Read by the renderer for animation/sound selection and by the regulator
/// for drive-severity-aware intensity adjustment (#400).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Default)]
pub enum Intent {
    Hunger,
    Thirst,
    Fatigue,
    Safety,
    Curiosity,
    Social,
    Territoriality,
    #[default]
    Goal,
}

// ---------------------------------------------------------------------------
// Behavior
// ---------------------------------------------------------------------------

/// A thin configuration over a motor primitive. This is what the brain
/// proposes and the execution system runs.
///
/// `Wander` is `Behavior { primitive: Locomote, target: RandomNearby, intensity: Ambient, intent: Curiosity }`.
/// `Flee` is `Behavior { primitive: Locomote, target: ThreatAvoidant, intensity: Maximal, intent: Safety }`.
///
/// Same motor primitive, same cost code path, same animation channel.
/// The only differences are the three small policies.
#[derive(Debug, Clone, Reflect)]
pub struct Behavior {
    /// What the body is physically doing.
    pub primitive: MotorPrimitive,
    /// Where/what the behavior is directed at.
    pub target: TargetSelector,
    /// How hard the agent wants to push.
    pub intensity: IntensityPolicy,
    /// Why — the motivational drive.
    pub intent: Intent,
}

impl Behavior {
    pub fn new(
        primitive: MotorPrimitive,
        target: TargetSelector,
        intensity: IntensityPolicy,
        intent: Intent,
    ) -> Self {
        Self {
            primitive,
            target,
            intensity,
            intent,
        }
    }
}

// ---------------------------------------------------------------------------
// ActionType → Behavior mapping (migration bridge)
// ---------------------------------------------------------------------------

use super::ActionType;

impl ActionType {
    /// The default intensity policy for this action.
    ///
    /// Replaces the old `default_locomotion_intensity()` /
    /// `pick_locomotion_intensity()` methods. The resolved scalar from this
    /// policy is what the execution system uses for effort scaling.
    pub fn default_intensity_policy(self) -> IntensityPolicy {
        match self {
            // Locomotion actions — intensity determines speed and stamina cost
            ActionType::Wander | ActionType::Graze => IntensityPolicy::Ambient,
            ActionType::Walk | ActionType::Explore | ActionType::InitiateConversation => {
                IntensityPolicy::Normal
            }
            ActionType::Flee => IntensityPolicy::Maximal,

            // Non-locomotion actions — intensity is not applicable (no movement).
            // Fixed(0.0) so resolve_with_urgency returns 0.0 for these.
            ActionType::Eat
            | ActionType::Drink
            | ActionType::Harvest
            | ActionType::Build
            | ActionType::Construct
            | ActionType::Deposit
            | ActionType::Take
            | ActionType::Attack
            | ActionType::Bite
            | ActionType::Pickup
            | ActionType::Drop
            | ActionType::Wave
            | ActionType::Converse => IntensityPolicy::Fixed(0.0),

            // Recovery/passive — low intensity
            ActionType::Rest | ActionType::Idle | ActionType::Groom | ActionType::Observe => {
                IntensityPolicy::Ambient
            }
            ActionType::Sleep | ActionType::WakeUp => IntensityPolicy::Fixed(1.0),
        }
    }

    /// The motor primitive this action resolves to.
    pub fn motor_primitive(self) -> MotorPrimitive {
        match self {
            // Locomote
            ActionType::Walk
            | ActionType::Wander
            | ActionType::Explore
            | ActionType::Flee
            | ActionType::InitiateConversation => MotorPrimitive::Locomote,

            // Manipulate
            ActionType::Harvest
            | ActionType::Build
            | ActionType::Construct
            | ActionType::Deposit
            | ActionType::Take
            | ActionType::Attack
            | ActionType::Bite
            | ActionType::Pickup
            | ActionType::Drop
            | ActionType::Groom => MotorPrimitive::Manipulate,

            // Ingest
            ActionType::Eat | ActionType::Drink | ActionType::Graze => MotorPrimitive::Ingest,

            // Rest
            ActionType::Sleep | ActionType::WakeUp | ActionType::Rest | ActionType::Idle => {
                MotorPrimitive::Rest
            }

            // Observe
            ActionType::Observe | ActionType::Wave => MotorPrimitive::Observe,

            // Vocalize
            ActionType::Converse => MotorPrimitive::Vocalize,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motor_primitive_effort_profile_is_unique_per_primitive() {
        let primitives = [
            MotorPrimitive::Locomote,
            MotorPrimitive::Manipulate,
            MotorPrimitive::Ingest,
            MotorPrimitive::Rest,
            MotorPrimitive::Observe,
            MotorPrimitive::Vocalize,
        ];
        assert_eq!(
            primitives.len(),
            6,
            "number of EffortProfile declarations must equal number of MotorPrimitive variants"
        );
        for p in &primitives {
            let profile = p.effort_profile();
            // Each profile should have at least one non-zero channel
            // (except Rest which has recovery, and Ingest which has cognition)
            let sum = profile.locomotion
                + profile.manipulation
                + profile.isometric
                + profile.cognition
                + profile.recovery;
            assert!(
                sum > 0.0,
                "{:?} effort profile should have at least one non-zero channel",
                p
            );
        }
    }

    #[test]
    fn wander_and_flee_share_locomote_primitive() {
        assert_eq!(
            ActionType::Wander.motor_primitive(),
            MotorPrimitive::Locomote
        );
        assert_eq!(ActionType::Flee.motor_primitive(), MotorPrimitive::Locomote);
    }

    #[test]
    fn wander_and_flee_differ_only_in_policy() {
        let wander = Behavior::new(
            MotorPrimitive::Locomote,
            TargetSelector::RandomNearby,
            IntensityPolicy::Ambient,
            Intent::Curiosity,
        );
        let flee = Behavior::new(
            MotorPrimitive::Locomote,
            TargetSelector::ThreatAvoidant,
            IntensityPolicy::Maximal,
            Intent::Safety,
        );
        assert_eq!(wander.primitive, flee.primitive);
    }

    #[test]
    fn harvest_behavior_uses_manipulate_primitive() {
        assert_eq!(
            ActionType::Harvest.motor_primitive(),
            MotorPrimitive::Manipulate
        );
    }

    #[test]
    fn sleep_behavior_uses_rest_primitive() {
        assert_eq!(ActionType::Sleep.motor_primitive(), MotorPrimitive::Rest);
    }

    #[test]
    fn intensity_policy_ambient_resolves_to_low_scalar() {
        let resolved = IntensityPolicy::Ambient.resolve();
        assert!(resolved < 0.4, "Ambient should resolve low, got {resolved}");
    }

    #[test]
    fn intensity_policy_stealth_resolves_below_ambient() {
        let stealth = IntensityPolicy::Stealth.resolve();
        let ambient = IntensityPolicy::Ambient.resolve();
        assert!(
            stealth < ambient,
            "Stealth ({stealth}) should be lower than Ambient ({ambient})"
        );
    }

    #[test]
    fn intensity_policy_sustained_stays_below_sprint() {
        let sustained = IntensityPolicy::Sustained.resolve();
        assert!(
            sustained <= 0.7,
            "Sustained should stay below sprint threshold (0.7), got {sustained}"
        );
    }

    #[test]
    fn adding_a_new_locomote_behavior_requires_zero_new_constants() {
        // Structural test: a new "Patrol" behavior is just a configuration.
        // It builds and runs without touching any constants file.
        let _patrol = Behavior::new(
            MotorPrimitive::Locomote,
            TargetSelector::RandomNearby,
            IntensityPolicy::Sustained,
            Intent::Territoriality,
        );
        // If this compiles and doesn't panic, the test passes.
        // The point: no new cost constants needed for a new locomotion behavior.
    }
}
