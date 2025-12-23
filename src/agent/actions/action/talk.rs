//! Talk action - have a topic-based conversation with someone.
//!
//! Conversations share beliefs/knowledge and build relationships.
//! Topics determine what gets shared:
//! - Greetings: Small talk, builds small amount of trust
//! - Knowledge: Share a belief about the world
//! - Feelings: Express emotions, builds affection
//! - Gossip: Share beliefs about other agents

use crate::agent::actions::ActionType;
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, CompletionContext, TargetType,
};
use crate::agent::brains::thinking::{ActionTemplate, TriplePattern};
use crate::agent::events::FailureReason;
use crate::agent::mind::conversation::{ConversationState, Intent, Topic, Turn};
use crate::agent::mind::knowledge::{Node, Predicate, Triple, Value};
use crate::agent::mind::social_perception::CONVERSATION_RANGE;
use crate::agent::psyche::emotions::{Emotion, EmotionType};
use bevy::prelude::*;

pub struct TalkAction;

impl Action for TalkAction {
    fn action_type(&self) -> ActionType {
        ActionType::Talk
    }

    fn name(&self) -> &'static str {
        "Talk"
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed { duration_ticks: 60 } // ~1 second conversation
    }

    // Planning: Need to know the target (must be introduced)
    fn preconditions(&self) -> Vec<TriplePattern> {
        vec![
            // Must have met this person before
            // Note: Planner binds target dynamically
        ]
    }

    // Planning: After talking, social drive is reduced
    fn plan_effects(&self) -> Vec<Triple> {
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
        true // Must be near target to talk
    }

    // Override to_template to set default topic
    fn to_template(
        &self,
        target_entity: Option<Entity>,
        target_position: Option<Vec2>,
    ) -> ActionTemplate {
        let mut template = ActionTemplate {
            name: self.name().to_string(),
            action_type: self.action_type(),
            target_entity,
            target_position,
            topic: Some(Topic::General), // Default topic to prevent None
            content: Vec::new(),
            preconditions: self.preconditions(),
            effects: self.plan_effects(),
            base_cost: self.cost(),
        };

        // Add proximity precondition if needed
        if self.requires_proximity()
            && let Some(pos) = target_position
        {
            const TILE_SIZE: f32 = 16.0;
            let tile = (
                (pos.x / TILE_SIZE).floor() as i32,
                (pos.y / TILE_SIZE).floor() as i32,
            );
            template.preconditions.push(TriplePattern::self_at(tile));
        }

        template
    }

    // Execution: Check if we can talk to this person
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

        // Must have been introduced
        let introduced = ctx.mind.query(
            Some(&Node::Entity(target)),
            Some(Predicate::Introduced),
            Some(&Value::Boolean(true)),
        );
        if introduced.is_empty() {
            // We don't know them - need to introduce first
            // But for now, allow talking anyway (they could introduce during the talk)
        }

        Ok(())
    }

    // Execution: What happens when we finish talking
    fn on_complete(&self, ctx: &mut CompletionContext) {
        // Reduce social drive - each conversation turn satisfies social need
        // Small reduction per turn, accumulates over conversation
        if let Some(drives) = &mut ctx.drives {
            drives.social = (drives.social - 0.1).max(0.0);
        }

        // 1. Get ConversationManager, Topic, Target, Actor
        if let (Some(cm), Some(topic), Some(target), Some(actor)) = (
            &mut ctx.conversation_manager,
            ctx.topic,
            ctx.target_entity,
            Some(ctx.actor),
        ) {
            // 2. Find or Start Conversation
            let participants = vec![actor, target];
            let conversation_id = if let Some(c) = cm.find_active(&participants) {
                c.id
            } else {
                cm.start_conversation(participants.clone(), ctx.tick)
            };

            // 3. Determine Intent
            // Logic: If we have content, we are Sharing. If we want info (topic specific) but no content, we are Asking.
            let mut intent = if !ctx.content.is_empty() {
                Intent::Share
            } else {
                match topic {
                    // Questions
                    Topic::Location(_) | Topic::Person(_) | Topic::State(_) => Intent::Ask,
                    // Greetings/Farewells/General
                    Topic::General => Intent::Share,
                    Topic::Help => Intent::Ask,
                }
            };

            // 4. Update Conversation State & Intent overrides
            if let Some(c) = cm.get_mut(conversation_id) {
                c.last_activity = ctx.tick;

                // Override intent based on flow
                intent = match (c.state.clone(), intent) {
                    // Ending the conversation?
                    (_, Intent::Farewell) => Intent::Farewell, // Keep explicit farewell

                    // Just started? Force Greeting
                    (ConversationState::Greeting, _) if c.turns.len() < 2 => Intent::Greet,

                    // Wrapping up?
                    (ConversationState::Wrapping, _) => Intent::Farewell,

                    // Otherwise keep determined intent
                    (_, i) => i,
                };

                // State Transitions
                c.state = match (c.state.clone(), &intent) {
                    (_, Intent::Farewell) => ConversationState::Ended,
                    (ConversationState::Greeting, _) if c.turns.len() >= 1 => {
                        ConversationState::Active
                    }
                    (ConversationState::Active, _) => {
                        // Check if we should wrap up (e.g. social satisfied)
                        ConversationState::Active
                    }
                    (state, _) => state,
                };
            }

            // 5. Determine if we expect a response
            let expects_response = matches!(intent, Intent::Ask | Intent::Greet);

            // 6. Create Turn
            let turn = Turn {
                speaker: actor,
                intent,
                topic: topic.clone(),
                emotion: Some(Emotion::new(EmotionType::Joy, 0.5)), // Placeholder positive emotion
                content: ctx.content.clone(),
                timestamp: ctx.tick,
                expects_response,
            };

            // 7. Add to Conversation
            if let Some(c) = cm.get_mut(conversation_id) {
                c.add_turn(turn);
            }
        }
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("talking")
    }

    fn complete_log(&self) -> Option<&'static str> {
        Some("finished talking")
    }
}
