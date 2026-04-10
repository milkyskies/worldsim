pub mod actions;
pub mod activity;
pub mod affordance;
pub mod biology;
pub mod body;
pub mod brains;
pub mod communication;
pub mod culture;
pub mod events;
pub mod invariants;
pub mod inventory;
pub mod item_slots;
pub mod mind;
pub mod movement;
pub mod nervous_system;
pub mod psyche;

pub mod subject;

use bevy::prelude::*;

/// Marker component for all thinking entities (humans, animals, etc.)
/// Systems that apply to all agents should query With<Agent>.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Agent;

/// Marker component for human agents specifically.
/// Use this for human-only behavior (speech, tool use, etc.)
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Person;

#[derive(Component, Default, Debug, Reflect)]
#[reflect(Component)]
pub struct TargetPosition(pub Option<Vec2>);

pub struct AgentPlugin;

impl Plugin for AgentPlugin {
    fn build(&self, app: &mut App) {
        use crate::core::not_paused;

        app.register_type::<Agent>()
            .register_type::<Person>()
            .register_type::<TargetPosition>()
            .register_type::<movement::MovementState>()
            .register_type::<affordance::Affordance>()
            .register_type::<item_slots::ItemSlots>()
            .register_type::<inventory::EntityType>()
            .register_type::<psyche::personality::Personality>()
            .register_type::<body::species::SpeciesProfile>()
            .register_type::<body::needs::PhysicalNeeds>()
            .register_type::<body::needs::Consciousness>()
            .register_type::<body::needs::PsychologicalDrives>()
            .register_type::<activity::CurrentActivity>()
            .register_type::<mind::memory::WorkingMemory>()
            .register_type::<psyche::emotions::EmotionalState>()
            .register_type::<mind::knowledge::MindGraph>()
            .register_type::<psyche::emotions::EmotionalState>()
            .register_type::<psyche::emotions::EmotionConfig>()
            .init_resource::<psyche::emotions::EmotionConfig>()
            .register_type::<mind::knowledge::MindGraph>()
            .register_type::<actions::ActiveActions>()
            .insert_resource(actions::ActionRegistry::new())
            .add_message::<events::GameEvent>()
            .add_message::<events::ActionOutcomeEvent>()
            .add_message::<events::SimEvent>()
            .add_plugins(biology::BiologyPlugin)
            .add_plugins(brains::BrainPlugin)
            .add_plugins(nervous_system::NervousSystemPlugin)
            .add_plugins(invariants::InvariantPlugin)
            .add_plugins(communication::CommunicationPlugin)
            // Unified action execution system
            .add_systems(
                Update,
                (
                    nervous_system::execution::start_actions
                        .after(brains::brain_system::three_brains_system),
                    nervous_system::execution::tick_actions
                        .after(nervous_system::execution::start_actions),
                    nervous_system::execution::apply_action_effects
                        .after(nervous_system::execution::tick_actions),
                    // Becomes substrate: process entity transformations after slot
                    // mutations from action effects. Runs before perception so observers
                    // see consistent post-transformation state on the same tick.
                    crate::world::becomes::becomes_system
                        .after(nervous_system::execution::apply_action_effects),
                )
                    .run_if(not_paused),
            )
            .add_systems(
                Update,
                (
                    // Perception must run first so agents can see resources.
                    // Run after `becomes_system` so a freshly transformed entity
                    // is observed at its new identity rather than its old one.
                    mind::perception::update_visual_perception
                        .after(crate::world::becomes::becomes_system),
                    mind::perception::write_perceptions_to_mind
                        .after(mind::perception::update_visual_perception),
                    mind::perception::update_body_perception,
                    // Perceive water tiles in vision range
                    mind::perception::perceive_water_tiles,
                    // React to perceived dangers (triggers fear based on knowledge)
                    mind::perception::react_to_danger
                        .after(mind::perception::write_perceptions_to_mind),
                    // Social perception: perceive other agents' activities and moods
                    mind::social_perception::perceive_other_agents
                        .after(mind::perception::write_perceptions_to_mind),
                    // Recognition: detect strangers vs known people
                    mind::recognition::check_recognition
                        .after(mind::social_perception::perceive_other_agents),
                )
                    .run_if(not_paused),
            )
            .add_systems(
                Update,
                (
                    crate::agent::mind::belief_updater::process_action_outcomes,
                    mind::memory::process_perception,
                    mind::memory::process_working_memory,
                    mind::memory::decay_stale_knowledge,
                    mind::consolidation::consolidate_knowledge,
                    psyche::emotions::decay_emotions,
                    psyche::emotions::update_mood,
                    psyche::emotions::update_stress,
                    psyche::emotions::react_to_events,
                    // Relationship dynamics
                    psyche::relationships::update_relationships
                        .after(psyche::emotions::react_to_events),
                    psyche::relationships::decay_relationships,
                )
                    .run_if(not_paused),
            )
            .init_resource::<psyche::relationships::RelationshipConfig>();
    }
}
