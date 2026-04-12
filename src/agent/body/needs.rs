//! Agent needs: PhysicalNeeds, Consciousness, and PsychologicalDrives components — the source of truth for agent state.
//!
//! Reads: nothing (pure data components, written by other systems)
//! Writes: PhysicalNeeds, Consciousness, PsychologicalDrives (ECS components)
//! Upstream: nervous_system::activity_effects (modifies values each tick)
//! Downstream: nervous_system::urgency (drives urgency scores), brains::arbitration (survival power), brain_system

use bevy::prelude::*;

use crate::agent::actions::ActionType;
use crate::agent::body::metabolism::Metabolism;

/// Physical fatigue with two biologically-inspired sub-pools.
///
/// **Anaerobic** is the sprint reserve — small, fast drain, fast refill at low
/// intensity. Models oxygen debt and glycolysis.
///
/// **Aerobic** is the sustained reserve — drains slowly during any activity and
/// only meaningfully recovers during real rest (sit, rest, sleep). Models
/// glycogen, hydration, muscle freshness.
///
/// Drain and recovery both use the `drain` / `recover` methods which follow
/// the formulas defined in issue #331. The concrete intensity is computed by
/// the locomotion system (filed separately).
#[derive(Reflect, Debug, Clone)]
pub struct Stamina {
    pub anaerobic: f32,
    pub anaerobic_max: f32,
    pub aerobic: f32,
    pub aerobic_max: f32,
}

impl Default for Stamina {
    fn default() -> Self {
        Self {
            anaerobic: 100.0,
            anaerobic_max: 100.0,
            aerobic: 100.0,
            aerobic_max: 100.0,
        }
    }
}

impl Stamina {
    /// Construct a stamina reserve with a given aerobic capacity and the
    /// standard 100-unit anaerobic pool. Both pools start full.
    pub fn with_aerobic_max(aerobic_max: f32) -> Self {
        Self {
            anaerobic: 100.0,
            anaerobic_max: 100.0,
            aerobic: aerobic_max,
            aerobic_max,
        }
    }

    /// Aerobic fill fraction in [0, 1]. This is the primary "how tired am I"
    /// value — used by movement speed, subjective cost, and survival sleep.
    pub fn aerobic_fraction(&self) -> f32 {
        if self.aerobic_max > 0.0 {
            (self.aerobic / self.aerobic_max).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Anaerobic fill fraction in [0, 1]. Sprint reserve.
    pub fn anaerobic_fraction(&self) -> f32 {
        if self.anaerobic_max > 0.0 {
            (self.anaerobic / self.anaerobic_max).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Intensity-based quadratic drain. See issue #331 for the formula.
    ///
    /// - `intensity > 0.7` → sprint. Burns anaerobic until empty, then aerobic at penalty.
    /// - `0.3 < intensity <= 0.7` → sustained. Burns aerobic gently plus a touch of anaerobic.
    /// - `intensity <= 0.3` → walking or idle. Tiny aerobic cost only.
    ///
    /// Drain scales quadratically with intensity so sprinting at 1.0 costs
    /// roughly 6x more than cruising at 0.4. `dt` is seconds; the per-second
    /// values are the issue's constants scaled by 60 ticks/second.
    ///
    /// When a sprint call lasts long enough to deplete anaerobic mid-step,
    /// the remaining time spills into the aerobic penalty branch — so the
    /// result is independent of how the caller subdivides dt.
    pub fn drain(&mut self, intensity: f32, dt: f32) {
        let i2 = intensity * intensity;
        let ticks = dt * 60.0;
        if intensity > 0.7 {
            // Sprint: anaerobic first, then aerobic penalty.
            let anaerobic_rate = 0.2 * i2 * 60.0;
            let aerobic_penalty_rate = 0.4 * i2 * 60.0;
            let available = (self.anaerobic - 5.0).max(0.0);
            let wanted = anaerobic_rate * dt;
            if wanted <= available {
                self.anaerobic -= wanted;
            } else {
                // Exhaust anaerobic down to the 5.0 reserve, then convert
                // the remaining time to aerobic penalty drain.
                self.anaerobic -= available;
                let spent_time = if anaerobic_rate > 0.0 {
                    available / anaerobic_rate
                } else {
                    0.0
                };
                let remaining_time = (dt - spent_time).max(0.0);
                self.aerobic -= aerobic_penalty_rate * remaining_time;
            }
        } else if intensity > 0.3 {
            self.aerobic -= 0.05 * i2 * ticks;
            self.anaerobic -= 0.02 * i2 * ticks;
        } else {
            self.aerobic -= 0.005 * ticks;
        }
        self.clamp();
    }

    /// Apply recovery for a rest-style action. Anaerobic always refills fast
    /// at low intensity; aerobic recovers at a rate that depends on how real
    /// the rest is (Sleep > Sit/Rest > Walk/Wander > other).
    ///
    /// Intensity is the ongoing activity intensity — recovery only applies
    /// when it's below the `0.3` low-intensity threshold.
    pub fn recover(&mut self, action: ActionType, intensity: f32, dt: f32) {
        if intensity >= 0.3 {
            return;
        }
        let ticks = dt * 60.0;
        self.anaerobic = (self.anaerobic + 0.5 * ticks).min(self.anaerobic_max);

        let aerobic_rate = match action {
            ActionType::Sleep => 0.3,
            ActionType::Idle => 0.05,
            ActionType::Walk | ActionType::Wander => 0.01,
            _ => 0.0,
        };
        self.aerobic = (self.aerobic + aerobic_rate * ticks).min(self.aerobic_max);
    }

    /// Restore both pools to maximum. Sleep and eat bursts do this.
    pub fn restore_full(&mut self) {
        self.anaerobic = self.anaerobic_max;
        self.aerobic = self.aerobic_max;
    }

    /// Adjust aerobic by a delta, clamped. Used by activity_effects for
    /// legacy rate-based drain/recover until the locomotion intensity system
    /// takes over.
    pub fn adjust_aerobic(&mut self, delta: f32) {
        self.aerobic = (self.aerobic + delta).clamp(0.0, self.aerobic_max);
    }

    fn clamp(&mut self) {
        self.aerobic = self.aerobic.clamp(0.0, self.aerobic_max);
        self.anaerobic = self.anaerobic.clamp(0.0, self.anaerobic_max);
    }
}

/// Physical needs - THE source of truth for survival needs
/// All agents have this
#[derive(Component, Reflect, Debug, Clone)]
#[reflect(Component)]
pub struct PhysicalNeeds {
    /// Nutrient / energy loop: stomach (carbs+fat) -> glucose -> reserves.
    /// Replaces the flat `hunger` scalar; see `metabolism.rs` for the model.
    pub metabolism: Metabolism,
    /// 0..100 hydration — high is good. Drains toward 0 over time,
    /// refills to 100 when the agent drinks. Replaces the old
    /// `thirst` field which stored the inverse (high = parched).
    pub hydration: f32,
    pub stamina: Stamina,
    /// Homeostatic sleep pressure (adenosine analogue). 1.0 = fully rested,
    /// 0.0 = must sleep. Decays while awake, accelerates at night (circadian),
    /// and restores during Sleep. Independent of stamina — a desk worker
    /// gets sleepy without running a marathon.
    pub wakefulness: f32,
}

impl Default for PhysicalNeeds {
    fn default() -> Self {
        Self {
            metabolism: Metabolism::default(),
            hydration: 100.0,
            stamina: Stamina::default(),
            wakefulness: 1.0,
        }
    }
}

impl PhysicalNeeds {
    /// 0..1 hunger urgency derived from the three metabolism pools. Every
    /// consumer of "how hungry is this agent" reads through this accessor so
    /// the underlying pool weights stay in one place (`Metabolism::hunger_urgency`).
    pub fn hunger_urgency(&self) -> f32 {
        self.metabolism.hunger_urgency()
    }
}

/// Consciousness state - alertness and awareness
/// All agents have this
#[derive(Component, Reflect, Debug, Clone)]
#[reflect(Component)]
pub struct Consciousness {
    pub alertness: f32, // 0-1, reduced during sleep
}

impl Default for Consciousness {
    fn default() -> Self {
        Self { alertness: 1.0 }
    }
}

/// Post-spawn override for baseline companionship satisfaction.
///
/// Normally `develop_phenotype_system` derives drives from genome-derived
/// personality. Tests that need a specific companionship value (e.g.
/// forcing two strangers to feel lonely) insert this component; the
/// system reads it and replaces `drives.companionship` with the value.
/// Name kept as `SocialDriveOverride` for backwards-compat during the
/// rename pass; consider renaming in a follow-up.
#[derive(Component, Reflect, Debug, Clone, Copy)]
#[reflect(Component)]
pub struct SocialDriveOverride(pub f32);

/// Psychological drives, stored as **satisfaction** in `0..=1`.
/// High = satisfied, low = unmet need. Matches the "+ = good" polarity
/// used by `PhysicalNeeds` (hydration, stamina, health). Urgency
/// generation inverts once at the CNS edge — see
/// `nervous_system::urgency::generate_urgency`.
///
/// All agents with a nervous system carry this — wolves and deer
/// included (previous comment said "humans only" but was stale).
#[derive(Component, Reflect, Debug, Clone)]
#[reflect(Component)]
pub struct PsychologicalDrives {
    /// Social satisfaction. 1.0 = recently connected, 0.0 = desperately lonely.
    pub companionship: f32,
    /// Playful enjoyment. 1.0 = content, 0.0 = bored.
    pub enjoyment: f32,
    /// Mental stimulation. 1.0 = engaged, 0.0 = starved for novelty.
    pub stimulation: f32,
    /// Social standing. 1.0 = respected, 0.0 = disrespected / low-status.
    pub esteem: f32,
    /// Felt safety. 1.0 = secure, 0.0 = threatened.
    pub safety: f32,
    /// Sense of freedom. 1.0 = self-directed, 0.0 = constrained.
    pub autonomy: f32,
    /// Territorial control. 1.0 = uncontested (no intruders),
    /// 0.0 = actively defending against an intruder. Updated each tick
    /// by the territoriality system based on perceived intruders.
    /// Species baseline from `SpeciesProfile::territoriality_baseline`
    /// determines the floor when threats are present.
    pub dominion: f32,
}

impl Default for PsychologicalDrives {
    fn default() -> Self {
        // Mid-satisfaction by default; territorial control full (no
        // threats perceived yet).
        Self {
            companionship: 0.5,
            enjoyment: 0.5,
            stimulation: 0.5,
            esteem: 0.5,
            safety: 0.5,
            autonomy: 0.5,
            dominion: 1.0,
        }
    }
}

impl PsychologicalDrives {
    /// Initialise drive baselines from Big Five personality traits.
    ///
    /// Personality shapes the baseline *deficit* an agent wakes up with —
    /// an extravert starts with low companionship (wants company sooner),
    /// an open agent starts with low stimulation (novelty-seeking), etc.
    /// Stored as satisfaction, so the trait mappings invert the pre-rename
    /// logic: `companionship = 1 - extraversion` (extraverts start unsatisfied).
    pub fn from_personality(traits: &crate::agent::psyche::personality::PersonalityTraits) -> Self {
        Self {
            // Extraverts start unsatisfied (low companionship) so they
            // reach toward socializing sooner.
            companionship: 1.0 - traits.extraversion,
            // Open personalities start understimulated, driving exploration.
            stimulation: 1.0 - traits.openness,
            // Neurotic agents feel less safe at baseline.
            safety: 1.0 - traits.neuroticism,
            // Conscientious agents start with lower esteem (more to prove).
            esteem: 1.0 - traits.conscientiousness,
            // Disagreeable agents start with low autonomy satisfaction
            // (feel constrained more easily).
            autonomy: traits.agreeableness,
            enjoyment: 0.5,
            // Starts uncontested; territoriality system lowers this when
            // intruders appear.
            dominion: 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::psyche::personality::PersonalityTraits;

    #[test]
    fn high_extraversion_lowers_baseline_companionship() {
        let traits = PersonalityTraits {
            extraversion: 0.9,
            ..Default::default()
        };
        let drives = PsychologicalDrives::from_personality(&traits);
        assert!(
            drives.companionship < 0.2,
            "extraverts wake up socially unsatisfied (got {})",
            drives.companionship
        );
    }

    #[test]
    fn low_extraversion_raises_baseline_companionship() {
        let traits = PersonalityTraits {
            extraversion: 0.1,
            ..Default::default()
        };
        let drives = PsychologicalDrives::from_personality(&traits);
        assert!(
            drives.companionship > 0.8,
            "introverts wake up content (got {})",
            drives.companionship
        );
    }

    #[test]
    fn high_openness_lowers_baseline_stimulation() {
        let traits = PersonalityTraits {
            openness: 0.9,
            ..Default::default()
        };
        let drives = PsychologicalDrives::from_personality(&traits);
        assert!(
            drives.stimulation < 0.2,
            "open agents wake up understimulated (got {})",
            drives.stimulation
        );
    }

    #[test]
    fn high_neuroticism_lowers_baseline_safety() {
        let traits = PersonalityTraits {
            neuroticism: 0.9,
            ..Default::default()
        };
        let drives = PsychologicalDrives::from_personality(&traits);
        assert!(
            drives.safety < 0.2,
            "neurotic agent feels less safe at baseline (got {})",
            drives.safety
        );
    }

    #[test]
    fn high_agreeableness_raises_baseline_autonomy() {
        let traits = PersonalityTraits {
            agreeableness: 0.9,
            ..Default::default()
        };
        let drives = PsychologicalDrives::from_personality(&traits);
        assert!(
            drives.autonomy > 0.8,
            "agreeable agent feels high autonomy satisfaction (got {})",
            drives.autonomy
        );
    }

    // ─────────────────────────────────────────────────────────────────────
    // Stamina two-pool behaviour (#331)
    // ─────────────────────────────────────────────────────────────────────

    /// Sprint (intensity > 0.7) drains anaerobic while aerobic stays nearly
    /// full. The sprint reserve is designed to absorb high-intensity bursts.
    #[test]
    fn sprint_drains_anaerobic_not_aerobic() {
        let mut s = Stamina::default();
        let aerobic_before = s.aerobic;
        // One second of full-intensity sprint.
        s.drain(1.0, 1.0);
        assert!(
            s.anaerobic < 100.0,
            "sprint should drain anaerobic, got {}",
            s.anaerobic
        );
        assert!(
            (s.aerobic - aerobic_before).abs() < 0.1,
            "sprint should not noticeably drain aerobic, delta={}",
            aerobic_before - s.aerobic
        );
    }

    /// Sustained activity (intensity in 0.3..0.7) burns aerobic primarily
    /// with a small anaerobic touch. This is the "cruise" mode.
    #[test]
    fn sustained_activity_drains_aerobic_gently() {
        let mut s = Stamina::default();
        s.drain(0.5, 1.0);
        assert!(s.aerobic < 100.0, "sustained drain should hit aerobic");
        assert!(
            s.aerobic > 95.0,
            "sustained drain should be gentle, got aerobic={}",
            s.aerobic
        );
    }

    /// Walking (intensity <= 0.3) is essentially free — minimal aerobic cost,
    /// anaerobic untouched.
    #[test]
    fn walking_is_essentially_free() {
        let mut s = Stamina::default();
        s.drain(0.2, 1.0);
        assert!(
            s.aerobic > 99.5,
            "walking 1s should barely move aerobic, got {}",
            s.aerobic
        );
        assert_eq!(s.anaerobic, 100.0, "walking must not touch anaerobic");
    }

    /// The drain formula is quadratic in intensity, so sprinting at 1.0 burns
    /// roughly 6x more per second than cruising at 0.4.
    #[test]
    fn drain_scales_quadratically_with_intensity() {
        // Measure aerobic drain for sprint-with-empty-anaerobic (to compare
        // on a single pool).
        let mut sprint = Stamina {
            anaerobic: 0.0,
            ..Default::default()
        };
        let mut cruise = Stamina {
            anaerobic: 0.0,
            ..Default::default()
        };
        sprint.drain(1.0, 1.0);
        cruise.drain(0.4, 1.0);
        let sprint_loss = 100.0 - sprint.aerobic;
        let cruise_loss = 100.0 - cruise.aerobic;
        // Sprint branch: 0.4 * i^2 * 60 = 24 per second at i=1.0
        // Cruise branch: 0.05 * i^2 * 60 = 0.48 per second at i=0.4
        // Ratio ~50x. Both drain branches differ in the leading constant,
        // but the i^2 term dominates: even on the same branch the ratio
        // of i=1.0 vs i=0.4 is (1.0/0.4)^2 = 6.25x. The assert below checks
        // the weaker condition that sprint drain is much greater than cruise.
        assert!(
            sprint_loss > cruise_loss * 5.0,
            "sprint (loss {sprint_loss}) should drain much more than cruise (loss {cruise_loss})"
        );
    }

    /// Anaerobic refills fast at low intensity — the "huff for 30 seconds and
    /// resume sprinting" behaviour.
    #[test]
    fn anaerobic_refills_fast_at_low_intensity() {
        let mut s = Stamina {
            anaerobic: 0.0,
            aerobic: 100.0,
            ..Default::default()
        };
        // Half a second of idle rest.
        s.recover(ActionType::Idle, 0.0, 0.5);
        assert!(
            s.anaerobic > 10.0,
            "anaerobic should refill quickly at rest, got {}",
            s.anaerobic
        );
    }

    /// High-intensity activity does NOT refill anaerobic — recovery is gated
    /// on intensity < 0.3.
    #[test]
    fn anaerobic_does_not_refill_during_exertion() {
        let mut s = Stamina {
            anaerobic: 50.0,
            ..Default::default()
        };
        s.recover(ActionType::Walk, 0.8, 1.0);
        assert_eq!(
            s.anaerobic, 50.0,
            "anaerobic must not refill while exerting hard"
        );
    }

    /// Aerobic recovers slow and only with real rest. Sleep > Idle > Walk.
    #[test]
    fn aerobic_recovery_rate_depends_on_action() {
        let drain = 30.0;
        let mut sleeping = Stamina {
            aerobic: 100.0 - drain,
            ..Default::default()
        };
        let mut idling = Stamina {
            aerobic: 100.0 - drain,
            ..Default::default()
        };
        let mut walking = Stamina {
            aerobic: 100.0 - drain,
            ..Default::default()
        };
        sleeping.recover(ActionType::Sleep, 0.0, 1.0);
        idling.recover(ActionType::Idle, 0.0, 1.0);
        walking.recover(ActionType::Walk, 0.2, 1.0);
        let sleep_gain = sleeping.aerobic - (100.0 - drain);
        let idle_gain = idling.aerobic - (100.0 - drain);
        let walk_gain = walking.aerobic - (100.0 - drain);
        assert!(
            sleep_gain > idle_gain,
            "sleep should recover aerobic faster than idle"
        );
        assert!(
            idle_gain > walk_gain,
            "idle should recover aerobic faster than walking"
        );
    }

    /// Sleep restores BOTH pools to full via `restore_full`. This is the
    /// "wake refreshed" behaviour.
    #[test]
    fn restore_full_refills_both_pools() {
        let mut s = Stamina {
            anaerobic: 0.0,
            aerobic: 0.0,
            ..Default::default()
        };
        s.restore_full();
        assert_eq!(s.anaerobic, s.anaerobic_max);
        assert_eq!(s.aerobic, s.aerobic_max);
    }

    /// aerobic_fraction and anaerobic_fraction clamp to [0, 1].
    #[test]
    fn fractions_clamp_and_honor_max() {
        let s = Stamina {
            aerobic: 50.0,
            aerobic_max: 200.0,
            anaerobic: 10.0,
            anaerobic_max: 100.0,
        };
        assert!((s.aerobic_fraction() - 0.25).abs() < 1e-6);
        assert!((s.anaerobic_fraction() - 0.1).abs() < 1e-6);
    }

    /// Drain does not push values below zero.
    #[test]
    fn drain_clamps_at_zero() {
        let mut s = Stamina {
            anaerobic: 1.0,
            aerobic: 1.0,
            ..Default::default()
        };
        s.drain(1.0, 100.0);
        assert!(s.anaerobic >= 0.0, "anaerobic clamped, got {}", s.anaerobic);
        assert!(s.aerobic >= 0.0, "aerobic clamped, got {}", s.aerobic);
    }

    /// Repeated long sprint-rest cycles show diminishing aerobic (acceptance
    /// criterion). Each sprint fully depletes anaerobic and then falls
    /// through to the aerobic penalty branch, which is expensive. Rest
    /// refills anaerobic completely but only slowly restores aerobic, so
    /// the ratchet compounds across cycles.
    #[test]
    fn repeated_sprint_rest_cycles_erode_aerobic_diminishingly() {
        let mut s = Stamina::default();
        // Four sprint-rest cycles. Sprint is 10s at full intensity — long
        // enough to empty anaerobic (~8s) and spend 2s on aerobic.
        for _ in 0..4 {
            s.drain(1.0, 10.0);
            // Rest: 5s idle — plenty of time to fully refill anaerobic
            // (100 at 30/s = ~3.3s) and recover a modest amount of aerobic
            // (5 * 3 = 15 aerobic per cycle).
            s.recover(ActionType::Idle, 0.0, 5.0);
        }
        assert!(
            s.aerobic < 100.0,
            "aerobic should erode across sprint-rest cycles, got {}",
            s.aerobic
        );
        assert!(
            s.aerobic < 80.0,
            "aerobic should meaningfully erode after 4 long sprints, got {}",
            s.aerobic
        );
        assert!(
            s.anaerobic > 90.0,
            "anaerobic should bounce back during rests, got {}",
            s.anaerobic
        );
    }
}

// ============================================================================
// UI HELPERS
// ============================================================================

/// Helper trait for UI display
pub trait StateDisplay {
    fn display_name() -> &'static str;
    fn get_values(&self) -> Vec<(&'static str, f32, Scale)>;
}

#[derive(Clone, Copy, Debug)]
pub enum Scale {
    Percentage, // 0-100
    Normalized, // 0-1
}

impl StateDisplay for PhysicalNeeds {
    fn display_name() -> &'static str {
        "Physical Needs"
    }
    fn get_values(&self) -> Vec<(&'static str, f32, Scale)> {
        vec![
            (
                "Stomach",
                self.metabolism.stomach_fullness(),
                Scale::Percentage,
            ),
            ("Glucose", self.metabolism.glucose, Scale::Percentage),
            ("Reserves", self.metabolism.reserves, Scale::Percentage),
            ("Hydration", self.hydration, Scale::Percentage),
            ("Aerobic", self.stamina.aerobic, Scale::Percentage),
            ("Anaerobic", self.stamina.anaerobic, Scale::Percentage),
            ("Wakefulness", self.wakefulness, Scale::Normalized),
        ]
    }
}

impl StateDisplay for Consciousness {
    fn display_name() -> &'static str {
        "Consciousness"
    }
    fn get_values(&self) -> Vec<(&'static str, f32, Scale)> {
        vec![("Alertness", self.alertness, Scale::Normalized)]
    }
}

impl StateDisplay for PsychologicalDrives {
    fn display_name() -> &'static str {
        "Psych Drives"
    }
    fn get_values(&self) -> Vec<(&'static str, f32, Scale)> {
        vec![
            ("Companionship", self.companionship, Scale::Normalized),
            ("Enjoyment", self.enjoyment, Scale::Normalized),
            ("Stimulation", self.stimulation, Scale::Normalized),
            ("Esteem", self.esteem, Scale::Normalized),
            ("Safety", self.safety, Scale::Normalized),
            ("Autonomy", self.autonomy, Scale::Normalized),
            ("Dominion", self.dominion, Scale::Normalized),
        ]
    }
}
