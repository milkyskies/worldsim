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
use worldsim::testing::TestWorld;

const HIGH_SOCIAL: f32 = AUTO_INITIATE_SOCIAL_THRESHOLD + 0.1;

#[test]
fn nearby_social_agents_start_conversation() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(100.0, 100.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .agent("bob")
        .pos(Vec2::new(110.0, 100.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .build();

    world.tick(5);

    let alice = agents["alice"];
    let bob = agents["bob"];

    if !world.in_conversation(alice) {
        world.print_recent_events(10);
        panic!("alice should be in a conversation after 5 ticks");
    }
    assert!(world.in_conversation(bob));
    assert_eq!(world.active_conversation_count(), 1);
}

#[test]
fn auto_initiate_emits_conversation_started_sim_event() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(100.0, 100.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .agent("bob")
        .pos(Vec2::new(110.0, 100.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .build();

    world.tick(5);

    let alice = agents["alice"];
    let bob = agents["bob"];

    let started_participants = world
        .sim_events()
        .all()
        .iter()
        .find_map(|e| match e {
            SimEvent::ConversationStarted { participants, .. } => Some(participants.clone()),
            _ => None,
        })
        .expect("CommunicationPlugin must emit SimEvent::ConversationStarted");

    assert!(started_participants.contains(&alice));
    assert!(started_participants.contains(&bob));
}

#[test]
fn distant_agents_do_not_start_conversation() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(64, 64)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(100.0, 100.0))
        .social_drive(0.9)
        .done()
        .agent("bob")
        .pos(Vec2::new(300.0, 300.0))
        .social_drive(0.9)
        .done()
        .build();

    world.tick(10);

    assert!(!world.in_conversation(agents["alice"]));
    assert!(!world.in_conversation(agents["bob"]));
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
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(100.0, 100.0))
        .social_drive(0.1)
        .done()
        .agent("bob")
        .pos(Vec2::new(110.0, 100.0))
        .social_drive(0.1)
        .done()
        .build();

    world.tick(10);

    assert!(!world.in_conversation(agents["alice"]));
    assert!(!world.in_conversation(agents["bob"]));
    assert_eq!(world.active_conversation_count(), 0);
}

#[test]
fn conversation_ends_gracefully_after_enough_turns() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(100.0, 100.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .agent("bob")
        .pos(Vec2::new(110.0, 100.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .build();

    // Turn interval is 30 ticks, natural end at 6 turns -> ~200 ticks.
    world.tick(300);

    let alice = agents["alice"];
    let bob = agents["bob"];

    if world.in_conversation(alice) || world.in_conversation(bob) {
        world.print_conversation(alice);
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
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(100.0, 100.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .agent("bob")
        .pos(Vec2::new(110.0, 100.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .build();

    world.tick(5);

    let alice = agents["alice"];
    assert!(world.in_conversation(alice), "auto-init should have fired");
    let active = world.get::<worldsim::agent::actions::ActiveActions>(alice);
    assert!(
        active.contains(ActionType::Converse),
        "conversing agent should have Converse in active actions"
    );
}
