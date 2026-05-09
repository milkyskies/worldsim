//! StockChest action — drive satisfier for `FoodSecurity`. Targets a
//! chest via `EntityIsAConcept` because the chest's single `Affordance`
//! slot is already claimed by `Take`.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, Pattern, PlanValidity,
    TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, CompletionContext, TargetSource};
use crate::agent::events::FailureReason;
use crate::agent::mind::knowledge::{Concept, Predicate};
use crate::constants::actions::stock_chest::DURATION_TICKS;

const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Manipulation, 0.4)];

pub static STOCK_CHEST_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::StockChest,
    kind: ActionKind::Timed {
        duration_ticks: DURATION_TICKS,
    },
    target_source: TargetSource::EntityIsAConcept(Concept::StorageChest),
    base_cost: 2.0,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Goal,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    interruptible: true,
    start_log: Some("started stocking the chest"),
    complete_log: Some("stocked the chest"),
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[
        Pattern::SelfContainsFood,
        Pattern::SelfNearConcept(Concept::StorageChest),
    ],
    plan_effects: &[EffectTemplate::SelfNeedExact {
        predicate: Predicate::FoodSecurity,
        value: 100.0,
    }],
    plan_consumes: &[Pattern::SelfContainsFood],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[
        Gate::TargetEntity(FailureReason::TargetGone),
        Gate::InventoryHasFood,
    ],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks {
        on_complete: Some(stock_chest_on_complete),
        ..Hooks::EMPTY
    },
    recipe: None,
};

fn stock_chest_on_complete(ctx: &mut CompletionContext) {
    let Some(target_inv) = ctx.target_inventory.as_deref_mut() else {
        return;
    };
    let edible: Option<Concept> = ctx
        .inventory
        .all_items()
        .map(|t| t.concept)
        .find(|c| ctx.mind.ontology.has_trait(*c, Concept::Edible));
    let Some(concept) = edible else { return };

    ctx.inventory
        .drain_concept_into(target_inv, concept, Some(&ctx.mind.ontology));
}
