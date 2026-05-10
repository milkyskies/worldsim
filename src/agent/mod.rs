pub mod actions;
pub mod affordance;
pub mod biology;
pub mod body;
pub mod brains;
pub mod culture;
pub mod drive_registry;
pub mod engagement;
pub mod events;
pub mod invariants;
pub mod inventory;
pub mod item_slots;
pub mod mind;
pub mod movement;
pub mod naming;
pub mod nervous_system;
pub mod psyche;
pub mod skills;
pub mod spawn_human;

pub mod subject;

use bevy::prelude::*;

/// Marker component for all thinking entities (humans, animals, etc.)
/// Removed by `kill_into_corpse` when the entity becomes a corpse.
/// For liveness checks, prefer `With<Alive>` — it is removed earlier
/// (by `die()`) and has no 1-tick gap.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Agent;

/// Marker for a living agent. Inserted at spawn, removed by `die()`.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Alive;

/// Marker for a dead agent. Inserted by `die()`, persists on the corpse.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Dead;

/// Marker component for human agents specifically.
/// Use this for human-only behavior (speech, tool use, etc.)
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Person;

/// Set when `pick_flee_target` exhausts every escape candidate. Read by
/// the threat-appraisal function as the `escape_available = false` input
/// — cornered agents lower their Fight threshold dramatically. Cleared
/// the next time a flee target is found, so this flips back and forth
/// as the situation changes.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Cornered;

/// Set on agents whose leg `BodyNode`s are below the lameness threshold.
/// Visible to perception (other agents see the Lame trait), used by
/// predator target selection to prefer wounded prey, and modulates speed
/// on top of the existing channel-capacity penalty.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Lame;

/// Set on agents who took heavy head damage; they skip action proposals
/// until `until_tick`. Cleared by the daze-recovery system once the tick
/// passes.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Dazed {
    pub until_tick: u64,
}

/// Last-perceived flee direction, used by `pick_flee_target` for
/// momentum: small threat-position drift no longer pivots the flee
/// vector every tick. Cleared when the agent is no longer fleeing.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct FleeMomentum {
    pub direction: Vec2,
    pub last_tick: u64,
}

#[derive(Component, Default, Debug, Reflect)]
#[reflect(Component)]
pub struct TargetPosition(pub Option<Vec2>);

pub struct AgentPlugin;

impl Plugin for AgentPlugin {
    fn build(&self, app: &mut App) {
        use crate::core::{every_n_ticks, not_paused};

        app.register_type::<Agent>()
            .register_type::<Alive>()
            .register_type::<Dead>()
            .register_type::<Person>()
            .register_type::<Cornered>()
            .register_type::<Lame>()
            .register_type::<Dazed>()
            .register_type::<FleeMomentum>()
            .register_type::<TargetPosition>()
            .register_type::<movement::MovementState>()
            .register_type::<affordance::Affordance>()
            .register_type::<item_slots::ItemSlots>()
            .register_type::<item_slots::Thing>()
            .register_type::<item_slots::ThingProperties>()
            .register_type::<inventory::EntityType>()
            .register_type::<psyche::personality::Personality>()
            .register_type::<psyche::values::Values>()
            .register_type::<body::species::SpeciesProfile>()
            .register_type::<body::genetics::genome::Genome>()
            .register_type::<body::genetics::phenotype::Phenotype>()
            .register_type::<body::needs::PhysicalNeeds>()
            .register_type::<body::needs::Consciousness>()
            .register_type::<body::needs::PsychologicalDrives>()
            .register_type::<body::needs::SocialDriveOverride>()
            .register_type::<mind::affective_tom::AffectiveToM>()
            .register_type::<mind::memory::WorkingMemory>()
            .register_type::<psyche::emotions::EmotionalState>()
            .register_type::<mind::knowledge::MindGraph>()
            .register_type::<psyche::emotions::EmotionalState>()
            .register_type::<psyche::emotions::EmotionConfig>()
            .init_resource::<psyche::emotions::EmotionConfig>()
            .register_type::<mind::knowledge::MindGraph>()
            .register_type::<skills::Skills>()
            .register_type::<skills::SkillsConfig>()
            .init_resource::<skills::SkillsConfig>()
            .register_type::<actions::ActiveActions>()
            .insert_resource(actions::ActionRegistry::new())
            .init_resource::<crate::core::SimRng>()
            .init_resource::<naming::NameCounters>()
            .add_message::<events::GameEvent>()
            .add_message::<events::ActionOutcomeEvent>()
            .add_message::<events::SimEvent>()
            .add_plugins(biology::BiologyPlugin)
            .add_plugins(brains::BrainPlugin)
            .add_plugins(nervous_system::NervousSystemPlugin)
            .add_plugins(invariants::InvariantPlugin)
            .init_resource::<psyche::greetings::GreetingCooldowns>()
            .add_plugins(engagement::EngagementPlugin)
            .add_systems(
                FixedUpdate,
                (
                    nervous_system::execution::start_actions
                        .after(brains::brain_system::arbitrate_every_tick),
                    nervous_system::execution::tick_actions
                        .after(nervous_system::execution::start_actions),
                    nervous_system::execution::apply_action_effects
                        .after(nervous_system::execution::tick_actions),
                )
                    .in_set(crate::core::PerfBucket::Action)
                    .in_set(crate::core::PerfSubBucket::ActionExecution)
                    .run_if(not_paused),
            )
            .add_systems(
                FixedUpdate,
                (
                    crate::world::becomes::labor_accumulation_system
                        .after(nervous_system::execution::apply_action_effects),
                    crate::world::becomes::becomes_system
                        .after(crate::world::becomes::labor_accumulation_system),
                    crate::world::emits_effect::emits_effect_system
                        .after(crate::world::becomes::becomes_system),
                )
                    .in_set(crate::core::PerfBucket::Action)
                    .in_set(crate::core::PerfSubBucket::ActionWorldMutation)
                    .run_if(not_paused),
            )
            // Visual perception is N² across visible entities — the dominant cost
            // inside the Perception bucket. Kept isolated so regressions here
            // show up under its own sub-bucket instead of hiding in a combined
            // row with the cheap sensory scans.
            .add_systems(
                FixedUpdate,
                (
                    mind::perception::update_visual_perception,
                    mind::perception::write_perceptions_to_mind
                        .after(mind::perception::update_visual_perception),
                )
                    .in_set(crate::core::PerfBucket::Perception)
                    .in_set(crate::core::PerfSubBucket::PerceptionVisual)
                    .run_if(not_paused),
            )
            .add_systems(
                FixedUpdate,
                (
                    mind::perception::update_body_perception,
                    mind::perception::perceive_temperature,
                    mind::perception::perceive_hearing,
                    mind::perception::emit_alarm_calls,
                    mind::perception::cleanup_sound_sources
                        .after(mind::perception::perceive_hearing)
                        .after(mind::perception::emit_alarm_calls),
                    mind::perception::react_to_danger
                        .after(mind::perception::write_perceptions_to_mind),
                )
                    .in_set(crate::core::PerfBucket::Perception)
                    .in_set(crate::core::PerfSubBucket::PerceptionSensory)
                    .run_if(not_paused),
            )
            .add_systems(
                FixedUpdate,
                (
                    mind::social_perception::perceive_other_agents
                        .after(mind::perception::write_perceptions_to_mind),
                    mind::recognition::check_recognition
                        .after(mind::social_perception::perceive_other_agents),
                    mind::theory_of_mind::update_shared_experience_tom
                        .after(mind::perception::write_perceptions_to_mind),
                    mind::affective_tom::update_affective_tom
                        .after(mind::social_perception::perceive_other_agents),
                    mind::affective_tom::decay_affective_tom
                        .after(mind::affective_tom::update_affective_tom),
                )
                    .in_set(crate::core::PerfBucket::Perception)
                    .in_set(crate::core::PerfSubBucket::PerceptionSocial)
                    .run_if(not_paused),
            )
            .add_systems(
                FixedUpdate,
                (
                    crate::agent::mind::belief_updater::process_action_outcomes,
                    mind::memory::process_perception,
                    mind::memory::process_working_memory,
                    mind::memory::decay_stale_knowledge,
                )
                    .in_set(crate::core::PerfBucket::Memory)
                    .in_set(crate::core::PerfSubBucket::MemoryWmTick)
                    .run_if(not_paused),
            )
            .add_systems(
                FixedUpdate,
                mind::consolidation::consolidate_knowledge
                    .in_set(crate::core::PerfBucket::Memory)
                    .in_set(crate::core::PerfSubBucket::MemoryConsolidation)
                    .run_if(not_paused),
            )
            .add_systems(
                FixedUpdate,
                mind::knowledge::drain_mindgraph_mutations
                    .after(mind::memory::process_perception)
                    .after(mind::memory::decay_stale_knowledge)
                    .in_set(crate::core::PerfBucket::Memory)
                    .in_set(crate::core::PerfSubBucket::MemoryMindgraphDrain)
                    .run_if(not_paused),
            )
            // Psyche bucket split into 3 sub-bucket groups (emotions / relationships /
            // social_drives). Ordering chain `react_to_events → update_relationships
            // → decay_social_from_proximity → social_acknowledgments` spans the
            // groups — Bevy preserves it via `.after()` on system identity, so the
            // tuple split doesn't break execution order.
            .add_systems(
                FixedUpdate,
                (
                    psyche::emotions::decay_emotions,
                    psyche::emotions::update_mood,
                    psyche::emotions::update_stress,
                    psyche::emotions::react_to_events,
                    psyche::emotions::react_to_combat_hit,
                )
                    .in_set(crate::core::PerfBucket::Psyche)
                    .in_set(crate::core::PerfSubBucket::PsycheEmotions)
                    .run_if(not_paused),
            )
            .add_systems(
                FixedUpdate,
                (
                    psyche::relationships::update_relationships
                        .after(psyche::emotions::react_to_events),
                    psyche::relationships::decay_relationships,
                )
                    .in_set(crate::core::PerfBucket::Psyche)
                    .in_set(crate::core::PerfSubBucket::PsycheRelationships)
                    .run_if(not_paused),
            )
            .add_systems(
                FixedUpdate,
                (
                    psyche::flocking::decay_social_from_proximity
                        .after(brains::brain_system::arbitrate_every_tick),
                    psyche::greetings::social_acknowledgments
                        .after(psyche::flocking::decay_social_from_proximity),
                )
                    .in_set(crate::core::PerfBucket::Psyche)
                    .in_set(crate::core::PerfSubBucket::PsycheSocial)
                    .run_if(not_paused),
            )
            .add_systems(
                FixedUpdate,
                (
                    skills::skill_progression_system.after(nervous_system::execution::tick_actions),
                    skills::decay_skills_system,
                )
                    .in_set(crate::core::PerfBucket::Skills)
                    .run_if(not_paused),
            )
            .add_systems(
                FixedUpdate,
                item_slots::freshness_decay_system
                    .in_set(crate::core::PerfBucket::Biology)
                    .run_if(every_n_ticks(100))
                    .run_if(not_paused),
            )
            .init_resource::<psyche::relationships::RelationshipConfig>()
            // Genetics: develop phenotype from genome once at spawn, before any
            // brain or personality system reads the derived traits. Lives in
            // FixedPreUpdate so it runs inside FixedMain with the rest of the
            // game logic — TestWorld::tick() skips Update/PreUpdate entirely.
            .add_systems(
                FixedPreUpdate,
                (
                    body::genetics::phenotype::develop_phenotype_system,
                    body::genetics::phenotype::apply_stamina_genetics_system,
                )
                    .chain(),
            );
    }
}
