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
pub mod social_initiation;
pub mod survival;
pub mod target_enumeration;
pub mod thinking;
pub mod threat_appraisal;
pub mod trace;
pub mod wakeup;

// Internal Tests moved inline

/// How often the brain runs, in FixedUpdate ticks. Default is 6 (10 Hz at
/// 60 TPS) — fast enough that nobody perceives "slow" reaction (~100 ms)
/// while cutting brain cost ~6× vs every-tick arbitration. Tests that
/// need every-tick decisions for tight assertions can set this to 1.
#[derive(Resource, Debug, Clone, Copy)]
pub struct BrainTickInterval(pub u64);

impl Default for BrainTickInterval {
    fn default() -> Self {
        Self(6)
    }
}

impl BrainTickInterval {
    pub fn is_due(&self, tick: u64) -> bool {
        tick.is_multiple_of(self.0.max(1))
    }
}

fn brain_tick_due(
    tick: Res<crate::core::tick::TickCount>,
    interval: Res<BrainTickInterval>,
) -> bool {
    interval.is_due(tick.current)
}

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
            .register_type::<social_initiation::SocialInitiationCooldowns>()
            .init_resource::<BrainTickInterval>()
            .init_resource::<wakeup::PendingBrainWakeups>()
            .init_resource::<trace::TraceConfig>()
            .init_resource::<trace::DecisionTraceBuffer>()
            .init_resource::<wakeup::UrgencyBandHistory>()
            .init_resource::<wakeup::PerceptionHistory>()
            .add_systems(
                FixedUpdate,
                (
                    wakeup::emit_initial_wakeups,
                    wakeup::emit_action_lifecycle_wakeups,
                    wakeup::emit_drive_threshold_wakeups,
                    wakeup::emit_new_perception_wakeups,
                    wakeup::emit_knowledge_change_wakeups,
                    wakeup::emit_conversation_state_wakeups,
                    wakeup::emit_periodic_safety_wakeups,
                )
                    .in_set(crate::core::PerfBucket::Brain)
                    .in_set(crate::core::PerfSubBucket::BrainArbitration)
                    .before(rational::update_rational_planning)
                    .before(brain_system::arbitrate_every_tick)
                    .after(crate::agent::nervous_system::urgency::generate_urgency)
                    .run_if(not_paused),
            )
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
                    .after(wakeup::emit_initial_wakeups)
                    .after(wakeup::emit_action_lifecycle_wakeups)
                    .after(wakeup::emit_drive_threshold_wakeups)
                    .after(wakeup::emit_new_perception_wakeups)
                    .after(wakeup::emit_knowledge_change_wakeups)
                    .after(wakeup::emit_conversation_state_wakeups)
                    .after(wakeup::emit_periodic_safety_wakeups)
                    .run_if(not_paused)
                    .run_if(brain_tick_due),
            )
            .add_systems(
                FixedUpdate,
                brain_system::tick_cognitive_drain
                    .in_set(crate::core::PerfBucket::Brain)
                    .in_set(crate::core::PerfSubBucket::BrainArbitration)
                    .run_if(not_paused),
            )
            .add_systems(
                FixedUpdate,
                brain_system::emit_agent_state_hash
                    .in_set(crate::core::PerfBucket::Brain)
                    .in_set(crate::core::PerfSubBucket::BrainArbitration)
                    .after(brain_system::arbitrate_every_tick)
                    .run_if(not_paused),
            )
            .add_systems(
                FixedUpdate,
                social_initiation::record_social_initiation_failures
                    .in_set(crate::core::PerfBucket::Brain)
                    .in_set(crate::core::PerfSubBucket::BrainArbitration)
                    .after(crate::agent::nervous_system::execution::apply_action_effects)
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
