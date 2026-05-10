//! Personality: Big Five with NEO-PI-R 30 facets.
//!
//! Each Big Five trait decomposes into six lower-level facets per Costa and
//! McCrae's NEO-PI-R. Trait-level scores are derived as the mean of their
//! facets, never stored separately.
//!
//! Reads: nothing (pure data)
//! Writes: Personality (set once at spawn)
//! Upstream: agent spawning
//! Downstream: nervous_system::urgency, psyche::emotions, ui::character_sheet

use bevy::prelude::*;
use rand::Rng;
use rand_distr::{Distribution, Normal};

#[derive(Component, Debug, Clone, Reflect, Default)]
pub struct Personality {
    pub traits: PersonalityTraits,
}

#[derive(Debug, Clone, Reflect, Default)]
pub struct PersonalityTraits {
    pub openness: OpennessFacets,
    pub conscientiousness: ConscientiousnessFacets,
    pub extraversion: ExtraversionFacets,
    pub agreeableness: AgreeablenessFacets,
    pub neuroticism: NeuroticismFacets,
}

pub const FACETS_PER_TRAIT: usize = 6;

/// Standard deviation of facet noise around the trait baseline when sampling.
/// Sized so sibling-facet Pearson correlation stays above 0.3 across founder
/// genomes (whose trait-score std is ≈0.1) while still producing visible
/// facet variation.
const FACET_NOISE_STD: f32 = 0.05;

macro_rules! facets {
    ($struct_name:ident { $( $field:ident => $display:literal ),+ $(,)? }) => {
        #[derive(Debug, Clone, Reflect)]
        pub struct $struct_name {
            $( pub $field: f32, )+
        }

        impl Default for $struct_name {
            fn default() -> Self {
                Self { $( $field: 0.5, )+ }
            }
        }

        impl $struct_name {
            #[inline]
            pub fn mean(&self) -> f32 {
                let sum: f32 = $( self.$field + )+ 0.0;
                sum / FACETS_PER_TRAIT as f32
            }

            pub fn uniform(value: f32) -> Self {
                Self { $( $field: value, )+ }
            }

            #[inline]
            pub fn as_array(&self) -> [f32; FACETS_PER_TRAIT] {
                [ $( self.$field, )+ ]
            }

            pub const FACET_NAMES: [&'static str; FACETS_PER_TRAIT] = [ $( $display, )+ ];

            /// Sample six facets around `trait_mean` using the supplied RNG.
            /// Each facet is `trait_mean + N(0, FACET_NOISE_STD)`, clamped to
            /// `[0, 1]`. Sibling facets share the trait mean as a common
            /// factor, producing the within-trait correlation observed in
            /// the NEO-PI-R.
            pub fn sample_around(trait_mean: f32, rng: &mut impl Rng) -> Self {
                let normal = Normal::new(trait_mean, FACET_NOISE_STD)
                    .expect("FACET_NOISE_STD is a valid finite positive constant");
                Self { $( $field: normal.sample(rng).clamp(0.0, 1.0), )+ }
            }
        }
    };
}

facets!(OpennessFacets {
    fantasy => "Fantasy",
    aesthetics => "Aesthetics",
    feelings => "Feelings",
    actions => "Actions",
    ideas => "Ideas",
    values => "Values",
});

facets!(ConscientiousnessFacets {
    competence => "Competence",
    order => "Order",
    dutifulness => "Dutifulness",
    achievement_striving => "Achievement-Striving",
    self_discipline => "Self-Discipline",
    deliberation => "Deliberation",
});

facets!(ExtraversionFacets {
    warmth => "Warmth",
    gregariousness => "Gregariousness",
    assertiveness => "Assertiveness",
    activity => "Activity",
    excitement_seeking => "Excitement-Seeking",
    positive_emotions => "Positive-Emotions",
});

facets!(AgreeablenessFacets {
    trust => "Trust",
    straightforwardness => "Straightforwardness",
    altruism => "Altruism",
    compliance => "Compliance",
    modesty => "Modesty",
    tender_mindedness => "Tender-Mindedness",
});

facets!(NeuroticismFacets {
    anxiety => "Anxiety",
    angry_hostility => "Angry-Hostility",
    depression => "Depression",
    self_consciousness => "Self-Consciousness",
    impulsiveness => "Impulsiveness",
    vulnerability => "Vulnerability",
});

impl PersonalityTraits {
    #[inline]
    pub fn openness(&self) -> f32 {
        self.openness.mean()
    }
    #[inline]
    pub fn conscientiousness(&self) -> f32 {
        self.conscientiousness.mean()
    }
    #[inline]
    pub fn extraversion(&self) -> f32 {
        self.extraversion.mean()
    }
    #[inline]
    pub fn agreeableness(&self) -> f32 {
        self.agreeableness.mean()
    }
    #[inline]
    pub fn neuroticism(&self) -> f32 {
        self.neuroticism.mean()
    }

    /// Build trait-level personality where every facet within each Big Five
    /// trait is set to the trait score. Loses facet variation; reserved for
    /// callers that only care about trait-level behavior.
    pub fn uniform(
        openness: f32,
        conscientiousness: f32,
        extraversion: f32,
        agreeableness: f32,
        neuroticism: f32,
    ) -> Self {
        Self {
            openness: OpennessFacets::uniform(openness),
            conscientiousness: ConscientiousnessFacets::uniform(conscientiousness),
            extraversion: ExtraversionFacets::uniform(extraversion),
            agreeableness: AgreeablenessFacets::uniform(agreeableness),
            neuroticism: NeuroticismFacets::uniform(neuroticism),
        }
    }

    /// Sample facets around per-trait baselines using `rng`. Sibling facets
    /// correlate because they share the trait baseline, while each gets its
    /// own small Gaussian perturbation.
    pub fn sample(
        openness: f32,
        conscientiousness: f32,
        extraversion: f32,
        agreeableness: f32,
        neuroticism: f32,
        rng: &mut impl Rng,
    ) -> Self {
        Self {
            openness: OpennessFacets::sample_around(openness, rng),
            conscientiousness: ConscientiousnessFacets::sample_around(conscientiousness, rng),
            extraversion: ExtraversionFacets::sample_around(extraversion, rng),
            agreeableness: AgreeablenessFacets::sample_around(agreeableness, rng),
            neuroticism: NeuroticismFacets::sample_around(neuroticism, rng),
        }
    }
}

#[derive(Debug, Clone, Copy, Reflect, Default, PartialEq, Eq)]
pub enum PersonalityTrait {
    #[default]
    Openness,
    Conscientiousness,
    Extraversion,
    Agreeableness,
    Neuroticism,
}

impl PersonalityTrait {
    pub const ALL: [PersonalityTrait; 5] = [
        Self::Openness,
        Self::Conscientiousness,
        Self::Extraversion,
        Self::Agreeableness,
        Self::Neuroticism,
    ];

    pub fn get(&self, traits: &PersonalityTraits) -> f32 {
        match self {
            Self::Openness => traits.openness(),
            Self::Conscientiousness => traits.conscientiousness(),
            Self::Extraversion => traits.extraversion(),
            Self::Agreeableness => traits.agreeableness(),
            Self::Neuroticism => traits.neuroticism(),
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Openness => "Openness",
            Self::Conscientiousness => "Conscientiousness",
            Self::Extraversion => "Extraversion",
            Self::Agreeableness => "Agreeableness",
            Self::Neuroticism => "Neuroticism",
        }
    }

    /// Three short descriptions covering low, mid, and high values of this
    /// trait. Ordered `[low, mid, high]`. Used by the character sheet to
    /// translate raw 0..1 values into readable personality blurbs.
    pub fn descriptions(&self) -> [&'static str; 3] {
        match self {
            Self::Openness => [
                "Closed off, prefers routine, sceptical of new things",
                "Practical, conventional, content with the familiar",
                "Curious, open to new experiences, enjoys variety",
            ],
            Self::Conscientiousness => [
                "Unreliable, easily distracted, impulsive",
                "Balanced, generally dependable",
                "Disciplined, organised, strong sense of duty",
            ],
            Self::Extraversion => [
                "Reserved, prefers solitude, quiet",
                "Ambiverted, comfortable alone or with others",
                "Outgoing, seeks social contact, energised by people",
            ],
            Self::Agreeableness => [
                "Competitive, blunt, sceptical of others' motives",
                "Fair-minded, neither pushover nor antagonist",
                "Warm, cooperative, trusting, quick to help",
            ],
            Self::Neuroticism => [
                "Calm, emotionally stable, resilient under stress",
                "Generally stable, occasional worries",
                "Anxious, reactive, easily stressed",
            ],
        }
    }

    pub fn facet_names(&self) -> [&'static str; FACETS_PER_TRAIT] {
        match self {
            Self::Openness => OpennessFacets::FACET_NAMES,
            Self::Conscientiousness => ConscientiousnessFacets::FACET_NAMES,
            Self::Extraversion => ExtraversionFacets::FACET_NAMES,
            Self::Agreeableness => AgreeablenessFacets::FACET_NAMES,
            Self::Neuroticism => NeuroticismFacets::FACET_NAMES,
        }
    }

    pub fn facet_values(&self, traits: &PersonalityTraits) -> [f32; FACETS_PER_TRAIT] {
        match self {
            Self::Openness => traits.openness.as_array(),
            Self::Conscientiousness => traits.conscientiousness.as_array(),
            Self::Extraversion => traits.extraversion.as_array(),
            Self::Agreeableness => traits.agreeableness.as_array(),
            Self::Neuroticism => traits.neuroticism.as_array(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::SeedableRng;

    #[test]
    fn default_traits_are_neutral_at_every_facet() {
        let traits = PersonalityTraits::default();
        for facet in traits.openness.as_array() {
            assert_eq!(facet, 0.5);
        }
        assert_eq!(traits.openness(), 0.5);
        assert_eq!(traits.conscientiousness(), 0.5);
        assert_eq!(traits.extraversion(), 0.5);
        assert_eq!(traits.agreeableness(), 0.5);
        assert_eq!(traits.neuroticism(), 0.5);
    }

    #[test]
    fn trait_accessor_returns_mean_of_facets() {
        let mut traits = PersonalityTraits::default();
        traits.openness.fantasy = 0.0;
        traits.openness.aesthetics = 0.0;
        traits.openness.feelings = 0.0;
        traits.openness.actions = 1.0;
        traits.openness.ideas = 1.0;
        traits.openness.values = 1.0;
        assert!((traits.openness() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn uniform_constructor_preserves_trait_level_values() {
        let traits = PersonalityTraits::uniform(0.7, 0.3, 0.6, 0.2, 0.8);
        assert!((traits.openness() - 0.7).abs() < 1e-6);
        assert!((traits.conscientiousness() - 0.3).abs() < 1e-6);
        assert!((traits.extraversion() - 0.6).abs() < 1e-6);
        assert!((traits.agreeableness() - 0.2).abs() < 1e-6);
        assert!((traits.neuroticism() - 0.8).abs() < 1e-6);
    }

    #[test]
    fn sibling_facets_correlate_within_trait() {
        // Sample a population of agents whose trait baselines vary across
        // the typical founder range (≈0.25 to 0.75). Sibling facets should
        // correlate above 0.3 because they share the trait baseline.
        let mut rng = ChaCha8Rng::seed_from_u64(42);
        let mut fantasy = Vec::with_capacity(200);
        let mut aesthetics = Vec::with_capacity(200);
        for _ in 0..200 {
            let baseline: f32 = 0.25 + rng.random::<f32>() * 0.5;
            let facets = OpennessFacets::sample_around(baseline, &mut rng);
            fantasy.push(facets.fantasy);
            aesthetics.push(facets.aesthetics);
        }
        let r = pearson(&fantasy, &aesthetics);
        assert!(
            r > 0.3,
            "expected sibling facets to correlate >0.3, got {r}"
        );
    }

    #[test]
    fn sampled_facets_stay_in_unit_interval() {
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        for _ in 0..500 {
            let baseline: f32 = rng.random();
            let facets = NeuroticismFacets::sample_around(baseline, &mut rng);
            for v in facets.as_array() {
                assert!((0.0..=1.0).contains(&v), "facet out of range: {v}");
            }
        }
    }

    fn pearson(xs: &[f32], ys: &[f32]) -> f32 {
        let n = xs.len() as f32;
        let mx: f32 = xs.iter().sum::<f32>() / n;
        let my: f32 = ys.iter().sum::<f32>() / n;
        let mut num = 0.0;
        let mut dx2 = 0.0;
        let mut dy2 = 0.0;
        for (x, y) in xs.iter().zip(ys.iter()) {
            let dx = x - mx;
            let dy = y - my;
            num += dx * dy;
            dx2 += dx * dx;
            dy2 += dy * dy;
        }
        num / (dx2.sqrt() * dy2.sqrt())
    }
}
