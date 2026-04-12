use bevy::prelude::*;

pub mod arbitration;

pub mod brain_system;
pub mod emotional;
pub mod history;
pub mod plan_memory;
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
            .register_type::<plan_memory::PlanMemory>()
            .register_type::<proposal::BrainState>()
            .register_type::<proposal::BrainType>()
            .register_type::<proposal::BrainPowers>()
            .register_type::<history::BrainHistory>()
            .init_resource::<trace::TraceConfig>()
            .init_resource::<trace::DecisionTraceBuffer>()
            .add_systems(
                Update,
                (
                    rational::update_rational_planning,
                    brain_system::arbitrate_every_tick,
                    // Note: start_actions is now in AgentPlugin to run after brain decides
                )
                    .chain() // planning runs before arbitration so fresh plan steps surface same-tick
                    // Brains read CentralNervousSystem (goals, urgency) written by
                    // formulate_goals — without this Bevy's multi-threaded scheduler
                    // may run the brain before goals are updated for this tick,
                    // causing stale state to drive incorrect action proposals. The
                    // perception → brain ordering is implied transitively because
                    // formulate_goals already runs after write_perceptions_to_mind.
                    .after(crate::agent::nervous_system::cns::formulate_goals)
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
