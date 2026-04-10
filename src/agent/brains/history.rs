//! Brain history: tracks per-brain action success rates and applies a modest power multiplier.
//!
//! Reads: ActionOutcomeEvent, BrainHistory (active attributions)
//! Writes: BrainHistory (outcome records)
//! Upstream: nervous_system::execution (emits ActionOutcomeEvent)
//! Downstream: brains::brain_system (reads multipliers before arbitration)

use super::proposal::BrainType;
use crate::agent::actions::ActionType;
use crate::agent::events::{ActionOutcome, ActionOutcomeEvent};
use bevy::prelude::*;
use std::collections::{HashMap, VecDeque};

const HISTORY_WINDOW: usize = 20;

/// Sliding window of recent action outcomes for a single brain.
#[derive(Debug, Clone, Reflect, Default)]
pub struct BrainRecord {
    #[reflect(ignore)]
    outcomes: VecDeque<bool>,
}

impl BrainRecord {
    pub fn record(&mut self, success: bool) {
        if self.outcomes.len() >= HISTORY_WINDOW {
            self.outcomes.pop_front();
        }
        self.outcomes.push_back(success);
    }

    /// Returns the recent success rate. Defaults to 0.5 (neutral) until enough data accumulates.
    pub fn success_rate(&self) -> f32 {
        if self.outcomes.is_empty() {
            return 0.5;
        }
        self.outcomes.iter().filter(|&&s| s).count() as f32 / self.outcomes.len() as f32
    }

    /// Multiplier to apply to brain power: ±10% based on recent success rate.
    /// At 100% success: 1.1, at 50%: 1.0, at 0%: 0.9.
    pub fn power_multiplier(&self) -> f32 {
        1.0 + (self.success_rate() - 0.5) * 0.2
    }
}

/// Per-agent component that tracks recent action success rates per brain type.
///
/// Used to apply a modest power boost/penalty to brains based on how well
/// their proposed actions have been working out. Agents that consistently
/// succeed via rational planning become slightly more rational over time;
/// agents that survive on instinct become slightly more instinctive.
#[derive(Component, Debug, Clone, Reflect, Default)]
#[reflect(Component)]
pub struct BrainHistory {
    pub survival: BrainRecord,
    pub emotional: BrainRecord,
    pub rational: BrainRecord,
    /// Maps currently active action types to the brain that proposed them,
    /// so completed/failed actions can be attributed to the right brain.
    #[reflect(ignore)]
    pub active: HashMap<ActionType, BrainType>,
}

impl BrainHistory {
    pub fn record_for(&mut self, brain: BrainType, success: bool) {
        match brain {
            BrainType::Survival => self.survival.record(success),
            BrainType::Emotional => self.emotional.record(success),
            BrainType::Rational => self.rational.record(success),
        }
    }

    pub fn power_multiplier(&self, brain: BrainType) -> f32 {
        match brain {
            BrainType::Survival => self.survival.power_multiplier(),
            BrainType::Emotional => self.emotional.power_multiplier(),
            BrainType::Rational => self.rational.power_multiplier(),
        }
    }
}

/// Reads action outcome events and updates brain success/failure records.
pub fn update_brain_history(
    mut outcomes: MessageReader<ActionOutcomeEvent>,
    mut query: Query<&mut BrainHistory>,
) {
    for event in outcomes.read() {
        let Ok(mut history) = query.get_mut(event.actor) else {
            continue;
        };
        let (action_type, success) = match &event.outcome {
            ActionOutcome::Success { action, .. } => (*action, true),
            ActionOutcome::Failed { action, .. } => (*action, false),
        };
        if let Some(brain) = history.active.get(&action_type).copied() {
            history.record_for(brain, success);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_record_is_neutral() {
        let record = BrainRecord::default();
        assert_eq!(record.success_rate(), 0.5);
        assert_eq!(record.power_multiplier(), 1.0);
    }

    #[test]
    fn perfect_success_boosts_power() {
        let mut record = BrainRecord::default();
        for _ in 0..10 {
            record.record(true);
        }
        assert!((record.success_rate() - 1.0).abs() < f32::EPSILON);
        assert!((record.power_multiplier() - 1.1).abs() < f32::EPSILON);
    }

    #[test]
    fn total_failure_penalizes_power() {
        let mut record = BrainRecord::default();
        for _ in 0..10 {
            record.record(false);
        }
        assert!(record.success_rate() < f32::EPSILON);
        assert!((record.power_multiplier() - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn window_evicts_oldest_outcomes() {
        let mut record = BrainRecord::default();
        // Fill with failures
        for _ in 0..HISTORY_WINDOW {
            record.record(false);
        }
        // Then fill with successes — old failures should be gone
        for _ in 0..HISTORY_WINDOW {
            record.record(true);
        }
        assert!((record.success_rate() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn history_routes_to_correct_brain() {
        let mut history = BrainHistory::default();
        history.record_for(BrainType::Rational, true);
        history.record_for(BrainType::Rational, true);
        history.record_for(BrainType::Survival, false);

        assert!((history.rational.success_rate() - 1.0).abs() < f32::EPSILON);
        assert!(history.rational.power_multiplier() > 1.0);

        assert!(history.survival.success_rate() < f32::EPSILON);
        assert!(history.survival.power_multiplier() < 1.0);

        // Emotional brain untouched — should remain neutral
        assert_eq!(history.emotional.success_rate(), 0.5);
    }
}
