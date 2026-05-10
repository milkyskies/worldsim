//! Affective theory of mind: per-agent store of last-observed mood / emotion.
//!
//! Reads: VisibleObjects, MindGraph (sentience filter), EmotionalState, TickCount
//! Writes: AffectiveToM, SimEvent
//! Upstream: mind::social_perception
//! Downstream: psyche::appraisal, prosocial behaviors, other-regarding drives

use bevy::prelude::*;
use std::collections::HashMap;

use crate::agent::Agent;
use crate::agent::events::{SimEvent, SimEventKind};
use crate::agent::mind::knowledge::{Concept, MindGraph, Node};
use crate::agent::mind::perception::VisibleObjects;
use crate::agent::psyche::emotions::{EmotionType, EmotionalState};
use crate::core::GameTime;
use crate::core::tick::{TICK_RARE_PERIOD, TickCount};

/// Cap on distinct targets per observer. Matches the first-order ToM
/// per-target cap so memory stays predictable as the social graph grows.
pub const MAX_AFFECTIVE_TARGETS: usize = 32;

/// Game-time over which observation confidence decays linearly to zero.
/// 24 game-hours: by next morning, what an agent saw of someone yesterday
/// is fully faded.
pub const CONFIDENCE_DECAY_TICKS: u64 = 24 * GameTime::TICKS_PER_HOUR;

/// Confidence floor below which an entry is pruned during decay.
pub const MIN_CONFIDENCE: f32 = 0.1;

/// Mood at or below this counts as "distressed" even when no acute
/// emotion is active — captures the drained / numb / stressed-out band.
const DISTRESSED_MOOD: f32 = -0.3;

/// Snapshot of another agent's affective state at a single observation.
/// Confidence is derived from `observed_at` via `confidence_at(now)` —
/// not stored, to avoid stale-by-default state.
#[derive(Debug, Clone, Copy)]
pub struct PerceivedMood {
    pub dominant_emotion: Option<EmotionType>,
    /// Valence at observation, range -1.0..=1.0.
    pub mood: f32,
    /// Stress at observation, range 0..=100.
    pub stress: f32,
    pub observed_at: u64,
}

impl PerceivedMood {
    /// Linearly-decayed confidence at `now`, clamped to [0, 1].
    pub fn confidence_at(&self, now: u64) -> f32 {
        let age = now.saturating_sub(self.observed_at) as f32;
        (1.0 - age / CONFIDENCE_DECAY_TICKS as f32).clamp(0.0, 1.0)
    }
}

#[derive(Component, Debug, Default, Clone, Reflect)]
#[reflect(Component)]
pub struct AffectiveToM {
    #[reflect(ignore)]
    beliefs: HashMap<Entity, PerceivedMood>,
}

impl AffectiveToM {
    /// Record what `target` looked like emotionally at `tick`. Refreshes
    /// the existing entry; on capacity overflow evicts the oldest target.
    pub fn record_observation(
        &mut self,
        target: Entity,
        dominant_emotion: Option<EmotionType>,
        mood: f32,
        stress: f32,
        tick: u64,
    ) {
        let entry = PerceivedMood {
            dominant_emotion,
            mood,
            stress,
            observed_at: tick,
        };

        if let Some(existing) = self.beliefs.get_mut(&target) {
            *existing = entry;
            return;
        }

        if self.beliefs.len() >= MAX_AFFECTIVE_TARGETS
            && let Some(oldest) = self
                .beliefs
                .iter()
                .min_by_key(|(_, e)| e.observed_at)
                .map(|(t, _)| *t)
        {
            self.beliefs.remove(&oldest);
        }

        self.beliefs.insert(target, entry);
    }

    pub fn perceived_mood(&self, target: Entity) -> Option<&PerceivedMood> {
        self.beliefs.get(&target)
    }

    /// True iff the last observation of `target` shows distress: a
    /// negative-valence dominant emotion, or — when no acute emotion is
    /// active — a mood at or below `DISTRESSED_MOOD`. The mood-only
    /// branch catches "drained / quietly miserable", which has no
    /// acute-emotion signature.
    pub fn has_seen_distressed(&self, target: Entity) -> bool {
        let Some(mood) = self.beliefs.get(&target) else {
            return false;
        };
        let by_emotion = matches!(
            mood.dominant_emotion,
            Some(EmotionType::Sadness | EmotionType::Fear | EmotionType::Anger)
        );
        by_emotion || mood.mood <= DISTRESSED_MOOD
    }

    pub fn target_count(&self) -> usize {
        self.beliefs.len()
    }

    /// Drop entries whose confidence at `now` has fallen below
    /// `MIN_CONFIDENCE`. Returns the number of evictions.
    pub fn decay(&mut self, now: u64) -> usize {
        let before = self.beliefs.len();
        self.beliefs
            .retain(|_, mood| mood.confidence_at(now) >= MIN_CONFIDENCE);
        before - self.beliefs.len()
    }
}

/// Per-observer system: sample affective state of every visible sentient
/// agent and record it on the observer's `AffectiveToM`. Runs every
/// tick — work per observer is `visible-sentient-count` HashMap ops.
/// SimEvents are gated to dominant-emotion changes so the log isn't
/// flooded with no-ops.
pub fn update_affective_tom(
    mut observers: Query<(Entity, &VisibleObjects, &MindGraph, &mut AffectiveToM), With<Agent>>,
    targets: Query<&EmotionalState, With<Agent>>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<SimEvent>,
) {
    let now = tick.current;

    for (observer, visible, mind, mut tom) in observers.iter_mut() {
        for visible_entity in
            visible.iter_by_concept(|c| mind.has_trait(&Node::Concept(c), Concept::Sentient))
        {
            if visible_entity == observer {
                continue;
            }
            let Ok(state) = targets.get(visible_entity) else {
                continue;
            };

            // Change-detection: only emit a SimEvent when the dominant
            // emotion flips. Mood / stress drift quietly under the live
            // sample but don't deserve their own log line.
            let new_emotion = state.dominant_emotion();
            let prev_emotion = tom
                .perceived_mood(visible_entity)
                .and_then(|m| m.dominant_emotion);

            tom.record_observation(
                visible_entity,
                new_emotion,
                state.current_mood,
                state.stress_level,
                now,
            );

            if new_emotion != prev_emotion {
                sim_events.write(SimEvent::single(
                    now,
                    observer,
                    SimEventKind::AffectiveToMUpdated {
                        agent: observer,
                        about: visible_entity,
                    },
                ));
            }
        }
    }
}

/// Slow staggered prune of stale entries. Cadence shares
/// `TICK_RARE_PERIOD` with the update system so each observer's
/// AffectiveToM is touched at most once per period.
pub fn decay_affective_tom(
    mut observers: Query<(Entity, &mut AffectiveToM), With<Agent>>,
    tick: Res<TickCount>,
) {
    for (entity, mut tom) in observers.iter_mut() {
        if !tick.should_run(entity, TICK_RARE_PERIOD) {
            continue;
        }
        tom.decay(tick.current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::psyche::emotions::Emotion;

    fn test_entity(id: u32) -> Entity {
        Entity::from_bits(id as u64)
    }

    #[test]
    fn recording_a_sad_target_stores_sadness_as_dominant_emotion() {
        let mut tom = AffectiveToM::default();
        let alice = test_entity(1);
        let mut state = EmotionalState::default();
        state.add_emotion(Emotion::new(EmotionType::Sadness, 0.7));

        tom.record_observation(alice, state.dominant_emotion(), -0.4, 30.0, 100);

        let mood = tom.perceived_mood(alice).expect("must record observation");
        assert_eq!(mood.dominant_emotion, Some(EmotionType::Sadness));
        assert!((mood.mood - -0.4).abs() < 1e-6);
        assert_eq!(mood.observed_at, 100);
    }

    #[test]
    fn never_observed_target_returns_none() {
        let tom = AffectiveToM::default();
        assert!(tom.perceived_mood(test_entity(99)).is_none());
        assert!(!tom.has_seen_distressed(test_entity(99)));
    }

    #[test]
    fn confidence_decays_linearly_with_age() {
        let mut tom = AffectiveToM::default();
        let alice = test_entity(1);
        tom.record_observation(alice, None, 0.0, 0.0, 0);

        let mood = tom.perceived_mood(alice).unwrap();
        let halfway = mood.confidence_at(CONFIDENCE_DECAY_TICKS / 2);
        assert!(
            (halfway - 0.5).abs() < 1e-3,
            "halfway through decay window should give ~0.5, got {halfway}"
        );
        assert!(mood.confidence_at(CONFIDENCE_DECAY_TICKS * 2).abs() < 1e-6);
    }

    #[test]
    fn decay_evicts_entries_below_threshold() {
        let mut tom = AffectiveToM::default();
        let alice = test_entity(1);
        tom.record_observation(alice, None, 0.0, 0.0, 0);

        let evicted = tom.decay(CONFIDENCE_DECAY_TICKS + 1);
        assert_eq!(evicted, 1);
        assert!(tom.perceived_mood(alice).is_none());
    }

    #[test]
    fn capacity_evicts_oldest_target() {
        let mut tom = AffectiveToM::default();
        // Entity::from_bits(0) is invalid in Bevy 0.18 — start ids at 1.
        for i in 1..=MAX_AFFECTIVE_TARGETS {
            tom.record_observation(test_entity(i as u32), None, 0.0, 0.0, i as u64);
        }
        assert_eq!(tom.target_count(), MAX_AFFECTIVE_TARGETS);

        let newcomer = test_entity(999);
        tom.record_observation(newcomer, None, 0.0, 0.0, 1000);
        assert_eq!(tom.target_count(), MAX_AFFECTIVE_TARGETS);
        assert!(tom.perceived_mood(test_entity(1)).is_none());
        assert!(tom.perceived_mood(newcomer).is_some());
    }

    #[test]
    fn re_observing_same_target_refreshes_in_place() {
        let mut tom = AffectiveToM::default();
        let alice = test_entity(1);
        tom.record_observation(alice, Some(EmotionType::Sadness), -0.5, 40.0, 100);
        tom.record_observation(alice, Some(EmotionType::Joy), 0.6, 5.0, 500);

        assert_eq!(tom.target_count(), 1);
        let mood = tom.perceived_mood(alice).unwrap();
        assert_eq!(mood.dominant_emotion, Some(EmotionType::Joy));
        assert_eq!(mood.observed_at, 500);
    }

    #[test]
    fn has_seen_distressed_fires_on_negative_dominant_emotion() {
        let mut tom = AffectiveToM::default();
        let alice = test_entity(1);
        tom.record_observation(alice, Some(EmotionType::Sadness), 0.0, 30.0, 0);
        assert!(tom.has_seen_distressed(alice));
    }

    #[test]
    fn has_seen_distressed_fires_on_low_mood_without_emotion() {
        let mut tom = AffectiveToM::default();
        let alice = test_entity(1);
        tom.record_observation(alice, None, -0.6, 10.0, 0);
        assert!(tom.has_seen_distressed(alice));
    }

    #[test]
    fn has_seen_distressed_quiet_for_neutral_target() {
        let mut tom = AffectiveToM::default();
        let alice = test_entity(1);
        tom.record_observation(alice, Some(EmotionType::Joy), 0.4, 10.0, 0);
        assert!(!tom.has_seen_distressed(alice));
    }
}
