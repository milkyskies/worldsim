//! Share Food action — hand a food item to a nearby liked agent.
//!
//! Reads:  agent inventory (food), target inventory (drop slot), MindGraph
//!         affection belief toward target
//! Writes: agent inventory (one food item removed), target inventory (added)
//! Upstream: emotional/rational brain proposing share when affection is high
//!           and the recipient is hungry / nearby
//! Downstream: belief updater bumps trust and affection on both sides

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Gate, Hooks, Pattern, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, CompletionContext, TargetSource};
use crate::agent::mind::knowledge::{Concept, Node};
use crate::constants::actions::share_food::{DURATION_TICKS, MIN_AFFECTION};

const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Manipulation, 0.3)];

pub static SHARE_FOOD_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::ShareFood,
    kind: ActionKind::Timed {
        duration_ticks: DURATION_TICKS,
    },
    target_source: TargetSource::EntityAffordance,
    base_cost: 1.0,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Social,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    interruptible: true,
    start_log: Some("offering food"),
    complete_log: Some("shared food"),
    joy_per_sec: 1.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[Pattern::SelfContainsFood],
    plan_effects: &[],
    plan_consumes: &[Pattern::SelfContainsFood],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[
        Gate::TargetEntity(crate::agent::events::FailureReason::NoTarget),
        Gate::InventoryHasFood,
        Gate::TargetAffectionAtLeast(MIN_AFFECTION),
    ],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks {
        on_complete: Some(share_food_on_complete),
        ..Hooks::EMPTY
    },
    recipe: None,
};

/// Move a single food item from the giver's inventory into the recipient's,
/// preserving freshness metadata. Picks the first IsA-Food item the
/// recipient's slots will accept — falls through silently if no acceptable
/// concept exists, mirroring Deposit's slot-aware behavior.
fn share_food_on_complete(ctx: &mut CompletionContext) {
    let Some(target_inv) = ctx.target_inventory.as_deref_mut() else {
        return;
    };
    let concept: Option<Concept> = ctx.inventory.all_items().map(|t| t.concept).find(|&c| {
        ctx.mind.is_a(&Node::Concept(c), Concept::Food)
            && target_inv.slots.iter().any(|s| s.can_deposit(c, 1, None))
    });
    let Some(concept) = concept else { return };
    let Some(thing) = ctx.inventory.remove_thing(concept) else {
        return;
    };
    if !target_inv.deposit_thing(thing.clone(), None) {
        ctx.inventory.add_thing(thing);
    }
}
