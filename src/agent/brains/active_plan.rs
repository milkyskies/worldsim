//! Active plan ownership and commitment: gives in-progress plans inertia to prevent flip-flopping.
//!
//! When a brain proposal wins arbitration and starts executing, it becomes an `ActivePlan`
//! owned by that brain. Active plans get a score bonus when the same brain re-proposes them,
//! and other brains have to clearly beat them to displace. Commitment strength decays when
//! the plan stalls and grows when making progress.
//!
//! Note: the nervous system's urgency momentum (`apply_momentum_and_gating`) also provides
//! inertia at the drive level. This module operates at the plan/action level — the two
//! stack multiplicatively (momentum inflates urgency, which feeds into proposal urgency,
//! which then gets the commitment bonus). Both are needed: momentum prevents drive-level
//! oscillation, commitment prevents plan-level flip-flopping.
//!
//! Reads: BrainState (chosen actions), ActionType, Personality (conscientiousness)
//! Writes: ActivePlan component
//! Upstream: arbitration (applies commitment bonus), brain_system (updates active plans)
//! Downstream: SimEvent::PlanAbandoned (emitted when a plan is replaced or stalls out)

use super::proposal::{BrainType, Intent};
use crate::agent::actions::ActionType;
use bevy::prelude::*;

/// Who owns an active plan.
#[derive(Debug, Clone, PartialEq, Reflect)]
pub enum PlanOwner {
    /// A brain proposed and won arbitration for this plan.
    Brain(BrainType),
    /// A verbal commitment made to another agent (for future #63 integration).
    Verbal {
        promised_to: Entity,
        agreement_tick: u64,
    },
    /// Self-directed commitment (e.g. internal resolve).
    #[allow(dead_code)]
    Self_,
}

/// How far along an active plan is.
#[derive(Debug, Clone, PartialEq, Reflect)]
pub enum PlanProgress {
    /// Just started this tick.
    Starting,
    /// Actively executing and making progress.
    Executing,
    /// No progress for some ticks.
    Stalled { since: u64 },
}

/// An active plan that the agent is currently committed to.
#[derive(Debug, Clone, Reflect)]
pub struct ActivePlanEntry {
    /// Who owns this plan.
    pub owner: PlanOwner,
    /// What drive it serves.
    pub intent: Intent,
    /// The committed action type.
    pub action: ActionType,
    /// Tick when this plan started.
    pub started_at: u64,
    /// Current progress state.
    pub progress: PlanProgress,
    /// Commitment strength: 0.0..1.0. Decays when stalled, stays high when progressing.
    pub commitment_strength: f32,
}

/// Per-tick decay rate for commitment strength when stalled.
pub const STALL_DECAY_RATE: f32 = 0.15;

/// Commitment strength below which the plan is automatically abandoned.
pub const ABANDON_THRESHOLD: f32 = 0.1;

/// Per-tick growth rate for commitment strength when making progress.
pub const PROGRESS_GROWTH_RATE: f32 = 0.05;

/// Cooldown ticks after abandoning a plan — the same action gets no inertia bonus during this period.
pub const ABANDONMENT_COOLDOWN_TICKS: u64 = 5;

/// Component tracking an agent's active plans.
#[derive(Component, Debug, Clone, Reflect, Default)]
#[reflect(Component)]
pub struct ActivePlans {
    /// Currently active plans keyed by intent.
    #[reflect(ignore)]
    pub plans: Vec<ActivePlanEntry>,
    /// Recently abandoned actions with cooldown expiry tick.
    #[reflect(ignore)]
    pub cooldowns: Vec<(ActionType, u64)>,
}

impl ActivePlans {
    /// Calculate the commitment bonus for a proposal that matches an active plan.
    ///
    /// Returns 0.0 if there's no matching active plan or if the action is on cooldown.
    /// The bonus is `commitment_strength * conscientiousness`, meaning conscientious
    /// agents stick with plans longer.
    pub fn commitment_bonus(
        &self,
        intent: Intent,
        action: ActionType,
        owner: &PlanOwner,
        conscientiousness: f32,
        current_tick: u64,
    ) -> f32 {
        // Check cooldown — recently abandoned actions get no bonus
        if self
            .cooldowns
            .iter()
            .any(|(a, expiry)| *a == action && current_tick < *expiry)
        {
            return 0.0;
        }

        // Find matching active plan
        if let Some(plan) = self
            .plans
            .iter()
            .find(|p| p.intent == intent && p.action == action && p.owner == *owner)
        {
            plan.commitment_strength * conscientiousness
        } else {
            0.0
        }
    }

    /// Register a new active plan or update an existing one for the same intent.
    pub fn activate(&mut self, owner: PlanOwner, intent: Intent, action: ActionType, tick: u64) {
        // If there's already an active plan for this intent, check if it's the same action
        if let Some(existing) = self.plans.iter_mut().find(|p| p.intent == intent) {
            if existing.action == action && existing.owner == owner {
                // Same plan — mark as progressing
                existing.progress = PlanProgress::Executing;
                existing.commitment_strength =
                    (existing.commitment_strength + PROGRESS_GROWTH_RATE).min(1.0);
                return;
            }
            // Different plan for same intent — will be replaced below after removal
        }

        // Remove any existing plan for this intent
        self.plans.retain(|p| p.intent != intent);

        self.plans.push(ActivePlanEntry {
            owner,
            intent,
            action,
            started_at: tick,
            progress: PlanProgress::Starting,
            commitment_strength: 0.8, // Start with high commitment
        });
    }

    /// Mark plans as stalled if their action wasn't re-admitted this tick.
    /// Returns a list of (intent, action) pairs that were abandoned.
    pub fn decay_stalled_plans(
        &mut self,
        admitted_actions: &[ActionType],
        current_tick: u64,
    ) -> Vec<(Intent, ActionType)> {
        let mut abandoned = Vec::new();

        for plan in &mut self.plans {
            if admitted_actions.contains(&plan.action) {
                // Plan is still being executed — keep progressing
                if matches!(plan.progress, PlanProgress::Stalled { .. }) {
                    plan.progress = PlanProgress::Executing;
                }
                plan.commitment_strength =
                    (plan.commitment_strength + PROGRESS_GROWTH_RATE).min(1.0);
            } else {
                // Plan wasn't re-admitted — mark as stalling
                match &plan.progress {
                    PlanProgress::Starting | PlanProgress::Executing => {
                        plan.progress = PlanProgress::Stalled {
                            since: current_tick,
                        };
                        plan.commitment_strength -= STALL_DECAY_RATE;
                    }
                    PlanProgress::Stalled { .. } => {
                        plan.commitment_strength -= STALL_DECAY_RATE;
                    }
                }
            }
        }

        // Abandon plans that fell below threshold
        self.plans.retain(|plan| {
            if plan.commitment_strength < ABANDON_THRESHOLD {
                abandoned.push((plan.intent, plan.action));
                false
            } else {
                true
            }
        });

        // Add cooldowns for abandoned actions
        for (_, action) in &abandoned {
            self.cooldowns
                .push((*action, current_tick + ABANDONMENT_COOLDOWN_TICKS));
        }

        // Clean up expired cooldowns
        self.cooldowns.retain(|(_, expiry)| current_tick < *expiry);

        abandoned
    }

    /// Mark a plan as completed and remove it.
    pub fn complete(&mut self, intent: Intent) {
        self.plans.retain(|p| p.intent != intent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commitment_bonus_returns_zero_without_active_plan() {
        let plans = ActivePlans::default();
        let bonus = plans.commitment_bonus(
            Intent::SatisfyHunger,
            ActionType::Walk,
            &PlanOwner::Brain(BrainType::Rational),
            0.8,
            10,
        );
        assert_eq!(bonus, 0.0);
    }

    #[test]
    fn commitment_bonus_scales_with_conscientiousness() {
        let mut plans = ActivePlans::default();
        plans.activate(
            PlanOwner::Brain(BrainType::Rational),
            Intent::SatisfyHunger,
            ActionType::Walk,
            0,
        );

        let bonus_high = plans.commitment_bonus(
            Intent::SatisfyHunger,
            ActionType::Walk,
            &PlanOwner::Brain(BrainType::Rational),
            1.0,
            5,
        );
        let bonus_low = plans.commitment_bonus(
            Intent::SatisfyHunger,
            ActionType::Walk,
            &PlanOwner::Brain(BrainType::Rational),
            0.2,
            5,
        );

        assert!(
            bonus_high > bonus_low,
            "high conscientiousness ({bonus_high}) should give more bonus than low ({bonus_low})"
        );
    }

    #[test]
    fn stalled_plan_decays_commitment() {
        let mut plans = ActivePlans::default();
        plans.activate(
            PlanOwner::Brain(BrainType::Rational),
            Intent::SatisfyHunger,
            ActionType::Walk,
            0,
        );

        let initial_strength = plans.plans[0].commitment_strength;

        // Not re-admitted for several ticks
        plans.decay_stalled_plans(&[], 1);
        plans.decay_stalled_plans(&[], 2);

        assert!(
            plans.plans[0].commitment_strength < initial_strength,
            "commitment should decay when stalled"
        );
    }

    #[test]
    fn stalled_plan_eventually_abandoned() {
        let mut plans = ActivePlans::default();
        plans.activate(
            PlanOwner::Brain(BrainType::Rational),
            Intent::SatisfyHunger,
            ActionType::Walk,
            0,
        );

        // Decay until abandoned
        for tick in 1..20 {
            let abandoned = plans.decay_stalled_plans(&[], tick);
            if !abandoned.is_empty() {
                assert_eq!(abandoned[0].0, Intent::SatisfyHunger);
                assert_eq!(abandoned[0].1, ActionType::Walk);
                assert!(plans.plans.is_empty(), "abandoned plan should be removed");
                return;
            }
        }
        panic!("plan should have been abandoned after enough stall ticks");
    }

    #[test]
    fn progressing_plan_maintains_commitment() {
        let mut plans = ActivePlans::default();
        plans.activate(
            PlanOwner::Brain(BrainType::Rational),
            Intent::SatisfyHunger,
            ActionType::Walk,
            0,
        );

        let initial_strength = plans.plans[0].commitment_strength;

        // Re-admitted every tick
        for tick in 1..10 {
            plans.decay_stalled_plans(&[ActionType::Walk], tick);
        }

        assert!(
            plans.plans[0].commitment_strength >= initial_strength,
            "commitment should not decay when plan is progressing"
        );
    }

    #[test]
    fn completed_plan_is_removed() {
        let mut plans = ActivePlans::default();
        plans.activate(
            PlanOwner::Brain(BrainType::Rational),
            Intent::SatisfyHunger,
            ActionType::Walk,
            0,
        );

        plans.complete(Intent::SatisfyHunger);
        assert!(plans.plans.is_empty());
    }

    #[test]
    fn cooldown_prevents_bonus_after_abandonment() {
        let mut plans = ActivePlans::default();
        plans.activate(
            PlanOwner::Brain(BrainType::Rational),
            Intent::SatisfyHunger,
            ActionType::Walk,
            0,
        );

        // Force abandonment by stalling — track when it actually happens
        let mut abandon_tick = 0;
        for tick in 1..20 {
            let abandoned = plans.decay_stalled_plans(&[], tick);
            if !abandoned.is_empty() {
                abandon_tick = tick;
                break;
            }
        }
        assert!(abandon_tick > 0, "plan should have been abandoned");

        // Re-activate the same action immediately after abandonment
        let reactivate_tick = abandon_tick + 1;
        plans.activate(
            PlanOwner::Brain(BrainType::Rational),
            Intent::SatisfyHunger,
            ActionType::Walk,
            reactivate_tick,
        );

        // During cooldown window, bonus should be 0
        let bonus = plans.commitment_bonus(
            Intent::SatisfyHunger,
            ActionType::Walk,
            &PlanOwner::Brain(BrainType::Rational),
            1.0,
            reactivate_tick, // within cooldown window
        );
        assert_eq!(bonus, 0.0, "should get no bonus during cooldown period");

        // After cooldown expires, bonus should be positive
        let after_cooldown_tick = abandon_tick + ABANDONMENT_COOLDOWN_TICKS + 1;
        // Clean up expired cooldowns
        plans.decay_stalled_plans(&[ActionType::Walk], after_cooldown_tick);
        let bonus_after = plans.commitment_bonus(
            Intent::SatisfyHunger,
            ActionType::Walk,
            &PlanOwner::Brain(BrainType::Rational),
            1.0,
            after_cooldown_tick,
        );
        assert!(
            bonus_after > 0.0,
            "bonus should be positive after cooldown expires"
        );
    }

    #[test]
    fn verbal_commitment_gets_inertia() {
        let promised_to = Entity::from_bits(42);
        let mut plans = ActivePlans::default();
        plans.activate(
            PlanOwner::Verbal {
                promised_to,
                agreement_tick: 0,
            },
            Intent::SatisfyHunger,
            ActionType::Harvest,
            0,
        );

        let bonus = plans.commitment_bonus(
            Intent::SatisfyHunger,
            ActionType::Harvest,
            &PlanOwner::Verbal {
                promised_to,
                agreement_tick: 0,
            },
            0.8,
            5,
        );

        assert!(
            bonus > 0.0,
            "verbal commitments should get inertia bonus, got {bonus}"
        );
    }

    #[test]
    fn different_action_same_intent_replaces_plan() {
        let mut plans = ActivePlans::default();
        plans.activate(
            PlanOwner::Brain(BrainType::Rational),
            Intent::SatisfyHunger,
            ActionType::Walk,
            0,
        );
        plans.activate(
            PlanOwner::Brain(BrainType::Rational),
            Intent::SatisfyHunger,
            ActionType::Explore,
            5,
        );

        assert_eq!(plans.plans.len(), 1);
        assert_eq!(plans.plans[0].action, ActionType::Explore);
    }
}
