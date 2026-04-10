use bevy::prelude::*;

pub mod active_plan;
pub mod arbitration;

pub mod brain_system;
pub mod emotional;
pub mod history;
// pub mod exploration; // REMOVED
pub mod planner;
pub mod proposal;
pub mod rational;
pub mod survival;
pub mod target_enumeration;
pub mod thinking;
pub mod trace;

// Internal Tests moved inline

pub struct BrainPlugin;

impl Plugin for BrainPlugin {
    fn build(&self, app: &mut App) {
        use crate::core::not_paused;

        app.register_type::<rational::RationalBrain>()
            .register_type::<proposal::BrainState>()
            .register_type::<proposal::BrainType>()
            .register_type::<proposal::BrainPowers>()
            .register_type::<history::BrainHistory>()
            .register_type::<active_plan::ActivePlans>()
            .init_resource::<trace::TraceConfig>()
            .init_resource::<trace::DecisionTraceBuffer>()
            .add_systems(
                Update,
                (
                    rational::update_rational_brain,
                    brain_system::three_brains_system,
                    // Note: start_actions is now in AgentPlugin to run after brain decides
                )
                    .chain() // update_rational_brain runs before three_brains_system
                    // Brain decision-making must read fresh perception data each
                    // tick. Without this explicit ordering Bevy is free to schedule
                    // the brain chain before perception (they only conflict on
                    // MindGraph access), which silently breaks reactive behavior
                    // including conversation initiation.
                    .after(crate::agent::mind::perception::write_perceptions_to_mind)
                    .run_if(not_paused), // ALL brain systems pause together
            )
            .add_systems(
                Update,
                history::update_brain_history
                    .after(crate::agent::nervous_system::execution::apply_action_effects)
                    .run_if(not_paused),
            )
            // Trace system runs in Last to read all SimEvents emitted during Update.
            .add_systems(Last, trace::update_decision_trace.run_if(not_paused));
    }
}
