//! Explore action - movement to find resources.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::motor::{
    ActionPrimitive, Behavior, IntensityPolicy, Intent, TargetSelector,
};
use crate::agent::actions::registry::{Action, ActionKind, LegCompleteContext, LegResult};
use bevy::prelude::Vec2;

pub struct ExploreAction;

impl Action for ExploreAction {
    fn action_type(&self) -> ActionType {
        ActionType::Explore
    }

    fn name(&self) -> &'static str {
        "Explore"
    }

    fn default_behavior(&self) -> Behavior {
        Behavior::new(
            ActionPrimitive::Locomote,
            TargetSelector::UnknownArea,
            IntensityPolicy::Normal,
            Intent::Curiosity,
        )
    }

    fn kind(&self) -> ActionKind {
        // Ambient: goal-directed movement that doesn't self-complete.
        // When one leg arrives without finding anything new, the
        // execution loop picks a new unknown-area target and keeps
        // going. Stops via preemption (a real plan wins).
        ActionKind::Ambient
    }

    fn cost(&self) -> f32 {
        3.0
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(Channel::Locomotion, 0.4),
            ChannelUsage::new(Channel::Focus, 0.15),
            ChannelUsage::new(Channel::Awareness, 0.2),
        ];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        Some(Posture::Moving)
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("exploring for resources")
    }

    fn on_leg_complete(&self, ctx: &mut LegCompleteContext) -> LegResult {
        // Pick a random walkable tile from the full map. Same sampling
        // pattern used by start_actions' initial target selection,
        // without MindGraph-aware scoring (future improvement).
        use rand::Rng;
        let (map_w, map_h) = ctx.world_map.pixel_bounds();
        for _ in 0..10 {
            let test_pos = Vec2::new(
                ctx.rng.random_range(0.0..map_w),
                ctx.rng.random_range(0.0..map_h),
            );
            if ctx.world_map.is_walkable(test_pos) {
                return LegResult::NextLeg(test_pos);
            }
        }
        LegResult::Complete
    }
}
