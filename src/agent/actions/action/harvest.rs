//! Harvest action - gather resources from targets.

use crate::agent::actions::ActionType;
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, CompletionContext, RuntimeEffects,
};
use crate::agent::brains::thinking::{ActionTemplate, TriplePattern};
use crate::agent::events::FailureReason;
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Triple, Value};
use bevy::prelude::*;

pub struct HarvestAction;

impl Action for HarvestAction {
    fn action_type(&self) -> ActionType {
        ActionType::Harvest
    }

    fn name(&self) -> &'static str {
        "Harvest"
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed { duration_ticks: 30 }
    }

    // Planning: Need to be at location and target must have items
    // Note: Actual preconditions are bound dynamically with target entity
    fn preconditions(&self) -> Vec<TriplePattern> {
        vec![]
    }

    // Planning: After harvesting, we have food
    fn plan_effects(&self) -> Vec<Triple> {
        vec![Triple::new(
            Node::Self_,
            Predicate::Contains,
            Value::Item(Concept::Apple, 1),
        )]
    }

    fn cost(&self) -> f32 {
        2.0
    }

    fn target_type(&self) -> crate::agent::actions::registry::TargetType {
        crate::agent::actions::registry::TargetType::Entity
    }

    fn requires_proximity(&self) -> bool {
        true // Must be at target location to harvest
    }

    // Execution: Must have a target entity
    fn can_start(&self, ctx: &ActionContext) -> Result<(), FailureReason> {
        if ctx.target_entity.is_some() {
            Ok(())
        } else {
            Err(FailureReason::TargetGone)
        }
    }

    // Planning: Only valid if we KNOW it produces something useful
    fn is_plan_valid(&self, target: Option<Entity>, mind: &MindGraph) -> bool {
        let Some(target_entity) = target else {
            return false;
        };

        // 1. Do we know it produces anything?
        // Query: (Target, Produces, ?Item)
        let produced_items = mind.query(
            Some(&Node::Entity(target_entity)),
            Some(Predicate::Produces),
            None,
        );

        if produced_items.is_empty() {
            return false; // Don't know it produces anything
        }

        // 2. Is any produced item useful (Food or Resource)?
        // We verify if the produced Concept IsA Food or Resource
        for triple in produced_items {
            if let Value::Item(concept, _) = triple.object {
                if mind.is_a(&Node::Concept(concept), Concept::Food)
                    || mind.is_a(&Node::Concept(concept), Concept::Resource)
                {
                    return true;
                }
            }
        }

        false
    }

    // Per-tick effects while harvesting
    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            energy_per_sec: -0.2,
            hunger_per_sec: 2.0,
            ..Default::default()
        }
    }

    // Execution: What happens when harvest completes
    fn on_complete(&self, ctx: &mut CompletionContext) {
        // Transfer item from target's inventory to agent's inventory
        // No hardcoding - takes whatever the target actually has!
        if let Some(target_inv) = &mut ctx.target_inventory
            && let Some(item) = target_inv.items.iter().find(|i| i.quantity > 0)
        {
            let concept = item.concept;
            target_inv.remove(concept, 1);
            ctx.inventory.add(concept, 1);
        }
    }

    fn complete_log(&self) -> Option<&'static str> {
        Some("harvested")
    }

    fn to_template(
        &self,
        target_entity: Option<Entity>,
        target_position: Option<Vec2>,
    ) -> ActionTemplate {
        // Use default template (which adds proximity precondition automatically)
        let mut template = ActionTemplate {
            name: self.name().to_string(),
            action_type: self.action_type(),
            target_entity,
            target_position,
            preconditions: self.preconditions(),
            effects: self.plan_effects(),
            base_cost: 10.0,
            topic: None,
            content: Vec::new(),
        };

        // Add location requirement (from requires_proximity)
        if let Some(pos) = target_position {
            const TILE_SIZE: f32 = 16.0;
            let tile = (
                (pos.x / TILE_SIZE).floor() as i32,
                (pos.y / TILE_SIZE).floor() as i32,
            );
            template.preconditions.push(TriplePattern::self_at(tile));
        }

        // Add content requirement (Harvest-specific)
        if let Some(entity) = target_entity {
            template
                .preconditions
                .push(TriplePattern::entity_contains(entity));
        }

        template
    }
}
