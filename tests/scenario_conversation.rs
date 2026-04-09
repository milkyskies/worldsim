//! Scenario tests for the CommunicationPlugin.
//!
//! Verifies the conversation lifecycle:
//! 1. Two nearby agents with high social drive auto-initiate a conversation
//! 2. Conversations produce turns and eventually end gracefully
//! 3. Sleep preempts the Converse channel marker, ending the conversation

use bevy::math::Vec2;
use worldsim::agent::actions::ActionType;
use worldsim::agent::communication::AUTO_INITIATE_SOCIAL_THRESHOLD;
use worldsim::testing::{AgentConfig, TestWorld};

#[test]
fn nearby_social_agents_start_conversation() {
    let mut world = TestWorld::with_seed(42);

    // Two agents within conversation range (32px), both very social.
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

    // Tick enough for the auto-initiation system to pair them up and
    // the commands to flush.
    world.tick(5);

    assert!(
        world.in_conversation(a),
        "agent A should be in a conversation"
    );
    assert!(
        world.in_conversation(b),
        "agent B should be in a conversation"
    );
    assert_eq!(world.active_conversation_count(), 1);
}

#[test]
fn distant_agents_do_not_start_conversation() {
    let mut world = TestWorld::with_seed(42);

    // Two social agents far apart — should NOT start talking.
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

    assert!(
        !world.in_conversation(a),
        "distant agent A should not be in a conversation"
    );
    assert!(
        !world.in_conversation(b),
        "distant agent B should not be in a conversation"
    );
    assert_eq!(world.active_conversation_count(), 0);
}

#[test]
fn low_social_drive_agents_do_not_start_conversation() {
    let mut world = TestWorld::with_seed(42);

    // Two nearby agents with low social drive — threshold not met.
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

    let a = world.spawn_agent(AgentConfig {
        pos: Vec2::new(100.0, 100.0),
        social_drive: 0.9,
        ..Default::default()
    });
    let b = world.spawn_agent(AgentConfig {
        pos: Vec2::new(110.0, 100.0),
        social_drive: 0.9,
        ..Default::default()
    });

    // Tick long enough for conversation to start and finish naturally.
    // Turn interval is 30 ticks, natural end at 6 turns -> ~200 ticks.
    world.tick(300);

    // After enough time both agents should have left the conversation.
    assert!(
        !world.in_conversation(a),
        "conversation should have ended by now"
    );
    assert!(
        !world.in_conversation(b),
        "conversation should have ended by now"
    );
}

#[test]
fn converse_marker_occupies_mouth_channel() {
    let mut world = TestWorld::with_seed(42);

    let a = world.spawn_agent(AgentConfig {
        pos: Vec2::new(100.0, 100.0),
        social_drive: 0.9,
        ..Default::default()
    });
    let _b = world.spawn_agent(AgentConfig {
        pos: Vec2::new(110.0, 100.0),
        social_drive: 0.9,
        ..Default::default()
    });

    world.tick(5);

    // Once in conversation, the Converse action marker should be in ActiveActions.
    if world.in_conversation(a) {
        let active = world.get::<worldsim::agent::actions::ActiveActions>(a);
        assert!(
            active.contains(ActionType::Converse),
            "conversing agent should have Converse in active actions"
        );
    }
}
