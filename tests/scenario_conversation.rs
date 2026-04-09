//! Scenario tests for the CommunicationPlugin.
//!
//! Verifies the conversation lifecycle:
//! 1. Two nearby agents with high social drive auto-initiate a conversation
//! 2. The Converse marker action occupies the Mouth channel
//! 3. Conversations end gracefully after enough turns
//! 4. SimEvent::ConversationStarted/Ended fire on the observability bus

use bevy::math::Vec2;
use worldsim::agent::actions::ActionType;
use worldsim::agent::communication::AUTO_INITIATE_SOCIAL_THRESHOLD;
use worldsim::agent::events::SimEvent;
use worldsim::testing::{AgentConfig, TestWorld};

/// Spawn two agents close enough and social enough to auto-pair.
fn spawn_chatty_pair(world: &mut TestWorld) -> (bevy::prelude::Entity, bevy::prelude::Entity) {
    let a = world.spawn_agent(AgentConfig {
        pos: Vec2::new(100.0, 100.0),
        social_drive: AUTO_INITIATE_SOCIAL_THRESHOLD + 0.1,
        ..Default::default()
    });
    let b = world.spawn_agent(AgentConfig {
        pos: Vec2::new(110.0, 100.0),
        social_drive: AUTO_INITIATE_SOCIAL_THRESHOLD + 0.1,
        ..Default::default()
    });
    (a, b)
}

#[test]
fn nearby_social_agents_start_conversation() {
    let mut world = TestWorld::with_seed(42);
    let (a, b) = spawn_chatty_pair(&mut world);

    world.tick(5);

    if !world.in_conversation(a) {
        world.print_recent_events(10);
        panic!("agent A should be in a conversation after 5 ticks");
    }
    assert!(world.in_conversation(b));
    assert_eq!(world.active_conversation_count(), 1);
}

#[test]
fn auto_initiate_emits_conversation_started_sim_event() {
    let mut world = TestWorld::with_seed(42);
    let (a, b) = spawn_chatty_pair(&mut world);

    world.tick(5);

    let started = world
        .sim_events()
        .all()
        .iter()
        .filter_map(|e| match e {
            SimEvent::ConversationStarted { participants, .. } => Some(participants.clone()),
            _ => None,
        })
        .next();

    let participants =
        started.expect("CommunicationPlugin must emit SimEvent::ConversationStarted");
    assert!(participants.contains(&a));
    assert!(participants.contains(&b));
}

#[test]
fn distant_agents_do_not_start_conversation() {
    let mut world = TestWorld::with_seed(42);

    let a = world.spawn_agent(AgentConfig {
        pos: Vec2::new(100.0, 100.0),
        social_drive: 0.9,
        ..Default::default()
    });
    let b = world.spawn_agent(AgentConfig {
        pos: Vec2::new(300.0, 300.0),
        social_drive: 0.9,
        ..Default::default()
    });

    world.tick(10);

    assert!(!world.in_conversation(a));
    assert!(!world.in_conversation(b));
    assert_eq!(world.active_conversation_count(), 0);

    let started = world
        .sim_events()
        .all()
        .iter()
        .any(|e| matches!(e, SimEvent::ConversationStarted { .. }));
    assert!(
        !started,
        "no ConversationStarted event should fire for distant agents"
    );
}

#[test]
fn low_social_drive_agents_do_not_start_conversation() {
    let mut world = TestWorld::with_seed(42);

    let a = world.spawn_agent(AgentConfig {
        pos: Vec2::new(100.0, 100.0),
        social_drive: 0.1,
        ..Default::default()
    });
    let b = world.spawn_agent(AgentConfig {
        pos: Vec2::new(110.0, 100.0),
        social_drive: 0.1,
        ..Default::default()
    });

    world.tick(10);

    assert!(!world.in_conversation(a));
    assert!(!world.in_conversation(b));
    assert_eq!(world.active_conversation_count(), 0);
}

#[test]
fn conversation_ends_gracefully_after_enough_turns() {
    let mut world = TestWorld::with_seed(42);
    let (a, b) = spawn_chatty_pair(&mut world);

    // Turn interval is 30 ticks, natural end at 6 turns -> ~200 ticks.
    world.tick(300);

    if world.in_conversation(a) || world.in_conversation(b) {
        world.print_conversation(a);
        world.print_recent_events(50);
        panic!("conversation should have ended by now");
    }

    let ended = world
        .sim_events()
        .all()
        .iter()
        .any(|e| matches!(e, SimEvent::ConversationEnded { .. }));
    assert!(
        ended,
        "ConversationEnded SimEvent must fire when a conversation finishes"
    );
}

#[test]
fn converse_marker_occupies_mouth_channel() {
    let mut world = TestWorld::with_seed(42);
    let (a, _b) = spawn_chatty_pair(&mut world);

    world.tick(5);

    assert!(world.in_conversation(a), "auto-init should have fired");
    let active = world.get::<worldsim::agent::actions::ActiveActions>(a);
    assert!(
        active.contains(ActionType::Converse),
        "conversing agent should have Converse in active actions"
    );
}
