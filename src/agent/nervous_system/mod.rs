use crate::agent::brains::thinking::{ActionTemplate, Goal, TriplePattern};
use bevy::prelude::*;

pub mod activity_effects;
pub mod cns;
pub mod config;
pub mod execution;
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
            .init_resource::<crate::agent::activity::ActivityConfig>()
            .init_resource::<crate::agent::brains::planner::PlannerConfig>()
            .init_resource::<crate::agent::mind::memory::MemoryDecayConfig>()
            .add_systems(
                Update,
                (
                    // BMR drain + stomach digestion + glucose/reserves
                    // overflow runs every tick for every agent with
                    // PhysicalNeeds. Lives separately from
                    // apply_activity_effects so agents that don't carry
                    // the legacy `CurrentActivity` component still
                    // metabolize (#416).
                    activity_effects::tick_metabolism,
                    crate::agent::body::wakefulness::tick_wakefulness
                        .after(activity_effects::tick_metabolism),
                    activity_effects::apply_activity_effects
                        .after(activity_effects::tick_metabolism),
                    // Territoriality reads MindGraph, which is written by
                    // write_perceptions_to_mind. Without this explicit edge Bevy
                    // may schedule the goals chain before perception, producing
                    // stale urgency values that cause agents to pick Wander
                    // instead of InitiateConversation on the first decision tick.
                    // Goal formulation inherits the ordering transitively via the
                    // territoriality → urgency → formulate_goals chain.
                    territoriality::update_territoriality
                        .after(activity_effects::apply_activity_effects)
                        .after(crate::agent::mind::perception::write_perceptions_to_mind),
                    urgency::generate_urgency.after(territoriality::update_territoriality),
                    cns::formulate_goals.after(urgency::generate_urgency),
                )
                    .run_if(not_paused), // ALL nervous system pauses together
            );
    }
}
