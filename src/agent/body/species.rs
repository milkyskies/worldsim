//! Species Profile System
//!
//! Defines cognitive and physical parameters that differ between species.
//! All agents use the same architecture but with different weights/limits.

use bevy::prelude::*;

/// What species this agent belongs to
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Default)]
pub enum Species {
    #[default]
    Human,
    Deer,
    Wolf,
    Rabbit,
    Bird,
}

/// Dietary requirements - determines what foods are edible
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, Default)]
pub enum Diet {
    Herbivore, // Plants only
    Carnivore, // Meat only
    #[default]
    Omnivore, // Everything
}

/// Defines cognitive and physical parameters for a species.
/// Attached as a component to each agent entity.
#[derive(Component, Clone, Reflect, Debug)]
#[reflect(Component)]
pub struct SpeciesProfile {
    /// What species this agent belongs to
    pub species: Species,

    // === Cognitive Parameters ===
    /// Maximum steps in a plan (1 = reactive, 10 = strategic)
    pub max_plan_depth: usize,

    /// Maximum triples in MindGraph before aggressive decay
    pub memory_capacity: usize,

    /// How fast memories fade (0.0 = perfect, 1.0 = instant)
    pub memory_decay_rate: f32,

    // === Brain Power Base Weights (should sum to ~1.0) ===
    /// Base influence of survival brain
    pub survival_weight: f32,

    /// Base influence of emotional brain
    pub emotional_weight: f32,

    /// Base influence of rational brain
    pub rational_weight: f32,

    // === Physical ===
    /// Movement speed multiplier
    pub base_speed: f32,

    /// How far can see
    pub vision_range: f32,

    /// Dietary requirements
    pub diet: Diet,
}

impl Default for SpeciesProfile {
    fn default() -> Self {
        Self::human()
    }
}

impl SpeciesProfile {
    /// Human profile - high cognition, balanced brains
    pub fn human() -> Self {
        Self {
            species: Species::Human,

            max_plan_depth: 10,
            memory_capacity: 10000,
            memory_decay_rate: 0.01,

            survival_weight: 0.33,
            emotional_weight: 0.33,
            rational_weight: 0.34,

            base_speed: 1.0,
            vision_range: 100.0,
            diet: Diet::Omnivore,
        }
    }

    /// Deer profile - survival-focused, limited planning
    pub fn deer() -> Self {
        Self {
            species: Species::Deer,

            max_plan_depth: 2,
            memory_capacity: 100,
            memory_decay_rate: 0.3,

            survival_weight: 0.70,
            emotional_weight: 0.20,
            rational_weight: 0.10,

            base_speed: 1.2,
            vision_range: 80.0,
            diet: Diet::Herbivore,
        }
    }

    /// Wolf profile - pack hunter, moderate planning
    pub fn wolf() -> Self {
        Self {
            species: Species::Wolf,

            max_plan_depth: 4,
            memory_capacity: 500,
            memory_decay_rate: 0.1,

            survival_weight: 0.40,
            emotional_weight: 0.35,
            rational_weight: 0.25,

            base_speed: 1.4,
            vision_range: 120.0,
            diet: Diet::Carnivore,
        }
    }

    /// Rabbit profile - very reactive, minimal planning
    pub fn rabbit() -> Self {
        Self {
            species: Species::Rabbit,

            max_plan_depth: 1,
            memory_capacity: 50,
            memory_decay_rate: 0.5,

            survival_weight: 0.85,
            emotional_weight: 0.10,
            rational_weight: 0.05,

            base_speed: 1.5,
            vision_range: 60.0,
            diet: Diet::Herbivore,
        }
    }
}
