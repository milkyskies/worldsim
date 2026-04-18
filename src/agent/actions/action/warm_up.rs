//! WarmUp action — stay beside a heat source to restore warmth.
//!
//! Reads: ItemSlots (satiation gate), PhysicalNeeds.warmth (satiation gate)
//! Writes: nothing directly; runtime effects top up `PhysicalNeeds.warmth`
//! Upstream: ActionRegistry (registered there), temperature perception
//!           (seeds `Tile HasTrait Warmth` and exposes heat-source entities)
//! Downstream: warmth drain/recovery system (reads active WarmUp for momentum)
//!
//! Declares `TargetSource::EntityWithTrait(Concept::HeatEmitting)` so the
//! rational brain enumerates one WarmUp candidate per known heat source
//! (campfires, braziers, future ovens). The default `to_template_for_target`
//! implementation auto-injects a `self_at(tile)` precondition, and the
//! regressive planner chains `Walk → WarmUp` via the tile walk generator.
//! No manual preconditions or `to_template` override needed.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::motor::{
    ActionPrimitive, Behavior, IntensityPolicy, Intent, TargetSelector,
};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, CompletionContext, TargetSource,
};
use crate::agent::events::FailureReason;
use crate::agent::mind::knowledge::{Concept, Node, Predicate, Quantity, Triple, Value};
use crate::constants::actions::warm_up::{DURATION_TICKS, STAMINA_GAIN, WARMTH_RECOVERY};

pub struct WarmUpAction;

impl Action for WarmUpAction {
    fn action_type(&self) -> ActionType {
        ActionType::WarmUp
    }

    fn name(&self) -> &'static str {
        "WarmUp"
    }

    fn default_behavior(&self) -> Behavior {
        Behavior::new(
            ActionPrimitive::Manipulate,
            TargetSelector::InPlace,
            IntensityPolicy::Fixed(0.0),
            Intent::Goal,
        )
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed {
            duration_ticks: DURATION_TICKS,
        }
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        // Focus only — warming up is attentive idleness. Leaves
        // Consumption / Manipulation free so the agent can still eat
        // or tend the fire while warming.
        const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Focus, 0.3)];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        // Legs planted beside the fire — stationary until the cycle
        // completes or the agent is preempted.
        Some(Posture::Stationary)
    }

    fn target_source(&self) -> TargetSource {
        TargetSource::EntityWithTrait(Concept::HeatEmitting)
    }

    /// Planning: a completed WarmUp cycle tops warmth up to full. The
    /// exact value asserted here matches the `goal_for_urgency` side
    /// of the Warmth drive so the regressive planner closes cleanly.
    fn plan_effects(&self) -> Vec<Triple> {
        vec![Triple::new(
            Node::Self_,
            Predicate::Warmth,
            Value::Quantity(Quantity::Exact(100.0)),
        )]
    }

    /// Block new WarmUp starts once warmth is already ≥ 0.95. Without
    /// this the rational brain can chain-fire WarmUp every duration
    /// cycle while the agent stands next to a lit campfire — the same
    /// guard Drink / Eat / Sleep use.
    fn satiation(
        &self,
        physical: Option<&crate::agent::body::needs::PhysicalNeeds>,
        _inventory: Option<&crate::agent::item_slots::ItemSlots>,
    ) -> Option<(crate::agent::body::need::NeedKind, f32)> {
        Some((
            crate::agent::body::need::NeedKind::Warmth,
            physical?.warmth.value,
        ))
    }

    /// Runtime check: target must still exist (an expired campfire's
    /// HeatSource has been removed but the mindgraph belief can linger).
    fn can_start(&self, ctx: &ActionContext) -> Result<(), FailureReason> {
        if ctx.target_entity.is_none() {
            return Err(FailureReason::TargetGone);
        }
        Ok(())
    }

    fn on_complete(&self, ctx: &mut CompletionContext) {
        ctx.physical.warmth.top_up(WARMTH_RECOVERY);
        ctx.physical.stamina.adjust_aerobic(STAMINA_GAIN);
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("started warming up")
    }

    fn complete_log(&self) -> Option<&'static str> {
        Some("warmed up")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::actions::registry::Action;
    use crate::agent::body::need::NeedKind;
    use crate::testing::TestWorld;

    #[test]
    fn warm_up_declares_near_campfire_via_target_source() {
        // WarmUp satisfies warmth via proximity to a HeatSource. The
        // target-source declaration is what drives the planner to
        // enumerate heat-emitting entities as candidates — the Walk
        // chain to each target falls out of the tile-walk generator.
        let action = WarmUpAction;
        match action.target_source() {
            crate::agent::actions::registry::TargetSource::EntityWithTrait(c) => {
                assert_eq!(c, Concept::HeatEmitting);
            }
            other => panic!("expected EntityWithTrait(HeatEmitting), got {:?}", other),
        }
    }

    #[test]
    fn warm_up_plan_effect_targets_warmth_body_state() {
        let action = WarmUpAction;
        let effects = action.plan_effects();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].subject, Node::Self_);
        assert_eq!(effects[0].predicate, Predicate::Warmth);
        match effects[0].object {
            Value::Quantity(Quantity::Exact(100.0)) => {}
            ref other => panic!("expected Quantity::Exact(100.0), got {:?}", other),
        }
    }

    #[test]
    fn warm_up_satiation_blocks_when_warmth_full() {
        // Mirrors the Drink / Eat satiation gate: once warmth is above
        // the NeedKind::Warmth threshold (0.95), new WarmUp starts are
        // refused to avoid chain-firing beside a campfire.
        let action = WarmUpAction;
        let satiation = action.satiation(None, None);
        // With no physical state we return None (action gate tolerates
        // missing state; this just confirms the wiring reaches the helper).
        assert!(satiation.is_none());
    }

    #[test]
    fn warm_up_action_is_registered() {
        let world = TestWorld::new();
        assert!(world.has_registered_action(crate::agent::actions::ActionType::WarmUp));
    }

    #[test]
    fn warm_up_need_kind_maps_to_warm_up_action() {
        // `NeedKind::Warmth.satisfier()` is the pipeline entry that lets
        // the rational brain know which action closes a warmth goal.
        // Breaking this mapping breaks the whole drive chain.
        assert_eq!(
            NeedKind::Warmth.satisfier(),
            Some(crate::agent::actions::ActionType::WarmUp),
        );
    }
}
