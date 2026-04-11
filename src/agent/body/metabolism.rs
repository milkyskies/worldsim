//! Nutrient / energy loop: the multi-pool replacement for the flat `hunger` scalar.
//!
//! Reads: nothing (pure data + pure tick function)
//! Writes: Metabolism (pools mutate during `tick`, `eat`, and starvation damage)
//! Upstream: nervous_system::activity_effects (drain rates per activity),
//!           actions::eat / actions::graze (food ingestion)
//! Downstream: urgency::hunger_urgency (derives a 0..1 urgency from pools),
//!             biology::process_starvation (gradient starvation damage),
//!             character_sheet UI (three bars)
//!
//! ## Model
//!
//! Food flows through three stages. `stomach_*` holds ingested food waiting to
//! be digested; `glucose` is the short-term cellular energy bus that activities
//! burn; `reserves` is the long-term fat/glycogen store that buffers the agent
//! between meals.
//!
//! ```text
//!   eat() ──▶ stomach_carbs ──▶ glucose ──▶ [burned by activities]
//!          ╲▶ stomach_fat  ─────────────▶ reserves ◀──▶ glucose (mobilize / overflow)
//! ```
//!
//! Starvation is a *gradient*, not a cliff. As glucose drops while reserves are
//! exhausted, the agent progresses through weakness, critical deficit, and
//! finally HP damage. Well-fed agents with large reserves survive meaningfully
//! longer than lean ones — this is the central gameplay feel of the refactor.

use crate::agent::mind::knowledge::Concept;
use bevy::prelude::*;

/// Maximum total mass (carbs + fat) the stomach can hold. A fresh meal adds
/// to stomach until this cap; extra is discarded (simulates "too full to eat").
pub const STOMACH_CAPACITY: f32 = 100.0;

/// Maximum blood glucose. Activities burn from here directly.
pub const GLUCOSE_MAX: f32 = 100.0;

/// Above this glucose level, excess spills into reserves (storage).
pub const GLUCOSE_OVERFLOW_THRESHOLD: f32 = 70.0;

/// Below this glucose level, reserves mobilize to top up glucose.
pub const GLUCOSE_MOBILIZE_THRESHOLD: f32 = 50.0;

/// Below this glucose level and with reserves drained, the agent is
/// progressively weakened (stamina ceiling drops). Not yet taking HP damage.
pub const GLUCOSE_WEAK_THRESHOLD: f32 = 30.0;

/// Below this glucose level and with reserves drained, the agent takes
/// progressive HP damage from starvation.
pub const GLUCOSE_CRITICAL_THRESHOLD: f32 = 15.0;

/// Long-term energy storage. A fully stocked reserve is multiple days of
/// normal activity without eating.
pub const RESERVES_MAX: f32 = 500.0;

/// Rate at which carbs in the stomach convert to glucose (per second).
pub const DIGEST_CARB_RATE: f32 = 1.0;

/// Rate at which fat in the stomach converts to reserves (per second).
/// Slower than carb digestion — fat is a long-burn fuel.
pub const DIGEST_FAT_RATE: f32 = 0.3;

/// Rate at which glucose above `GLUCOSE_OVERFLOW_THRESHOLD` converts to
/// reserves (per second). "Storing" excess energy as fat.
pub const GLUCOSE_OVERFLOW_RATE: f32 = 0.4;

/// Rate at which reserves mobilize back to glucose when glucose is below
/// `GLUCOSE_MOBILIZE_THRESHOLD` (per second). "Burning fat" during fasting.
pub const RESERVE_MOBILIZE_RATE: f32 = 0.8;

/// HP damage per second when glucose is critical and reserves are empty.
/// Calibrated so a fresh agent without food survives ~several in-game days
/// before dying, not minutes.
pub const STARVATION_DAMAGE_PER_SEC: f32 = 0.3;

/// Macros carried by a food item. Lookup via `food_macros` for each edible
/// `Concept`. Non-edible concepts return `None`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoodMacros {
    pub carbs: f32,
    pub fat: f32,
}

impl FoodMacros {
    pub const fn new(carbs: f32, fat: f32) -> Self {
        Self { carbs, fat }
    }

    pub fn total_mass(&self) -> f32 {
        self.carbs + self.fat
    }
}

/// Map an edible `Concept` to its macro breakdown. Returns `None` for
/// anything not currently modeled as food. New food items get added here.
pub fn food_macros(concept: Concept) -> Option<FoodMacros> {
    match concept {
        // Fruit: mostly carbs, trace fat.
        Concept::Apple => Some(FoodMacros::new(30.0, 1.0)),
        // Berries: pure carbs, small portion.
        Concept::Berry => Some(FoodMacros::new(20.0, 0.0)),
        // Meat: no carbs, heavy fat — long-burn fuel that mostly fills reserves.
        Concept::Meat => Some(FoodMacros::new(0.0, 40.0)),
        _ => None,
    }
}

/// The three-stage nutrient / energy loop. Lives inside `PhysicalNeeds`.
///
/// Starts with `Metabolism::well_fed()` by default — matches spawn behavior
/// of a freshly initialized agent. Tests that want a specific starting state
/// should construct with explicit field values.
#[derive(Reflect, Debug, Clone)]
pub struct Metabolism {
    /// Carbohydrate mass currently in the stomach, waiting to digest into glucose.
    pub stomach_carbs: f32,
    /// Fat mass currently in the stomach, waiting to digest into reserves.
    pub stomach_fat: f32,
    /// Blood glucose — the short-term energy bus. Activities burn from here.
    pub glucose: f32,
    /// Long-term energy storage. Mobilizes to glucose when glucose is low.
    pub reserves: f32,
}

impl Default for Metabolism {
    fn default() -> Self {
        Self::well_fed()
    }
}

impl Metabolism {
    /// Freshly-spawned agent with every pool saturated. `hunger_urgency()`
    /// returns 0.0 exactly, which matches the legacy `hunger: 0.0` default
    /// state and keeps `desperation`-style multipliers (perception threat
    /// assessment, stress recovery) reading as "fully calm" at baseline.
    ///
    /// On the first few ticks the surplus glucose above
    /// `GLUCOSE_OVERFLOW_THRESHOLD` spills into reserves — physiologically
    /// this is "just finished a meal, body is storing the surplus as fat".
    pub fn well_fed() -> Self {
        Self {
            stomach_carbs: STOMACH_CAPACITY * 0.6,
            stomach_fat: STOMACH_CAPACITY * 0.4,
            glucose: GLUCOSE_MAX,
            reserves: RESERVES_MAX,
        }
    }

    /// Completely empty across all pools. The agent is already at the
    /// starvation threshold. Used by tests that want to exercise the
    /// gradient starvation path directly.
    pub fn empty() -> Self {
        Self {
            stomach_carbs: 0.0,
            stomach_fat: 0.0,
            glucose: 0.0,
            reserves: 0.0,
        }
    }

    /// Construct a metabolism whose `hunger_urgency()` matches the given
    /// 0..1 target. Sets every pool to `(1 - urgency)` of its capacity so the
    /// weighted urgency sum collapses cleanly to `urgency`. Used by tests
    /// and scenarios that need a precise hunger level without caring about
    /// the specific pool distribution.
    pub fn at_urgency(urgency: f32) -> Self {
        let urgency = urgency.clamp(0.0, 1.0);
        let satiety_fraction = 1.0 - urgency;
        Self {
            stomach_carbs: STOMACH_CAPACITY * satiety_fraction * 0.6,
            stomach_fat: STOMACH_CAPACITY * satiety_fraction * 0.4,
            glucose: GLUCOSE_MAX * satiety_fraction,
            reserves: RESERVES_MAX * satiety_fraction,
        }
    }

    /// Total stomach contents (carbs + fat). Saturates at `STOMACH_CAPACITY`
    /// when eating; never exceeds it.
    pub fn stomach_fullness(&self) -> f32 {
        self.stomach_carbs + self.stomach_fat
    }

    pub fn stomach_fraction(&self) -> f32 {
        (self.stomach_fullness() / STOMACH_CAPACITY).clamp(0.0, 1.0)
    }

    pub fn glucose_fraction(&self) -> f32 {
        (self.glucose / GLUCOSE_MAX).clamp(0.0, 1.0)
    }

    pub fn reserves_fraction(&self) -> f32 {
        (self.reserves / RESERVES_MAX).clamp(0.0, 1.0)
    }

    /// 0..1 urgency signal for "how much do I need to eat". Blends all three
    /// pools so the agent feels hungry when *any* of them runs low:
    /// - stomach satiety (20%): short-term "just ate" signal (stretch receptors)
    /// - glucose satiety (50%): ongoing cellular energy availability
    /// - reserves satiety (30%): long-term security, the buffer that keeps
    ///   an agent confident between meals
    ///
    /// Returns 0.0 when well-fed, 1.0 when every pool is empty. Existing
    /// urgency curves (Sigmoid with midpoint ~0.6) apply on top of this.
    pub fn hunger_urgency(&self) -> f32 {
        let stomach_satiety = self.stomach_fraction() * 0.2;
        let glucose_satiety = self.glucose_fraction() * 0.5;
        let reserves_satiety = self.reserves_fraction() * 0.3;
        (1.0 - stomach_satiety - glucose_satiety - reserves_satiety).clamp(0.0, 1.0)
    }

    /// True when the agent is in the progressive-weakness zone: glucose low
    /// AND reserves effectively gone. Downstream systems use this to cap
    /// stamina or apply a movement penalty.
    pub fn is_weak_from_hunger(&self) -> bool {
        self.glucose < GLUCOSE_WEAK_THRESHOLD && self.reserves < 20.0
    }

    /// True when the agent is in the damage zone: glucose critical AND
    /// reserves empty. `process_starvation` will apply HP damage per tick
    /// while this is true.
    pub fn is_starving(&self) -> bool {
        self.glucose < GLUCOSE_CRITICAL_THRESHOLD && self.reserves <= 0.0
    }

    /// Ingest food. Fills stomach up to `STOMACH_CAPACITY`; any excess mass
    /// beyond capacity is discarded proportionally between carbs and fat.
    /// Called by the eat and graze actions.
    pub fn eat(&mut self, macros: FoodMacros) {
        let headroom = (STOMACH_CAPACITY - self.stomach_fullness()).max(0.0);
        let incoming = macros.total_mass();
        if incoming <= 0.0 {
            return;
        }
        let scale = if incoming > headroom {
            headroom / incoming
        } else {
            1.0
        };
        self.stomach_carbs = (self.stomach_carbs + macros.carbs * scale).max(0.0);
        self.stomach_fat = (self.stomach_fat + macros.fat * scale).max(0.0);
    }

    /// Advance the metabolism one tick.
    ///
    /// `bmr_drain` is the basal metabolic rate (glucose burned just to stay
    /// alive). `activity_drain` is the additional glucose cost of whatever
    /// the agent is currently doing (walking, harvesting, etc.). Both are in
    /// glucose-units per second.
    ///
    /// Order of operations matters: burn first so the overflow/mobilize
    /// logic sees the post-burn glucose level.
    pub fn tick(&mut self, dt: f32, bmr_drain: f32, activity_drain: f32) {
        // 1. Burn glucose for BMR + current activity.
        self.glucose -= (bmr_drain + activity_drain) * dt;

        // 2. Digest carbs from stomach into glucose.
        let carbs_ready = self.stomach_carbs.min(DIGEST_CARB_RATE * dt);
        self.stomach_carbs -= carbs_ready;
        self.glucose += carbs_ready;

        // 3. Digest fat from stomach directly into reserves.
        let fat_ready = self.stomach_fat.min(DIGEST_FAT_RATE * dt);
        self.stomach_fat -= fat_ready;
        self.reserves += fat_ready;

        // 4. Overflow: above the storage threshold, glucose becomes reserves.
        if self.glucose > GLUCOSE_OVERFLOW_THRESHOLD {
            let overflow =
                (GLUCOSE_OVERFLOW_RATE * dt).min(self.glucose - GLUCOSE_OVERFLOW_THRESHOLD);
            self.glucose -= overflow;
            self.reserves += overflow;
        }

        // 5. Mobilization: below the mobilize threshold, reserves top up glucose.
        if self.glucose < GLUCOSE_MOBILIZE_THRESHOLD && self.reserves > 0.0 {
            let deficit = GLUCOSE_MOBILIZE_THRESHOLD - self.glucose;
            let mobilized = (RESERVE_MOBILIZE_RATE * dt).min(deficit).min(self.reserves);
            self.reserves -= mobilized;
            self.glucose += mobilized;
        }

        // Clamp all pools.
        self.stomach_carbs = self.stomach_carbs.max(0.0);
        self.stomach_fat = self.stomach_fat.max(0.0);
        self.glucose = self.glucose.clamp(0.0, GLUCOSE_MAX);
        self.reserves = self.reserves.clamp(0.0, RESERVES_MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A well-fed agent reports low hunger urgency.
    #[test]
    fn well_fed_has_low_urgency() {
        let m = Metabolism::well_fed();
        assert!(
            m.hunger_urgency() < 0.3,
            "well-fed should be mostly satiated, got {}",
            m.hunger_urgency()
        );
    }

    /// An empty agent reports maximum hunger urgency.
    #[test]
    fn empty_has_max_urgency() {
        let m = Metabolism::empty();
        assert!(
            m.hunger_urgency() > 0.95,
            "empty metabolism should report near-max urgency, got {}",
            m.hunger_urgency()
        );
    }

    /// BMR burns glucose slowly over time.
    #[test]
    fn tick_burns_glucose_at_rest() {
        let mut m = Metabolism {
            stomach_carbs: 0.0,
            stomach_fat: 0.0,
            glucose: 60.0,
            reserves: 0.0,
        };
        m.tick(1.0, 0.2, 0.0);
        assert!(
            m.glucose < 60.0,
            "BMR should drain glucose, got {}",
            m.glucose
        );
    }

    /// Eating carbs fills the stomach, which then digests into glucose.
    #[test]
    fn eat_carbs_then_digest_raises_glucose() {
        let mut m = Metabolism {
            stomach_carbs: 0.0,
            stomach_fat: 0.0,
            glucose: 40.0,
            reserves: 0.0,
        };
        m.eat(FoodMacros::new(30.0, 0.0));
        assert_eq!(m.stomach_carbs, 30.0, "carbs go into stomach first");
        assert_eq!(m.glucose, 40.0, "glucose unchanged until digestion tick");

        // Tick 60 seconds: at DIGEST_CARB_RATE = 1.0 carb/s, the 30-carb
        // meal needs 30 seconds to fully digest. 60s leaves headroom.
        m.tick(60.0, 0.0, 0.0);
        assert!(m.stomach_carbs < 0.001, "carbs fully digested");
        assert!(
            m.glucose > 60.0,
            "glucose should rise from digestion, got {}",
            m.glucose
        );
    }

    /// Eating fat fills the stomach and slowly routes into reserves.
    #[test]
    fn eat_fat_digests_into_reserves() {
        let mut m = Metabolism {
            stomach_carbs: 0.0,
            stomach_fat: 0.0,
            glucose: 80.0,
            reserves: 100.0,
        };
        m.eat(FoodMacros::new(0.0, 20.0));
        let pre_reserves = m.reserves;
        m.tick(10.0, 0.0, 0.0);
        assert!(
            m.reserves > pre_reserves,
            "reserves should grow from fat digestion, got {}",
            m.reserves
        );
    }

    /// Excess glucose overflows into reserves for later use.
    #[test]
    fn high_glucose_overflows_to_reserves() {
        let mut m = Metabolism {
            stomach_carbs: 0.0,
            stomach_fat: 0.0,
            glucose: 100.0,
            reserves: 100.0,
        };
        m.tick(10.0, 0.0, 0.0);
        assert!(
            m.reserves > 100.0,
            "high glucose should overflow into reserves, got {}",
            m.reserves
        );
        assert!(
            m.glucose < 100.0,
            "overflowing glucose should drop, got {}",
            m.glucose
        );
    }

    /// Reserves mobilize when glucose drops below the threshold.
    #[test]
    fn low_glucose_mobilizes_reserves() {
        let mut m = Metabolism {
            stomach_carbs: 0.0,
            stomach_fat: 0.0,
            glucose: 20.0,
            reserves: 100.0,
        };
        m.tick(5.0, 0.0, 0.0);
        assert!(
            m.reserves < 100.0,
            "reserves should mobilize, got {}",
            m.reserves
        );
        assert!(
            m.glucose > 20.0,
            "glucose should rise from mobilization, got {}",
            m.glucose
        );
    }

    /// With both pools drained, the agent is flagged starving.
    #[test]
    fn empty_pools_report_starving() {
        let m = Metabolism::empty();
        assert!(m.is_starving(), "empty metabolism should be starving");
        assert!(m.is_weak_from_hunger(), "empty metabolism should be weak");
    }

    /// A well-fed agent is neither weak nor starving.
    #[test]
    fn well_fed_not_weak_or_starving() {
        let m = Metabolism::well_fed();
        assert!(!m.is_starving(), "well-fed should not be starving");
        assert!(!m.is_weak_from_hunger(), "well-fed should not be weak");
    }

    /// Stomach capacity caps incoming food.
    #[test]
    fn stomach_capacity_caps_large_meal() {
        let mut m = Metabolism::empty();
        m.eat(FoodMacros::new(200.0, 100.0));
        assert!(
            m.stomach_fullness() <= STOMACH_CAPACITY + 0.001,
            "stomach must not exceed capacity, got {}",
            m.stomach_fullness()
        );
    }

    /// Running out of food triggers a gradient: well-fed -> weak -> starving.
    /// This is the central gameplay feel of the refactor.
    #[test]
    fn prolonged_fast_progresses_through_gradient() {
        let mut m = Metabolism::well_fed();
        // Simulate an extended fast under heavy exertion. The rate-limiting
        // step is reserve mobilization (RESERVE_MOBILIZE_RATE = 0.8/s), not
        // raw glucose drain, because once glucose is exhausted the agent
        // lives off reserves at the mobilization cap regardless of BMR +
        // activity. Starting reserves ≈400, needing to reach < 20 (weak)
        // and then 0 (starving), takes ~475+ simulated seconds. 1200
        // seconds leaves comfortable headroom.
        let mut saw_weak = false;
        let mut saw_starving = false;
        for _ in 0..12_000 {
            m.tick(0.1, 1.0, 4.0);
            if m.is_weak_from_hunger() {
                saw_weak = true;
            }
            if m.is_starving() {
                saw_starving = true;
            }
        }
        assert!(saw_weak, "agent should have passed through weak state");
        assert!(saw_starving, "agent should have ended up starving");
    }

    /// Apple lookup table returns a carb-heavy meal.
    #[test]
    fn apple_is_carb_heavy() {
        let apple = food_macros(Concept::Apple).expect("apple is edible");
        assert!(apple.carbs > apple.fat, "apples are mostly carbs");
    }

    /// Meat lookup table returns a fat-heavy meal.
    #[test]
    fn meat_is_fat_heavy() {
        let meat = food_macros(Concept::Meat).expect("meat is edible");
        assert!(meat.fat > meat.carbs, "meat is mostly fat");
    }
}
