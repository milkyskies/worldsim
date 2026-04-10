//! Deposit action — transfer items from the agent's slots into a target
//! entity's slots.
//!
//! Polymorphic across construction sites, chests, furnaces, and other agents.
//! The polymorphism lives in `ItemSlots`: the action just walks the target's
//! slots, finds one that accepts an item the agent has, and transfers it.
//! `SlotFilter` and `Access` rules on the target decide what's possible.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, CompletionContext, RuntimeEffects, TargetCandidate,
    TargetSource,
};
use crate::agent::brains::thinking::TriplePattern;
use crate::agent::events::FailureReason;
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Triple, Value};
use crate::constants::actions::deposit::{DURATION_TICKS, HUNGER_PER_SEC, STAMINA_PER_SEC};

pub struct DepositAction;

impl Action for DepositAction {
    fn action_type(&self) -> ActionType {
        ActionType::Deposit
    }

    fn name(&self) -> &'static str {
        "Deposit"
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed {
            duration_ticks: DURATION_TICKS,
        }
    }

    fn target_source(&self) -> TargetSource {
        TargetSource::EntityAffordance
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Manipulation, 0.4)];
        CHANNELS
    }

    fn cost(&self) -> f32 {
        2.0
    }

    /// Generic precondition: the agent must have something to deposit.
    /// The `to_template` override binds the target tile and a target-specific
    /// content requirement when a real target entity is supplied.
    fn preconditions(&self) -> Vec<TriplePattern> {
        vec![TriplePattern::new(
            Some(Node::Self_),
            Some(Predicate::Contains),
            None,
        )]
    }

    /// Generic plan effect: the target ends up containing more.
    /// Per-target specialization happens in `plan_effects_for_target`.
    fn plan_effects(&self) -> Vec<Triple> {
        vec![]
    }

    /// Depositing destroys items from the agent's inventory — declare it
    /// so the planner doesn't double-count the same wood across two plan steps.
    fn plan_consumes(&self) -> Vec<TriplePattern> {
        vec![TriplePattern::new(
            Some(Node::Self_),
            Some(Predicate::Contains),
            None,
        )]
    }

    /// Plan-time view of "what depositing into this target accomplishes":
    /// for each item the target already accepts (has Construction slots for,
    /// expressed via the recipe's `Requires` triples on the target's `Becomes`
    /// concept), produce a `(target, Contains, Item)` effect so the planner
    /// can chain Harvest → Walk → Deposit toward a build goal.
    ///
    /// Falls back to a generic empty effect for targets the agent has no
    /// slot-shape beliefs about — runtime still works, the planner just
    /// can't reason about it.
    fn plan_effects_for_target(&self, target: &TargetCandidate, mind: &MindGraph) -> Vec<Triple> {
        let Some(entity) = target.as_entity() else {
            return self.plan_effects();
        };

        // What does this target want to become? (#61 Becomes belief triple)
        let becomes = mind.query(Some(&Node::Entity(entity)), Some(Predicate::Becomes), None);

        let recipe_concepts: Vec<Concept> = becomes
            .iter()
            .filter_map(|t| {
                if let Value::Concept(c) = t.object {
                    Some(c)
                } else {
                    None
                }
            })
            .collect();

        // For each recipe concept, look up what materials it requires.
        // Effect: depositing into the target makes it contain those materials.
        let mut effects = Vec::new();
        for recipe in recipe_concepts {
            let requirements = mind.query(
                Some(&Node::Concept(recipe)),
                Some(Predicate::Requires),
                None,
            );
            for req in requirements {
                if let Value::Item(material, qty) = req.object {
                    effects.push(Triple::new(
                        Node::Entity(entity),
                        Predicate::Contains,
                        Value::Item(material, qty),
                    ));
                }
            }
        }

        effects
    }

    fn is_plan_valid(&self, target: &TargetCandidate, mind: &MindGraph) -> bool {
        // Valid if the target is known to want something (e.g. a Becomes triple
        // exists pointing at a recipe). Without that the planner has no way to
        // chain through this action sensibly.
        let Some(entity) = target.as_entity() else {
            return false;
        };
        !mind
            .query(Some(&Node::Entity(entity)), Some(Predicate::Becomes), None)
            .is_empty()
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            stamina_per_sec: STAMINA_PER_SEC,
            hunger_per_sec: HUNGER_PER_SEC,
            ..Default::default()
        }
    }

    fn can_start(&self, ctx: &ActionContext) -> Result<(), FailureReason> {
        if ctx.target_entity.is_none() {
            return Err(FailureReason::TargetGone);
        }
        if ctx.inventory.all_items().next().is_none() {
            return Err(FailureReason::MissingMaterials);
        }
        Ok(())
    }

    /// Transfer items from agent inventory into the target's first accepting
    /// slot, preserving per-instance properties (freshness, quality, provenance).
    ///
    /// Finds the first concept the target accepts, then transfers as many Things
    /// of that concept as the target slot will hold (respecting its capacity).
    fn on_complete(&self, ctx: &mut CompletionContext) {
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
                // Slot full or rejected — put it back and stop.
                ctx.inventory.add_thing(thing);
                break;
            }
        }
    }

    fn complete_log(&self) -> Option<&'static str> {
        Some("deposited")
    }
}
