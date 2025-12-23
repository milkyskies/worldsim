use crate::agent::brains::thinking::{ActionTemplate, Goal, TriplePattern};
use bevy::prelude::*;

pub mod activity_effects;
pub mod cns;
pub mod config;
pub mod execution;

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
            .init_resource::<crate::agent::activity::ActivityConfig>()
            .init_resource::<crate::agent::brains::planner::PlannerConfig>()
            .init_resource::<crate::agent::mind::memory::MemoryDecayConfig>()
            .add_systems(
                Update,
                (
                    activity_effects::apply_activity_effects,
                    urgency::generate_urgency.after(activity_effects::apply_activity_effects),
                    cns::formulate_goals.after(urgency::generate_urgency),
                )
                    .run_if(not_paused), // ALL nervous system pauses together
            );
    }
}
