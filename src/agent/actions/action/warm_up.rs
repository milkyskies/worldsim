//! WarmUp action — stay beside a heat source to restore warmth.
//!
//! Reads: PhysicalNeeds.warmth (satiation gate), MindGraph (runtime proximity)
//! Writes: runtime effects top up `PhysicalNeeds.warmth` via `on_complete`
//! Upstream: ActionRegistry, temperature perception (writes heat-source
//!           entities into mindgraph with `IsA` + `LocatedAt`)
//! Downstream: warmth drain/recovery system (reads active WarmUp for momentum)
//!
//! Uses `TargetSource::None` with an explicit `(Self, Near, Campfire)`
//! precondition. The concept-near walk generator grounds the precondition
//! into a specific tile when an entity is known, and Build's `Near` effect
//! closes it when no heat source exists yet. Execution narrows "near some
//! campfire" to a concrete entity via the `can_start` proximity check.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::motor::{
    ActionPrimitive, Behavior, IntensityPolicy, Intent, TargetSelector,
};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, CompletionContext, TargetSource,
};
use crate::agent::brains::thinking::TriplePattern;
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
        const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Focus, 0.3)];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        Some(Posture::Stationary)
    }

    fn target_source(&self) -> TargetSource {
        TargetSource::None
    }

    fn preconditions(&self) -> Vec<TriplePattern> {
        vec![TriplePattern::new(
            Some(Node::Self_),
            Some(Predicate::Near),
            Some(Value::Concept(Concept::Campfire)),
        )]
    }

    fn plan_effects(&self) -> Vec<Triple> {
        vec![Triple::new(
            Node::Self_,
            Predicate::Warmth,
            Value::Quantity(Quantity::Exact(100.0)),
        )]
    }

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

    fn can_start(&self, ctx: &ActionContext) -> Result<(), FailureReason> {
        if is_near_any_heat_source(ctx) {
            Ok(())
        } else {
            Err(FailureReason::TargetGone)
        }
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

/// Runtime mirror of the planner's `(Self, Near, HeatEmitting)` relation:
/// true when a known heat-emitting entity sits on self's current tile.
fn is_near_any_heat_source(ctx: &ActionContext) -> bool {
    let Some(Value::Tile(self_tile)) = ctx.mind.get(&Node::Self_, Predicate::LocatedAt).cloned()
    else {
        return false;
    };
    ctx.mind
        .query(
            None,
            Some(Predicate::LocatedAt),
            Some(&Value::Tile(self_tile)),
        )
        .iter()
        .any(|t| {
            matches!(t.subject, Node::Entity(_))
                && ctx.mind.has_trait(&t.subject, Concept::HeatEmitting)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::actions::registry::Action;
    use crate::agent::body::need::NeedKind;
    use crate::testing::TestWorld;

    #[test]
    fn warm_up_declares_near_campfire_precondition() {
        let action = WarmUpAction;
        assert!(matches!(
            action.target_source(),
            crate::agent::actions::registry::TargetSource::None
        ));
        let preconditions = action.preconditions();
        assert_eq!(preconditions.len(), 1);
        assert_eq!(preconditions[0].predicate, Some(Predicate::Near));
        assert_eq!(
            preconditions[0].object,
            Some(Value::Concept(Concept::Campfire))
        );
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
    fn warm_up_satiation_returns_none_without_physical_state() {
        let action = WarmUpAction;
        assert!(action.satiation(None, None).is_none());
    }

    #[test]
    fn warm_up_action_is_registered() {
        let world = TestWorld::new();
        assert!(world.has_registered_action(crate::agent::actions::ActionType::WarmUp));
    }

    #[test]
    fn warm_up_need_kind_maps_to_warm_up_action() {
        assert_eq!(
            NeedKind::Warmth.satisfier(),
            Some(crate::agent::actions::ActionType::WarmUp),
        );
    }
}
