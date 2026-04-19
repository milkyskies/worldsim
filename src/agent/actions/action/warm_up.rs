//! WarmUp action — stay beside a heat source to restore warmth.
//!
//! Declares `(Self, Near, Campfire)` as a planner precondition — the
//! concept-near walk generator grounds this to a specific tile when a heat
//! source is known, and Build's `Near` effect closes it when no heat
//! source exists yet. Execution narrows "near some campfire" to a concrete
//! entity via the [`Gate::NearHeatEmitter`] runtime check.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, Pattern, PlanValidity,
    RuntimeOp, SatiationGate, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::{Concept, Predicate};
use crate::constants::actions::warm_up::{DURATION_TICKS, STAMINA_GAIN, WARMTH_RECOVERY};

const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Focus, 0.3)];

pub static WARM_UP_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::WarmUp,
    name: "WarmUp",
    kind: ActionKind::Timed {
        duration_ticks: DURATION_TICKS,
    },
    target_source: TargetSource::None,
    base_cost: 1.0,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Goal,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    interruptible: true,
    start_log: Some("started warming up"),
    complete_log: Some("warmed up"),
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[Pattern::SelfNearConcept(Concept::Campfire)],
    plan_effects: &[EffectTemplate::SelfNeedExact {
        predicate: Predicate::Warmth,
        value: 100.0,
    }],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[Gate::NearHeatEmitter],
    satiation: Some(SatiationGate::WarmthValue),
    completion: CompletionPredicate::Never,
    on_complete_ops: &[
        RuntimeOp::TopUpWarmth(WARMTH_RECOVERY),
        RuntimeOp::AdjustAerobic(STAMINA_GAIN),
    ],
    hooks: Hooks::EMPTY,
    recipe: None,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::actions::registry::Action;
    use crate::agent::actions::{ActionRegistry, ActionType};
    use crate::agent::body::need::NeedKind;
    use crate::agent::mind::knowledge::Value;
    use crate::testing::TestWorld;

    fn warm_up() -> Box<dyn Action> {
        Box::new(crate::agent::actions::GenericAction::new(&WARM_UP_DEF))
    }

    #[test]
    fn warm_up_declares_near_campfire_precondition() {
        let action = warm_up();
        assert!(matches!(action.target_source(), TargetSource::None));
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
        let action = warm_up();
        let effects = action.plan_effects();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].predicate, Predicate::Warmth);
    }

    #[test]
    fn warm_up_satiation_returns_none_without_physical_state() {
        let action = warm_up();
        assert!(action.satiation(None, None).is_none());
    }

    #[test]
    fn warm_up_action_is_registered() {
        let world = TestWorld::new();
        assert!(world.has_registered_action(ActionType::WarmUp));
    }

    #[test]
    fn warm_up_need_kind_maps_to_warm_up_action() {
        assert_eq!(NeedKind::Warmth.satisfier(), Some(ActionType::WarmUp),);
    }

    #[test]
    #[allow(dead_code)]
    fn registry_exposes_warm_up() {
        let registry = ActionRegistry::new();
        assert!(registry.get(ActionType::WarmUp).is_some());
    }
}
