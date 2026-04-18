//! `Need`: the unified storage primitive for appetitive agent needs.
//!
//! Reads: nothing (pure data)
//! Writes: `Need::value` via `set`, `drain`, `top_up`
//! Upstream: `PhysicalNeeds` (hydration, wakefulness), `PsychologicalDrives`
//!           (companionship, enjoyment, etc.), and any future appetitive drive
//! Downstream: `nervous_system::urgency` (reads `.deficit()`), `Action::satiation`
//!             (reads `.is_satisfied(threshold)`), UI / SimEvent / field logger
//!
//! ## Why a shared primitive?
//!
//! Before this file, every need used a different scale (0..100 for hydration,
//! 0..1 for wakefulness, 0..aerobic_max for stamina), polarity (hydration
//! high=good, pain high=bad), and write idiom (raw `x = y` vs clamped
//! `(x + delta).min(100.0)`). Consumers had to remember each one. `Need`
//! normalises storage to `0..1, high = satisfied` and exposes `.deficit()`,
//! `.is_satisfied()`, `.top_up()`, `.drain()` so urgency inversion, satiation
//! gates, and top-up math are one-liners at the call site.
//!
//! ### What `Need` deliberately does NOT own
//!
//! - **Decay rate.** Real drains depend on BMR × phenotype × sleep state
//!   (glucose), circadian multiplier × light (wakefulness), proximity
//!   (companionship), intruder count (dominion), injury (pain). A single
//!   `decay_per_sec` field would force every system to collapse its real
//!   logic into one scalar. Systems keep owning their drain; they call
//!   `need.drain(amount)` with the amount they computed.
//! - **Satisfier action.** That lives on `NeedKind` (see below).
//! - **Initialisation baselines.** Personality, genetics, and species rules
//!   stay in their owning modules; each produces a starting `value` that is
//!   wrapped via `Need::new(v)`.

use bevy::prelude::*;

use crate::agent::actions::ActionType;

/// A normalised appetitive need. `value` is `0..1` where `1.0` means fully
/// satisfied and `0.0` means maximally unmet. Every appetitive drive on an
/// agent goes through this type so urgency inversion, satiation gates, and
/// top-up math look identical at the call site.
///
/// `Need` is pure storage — decay logic lives in the owning system (see the
/// file-level doc for why).
#[derive(Debug, Clone, Copy, PartialEq, Reflect)]
pub struct Need {
    pub value: f32,
}

impl Need {
    /// A fully-satisfied need — `value = 1.0`. Use this as the default for
    /// freshly-spawned agents where "starts at max" is the natural baseline
    /// (hydration, wakefulness, all drives with no personality modifier).
    pub fn full() -> Self {
        Self { value: 1.0 }
    }

    /// A fully-unmet need — `value = 0.0`. Rare as a starting state; mostly
    /// useful in tests that need to exercise the high-urgency branch.
    pub fn empty() -> Self {
        Self { value: 0.0 }
    }

    /// Construct a need from a raw `0..1` satisfaction value. Values outside
    /// the range are clamped in the constructor so callers don't need to.
    pub fn new(value: f32) -> Self {
        Self {
            value: value.clamp(0.0, 1.0),
        }
    }

    /// The unmet portion of the need, in `0..1`. Most urgency generators
    /// read this directly — e.g. `UrgencySource::Thirst` is
    /// `physical.hydration.deficit()`. Equivalent to `1.0 - value`.
    pub fn deficit(&self) -> f32 {
        1.0 - self.value
    }

    /// Returns `true` when the need is at least `threshold` satisfied.
    /// Used by the unified satiation gate — e.g. Eat refuses to start when
    /// `stomach_need().is_satisfied(0.8)` because the stomach is already
    /// 80% full. Thresholds are per-`NeedKind` and declared on that enum.
    pub fn is_satisfied(&self, threshold: f32) -> bool {
        self.value >= threshold
    }

    /// Replace the value, clamping into `0..1`. Use this when the owning
    /// system has already computed a fresh absolute value (territoriality
    /// recomputes dominion from intruder count; test fixtures set specific
    /// satisfaction levels).
    pub fn set(&mut self, value: f32) {
        self.value = value.clamp(0.0, 1.0);
    }

    /// Increase satisfaction by `amount` (clamped at `1.0`). Negative
    /// amounts are ignored — drains go through `drain` so the intent
    /// reads clearly at the call site.
    pub fn top_up(&mut self, amount: f32) {
        if amount <= 0.0 {
            return;
        }
        self.value = (self.value + amount).min(1.0);
    }

    /// Decrease satisfaction by `amount` (clamped at `0.0`). Negative
    /// amounts are ignored — top-ups go through `top_up` so the intent
    /// reads clearly at the call site.
    pub fn drain(&mut self, amount: f32) {
        if amount <= 0.0 {
            return;
        }
        self.value = (self.value - amount).max(0.0);
    }
}

impl Default for Need {
    /// A new `Need` is fully satisfied. Almost every call site wants this
    /// (fresh agents start rested, hydrated, socially content). Callers
    /// who need a specific baseline construct via `Need::new(v)`.
    fn default() -> Self {
        Self::full()
    }
}

// ─── NeedKind ────────────────────────────────────────────────────────────────

/// Every appetitive need the simulation tracks. The enum is the canonical
/// identifier used by urgency generation, satiation gates, SimEvent
/// telemetry, and the rational brain's goal-for-urgency dispatch.
///
/// Adding a new appetitive need is a three-step change:
/// 1. Add a variant here.
/// 2. Add a storage field (of type `Need`) to `PhysicalNeeds` or
///    `PsychologicalDrives`.
/// 3. Wire the variant through `satisfier` / `satiation_threshold` /
///    `goal_pattern` so the brain can plan for it.
///
/// Everything downstream — urgency inversion, satiation gates, telemetry —
/// picks the new variant up automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum NeedKind {
    // Physical pools
    Hunger,
    Thirst,
    Sleep,
    Stamina,

    // Psychological drives
    Social,
    Fun,
    Curiosity,
    Territory,

    // Affective state (stored as deficit in `pain`/`fear`; surfaced via
    // `PhysicalNeeds::pain_need()` / `emotions.fear_need()` as a Need-shaped
    // view so urgency generation reads them uniformly).
    Pain,
    Fear,

    // Maslow-style satisfaction drives. Currently read-only inputs to
    // emotion systems; no dedicated satisfier action yet.
    Safety,
    Esteem,
    Autonomy,
}

impl NeedKind {
    /// The action that directly satisfies this need, if one exists. Returns
    /// `None` for needs that have no single satisfier action — Fear and Pain
    /// are resolved indirectly (flee the threat, heal the injury), and the
    /// Maslow drives (Safety, Esteem, Autonomy) are event-driven with no
    /// obvious "top-up" verb.
    pub fn satisfier(&self) -> Option<ActionType> {
        match self {
            NeedKind::Hunger => Some(ActionType::Eat),
            NeedKind::Thirst => Some(ActionType::Drink),
            NeedKind::Sleep => Some(ActionType::Sleep),
            NeedKind::Stamina => Some(ActionType::Rest),
            NeedKind::Social => Some(ActionType::InitiateConversation),
            NeedKind::Fun | NeedKind::Curiosity => Some(ActionType::Explore),
            NeedKind::Territory
            | NeedKind::Pain
            | NeedKind::Fear
            | NeedKind::Safety
            | NeedKind::Esteem
            | NeedKind::Autonomy => None,
        }
    }

    /// The fullness threshold at which the satisfier action refuses to start
    /// — stomach ~80% full, hydration ~95% topped up, wakefulness ~95%
    /// rested, etc. Mirrors real satiety signals (stretch receptors + CCK
    /// for hunger, ADH and hypothalamic osmoreceptors for thirst) so meals
    /// and drinks emerge as bursts that end naturally.
    ///
    /// Returned as a `0..1` satisfaction threshold; `Need::is_satisfied`
    /// consumes it directly. Needs without a satisfier return `1.0` so a
    /// naïve caller always gets "not satisfied" (the action still declines
    /// to start via its normal precondition chain).
    pub fn satiation_threshold(&self) -> f32 {
        match self {
            NeedKind::Hunger => 0.8,
            NeedKind::Thirst => 0.95,
            NeedKind::Sleep => 0.95,
            NeedKind::Stamina => 0.95,
            NeedKind::Social | NeedKind::Fun | NeedKind::Curiosity => 0.9,
            NeedKind::Territory
            | NeedKind::Pain
            | NeedKind::Fear
            | NeedKind::Safety
            | NeedKind::Esteem
            | NeedKind::Autonomy => 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_clamps_into_0_to_1() {
        assert_eq!(Need::new(-0.5).value, 0.0);
        assert_eq!(Need::new(1.5).value, 1.0);
        assert_eq!(Need::new(0.3).value, 0.3);
    }

    #[test]
    fn deficit_is_one_minus_value() {
        let need = Need::new(0.3);
        assert!((need.deficit() - 0.7).abs() < 1e-6);
    }

    #[test]
    fn is_satisfied_honours_threshold() {
        let full = Need::full();
        assert!(full.is_satisfied(0.8));
        assert!(full.is_satisfied(1.0));
        let empty = Need::empty();
        assert!(!empty.is_satisfied(0.1));
    }

    #[test]
    fn top_up_clamps_at_1() {
        let mut need = Need::new(0.9);
        need.top_up(0.3);
        assert_eq!(need.value, 1.0);
    }

    #[test]
    fn top_up_ignores_non_positive() {
        let mut need = Need::new(0.5);
        need.top_up(-0.2);
        assert_eq!(need.value, 0.5);
        need.top_up(0.0);
        assert_eq!(need.value, 0.5);
    }

    #[test]
    fn drain_clamps_at_0() {
        let mut need = Need::new(0.1);
        need.drain(0.5);
        assert_eq!(need.value, 0.0);
    }

    #[test]
    fn drain_ignores_non_positive() {
        let mut need = Need::new(0.5);
        need.drain(-0.2);
        assert_eq!(need.value, 0.5);
    }

    #[test]
    fn set_clamps_into_range() {
        let mut need = Need::new(0.5);
        need.set(1.8);
        assert_eq!(need.value, 1.0);
        need.set(-0.2);
        assert_eq!(need.value, 0.0);
    }

    #[test]
    fn default_is_fully_satisfied() {
        let need = Need::default();
        assert_eq!(need.value, 1.0);
    }

    #[test]
    fn satisfier_matches_action() {
        assert_eq!(NeedKind::Hunger.satisfier(), Some(ActionType::Eat));
        assert_eq!(NeedKind::Thirst.satisfier(), Some(ActionType::Drink));
        assert_eq!(NeedKind::Sleep.satisfier(), Some(ActionType::Sleep));
        assert_eq!(NeedKind::Pain.satisfier(), None);
    }

    #[test]
    fn satiation_thresholds_are_sensible() {
        assert_eq!(NeedKind::Hunger.satiation_threshold(), 0.8);
        assert_eq!(NeedKind::Thirst.satiation_threshold(), 0.95);
        // Needs without a satisfier return 1.0 so a naive caller never
        // short-circuits the action's own preconditions.
        assert_eq!(NeedKind::Pain.satiation_threshold(), 1.0);
    }
}
