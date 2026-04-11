//! Bite action — jaws-as-weapon variant of Attack for species with `Channel::Bite`.
//! Planning semantics (Prey enumeration, Produces yield projection) are
//! shared with Attack via the helpers in `attack.rs`. Damage, hit
//! resolution, and death live in `biology::combat`.

use crate::agent::actions::ActionType;
use crate::agent::actions::action::attack::{prey_produces_useful_item, prey_yield_effects};
use crate::agent::actions::channel::{Channel, ChannelUsage};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, CompletionContext, RuntimeEffects, TargetCandidate,
    TargetSource,
};
use crate::agent::mind::knowledge::{Concept, MindGraph, Triple};
use crate::constants::actions::attack::{BASE_COST, DURATION_TICKS, STAMINA_PER_SEC};

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
            stamina_per_sec: STAMINA_PER_SEC,
            ..Default::default()
        }
    }

    // Damage, bleed, death, and meat deposit all live in
    // `biology::combat::resolve_combat_hits`. Keeping this empty means
    // the Bite action definition only knows about channels and planning;
    // combat semantics stay in the combat module.
    fn on_complete(&self, _ctx: &mut CompletionContext) {}
}
