//! End-to-end: Compassion urgency rises for a distressed peer when
//! affection is high, and the urgency carries the peer as `target`.

use bevy::math::Vec2;
use worldsim::agent::body::genetics::builder::personality;
use worldsim::agent::mind::affective_tom::AffectiveToM;
use worldsim::agent::mind::knowledge::{
    Metadata, MindGraph, Node, Predicate, Quantity, Triple, Value,
};
use worldsim::agent::nervous_system::cns::CentralNervousSystem;
use worldsim::agent::nervous_system::urgency::{Urgency, UrgencySource};
use worldsim::agent::psyche::emotions::{Emotion, EmotionType, EmotionalState};
use worldsim::testing::{AgentConfig, TestWorld};

fn set_affection(
    world: &mut TestWorld,
    observer: bevy::prelude::Entity,
    target: bevy::prelude::Entity,
    value: f32,
) {
    let mut mind = world.get_mut::<MindGraph>(observer);
    mind.assert(Triple::with_meta(
        Node::Entity(target),
        Predicate::Affection,
        Value::Quantity(Quantity::Exact(value)),
        Metadata::default(),
    ));
}

fn seed_distress(world: &mut TestWorld, agent: bevy::prelude::Entity) {
    let mut state = world.get_mut::<EmotionalState>(agent);
    state.add_emotion(Emotion::new(EmotionType::Sadness, 0.8));
    state.current_mood = -0.7;
    state.stress_level = 70.0;
}

fn compassion_urgencies(world: &TestWorld, observer: bevy::prelude::Entity) -> Vec<Urgency> {
    let cns = world.get::<CentralNervousSystem>(observer);
    cns.urgencies
        .iter()
        .filter(|u| u.source == UrgencySource::Compassion)
        .cloned()
        .collect()
}

#[test]
fn high_affection_observer_emits_compassion_urgency_for_distressed_peer() {
    let mut world = TestWorld::with_seed(0);
    world.enable_fast_brains();
    let observer = world.spawn_agent(AgentConfig::at(Vec2::new(0.0, 0.0)));
    let target = world.spawn_agent(AgentConfig::at(Vec2::new(20.0, 0.0)));

    seed_distress(&mut world, target);
    set_affection(&mut world, observer, target, 0.9);

    // Tick: vision → AffectiveToM update → urgency loop.
    world.tick(5);

    let urgencies = compassion_urgencies(&world, observer);
    assert!(
        !urgencies.is_empty(),
        "high-affection observer should emit Compassion for distressed peer"
    );
    let care = &urgencies[0];
    assert_eq!(
        care.target,
        Some(target),
        "Compassion urgency must carry the peer's entity as target"
    );
    assert!(
        care.value > 0.05,
        "Compassion urgency value should be above min threshold, got {:.3}",
        care.value
    );
}

#[test]
fn low_affection_observer_does_not_emit_compassion() {
    let mut world = TestWorld::with_seed(0);
    world.enable_fast_brains();
    let observer = world.spawn_agent(AgentConfig::at(Vec2::new(0.0, 0.0)));
    let target = world.spawn_agent(AgentConfig::at(Vec2::new(20.0, 0.0)));

    seed_distress(&mut world, target);
    // Below-neutral affection: the affection channel contributes 0.
    set_affection(&mut world, observer, target, 0.3);

    world.tick(5);

    assert!(
        compassion_urgencies(&world, observer).is_empty(),
        "low-affection observer should not generate Compassion urgency"
    );
}

#[test]
fn high_affection_pair_produces_stronger_urgency_than_low_affection_pair() {
    fn measure(affection: f32) -> f32 {
        let mut world = TestWorld::with_seed(0);
        world.enable_fast_brains();
        let observer = world.spawn_agent(AgentConfig {
            pos: Vec2::new(0.0, 0.0),
            genome: personality().conscientiousness(0.5).into(),
            ..Default::default()
        });
        let target = world.spawn_agent(AgentConfig::at(Vec2::new(20.0, 0.0)));

        seed_distress(&mut world, target);
        set_affection(&mut world, observer, target, affection);
        world.tick(5);

        compassion_urgencies(&world, observer)
            .first()
            .map(|u| u.value)
            .unwrap_or(0.0)
    }

    let weak = measure(0.6);
    let strong = measure(0.95);
    assert!(
        strong > weak,
        "stronger affection should produce stronger Compassion urgency \
         (weak={weak:.3}, strong={strong:.3})"
    );
}

#[test]
fn no_perceived_distress_means_no_compassion_even_at_high_affection() {
    let mut world = TestWorld::with_seed(0);
    world.enable_fast_brains();
    let observer = world.spawn_agent(AgentConfig::at(Vec2::new(0.0, 0.0)));
    let target = world.spawn_agent(AgentConfig::at(Vec2::new(20.0, 0.0)));

    // High affection but the peer is content.
    set_affection(&mut world, observer, target, 0.95);
    world.tick(5);

    assert!(
        compassion_urgencies(&world, observer).is_empty(),
        "no distress in the peer should suppress Compassion regardless of affection"
    );
}

#[test]
fn observer_without_affective_tom_entry_does_not_fire_compassion() {
    // Verifies the gating: even a distressed agent that the observer
    // has never perceived produces no urgency.
    let mut world = TestWorld::with_seed(0);
    world.enable_fast_brains();
    let observer = world.spawn_agent(AgentConfig::at(Vec2::new(0.0, 0.0)));
    // Place target far outside vision range so AffectiveToM never logs them.
    let target = world.spawn_agent(AgentConfig::at(Vec2::new(5000.0, 5000.0)));

    seed_distress(&mut world, target);
    set_affection(&mut world, observer, target, 0.95);
    world.tick(5);

    let tom = world.get::<AffectiveToM>(observer);
    assert!(
        tom.perceived_mood(target).is_none(),
        "test setup: observer must not have an AffectiveToM record of the unseen target"
    );
    assert!(
        compassion_urgencies(&world, observer).is_empty(),
        "Compassion requires an Affective ToM observation; unseen targets shouldn't fire urgency"
    );
}
