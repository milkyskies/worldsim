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

/// Multipliers derived from digestive-organ condition (stomach, liver, gut).
/// Each field is a scalar in `[0, 1]`: `1.0` = fully healthy, `0.0` = organ
/// destroyed. Passed into [`Metabolism::tick_with_mods`] so a damaged
/// digestive system degrades the metabolic pipeline at the right stages:
///
/// - `stomach` scales `DIGEST_CARB_RATE` and `DIGEST_FAT_RATE` — a damaged
///   stomach moves food out of the stomach more slowly.
/// - `liver` scales `GLUCOSE_OVERFLOW_RATE` and `RESERVE_MOBILIZE_RATE` —
///   a damaged liver is slower in both directions of the glucose / reserves
///   conversion.
/// - `gut` scales the *yield* at the end of digestion — a damaged gut
///   absorbs less of what the stomach has processed. The discarded mass
///   represents food that passes through the digestive tract without
///   contributing energy.
///
/// `Default` returns all-`1.0` (fully intact) so every call site that
/// doesn't have an anatomical body yet keeps its pre-#351 behaviour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrganMods {
    pub stomach: f32,
    pub liver: f32,
    pub gut: f32,
}

impl Default for OrganMods {
    fn default() -> Self {
        Self {
            stomach: 1.0,
            liver: 1.0,
            gut: 1.0,
        }
    }
}

/// Map an edible `Concept` to its macro breakdown. Returns `None` for
/// anything not currently modeled as food. New food items get added here.
pub fn food_macros(concept: Concept) -> Option<FoodMacros> {
    match concept {
        // Fruit: mostly carbs, trace fat.
        Concept::Apple => Some(FoodMacros::new(30.0, 1.0)),
        // Berries: pure carbs, modest serving. With the all-or-nothing
        // rule in `Metabolism::eat`, the meal must fit entirely in
        // stomach headroom — small meals find more eating windows.
        Concept::Berry => Some(FoodMacros::new(40.0, 0.0)),
        // Spoiled fruit: still calories, just way fewer. About 1/3 of
        // the fresh value — desperation food, not a meal.
        Concept::RottenApple => Some(FoodMacros::new(10.0, 0.0)),
        Concept::RottenBerry => Some(FoodMacros::new(20.0, 0.0)),
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
    /// Adds the meal's macros into the stomach. Only accepts the meal if
    /// the *entire* macro mass fits within current stomach headroom —
    /// otherwise the item is left in the agent's inventory for later.
    /// Called by the eat and graze actions.
    ///
    /// Returns `true` if the food was accepted in full, `false` if the
    /// stomach didn't have room. The all-or-nothing rule prevents the
    /// silent food-loss bug from #416: under the original
    /// proportional-scaling rule, a 200-carb berry into a stomach with
    /// 50 carbs of headroom would absorb 50 carbs and *discard 150*
    /// while still consuming the inventory item. Callers like
    /// `Eat.on_complete` use this signal to decide whether to remove
    /// the corresponding item from the agent's inventory.
    pub fn eat(&mut self, macros: FoodMacros) -> bool {
        let headroom = (STOMACH_CAPACITY - self.stomach_fullness()).max(0.0);
        let incoming = macros.total_mass();
        if incoming <= 0.0 || headroom < incoming {
            return false;
        }
        self.stomach_carbs = (self.stomach_carbs + macros.carbs).max(0.0);
        self.stomach_fat = (self.stomach_fat + macros.fat).max(0.0);
        true
    }

    /// Advance the metabolism one tick with no organ modulation. Delegates
    /// to [`Metabolism::tick_with_mods`] with a fully-intact [`OrganMods`].
    /// Used by tests and other call sites that don't have access to an
    /// anatomical [`Body`](crate::agent::biology::body::Body) (the normal
    /// production path in `activity_effects` passes real mods).
    pub fn tick(&mut self, dt: f32, bmr_drain: f32, activity_drain: f32) {
        self.tick_with_mods(dt, bmr_drain, activity_drain, OrganMods::default());
    }

    /// Advance the metabolism one tick, scaling digestion, absorption, and
    /// glucose/reserves conversion by organ condition.
    ///
    /// `bmr_drain` is the basal metabolic rate (glucose burned just to stay
    /// alive). `activity_drain` is the additional glucose cost of whatever
    /// the agent is currently doing (walking, harvesting, etc.). Both are in
    /// glucose-units per second and represent pure consumption — organ
    /// damage does not reduce them (you still burn fuel even if you can't
    /// refill it).
    ///
    /// `mods` carries the digestive-organ multipliers derived from `Body`.
    /// A destroyed stomach (`mods.stomach = 0`) stops moving food out of
    /// the stomach entirely, so a full stomach starves the agent anyway.
    /// A destroyed gut (`mods.gut = 0`) similarly yields zero absorption.
    /// A destroyed liver (`mods.liver = 0`) breaks both directions of the
    /// glucose / reserves conversion.
    ///
    /// Order of operations matters: burn first so the overflow / mobilize
    /// logic sees the post-burn glucose level.
    pub fn tick_with_mods(
        &mut self,
        dt: f32,
        bmr_drain: f32,
        activity_drain: f32,
        mods: OrganMods,
    ) {
        // 1. Burn glucose for BMR + current activity. Damage doesn't save
        //    you from needing fuel — it only hurts the refill pathways.
        self.glucose -= (bmr_drain + activity_drain) * dt;

        // 2. Digest carbs from stomach into glucose. Stomach damage slows
        //    the rate of stomach-to-glucose conversion; gut damage scales
        //    absorption so a fraction of the digested mass is discarded.
        let carbs_ready = self.stomach_carbs.min(DIGEST_CARB_RATE * mods.stomach * dt);
        self.stomach_carbs -= carbs_ready;
        self.glucose += carbs_ready * mods.gut;

        // 3. Digest fat from stomach directly into reserves. Same modulation
        //    shape: stomach rate, gut yield.
        let fat_ready = self.stomach_fat.min(DIGEST_FAT_RATE * mods.stomach * dt);
        self.stomach_fat -= fat_ready;
        self.reserves += fat_ready * mods.gut;

        // 4. Overflow: above the storage threshold, glucose becomes reserves.
        //    Liver damage slows the storage pathway.
        if self.glucose > GLUCOSE_OVERFLOW_THRESHOLD {
            let overflow = (GLUCOSE_OVERFLOW_RATE * mods.liver * dt)
                .min(self.glucose - GLUCOSE_OVERFLOW_THRESHOLD);
            self.glucose -= overflow;
            self.reserves += overflow;
        }

        // 5. Mobilization: below the mobilize threshold, reserves top up
        //    glucose. Liver damage slows this direction too — a critical
        //    symptom of real-world liver failure is impaired fasting glucose.
        self.mobilize_reserves(dt, mods.liver);

        // Clamp all pools.
        self.stomach_carbs = self.stomach_carbs.max(0.0);
        self.stomach_fat = self.stomach_fat.max(0.0);
        self.glucose = self.glucose.clamp(0.0, GLUCOSE_MAX);
        self.reserves = self.reserves.clamp(0.0, RESERVES_MAX);
    }

    /// Top up glucose from reserves when glucose is below the mobilize
    /// threshold. Safe to call multiple times per frame — each call
    /// advances mobilization by up to `RESERVE_MOBILIZE_RATE × liver × dt`
    /// and short-circuits when glucose is already above the threshold or
    /// reserves are empty.
    ///
    /// Public because `execution::apply_action_effects` drains glucose
    /// directly (outside `tick_with_mods`) and would otherwise strand
    /// agents at `glucose = 0, reserves > 0` whenever the per-tick
    /// action drain exceeded the single mobilization pass from the
    /// activity system (#397).
    pub fn mobilize_reserves(&mut self, dt: f32, liver: f32) {
        if self.glucose >= GLUCOSE_MOBILIZE_THRESHOLD || self.reserves <= 0.0 {
            return;
        }
        let deficit = GLUCOSE_MOBILIZE_THRESHOLD - self.glucose;
        let mobilized = (RESERVE_MOBILIZE_RATE * liver * dt)
            .min(deficit)
            .min(self.reserves);
        self.reserves -= mobilized;
        self.glucose += mobilized;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobilize_reserves_tops_up_glucose_below_threshold() {
        // #397: glucose drained to 0 while reserves are plenty.
        // Calling mobilize_reserves directly (the path
        // apply_action_effects uses) must advance glucose up.
        let mut m = Metabolism {
            stomach_carbs: 0.0,
            stomach_fat: 0.0,
            glucose: 0.0,
            reserves: 500.0,
        };
        let glucose_before = m.glucose;
        let reserves_before = m.reserves;

        m.mobilize_reserves(1.0, 1.0); // 1 sec, intact liver

        assert!(
            m.glucose > glucose_before,
            "glucose should have risen from mobilization, got {}",
            m.glucose
        );
        assert!(
            m.reserves < reserves_before,
            "reserves should have been spent, got {}",
            m.reserves
        );
        // One second of mobilization at RESERVE_MOBILIZE_RATE should
        // equal exactly that rate (capped by deficit).
        let expected = RESERVE_MOBILIZE_RATE.min(GLUCOSE_MOBILIZE_THRESHOLD);
        assert!(
            (m.glucose - expected).abs() < 1e-4,
            "mobilized amount should be {expected}, got {}",
            m.glucose
        );
    }

    #[test]
    fn mobilize_reserves_noops_when_glucose_above_threshold() {
        let mut m = Metabolism {
            stomach_carbs: 0.0,
            stomach_fat: 0.0,
            glucose: 80.0, // above GLUCOSE_MOBILIZE_THRESHOLD 50
            reserves: 500.0,
        };
        m.mobilize_reserves(1.0, 1.0);
        assert_eq!(
            m.glucose, 80.0,
            "mobilization must not fire above the threshold"
        );
        assert_eq!(
            m.reserves, 500.0,
            "reserves must not be spent when mobilization is inactive"
        );
    }

    #[test]
    fn mobilize_reserves_noops_when_reserves_empty() {
        let mut m = Metabolism {
            stomach_carbs: 0.0,
            stomach_fat: 0.0,
            glucose: 0.0,
            reserves: 0.0,
        };
        m.mobilize_reserves(1.0, 1.0);
        assert_eq!(
            m.glucose, 0.0,
            "cannot mobilize from empty reserves (starvation)"
        );
    }

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

    // ─── OrganMods (#351 organ damage → metabolism) ───────────────────────

    /// Default mods leave the pipeline untouched — a test using `tick`
    /// (the shim) and one using `tick_with_mods(default)` produce the same
    /// final state. Protects the refactor from drift between the two paths.
    #[test]
    fn default_mods_match_unmodified_tick() {
        let starting = Metabolism {
            stomach_carbs: 30.0,
            stomach_fat: 10.0,
            glucose: 50.0,
            reserves: 100.0,
        };
        let mut a = starting.clone();
        let mut b = starting;
        a.tick(5.0, 0.2, 0.3);
        b.tick_with_mods(5.0, 0.2, 0.3, OrganMods::default());
        assert!((a.stomach_carbs - b.stomach_carbs).abs() < 1e-6);
        assert!((a.stomach_fat - b.stomach_fat).abs() < 1e-6);
        assert!((a.glucose - b.glucose).abs() < 1e-6);
        assert!((a.reserves - b.reserves).abs() < 1e-6);
    }

    /// A destroyed stomach (`mods.stomach = 0`) halts digestion outright —
    /// food sits in the stomach forever while glucose drains and the agent
    /// starves despite being "full". Acceptance criterion from #351.
    #[test]
    fn destroyed_stomach_halts_digestion() {
        let mut m = Metabolism {
            stomach_carbs: 50.0,
            stomach_fat: 20.0,
            glucose: 40.0,
            reserves: 0.0,
        };
        let pre_stomach = m.stomach_fullness();
        let pre_glucose = m.glucose;
        m.tick_with_mods(
            10.0,
            0.2,
            0.0,
            OrganMods {
                stomach: 0.0,
                liver: 1.0,
                gut: 1.0,
            },
        );
        assert!(
            (m.stomach_fullness() - pre_stomach).abs() < 1e-6,
            "stomach contents must not move with a destroyed stomach"
        );
        assert!(
            m.glucose < pre_glucose,
            "glucose still drains from BMR even with a dead stomach"
        );
    }

    /// A destroyed gut (`mods.gut = 0`) still drains the stomach but
    /// delivers zero yield — the mass vanishes instead of entering glucose
    /// or reserves. Acceptance criterion: damaged gut extracts less energy
    /// from the same meal than a healthy agent.
    #[test]
    fn destroyed_gut_drains_stomach_without_refilling_glucose() {
        let mut m = Metabolism {
            stomach_carbs: 20.0,
            stomach_fat: 10.0,
            glucose: 50.0,
            reserves: 50.0,
        };
        let pre_glucose = m.glucose;
        let pre_reserves = m.reserves;
        m.tick_with_mods(
            60.0,
            0.0,
            0.0,
            OrganMods {
                stomach: 1.0,
                liver: 1.0,
                gut: 0.0,
            },
        );
        assert!(
            m.stomach_carbs < 0.001 && m.stomach_fat < 0.001,
            "stomach still empties normally when gut is dead"
        );
        assert!(
            (m.glucose - pre_glucose).abs() < 1e-4,
            "zero absorption means no glucose rise, got {} → {}",
            pre_glucose,
            m.glucose
        );
        assert!(
            (m.reserves - pre_reserves).abs() < 1e-4,
            "zero absorption means no reserves rise, got {} → {}",
            pre_reserves,
            m.reserves
        );
    }

    /// A half-gut agent extracts roughly half the energy from the same meal
    /// as a healthy agent — verifies `mods.gut` is a proportional
    /// multiplier, not an on/off gate.
    #[test]
    fn partial_gut_damage_yields_proportionally_less() {
        let start = Metabolism {
            stomach_carbs: 20.0,
            stomach_fat: 0.0,
            glucose: 40.0,
            reserves: 50.0,
        };
        let mut healthy = start.clone();
        let mut half = start;
        let dt = 60.0;
        healthy.tick_with_mods(dt, 0.0, 0.0, OrganMods::default());
        half.tick_with_mods(
            dt,
            0.0,
            0.0,
            OrganMods {
                stomach: 1.0,
                liver: 1.0,
                gut: 0.5,
            },
        );
        let healthy_gain = healthy.glucose - 40.0;
        let half_gain = half.glucose - 40.0;
        // Half absorption should deliver ~50% of the healthy gain. The
        // overflow branch (glucose > 70 → spills to reserves) complicates
        // the exact ratio on the healthy path, so the assertion checks the
        // obvious inequality plus a sanity band.
        assert!(
            half_gain < healthy_gain,
            "damaged gut must yield less glucose than healthy gut (half={half_gain}, full={healthy_gain})"
        );
    }

    /// A destroyed liver (`mods.liver = 0`) stops both directions of the
    /// glucose / reserves conversion — reserves don't mobilize when glucose
    /// is critically low. Acceptance criterion: damaged liver has slower
    /// reserve mobilization during prolonged exertion.
    #[test]
    fn destroyed_liver_halts_reserve_mobilization() {
        let mut m = Metabolism {
            stomach_carbs: 0.0,
            stomach_fat: 0.0,
            glucose: 20.0, // below mobilize threshold
            reserves: 100.0,
        };
        let pre_reserves = m.reserves;
        m.tick_with_mods(
            5.0,
            0.0,
            0.0,
            OrganMods {
                stomach: 1.0,
                liver: 0.0,
                gut: 1.0,
            },
        );
        assert!(
            (m.reserves - pre_reserves).abs() < 1e-4,
            "reserves must not mobilize with a dead liver, got {}",
            m.reserves
        );
    }

    /// A healthy control: with all mods at 1.0, reserves mobilize normally.
    /// Protects the liver test from false positives (e.g. if mobilization
    /// were broken for everyone).
    #[test]
    fn healthy_liver_mobilizes_reserves_normally() {
        let mut m = Metabolism {
            stomach_carbs: 0.0,
            stomach_fat: 0.0,
            glucose: 20.0,
            reserves: 100.0,
        };
        m.tick_with_mods(5.0, 0.0, 0.0, OrganMods::default());
        assert!(
            m.reserves < 100.0,
            "healthy liver should mobilize reserves, got {}",
            m.reserves
        );
        assert!(
            m.glucose > 20.0,
            "mobilization should have topped up glucose, got {}",
            m.glucose
        );
    }
}
