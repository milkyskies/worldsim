//! Eat action - consume food from inventory.

use crate::agent::actions::ActionType;
use crate::agent::actions::registry::{Action, ActionContext, ActionKind, CompletionContext};
use crate::agent::brains::thinking::TriplePattern;
use crate::agent::events::FailureReason;
use crate::agent::mind::knowledge::{Node, Predicate, Triple, Value};

pub struct EatAction;

impl Action for EatAction {
    fn action_type(&self) -> ActionType {
        ActionType::Eat
    }

    fn name(&self) -> &'static str {
        "Eat"
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed { duration_ticks: 20 }
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
        if ctx.inventory.items.iter().any(|item| item.quantity > 0) {
            Ok(())
        } else {
            Err(FailureReason::NoEdibleFood)
        }
    }

    // Execution: What happens when we finish eating
    fn on_complete(&self, ctx: &mut CompletionContext) {
        // Reduce hunger
        ctx.physical.hunger = (ctx.physical.hunger - 50.0).max(0.0);

        // Gain energy
        ctx.physical.energy = (ctx.physical.energy + 10.0).min(100.0);

        // Consume first edible item from inventory
        if let Some(item) = ctx.inventory.items.iter().find(|i| i.quantity > 0) {
            let concept = item.concept;
            ctx.inventory.remove(concept, 1);
        }
    }

    fn complete_log(&self) -> Option<&'static str> {
        Some("ate food")
    }
}
