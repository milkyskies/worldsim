//! Scenario tests for the CommunicationPlugin entry point.
//!
//! Verifies the InitiateConversation -> Conversation lifecycle:
//! 1. Emotional brain proposes InitiateConversation when social drive is high
//!    and a person is visible
//! 2. The action walks the agent toward the partner
//! 3. On arrival within CONVERSATION_RANGE the plugin registers a Conversation,
//!    swaps InitiateConversation -> Converse, and inserts InConversation on both
//! 4. SimEvent::ConversationStarted/Ended fire on the observability bus
//!
//! Intent selection tests (issue #46):
//! 5. After a Greet turn (expects_response=true), the partner uses Answer intent
//! 6. An agent with personal high-salience danger knowledge warns their partner

use bevy::math::Vec2;
use worldsim::agent::actions::ActionType;
use worldsim::agent::events::SimEvent;
use worldsim::agent::mind::conversation::{ConversationManager, Intent};
use worldsim::agent::mind::knowledge::{
    Concept, MemoryType, Metadata, MindGraph, Node, Predicate, Source, Triple, Value,
};
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
fn conversations_can_end_gracefully_after_enough_turns() {
    // Verify the conversation lifecycle reaches its natural end at least once.
    // With fast_brains the agents may re-initiate immediately after — this
    // test asserts that the lifecycle CAN complete, not that they stop talking.
    let (mut world, _agents) = TestWorld::scenario(42)
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
    world.tick(600);

    let started_count = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| matches!(e, SimEvent::ConversationStarted { .. }))
        .count();
    let ended_count = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| matches!(e, SimEvent::ConversationEnded { .. }))
        .count();

    assert!(
        started_count >= 1,
        "expected at least one ConversationStarted, got {started_count}"
    );
    assert!(
        ended_count >= 1,
        "expected at least one ConversationEnded SimEvent, got {ended_count}"
    );
}

// ─── Intent selection tests (#46) ────────────────────────────────────────────

/// After the first Greet turn (which sets `expects_response = true`), the
/// partner's next turn must use `Intent::Answer`. This verifies that the
/// priority order in `select_intent` routes through Answer before falling
/// back to Share or Acknowledge.
#[test]
fn second_turn_intent_is_answer_after_greet() {
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
    world.tick(200);

    let alice = agents["alice"];

    // Conversations may have ended by tick 200 — check the full history.
    let had_conversation = world
        .sim_events()
        .all()
        .iter()
        .any(|e| matches!(e, SimEvent::ConversationStarted { .. }));
    if !had_conversation {
        world.print_agent_state(alice);
        world.print_recent_events(200);
        panic!("alice and bob should have started a conversation within 200 ticks");
    }

    // ConversationManager retains ended conversations — read all turns.
    let app_world = world.app().world();
    let manager = app_world.resource::<ConversationManager>();
    let intents: Vec<Intent> = manager
        .conversations
        .values()
        .flat_map(|c| c.turns.iter().map(|t| t.intent))
        .collect();

    assert!(
        intents.contains(&Intent::Answer),
        "expected Answer turn after Greet — got: {intents:?}"
    );
}

/// An agent who has personally observed a high-salience danger (wolf nearby)
/// should warn their conversation partner via `Intent::Share` with
/// `Topic::Help` content. After the conversation the partner's personal
/// MindGraph should contain the wolf-danger triple as hearsay.
#[test]
fn agent_warns_partner_about_personally_observed_danger() {
    // High-salience personal observation that wolves are dangerous. Alice has
    // experienced this directly; bob only carries the abstract ontology fact.
    // The novelty check in `pick_deliberate_content` ignores ontology entries
    // (a primitive stranger model — see #67), so this triple still scores as
    // novel content for bob and gets delivered as the warn payload.
    let wolf_danger_triple = Triple::with_meta(
        Node::Concept(Concept::Wolf),
        Predicate::HasTrait,
        Value::Concept(Concept::Dangerous),
        Metadata {
            source: Source::Experienced,
            memory_type: MemoryType::Episodic,
            timestamp: 0,
            confidence: 1.0,
            informant: None,
            evidence: Vec::new(),
            salience: 0.9, // above DANGER_WARN_SALIENCE (0.7)
            source_sense: None,
            strength: 1.0,
        },
    );

    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(64, 64)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(200.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .knowledge(vec![wolf_danger_triple])
        .done()
        .agent("bob")
        .pos(Vec2::new(210.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .build();

    fast_brains(&mut world);
    world.tick(300);

    let alice = agents["alice"];
    let bob = agents["bob"];

    // Verify a conversation occurred — it may have ended by tick 200.
    let had_conversation = world
        .sim_events()
        .all()
        .iter()
        .any(|e| matches!(e, SimEvent::ConversationStarted { .. }));
    if !had_conversation {
        world.print_agent_state(alice);
        world.print_recent_events(200);
        panic!("alice and bob should have started a conversation within 200 ticks");
    }

    // Bob should have received the wolf-danger triple as hearsay from alice.
    // We check personal triples only — bob's ontology already says wolves are
    // dangerous abstractly, but the warn delivers a specific personal record
    // attributed to alice.
    let bob_mind = world.get::<MindGraph>(bob);
    let bob_received_warning = bob_mind.iter().any(|t| {
        t.predicate == Predicate::HasTrait
            && t.object == Value::Concept(Concept::Dangerous)
            && t.subject == Node::Concept(Concept::Wolf)
            && t.meta.informant == Some(alice)
    });

    if !bob_received_warning {
        world.print_conversation(alice);
        world.print_mind_graph(bob);
        panic!("bob should have received wolf-danger warning from alice as hearsay");
    }
}

// ─── Group conversation tests (#65) ──────────────────────────────────────────

/// Three social agents clustered in vision range form a single group
/// conversation, not three disjoint pair conversations.
#[test]
fn three_social_agents_form_single_group_conversation() {
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
        .agent("carol")
        .pos(Vec2::new(220.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .build();

    fast_brains(&mut world);
    world.tick(TICKS_TO_INITIATE);

    let alice = agents["alice"];
    let bob = agents["bob"];
    let carol = agents["carol"];

    // All three should end up in conversation.
    if !world.in_conversation(alice) || !world.in_conversation(bob) || !world.in_conversation(carol)
    {
        world.print_recent_events(TICKS_TO_INITIATE);
        panic!("all three agents should be in a conversation after {TICKS_TO_INITIATE} ticks");
    }

    // And it should be the *same* conversation — one group, not three pairs.
    let manager = world.app().world().resource::<ConversationManager>();
    let active: Vec<&worldsim::agent::mind::conversation::Conversation> =
        manager.active_conversations().collect();
    assert_eq!(
        active.len(),
        1,
        "expected a single group conversation, got {}",
        active.len()
    );
    let group = active[0];
    assert_eq!(
        group.participants.len(),
        3,
        "group should contain all three agents, got {:?}",
        group.participants
    );
    assert!(group.participants.contains(&alice));
    assert!(group.participants.contains(&bob));
    assert!(group.participants.contains(&carol));
}

/// A ConversationJoined SimEvent fires when a third agent joins an active
/// 1-on-1 conversation.
#[test]
fn third_agent_joining_emits_conversation_joined_event() {
    let (mut world, _agents) = TestWorld::scenario(42)
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
        .agent("carol")
        .pos(Vec2::new(220.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .build();

    fast_brains(&mut world);
    world.tick(TICKS_TO_INITIATE);

    let joined = world
        .sim_events()
        .all()
        .iter()
        .any(|e| matches!(e, SimEvent::ConversationJoined { .. }));
    assert!(
        joined,
        "ConversationJoined should fire when a third agent joins an active conversation"
    );
}

/// A speaker's shared knowledge reaches every listener in the group, not
/// just one. This is the core "broadcast to all listeners" property from #65.
#[test]
fn shared_knowledge_broadcasts_to_all_group_listeners() {
    let wolf_danger_triple = Triple::with_meta(
        Node::Concept(Concept::Wolf),
        Predicate::HasTrait,
        Value::Concept(Concept::Dangerous),
        Metadata {
            source: Source::Experienced,
            memory_type: MemoryType::Episodic,
            timestamp: 0,
            confidence: 1.0,
            informant: None,
            evidence: Vec::new(),
            salience: 0.9,
            source_sense: None,
        },
    );

    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(64, 64)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(200.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .knowledge(vec![wolf_danger_triple])
        .done()
        .agent("bob")
        .pos(Vec2::new(210.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .agent("carol")
        .pos(Vec2::new(220.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .build();

    fast_brains(&mut world);
    world.tick(300);

    let alice = agents["alice"];
    let bob = agents["bob"];
    let carol = agents["carol"];

    let received_from_alice = |e| {
        let mind: &MindGraph = world.get::<MindGraph>(e);
        mind.iter().any(|t| {
            t.predicate == Predicate::HasTrait
                && t.object == Value::Concept(Concept::Dangerous)
                && t.subject == Node::Concept(Concept::Wolf)
                && t.meta.informant == Some(alice)
        })
    };

    let bob_heard = received_from_alice(bob);
    let carol_heard = received_from_alice(carol);

    if !bob_heard || !carol_heard {
        world.print_conversation(alice);
        world.print_mind_graph(bob);
        world.print_mind_graph(carol);
        panic!(
            "both listeners should have received the warning — bob_heard={bob_heard} carol_heard={carol_heard}"
        );
    }
}

/// When enough participants leave, a group conversation collapses back into
/// a 2-person conversation (rather than ending entirely) and eventually
/// ends when the count drops below 2.
#[test]
fn group_shrinks_then_ends_as_participants_leave() {
    use worldsim::agent::mind::conversation::{Conversation, ConversationManager};

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
        .agent("carol")
        .pos(Vec2::new(220.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .build();

    fast_brains(&mut world);
    world.tick(TICKS_TO_INITIATE);

    // All three should be in one conversation.
    {
        let manager = world.app().world().resource::<ConversationManager>();
        let group: Option<&Conversation> = manager.active_conversations().next();
        assert!(group.is_some(), "no active conversation after init phase");
        assert_eq!(
            group.unwrap().participants.len(),
            3,
            "group should start with three participants"
        );
    }

    // Teleport carol far away — she loses range to the group and should be
    // removed from it on the next tick.
    world
        .get_mut::<bevy::prelude::Transform>(agents["carol"])
        .translation = bevy::prelude::Vec3::new(600.0, 600.0, 0.0);
    world.tick(5);

    {
        let manager = world.app().world().resource::<ConversationManager>();
        let active: Vec<&Conversation> = manager.active_conversations().collect();
        assert_eq!(
            active.len(),
            1,
            "conversation should still be active with alice+bob"
        );
        assert_eq!(
            active[0].participants.len(),
            2,
            "group should have shrunk to two participants after carol left"
        );
        assert!(!active[0].participants.contains(&agents["carol"]));
    }

    // Now teleport bob away too — the last remaining pair drops to a single
    // participant and the conversation must end.
    world
        .get_mut::<bevy::prelude::Transform>(agents["bob"])
        .translation = bevy::prelude::Vec3::new(700.0, 700.0, 0.0);
    world.tick(5);

    let manager = world.app().world().resource::<ConversationManager>();
    let active_count = manager.active_conversations().count();
    assert_eq!(
        active_count, 0,
        "conversation should have ended once only one participant remained"
    );
}
