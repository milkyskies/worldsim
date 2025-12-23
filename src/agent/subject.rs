use bevy::prelude::*;

/// What can have emotional associations or beliefs attached to it.
/// Used as a key in EmotionalProfile and Beliefs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum Subject {
    Agent(Entity),
    Action(super::actions::ActionType),
    Concept(crate::agent::mind::knowledge::Concept),
}
