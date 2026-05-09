//! Stand Watch action — nighttime sentinel posture near a campfire.
//!
//! Reads:  current sim hour, agent MindGraph (heat-emitter belief)
//! Writes: SimEvent::ActionStarted/Completed (no direct state mutation)
//! Upstream: rational brain proposing watch when night falls and the camp has a fire
//! Downstream: future visibility/threat-deterrence systems can key off active StandWatch

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Gate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::constants::actions::stand_watch::{NIGHT_END_HOUR, NIGHT_START_HOUR};

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Locomotion, 0.2),
    ChannelUsage::new(Channel::Awareness, 1.0),
];

pub static STAND_WATCH_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::StandWatch,
    kind: ActionKind::Timed {
        duration_ticks: u32::MAX,
    },
    target_source: TargetSource::None,
    base_cost: 1.5,
    primitive: ActionPrimitive::Observe,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.3),
    intent: Intent::Safety,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    interruptible: true,
    start_log: Some("standing watch"),
    complete_log: None,
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[
        Gate::Nighttime {
            start_hour: NIGHT_START_HOUR,
            end_hour: NIGHT_END_HOUR,
        },
        Gate::NearHeatEmitter,
    ],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks::EMPTY,
    recipe: None,
};
