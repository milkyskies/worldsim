//! Integration tests for the Skills system (#336).
//!
//! Unit tests for the learning-curve and decay math live in
//! `src/agent/skills.rs`. This file covers the ECS wiring:
//!   - `Skills` is attached to freshly-spawned agents.
//!   - The decay system fires on its tick interval and pulls disused
//!     skills toward the configured floor.
//!   - The grace window skips recently-practiced skills.
//!   - `HarvestAction::on_complete` scales yield by the harvester's
//!     Harvesting skill level.

use bevy::prelude::*;
use worldsim::agent::actions::GenericAction;
use worldsim::agent::actions::action::HARVEST_DEF;
use worldsim::agent::actions::registry::{Action, CompletionContext, SpawnRequest};
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::item_slots::ItemSlots;
use worldsim::agent::mind::knowledge::{Concept, MindGraph, Ontology};
use worldsim::agent::skills::{SkillKind, Skills, SkillsConfig};
use worldsim::testing::{AgentConfig, TestWorld};

fn get_skills(world: &TestWorld, agent: Entity) -> Skills {
    world
        .app()
        .world()
        .get::<Skills>(agent)
        .cloned()
        .expect("agent should have a Skills component")
}

fn set_harvesting_level(world: &mut TestWorld, agent: Entity, level: f32, tick: u64) {
    let mut skills = world
        .app_mut()
        .world_mut()
        .get_mut::<Skills>(agent)
        .expect("agent should have Skills");
    skills.set_level(SkillKind::Harvesting, level, tick);
}

/// Override SkillsConfig with fast-decay values so the test can tick a
/// handful of ticks instead of marching through a full game day.
fn set_fast_decay(
    world: &mut TestWorld,
    interval_ticks: u64,
    grace_ticks: u64,
    step_days: f32,
    half_life_days: f32,
    floor: f32,
) {
    let mut cfg = world.app_mut().world_mut().resource_mut::<SkillsConfig>();
    cfg.decay_interval_ticks = interval_ticks;
    cfg.decay_grace_ticks = grace_ticks;
    cfg.decay_step_days = step_days;
    cfg.decay_half_life_days = half_life_days;
    cfg.decay_floor = floor;
}

// ─── Component wiring ──────────────────────────────────────────────────────

#[test]
fn agents_spawn_with_empty_skills_component() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::default());

    let skills = get_skills(&world, agent);
    assert_eq!(
        skills.level(SkillKind::Harvesting),
        0.0,
        "harvest skill should start at zero"
    );
    assert_eq!(
        skills.level(SkillKind::Combat),
        0.0,
        "combat skill should start at zero"
    );
}

#[test]
fn deer_and_wolf_also_spawn_with_skills() {
    let mut world = TestWorld::with_seed(42);
    let deer = world.spawn_deer(Vec2::new(10.0, 10.0));
    let wolf = world.spawn_wolf(Vec2::new(20.0, 20.0));

    assert!(
        world.app().world().get::<Skills>(deer).is_some(),
        "deer should have Skills component"
    );
    assert!(
        world.app().world().get::<Skills>(wolf).is_some(),
        "wolf should have Skills component"
    );
}

// ─── Decay system ──────────────────────────────────────────────────────────

#[test]
fn decay_fires_and_pulls_disused_skill_toward_floor() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::default());

    // Pre-practice at tick 0, then skip the grace window so the skill
    // becomes eligible for decay.
    set_harvesting_level(&mut world, agent, 0.8, 0);
    // Fire every 10 ticks, no grace, aggressive half-life so the delta
    // after one fire is measurable.
    set_fast_decay(&mut world, 10, 0, 1.0, 1.0, 0.05);

    world.tick(11);

    let level = get_skills(&world, agent).level(SkillKind::Harvesting);
    assert!(
        level < 0.8,
        "level should have decayed from 0.8, got {level}"
    );
    assert!(
        level >= 0.05,
        "level should not sink below the floor, got {level}"
    );
}

#[test]
fn decay_grace_window_skips_recently_practiced_skill() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::default());

    // Set a very long grace window so the recent practice blocks decay.
    set_fast_decay(&mut world, 10, 1_000, 1.0, 1.0, 0.05);
    // Stamp last_practiced = 5 so when the first decay fire runs at t=10,
    // age = 5 ticks < 1000 grace → skipped.
    world.tick(5);
    set_harvesting_level(&mut world, agent, 0.8, 5);

    world.tick(10);

    let level = get_skills(&world, agent).level(SkillKind::Harvesting);
    assert!(
        (level - 0.8).abs() < 1e-5,
        "recently practiced skill should be untouched, got {level}"
    );
}

#[test]
fn decay_never_sinks_below_floor() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::default());

    set_harvesting_level(&mut world, agent, 0.2, 0);
    set_fast_decay(&mut world, 5, 0, 10.0, 1.0, 0.1);

    // March through many decay fires so the value asymptotes to the floor.
    world.tick(200);

    let level = get_skills(&world, agent).level(SkillKind::Harvesting);
    assert!(
        (level - 0.1).abs() < 1e-3,
        "level should converge to the 0.1 floor, got {level}"
    );
}

// ─── HarvestAction yield scaling ──────────────────────────────────────────

/// Direct invocation of `HarvestAction::on_complete` with a manually
/// constructed `CompletionContext`. Mirrors the pattern used in
/// `test_deposit_and_take.rs` so the yield scaling can be tested without
/// driving the whole brain pipeline.
fn run_harvest_on_complete(
    inventory: &mut ItemSlots,
    target_inventory: &mut ItemSlots,
    skills: Option<&Skills>,
) {
    let mut physical = PhysicalNeeds::default();
    let mind = MindGraph::new(Ontology::default());
    let mut spawn_requests: Vec<SpawnRequest> = Vec::new();

    let mut ctx = CompletionContext {
        physical: &mut physical,
        inventory,
        drives: None,
        mind: &mind,
        skills,
        target_inventory: Some(target_inventory),
        target_entity: None,
        tick: 0,
        agent_position: Vec2::ZERO,
        spawn_requests: &mut spawn_requests,
    };

    GenericAction::new(&HARVEST_DEF).on_complete(&mut ctx);
}

#[test]
fn novice_harvester_extracts_one_item_per_action() {
    let mut inventory = ItemSlots::agent_carry();
    let mut target = ItemSlots::agent_carry();
    target.add(Concept::Berry, 10);

    let skills = Skills::default();
    run_harvest_on_complete(&mut inventory, &mut target, Some(&skills));

    assert_eq!(
        inventory.count(Concept::Berry),
        1,
        "novice harvest should yield one berry"
    );
    assert_eq!(
        target.count(Concept::Berry),
        9,
        "one berry should be removed from the target"
    );
}

#[test]
fn master_harvester_extracts_multiple_items_per_action() {
    let mut inventory = ItemSlots::agent_carry();
    let mut target = ItemSlots::agent_carry();
    target.add(Concept::Berry, 10);

    let mut skills = Skills::default();
    skills.set_level(SkillKind::Harvesting, 1.0, 0);
    run_harvest_on_complete(&mut inventory, &mut target, Some(&skills));

    let gained = inventory.count(Concept::Berry);
    assert!(
        gained >= 2,
        "master harvest should extract more than one, got {gained}"
    );
    assert_eq!(
        target.count(Concept::Berry) + gained,
        10,
        "conservation: everything removed from target should appear in inventory"
    );
}

#[test]
fn harvester_is_bounded_by_target_stock() {
    let mut inventory = ItemSlots::agent_carry();
    let mut target = ItemSlots::agent_carry();
    target.add(Concept::Berry, 1);

    let mut skills = Skills::default();
    skills.set_level(SkillKind::Harvesting, 1.0, 0);
    run_harvest_on_complete(&mut inventory, &mut target, Some(&skills));

    assert_eq!(
        inventory.count(Concept::Berry),
        1,
        "cannot extract more than the target holds"
    );
    assert_eq!(target.count(Concept::Berry), 0);
}

#[test]
fn harvest_without_skills_component_still_yields_one() {
    let mut inventory = ItemSlots::agent_carry();
    let mut target = ItemSlots::agent_carry();
    target.add(Concept::Berry, 10);

    // Simulates an agent without a Skills component (shouldn't happen in
    // practice, but the action must tolerate `None`).
    run_harvest_on_complete(&mut inventory, &mut target, None);

    assert_eq!(inventory.count(Concept::Berry), 1);
}
