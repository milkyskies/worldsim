//! Wander action - random movement.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::motor::{
    ActionPrimitive, Behavior, IntensityPolicy, Intent, TargetSelector,
};
use crate::agent::actions::registry::{Action, ActionKind, LegCompleteContext, LegResult};
use bevy::prelude::Vec2;

pub struct WanderAction;

impl Action for WanderAction {
    fn action_type(&self) -> ActionType {
        ActionType::Wander
    }

    fn name(&self) -> &'static str {
        "Wander"
    }

    fn default_behavior(&self) -> Behavior {
        Behavior::new(
            ActionPrimitive::Locomote,
            TargetSelector::RandomNearby,
            IntensityPolicy::Ambient,
            Intent::Curiosity,
        )
    }

    fn kind(&self) -> ActionKind {
        // Ambient: never self-completes. On arrival at a random nearby
        // tile, the execution loop picks a new target in place and keeps
        // going. This kills the 1-tick Wander→Idle→Wander oscillation
        // that used to fire every time a Wander leg completed.
        ActionKind::Ambient
    }

    fn cost(&self) -> f32 {
        5.0
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(Channel::Locomotion, 0.4),
            ChannelUsage::new(Channel::Awareness, 0.15),
        ];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        Some(Posture::Moving)
    }

    fn on_leg_complete(&self, ctx: &mut LegCompleteContext) -> LegResult {
        // Pick a new random walkable tile 10-30 units away and keep
        // wandering. Returning Complete here would let the action
        // terminate (Ambient would fall back to a default reselect
        // anyway, but doing it explicitly makes the intent readable).
        use rand::Rng;
        let base_angle: f32 = ctx.rng.random_range(0.0..std::f32::consts::TAU);
        let dist: f32 = ctx.rng.random_range(10.0..30.0);
        for i in 0..8 {
            let angle = base_angle + (i as f32 * std::f32::consts::TAU / 8.0);
            let test_pos = ctx.agent_position + Vec2::new(angle.cos(), angle.sin()) * dist;
            if ctx.world_map.in_bounds(test_pos) && ctx.world_map.is_walkable(test_pos) {
                return LegResult::NextLeg(test_pos);
            }
        }
        LegResult::Complete
    }
}
