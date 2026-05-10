//! Integration tests for the hunting loop (#225).
//!
//! Verifies that a hungry hunter (or wolf) with the right knowledge can plan
//! and execute the full chain from "I'm hungry" to "I ate meat from a deer."
//! The plan emerges from the existing GOAP planner the moment the agent's
//! mind has the three triples that knit hunting together:
//!
//! ```
//! (Deer, HasTrait, Prey)            -- huntable target
//! (Deer, Produces, Item(Meat, 1))   -- yields meat when killed
//! (Meat, IsA, Food)                 -- meat is edible (lives in ontology)
//! ```
//!
//! After a kill the deer entity transforms in-place into a Corpse
//! (`Becomes` substrate, `InPlace` mode) so the entity ID survives — future
//! episodic memory and relationship triples keep pointing at a meaningful
//! entity even after death.

use bevy::prelude::*;
use worldsim::agent::actions::ActionType;
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::culture::{Culture, create_cultural_knowledge};
use worldsim::agent::events::{SimEvent, SimEventKind};
use worldsim::agent::inventory::EntityType;
use worldsim::agent::item_slots::ItemSlots;
use worldsim::agent::mind::knowledge::Concept;
use worldsim::agent::mind::perception::Vision;
use worldsim::testing::{AgentConfig, TestWorld};

/// Neuter the deer's brain so it can't move or react, while keeping
/// all ECS components intact (Agent, Physical, GlobalTransform, etc.)
/// so perception and spatial index queries still find it. We zero its
/// vision so it never perceives threats (no flee) and remove its
/// RationalBrain so the three_brains_system can't produce a Wander.
fn pin_deer(world: &mut TestWorld, deer: Entity) {
    {
        let mut vision = world
            .app_mut()
            .world_mut()
            .get_mut::<Vision>(deer)
            .expect("deer should have Vision");
        vision.range = 0.0;
    }
    world
        .app_mut()
        .world_mut()
        .entity_mut(deer)
        .remove::<worldsim::agent::brains::rational::RationalBrain>()
        .remove::<worldsim::agent::nervous_system::cns::CentralNervousSystem>();
}

/// True if any ActionStarted SimEvent for `agent` matches `action_type`.
fn agent_started_action(world: &TestWorld, agent: Entity, action_type: ActionType) -> bool {
    world.sim_events().all().iter().any(|e| {
        matches!(
            e,
            SimEvent { kind: SimEventKind::ActionStarted { agent: a, action, .. }, .. }
                if *a == agent && *action == action_type
        )
    })
}

/// A hungry agent with hunter cultural knowledge and a nearby deer should
/// plan and execute the full chain with hunger satisfied at the end. After
/// the kill, the deer entity is transformed in-place into a Corpse holding
/// meat for scavengers — the entity ID survives the transition.
#[test]
#[ignore = "flaky: #477"]
fn hungry_hunter_kills_and_eats_nearby_deer() {
    let mut world = TestWorld::with_seed(42);

    // Spawn the deer first and tick once so the spatial index registers it.
    let deer = world.spawn_deer(Vec2::new(50.0, 50.0));
    pin_deer(&mut world, deer);
    world.tick(2);

    let hunter = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        // Urgency 0.85 (not 0.95): the effort model now assigns energy
        // cost to combat actions, so a nearly-starving hunter at 0.95
        // may exhaust glucose before finishing the kill. 0.85 is hungry
        // enough to drive the plan but leaves enough reserves for the
        // multi-cycle attack and subsequent Eat.
        metabolism: worldsim::agent::body::metabolism::Metabolism::at_urgency(0.85),
        knowledge: create_cultural_knowledge(Culture::Hunter),
        ..Default::default()
    });

    // Real-damage combat takes many attack cycles to drop a deer via
    // incapacitation (function_rate < 0.2 on a vital part). 3000 ticks
    // matches the wolf test's budget, which needs the same headroom.
    world.tick(3000);

    let final_hunger = world.agent_hunger(hunter);

    assert!(
        agent_started_action(&world, hunter, ActionType::Walk),
        "hunter should have walked toward the deer"
    );
    assert!(
        agent_started_action(&world, hunter, ActionType::Attack),
        "hunter should have started Attack on the deer"
    );
    assert!(
        agent_started_action(&world, hunter, ActionType::Eat),
        "hunter should have eaten the meat"
    );
    assert!(
        final_hunger < 0.95,
        "hunter hunger should drop after eating meat (got {final_hunger})"
    );

    // The deer entity must still exist — in-place transformation preserves
    // identity. It is now a Corpse holding meat for scavengers.
    assert!(
        world.entity_exists(deer),
        "deer entity should survive the kill (in-place Corpse transformation)"
    );
    let entity_type = world.get::<EntityType>(deer);
    assert_eq!(
        entity_type.0,
        Concept::Corpse,
        "the slain deer should now be classified as a Corpse"
    );
    let corpse_inventory = world.get::<ItemSlots>(deer);
    assert!(
        corpse_inventory.count(Concept::Meat) > 0,
        "the corpse should hold meat for harvesting"
    );
}

/// A hungry wolf with no cultural knowledge should plan and execute the
/// same chain via Bite, since wolf-intrinsic knowledge already mirrors
/// the hunter culture's prey/produces triples.
#[ignore = "TODO #716/#744/#746: needs follow-up to migrate behavior to engagement-driven path"]
#[test]
fn hungry_wolf_kills_and_eats_nearby_deer() {
    let mut world = TestWorld::with_seed(42);

    let deer = world.spawn_deer(Vec2::new(50.0, 50.0));
    pin_deer(&mut world, deer);
    world.tick(5);

    let wolf = world.spawn_wolf(Vec2::new(50.0, 50.0));
    world.tick(1);

    {
        let mut needs = world
            .app_mut()
            .world_mut()
            .get_mut::<PhysicalNeeds>(wolf)
            .expect("wolf should have PhysicalNeeds");
        needs.metabolism = worldsim::agent::body::metabolism::Metabolism::at_urgency(0.95);
    }

    world.tick(3000);

    let final_hunger = world
        .app()
        .world()
        .get::<PhysicalNeeds>(wolf)
        .expect("wolf still has needs")
        .hunger_urgency();

    assert!(
        agent_started_action(&world, wolf, ActionType::Walk),
        "wolf should have walked toward the deer"
    );
    assert!(
        agent_started_action(&world, wolf, ActionType::InitiateHunt),
        "wolf should have bitten the deer"
    );
    assert!(
        agent_started_action(&world, wolf, ActionType::Eat),
        "wolf should have eaten the meat"
    );
    assert!(
        final_hunger < 0.95,
        "wolf hunger should drop after eating meat (got {final_hunger})"
    );
    assert!(
        world.entity_exists(deer),
        "deer entity should survive the kill (in-place Corpse transformation)"
    );
    let entity_type = world.get::<EntityType>(deer);
    assert_eq!(entity_type.0, Concept::Corpse);
}

/// Sanity check on the planner's symbol layer: a fresh hunter mind has
/// every triple needed to chain hunger → meat → eat without any further
/// world state. If this fails, the higher-level scenario tests will fail
/// with a much more confusing symptom.
#[test]
fn hunter_culture_grants_full_hunting_chain() {
    use worldsim::agent::mind::knowledge::{Node, Predicate, Value, setup_ontology};

    let triples = create_cultural_knowledge(Culture::Hunter);
    let has = |sub, pred, obj| {
        triples
            .iter()
            .any(|t: &worldsim::agent::mind::knowledge::Triple| {
                t.subject == sub && t.predicate == pred && t.object == obj
            })
    };

    assert!(
        has(
            Node::Concept(Concept::Deer),
            Predicate::HasTrait,
            Value::Concept(Concept::Prey)
        ),
        "hunter should know Deer HasTrait Prey"
    );
    assert!(
        has(
            Node::Concept(Concept::Deer),
            Predicate::Produces,
            Value::Item(Concept::Meat, 1)
        ),
        "hunter should know Deer Produces Meat"
    );

    // The category-error triples from the broken implementation must be gone.
    assert!(
        !has(
            Node::Concept(Concept::Animal),
            Predicate::IsA,
            Value::Concept(Concept::Food)
        ),
        "hunter must not assert Animal IsA Food (a live animal is not food)"
    );
    assert!(
        !has(
            Node::Concept(Concept::Animal),
            Predicate::HasTrait,
            Value::Concept(Concept::Harvestable)
        ),
        "hunter must not assert Animal HasTrait Harvestable"
    );

    // Meat IsA Food belongs in the ontology — universally true, not cultural.
    let ontology = setup_ontology();
    assert!(
        ontology.is_a(Concept::Meat, Concept::Food),
        "ontology should classify Meat IsA Food"
    );
    assert!(
        ontology.is_a(Concept::Meat, Concept::Resource),
        "ontology should classify Meat IsA Resource"
    );
}

/// `spawn_concept_entity(Corpse, ...)` should produce a harvestable corpse
/// holding meat — used by the Becomes substrate's `Replace` mode for the
/// rare case where a corpse is summoned standalone.
#[test]
fn spawn_concept_entity_corpse_creates_harvestable_meat_entity() {
    use worldsim::agent::affordance::Affordance;

    let mut world = TestWorld::with_seed(0);

    let corpse = {
        let world_mut = world.app_mut().world_mut();
        let palette = world_mut.resource::<worldsim::palette::Palette>().clone();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let entity = {
            let mut commands = Commands::new(&mut queue, world_mut);
            worldsim::world::spawn::spawn_concept_entity(
                &mut commands,
                &palette,
                Concept::Corpse,
                Vec2::new(40.0, 40.0),
                0,
            )
            .expect("Corpse should be spawnable")
        };
        queue.apply(world_mut);
        entity
    };

    let entity_type = world.get::<EntityType>(corpse);
    assert_eq!(entity_type.0, Concept::Corpse);

    let inventory = world.get::<ItemSlots>(corpse);
    assert!(
        inventory.count(Concept::Meat) > 0,
        "corpse should hold meat"
    );

    let affordance = world.get::<Affordance>(corpse);
    assert_eq!(
        affordance.action_type,
        ActionType::Harvest,
        "corpse should expose a Harvest affordance for scavengers"
    );
}
