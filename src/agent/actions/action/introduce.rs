//! Introduce action - exchange names with a stranger.
//!
//! When an agent introduces themselves to another agent:
//! 1. Both parties learn each other's names
//! 2. Both mark each other as "Knows" and "Introduced"
//! 3. Initial relationship values are set (neutral)

use crate::agent::actions::ActionType;
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, CompletionContext, TargetType,
};
use crate::agent::brains::thinking::TriplePattern;
use crate::agent::events::FailureReason;
use crate::agent::mind::knowledge::{Node, Predicate, Triple, Value};
use crate::agent::mind::social_perception::CONVERSATION_RANGE;

pub struct IntroduceAction;

impl Action for IntroduceAction {
    fn action_type(&self) -> ActionType {
        ActionType::Introduce
    }

    fn name(&self) -> &'static str {
        "Introduce"
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed { duration_ticks: 30 } // ~0.5 second introduction
    }

    // Planning: Need a target that is a stranger
    fn preconditions(&self) -> Vec<TriplePattern> {
        // Target must be a Person and a Stranger (not yet known)
        vec![
            // We need a target entity that is a Person
            // Note: The planner will bind this when generating actions
        ]
    }

    // Planning: After introducing, we know the target
    fn plan_effects(&self) -> Vec<Triple> {
        // After introduction, SocialDrive is satisfied
        vec![Triple::new(
            Node::Self_,
            Predicate::SocialDrive,
            Value::Int(0),
        )]
    }

    fn target_type(&self) -> TargetType {
        TargetType::Entity // Needs another agent as target
    }

    fn requires_proximity(&self) -> bool {
        true // Must be near target to introduce
    }

    // Execution: Check if we can actually introduce ourselves
    fn can_start(&self, ctx: &ActionContext) -> Result<(), FailureReason> {
        // Must have a target
        let Some(target) = ctx.target_entity else {
            return Err(FailureReason::NoTarget);
        };

        // Must be close enough to talk
        if let Some(target_pos) = ctx.target_position
            && ctx.agent_position.distance(target_pos) > CONVERSATION_RANGE
        {
            return Err(FailureReason::TooFar);
        }

        // Check if we already know this person (don't re-introduce)
        let knows = ctx.mind.query(
            Some(&Node::Entity(target)),
            Some(Predicate::Introduced),
            Some(&Value::Boolean(true)),
        );
        if !knows.is_empty() {
            // Already introduced - can't introduce again
            return Err(FailureReason::AlreadyDone);
        }

        Ok(())
    }

    // Execution: What happens when we finish introducing
    fn on_complete(&self, _ctx: &mut CompletionContext) {
        // Note: The actual relationship initialization happens via events
        // The system will emit a GameEvent::SocialInteraction
        // which the relationships system will process
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("introducing self")
    }

    fn complete_log(&self) -> Option<&'static str> {
        Some("introduced self")
    }

    /// Custom to_template that adds target-bound effect
    /// This is needed for is_step_complete to detect when the introduction is done
    fn to_template(
        &self,
        target_entity: Option<bevy::prelude::Entity>,
        target_position: Option<bevy::prelude::Vec2>,
    ) -> crate::agent::brains::thinking::ActionTemplate {
        use crate::agent::brains::thinking::ActionTemplate;
        use crate::agent::brains::thinking::TriplePattern;

        let mut preconditions = self.preconditions();

        // Add location requirement (from requires_proximity)
        if let Some(pos) = target_position {
            const TILE_SIZE: f32 = 16.0;
            let tile = (
                (pos.x / TILE_SIZE).floor() as i32,
                (pos.y / TILE_SIZE).floor() as i32,
            );
            preconditions.push(TriplePattern::self_at(tile));
        }

        // Build effects with target-bound Knows predicate
        let mut effects = Vec::new();
        if let Some(entity) = target_entity {
            // After introduction, target Knows = true (this is what initialize_relationship writes)
            effects.push(Triple::new(
                Node::Entity(entity),
                Predicate::Knows,
                Value::Boolean(true),
            ));
        }

        ActionTemplate {
            name: self.name().to_string(),
            action_type: self.action_type(),
            target_entity,
            target_position,
            topic: None,
            content: Vec::new(),
            preconditions,
            effects,
            base_cost: self.cost(),
        }
    }
}
