//! Eat action — consume food from inventory.
//!
//! Custom `on_complete`: the metabolism's `eat()` call returns false when
//! the stomach is full. The inventory item only comes out if the metabolism
//! actually accepted the food; otherwise the berry stays put and can be
//! eaten once digestion makes room (#416).

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, Pattern, PlanValidity,
    SatiationGate, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, CompletionContext, TargetSource};
use crate::agent::body::metabolism::{FALLBACK_MEAL, food_macros};
use crate::agent::mind::knowledge::{Concept, Node, Predicate};
use crate::constants::actions::eat::{DURATION_TICKS, STAMINA_GAIN};

const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Consumption, 0.8)];

pub static EAT_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Eat,
    name: "Eat",
    kind: ActionKind::Timed {
        duration_ticks: DURATION_TICKS,
    },
    target_source: TargetSource::None,
    base_cost: 1.0,
    primitive: ActionPrimitive::Ingest,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Hunger,
    body_channels: CHANNELS,
    // Posture-agnostic: humans snack on the move, deer nibble mid-stride.
    posture: None,
    interruptible: true,
    start_log: None,
    complete_log: Some("ate food"),
    joy_per_sec: 5.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[Pattern::SelfContainsFood],
    plan_effects: &[EffectTemplate::SelfNeedExact {
        predicate: Predicate::Hunger,
        value: 0.0,
    }],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[Gate::InventoryHasFood],
    satiation: Some(SatiationGate::EatStomach),
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks {
        on_complete: Some(eat_on_complete),
        ..Hooks::EMPTY
    },
    recipe: None,
};

fn eat_on_complete(ctx: &mut CompletionContext) {
    // Pick the first food item (IsA Food) from inventory. Unknown edibles
    // fall back to a generic meal so the action always produces some satiety.
    let concept = ctx
        .inventory
        .all_items()
        .find(|item| ctx.mind.is_a(&Node::Concept(item.concept), Concept::Food))
        .map(|t| t.concept);
    if let Some(concept) = concept {
        let macros = food_macros(concept).unwrap_or(FALLBACK_MEAL);
        // Only consume the inventory item if metabolism actually accepted
        // the food (#416).
        if ctx.physical.metabolism.eat(macros) {
            ctx.inventory.remove(concept, 1);
        }
    }
    ctx.physical.stamina.adjust_aerobic(STAMINA_GAIN);
}
