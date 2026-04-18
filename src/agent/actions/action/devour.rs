//! Devour action — predator/scavenger feeding from a corpse.
//!
//! Tears one bite of meat off the target's `ItemSlots` per completion and
//! sends it straight to the agent's metabolism. No Harvest hop into a
//! personal inventory — the wolf is face-down in the carcass, swallowing
//! flesh, not pocketing it.
//!
//! Pack feeding emerges naturally: multiple wolves can run Devour against
//! the same corpse on the same tick, each completion decrementing the
//! shared `ItemSlots` by one.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::motor::{
    ActionPrimitive, Behavior, IntensityPolicy, Intent, TargetSelector,
};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, CompletionContext, RuntimeEffects, TargetCandidate,
    TargetSource,
};
use crate::agent::body::metabolism::{FALLBACK_MEAL, food_macros};
use crate::agent::events::FailureReason;
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Quantity, Triple, Value};
use crate::constants::actions::devour::{DURATION_TICKS, STAMINA_GAIN};

pub struct DevourAction;

impl Action for DevourAction {
    fn action_type(&self) -> ActionType {
        ActionType::Devour
    }

    fn name(&self) -> &'static str {
        "Devour"
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

    fn target_source(&self) -> TargetSource {
        // Carrion = corpses. The Dead-bearing variant of the trait
        // enumerator skips the alive-only filter, so dead-marked entities
        // (corpses) flow through instead of being filtered out the way
        // Bite/Attack want.
        TargetSource::DeadEntityWithTrait(Concept::Carrion)
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        // Jaws to tear, throat to swallow. Light Focus — the wolf is
        // committed to the carcass but still tracks pack-mates and
        // potential threats around it.
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(Channel::Bite, 0.5),
            ChannelUsage::new(Channel::Consumption, 0.8),
            ChannelUsage::new(Channel::Focus, 0.2),
        ];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        // Stationary — the wolf is standing over the corpse. Walking off
        // ends the feed; the planner can re-propose Devour on the next
        // approach.
        Some(Posture::Stationary)
    }

    fn plan_effects(&self) -> Vec<Triple> {
        // After devouring, hunger is satisfied. Same shape as Eat so the
        // GOAP planner can chain Walk → Devour against a hunger goal.
        vec![Triple::new(
            Node::Self_,
            Predicate::Hunger,
            Value::Quantity(Quantity::Exact(0.0)),
        )]
    }

    fn cost(&self) -> f32 {
        1.0
    }

    fn is_plan_valid(&self, target: &TargetCandidate, mind: &MindGraph) -> bool {
        let Some(entity) = target.as_entity() else {
            return false;
        };
        // The mind must have at least a hint that this corpse contains an
        // edible item. Perception writes `(corpse, Contains, Item(Meat, n))`
        // when the wolf sees the carcass; if that belief is missing or
        // exhausted, drop the candidate.
        mind.query(Some(&Node::Entity(entity)), Some(Predicate::Contains), None)
            .iter()
            .any(|t| match &t.object {
                Value::Item(concept, qty) => {
                    *qty > 0 && mind.is_a(&Node::Concept(*concept), Concept::Food)
                }
                _ => false,
            })
    }

    fn can_start(&self, ctx: &ActionContext) -> Result<(), FailureReason> {
        if ctx.target_entity.is_none() {
            return Err(FailureReason::NoTarget);
        }
        Ok(())
    }

    fn satiation(
        &self,
        physical: Option<&crate::agent::body::needs::PhysicalNeeds>,
        _inventory: Option<&crate::agent::item_slots::ItemSlots>,
    ) -> Option<(crate::agent::body::need::NeedKind, f32)> {
        // Mirror Eat's stomach gate: a full wolf stops devouring even if
        // there's still meat on the corpse. The Devour action doesn't
        // touch the wolf's own ItemSlots, so we only check the metabolism
        // headroom — no inventory side channel needed.
        let metabolism = &physical?.metabolism;
        Some((
            crate::agent::body::need::NeedKind::Hunger,
            metabolism.stomach_fraction(),
        ))
    }

    fn on_complete(&self, ctx: &mut CompletionContext) {
        let Some(target_inv) = &mut ctx.target_inventory else {
            return;
        };

        // Pick the first edible (IsA Food) item the corpse holds. In
        // practice this is Meat — but the lookup stays generic so any
        // future carrion-borne item (organs, bone marrow, etc.) flows
        // through without further plumbing.
        let concept = target_inv
            .all_items()
            .find(|item| ctx.mind.is_a(&Node::Concept(item.concept), Concept::Food))
            .map(|t| t.concept);
        let Some(concept) = concept else {
            return;
        };

        let macros = food_macros(concept).unwrap_or(FALLBACK_MEAL);

        // Only decrement the corpse if metabolism actually accepted the
        // bite. A full stomach returns false from `eat()` and the meat
        // stays on the carcass for later, mirroring how Eat protects
        // inventory items from silently disappearing into a full agent.
        if ctx.physical.metabolism.eat(macros) {
            target_inv.remove(concept, 1);
        }

        // A meal still grants a small stamina boost — fast glucose bolt.
        ctx.physical.stamina.adjust_aerobic(STAMINA_GAIN);
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            joy_per_sec: 5.0,
            ..Default::default()
        }
    }

    fn complete_log(&self) -> Option<&'static str> {
        Some("devoured a bite")
    }
}
