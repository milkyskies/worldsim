use bevy::prelude::*;
use worldsim::agent::actions::{ActionRegistry, ActionType, TargetCandidate};
use worldsim::agent::brains::planner::{PlanCostContext, regressive_plan};
use worldsim::agent::brains::thinking::{Goal, TriplePattern};
use worldsim::agent::culture::{Culture, create_cultural_knowledge};
use worldsim::agent::mind::knowledge::{
    Concept, MemoryType, Metadata, MindGraph, Node as MindNode, Predicate, Source, Triple, Value,
    setup_ontology,
};

/// All cultures know the campfire recipe.
#[test]
fn all_cultures_know_campfire_recipe() {
    use worldsim::constants::actions::build::CAMPFIRE_WOOD_REQUIRED;

    for culture in [
        Culture::Nomad,
        Culture::Farmer,
        Culture::Hunter,
        Culture::Gatherer,
    ] {
        let knowledge = create_cultural_knowledge(culture);
        let knows_requires = knowledge.iter().any(|t| {
            t.subject == MindNode::Concept(Concept::Campfire)
                && t.predicate == Predicate::Requires
                && t.object == Value::Item(Concept::Wood, CAMPFIRE_WOOD_REQUIRED)
        });
        assert!(
            knows_requires,
            "{culture:?} should know Campfire requires Wood({CAMPFIRE_WOOD_REQUIRED})"
        );
    }
}

/// All cultures know what a campfire provides.
#[test]
fn all_cultures_know_campfire_provides_warmth() {
    for culture in [
        Culture::Nomad,
        Culture::Farmer,
        Culture::Hunter,
        Culture::Gatherer,
    ] {
        let knowledge = create_cultural_knowledge(culture);
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

/// The GOAP planner generates a multi-step plan: Harvest wood × N → Build campfire.
/// With the at-least quantity rule + partial-satisfaction backward search,
/// the planner chains `CAMPFIRE_WOOD_REQUIRED` harvests (one per log) to
/// accumulate enough wood for Build. Per-entity consume patterns prevent
/// double-harvesting any individual log; quantity accumulation across
/// distinct entities is the intended GOAP path.
#[test]
fn goap_plans_harvest_then_build() {
    use worldsim::constants::actions::build::CAMPFIRE_WOOD_REQUIRED;

    let ontology = setup_ontology();
    let mut mind = MindGraph::new(ontology.clone());

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
        Value::Item(Concept::Wood, CAMPFIRE_WOOD_REQUIRED),
        cultural_meta,
    ));

    // Agent's current position (required by the planner's implicit tile-walk generation)
    mind.assert(Triple::new(
        MindNode::Self_,
        Predicate::LocatedAt,
        Value::Tile((0, 0)),
    ));

    // Type-level ontology: WoodLog is harvestable and yields Wood(1) per harvest.
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

    // Spawn `CAMPFIRE_WOOD_REQUIRED` distinct log entities so the planner
    // has one harvestable per required wood unit — per-entity consume
    // tracking prevents re-harvesting any single log within one plan.
    let mut harvest_templates = Vec::new();
    for i in 0..CAMPFIRE_WOOD_REQUIRED {
        let log_entity = Entity::from_bits(1 + i as u64);
        let log_pos = Vec2::new(64.0 + i as f32 * 16.0, 64.0);
        mind.assert(Triple::new(
            MindNode::Entity(log_entity),
            Predicate::IsA,
            Value::Concept(Concept::WoodLog),
        ));
        mind.assert(Triple::new(
            MindNode::Entity(log_entity),
            Predicate::Contains,
            Value::Item(Concept::Wood, 1),
        ));
        mind.assert(Triple::new(
            MindNode::Entity(log_entity),
            Predicate::LocatedAt,
            Value::Tile((4 + i as i32, 4)),
        ));

        let registry = ActionRegistry::new();
        let harvest_action = registry.get(ActionType::Harvest).unwrap();
        let harvest_target = TargetCandidate::Entity {
            entity: log_entity,
            pos: log_pos,
        };
        harvest_templates.push(harvest_action.to_template_for_target(&harvest_target, &mind));
    }

    let registry = ActionRegistry::new();
    let build_template = registry.get(ActionType::Build).unwrap().to_template(None);

    let mut available = vec![build_template];
    available.extend(harvest_templates);

    let goal = Goal {
        conditions: vec![TriplePattern::new(
            Some(MindNode::Self_),
            Some(Predicate::Near),
            Some(Value::Concept(Concept::Campfire)),
        )],
        priority: 50.0,
    };

    let (plan, _) = regressive_plan(&mind, &goal, &available, &PlanCostContext::neutral());
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
    use worldsim::constants::actions::build::CAMPFIRE_WOOD_REQUIRED;
    use worldsim::world::map::{WORLD_HEIGHT, WORLD_WIDTH, WorldMap};

    let ontology = setup_ontology();
    let mind = MindGraph::new(ontology);
    let registry = ActionRegistry::new();
    let build_action = registry.get(ActionType::Build).unwrap();

    let mut inventory = ItemSlots::agent_carry();
    inventory.add(Concept::Wood, CAMPFIRE_WOOD_REQUIRED);

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
        physical: None,
        drives: None,
        emotional: None,
        current_tick: 0,
        unreachable_tiles: &[],
    };

    assert!(
        build_action.can_start(&can_start_ctx).is_ok(),
        "Build must be startable when agent has the required wood"
    );

    let mut ctx = CompletionContext {
        physical: &mut physical,
        inventory: &mut inventory,
        drives: None,
        mind: &mind,
        skills: None,
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
        physical: None,
        drives: None,
        emotional: None,
        current_tick: 0,
        unreachable_tiles: &[],
    };

    assert!(
        build_action.can_start(&ctx).is_err(),
        "Build must fail when agent has no materials"
    );
}
