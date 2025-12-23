use bevy::prelude::*;
use rand::Rng;

#[derive(Component, Debug, Clone, Reflect)]
#[derive(Default)]
pub struct Personality {
    pub traits: PersonalityTraits,
}


impl Personality {
    pub fn random() -> Self {
        Self {
            traits: PersonalityTraits::random(),
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
        let mut rng = rand::rng();
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
    pub fn get(&self, traits: &PersonalityTraits) -> f32 {
        match self {
            Self::Openness => traits.openness,
            Self::Conscientiousness => traits.conscientiousness,
            Self::Extraversion => traits.extraversion,
            Self::Agreeableness => traits.agreeableness,
            Self::Neuroticism => traits.neuroticism,
        }
    }
}
