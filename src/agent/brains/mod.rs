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

// Internal Tests moved inline

pub struct BrainPlugin;

impl Plugin for BrainPlugin {
    fn build(&self, app: &mut App) {
        use crate::core::not_paused;

        app.register_type::<rational::RationalBrain>()
            .register_type::<proposal::BrainState>()
            .register_type::<proposal::BrainType>()
            .register_type::<proposal::BrainPowers>()
            .add_systems(
                Update,
                (
                    rational::update_rational_brain,
                    brain_system::three_brains_system,
                    // Note: start_actions is now in AgentPlugin to run after brain decides
                )
                    .run_if(not_paused), // ALL brain systems pause together
            );
    }
}
