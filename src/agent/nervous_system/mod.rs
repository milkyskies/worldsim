use crate::agent::brains::thinking::{ActionTemplate, Goal, TriplePattern};
use bevy::prelude::*;

pub mod cns;
pub mod config;
pub mod execution;
pub mod metabolism;
pub mod territoriality;
pub mod urgency;

pub struct NervousSystemPlugin;

impl Plugin for NervousSystemPlugin {
    fn build(&self, app: &mut App) {
        use crate::core::not_paused;

        app.register_type::<cns::CentralNervousSystem>()
            .register_type::<Goal>()
            .register_type::<TriplePattern>()
            .register_type::<ActionTemplate>()
            .init_resource::<config::NervousSystemConfig>()
            .init_resource::<crate::agent::brains::planner::PlannerConfig>()
            .init_resource::<crate::agent::mind::memory::MemoryDecayConfig>()
            .add_systems(
                FixedUpdate,
                (
                    metabolism::tick_metabolism,
                    crate::agent::body::wakefulness::tick_wakefulness
                        .after(metabolism::tick_metabolism),
                )
                    .in_set(crate::core::PerfBucket::Biology)
                    .run_if(not_paused),
            )
            .add_systems(
                FixedUpdate,
                territoriality::update_territoriality
                    .in_set(crate::core::PerfBucket::Psyche)
                    .after(metabolism::tick_metabolism)
                    .after(crate::agent::mind::perception::write_perceptions_to_mind)
                    .run_if(not_paused),
            )
            .add_systems(
                FixedUpdate,
                urgency::generate_urgency
                    .in_set(crate::core::PerfBucket::Brain)
                    .in_set(crate::core::PerfSubBucket::BrainUrgency)
                    .after(territoriality::update_territoriality)
                    .run_if(not_paused),
            );
    }
}
