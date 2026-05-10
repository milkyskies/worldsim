//! Affective theory of mind: per-agent store of last-observed mood / emotion.
//!
//! Reads: VisibleObjects, EmotionalState, TickCount
//! Writes: AffectiveToM (component on the observer), SimEvent
//! Upstream: mind::social_perception (visible-agent enumeration)
//! Downstream: psyche::appraisal (fortune-of-others), prosocial behaviors,
//!             other-regarding drives (#736)
//!
//! Where the first-order `TheoryOfMind` tracks "what does Alice know",
//! this tracks "how does Alice seem to feel" — the substrate for Pity,
//! HappyFor, comforting, and other other-regarding emotions. Memory is
//! finite: 32 targets, oldest evicted; confidence decays linearly with
//! age and entries below `MIN_CONFIDENCE` are pruned.

use bevy::prelude::*;
use std::collections::HashMap;

use crate::agent::Agent;
use crate::agent::events::{SimEvent, SimEventKind};
use crate::agent::mind::perception::VisibleObjects;
use crate::agent::psyche::emotions::{EmotionType, EmotionalState};
use crate::core::GameTime;
use crate::core::tick::TickCount;

/// Maximum number of distinct targets a single agent can model. Matches
/// the per-agent cap on first-order `TheoryOfMind` to keep the memory
/// footprint predictable.
pub const MAX_AFFECTIVE_TARGETS: usize = 32;

/// Game-time over which observation confidence decays linearly from 1.0
/// to 0.0. 24 game-hours — by the next morning, what an agent saw of
/// someone yesterday is already half-faded.
pub const CONFIDENCE_DECAY_TICKS: u64 = 24 * GameTime::TICKS_PER_HOUR;

/// Confidence floor below which an entry is dropped during decay.
pub const MIN_CONFIDENCE: f32 = 0.1;

/// Mood threshold at or below which an agent is considered "distressed"
/// for `has_seen_distressed`. Pairs with the dominant-emotion check —
/// either a clearly negative dominant emotion or a low overall mood
/// counts as distress.
const DISTRESSED_MOOD: f32 = -0.3;

/// What we last saw of another agent's affective state.
#[derive(Debug, Clone, Copy)]
pub struct PerceivedMood {
    /// Strongest active emotion at the moment of observation, if any
    /// emotion was active.
    pub dominant_emotion: Option<EmotionType>,
    /// Overall valence at observation, range -1.0..=1.0.
    pub mood: f32,
    /// Stress level at observation, range 0..=100.
    pub stress: f32,
    /// Tick on which this observation was recorded.
    pub observed_at: u64,
    /// Decayed confidence in [0, 1]. Equal to `1.0 - age / DECAY_TICKS`,
    /// clamped at zero.
    pub confidence: f32,
}

impl PerceivedMood {
    /// Confidence at `now` given the observation timestamp. Pure function
    /// — does not mutate the entry.
    pub fn confidence_at(&self, now: u64) -> f32 {
        let age = now.saturating_sub(self.observed_at) as f32;
        (1.0 - age / CONFIDENCE_DECAY_TICKS as f32).clamp(0.0, 1.0)
    }
}

/// Component: per-observer store of last-observed mood per known agent.
#[derive(Component, Debug, Default, Clone, Reflect)]
#[reflect(Component)]
pub struct AffectiveToM {
    #[reflect(ignore)]
    beliefs: HashMap<Entity, PerceivedMood>,
}

impl AffectiveToM {
    /// Record what `target` looked like emotionally at `tick`. Refreshes
    /// the existing entry if there is one; evicts the oldest target on
    /// capacity overflow so the bookkeeping stays bounded.
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
            confidence: 1.0,
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

    /// Most recent observation for `target`, or `None` if never seen.
    pub fn perceived_mood(&self, target: Entity) -> Option<&PerceivedMood> {
        self.beliefs.get(&target)
    }

    /// True iff the agent's last observation of `target` was clearly
    /// negative — a distress-band dominant emotion or a mood at or
    /// below `DISTRESSED_MOOD`. Drives prosocial / fortune-of-others
    /// reasoning ("Alice looked sad — should I check on her?").
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

    /// Number of targets currently modeled.
    pub fn target_count(&self) -> usize {
        self.beliefs.len()
    }

    /// Re-decay every entry to its confidence at `now` and drop any that
    /// dipped below `MIN_CONFIDENCE`. Returns the number of evictions.
    pub fn decay(&mut self, now: u64) -> usize {
        let before = self.beliefs.len();
        self.beliefs.retain(|_, mood| {
            mood.confidence = mood.confidence_at(now);
            mood.confidence >= MIN_CONFIDENCE
        });
        before - self.beliefs.len()
    }
}

/// Strongest active emotion in `state`, or `None` if no emotion is active.
fn dominant_emotion(state: &EmotionalState) -> Option<EmotionType> {
    state
        .active_emotions
        .iter()
        .filter(|e| e.intensity > 0.0)
        .max_by(|a, b| {
            a.intensity
                .partial_cmp(&b.intensity)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|e| e.emotion_type)
}

/// Per-tick: for each observer, sample the affective state of every
/// agent in their `VisibleObjects` and record it on their `AffectiveToM`.
/// Runs after `perceive_other_agents` so the observer's MindGraph already
/// classifies visible entities as Sentient.
pub fn update_affective_tom(
    mut observers: Query<(Entity, &VisibleObjects, &mut AffectiveToM), With<Agent>>,
    targets: Query<&EmotionalState, With<Agent>>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<SimEvent>,
) {
    let now = tick.current;

    for (observer, visible, mut tom) in observers.iter_mut() {
        for &visible_entity in &visible.entities {
            if visible_entity == observer {
                continue;
            }
            let Ok(state) = targets.get(visible_entity) else {
                continue;
            };

            tom.record_observation(
                visible_entity,
                dominant_emotion(state),
                state.current_mood,
                state.stress_level,
                now,
            );

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

/// Slow-tick: drop entries whose confidence has decayed past the floor
/// and refresh confidence on the rest. Stagger by entity so the work is
/// spread across ticks rather than spiking each pass.
pub fn decay_affective_tom(
    mut observers: Query<(Entity, &mut AffectiveToM), With<Agent>>,
    tick: Res<TickCount>,
) {
    // One pass per game-minute is plenty — confidence changes by ~0.07%
    // per game-second.
    let stagger = GameTime::TICKS_PER_MINUTE;
    for (entity, mut tom) in observers.iter_mut() {
        if !tick.should_run(entity, stagger) {
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

        tom.record_observation(alice, dominant_emotion(&state), -0.4, 30.0, 100);

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

        let half_decay = CONFIDENCE_DECAY_TICKS / 2;
        let mood = tom.perceived_mood(alice).unwrap();
        let halfway = mood.confidence_at(half_decay);
        assert!(
            (halfway - 0.5).abs() < 1e-3,
            "halfway through decay window should give ~0.5, got {halfway}"
        );

        // Past the full window confidence saturates at zero.
        assert!(mood.confidence_at(CONFIDENCE_DECAY_TICKS * 2).abs() < 1e-6);
    }

    #[test]
    fn decay_evicts_entries_below_threshold() {
        let mut tom = AffectiveToM::default();
        let alice = test_entity(1);
        tom.record_observation(alice, None, 0.0, 0.0, 0);

        // After full decay window the entry is at confidence 0 — well
        // under MIN_CONFIDENCE.
        let evicted = tom.decay(CONFIDENCE_DECAY_TICKS + 1);
        assert_eq!(evicted, 1);
        assert!(tom.perceived_mood(alice).is_none());
    }

    #[test]
    fn capacity_evicts_oldest_target() {
        let mut tom = AffectiveToM::default();
        // Fill to capacity.
        for i in 0..MAX_AFFECTIVE_TARGETS {
            tom.record_observation(test_entity(i as u32), None, 0.0, 0.0, i as u64);
        }
        assert_eq!(tom.target_count(), MAX_AFFECTIVE_TARGETS);

        // The oldest entry (id 0, observed_at 0) must be the one evicted.
        let newcomer = test_entity(999);
        tom.record_observation(newcomer, None, 0.0, 0.0, 1000);
        assert_eq!(tom.target_count(), MAX_AFFECTIVE_TARGETS);
        assert!(tom.perceived_mood(test_entity(0)).is_none());
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
        // No active emotion, but mood is well below the distress floor.
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

    #[test]
    fn dominant_emotion_picks_strongest() {
        let mut state = EmotionalState::default();
        state.add_emotion(Emotion::new(EmotionType::Sadness, 0.3));
        state.add_emotion(Emotion::new(EmotionType::Fear, 0.7));
        state.add_emotion(Emotion::new(EmotionType::Joy, 0.1));
        assert_eq!(dominant_emotion(&state), Some(EmotionType::Fear));
    }

    #[test]
    fn dominant_emotion_is_none_for_quiet_state() {
        let state = EmotionalState::default();
        assert_eq!(dominant_emotion(&state), None);
    }
}
