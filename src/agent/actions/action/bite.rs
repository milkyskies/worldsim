//! Bite action - jaws-as-weapon attack for species with Bite capability.
//!
//! Distinct from [`AttackAction`](super::attack::AttackAction) because the
//! two actions require different capability channels: Attack needs
//! Manipulation (humans holding weapons or striking with hands) while Bite
//! needs the dedicated `Channel::Bite` that only jaws / beaks / pincers
//! provide. A wolf can `Bite` but not `Attack`; a human can `Attack` but
//! not `Bite`.
//!
//! Both actions share the same hunting semantics: enumerated against any
//! entity the agent's mind tags as `HasTrait Prey`, and on completion they
//! drop the prey's yield into the hunter's inventory and queue a `Becomes`
//! transformation so the slain prey turns into a meat-drop entity.

use crate::agent::actions::ActionType;
use crate::agent::actions::action::attack::{
    apply_hunt_kill, prey_produces_useful_item, prey_yield_effects,
};
use crate::agent::actions::channel::{Channel, ChannelUsage};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, CompletionContext, RuntimeEffects, TargetCandidate,
    TargetSource,
};
use crate::agent::mind::knowledge::{Concept, MindGraph, Triple};
use crate::constants::actions::attack::{BASE_COST, DURATION_TICKS, ENERGY_PER_SEC};

pub struct BiteAction;

impl Action for BiteAction {
    fn name(&self) -> &'static str {
        "Bite"
    }

    fn action_type(&self) -> ActionType {
        ActionType::Bite
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed {
            duration_ticks: DURATION_TICKS,
        }
    }

    fn cost(&self) -> f32 {
        BASE_COST
    }

    fn target_source(&self) -> TargetSource {
        // Wolves and other biters hunt prey via the same trait gate as
        // humans use for Attack. The emotional brain still bypasses
        // enumeration when proposing Bite on a perceived threat.
        TargetSource::EntityWithTrait(Concept::Prey)
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(Channel::Bite, 1.0),
            ChannelUsage::new(Channel::Locomotion, 0.6),
            ChannelUsage::new(Channel::FullBody, 0.7),
        ];
        CHANNELS
    }

    fn can_start(&self, ctx: &ActionContext) -> Result<(), crate::agent::events::FailureReason> {
        if ctx.target_entity.is_none() {
            return Err(crate::agent::events::FailureReason::NoTarget);
        }
        Ok(())
    }

    fn plan_effects_for_target(&self, target: &TargetCandidate, mind: &MindGraph) -> Vec<Triple> {
        let Some(entity) = target.as_entity() else {
            return self.plan_effects();
        };
        prey_yield_effects(entity, mind)
    }

    fn is_plan_valid(&self, target: &TargetCandidate, mind: &MindGraph) -> bool {
        let Some(entity) = target.as_entity() else {
            return false;
        };
        prey_produces_useful_item(entity, mind)
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            energy_per_sec: ENERGY_PER_SEC,
            ..Default::default()
        }
    }

    fn on_complete(&self, ctx: &mut CompletionContext) {
        apply_hunt_kill(ctx);
    }
}
