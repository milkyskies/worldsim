//! End-to-end tests for the desperate-fight loop:
//! `DefendSelf`, `react_to_combat_hit`, and the starving-predator
//! proposal that bootstraps the first cross-species hit.

use bevy::prelude::*;
use worldsim::agent::actions::ActionType;
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::events::{SimEvent, SimEventKind};
use worldsim::agent::psyche::emotions::{Emotion, EmotionType, EmotionalState};
use worldsim::testing::{AgentConfig, TestWorld};

fn agent_started_action(world: &TestWorld, agent: Entity, action: ActionType) -> bool {
    world.sim_events().all().iter().any(|e| {
        matches!(
            e,
            SimEvent { kind: SimEventKind::ActionStarted { agent: a, action: act, .. }, .. }
                if *a == agent && *act == action
        )
    })
}

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

fn defender_was_hit(world: &TestWorld, defender: Entity) -> bool {
    world.sim_events().all().iter().any(|e| {
        matches!(
            e,
            SimEvent {
                kind: SimEventKind::CombatHit { defender: d, .. },
                ..
            }
                if *d == defender
        )
    })
}

fn agent_anger(world: &TestWorld, agent: Entity) -> f32 {
    world
        .get::<EmotionalState>(agent)
        .active_emotions
        .iter()
        .filter(|e| e.emotion_type == EmotionType::Anger)
        .map(|e| e.intensity)
        .sum()
}

fn set_starving(world: &mut TestWorld, wolf: Entity) {
    let mut needs = world
        .app_mut()
        .world_mut()
        .get_mut::<PhysicalNeeds>(wolf)
        .expect("wolf has needs");
    needs.metabolism = worldsim::agent::body::metabolism::Metabolism::at_urgency(0.95);
}

#[ignore = "TODO #716/#744/#746: needs follow-up to migrate behavior to engagement-driven path"]
#[test]
fn starving_wolf_bites_nearby_human() {
    let mut world = TestWorld::with_seed(7);
    let human = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        ..Default::default()
    });
    let wolf = world.spawn_wolf(Vec2::new(51.0, 50.0));
    world.tick(1);
    set_starving(&mut world, wolf);
    world.tick(600);

    assert!(
        agent_started_action_against(&world, wolf, ActionType::InitiateHunt, human),
        "starving wolf should start a Bite action targeting the visible human"
    );
}

/// `react_to_combat_hit` must lift defender Anger above zero by the tick
/// after the first hit lands. Anger decays naturally, so we sample
/// immediately rather than after a fixed tick budget.
#[ignore = "TODO #716/#744/#746: needs follow-up to migrate behavior to engagement-driven path"]
#[test]
fn human_bitten_by_wolf_accumulates_anger() {
    let mut world = TestWorld::with_seed(11);
    let human = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        ..Default::default()
    });
    let wolf = world.spawn_wolf(Vec2::new(51.0, 50.0));
    world.tick(1);
    set_starving(&mut world, wolf);

    for _ in 0..600 {
        world.tick(1);
        if defender_was_hit(&world, human) {
            world.tick(1);
            break;
        }
    }

    assert!(
        defender_was_hit(&world, human),
        "wolf should bite the human within 600 ticks (seed 11)"
    );
    assert!(
        agent_anger(&world, human) > 0.0,
        "human's anger should rise after being bitten"
    );
}

#[test]
fn well_fed_wolf_does_not_bite_nearby_human() {
    let mut world = TestWorld::with_seed(13);
    let human = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        ..Default::default()
    });
    let wolf = world.spawn_wolf(Vec2::new(51.0, 50.0));
    world.tick(1);
    world.tick(600);

    assert!(
        !agent_started_action_against(&world, wolf, ActionType::InitiateHunt, human),
        "well-fed wolf must not bite a human under default mutual avoidance"
    );
    assert!(
        !defender_was_hit(&world, human),
        "human must not be hit when no predator is desperate"
    );
}

/// Asserts the bootstrap fires in a majority of seeds; perception and
/// path noise can keep individual seeds from connecting.
#[ignore = "TODO #716/#744/#746: needs follow-up to migrate behavior to engagement-driven path"]
#[test]
fn starving_wolves_bite_humans_in_majority_of_seeds() {
    let mut hits = 0;
    let trials = 12;

    for seed in 0..trials {
        let mut world = TestWorld::with_seed(seed);
        let human = world.spawn_agent(AgentConfig {
            pos: Vec2::new(50.0, 50.0),
            ..Default::default()
        });
        let wolf = world.spawn_wolf(Vec2::new(51.0, 50.0));
        world.tick(1);
        set_starving(&mut world, wolf);
        world.tick(600);

        if agent_started_action_against(&world, wolf, ActionType::InitiateHunt, human) {
            hits += 1;
        }
    }

    assert!(
        hits as f32 / trials as f32 >= 0.5,
        "starving wolves should bite nearby humans in a majority of seeds: \
         got {hits}/{trials}"
    );
}

#[test]
fn furious_human_with_visible_wolf_proposes_defend_self() {
    let mut world = TestWorld::with_seed(17);
    let human = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        ..Default::default()
    });
    let _wolf = world.spawn_wolf(Vec2::new(51.0, 50.0));
    world.tick(2);

    {
        let mut state = world
            .app_mut()
            .world_mut()
            .get_mut::<EmotionalState>(human)
            .expect("human has emotional state");
        state.add_emotion(Emotion::new(EmotionType::Anger, 1.5));
    }

    world.tick(120);

    assert!(
        agent_started_action(&world, human, ActionType::DefendSelf),
        "saturated-anger human looking at a wolf should start DefendSelf"
    );
}
