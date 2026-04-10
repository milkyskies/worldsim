//! Commitment substrate: agents promise to pursue a goal, then decay over time.
//!
//! Reads: Commitments, Personality (conscientiousness modulates decay)
//! Writes: Commitments (strength decay, expired commitments removed)
//! Upstream: communication (Intent::Agree creates commitments via verbal agreement)
//! Downstream: nervous_system::cns (commitments override urgency-driven goals
//!             when their priority is higher)
//!
//! A commitment represents "I said I'd do X." It has a strength that starts
//! at 1.0 and decays per tick. Conscientious agents decay slower (stronger
//! follow-through); spontaneous agents decay faster (flakiness). When
//! strength drops below a minimum threshold, the commitment is forgotten.
//!
//! The commitment's *goal* is a [`Concept`] — the type of thing being
//! pursued (e.g. `Campfire`). The CNS maps it to a concrete [`Goal`]
//! pattern using the same recipe machinery the Build action uses.

use bevy::prelude::*;

use crate::agent::mind::knowledge::Concept;
use crate::agent::psyche::personality::Personality;

/// Per-tick decay applied to a fresh commitment at neutral conscientiousness.
const DECAY_BASE: f32 = 0.001;

/// Below this strength, commitments are removed as "forgotten".
const MIN_STRENGTH: f32 = 0.05;

/// Multiplier on conscientiousness that scales commitment priority.
/// `priority = base + strength * conscientiousness * PRIORITY_BONUS`.
pub const PRIORITY_BONUS: f32 = 0.3;

/// Base priority every active commitment contributes, before the
/// conscientiousness-scaled bonus. Keeps committed goals attractive but
/// not so attractive that they override life-threatening needs like
/// hunger or fear.
pub const PRIORITY_BASE: f32 = 0.4;

/// A single promise to pursue a concept-level goal.
#[derive(Debug, Clone, Reflect)]
pub struct Commitment {
    pub goal: Concept,
    pub committed_at: u64,
    /// 0.0..=1.0. Decays over time; high conscientiousness decays slower.
    pub strength: f32,
}

impl Commitment {
    pub fn new(goal: Concept, committed_at: u64) -> Self {
        Self {
            goal,
            committed_at,
            strength: 1.0,
        }
    }

    /// Priority this commitment contributes to goal selection.
    pub fn priority(&self, conscientiousness: f32) -> f32 {
        PRIORITY_BASE + self.strength * conscientiousness * PRIORITY_BONUS
    }
}

/// Component holding all active commitments for an agent.
#[derive(Component, Debug, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct Commitments {
    pub active: Vec<Commitment>,
}

impl Commitments {
    /// Record a new commitment, or refresh strength of an existing one.
    pub fn add(&mut self, goal: Concept, tick: u64) {
        if let Some(existing) = self.active.iter_mut().find(|c| c.goal == goal) {
            existing.strength = 1.0;
            existing.committed_at = tick;
            return;
        }
        self.active.push(Commitment::new(goal, tick));
    }

    /// Return the commitment with the highest current strength, if any.
    pub fn strongest(&self) -> Option<&Commitment> {
        self.active.iter().max_by(|a, b| {
            a.strength
                .partial_cmp(&b.strength)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    /// Decay all commitment strengths by one tick. Lower conscientiousness
    /// decays faster (conscientiousness 0.0 → 2× decay; 1.0 → 1× decay).
    /// Commitments that fall below `MIN_STRENGTH` are removed.
    pub fn decay_tick(&mut self, conscientiousness: f32) {
        let rate = DECAY_BASE * (2.0 - conscientiousness.clamp(0.0, 1.0));
        for c in &mut self.active {
            c.strength = (c.strength - rate).max(0.0);
        }
        self.active.retain(|c| c.strength > MIN_STRENGTH);
    }
}

/// Per-tick system that decays every agent's commitment strengths.
pub fn decay_commitments_system(mut query: Query<(&mut Commitments, &Personality)>) {
    for (mut commitments, personality) in query.iter_mut() {
        commitments.decay_tick(personality.traits.conscientiousness);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_commitment_has_full_strength() {
        let c = Commitment::new(Concept::Campfire, 100);
        assert_eq!(c.strength, 1.0);
        assert_eq!(c.goal, Concept::Campfire);
        assert_eq!(c.committed_at, 100);
    }

    #[test]
    fn adding_same_goal_refreshes_strength() {
        let mut commitments = Commitments::default();
        commitments.add(Concept::Campfire, 0);
        // Decay once so we can observe the refresh.
        commitments.decay_tick(0.5);
        let weaker = commitments.strongest().unwrap().strength;
        commitments.add(Concept::Campfire, 100);
        assert_eq!(
            commitments.active.len(),
            1,
            "re-committing to same goal should not duplicate entries"
        );
        assert!(
            commitments.strongest().unwrap().strength > weaker,
            "re-committing should restore full strength"
        );
        assert_eq!(commitments.strongest().unwrap().committed_at, 100);
    }

    #[test]
    fn decay_reduces_strength() {
        let mut commitments = Commitments::default();
        commitments.add(Concept::Campfire, 0);
        commitments.decay_tick(0.5);
        assert!(commitments.strongest().unwrap().strength < 1.0);
    }

    #[test]
    fn low_conscientiousness_decays_faster_than_high() {
        let mut flaky = Commitments::default();
        flaky.add(Concept::Campfire, 0);
        let mut reliable = Commitments::default();
        reliable.add(Concept::Campfire, 0);

        for _ in 0..50 {
            flaky.decay_tick(0.0);
            reliable.decay_tick(1.0);
        }

        let flaky_strength = flaky.strongest().unwrap().strength;
        let reliable_strength = reliable.strongest().unwrap().strength;
        assert!(
            flaky_strength < reliable_strength,
            "flaky agent ({flaky_strength}) should decay faster than reliable one ({reliable_strength})"
        );
    }

    #[test]
    fn expired_commitments_are_removed() {
        let mut commitments = Commitments::default();
        commitments.add(Concept::Campfire, 0);
        // Decay many times with max-flaky personality until it drops below MIN_STRENGTH.
        for _ in 0..10_000 {
            commitments.decay_tick(0.0);
            if commitments.active.is_empty() {
                break;
            }
        }
        assert!(
            commitments.active.is_empty(),
            "commitment should be forgotten after sustained decay"
        );
    }

    #[test]
    fn priority_increases_with_conscientiousness() {
        let c = Commitment::new(Concept::Campfire, 0);
        let low = c.priority(0.0);
        let high = c.priority(1.0);
        assert!(
            high > low,
            "higher conscientiousness should produce higher commitment priority ({high} vs {low})"
        );
    }
}
