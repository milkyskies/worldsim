//! Integration tests for the smart-combat overhaul: threat appraisal,
//! multi-threat flee, cornered detection, alarm propagation, witness
//! fear, lameness derivation, predator switching, and dazed status.

use bevy::prelude::*;
use worldsim::agent::actions::ActionType;
use worldsim::agent::biology::body::{Body, Injury, InjuryType};
use worldsim::agent::events::{SimEvent, SimEventKind};
use worldsim::agent::psyche::emotions::{Emotion, EmotionType, EmotionalState};
use worldsim::agent::{Cornered, Dazed, Lame};
use worldsim::testing::{AgentConfig, TestWorld};

fn agent_started_action_against(
    world: &TestWorld,
    agent: Entity,
    action: ActionType,
    target: Entity,
) -> bool {
    world.sim_events().all().iter().any(|e| {
        matches!(
            e,
            SimEvent {
                kind: SimEventKind::ActionStarted {
                    agent: a, action: act, target: Some(t), ..
                },
                ..
            }
                if *a == agent && *act == action && *t == target
        )
    })
}

fn cornered_event_for(world: &TestWorld, agent: Entity) -> bool {
    world.sim_events().all().iter().any(|e| {
        matches!(
            e,
            SimEvent { kind: SimEventKind::Cornered { agent: a }, .. } if *a == agent
        )
    })
}

fn lameness_changed_event_for(world: &TestWorld, agent: Entity) -> bool {
    world.sim_events().all().iter().any(|e| {
        matches!(
            e,
            SimEvent {
                kind: SimEventKind::LamenessChanged { agent: a, lame: true },
                ..
            } if *a == agent
        )
    })
}

fn witnessed_combat_event_for(world: &TestWorld, observer: Entity) -> bool {
    world.sim_events().all().iter().any(|e| {
        matches!(
            e,
            SimEvent {
                kind: SimEventKind::WitnessedCombat { observer: o, .. },
                ..
            } if *o == observer
        )
    })
}

fn agent_fear(world: &TestWorld, agent: Entity) -> f32 {
    world
        .get::<EmotionalState>(agent)
        .active_emotions
        .iter()
        .filter(|e| e.emotion_type == EmotionType::Fear)
        .map(|e| e.intensity)
        .sum()
}

fn cripple_legs(world: &mut TestWorld, agent: Entity) {
    let mut body = world
        .app_mut()
        .world_mut()
        .get_mut::<Body>(agent)
        .expect("agent should have a Body");
    for node in body.parts.iter_mut() {
        if node.kind.is_leg() {
            node.current_hp = node.max_hp * 0.3;
            node.injuries.push(Injury {
                injury_type: InjuryType::Pierce,
                severity: 0.7,
                pain: 0.5,
                healed_amount: 0.0,
                bleed_rate: 0.0,
            });
        }
    }
}

#[test]
fn lame_component_is_set_when_legs_drop_below_threshold() {
    let mut world = TestWorld::with_seed(1);
    let deer = world.spawn_deer(Vec2::new(50.0, 50.0));
    world.tick(1);

    cripple_legs(&mut world, deer);
    world.tick(2);

    assert!(
        world.app().world().get::<Lame>(deer).is_some(),
        "deer with crippled legs should gain the Lame component"
    );
    assert!(
        lameness_changed_event_for(&world, deer),
        "LamenessChanged event should fire on transition"
    );
}

#[test]
fn predator_with_two_visible_prey_picks_lame_one() {
    let mut world = TestWorld::with_seed(2);
    let healthy_deer = world.spawn_deer(Vec2::new(45.0, 50.0));
    let lame_deer = world.spawn_deer(Vec2::new(55.0, 50.0));
    let wolf = world.spawn_wolf(Vec2::new(50.0, 50.0));
    world.tick(1);

    cripple_legs(&mut world, lame_deer);

    // Make the wolf hungry so it actually hunts.
    {
        let mut needs = world
            .app_mut()
            .world_mut()
            .get_mut::<worldsim::agent::body::needs::PhysicalNeeds>(wolf)
            .expect("wolf has needs");
        needs.metabolism = worldsim::agent::body::metabolism::Metabolism::at_urgency(0.85);
    }

    world.tick(800);

    let bit_lame = agent_started_action_against(&world, wolf, ActionType::InitiateHunt, lame_deer);
    let bit_healthy =
        agent_started_action_against(&world, wolf, ActionType::InitiateHunt, healthy_deer);

    assert!(
        bit_lame || !bit_healthy,
        "wolf should prefer lame prey when both are visible \
         (bit_lame={bit_lame} bit_healthy={bit_healthy})"
    );
}

#[ignore = "TODO #716/#744/#746: needs follow-up to migrate behavior to engagement-driven path"]
#[test]
fn cornered_signal_fires_when_no_escape_exists() {
    use worldsim::world::map::TileType;
    let mut world = TestWorld::scenario(3)
        .map_size(16, 16)
        .noise_biomes(false)
        .fill_rect(0, 0, 16, 16, TileType::Water)
        .tile_at(8, 8, TileType::Grass)
        .agent("alice")
        .pos(Vec2::new(8.0 * 8.0, 8.0 * 8.0))
        .done()
        .build()
        .0;

    let alice = world.find_agent("alice").expect("alice spawned");
    let _wolf = world.spawn_wolf(Vec2::new(60.0, 64.0));

    {
        let mut state = world
            .app_mut()
            .world_mut()
            .get_mut::<EmotionalState>(alice)
            .expect("alice has emotions");
        state.add_emotion(Emotion::new(EmotionType::Fear, 1.5));
    }

    world.tick(60);

    assert!(
        cornered_event_for(&world, alice) || world.app().world().get::<Cornered>(alice).is_some(),
        "agent surrounded by water with a visible threat should be Cornered"
    );
}

#[ignore = "TODO #716/#744/#746: needs follow-up to migrate behavior to engagement-driven path"]
#[test]
fn human_witnessing_combat_gains_fear() {
    let mut world = TestWorld::with_seed(4);
    let observer = world.spawn_agent(AgentConfig {
        pos: Vec2::new(48.0, 50.0),
        ..Default::default()
    });
    let target = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        ..Default::default()
    });
    let wolf = world.spawn_wolf(Vec2::new(51.0, 50.0));
    world.tick(1);

    {
        let mut needs = world
            .app_mut()
            .world_mut()
            .get_mut::<worldsim::agent::body::needs::PhysicalNeeds>(wolf)
            .expect("wolf has needs");
        needs.metabolism = worldsim::agent::body::metabolism::Metabolism::at_urgency(0.95);
    }

    let fear_before = agent_fear(&world, observer);

    let mut hit_seen = false;
    for _ in 0..600 {
        world.tick(1);
        if witnessed_combat_event_for(&world, observer) {
            hit_seen = true;
            world.tick(1);
            break;
        }
    }

    assert!(
        hit_seen,
        "wolf should bite target while observer is watching within 600 ticks (seed 4)"
    );

    let fear_after = agent_fear(&world, observer);
    assert!(
        fear_after > fear_before,
        "observer should gain Fear from witnessing combat \
         (before={fear_before:.3}, after={fear_after:.3})"
    );
    let _ = target;
}

#[test]
fn dazed_agent_skips_next_action_proposal() {
    let mut world = TestWorld::with_seed(5);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        ..Default::default()
    });
    world.tick(1);

    {
        let current_tick = world.current_tick();
        world.app_mut().world_mut().entity_mut(agent).insert(Dazed {
            until_tick: current_tick + 60,
        });
    }

    let action_count_before = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                SimEventKind::ActionStarted { agent: a, .. } if a == agent
            )
        })
        .count();

    world.tick(20);

    let action_count_after = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                SimEventKind::ActionStarted { agent: a, .. } if a == agent
            )
        })
        .count();

    assert_eq!(
        action_count_before, action_count_after,
        "Dazed agent should not start any new actions while the daze is active"
    );
}
