//! Bite action — jaws-as-weapon variant of Attack for species with `Channel::Bite`.
//! Hunting semantics (Prey enumeration, Produces yield, Becomes transformation)
//! are shared with Attack via the helpers in `attack.rs`.

use crate::agent::actions::ActionType;
use crate::agent::actions::action::attack::{
    apply_hunt_kill, prey_produces_useful_item, prey_yield_effects,
};
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
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
        // Jaws and whole-body commitment — no Locomotion claim so a wolf
        // can keep charging while biting. Posture-agnostic handles the
        // stance side of the equation.
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(Channel::Bite, 1.0),
            ChannelUsage::new(Channel::FullBody, 0.7),
        ];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        // Posture-agnostic: a charging wolf biting its prey is the
        // canonical example. Bite works from a standstill (ambush) or
        // mid-sprint (hunt).
        None
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

    fn on_complete(&self, ctx: &mut CompletionContext) {
        apply_hunt_kill(ctx);
    }
}
