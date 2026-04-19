//! Wander action — random movement.
//!
//! Ambient: never self-completes. On arrival at a random nearby tile the
//! custom `on_leg_complete` picker chains a new target, which kills the
//! 1-tick Wander → Idle → Wander oscillation the old Movement-kind shape
//! used to fire.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, LegCompleteContext, LegResult, TargetSource};
use bevy::prelude::Vec2;

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Locomotion, 0.4),
    ChannelUsage::new(Channel::Awareness, 0.15),
];

pub static WANDER_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Wander,
    name: "Wander",
    kind: ActionKind::Ambient,
    target_source: TargetSource::None,
    base_cost: 5.0,
    primitive: ActionPrimitive::Locomote,
    target_selector: TargetSelector::RandomNearby,
    intensity: IntensityPolicy::Ambient,
    intent: Intent::Curiosity,
    body_channels: CHANNELS,
    posture: Some(Posture::Moving),
    interruptible: true,
    start_log: None,
    complete_log: None,
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks {
        on_leg_complete: Some(wander_on_leg_complete),
        ..Hooks::EMPTY
    },
    recipe: None,
};

fn wander_on_leg_complete(ctx: &mut LegCompleteContext) -> LegResult {
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
