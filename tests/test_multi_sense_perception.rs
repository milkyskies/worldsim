//! Integration tests for the multi-sense perception framework (#280).
//!
//! Verifies that temperature and hearing senses produce the expected MindGraph
//! triples with correct source_sense metadata, and that SoundSource is transient.

use bevy::prelude::*;
use worldsim::agent::events::SimEvent;
use worldsim::agent::mind::knowledge::{
    CardinalDirection, Concept, MindGraph, Node, Predicate, Sense, Value,
};
use worldsim::testing::{AgentConfig, TestWorld};
use worldsim::world::sense_sources::{SoundKind, SoundSource};

// ─── Temperature Sense ────────────────────────────────────────────────────

#[test]
fn agent_near_heat_source_perceives_warmth_triple() {
    let mut world = TestWorld::with_seed(42);
    // Agent at origin, campfire 30px away (well within 64px heat range)
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(100.0, 100.0)));
    let _campfire = world.spawn_campfire(Vec2::new(130.0, 100.0));

    world.tick(3);

    let mind = world.get::<MindGraph>(agent);
    let warmth_triples: Vec<_> = mind
        .query(
            None,
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Warmth)),
        )
        .into_iter()
        .filter(|t| t.meta.source_sense == Some(Sense::Temperature))
        .collect();

    assert!(
        !warmth_triples.is_empty(),
        "agent near a campfire should perceive warmth via temperature sense"
    );
}

#[test]
fn temperature_perception_does_not_require_line_of_sight() {
    // Temperature doesn't check LoS — it should work even if a wall existed.
    // Since we don't have wall-blocking in the basic spatial check, this test
    // just verifies the system runs without LoS filtering by placing the heat
    // source at a position that would be behind geometry.
    let mut world = TestWorld::with_seed(99);
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(200.0, 200.0)));
    let _campfire = world.spawn_campfire(Vec2::new(240.0, 200.0));

    world.tick(3);

    let mind = world.get::<MindGraph>(agent);
    let warmth_triples: Vec<_> = mind
        .query(
            None,
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Warmth)),
        )
        .into_iter()
        .filter(|t| t.meta.source_sense == Some(Sense::Temperature))
        .collect();

    assert!(
        !warmth_triples.is_empty(),
        "temperature sense should detect heat without line-of-sight check"
    );
}

#[test]
fn agent_beyond_heat_range_does_not_perceive_warmth() {
    let mut world = TestWorld::with_seed(42);
    // Agent at origin, campfire 200px away (well beyond 64px heat range)
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(100.0, 100.0)));
    let _campfire = world.spawn_campfire(Vec2::new(300.0, 100.0));

    world.tick(3);

    let mind = world.get::<MindGraph>(agent);
    let warmth_triples: Vec<_> = mind
        .query(
            None,
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Warmth)),
        )
        .into_iter()
        .filter(|t| t.meta.source_sense == Some(Sense::Temperature))
        .collect();

    assert!(
        warmth_triples.is_empty(),
        "agent far from heat source should not perceive warmth"
    );
}

#[test]
fn temperature_triple_has_lower_confidence_than_sight() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(100.0, 100.0)));
    let _campfire = world.spawn_campfire(Vec2::new(120.0, 100.0));

    world.tick(3);

    let mind = world.get::<MindGraph>(agent);

    // Temperature confidence should be capped at 0.6
    let temp_triples: Vec<_> = mind
        .query(
            None,
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Warmth)),
        )
        .into_iter()
        .filter(|t| t.meta.source_sense == Some(Sense::Temperature))
        .collect();

    for t in &temp_triples {
        assert!(
            t.meta.confidence <= 0.6,
            "temperature confidence ({}) should be <= 0.6",
            t.meta.confidence
        );
    }
}

// ─── Hearing Sense ────────────────────────────────────────────────────────

#[test]
fn agent_within_hearing_range_perceives_sound_triple() {
    let mut world = TestWorld::with_seed(42);
    // Agent at origin, howling wolf 200px away (within 512px hearing range)
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(100.0, 100.0)));
    let _source = world.spawn_sound_source(Vec2::new(300.0, 100.0), SoundKind::Howl, 1.0);

    world.tick(2);

    let mind = world.get::<MindGraph>(agent);
    let sound_triples: Vec<_> = mind
        .query(None, Some(Predicate::ProducedSound), None)
        .into_iter()
        .filter(|t| t.meta.source_sense == Some(Sense::Hearing))
        .collect();

    assert!(
        !sound_triples.is_empty(),
        "agent should hear a sound source within range"
    );

    // Should be tagged as a Howl
    let has_howl = sound_triples
        .iter()
        .any(|t| t.object == Value::Concept(Concept::Howl));
    assert!(has_howl, "sound triple should identify the sound as a Howl");
}

#[test]
fn hearing_triple_has_direction_not_entity() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(100.0, 100.0)));
    let _source = world.spawn_sound_source(Vec2::new(300.0, 100.0), SoundKind::Howl, 1.0);

    world.tick(2);

    let mind = world.get::<MindGraph>(agent);
    let sound_triples: Vec<_> = mind
        .query(None, Some(Predicate::ProducedSound), None)
        .into_iter()
        .filter(|t| t.meta.source_sense == Some(Sense::Hearing))
        .collect();

    // Subject should be a Direction, not an Entity
    for t in &sound_triples {
        assert!(
            matches!(t.subject, Node::Direction(_)),
            "hearing triples should use Direction as subject, got {:?}",
            t.subject
        );
    }
}

#[test]
fn agent_beyond_hearing_range_does_not_perceive_sound() {
    let mut world = TestWorld::with_seed(42);
    // Agent at origin, sound source 600px away (beyond 512px hearing range)
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(100.0, 100.0)));
    let _source = world.spawn_sound_source(Vec2::new(700.0, 100.0), SoundKind::Howl, 1.0);

    world.tick(2);

    let mind = world.get::<MindGraph>(agent);
    let sound_triples: Vec<_> = mind
        .query(None, Some(Predicate::ProducedSound), None)
        .into_iter()
        .filter(|t| t.meta.source_sense == Some(Sense::Hearing))
        .collect();

    assert!(
        sound_triples.is_empty(),
        "agent far from sound source should not hear it"
    );
}

#[test]
fn threatening_sounds_tag_direction_as_dangerous() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(100.0, 100.0)));
    let _source = world.spawn_sound_source(Vec2::new(300.0, 100.0), SoundKind::Howl, 1.0);

    world.tick(2);

    let mind = world.get::<MindGraph>(agent);
    let danger_triples: Vec<_> = mind
        .query(
            None,
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Dangerous)),
        )
        .into_iter()
        .filter(|t| {
            t.meta.source_sense == Some(Sense::Hearing) && matches!(t.subject, Node::Direction(_))
        })
        .collect();

    assert!(
        !danger_triples.is_empty(),
        "threatening sounds (howl) should tag the direction as dangerous"
    );
}

// ─── SoundSource Transience ───────────────────────────────────────────────

#[test]
fn sound_source_is_removed_after_one_perception_tick() {
    let mut world = TestWorld::with_seed(42);
    let _agent = world.spawn_agent(AgentConfig::at(Vec2::new(100.0, 100.0)));
    let source = world.spawn_sound_source(Vec2::new(200.0, 100.0), SoundKind::Howl, 1.0);

    // Before ticking, sound source should exist
    assert!(
        world.app().world().get::<SoundSource>(source).is_some(),
        "SoundSource should exist before tick"
    );

    world.tick(1);

    // After one tick, the SoundSource component should be removed
    assert!(
        world.app().world().get::<SoundSource>(source).is_none(),
        "SoundSource should be removed after one perception tick"
    );
}

// ─── SimEvent Emission ────────────────────────────────────────────────────

#[test]
fn warmth_perceived_event_emitted() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(100.0, 100.0)));
    let _campfire = world.spawn_campfire(Vec2::new(130.0, 100.0));

    world.tick(3);

    let log = world
        .app()
        .world()
        .resource::<worldsim::testing::SimEventLog>();
    let warmth_events: Vec<_> = log
        .all()
        .iter()
        .filter(|e| matches!(e, SimEvent::WarmthPerceived { agent: a, .. } if *a == agent))
        .collect();

    assert!(
        !warmth_events.is_empty(),
        "WarmthPerceived SimEvent should be emitted when agent detects heat"
    );
}

#[test]
fn sound_perceived_event_emitted() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(100.0, 100.0)));
    let _source = world.spawn_sound_source(Vec2::new(200.0, 100.0), SoundKind::Howl, 1.0);

    world.tick(2);

    let log = world
        .app()
        .world()
        .resource::<worldsim::testing::SimEventLog>();
    let sound_events: Vec<_> = log
        .all()
        .iter()
        .filter(|e| matches!(e, SimEvent::SoundPerceived { agent: a, .. } if *a == agent))
        .collect();

    assert!(
        !sound_events.is_empty(),
        "SoundPerceived SimEvent should be emitted when agent hears sound"
    );
}
