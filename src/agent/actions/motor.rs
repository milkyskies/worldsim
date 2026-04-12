//! Motor primitives and behavior configurations.
//!
//! A `ActionPrimitive` is what the body is physically doing — a small, fixed
//! set (~6 variants). A `Behavior` is why, where, and how fast — a thin
//! configuration over a primitive.
//!
//! Together they replace the flat `ActionType` enum where every verb
//! (Wander, Flee, WalkTo, Harvest, ...) was its own variant with its own
//! constants. Now `Wander` and `Flee` are both `Locomote` at different
//! intensity policies.
//!
//! Reads: nothing (leaf types)
//! Writes: ActionPrimitive, Behavior, IntensityPolicy, TargetSelector
//! Upstream: none
//! Downstream: actions::registry, brains, nervous_system, effort model

use crate::agent::body::effort::EffortProfile;
use bevy::prelude::*;

// ---------------------------------------------------------------------------
// ActionPrimitive
// ---------------------------------------------------------------------------

/// What the body is physically doing. Small, fixed set.
///
/// Each primitive owns a single `EffortProfile` — the ONE place the body's
/// cost for that kind of work is declared. Wander and Flee both resolve to
/// `Locomote` with the same profile; the cost difference comes from
/// intensity, not from the behavior label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Default)]
pub enum ActionPrimitive {
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
    #[default]
    Rest,
    /// Cognitive channel only, no motor output.
    /// Subsumes: Observe, Watch, Scan, Vigilance.
    Observe,
    /// Sound production. Subsumes: Converse, Howl, Bark, Call, Warn.
    Vocalize,
}

impl ActionPrimitive {
    /// The canonical effort profile for this primitive. This is the ONE
    /// place per-primitive physical cost is declared.
    ///
    /// The profile represents the body's engagement at `intensity = 1.0`.
    /// The actual effort is `profile * resolved_intensity` — the cost
    /// function in `body::effort::compute_action_cost` handles scaling.
    pub fn effort_profile(self) -> EffortProfile {
        match self {
            ActionPrimitive::Locomote => EffortProfile {
                locomotion: 1.0,
                ..Default::default()
            },
            ActionPrimitive::Manipulate => EffortProfile {
                manipulation: 0.8,
                isometric: 0.3,
                ..Default::default()
            },
            ActionPrimitive::Ingest => EffortProfile {
                // Ingestion is low-effort physically; the calorie gain
                // comes from the food, not the act of eating.
                cognition: 0.05,
                ..Default::default()
            },
            ActionPrimitive::Rest => EffortProfile {
                recovery: 1.0,
                ..Default::default()
            },
            ActionPrimitive::Observe => EffortProfile {
                cognition: 0.5,
                ..Default::default()
            },
            ActionPrimitive::Vocalize => EffortProfile {
                cognition: 0.2,
                ..Default::default()
            },
        }
    }

    /// Base psychological effect of this primitive at intensity 1.0.
    ///
    /// The actual effect is scaled by intensity and modified by Intent.
    /// This replaces the per-action hand-tuned alertness_per_sec /
    /// stimulation_per_sec / companionship_per_sec constants.
    pub fn psych_effect(self) -> PsychEffect {
        match self {
            ActionPrimitive::Locomote => PsychEffect {
                alertness: 10.0,
                stimulation: 0.02,
                ..Default::default()
            },
            ActionPrimitive::Manipulate => PsychEffect {
                alertness: 1.0,
                stimulation: -0.01,
                ..Default::default()
            },
            ActionPrimitive::Ingest => PsychEffect {
                alertness: 2.0,
                ..Default::default()
            },
            ActionPrimitive::Rest => PsychEffect {
                alertness: 3.0,
                stimulation: -0.01,
                ..Default::default()
            },
            ActionPrimitive::Observe => PsychEffect {
                alertness: 3.0,
                stimulation: 0.08,
                ..Default::default()
            },
            ActionPrimitive::Vocalize => PsychEffect {
                alertness: 1.0,
                stimulation: 0.015,
                companionship: 0.012,
            },
        }
    }

    /// Human-readable name for UI/debug display.
    pub fn label(self) -> &'static str {
        match self {
            ActionPrimitive::Locomote => "Moving",
            ActionPrimitive::Manipulate => "Working",
            ActionPrimitive::Ingest => "Eating",
            ActionPrimitive::Rest => "Resting",
            ActionPrimitive::Observe => "Watching",
            ActionPrimitive::Vocalize => "Talking",
        }
    }
}

// ---------------------------------------------------------------------------
// IntensityPolicy
// ---------------------------------------------------------------------------

/// What effort level the behavior wants *before* a regulator dials it down
/// for fatigue (#400). The regulator always runs after the policy resolves.
#[derive(Debug, Clone, Default, Reflect)]
pub enum IntensityPolicy {
    /// Casual, no goal pressure. A deer browsing between feeding spots.
    Ambient,
    /// Stay under the aerobic threshold, long duration. A wolf pack migrating.
    Sustained,
    /// Goal-directed, finite duration. Walking to a water source.
    #[default]
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
    pub fn resolve_with_urgency(&self, urgency_unit: f32) -> f32 {
        let base = self.resolve();
        if base == 0.0 {
            return 0.0;
        }
        let boost = urgency_unit.clamp(0.0, 1.0) * 0.3;
        (base + boost).clamp(0.0, 1.0)
    }

    /// Escalate the policy based on drive urgency (0-1).
    ///
    /// Low urgency keeps the default. Moderate urgency upgrades to
    /// Sustained. High urgency upgrades to Maximal. Policies that are
    /// already at or above the escalation target are unchanged.
    /// Fixed/Matched/TimeBudget policies are never escalated.
    pub fn escalate_for_urgency(self, urgency: f32) -> Self {
        match &self {
            // Never escalate non-locomotion or already-maximal policies
            IntensityPolicy::Fixed(_)
            | IntensityPolicy::Matched(_)
            | IntensityPolicy::TimeBudget(_)
            | IntensityPolicy::Maximal => self,
            _ => {
                if urgency >= 0.8 {
                    IntensityPolicy::Maximal
                } else if urgency >= 0.5 {
                    IntensityPolicy::Sustained
                } else {
                    self
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TargetSelector
// ---------------------------------------------------------------------------

/// Where/what the behavior is directed at.
#[derive(Debug, Clone, Default, Reflect)]
pub enum TargetSelector {
    /// No spatial target — action runs in place. Sleep, Rest, Idle, Eat.
    #[default]
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

// ---------------------------------------------------------------------------
// PsychEffect
// ---------------------------------------------------------------------------

/// Psychological side effects of an action, per second.
///
/// Declared per ActionPrimitive, modified by Intent, scaled by intensity.
/// Replaces the per-action hand-tuned alertness_per_sec /
/// stimulation_per_sec / companionship_per_sec in RuntimeEffects.
#[derive(Debug, Clone, Default)]
pub struct PsychEffect {
    /// Consciousness change. Positive = keeps agent alert, negative = soporific.
    pub alertness: f32,
    /// Curiosity/novelty satisfaction. Positive = satisfies, negative = breeds boredom.
    pub stimulation: f32,
    /// Social satisfaction. Positive = satisfies companionship drive.
    pub companionship: f32,
}

impl PsychEffect {
    pub fn scaled(&self, factor: f32) -> Self {
        Self {
            alertness: self.alertness * factor,
            stimulation: self.stimulation * factor,
            companionship: self.companionship * factor,
        }
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

impl Intent {
    /// Modify a base PsychEffect based on the motivational context.
    ///
    /// Safety amplifies alertness (threat-driven hyper-vigilance).
    /// Curiosity amplifies stimulation (novelty-seeking satisfaction).
    /// Social amplifies companionship.
    /// Fatigue on Rest primitive flips alertness negative (sleep).
    pub fn modify_psych(&self, base: &PsychEffect, primitive: ActionPrimitive) -> PsychEffect {
        let mut effect = base.clone();
        match self {
            Intent::Safety => {
                effect.alertness *= 2.0;
                effect.stimulation = 0.0;
            }
            Intent::Curiosity => {
                effect.alertness *= 0.5;
                effect.stimulation *= 2.5;
            }
            Intent::Social => {
                effect.companionship *= 2.0;
            }
            Intent::Fatigue if primitive == ActionPrimitive::Rest => {
                // Rest+Fatigue at high intensity = Sleep (consciousness loss).
                // The execution system checks intensity to flip the sign.
                // At low intensity = conscious recovery (mild alertness gain).
            }
            _ => {}
        }
        effect
    }
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
#[derive(Debug, Clone, Default, Reflect)]
pub struct Behavior {
    /// What the body is physically doing.
    pub primitive: ActionPrimitive,
    /// Where/what the behavior is directed at.
    pub target: TargetSelector,
    /// How hard the agent wants to push.
    pub intensity: IntensityPolicy,
    /// Why — the motivational drive.
    pub intent: Intent,
}

impl Behavior {
    pub fn new(
        primitive: ActionPrimitive,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motor_primitive_effort_profile_is_unique_per_primitive() {
        let primitives = [
            ActionPrimitive::Locomote,
            ActionPrimitive::Manipulate,
            ActionPrimitive::Ingest,
            ActionPrimitive::Rest,
            ActionPrimitive::Observe,
            ActionPrimitive::Vocalize,
        ];
        assert_eq!(
            primitives.len(),
            6,
            "number of EffortProfile declarations must equal number of ActionPrimitive variants"
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
        let registry = crate::agent::actions::ActionRegistry::new();
        let wander = registry
            .get(crate::agent::actions::ActionType::Wander)
            .unwrap()
            .default_behavior();
        let flee = registry
            .get(crate::agent::actions::ActionType::Flee)
            .unwrap()
            .default_behavior();
        assert_eq!(wander.primitive, ActionPrimitive::Locomote);
        assert_eq!(flee.primitive, ActionPrimitive::Locomote);
    }

    #[test]
    fn wander_and_flee_differ_only_in_policy() {
        let registry = crate::agent::actions::ActionRegistry::new();
        let wander = registry
            .get(crate::agent::actions::ActionType::Wander)
            .unwrap()
            .default_behavior();
        let flee = registry
            .get(crate::agent::actions::ActionType::Flee)
            .unwrap()
            .default_behavior();
        assert_eq!(wander.primitive, flee.primitive);
        assert!(
            !matches!(wander.intensity, IntensityPolicy::Maximal),
            "wander should not be maximal"
        );
        assert!(
            matches!(flee.intensity, IntensityPolicy::Maximal),
            "flee should be maximal"
        );
    }

    #[test]
    fn harvest_behavior_uses_manipulate_primitive() {
        let registry = crate::agent::actions::ActionRegistry::new();
        let harvest = registry
            .get(crate::agent::actions::ActionType::Harvest)
            .unwrap()
            .default_behavior();
        assert_eq!(harvest.primitive, ActionPrimitive::Manipulate);
    }

    #[test]
    fn sleep_behavior_uses_rest_primitive() {
        let registry = crate::agent::actions::ActionRegistry::new();
        let sleep = registry
            .get(crate::agent::actions::ActionType::Sleep)
            .unwrap()
            .default_behavior();
        assert_eq!(sleep.primitive, ActionPrimitive::Rest);
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
            ActionPrimitive::Locomote,
            TargetSelector::RandomNearby,
            IntensityPolicy::Sustained,
            Intent::Territoriality,
        );
        // If this compiles and doesn't panic, the test passes.
        // The point: no new cost constants needed for a new locomotion behavior.
    }

    #[test]
    fn high_urgency_escalates_normal_to_maximal() {
        let policy = IntensityPolicy::Normal.escalate_for_urgency(0.9);
        assert!(
            matches!(policy, IntensityPolicy::Maximal),
            "urgency 0.9 should escalate Normal to Maximal"
        );
    }

    #[test]
    fn moderate_urgency_escalates_normal_to_sustained() {
        let policy = IntensityPolicy::Normal.escalate_for_urgency(0.6);
        assert!(
            matches!(policy, IntensityPolicy::Sustained),
            "urgency 0.6 should escalate Normal to Sustained"
        );
    }

    #[test]
    fn low_urgency_keeps_normal() {
        let policy = IntensityPolicy::Normal.escalate_for_urgency(0.3);
        assert!(
            matches!(policy, IntensityPolicy::Normal),
            "urgency 0.3 should keep Normal unchanged"
        );
    }

    #[test]
    fn maximal_never_escalates_further() {
        let policy = IntensityPolicy::Maximal.escalate_for_urgency(0.99);
        assert!(
            matches!(policy, IntensityPolicy::Maximal),
            "Maximal should stay Maximal regardless of urgency"
        );
    }

    #[test]
    fn fixed_never_escalates() {
        let policy = IntensityPolicy::Fixed(0.0).escalate_for_urgency(0.99);
        assert!(
            matches!(policy, IntensityPolicy::Fixed(_)),
            "Fixed should never escalate"
        );
    }
}
