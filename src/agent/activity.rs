use crate::agent::actions::ActionType;
use crate::agent::psyche::emotions::EmotionType;
use bevy::prelude::*;

#[derive(Component, Debug, Clone, Reflect, PartialEq)]
#[reflect(Component)]
#[derive(Default)]
pub enum CurrentActivity {
    #[default]
    Idle,
    Wandering,
    Sleeping,
    WakeUp, // Transition state
    Eating(u32),
    Harvesting(Entity, u32), // (target, countdown per apple)
    MovingTo(Vec2),
    Exploring(Vec2), // Exploring toward a direction to find resources
}


impl CurrentActivity {
    /// Helper to access mutable state for a specific action type.
    /// Returns (Target, Timer) if the current activity matches the action type.
    /// This allows generic systems to tick the timer and get target info without pattern matching boilerplate.
    pub fn get_action_state_mut(
        &mut self,
        action_type: ActionType,
    ) -> Option<(Option<Entity>, &mut u32)> {
        match (self, action_type) {
            (Self::Harvesting(target, timer), ActionType::Harvest) => Some((Some(*target), timer)),
            (Self::Eating(timer), ActionType::Eat) => Some((None, timer)),
            _ => None,
        }
    }

    /// Maps this activity to its corresponding ActionType for process matching.
    pub fn action_type(&self) -> ActionType {
        match self {
            Self::Idle => ActionType::Idle,
            Self::Wandering => ActionType::Wander,
            Self::Sleeping => ActionType::Sleep,
            Self::WakeUp => ActionType::WakeUp,
            Self::Eating(_) => ActionType::Eat,
            Self::Harvesting(_, _) => ActionType::Harvest,
            Self::MovingTo(_) => ActionType::Walk,
            Self::Exploring(_) => ActionType::Explore,
        }
    }
}

// ============================================================================
// ACTIVITY EFFECTS CONFIGURATION
// ============================================================================

/// Explicit effects applied by an activity per second
#[derive(Debug, Clone, Reflect, Default)]
pub struct ActivityEffects {
    /// Physical Needs
    pub energy_change: f32, // +gain / -loss
    pub hunger_change: f32, // +increase (getting hungrier)
    pub thirst_change: f32, // +increase (getting thirstier)
    pub health_change: f32, // +healing / -damage

    /// Consciousness
    pub alertness_change: f32, // +waking up / -falling asleep

    /// Psychological Drives (Satisfiers)
    /// Negative means satisfying the drive (reducing the need/deficit)
    /// Positive means increasing the need
    pub social_change: f32,
    pub fun_change: f32,
    pub curiosity_change: f32,

    /// Emotions
    /// Triggers joy, etc.
    pub emotion_changes: Vec<(EmotionType, f32)>,
}

/// Configuration for a single activity type
#[derive(Debug, Clone, Reflect, Default)]
pub struct ActivityTypeConfig {
    /// Name for debugging
    pub name: String,
    /// Effects applied per second while in this activity
    pub effects: ActivityEffects,
}

/// Central configuration for all activities
#[derive(Resource, Debug, Clone, Reflect)]
#[reflect(Resource)]
pub struct ActivityConfig {
    pub base: ActivityTypeConfig,
    pub idle: ActivityTypeConfig,
    pub wandering: ActivityTypeConfig,
    pub sleeping: ActivityTypeConfig,
    pub eating: ActivityTypeConfig,
    pub harvesting: ActivityTypeConfig,
    pub moving: ActivityTypeConfig,
    pub wake_up: ActivityTypeConfig,
    pub exploring: ActivityTypeConfig,
}

impl Default for ActivityConfig {
    fn default() -> Self {
        Self {
            base: ActivityTypeConfig {
                name: "Base Metabolism".to_string(),
                effects: ActivityEffects {
                    energy_change: -0.15,
                    hunger_change: 0.5,
                    ..Default::default()
                },
            },
            idle: ActivityTypeConfig {
                name: "Idle".to_string(),
                effects: ActivityEffects {
                    alertness_change: 5.0, // Slowly waking up / staying awake
                    // Relaxing reduces stress? (Stress is emotion-derived now, maybe implicit)
                    ..Default::default()
                },
            },
            wandering: ActivityTypeConfig {
                name: "Wander".to_string(),
                effects: ActivityEffects {
                    energy_change: -0.2,
                    hunger_change: 2.0,
                    ..Default::default()
                },
            },
            sleeping: ActivityTypeConfig {
                name: "Sleeping".to_string(),
                effects: ActivityEffects {
                    energy_change: 20.0,
                    hunger_change: 0.2,      // Low hunger rate
                    alertness_change: -50.0, // Fall asleep hard
                    emotion_changes: vec![(EmotionType::Joy, 2.0)], // Comfort
                    ..Default::default()
                },
            },
            eating: ActivityTypeConfig {
                name: "Eating".to_string(),
                effects: ActivityEffects {
                    // Hunger reduction is handled by Action logic mostly,
                    // but could be here too. Action logic is discrete, this is continuous.
                    // Eating takes time, so maybe?
                    // For now let's keep it minimal here.
                    emotion_changes: vec![(EmotionType::Joy, 5.0)],
                    ..Default::default()
                },
            },
            harvesting: ActivityTypeConfig {
                name: "Harvesting".to_string(),
                effects: ActivityEffects {
                    energy_change: -0.2,
                    hunger_change: 2.0,
                    ..Default::default()
                },
            },
            moving: ActivityTypeConfig {
                name: "Moving".to_string(),
                effects: ActivityEffects {
                    energy_change: -0.3,
                    alertness_change: 10.0,
                    ..Default::default()
                },
            },
            wake_up: ActivityTypeConfig {
                name: "Waking Up".to_string(),
                effects: ActivityEffects {
                    alertness_change: 100.0,
                    ..Default::default()
                },
            },
            exploring: ActivityTypeConfig {
                name: "Exploring".to_string(),
                effects: ActivityEffects {
                    energy_change: -0.25,
                    hunger_change: 2.5,
                    alertness_change: 5.0,
                    ..Default::default()
                },
            },
        }
    }
}

impl ActivityConfig {
    pub fn get(&self, activity: &CurrentActivity) -> &ActivityTypeConfig {
        match activity {
            CurrentActivity::Idle => &self.idle,
            CurrentActivity::Wandering => &self.wandering,
            CurrentActivity::Sleeping => &self.sleeping,
            CurrentActivity::Eating(_) => &self.eating,
            CurrentActivity::Harvesting(_, _) => &self.harvesting,
            CurrentActivity::MovingTo(_) => &self.moving,
            CurrentActivity::WakeUp => &self.wake_up,
            CurrentActivity::Exploring(_) => &self.exploring,
        }
    }
}
