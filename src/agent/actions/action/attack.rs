use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{BodyChannel, ChannelUsage};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, RuntimeEffects, TargetSource,
};
use crate::constants::actions::attack::{BASE_COST, DURATION_TICKS, ENERGY_PER_SEC};

pub struct AttackAction;

impl Action for AttackAction {
    fn name(&self) -> &'static str {
        "Attack"
    }

    fn action_type(&self) -> ActionType {
        ActionType::Attack
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
        TargetSource::EntityAffordance
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(BodyChannel::Hands, 0.9),
            ChannelUsage::new(BodyChannel::Legs, 0.6),
            ChannelUsage::new(BodyChannel::FullBody, 0.7),
        ];
        CHANNELS
    }

    fn can_start(&self, ctx: &ActionContext) -> Result<(), crate::agent::events::FailureReason> {
        if ctx.target_entity.is_none() {
            return Err(crate::agent::events::FailureReason::NoTarget);
        }
        Ok(())
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            energy_per_sec: ENERGY_PER_SEC,
            ..Default::default()
        }
    }
}
