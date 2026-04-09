//! Scenario tests for the CommunicationPlugin entry point.
//!
//! Verifies the InitiateConversation -> Conversation lifecycle:
//! 1. Emotional brain proposes InitiateConversation when social drive is high
//!    and a person is visible
//! 2. The action walks the agent toward the partner
//! 3. On arrival within CONVERSATION_RANGE the plugin registers a Conversation,
//!    swaps InitiateConversation -> Converse, and inserts InConversation on both
//! 4. SimEvent::ConversationStarted/Ended fire on the observability bus

use bevy::math::Vec2;
use worldsim::agent::actions::ActionType;
use worldsim::agent::events::SimEvent;
use worldsim::agent::nervous_system::config::NervousSystemConfig;
use worldsim::testing::TestWorld;

const HIGH_SOCIAL: f32 = 0.8;
const LOW_SOCIAL: f32 = 0.1;

/// With brains running every tick (see `fast_brains`), 100 ticks gives
/// plenty of time for perception → brain → action → walk → registration.
const TICKS_TO_INITIATE: u64 = 100;

/// Force brains to run every tick so tests don't fight the 60-tick stagger.
fn fast_brains(world: &mut TestWorld) {
    let mut config = world
        .app_mut()
        .world_mut()
        .resource_mut::<NervousSystemConfig>();
    config.thinking_interval = 1;
}

#[test]
fn social_agents_in_vision_range_start_conversation() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(64, 64)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(200.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .agent("bob")
        .pos(Vec2::new(210.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .build();

    fast_brains(&mut world);
    world.tick(TICKS_TO_INITIATE);

    let alice = agents["alice"];
    let bob = agents["bob"];

    if !world.in_conversation(alice) {
        world.print_agent_state(alice);
        world.print_brain_decision(alice);
        world.print_recent_events(50);
        panic!("alice should be in a conversation after {TICKS_TO_INITIATE} ticks");
    }
    assert!(world.in_conversation(bob));
    assert_eq!(world.active_conversation_count(), 1);
}

#[test]
fn initiation_emits_conversation_started_sim_event() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(64, 64)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(200.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .agent("bob")
        .pos(Vec2::new(210.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .build();

    fast_brains(&mut world);
    world.tick(TICKS_TO_INITIATE);

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
fn out_of_vision_agents_do_not_start_conversation() {
    // Vision range for test agents is 100px (see testing::spawn).
    // 300px apart -> they never perceive each other -> no Person belief ->
    // no InitiateConversation proposal.
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(64, 64)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(100.0, 100.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .agent("bob")
        .pos(Vec2::new(500.0, 500.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .build();

    fast_brains(&mut world);
    world.tick(TICKS_TO_INITIATE);

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
        "no ConversationStarted event should fire for agents that never perceive each other"
    );
}

#[test]
fn low_social_drive_agents_do_not_initiate() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(64, 64)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(200.0, 200.0))
        .social_drive(LOW_SOCIAL)
        .done()
        .agent("bob")
        .pos(Vec2::new(210.0, 200.0))
        .social_drive(LOW_SOCIAL)
        .done()
        .build();

    fast_brains(&mut world);
    world.tick(TICKS_TO_INITIATE);

    assert!(!world.in_conversation(agents["alice"]));
    assert!(!world.in_conversation(agents["bob"]));
    assert_eq!(world.active_conversation_count(), 0);
}

#[test]
fn converse_marker_replaces_initiate_on_arrival() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(64, 64)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(200.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .agent("bob")
        .pos(Vec2::new(210.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .build();

    fast_brains(&mut world);
    world.tick(TICKS_TO_INITIATE);

    let alice = agents["alice"];
    assert!(
        world.in_conversation(alice),
        "alice should have initiated by now"
    );
    let active = world.get::<worldsim::agent::actions::ActiveActions>(alice);
    assert!(
        active.contains(ActionType::Converse),
        "Converse marker should occupy the Mouth channel after arrival"
    );
    assert!(
        !active.contains(ActionType::InitiateConversation),
        "InitiateConversation marker should be removed after arrival"
    );
}

#[test]
fn conversation_ends_gracefully_after_enough_turns() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(64, 64)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(200.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .agent("bob")
        .pos(Vec2::new(210.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .build();

    // Initiation (~200) + 6 turns @ 30 ticks each + cleanup ~= 500 ticks.
    fast_brains(&mut world);
    world.tick(600);

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
