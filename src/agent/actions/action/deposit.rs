//! Deposit action — transfer items from agent slots into a target's slots.
//!
//! Polymorphic across construction sites, chests, furnaces, and other agents.
//! The target's `SlotFilter` and `Access` rules decide what's possible. Plan
//! effects derive from the target's recipe requirements via the
//! `FromTargetBecomesRequirements` projection.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Gate, Hooks, Pattern, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, CompletionContext, TargetSource};
use crate::constants::actions::deposit::DURATION_TICKS;

const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Manipulation, 0.4)];

pub static DEPOSIT_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Deposit,
    name: "Deposit",
    kind: ActionKind::Timed {
        duration_ticks: DURATION_TICKS,
    },
    target_source: TargetSource::EntityAffordance,
    base_cost: 2.0,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Goal,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    interruptible: true,
    start_log: None,
    complete_log: Some("deposited"),
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[Pattern::SelfContainsAny],
    plan_effects: &[],
    plan_consumes: &[Pattern::SelfContainsAny],
    target_effects: TargetEffects::FromTargetBecomesRequirements,
    plan_validity: PlanValidity::TargetHasBecomes,
    gates: &[Gate::TargetEntityExists, Gate::InventoryNonEmpty],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks {
        on_complete: Some(deposit_on_complete),
        ..Hooks::EMPTY
    },
    recipe: None,
};

fn deposit_on_complete(ctx: &mut CompletionContext) {
    let Some(target_inv) = ctx.target_inventory.as_deref_mut() else {
        return;
    };
    let concept = ctx
        .inventory
        .all_items()
        .map(|t| t.concept)
        .find(|&c| target_inv.slots.iter().any(|s| s.can_deposit(c, 1, None)));
    let Some(concept) = concept else { return };

    while let Some(thing) = ctx.inventory.remove_thing(concept) {
        if !target_inv.deposit_thing(thing.clone(), None) {
            ctx.inventory.add_thing(thing);
            break;
        }
    }
}
