//! Take action — transfer items from a target entity's slots into the
//! agent's own slots.
//!
//! Polymorphic across chests, dropped piles, furnace outputs, and any other
//! entity with `ItemSlots`. The polymorphism lives in `ItemSlots`: the action
//! walks the target's slots, finds the first item with `extract_access != None`,
//! and transfers it. Construction sites are explicitly NOT extractable —
//! their slots have `extract_access: None`, so Take silently skips them.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, CompletionContext, RuntimeEffects, TargetCandidate,
    TargetSource,
};
use crate::agent::brains::thinking::TriplePattern;
use crate::agent::events::FailureReason;
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Triple, Value};
use crate::constants::actions::take::{DURATION_TICKS, HUNGER_PER_SEC, STAMINA_PER_SEC};

pub struct TakeAction;

impl Action for TakeAction {
    fn action_type(&self) -> ActionType {
        ActionType::Take
    }

    fn name(&self) -> &'static str {
        "Take"
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed {
            duration_ticks: DURATION_TICKS,
        }
    }

    fn target_source(&self) -> TargetSource {
        TargetSource::EntityAffordance
    }

    /// Per-target consumed pattern: Take destroys items from the target so the
    /// planner doesn't double-count the same stack across two plan steps.
    fn target_consumes(&self, target: &TargetCandidate, _mind: &MindGraph) -> Vec<TriplePattern> {
        match target.as_entity() {
            Some(entity) => vec![TriplePattern::entity_contains(entity)],
            None => vec![],
        }
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Manipulation, 0.4)];
        CHANNELS
    }

    fn cost(&self) -> f32 {
        2.0
    }

    /// Plan-time view: agent gains whatever the target is known to contain.
    /// One effect per item the agent's mind says the target holds.
    fn plan_effects_for_target(&self, target: &TargetCandidate, mind: &MindGraph) -> Vec<Triple> {
        let Some(entity) = target.as_entity() else {
            return self.plan_effects();
        };

        mind.query(Some(&Node::Entity(entity)), Some(Predicate::Contains), None)
            .into_iter()
            .filter_map(|triple| {
                if let Value::Item(_, qty) = triple.object
                    && qty > 0
                {
                    Some(Triple::new(
                        Node::Self_,
                        Predicate::Contains,
                        triple.object.clone(),
                    ))
                } else {
                    None
                }
            })
            .collect()
    }

    fn is_plan_valid(&self, target: &TargetCandidate, mind: &MindGraph) -> bool {
        // Valid if the target is known to contain at least one item with a
        // non-zero quantity. Belief about extract_access lives on the world
        // entity, not the mind, so the runtime check filters sealed slots.
        let Some(entity) = target.as_entity() else {
            return false;
        };
        mind.query(Some(&Node::Entity(entity)), Some(Predicate::Contains), None)
            .iter()
            .any(|t| matches!(t.object, Value::Item(_, qty) if qty > 0))
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            stamina_per_sec: STAMINA_PER_SEC,
            hunger_per_sec: HUNGER_PER_SEC,
            ..Default::default()
        }
    }

    fn can_start(&self, ctx: &ActionContext) -> Result<(), FailureReason> {
        let Some(target_entity) = ctx.target_entity else {
            return Err(FailureReason::TargetGone);
        };

        // We can't read the target's actual ItemSlots from ActionContext (it
        // only has the agent's own inventory). The runtime check is "we have
        // a target". on_complete handles the case where the target turns out
        // to have nothing extractable by simply doing nothing.
        let _ = target_entity;
        Ok(())
    }

    /// Pull extractable items from the target into the agent's inventory,
    /// preserving per-instance properties (freshness, quality, provenance).
    ///
    /// Finds the first concept that can be extracted and transfers all items
    /// of that concept (mimicking the old stack-based "take the whole pile").
    fn on_complete(&self, ctx: &mut CompletionContext) {
        let Some(target_inv) = ctx.target_inventory.as_deref_mut() else {
            return;
        };

        // Collect extractable concepts first (snapshot avoids borrow conflict).
        let extractable: Vec<Concept> = target_inv
            .all_items()
            .filter(|t| target_inv.can_extract(t.concept))
            .map(|t| t.concept)
            .collect();

        let Some(&concept) = extractable.first() else {
            return;
        };

        // Transfer all Things of that concept.
        while let Some(thing) = target_inv.extract_thing(concept) {
            ctx.inventory.add_thing(thing);
        }
    }

    fn complete_log(&self) -> Option<&'static str> {
        Some("took")
    }
}
