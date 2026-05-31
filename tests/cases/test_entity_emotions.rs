//! Integration tests for entity-targeted emotion writes.
//!
//! `(entity, TriggersEmotion, Emotion(Type, intensity))` triples are
//! written by the runtime systems (combat-hit reactions, danger
//! perception) so the emotional brain's entity-targeted proposals fire
//! against real data, and the inner-life UI's "Feels about" panel
//! shows what the agent actually feels about specific entities.

use bevy::prelude::*;
use worldsim::agent::brains::emotional::{entity_emotion_intensity, entity_feelings};
use worldsim::agent::events::{SimEvent, SimEventKind};
use worldsim::agent::mind::knowledge::{MindGraph, Node, Predicate, Value};
use worldsim::agent::psyche::emotions::EmotionType;
use worldsim::testing::{AgentConfig, TestWorld};

fn defender_was_hit(world: &TestWorld, defender: Entity) -> bool {
    world.sim_events().all().iter().any(|e| {
        matches!(
            e,
            SimEvent {
                kind: SimEventKind::CombatHit { defender: d, .. },
                ..
            } if *d == defender
        )
    })
}

fn make_starving(world: &mut TestWorld, wolf: Entity) {
    let mut needs = world
        .app_mut()
        .world_mut()
        .get_mut::<worldsim::agent::body::needs::PhysicalNeeds>(wolf)
        .expect("wolf has needs");
    needs.metabolism = worldsim::agent::body::metabolism::Metabolism::at_urgency(0.95);
}

#[test]
fn human_bitten_by_wolf_gains_entity_anger_toward_that_wolf() {
    let mut world = TestWorld::with_seed(7);
    let human = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        ..Default::default()
    });
    let wolf = world.spawn_wolf(Vec2::new(51.0, 50.0));
    world.tick(1);
    make_starving(&mut world, wolf);

    for _ in 0..600 {
        world.tick(1);
        if defender_was_hit(&world, human) {
            world.tick(1);
            break;
        }
    }
    assert!(
        defender_was_hit(&world, human),
        "wolf should bite human within 600 ticks at seed 7"
    );

    let mind = world.get::<MindGraph>(human);
    let anger = entity_emotion_intensity(mind, wolf, EmotionType::Anger);
    assert!(
        anger > 0.0,
        "human should have entity-Anger toward the wolf that bit them \
         (got {anger:.3})"
    );
}

#[test]
fn entity_anger_distinguishes_attacker_from_bystander() {
    let mut world = TestWorld::with_seed(11);
    let human = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        ..Default::default()
    });
    let attacker_wolf = world.spawn_wolf(Vec2::new(51.0, 50.0));
    let bystander_wolf = world.spawn_wolf(Vec2::new(45.0, 53.0));
    world.tick(1);
    make_starving(&mut world, attacker_wolf);
    // Bystander stays well-fed so it just walks around without attacking.

    for _ in 0..600 {
        world.tick(1);
        if defender_was_hit(&world, human) {
            world.tick(1);
            break;
        }
    }
    assert!(
        defender_was_hit(&world, human),
        "starving wolf should bite human within 600 ticks at seed 11"
    );

    let mind = world.get::<MindGraph>(human);
    let anger_attacker = entity_emotion_intensity(mind, attacker_wolf, EmotionType::Anger);
    let anger_bystander = entity_emotion_intensity(mind, bystander_wolf, EmotionType::Anger);
    assert!(
        anger_attacker > anger_bystander,
        "Anger should be higher toward the actual attacker \
         (attacker={anger_attacker:.3}, bystander={anger_bystander:.3})"
    );
}

#[test]
fn perceiving_dangerous_entity_writes_entity_fear() {
    let mut world = TestWorld::with_seed(13);
    let human = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        ..Default::default()
    });
    let wolf = world.spawn_wolf(Vec2::new(52.0, 50.0));
    world.tick(60);

    let mind = world.get::<MindGraph>(human);
    // Sanity: the wolf entity should appear in the entities-with-feelings
    // set since react_to_danger has been writing entity-Fear triples for
    // the visible wolf.
    let triples = mind.query(
        Some(&Node::Entity(wolf)),
        Some(Predicate::TriggersEmotion),
        None,
    );
    let has_fear = triples.iter().any(|t| {
        matches!(
            t.object,
            Value::Emotion(EmotionType::Fear, intensity) if intensity > 0.0
        )
    });
    assert!(
        has_fear,
        "human seeing a wolf should accumulate entity-Fear toward the wolf"
    );
}

#[test]
fn entity_feelings_helpers_return_combat_anger() {
    // Direct unit-style check: assert a TriggersEmotion triple manually
    // and verify the read helpers see it. Decoupled from combat
    // bootstrap; isolates the helper API.
    let mut world = TestWorld::with_seed(17);
    let human = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        ..Default::default()
    });
    let wolf = world.spawn_wolf(Vec2::new(60.0, 50.0));
    world.tick(1);

    {
        use worldsim::agent::mind::knowledge::{Metadata, Triple};
        let mut mind = world
            .app_mut()
            .world_mut()
            .get_mut::<MindGraph>(human)
            .expect("human has mind");
        mind.assert(Triple::with_meta(
            Node::Entity(wolf),
            Predicate::TriggersEmotion,
            Value::Emotion(EmotionType::Anger, 0.7),
            Metadata::default(),
        ));
    }

    let mind = world.get::<MindGraph>(human);
    let intensity = entity_emotion_intensity(mind, wolf, EmotionType::Anger);
    assert!(
        (intensity - 0.7).abs() < 1e-3,
        "entity_emotion_intensity should sum the asserted intensity (got {intensity:.3})"
    );

    let feelings = entity_feelings(wolf, mind);
    assert!(
        feelings
            .iter()
            .any(|(t, i)| *t == EmotionType::Anger && (*i - 0.7).abs() < 1e-3),
        "entity_feelings should return the asserted (Anger, 0.7) tuple"
    );
}
