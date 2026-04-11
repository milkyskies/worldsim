//! Eat action - consume food from inventory.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage};
use crate::agent::actions::registry::{Action, ActionContext, ActionKind, CompletionContext};
use crate::agent::body::metabolism::{FoodMacros, food_macros};
use crate::agent::brains::thinking::TriplePattern;
use crate::agent::events::FailureReason;
use crate::agent::mind::knowledge::{Node, Predicate, Triple, Value};
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

    // Planning: Need to have food to eat
    fn preconditions(&self) -> Vec<TriplePattern> {
        vec![
            // Self must contain some edible item
            TriplePattern::self_contains(),
        ]
    }

    // Planning: After eating, hunger is satisfied
    fn plan_effects(&self) -> Vec<Triple> {
        vec![Triple::new(Node::Self_, Predicate::Hunger, Value::Int(0))]
    }

    // Execution: Actually check if we have edible food
    fn can_start(&self, ctx: &ActionContext) -> Result<(), FailureReason> {
        if ctx.inventory.all_items().next().is_some() {
            Ok(())
        } else {
            Err(FailureReason::NoEdibleFood)
        }
    }

    // Execution: What happens when we finish eating
    fn on_complete(&self, ctx: &mut CompletionContext) {
        // Identify what's being eaten and look up its macros. Unknown edibles
        // fall back to a generic meal so the action always produces *some*
        // satiety (prevents silently eating junk that does nothing).
        let concept = ctx.inventory.all_items().next().map(|t| t.concept);
        if let Some(concept) = concept {
            let macros = food_macros(concept).unwrap_or(FALLBACK_MEAL);
            ctx.physical.metabolism.eat(macros);
            ctx.inventory.remove(concept, 1);
        }

        // Meals still grant a small stamina boost (fast glucose bolt).
        ctx.physical.stamina.adjust_aerobic(STAMINA_GAIN);
    }

    fn complete_log(&self) -> Option<&'static str> {
        Some("ate food")
    }
}
