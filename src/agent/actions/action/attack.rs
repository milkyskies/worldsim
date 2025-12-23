use crate::agent::actions::ActionType;
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, RuntimeEffects, TargetType,
};
use crate::agent::mind::knowledge::Triple;
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
        ActionKind::Timed { duration_ticks: 30 }
    }

    fn cost(&self) -> f32 {
        10.0 // Expensive
    }

    fn target_type(&self) -> TargetType {
        TargetType::Entity
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
            energy_per_sec: -2.0, // Tiring
            ..Default::default()
        }
    }

    fn to_template(
        &self,
        target_entity: Option<Entity>,
        target_position: Option<Vec2>,
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
