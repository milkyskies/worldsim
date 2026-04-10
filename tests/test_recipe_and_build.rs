use bevy::prelude::*;
use worldsim::agent::actions::{ActionRegistry, ActionType, TargetCandidate};
use worldsim::agent::brains::planner::regressive_plan;
use worldsim::agent::brains::thinking::{Goal, TriplePattern};
use worldsim::agent::culture::{Culture, create_cultural_knowledge};
use worldsim::agent::mind::knowledge::{
    Concept, MemoryType, Metadata, MindGraph, Node as MindNode, Predicate, Source, Triple, Value,
    setup_ontology,
};

/// All cultures know the campfire recipe: Wood(3) → Campfire.
#[test]
fn all_cultures_know_campfire_recipe() {
    let ontology = setup_ontology();
    for culture in [
        Culture::Nomad,
        Culture::Farmer,
        Culture::Hunter,
        Culture::Gatherer,
    ] {
        let knowledge = create_cultural_knowledge(culture, &ontology);
        let knows_requires = knowledge.iter().any(|t| {
            t.subject == MindNode::Concept(Concept::Campfire)
                && t.predicate == Predicate::Requires
                && t.object == Value::Item(Concept::Wood, 3)
        });
        assert!(
            knows_requires,
            "{culture:?} should know Campfire requires Wood(3)"
        );
    }
}

/// All cultures know what a campfire provides.
#[test]
fn all_cultures_know_campfire_provides_warmth() {
    let ontology = setup_ontology();
    for culture in [
        Culture::Nomad,
        Culture::Farmer,
        Culture::Hunter,
        Culture::Gatherer,
    ] {
        let knowledge = create_cultural_knowledge(culture, &ontology);
        let knows_provides = knowledge.iter().any(|t| {
            t.subject == MindNode::Concept(Concept::Campfire)
                && t.predicate == Predicate::Provides
                && t.object == Value::Concept(Concept::Warmth)
        });
        assert!(
            knows_provides,
            "{culture:?} should know Campfire provides Warmth"
        );
    }
}

/// The Build action exists in the registry.
#[test]
fn build_action_registered() {
    let registry = ActionRegistry::new();
    assert!(
        registry.get(ActionType::Build).is_some(),
        "Build action must be registered"
    );
}

/// The GOAP planner generates a multi-step plan: Harvest wood → Build campfire.
#[test]
fn goap_plans_harvest_then_build() {
    let ontology = setup_ontology();
    let mut mind = MindGraph::new(ontology.clone());

    // Agent knows a wood log entity exists and contains wood.
    let log_entity = Entity::from_bits(1);
    let log_pos = Vec2::new(64.0, 64.0);

    let cultural_meta = Metadata {
        source: Source::Cultural,
        memory_type: MemoryType::Cultural,
        timestamp: 0,
        confidence: 1.0,
        salience: 0.5,
        ..Default::default()
    };

    // Recipe knowledge
    mind.assert(Triple::with_meta(
        MindNode::Concept(Concept::Campfire),
        Predicate::Requires,
        Value::Item(Concept::Wood, 3),
        cultural_meta,
    ));

    // Agent's current position (required by the planner's implicit tile-walk generation)
    mind.assert(Triple::new(
        MindNode::Self_,
        Predicate::LocatedAt,
        Value::Tile((0, 0)),
    ));

    // World knowledge: a log entity exists with wood
    mind.assert(Triple::new(
        MindNode::Entity(log_entity),
        Predicate::IsA,
        Value::Concept(Concept::WoodLog),
    ));
    mind.assert(Triple::new(
        MindNode::Entity(log_entity),
        Predicate::Contains,
        Value::Item(Concept::Wood, 3),
    ));
    mind.assert(Triple::new(
        MindNode::Entity(log_entity),
        Predicate::LocatedAt,
        Value::Tile((4, 4)), // tile 4,4 at TILE_SIZE=16 → pixel (64, 64)
    ));
    mind.assert(Triple::new(
        MindNode::Concept(Concept::WoodLog),
        Predicate::HasTrait,
        Value::Concept(Concept::Harvestable),
    ));
    mind.assert(Triple::new(
        MindNode::Concept(Concept::WoodLog),
        Predicate::Produces,
        Value::Item(Concept::Wood, 1),
    ));

    let registry = ActionRegistry::new();

    // Build action template (no entity target — agent builds at their location)
    let build_template = registry.get(ActionType::Build).unwrap().to_template(None);

    // Harvest template for the log — go through `to_template_for_target` so the
    // planner sees "produces Wood" (per-target effect) and the auto-injected
    // proximity precondition + entity-content precondition + consumes pattern.
    let harvest_action = registry.get(ActionType::Harvest).unwrap();
    let harvest_target = TargetCandidate::Entity {
        entity: log_entity,
        pos: log_pos,
    };
    let harvest_template = harvest_action.to_template_for_target(&harvest_target, &mind);

    let available = vec![build_template, harvest_template];

    // Goal: self contains Campfire (conceptual — what Build's effect produces)
    let goal = Goal {
        conditions: vec![TriplePattern::new(
            Some(MindNode::Self_),
            Some(Predicate::Contains),
            Some(Value::Item(Concept::Campfire, 1)),
        )],
        priority: 50.0,
    };

    let plan = regressive_plan(&mind, &goal, &available);
    assert!(plan.is_some(), "Planner must find a plan");

    let plan = plan.unwrap();
    assert!(
        !plan.is_empty(),
        "Plan must not be empty (goal is not already met)"
    );

    let has_build = plan.iter().any(|a| a.action_type == ActionType::Build);
    assert!(has_build, "Plan must include a Build step");

    let has_harvest = plan.iter().any(|a| a.action_type == ActionType::Harvest);
    assert!(has_harvest, "Plan must include a Harvest step");

    // Harvest must come before Build in execution order
    let harvest_idx = plan
        .iter()
        .position(|a| a.action_type == ActionType::Harvest);
    let build_idx = plan.iter().position(|a| a.action_type == ActionType::Build);
    assert!(
        harvest_idx < build_idx,
        "Harvest must precede Build in the plan"
    );
}

/// Build action consumes Wood from inventory and signals a campfire should be spawned.
#[test]
fn build_action_consumes_wood() {
    use worldsim::agent::actions::registry::{ActionContext, CompletionContext, SpawnRequest};
    use worldsim::agent::body::needs::PhysicalNeeds;
    use worldsim::agent::item_slots::ItemSlots;
    use worldsim::world::map::{WORLD_HEIGHT, WORLD_WIDTH, WorldMap};

    let ontology = setup_ontology();
    let mind = MindGraph::new(ontology);
    let registry = ActionRegistry::new();
    let build_action = registry.get(ActionType::Build).unwrap();

    let mut inventory = ItemSlots::agent_carry();
    inventory.add(Concept::Wood, 3);

    let mut physical = PhysicalNeeds::default();
    let mut spawn_requests: Vec<SpawnRequest> = Vec::new();

    let world_map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);

    let can_start_ctx = ActionContext {
        inventory: &inventory,
        mind: &mind,
        world_map: &world_map,
        target_entity: None,
        target_position: None,
        agent_position: Vec2::ZERO,
    };

    assert!(
        build_action.can_start(&can_start_ctx).is_ok(),
        "Build must be startable when agent has Wood(3)"
    );

    let mut ctx = CompletionContext {
        physical: &mut physical,
        inventory: &mut inventory,
        drives: None,
        target_inventory: None,
        target_entity: None,
        tick: 0,
        agent_position: Vec2::ZERO,
        spawn_requests: &mut spawn_requests,
    };

    build_action.on_complete(&mut ctx);

    assert_eq!(
        inventory.count(Concept::Wood),
        0,
        "Build must consume all required Wood from inventory"
    );

    // Build now spawns a construction site (target = Campfire) with the
    // materials pre-deposited rather than spawning the campfire directly.
    // The Becomes substrate transforms the filled site into a campfire on the
    // next tick — see test_becomes_substrate.rs for that path.
    assert!(
        spawn_requests.iter().any(|r| matches!(
            r,
            SpawnRequest::Site {
                target: Concept::Campfire,
                ..
            }
        )),
        "Build must request a Campfire construction site to be spawned"
    );
}

/// Build cannot start without required materials.
#[test]
fn build_fails_without_materials() {
    use worldsim::agent::actions::registry::ActionContext;
    use worldsim::agent::item_slots::ItemSlots;
    use worldsim::world::map::{WORLD_HEIGHT, WORLD_WIDTH, WorldMap};

    let ontology = setup_ontology();
    let mind = MindGraph::new(ontology);
    let registry = ActionRegistry::new();
    let build_action = registry.get(ActionType::Build).unwrap();

    let empty_inventory = ItemSlots::agent_carry();
    let world_map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);

    let ctx = ActionContext {
        inventory: &empty_inventory,
        mind: &mind,
        world_map: &world_map,
        target_entity: None,
        target_position: None,
        agent_position: Vec2::ZERO,
    };

    assert!(
        build_action.can_start(&ctx).is_err(),
        "Build must fail when agent has no materials"
    );
}
