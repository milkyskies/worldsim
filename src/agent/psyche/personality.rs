//! Personality: Big Five trait component and the PersonalityTrait indexing enum.
//!
//! Reads: nothing (pure data)
//! Writes: Personality (set once at spawn)
//! Upstream: agent spawning
//! Downstream: nervous_system::urgency (trait modifiers), psyche::emotions, ui::character_sheet

use bevy::prelude::*;
use rand::Rng;

#[derive(Component, Debug, Clone, Reflect, Default)]
pub struct Personality {
    pub traits: PersonalityTraits,
}

impl Personality {
    pub fn random() -> Self {
        Self {
            traits: PersonalityTraits::random(),
        }
    }

    /// Deterministic variant — used by TestWorld and other seeded contexts
    /// where reproducibility matters.
    pub fn from_rng<R: Rng>(rng: &mut R) -> Self {
        Self {
            traits: PersonalityTraits::from_rng(rng),
        }
    }
}

#[derive(Debug, Clone, Reflect)]
pub struct PersonalityTraits {
    /// Curiosity vs Traditionalism (0.0 - 1.0)
    pub openness: f32,
    /// Discipline vs Spontaneity (0.0 - 1.0)
    pub conscientiousness: f32,
    /// Social Energy vs Solitude (0.0 - 1.0)
    pub extraversion: f32,
    /// Compassion vs Self-Interest (0.0 - 1.0)
    pub agreeableness: f32,
    /// Anxiety vs Emotional Stability (0.0 - 1.0)
    /// Note: Roadmap says "Neuroticism", often "Emotional Stability" is the inverse.
    /// We'll stick to Neuroticism as per roadmap (High = Anxious).
    pub neuroticism: f32,
}

impl Default for PersonalityTraits {
    fn default() -> Self {
        Self {
            openness: 0.5,
            conscientiousness: 0.5,
            extraversion: 0.5,
            agreeableness: 0.5,
            neuroticism: 0.5,
        }
    }
}

impl PersonalityTraits {
    pub fn random() -> Self {
        Self::from_rng(&mut rand::rng())
    }

    /// Sample traits from an explicit RNG (deterministic when caller seeds it).
    pub fn from_rng<R: Rng>(rng: &mut R) -> Self {
        Self {
            openness: rng.random_range(0.0..=1.0),
            conscientiousness: rng.random_range(0.0..=1.0),
            extraversion: rng.random_range(0.0..=1.0),
            agreeableness: rng.random_range(0.0..=1.0),
            neuroticism: rng.random_range(0.0..=1.0),
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
            Self::Openness => traits.openness,
            Self::Conscientiousness => traits.conscientiousness,
            Self::Extraversion => traits.extraversion,
            Self::Agreeableness => traits.agreeableness,
            Self::Neuroticism => traits.neuroticism,
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
}
