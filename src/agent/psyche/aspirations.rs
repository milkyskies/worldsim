//! Aspirations: long-term goals that persist across many sim ticks.
//!
//! Seeded at character creation from personality (and later, from background
//! tags). Aspirations give agents narrative arcs — "become a great hunter",
//! "protect the village", "find belonging" — that outlast any single reactive
//! `Goal`. Future tiers feed aspirations into ideal-self, desirability
//! appraisal, and the goal-arbitration biaser.
//!
//! Reads: nothing (pure data)
//! Writes: Aspirations (set once at spawn; progress mutated by other systems)
//! Upstream: psyche::personality
//! Downstream: ui::character_sheet, future appraisal/ideal-self/goal bias

use bevy::prelude::*;
use rand::Rng;
use rand::seq::IndexedRandom;
use smallvec::SmallVec;

use crate::agent::psyche::personality::PersonalityTraits;
use crate::agent::skills::SkillKind;

/// Maximum concurrent aspirations per agent. Keeps narrative arcs legible
/// and per-agent storage bounded.
pub const MAX_ASPIRATIONS: usize = 3;

const SEEDED_INTENSITY: f32 = 0.6;

/// Personality facet score above which a "high" trait pushes its candidate
/// into the seed pool. ≈ top-third of a uniform [0, 1] facet distribution,
/// so an average personality yields one or two seeded aspirations rather
/// than zero or maxing out the cap.
const SEED_THRESHOLD: f32 = 0.65;

#[derive(Component, Debug, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct Aspirations {
    #[reflect(ignore)]
    pub goals: SmallVec<[Aspiration; MAX_ASPIRATIONS]>,
}

#[derive(Debug, Clone, Reflect)]
pub struct Aspiration {
    pub kind: AspirationKind,
    pub target: Option<AspirationTarget>,
    /// Progress toward fulfillment in `[0, 1]`. At 1.0 downstream systems
    /// fire Satisfaction (T2.6) on the crossing tick.
    pub progress: f32,
    pub formed_at: u64,
    /// Strength of the held goal in `[0, 1]`. Affects future appraisal
    /// weighting and goal-bias magnitude.
    pub intensity: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum AspirationKind {
    BecomeGreatAt,
    FindPerson,
    Protect,
    Avenge,
    BuildHome,
    GainStatus,
    EscapeFate,
    Understand,
    FindBelonging,
    ProveSelf,
}

/// Concrete object of an aspiration. Only typed targets are represented —
/// the variants here are exactly the ones some seeder constructs today.
/// Future seeders (Background tags, runtime formation) will add typed
/// variants as they need them.
#[derive(Debug, Clone, Reflect)]
pub enum AspirationTarget {
    Skill(SkillKind),
    Entity(Entity),
}

/// Display metadata per kind, indexed by enum discriminant. Single source
/// of truth so adding a kind is a one-row change. Alignment with the enum
/// is asserted in tests.
const ASPIRATION_META: [(AspirationKind, &str); 10] = [
    (AspirationKind::BecomeGreatAt, "Master a craft"),
    (AspirationKind::FindPerson, "Find someone"),
    (AspirationKind::Protect, "Protect what matters"),
    (AspirationKind::Avenge, "Avenge a wrong"),
    (AspirationKind::BuildHome, "Build a home"),
    (AspirationKind::GainStatus, "Gain status"),
    (AspirationKind::EscapeFate, "Escape fate"),
    (AspirationKind::Understand, "Understand the world"),
    (AspirationKind::FindBelonging, "Find belonging"),
    (AspirationKind::ProveSelf, "Prove themselves"),
];

impl AspirationKind {
    pub const ALL: [AspirationKind; 10] = [
        Self::BecomeGreatAt,
        Self::FindPerson,
        Self::Protect,
        Self::Avenge,
        Self::BuildHome,
        Self::GainStatus,
        Self::EscapeFate,
        Self::Understand,
        Self::FindBelonging,
        Self::ProveSelf,
    ];

    pub fn display_name(&self) -> &'static str {
        ASPIRATION_META[*self as usize].1
    }
}

impl Aspiration {
    pub fn new(kind: AspirationKind, target: Option<AspirationTarget>, formed_at: u64) -> Self {
        Self {
            kind,
            target,
            progress: 0.0,
            formed_at,
            intensity: SEEDED_INTENSITY,
        }
    }

    pub fn is_fulfilled(&self) -> bool {
        self.progress >= 1.0
    }

    /// Add to progress, clamping to `[0, 1]`. Returns `true` exactly on the
    /// transition that crosses 1.0 — callers fire Satisfaction once on that
    /// edge. Subsequent calls after fulfillment return `false`.
    pub fn advance(&mut self, delta: f32) -> bool {
        let was_fulfilled = self.is_fulfilled();
        self.progress = (self.progress + delta).clamp(0.0, 1.0);
        !was_fulfilled && self.is_fulfilled()
    }

    pub fn label(&self) -> String {
        let suffix = match &self.target {
            Some(AspirationTarget::Skill(s)) => format!(": {}", s.display_name()),
            Some(AspirationTarget::Entity(e)) => format!(" (#{})", e.index()),
            None => String::new(),
        };
        format!("{}{suffix}", self.kind.display_name())
    }
}

impl Aspirations {
    /// Seed aspirations from personality at character creation. Picks up to
    /// `MAX_ASPIRATIONS` candidates whose triggering facet score exceeds
    /// `SEED_THRESHOLD`; ties broken by facet score, then deterministic RNG.
    pub fn from_personality(
        traits: &PersonalityTraits,
        formed_at: u64,
        rng: &mut impl Rng,
    ) -> Self {
        let candidates: [(f32, AspirationKind, Option<AspirationTarget>); 7] = [
            // Conscientiousness facets → achievement-flavoured arcs.
            (
                traits.conscientiousness.achievement_striving,
                AspirationKind::BecomeGreatAt,
                Some(pick_starter_skill(rng)),
            ),
            (
                traits.conscientiousness.competence,
                AspirationKind::ProveSelf,
                None,
            ),
            (
                traits.conscientiousness.order,
                AspirationKind::BuildHome,
                None,
            ),
            // Agreeableness facets → prosocial arcs.
            (traits.agreeableness.altruism, AspirationKind::Protect, None),
            (
                traits.agreeableness.tender_mindedness,
                AspirationKind::FindBelonging,
                None,
            ),
            // Openness facets → exploratory arcs.
            (traits.openness.ideas, AspirationKind::Understand, None),
            // Extraversion facets → status arcs.
            (
                traits.extraversion.assertiveness,
                AspirationKind::GainStatus,
                None,
            ),
        ];

        let mut filtered: Vec<_> = candidates
            .into_iter()
            .filter(|(score, _, _)| *score >= SEED_THRESHOLD)
            .collect();
        filtered.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        filtered.truncate(MAX_ASPIRATIONS);

        let goals = filtered
            .into_iter()
            .map(|(_, kind, target)| Aspiration::new(kind, target, formed_at))
            .collect();

        Self { goals }
    }
}

fn pick_starter_skill(rng: &mut impl Rng) -> AspirationTarget {
    let skill = *SkillKind::ALL
        .choose(rng)
        .expect("SkillKind::ALL is non-empty");
    AspirationTarget::Skill(skill)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::psyche::personality::{
        AgreeablenessFacets, ConscientiousnessFacets, OpennessFacets, PersonalityTraits,
    };
    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::SeedableRng;

    fn rng() -> ChaCha8Rng {
        ChaCha8Rng::seed_from_u64(0xA5)
    }

    #[test]
    fn caps_at_three_aspirations() {
        let traits = PersonalityTraits::uniform(1.0, 1.0, 1.0, 1.0, 0.5);
        let asp = Aspirations::from_personality(&traits, 0, &mut rng());
        assert_eq!(asp.goals.len(), MAX_ASPIRATIONS);
    }

    #[test]
    fn neutral_personality_seeds_no_aspirations() {
        let traits = PersonalityTraits::default();
        let asp = Aspirations::from_personality(&traits, 0, &mut rng());
        assert!(
            asp.goals.is_empty(),
            "neutral facets should seed zero aspirations, got {}",
            asp.goals.len()
        );
    }

    #[test]
    fn high_achievement_striving_seeds_become_great_at() {
        let traits = PersonalityTraits {
            conscientiousness: ConscientiousnessFacets {
                achievement_striving: 0.95,
                ..Default::default()
            },
            ..Default::default()
        };
        let asp = Aspirations::from_personality(&traits, 0, &mut rng());
        assert!(
            asp.goals
                .iter()
                .any(|g| g.kind == AspirationKind::BecomeGreatAt),
            "high achievement-striving should seed BecomeGreatAt; got {:?}",
            asp.goals.iter().map(|g| g.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn high_altruism_seeds_protect() {
        let traits = PersonalityTraits {
            agreeableness: AgreeablenessFacets {
                altruism: 0.95,
                ..Default::default()
            },
            ..Default::default()
        };
        let asp = Aspirations::from_personality(&traits, 0, &mut rng());
        assert!(
            asp.goals.iter().any(|g| g.kind == AspirationKind::Protect),
            "high altruism should seed Protect"
        );
    }

    #[test]
    fn high_openness_ideas_seeds_understand() {
        let traits = PersonalityTraits {
            openness: OpennessFacets {
                ideas: 0.95,
                ..Default::default()
            },
            ..Default::default()
        };
        let asp = Aspirations::from_personality(&traits, 0, &mut rng());
        assert!(
            asp.goals
                .iter()
                .any(|g| g.kind == AspirationKind::Understand)
        );
    }

    #[test]
    fn same_personality_and_seed_produces_same_aspirations() {
        let traits = PersonalityTraits::uniform(0.7, 0.8, 0.6, 0.85, 0.4);
        let a = Aspirations::from_personality(&traits, 100, &mut ChaCha8Rng::seed_from_u64(7));
        let b = Aspirations::from_personality(&traits, 100, &mut ChaCha8Rng::seed_from_u64(7));
        assert_eq!(a.goals.len(), b.goals.len());
        for (ga, gb) in a.goals.iter().zip(b.goals.iter()) {
            assert_eq!(ga.kind, gb.kind);
            assert_eq!(ga.formed_at, gb.formed_at);
        }
    }

    #[test]
    fn advance_clamps_and_signals_fulfilment_once() {
        let mut a = Aspiration::new(AspirationKind::BuildHome, None, 0);
        assert!(!a.advance(0.5));
        assert!(!a.is_fulfilled());

        // Crossing 1.0 fires once.
        assert!(a.advance(0.6));
        assert!(a.is_fulfilled());
        assert_eq!(a.progress, 1.0);

        // Subsequent advances do not re-fire.
        assert!(!a.advance(0.5));
        assert_eq!(a.progress, 1.0);
    }

    #[test]
    fn label_includes_target_when_present() {
        let a = Aspiration::new(
            AspirationKind::BecomeGreatAt,
            Some(AspirationTarget::Skill(SkillKind::Combat)),
            0,
        );
        let label = a.label();
        assert!(label.contains("Master a craft"));
        assert!(label.contains("Combat"));
    }

    #[test]
    fn aspiration_meta_table_aligns_with_enum() {
        for kind in AspirationKind::ALL {
            assert_eq!(
                ASPIRATION_META[kind as usize].0, kind,
                "ASPIRATION_META misaligned at {kind:?}; reorder the table to match the enum"
            );
        }
    }
}
