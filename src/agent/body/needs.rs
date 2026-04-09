//! Agent needs: PhysicalNeeds, Consciousness, and PsychologicalDrives components — the source of truth for agent state.
//!
//! Reads: nothing (pure data components, written by other systems)
//! Writes: PhysicalNeeds, Consciousness, PsychologicalDrives (ECS components)
//! Upstream: nervous_system::activity_effects (modifies values each tick)
//! Downstream: nervous_system::urgency (drives urgency scores), brains::arbitration (survival power), brain_system

use bevy::prelude::*;

/// Physical needs - THE source of truth for survival needs
/// All agents have this
#[derive(Component, Reflect, Debug, Clone)]
#[reflect(Component)]
pub struct PhysicalNeeds {
    pub hunger: f32, // 0-100, increases over time
    pub thirst: f32, // 0-100, increases over time
    pub energy: f32, // 0-100, decreases with activity, restored by sleep
    pub health: f32, // 0-100, damaged by starvation/injuries
}

impl Default for PhysicalNeeds {
    fn default() -> Self {
        Self {
            hunger: 0.0,
            thirst: 0.0,
            energy: 100.0,
            health: 100.0,
        }
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

/// Higher psychological drives (Humans only)
#[derive(Component, Reflect, Debug, Clone)]
#[reflect(Component)]
pub struct PsychologicalDrives {
    pub social: f32,    // 0-1
    pub fun: f32,       // 0-1
    pub curiosity: f32, // 0-1
    pub status: f32,    // 0-1
    pub security: f32,  // 0-1
    pub autonomy: f32,  // 0-1
}

impl Default for PsychologicalDrives {
    fn default() -> Self {
        Self {
            social: 0.5,
            fun: 0.5,
            curiosity: 0.5,
            status: 0.5,
            security: 0.5,
            autonomy: 0.5,
        }
    }
}

impl PsychologicalDrives {
    /// Initialise drive baselines from Big Five personality traits.
    ///
    /// Personality shapes what an agent fundamentally wants, not just how
    /// urgently they pursue it. The urgency system (nervous_system::urgency)
    /// further modulates moment-to-moment priority via `PersonalityMod`.
    pub fn from_personality(traits: &crate::agent::psyche::personality::PersonalityTraits) -> Self {
        Self {
            // Extraverts need more social contact as a baseline
            social: traits.extraversion,
            // Open personalities are naturally more curious
            curiosity: traits.openness,
            // Neurotic personalities have a higher baseline need for security
            security: traits.neuroticism,
            // Conscientious personalities care more about status/achievement
            status: traits.conscientiousness,
            // Disagreeable personalities need more autonomy
            autonomy: 1.0 - traits.agreeableness,
            fun: 0.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::psyche::personality::PersonalityTraits;

    #[test]
    fn high_extraversion_raises_social_drive() {
        let traits = PersonalityTraits {
            extraversion: 0.9,
            ..Default::default()
        };
        let drives = PsychologicalDrives::from_personality(&traits);
        assert!(
            drives.social > 0.8,
            "social should track extraversion (got {})",
            drives.social
        );
    }

    #[test]
    fn low_extraversion_lowers_social_drive() {
        let traits = PersonalityTraits {
            extraversion: 0.1,
            ..Default::default()
        };
        let drives = PsychologicalDrives::from_personality(&traits);
        assert!(
            drives.social < 0.2,
            "introvert should have low social drive (got {})",
            drives.social
        );
    }

    #[test]
    fn high_openness_raises_curiosity() {
        let traits = PersonalityTraits {
            openness: 0.9,
            ..Default::default()
        };
        let drives = PsychologicalDrives::from_personality(&traits);
        assert!(
            drives.curiosity > 0.8,
            "curiosity should track openness (got {})",
            drives.curiosity
        );
    }

    #[test]
    fn high_neuroticism_raises_security_need() {
        let traits = PersonalityTraits {
            neuroticism: 0.9,
            ..Default::default()
        };
        let drives = PsychologicalDrives::from_personality(&traits);
        assert!(
            drives.security > 0.8,
            "neurotic agent should have high security need (got {})",
            drives.security
        );
    }

    #[test]
    fn high_agreeableness_lowers_autonomy_need() {
        let traits = PersonalityTraits {
            agreeableness: 0.9,
            ..Default::default()
        };
        let drives = PsychologicalDrives::from_personality(&traits);
        assert!(
            drives.autonomy < 0.2,
            "agreeable agent should have low autonomy need (got {})",
            drives.autonomy
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
            ("Hunger", self.hunger, Scale::Percentage),
            ("Thirst", self.thirst, Scale::Percentage),
            ("Energy", self.energy, Scale::Percentage),
            ("Health", self.health, Scale::Percentage),
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
            ("Social", self.social, Scale::Normalized),
            ("Fun", self.fun, Scale::Normalized),
            ("Curiosity", self.curiosity, Scale::Normalized),
            ("Status", self.status, Scale::Normalized),
            ("Security", self.security, Scale::Normalized),
            ("Autonomy", self.autonomy, Scale::Normalized),
        ]
    }
}
