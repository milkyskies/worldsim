//! Devour action — predator/scavenger feeding from a corpse.
//!
//! Tears one bite off the target's `ItemSlots` per completion and sends it
//! straight to the agent's metabolism. No Harvest hop into personal
//! inventory — the wolf is face-down in the carcass. Pack feeding emerges
//! naturally: multiple wolves Devour the same corpse, each completion
//! decrementing shared `ItemSlots`.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, PlanValidity,
    SatiationGate, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, CompletionContext, TargetSource};
use crate::agent::body::metabolism::{FALLBACK_MEAL, food_macros};
use crate::agent::mind::knowledge::{Concept, Node, Predicate};
use crate::constants::actions::devour::{DURATION_TICKS, STAMINA_GAIN};

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Bite, 0.5),
    ChannelUsage::new(Channel::Consumption, 0.8),
    ChannelUsage::new(Channel::Focus, 0.2),
];

pub static DEVOUR_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Devour,
    name: "Devour",
    kind: ActionKind::Timed {
        duration_ticks: DURATION_TICKS,
    },
    target_source: TargetSource::DeadEntityWithTrait(Concept::Carrion),
    base_cost: 1.0,
    primitive: ActionPrimitive::Ingest,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Hunger,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    interruptible: true,
    start_log: None,
    complete_log: Some("devoured a bite"),
    joy_per_sec: 5.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[EffectTemplate::SelfNeedExact {
        predicate: Predicate::Hunger,
        value: 0.0,
    }],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::TargetContainsEdible,
    gates: &[Gate::TargetEntityRequired],
    satiation: Some(SatiationGate::HungerStomach),
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks {
        on_complete: Some(devour_on_complete),
        ..Hooks::EMPTY
    },
    recipe: None,
};

fn devour_on_complete(ctx: &mut CompletionContext) {
    let Some(target_inv) = &mut ctx.target_inventory else {
        return;
    };
    let concept = target_inv
        .all_items()
        .find(|item| ctx.mind.is_a(&Node::Concept(item.concept), Concept::Food))
        .map(|t| t.concept);
    let Some(concept) = concept else {
        return;
    };
    let macros = food_macros(concept).unwrap_or(FALLBACK_MEAL);
    // Only decrement the corpse if metabolism accepted the bite.
    if ctx.physical.metabolism.eat(macros) {
        target_inv.remove(concept, 1);
    }
    ctx.physical.stamina.adjust_aerobic(STAMINA_GAIN);
}
