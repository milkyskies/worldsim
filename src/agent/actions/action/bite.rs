//! Bite action - jaws-as-weapon attack for species with Bite capability.
//!
//! Distinct from [`AttackAction`](super::attack::AttackAction) because the
//! two actions require different capability channels: Attack needs
//! Manipulation (humans holding weapons or striking with hands) while Bite
//! needs the dedicated `Channel::Bite` that only jaws / beaks / pincers
//! provide. A wolf can `Bite` but not `Attack`; a human can `Attack` but
//! not `Bite`.
//!
//! No AI currently proposes this action — it exists so the capability is
//! reachable by tests and can be wired into future wolf predator behaviour
//! without another round of channel refactoring.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, RuntimeEffects, TargetSource,
};
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
        TargetSource::EntityAffordance
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

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            energy_per_sec: ENERGY_PER_SEC,
            ..Default::default()
        }
    }
}
