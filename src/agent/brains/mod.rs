use bevy::prelude::*;

pub mod arbitration;

pub mod brain_system;
pub mod emotional;
// pub mod exploration; // REMOVED
pub mod planner;
pub mod proposal;
pub mod rational;
pub mod survival;
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
                    .run_if(not_paused), // ALL brain systems pause together
            )
            // Trace system runs in Last to read all SimEvents emitted during Update.
            .add_systems(Last, trace::update_decision_trace.run_if(not_paused));
    }
}
