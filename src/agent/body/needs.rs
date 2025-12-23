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
