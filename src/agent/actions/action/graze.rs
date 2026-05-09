//! Graze action — slow drift across grass while continuously eating.
//!
//! Fused walk-and-eat action expressed via capability channels: Locomotion
//! at low intensity (slow drift) plus Consumption at high intensity
//! (continuous nibbling). Plant carbs flow continuously via
//! `stomach_carbs_per_sec`, not a completion hook — the animal feeds
//! throughout the drift.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::{Concept, Predicate};
use crate::constants::actions::graze::STOMACH_CARBS_PER_SEC;

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Locomotion, 0.3),
    ChannelUsage::new(Channel::Consumption, 0.8),
];

pub static GRAZE_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Graze,
    kind: ActionKind::Movement,
    target_source: TargetSource::TileWithTrait(Concept::Grazable),
    base_cost: 2.0,
    primitive: ActionPrimitive::Ingest,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Ambient,
    intent: Intent::Hunger,
    body_channels: CHANNELS,
    posture: Some(Posture::Moving),
    interruptible: true,
    start_log: Some("grazing"),
    complete_log: None,
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: STOMACH_CARBS_PER_SEC,
    preconditions: &[],
    plan_effects: &[EffectTemplate::SelfNeedExact {
        predicate: Predicate::Hunger,
        value: 0.0,
    }],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[Gate::OnGrassTile],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks::EMPTY,
    recipe: None,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::actions::GenericAction;
    use crate::agent::actions::channel::{ChannelCapacities, ChannelLoad};
    use crate::agent::actions::registry::Action;

    fn graze() -> GenericAction {
        GenericAction::new(&GRAZE_DEF)
    }

    #[test]
    fn graze_admits_cleanly_on_empty_load() {
        let load = ChannelLoad::new();
        let caps = ChannelCapacities::full();
        assert!(!load.would_hard_conflict(graze().body_channels(), &caps));
    }

    #[test]
    fn graze_leaves_most_locomotion_free_for_flee() {
        let mut load = ChannelLoad::new();
        load.add(graze().body_channels());
        assert!(load.saturation(Channel::Locomotion) < 0.5);
    }
}
