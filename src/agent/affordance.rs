use bevy::prelude::*;

use super::actions::ActionType;

#[derive(Component, Debug, Clone, Reflect, Default)]
#[reflect(Component)]
pub struct Affordance {
    pub action_type: ActionType,
    pub cost: f32,
    pub distance: f32, // Distance required to interact
    pub risk: f32,     // 0.0 to 1.0 (Probability of negative outcome / danger level)
}
