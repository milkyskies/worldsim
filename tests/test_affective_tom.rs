//! Integration tests for AffectiveToM (#540): observers store the last-
//! observed mood of visible agents, decay it over time, and surface it
//! through query helpers.

use bevy::math::Vec2;
use worldsim::agent::mind::affective_tom::{AffectiveToM, CONFIDENCE_DECAY_TICKS, MIN_CONFIDENCE};
use worldsim::agent::psyche::emotions::{Emotion, EmotionType, EmotionalState};
use worldsim::core::tick::TickCount;
use worldsim::testing::{AgentConfig, TestWorld};

fn seed_emotion(world: &mut TestWorld, agent: bevy::prelude::Entity, emotion: Emotion) {
    let etype = emotion.emotion_type;
    let mut state = world.get_mut::<EmotionalState>(agent);
    state.add_emotion(emotion);
    state.current_mood = match etype {
        EmotionType::Sadness => -0.6,
        EmotionType::Joy => 0.6,
        EmotionType::Fear => -0.4,
        _ => 0.0,
    };
}

#[test]
fn observer_records_dominant_emotion_of_visible_agent() {
    let mut world = TestWorld::with_seed(0);
    let observer = world.spawn_agent(AgentConfig::at(Vec2::new(0.0, 0.0)));
    let target = world.spawn_agent(AgentConfig::at(Vec2::new(20.0, 0.0)));
    seed_emotion(&mut world, target, Emotion::new(EmotionType::Sadness, 0.7));

    // A handful of ticks: vision pass populates VisibleObjects, then
    // update_affective_tom samples the target's emotional state.
    world.tick(5);

    let tom = world.get::<AffectiveToM>(observer);
    let mood = tom
        .perceived_mood(target)
        .expect("observer should have recorded the visible target");
    assert_eq!(mood.dominant_emotion, Some(EmotionType::Sadness));
    assert!(mood.mood < 0.0);
    assert!(tom.has_seen_distressed(target));
}

#[test]
fn never_observed_target_returns_none() {
    let mut world = TestWorld::with_seed(0);
    let observer = world.spawn_agent(AgentConfig::at(Vec2::new(0.0, 0.0)));
    // A second agent placed well outside vision range — never seen.
    let stranger = world.spawn_agent(AgentConfig::at(Vec2::new(2000.0, 2000.0)));
    world.tick(5);

    let tom = world.get::<AffectiveToM>(observer);
    assert!(tom.perceived_mood(stranger).is_none());
    assert!(!tom.has_seen_distressed(stranger));
}

#[test]
fn confidence_decays_after_observation() {
    let mut world = TestWorld::with_seed(0);
    let observer = world.spawn_agent(AgentConfig::at(Vec2::new(0.0, 0.0)));
    let target = world.spawn_agent(AgentConfig::at(Vec2::new(20.0, 0.0)));
    seed_emotion(&mut world, target, Emotion::new(EmotionType::Sadness, 0.5));

    world.tick(5);

    // Move target far away so subsequent ticks don't refresh the entry.
    world
        .get_mut::<bevy::prelude::Transform>(target)
        .translation = bevy::prelude::Vec3::new(5000.0, 5000.0, 0.0);

    let observed_at = world
        .get::<AffectiveToM>(observer)
        .perceived_mood(target)
        .unwrap()
        .observed_at;

    // Jump the clock past the half-life and run a tick so decay runs.
    let half_decay = CONFIDENCE_DECAY_TICKS / 2;
    world
        .app_mut()
        .world_mut()
        .resource_mut::<TickCount>()
        .current = observed_at + half_decay;
    world.tick(60); // long enough for decay system to fire

    let tom = world.get::<AffectiveToM>(observer);
    let mood = tom
        .perceived_mood(target)
        .expect("entry should still exist at half-decay");
    assert!(
        mood.confidence < 0.6,
        "confidence should have decayed past ~0.5, got {:.3}",
        mood.confidence
    );
}

#[test]
fn entries_evict_when_confidence_drops_below_threshold() {
    let mut world = TestWorld::with_seed(0);
    let observer = world.spawn_agent(AgentConfig::at(Vec2::new(0.0, 0.0)));
    let target = world.spawn_agent(AgentConfig::at(Vec2::new(20.0, 0.0)));
    seed_emotion(&mut world, target, Emotion::new(EmotionType::Sadness, 0.5));

    world.tick(5);
    world
        .get_mut::<bevy::prelude::Transform>(target)
        .translation = bevy::prelude::Vec3::new(5000.0, 5000.0, 0.0);

    // Past the full decay window the entry must be pruned.
    world
        .app_mut()
        .world_mut()
        .resource_mut::<TickCount>()
        .current = CONFIDENCE_DECAY_TICKS + 1000;
    world.tick(120);

    let tom = world.get::<AffectiveToM>(observer);
    assert!(
        tom.perceived_mood(target).is_none(),
        "entry should have been pruned after full-decay window (MIN_CONFIDENCE={MIN_CONFIDENCE})"
    );
}

#[test]
fn two_observers_track_the_target_independently() {
    let mut world = TestWorld::with_seed(0);
    let observer_a = world.spawn_agent(AgentConfig::at(Vec2::new(0.0, 0.0)));
    let target = world.spawn_agent(AgentConfig::at(Vec2::new(20.0, 0.0)));
    seed_emotion(&mut world, target, Emotion::new(EmotionType::Sadness, 0.6));

    // Only observer_a sees the target during this window.
    world.tick(5);
    let observation_a = world
        .get::<AffectiveToM>(observer_a)
        .perceived_mood(target)
        .map(|m| m.observed_at);

    // Spawn observer_b later — they have no record yet.
    let observer_b = world.spawn_agent(AgentConfig::at(Vec2::new(20.0, 0.0)));
    let observation_b = world
        .get::<AffectiveToM>(observer_b)
        .perceived_mood(target)
        .copied();
    assert!(observation_a.is_some());
    assert!(observation_b.is_none());

    // Now both can see the target and the timestamps differ.
    world.tick(5);
    let later_a = world
        .get::<AffectiveToM>(observer_a)
        .perceived_mood(target)
        .unwrap()
        .observed_at;
    let later_b = world
        .get::<AffectiveToM>(observer_b)
        .perceived_mood(target)
        .unwrap()
        .observed_at;
    assert!(later_a > observation_a.unwrap());
    assert!(later_b >= later_a - 4 && later_b <= later_a + 4);
}
