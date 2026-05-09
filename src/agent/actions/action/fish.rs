//! Fish action — wait at the water's edge for a catch.
//!
//! Reads:  agent position, world map (water-adjacency gate)
//! Writes: agent inventory (Fish item with freshness stamp), SimEvent::ActionCompleted
//! Upstream: rational brain proposing Fish for hunger when meat is far and water is near
//! Downstream: eat_on_complete (Fish → satiety via `food_macros`), perishable decay

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, CompletionContext, TargetSource};
use crate::agent::item_slots::{Thing, perishable_decay_rate};
use crate::agent::mind::knowledge::Concept;
use crate::constants::actions::fish::DURATION_TICKS;

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Manipulation, 0.6),
    ChannelUsage::new(Channel::Locomotion, 0.2),
];

pub static FISH_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Fish,
    kind: ActionKind::Timed {
        duration_ticks: DURATION_TICKS,
    },
    // Tile-targeted on `Drinkable` (water): the planner only considers
    // Fish when the agent knows a water tile. Without this, the cheaper
    // 2-step `Fish → Eat` path crowds out the 4-step apple chain even
    // for agents nowhere near water.
    target_source: TargetSource::TileWithTrait(Concept::Drinkable),
    base_cost: 2.0,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Hunger,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    interruptible: true,
    start_log: Some("started fishing"),
    complete_log: Some("caught a fish"),
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[EffectTemplate::SelfContains {
        concept: Concept::Fish,
        quantity: 1,
    }],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[Gate::AdjacentToWater],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks {
        on_complete: Some(fish_on_complete),
        ..Hooks::EMPTY
    },
    recipe: None,
};

fn fish_on_complete(ctx: &mut CompletionContext) {
    let thing = if perishable_decay_rate(Concept::Fish).is_some() {
        Thing::fresh(Concept::Fish, ctx.tick)
    } else {
        Thing::new(Concept::Fish)
    };
    ctx.inventory.add_thing(thing);
}
