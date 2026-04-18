//! Eat action - consume food from inventory.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::motor::{
    ActionPrimitive, Behavior, IntensityPolicy, Intent, TargetSelector,
};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, CompletionContext, RuntimeEffects,
};
use crate::agent::body::metabolism::{FoodMacros, food_macros};
use crate::agent::brains::thinking::TriplePattern;
use crate::agent::events::FailureReason;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::{Concept, Node, Predicate, Quantity, Triple, Value};
use crate::constants::actions::eat::{DURATION_TICKS, STAMINA_GAIN};

/// Fallback macros for edible items that don't yet have an entry in the
/// `food_macros` lookup table. Tuned to match the legacy "eat grants 50
/// hunger reduction" feel — 30 carbs + 10 fat is a medium meal that digests
/// into a full glucose top-up plus a small reserve contribution.
const FALLBACK_MEAL: FoodMacros = FoodMacros::new(30.0, 10.0);

pub struct EatAction;

impl Action for EatAction {
    fn action_type(&self) -> ActionType {
        ActionType::Eat
    }

    fn name(&self) -> &'static str {
        "Eat"
    }

    fn default_behavior(&self) -> Behavior {
        Behavior::new(
            ActionPrimitive::Ingest,
            TargetSelector::InPlace,
            IntensityPolicy::Fixed(0.0),
            Intent::Hunger,
        )
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed {
            duration_ticks: DURATION_TICKS,
        }
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        // Consumption only — animals eat mouth-first. Humans may nominally
        // use a hand to bring food to their face but the gate is the mouth.
        const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Consumption, 0.8)];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        // Posture-agnostic. Munching a berry while walking is normal —
        // humans snack on the move, deer nibble a fruit mid-stride, a
        // wolf chews a bite of meat while trotting. Graze is a separate
        // fused walk-and-eat for grass tiles specifically (continuous
        // consumption of a terrain resource), not the general "I have
        // food in hand" eating path.
        None
    }

    // Planning: Need to have food to eat
    fn preconditions(&self) -> Vec<TriplePattern> {
        // isa_filter = Food prevents the planner from chaining "harvest stone → eat".
        vec![TriplePattern::self_contains_food()]
    }

    // Planning: After eating, hunger is satisfied
    fn plan_effects(&self) -> Vec<Triple> {
        vec![Triple::new(
            Node::Self_,
            Predicate::Hunger,
            Value::Quantity(Quantity::Exact(0.0)),
        )]
    }

    // Execution: Actually check if we have edible food (not just any item)
    fn can_start(&self, ctx: &ActionContext) -> Result<(), FailureReason> {
        if ctx
            .inventory
            .all_items()
            .any(|item| ctx.mind.is_a(&Node::Concept(item.concept), Concept::Food))
        {
            Ok(())
        } else {
            Err(FailureReason::NoEdibleFood)
        }
    }

    /// Block new Eat starts when the stomach is above the satiation
    /// threshold OR the next food item in inventory wouldn't fit in
    /// current headroom. The bite-aware arm prevents chain-firing Eat
    /// completions that `Metabolism::eat` would silently reject: the
    /// action finishes, grants stamina, logs "ate food", but the berry
    /// stays in inventory because it can't physically go in.
    fn satiation(
        &self,
        physical: Option<&crate::agent::body::needs::PhysicalNeeds>,
        inventory: Option<&ItemSlots>,
    ) -> Option<(crate::agent::body::need::NeedKind, f32)> {
        let metabolism = &physical?.metabolism;
        if let Some(inv) = inventory
            && let Some(macros) = inv.all_items().find_map(|t| food_macros(t.concept))
            && !metabolism.would_fit(macros)
        {
            return Some((crate::agent::body::need::NeedKind::Hunger, 1.0));
        }
        Some((
            crate::agent::body::need::NeedKind::Hunger,
            metabolism.stomach_fraction(),
        ))
    }

    // Execution: What happens when we finish eating
    fn on_complete(&self, ctx: &mut CompletionContext) {
        // Pick the first food item (IsA Food) from inventory.
        // Unknown edibles fall back to a generic meal so the action always
        // produces *some* satiety (prevents silently eating junk that does nothing).
        let concept = ctx
            .inventory
            .all_items()
            .find(|item| ctx.mind.is_a(&Node::Concept(item.concept), Concept::Food))
            .map(|t| t.concept);
        if let Some(concept) = concept {
            let macros = food_macros(concept).unwrap_or(FALLBACK_MEAL);
            // Only consume the inventory item if the metabolism actually
            // accepted the food. A full stomach returns false from
            // `eat()` and the item stays put, ready for when the agent
            // has digested enough to make room. Without this guard a
            // hungry-but-already-full agent silently threw away one
            // berry per Eat tick (#416).
            if ctx.physical.metabolism.eat(macros) {
                ctx.inventory.remove(concept, 1);
            }
        }

        // Meals still grant a small stamina boost (fast glucose bolt).
        ctx.physical.stamina.adjust_aerobic(STAMINA_GAIN);
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            joy_per_sec: 5.0,
            ..Default::default()
        }
    }

    fn complete_log(&self) -> Option<&'static str> {
        Some("ate food")
    }
}
