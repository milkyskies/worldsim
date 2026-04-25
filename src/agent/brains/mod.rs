use bevy::prelude::*;

pub mod arbitration;

pub mod brain_system;
pub mod drift;
pub mod emotional;
pub mod history;
pub mod plan_memory;
pub mod planner;
pub mod proposal;
pub mod rational;
pub mod survival;
pub mod target_enumeration;
pub mod thinking;
pub mod threat_appraisal;
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
                FixedUpdate,
                rational::update_rational_planning
                    .in_set(crate::core::PerfBucket::Brain)
                    .in_set(crate::core::PerfSubBucket::BrainPlanning)
                    .before(brain_system::arbitrate_every_tick)
                    .after(crate::agent::nervous_system::urgency::generate_urgency)
                    .run_if(not_paused),
            )
            .add_systems(
                FixedUpdate,
                brain_system::arbitrate_every_tick
                    .in_set(crate::core::PerfBucket::Brain)
                    .in_set(crate::core::PerfSubBucket::BrainArbitration)
                    .after(crate::agent::nervous_system::urgency::generate_urgency)
                    .run_if(not_paused),
            )
            .add_systems(
                FixedUpdate,
                history::update_brain_history
                    .in_set(crate::core::PerfBucket::Brain)
                    .in_set(crate::core::PerfSubBucket::BrainHistory)
                    .after(crate::agent::nervous_system::execution::apply_action_effects)
                    .run_if(not_paused),
            )
            // Trace system runs in Last to read all SimEvents emitted during Update.
            .add_systems(Last, trace::update_decision_trace.run_if(not_paused));
    }
}
