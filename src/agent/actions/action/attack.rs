use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{BodyChannel, ChannelUsage};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, RuntimeEffects, TargetType,
};
use crate::agent::mind::knowledge::Triple;
use crate::constants::actions::attack::{BASE_COST, DURATION_TICKS, ENERGY_PER_SEC};
use bevy::prelude::*;

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

    fn target_type(&self) -> TargetType {
        TargetType::Entity
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(BodyChannel::Hands, 0.9),
            ChannelUsage::new(BodyChannel::Legs, 0.6),
            ChannelUsage::new(BodyChannel::FullBody, 0.7),
        ];
        CHANNELS
    }

    fn preconditions(&self) -> Vec<crate::agent::brains::thinking::TriplePattern> {
        vec![] // Needs proximity, but handled by can_start usually
    }

    fn plan_effects(&self) -> Vec<Triple> {
        vec![] // Attack effects are complex (damage), maybe just dummy for now
    }

    fn can_start(&self, ctx: &ActionContext) -> Result<(), crate::agent::events::FailureReason> {
        if ctx.target_entity.is_none() {
            return Err(crate::agent::events::FailureReason::NoTarget);
        }
        // Check range?
        Ok(())
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            energy_per_sec: ENERGY_PER_SEC,
            ..Default::default()
        }
    }

    fn to_template(
        &self,
        target_entity: Option<Entity>,
        _target_position: Option<Vec2>,
    ) -> crate::agent::brains::thinking::ActionTemplate {
        use crate::agent::brains::thinking::ActionTemplate;

        let preconditions = self.preconditions();
        if let Some(_entity) = target_entity {
            // Need to be close to attack?
            // preconditions.push(TriplePattern::located_at_entity(_entity));
        }

        ActionTemplate {
            name: if let Some(e) = target_entity {
                format!("Attack {:?}", e)
            } else {
                "Attack".to_string()
            },
            action_type: self.action_type(),
            target_entity,
            target_position: None,
            topic: None,
            content: Vec::new(),
            preconditions,
            effects: self.plan_effects(),
            base_cost: self.cost(),
        }
    }
}
