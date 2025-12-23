use crate::agent::nervous_system::urgency::UrgencySource;
use crate::agent::psyche::personality::PersonalityTrait;
use bevy::prelude::*;

/// Curve type for mapping input (0-1) to output (0-1)
#[derive(Debug, Clone, Reflect)]
#[derive(Default)]
pub enum ResponseCurve {
    /// Linear: output = input
    #[default]
    Linear,
    /// Exponential: output = input^power
    Exponential(f32),
    /// Sigmoid: smooth S-curve, k controls steepness
    Sigmoid { k: f32, midpoint: f32 },
    /// Step: 0 below threshold, 1 above
    Step { threshold: f32 },
}

impl ResponseCurve {
    pub fn apply(&self, input: f32) -> f32 {
        let clamped = input.clamp(0.0, 1.0);
        match self {
            ResponseCurve::Linear => clamped,
            ResponseCurve::Exponential(power) => clamped.powf(*power),
            ResponseCurve::Sigmoid { k, midpoint } => {
                1.0 / (1.0 + (-(k * (clamped - midpoint))).exp())
            }
            ResponseCurve::Step { threshold } => {
                if clamped >= *threshold {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
}


/// Personality modifier configuration
#[derive(Debug, Clone, Reflect, Default)]
pub struct PersonalityMod {
    /// Which trait modifies this drive
    pub trait_type: PersonalityTrait,
    /// Base multiplier when trait is 0
    pub base: f32,
    /// Scaling factor: final = base + (trait_value * scale)
    pub scale: f32,
}

impl PersonalityMod {
    pub fn compute(&self, personality: &crate::agent::psyche::personality::Personality) -> f32 {
        let trait_value = self.trait_type.get(&personality.traits);
        self.base + (trait_value * self.scale)
    }
}

/// How a context modifier affects the urgency score
#[derive(Debug, Clone, Copy, Reflect, Default, PartialEq, Eq)]
pub enum ModifierOp {
    #[default]
    DampenByHigh, // score *= (1.0 - input * factor)
    DampenByLow, // score *= input * factor (Linear scaling)
    BoostBy,     // score *= (1.0 + input * factor)
    Add,         // score += input * factor
    Subtract,    // score -= input * factor
}

/// A context-dependent modifier to the urgency score
#[derive(Debug, Clone, Reflect)]
pub struct ContextModifier {
    /// Which urgency source to read as input (using source as a proxy for the variable)
    /// e.g. UrgencySource::Hunger implies reading the Hunger need
    pub input_source: UrgencySource,
    /// How to apply it
    pub operation: ModifierOp,
    /// Scaling factor
    pub factor: f32,
}

/// Configuration for a single drive/urgency source
#[derive(Debug, Clone, Reflect)]
pub struct DriveConfig {
    /// Human-readable name for debugging
    pub name: String,
    /// Which urgency source this produces
    pub source: UrgencySource,

    // --- INPUT ---
    // Input is now implicitly defined by the source type in urgency.rs
    // e.g. Hunger source always reads PhysicalNeeds.hunger
    /// Constant base value if no dynamic input is available
    pub base_constant: f32,

    // --- MATH ---
    /// Response curve for the base value
    pub curve: ResponseCurve,
    /// Personality-based sensitivity modifier
    pub sensitivity: PersonalityMod,

    // --- CONTEXT MODIFIERS ---
    /// Additional modifiers based on other state (dampeners, boosters)
    pub modifiers: Vec<ContextModifier>,

    // --- THRESHOLDS ---
    /// Minimum score to emit (ignore noise)
    pub min_threshold: f32,

    /// If true, ignores sensory dampening (e.g. Pain wakes you up)
    pub bypasses_gating: bool,
}

impl Default for DriveConfig {
    fn default() -> Self {
        Self {
            name: "Unknown".to_string(),
            source: UrgencySource::default(),
            base_constant: 0.0,
            curve: ResponseCurve::Linear,
            sensitivity: PersonalityMod::default(),
            modifiers: vec![],
            min_threshold: 0.01,
            bypasses_gating: false,
        }
    }
}

/// Sensory channel configuration
#[derive(Debug, Clone, Reflect, Default)]
pub struct SensoryChannelConfig {
    /// Which urgency sources belong to this channel
    pub sources: Vec<UrgencySource>,
}

/// Central configuration resource for the nervous system
#[derive(Resource, Debug, Clone, Reflect)]
#[reflect(Resource)]
pub struct NervousSystemConfig {
    /// Drive configurations
    pub drives: Vec<DriveConfig>,
    /// Momentum bonus multiplier for current activity
    pub momentum_bonus: f32,
    /// Sensory channels (for future emergent gating)
    pub interoception: SensoryChannelConfig,
    pub exteroception: SensoryChannelConfig,
    pub proprioception: SensoryChannelConfig,
    /// Tick interval for running expensive thinking/urgency updates
    pub thinking_interval: u64,
    /// Tick interval for running perception updates (default: 10 ticks = 6 Hz)
    pub perception_interval: u64,
}

impl Default for NervousSystemConfig {
    fn default() -> Self {
        Self {
            drives: vec![
                // PAIN
                DriveConfig {
                    name: "Pain".to_string(),
                    source: UrgencySource::Pain,
                    base_constant: 0.0,
                    curve: ResponseCurve::Exponential(2.0),
                    sensitivity: PersonalityMod {
                        trait_type: PersonalityTrait::Neuroticism,
                        base: 1.0,
                        scale: 0.5,
                    },
                    modifiers: vec![],
                    min_threshold: 0.05,
                    bypasses_gating: true,
                },
                // THIRST
                DriveConfig {
                    name: "Thirst".to_string(),
                    source: UrgencySource::Thirst,
                    base_constant: 0.0,
                    curve: ResponseCurve::Sigmoid {
                        k: 10.0,
                        midpoint: 0.6,
                    },
                    sensitivity: PersonalityMod {
                        trait_type: PersonalityTrait::Neuroticism,
                        base: 0.8,
                        scale: 0.4,
                    },
                    modifiers: vec![],
                    min_threshold: 0.01,
                    bypasses_gating: false,
                },
                // HUNGER
                DriveConfig {
                    name: "Hunger".to_string(),
                    source: UrgencySource::Hunger,
                    base_constant: 0.0,
                    curve: ResponseCurve::Sigmoid {
                        k: 10.0,
                        midpoint: 0.6,
                    },
                    sensitivity: PersonalityMod {
                        trait_type: PersonalityTrait::Neuroticism,
                        base: 0.8,
                        scale: 0.4,
                    },
                    modifiers: vec![],
                    min_threshold: 0.01,
                    bypasses_gating: false,
                },
                // ENERGY (Fatigue): Note inputs are inverted logic in urgency.rs if needed,
                // but config just defines the response curve.
                DriveConfig {
                    name: "Fatigue".to_string(),
                    source: UrgencySource::Energy,
                    base_constant: 0.0,
                    curve: ResponseCurve::Sigmoid {
                        k: 10.0,
                        midpoint: 0.6,
                    },
                    sensitivity: PersonalityMod {
                        trait_type: PersonalityTrait::Neuroticism,
                        base: 0.8,
                        scale: 0.3,
                    },
                    modifiers: vec![],
                    min_threshold: 0.01,
                    bypasses_gating: false,
                },
                // SOCIAL
                DriveConfig {
                    name: "Social".to_string(),
                    source: UrgencySource::Social,
                    base_constant: 0.0,
                    curve: ResponseCurve::Linear,
                    sensitivity: PersonalityMod {
                        trait_type: PersonalityTrait::Extraversion,
                        base: 0.5,
                        scale: 1.0,
                    },
                    modifiers: vec![
                        // UrgencySource::Energy is implicit "Energy" input
                        ContextModifier {
                            input_source: UrgencySource::Energy, // Acts as "Have Energy?"
                            operation: ModifierOp::DampenByLow,
                            factor: 1.0,
                        },
                    ],
                    min_threshold: 0.01,
                    bypasses_gating: false,
                },
                // FEAR
                DriveConfig {
                    name: "Fear".to_string(),
                    source: UrgencySource::Fear,
                    base_constant: 0.0,
                    curve: ResponseCurve::Sigmoid {
                        midpoint: 0.4,
                        k: 10.0,
                    },
                    sensitivity: PersonalityMod {
                        trait_type: PersonalityTrait::Neuroticism,
                        base: 1.0,
                        scale: 1.0,
                    },
                    modifiers: vec![],
                    min_threshold: 0.0,
                    bypasses_gating: true,
                },
                // BOREDOM
                DriveConfig {
                    name: "Boredom".to_string(),
                    source: UrgencySource::Boredom,
                    base_constant: 0.2,
                    curve: ResponseCurve::Linear,
                    sensitivity: PersonalityMod {
                        trait_type: PersonalityTrait::Openness,
                        base: 0.5,
                        scale: 1.0,
                    },
                    modifiers: vec![],
                    min_threshold: 0.0,
                    bypasses_gating: false,
                },
            ],
            momentum_bonus: 1.5,
            interoception: SensoryChannelConfig {
                sources: vec![
                    UrgencySource::Hunger,
                    UrgencySource::Pain,
                    UrgencySource::Thirst,
                ],
            },
            exteroception: SensoryChannelConfig {
                sources: vec![UrgencySource::Social, UrgencySource::Fear],
            },
            proprioception: SensoryChannelConfig {
                sources: vec![
                    UrgencySource::Boredom,
                    UrgencySource::Fun,
                    UrgencySource::Energy,
                ],
            },
            thinking_interval: 60,
            perception_interval: 10,
        }
    }
}

impl NervousSystemConfig {
    /// Get config for a specific drive source
    pub fn get_drive(&self, source: UrgencySource) -> Option<&DriveConfig> {
        self.drives.iter().find(|d| d.source == source)
    }
}
