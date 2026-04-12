//! Derived effort model: channel-based EffortProfile and cost computation.
//!
//! Actions declare *what the body is physically doing* via an `EffortProfile`.
//! The cost model (`compute_action_cost`) derives all metabolic drain rates
//! from the profile plus the agent's body mass. No per-action tuning constants.
//!
//! Reads: nothing (pure data + pure functions)
//! Writes: nothing (ActionCost is consumed by execution::apply_action_effects)
//! Upstream: Action trait implementations (declare profiles)
//! Downstream: execution::apply_action_effects (applies ActionCost to pools)

// ---------------------------------------------------------------------------
// Tuning constants — the ONE place physical cost lives
// ---------------------------------------------------------------------------

/// Energy (glucose/reserves) cost per second at full locomotion intensity.
/// Scales with intensity^1.5 and body mass. Calibrated so Flee (i=1.0) at
/// 70kg costs ~1.5 energy/s, matching the pre-refactor constant.
const LOCOMOTION_ENERGY_RATE: f32 = 1.5;

/// Energy cost per second at full manipulation intensity.
/// Linear scaling. Calibrated so Harvest (m=0.6) costs ~1.0 energy/s.
const MANIPULATION_ENERGY_RATE: f32 = 1.7;

/// Energy cost per second at full isometric hold.
const ISOMETRIC_ENERGY_RATE: f32 = 1.0;

/// Energy cost per second at full cognitive load. Active cognition
/// (scanning, tracking, vigilance) burns glucose faster than resting
/// brain baseline. Calibrated so Explore (loco=0.5 + cog=0.3) produces
/// ~1.1 energy/s, close to the pre-refactor 1.25.
const COGNITION_ENERGY_RATE: f32 = 2.0;

/// Energy cost of recovery (anabolism). Recovery isn't free — the body
/// burns energy to repair and restore.
const RECOVERY_ENERGY_COST: f32 = 0.2;

/// Aerobic stamina recovered per second at full recovery (recovery=1.0).
/// Calibrated so Sleep (recovery=1.0) produces +20/s, matching pre-refactor.
const RECOVERY_AEROBIC_RATE: f32 = 20.0;

/// Anaerobic stamina recovered per second at full recovery.
const RECOVERY_ANAEROBIC_RATE: f32 = 5.0;

/// Aerobic stamina drain per second from manipulation at full intensity.
/// Quadratic scaling: drain = rate * m^2. Calibrated so Attack (m=1.0)
/// plus locomotion produces ~2.0/s total, and Harvest (m=0.6) produces
/// ~0.2/s, matching pre-refactor constants.
const MANIPULATION_AEROBIC_RATE: f32 = 0.55;

/// Aerobic stamina drain per second from isometric hold at full intensity.
/// Quadratic scaling: drain = rate * s^2.
const ISOMETRIC_AEROBIC_RATE: f32 = 0.4;

/// Reference body mass in kg. Locomotion energy scales linearly with mass
/// relative to this reference.
pub const DEFAULT_BODY_MASS: f32 = 70.0;

// ---------------------------------------------------------------------------
// EffortProfile
// ---------------------------------------------------------------------------

/// What the body is physically doing during an action, expressed as channel
/// engagement levels in [0, 1].
///
/// Actions declare this. The cost model owns all the math.
///
/// Future channel: thermoregulation (shiver/sweat cost).
#[derive(Debug, Clone, PartialEq)]
pub struct EffortProfile {
    /// Moving the body through space. 0 = stationary, 1 = full sprint.
    pub locomotion: f32,
    /// Applying force to external objects (swing, lift, dig, strike).
    pub manipulation: f32,
    /// Holding force without motion (carry, brace, grip).
    pub isometric: f32,
    /// Brain glucose burn (planning, tracking, vigilance).
    pub cognition: f32,
    /// Anabolic / restorative activity. Inverts sign in the cost model:
    /// recovery *produces* stamina instead of consuming it.
    pub recovery: f32,
}

impl Default for EffortProfile {
    fn default() -> Self {
        Self {
            locomotion: 0.0,
            manipulation: 0.0,
            isometric: 0.0,
            cognition: 0.0,
            recovery: 0.0,
        }
    }
}

impl EffortProfile {
    /// Empty profile — BMR only, zero action cost.
    pub const ZERO: Self = Self {
        locomotion: 0.0,
        manipulation: 0.0,
        isometric: 0.0,
        cognition: 0.0,
        recovery: 0.0,
    };

    /// The dominant channel intensity, used for fuel partitioning.
    /// Higher intensity shifts the fuel mix toward glucose; lower
    /// intensity shifts toward reserves (fat).
    pub fn peak_intensity(&self) -> f32 {
        self.locomotion
            .max(self.manipulation)
            .max(self.isometric)
            .max(self.cognition)
    }
}

// ---------------------------------------------------------------------------
// ActionCost
// ---------------------------------------------------------------------------

/// Per-second metabolic cost vector, derived from EffortProfile + body state.
///
/// Fuel-pool partitioning is NOT baked in here — the execution system splits
/// `energy` between glucose and reserves based on `peak_intensity` at
/// application time. Stamina drain is split into aerobic and anaerobic.
#[derive(Debug, Clone, Default)]
pub struct ActionCost {
    /// Total metabolic energy expenditure per second. Split between glucose
    /// and reserves at application time based on intensity.
    pub energy: f32,
    /// Aerobic stamina pool change per second. Positive = drain, negative = recovery.
    pub aerobic_drain: f32,
    /// Anaerobic stamina pool change per second. Positive = drain, negative = recovery.
    pub anaerobic_drain: f32,
    /// Alertness drain per second from cognitive effort. Always >= 0.
    /// Distinct from the behavioral alertness_per_sec on RuntimeEffects.
    pub cognitive_drain: f32,
}

// ---------------------------------------------------------------------------
// Cost computation
// ---------------------------------------------------------------------------

/// Derive the per-second metabolic cost of running an action with the given
/// effort profile.
///
/// `body_mass` is the agent's mass in kg. Locomotion cost scales linearly
/// with mass relative to [`DEFAULT_BODY_MASS`].
///
/// This function does NOT import or know about action types. It takes pure
/// physics: a profile and a body mass. That keeps #420's migration (moving
/// profiles from actions to motor primitives) a simple relocation.
pub fn compute_action_cost(profile: &EffortProfile, body_mass: f32) -> ActionCost {
    let mut cost = ActionCost::default();
    let mass_factor = body_mass / DEFAULT_BODY_MASS;

    // --- Locomotion ---
    // Energy: superlinear with intensity (running costs more than 2x walking).
    // Stamina: mirrors the pre-refactor intensity-gated drain from Stamina::drain().
    if profile.locomotion > 0.0 {
        let i = profile.locomotion;
        let i2 = i * i;

        cost.energy += LOCOMOTION_ENERGY_RATE * i.powf(1.5) * mass_factor;

        if i > 0.7 {
            // Sprint: heavy anaerobic + aerobic penalty.
            cost.anaerobic_drain += 0.2 * i2 * 60.0;
            cost.aerobic_drain += 0.4 * i2 * 60.0;
        } else if i > 0.3 {
            // Sustained: mostly aerobic with a touch of anaerobic.
            cost.aerobic_drain += 0.05 * i2 * 60.0;
            cost.anaerobic_drain += 0.02 * i2 * 60.0;
        } else {
            // Walking/idle: minimal aerobic cost.
            cost.aerobic_drain += 0.005 * 60.0;
        }
    }

    // --- Manipulation ---
    // Quadratic stamina drain so light work (Harvest at 0.6) is cheap but
    // intense exertion (Attack at 1.0) is punishing.
    if profile.manipulation > 0.0 {
        let m = profile.manipulation;
        cost.energy += MANIPULATION_ENERGY_RATE * m * mass_factor;
        cost.aerobic_drain += MANIPULATION_AEROBIC_RATE * m * m;
    }

    // --- Isometric ---
    if profile.isometric > 0.0 {
        let s = profile.isometric;
        cost.energy += ISOMETRIC_ENERGY_RATE * s * mass_factor;
        cost.aerobic_drain += ISOMETRIC_AEROBIC_RATE * s * s;
    }

    // --- Cognition ---
    if profile.cognition > 0.0 {
        let c = profile.cognition;
        cost.energy += COGNITION_ENERGY_RATE * c;
        cost.cognitive_drain += c * 0.5;
    }

    // --- Recovery ---
    // Recovery is the negative-cost channel: it restores stamina pools and
    // costs a small amount of energy (anabolism isn't free).
    if profile.recovery > 0.0 {
        let r = profile.recovery;
        cost.energy += RECOVERY_ENERGY_COST * r;
        cost.aerobic_drain -= RECOVERY_AEROBIC_RATE * r;
        cost.anaerobic_drain -= RECOVERY_ANAEROBIC_RATE * r;
    }

    cost
}

/// Fraction of energy cost that should be drawn from glucose, based on the
/// action's peak intensity. Higher intensity shifts fuel toward glucose;
/// lower intensity burns a small fraction from reserves (fat).
///
/// The split is conservative for now: glucose provides the vast majority
/// of fuel at all intensities. This preserves the pre-refactor calorie
/// economy where actions drained glucose directly. A future balance pass
/// can steepen the curve once the effort architecture is stable and the
/// metabolism model has been tuned for realistic fat oxidation.
///
/// At low intensity (< 0.3): ~85% glucose, ~15% reserves.
/// At moderate intensity (0.3-0.7): ramps from 90% to 100%.
/// At high intensity (> 0.7): 100% glucose.
pub fn glucose_fraction(peak_intensity: f32) -> f32 {
    if peak_intensity > 0.7 {
        1.0
    } else if peak_intensity > 0.3 {
        0.9 + (peak_intensity - 0.3) * 0.25
    } else {
        0.85 + peak_intensity * 0.17
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const HUMAN_MASS: f32 = 70.0;

    #[test]
    fn effort_profile_zero_produces_bmr_only_cost() {
        let cost = compute_action_cost(&EffortProfile::ZERO, HUMAN_MASS);
        assert_eq!(
            cost.energy, 0.0,
            "empty profile should have zero action cost"
        );
        assert_eq!(cost.aerobic_drain, 0.0);
        assert_eq!(cost.anaerobic_drain, 0.0);
        assert_eq!(cost.cognitive_drain, 0.0);
    }

    #[test]
    fn locomotion_cost_scales_with_mass() {
        let profile = EffortProfile {
            locomotion: 0.5,
            ..Default::default()
        };
        let light = compute_action_cost(&profile, 35.0);
        let heavy = compute_action_cost(&profile, 70.0);
        let ratio = heavy.energy / light.energy;
        assert!(
            (ratio - 2.0).abs() < 0.1,
            "doubling mass should roughly double locomotion energy, ratio = {ratio}"
        );
    }

    #[test]
    fn locomotion_cost_scales_superlinearly_with_intensity() {
        let walk = EffortProfile {
            locomotion: 0.5,
            ..Default::default()
        };
        let stroll = EffortProfile {
            locomotion: 0.25,
            ..Default::default()
        };
        let walk_cost = compute_action_cost(&walk, HUMAN_MASS);
        let stroll_cost = compute_action_cost(&stroll, HUMAN_MASS);
        assert!(
            walk_cost.energy > 2.0 * stroll_cost.energy,
            "walk at 0.5 should cost more than 2x stroll at 0.25: walk={}, stroll={}",
            walk_cost.energy,
            stroll_cost.energy,
        );
    }

    #[test]
    fn manipulation_channel_adds_cost_independent_of_locomotion() {
        let manip_only = EffortProfile {
            manipulation: 0.6,
            ..Default::default()
        };
        let cost = compute_action_cost(&manip_only, HUMAN_MASS);
        assert!(
            cost.energy > 0.0,
            "manipulation-only profile should have energy cost"
        );
        assert!(
            cost.aerobic_drain > 0.0,
            "manipulation-only profile should drain aerobic stamina"
        );
    }

    #[test]
    fn recovery_channel_produces_negative_drain() {
        let recovery = EffortProfile {
            recovery: 1.0,
            ..Default::default()
        };
        let cost = compute_action_cost(&recovery, HUMAN_MASS);
        assert!(
            cost.aerobic_drain < 0.0,
            "recovery should produce negative aerobic drain (= restoration), got {}",
            cost.aerobic_drain,
        );
        assert!(
            cost.anaerobic_drain < 0.0,
            "recovery should produce negative anaerobic drain (= restoration), got {}",
            cost.anaerobic_drain,
        );
    }

    #[test]
    fn low_intensity_burns_reserves_not_glucose() {
        let fraction = glucose_fraction(0.15);
        assert!(
            fraction < 1.0,
            "low intensity should divert some energy to reserves, glucose fraction = {fraction}"
        );
        assert!(
            fraction < glucose_fraction(0.9),
            "low intensity should have lower glucose fraction than high intensity"
        );
    }

    #[test]
    fn high_intensity_burns_glucose() {
        let fraction = glucose_fraction(0.9);
        assert!(
            (fraction - 1.0).abs() < 0.01,
            "high intensity should burn 100% glucose, fraction = {fraction}"
        );
    }

    #[test]
    fn wander_and_explore_have_identical_locomotion_cost_at_equal_intensity() {
        let wander = EffortProfile {
            locomotion: 0.5,
            ..Default::default()
        };
        let explore = EffortProfile {
            locomotion: 0.5,
            cognition: 0.3,
            ..Default::default()
        };
        let wander_cost = compute_action_cost(&wander, HUMAN_MASS);
        let explore_cost = compute_action_cost(&explore, HUMAN_MASS);
        // Same locomotion component
        let wander_loco_energy = LOCOMOTION_ENERGY_RATE * 0.5_f32.powf(1.5);
        let explore_loco_energy = LOCOMOTION_ENERGY_RATE * 0.5_f32.powf(1.5);
        assert!(
            (wander_loco_energy - explore_loco_energy).abs() < 0.001,
            "locomotion energy should be identical at equal intensity"
        );
        // Total differs because explore has cognition
        assert!(
            explore_cost.energy > wander_cost.energy,
            "explore should cost more due to cognition channel"
        );
    }

    #[test]
    fn harvest_costs_more_than_wander_at_same_locomotion_due_to_manipulation() {
        let wander = EffortProfile {
            locomotion: 0.25,
            ..Default::default()
        };
        let harvest = EffortProfile {
            manipulation: 0.6,
            ..Default::default()
        };
        let wander_cost = compute_action_cost(&wander, HUMAN_MASS);
        let harvest_cost = compute_action_cost(&harvest, HUMAN_MASS);
        assert!(
            harvest_cost.energy > wander_cost.energy,
            "harvest should cost more due to manipulation channel: harvest={}, wander={}",
            harvest_cost.energy,
            wander_cost.energy,
        );
    }

    #[test]
    fn sleep_profile_restores_stamina_over_time() {
        let sleep = EffortProfile {
            recovery: 1.0,
            ..Default::default()
        };
        let cost = compute_action_cost(&sleep, HUMAN_MASS);
        assert!(
            cost.aerobic_drain < -10.0,
            "sleep should restore aerobic significantly, got {}",
            cost.aerobic_drain,
        );
    }

    #[test]
    fn peak_intensity_returns_max_channel() {
        let profile = EffortProfile {
            locomotion: 0.3,
            manipulation: 0.7,
            isometric: 0.1,
            cognition: 0.5,
            recovery: 0.0,
        };
        assert!((profile.peak_intensity() - 0.7).abs() < 0.001);
    }
}
