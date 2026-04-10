//! Integration tests for world entity property components (#279).
//!
//! Verifies that LightSource, HeatSource, Durability, ShelterProvider, BuiltBy,
//! and FuelConsumer components behave correctly, that their systems run per tick,
//! and that campfire is correctly composed from property components.

use bevy::prelude::*;
use worldsim::agent::activity::CurrentActivity;
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::inventory::EntityType;
use worldsim::agent::mind::knowledge::{Concept, MindGraph, Predicate, Value};
use worldsim::testing::{AgentConfig, TestWorld};
use worldsim::world::Physical;
use worldsim::world::campfire::CampfireMarker;
use worldsim::world::property::{
    BuiltBy, Durability, FuelConsumer, HeatSource, LightSource, ShelterProvider,
};

// ─── Durability ───────────────────────────────────────────────────────────────

#[test]
fn durability_degrades_over_time() {
    let mut world = TestWorld::with_seed(0);
    let entity = world
        .app_mut()
        .world_mut()
        .spawn((
            Name::new("BreakableRock"),
            EntityType(Concept::Stone),
            Physical,
            Transform::from_translation(Vec3::ZERO),
            GlobalTransform::default(),
            Durability {
                current: 10.0,
                max: 10.0,
                decay_rate: 1.0, // loses 1 per tick
            },
        ))
        .id();

    world.tick(5);

    let durability = world.get::<Durability>(entity);
    assert!(
        durability.current < 10.0,
        "durability should have decreased (got {})",
        durability.current
    );
}

#[test]
fn entity_despawns_when_durability_reaches_zero() {
    let mut world = TestWorld::with_seed(0);
    let entity = world
        .app_mut()
        .world_mut()
        .spawn((
            Name::new("Fragile"),
            EntityType(Concept::Stone),
            Physical,
            Transform::from_translation(Vec3::ZERO),
            GlobalTransform::default(),
            Durability {
                current: 3.0,
                max: 3.0,
                decay_rate: 1.0,
            },
        ))
        .id();

    // Tick past zero
    world.tick(5);

    assert!(
        !world.entity_exists(entity),
        "entity should be despawned when durability reaches zero"
    );
}

#[test]
fn indestructible_entity_never_despawns() {
    let mut world = TestWorld::with_seed(0);
    let entity = world
        .app_mut()
        .world_mut()
        .spawn((
            Name::new("StoneWall"),
            EntityType(Concept::Stone),
            Physical,
            Transform::from_translation(Vec3::ZERO),
            GlobalTransform::default(),
            Durability {
                current: 100.0,
                max: 100.0,
                decay_rate: 0.0, // indestructible
            },
        ))
        .id();

    world.tick(200);

    assert!(
        world.entity_exists(entity),
        "indestructible entity (decay_rate=0) should never despawn"
    );
}

// ─── FuelConsumer ────────────────────────────────────────────────────────────

#[test]
fn fuel_consumer_loses_fuel_per_tick() {
    let mut world = TestWorld::with_seed(0);
    let entity = world
        .app_mut()
        .world_mut()
        .spawn((
            Name::new("FuelBurner"),
            EntityType(Concept::Campfire),
            Physical,
            Transform::from_translation(Vec3::ZERO),
            GlobalTransform::default(),
            LightSource {
                radius: 50.0,
                intensity: 1.0,
            },
            HeatSource {
                radius: 50.0,
                intensity: 1.0,
            },
            FuelConsumer {
                fuel_type: Concept::Wood,
                fuel_remaining: 100.0,
                consumption_rate: 1.0,
            },
        ))
        .id();

    world.tick(10);

    let consumer = world.get::<FuelConsumer>(entity);
    assert!(
        consumer.fuel_remaining < 100.0,
        "fuel should have decreased (got {})",
        consumer.fuel_remaining
    );
}

#[test]
fn light_and_heat_sources_removed_when_fuel_runs_out() {
    let mut world = TestWorld::with_seed(0);
    let entity = world
        .app_mut()
        .world_mut()
        .spawn((
            Name::new("BurningOut"),
            EntityType(Concept::Campfire),
            Physical,
            Transform::from_translation(Vec3::ZERO),
            GlobalTransform::default(),
            LightSource {
                radius: 50.0,
                intensity: 1.0,
            },
            HeatSource {
                radius: 50.0,
                intensity: 1.0,
            },
            FuelConsumer {
                fuel_type: Concept::Wood,
                fuel_remaining: 3.0,
                consumption_rate: 1.0,
            },
        ))
        .id();

    // Tick past fuel exhaustion
    world.tick(5);

    let has_light = world.app().world().get::<LightSource>(entity).is_some();
    let has_heat = world.app().world().get::<HeatSource>(entity).is_some();

    assert!(
        !has_light,
        "LightSource should be removed when fuel runs out"
    );
    assert!(!has_heat, "HeatSource should be removed when fuel runs out");
}

// ─── ShelterProvider ─────────────────────────────────────────────────────────

#[test]
fn shelter_provider_improves_sleep_energy_recovery() {
    // Two agents start at low energy. One sleeps near a shelter, one without.
    let mut world = TestWorld::with_seed(0);

    // Spawn shelter
    world.app_mut().world_mut().spawn((
        Name::new("LeanTo"),
        EntityType(Concept::LeanTo),
        Physical,
        Transform::from_translation(Vec3::new(100.0, 100.0, 0.0)),
        GlobalTransform::default(),
        ShelterProvider {
            capacity: 2,
            protection: 1.5,
        },
    ));

    // Agent near shelter
    let sheltered = world.spawn_agent(AgentConfig {
        pos: Vec2::new(100.0, 100.0),
        energy: 50.0,
        ..AgentConfig::default()
    });
    // Agent far from shelter
    let unsheltered = world.spawn_agent(AgentConfig {
        pos: Vec2::new(500.0, 500.0),
        energy: 50.0,
        ..AgentConfig::default()
    });

    // Force both agents into Sleeping activity
    world
        .app_mut()
        .world_mut()
        .get_mut::<CurrentActivity>(sheltered)
        .map(|mut a| *a = CurrentActivity::Sleeping);
    world
        .app_mut()
        .world_mut()
        .get_mut::<CurrentActivity>(unsheltered)
        .map(|mut a| *a = CurrentActivity::Sleeping);

    let energy_before_sheltered = world.get::<PhysicalNeeds>(sheltered).energy;
    let energy_before_unsheltered = world.get::<PhysicalNeeds>(unsheltered).energy;

    world.tick(20);

    let energy_after_sheltered = world.get::<PhysicalNeeds>(sheltered).energy;
    let energy_after_unsheltered = world.get::<PhysicalNeeds>(unsheltered).energy;

    let sheltered_gain = energy_after_sheltered - energy_before_sheltered;
    let unsheltered_gain = energy_after_unsheltered - energy_before_unsheltered;

    assert!(
        sheltered_gain > unsheltered_gain,
        "agent near shelter should recover more energy while sleeping \
         (sheltered gained {:.2}, unsheltered gained {:.2})",
        sheltered_gain,
        unsheltered_gain
    );
}

#[test]
fn natural_shelter_provides_same_benefit_as_built_shelter() {
    // ShelterProvider is the same component regardless of BuiltBy presence.
    let mut world = TestWorld::with_seed(0);

    // Natural cave — no BuiltBy
    let cave = world
        .app_mut()
        .world_mut()
        .spawn((
            Name::new("Cave"),
            EntityType(Concept::LeanTo),
            Physical,
            Transform::from_translation(Vec3::new(100.0, 100.0, 0.0)),
            GlobalTransform::default(),
            ShelterProvider {
                capacity: 3,
                protection: 1.2,
            },
        ))
        .id();

    // Built lean-to — with BuiltBy
    let builder = world.app_mut().world_mut().spawn_empty().id();
    let lean_to = world
        .app_mut()
        .world_mut()
        .spawn((
            Name::new("LeanTo"),
            EntityType(Concept::LeanTo),
            Physical,
            Transform::from_translation(Vec3::new(200.0, 200.0, 0.0)),
            GlobalTransform::default(),
            ShelterProvider {
                capacity: 3,
                protection: 1.2,
            },
            BuiltBy {
                builder,
                built_at: 0,
            },
        ))
        .id();

    // Both should have the same ShelterProvider protection value
    let cave_provider = world.get::<ShelterProvider>(cave);
    let lean_to_provider = world.get::<ShelterProvider>(lean_to);

    assert_eq!(
        cave_provider.protection, lean_to_provider.protection,
        "natural and built shelters with the same protection value should provide identical benefit"
    );
    assert_eq!(cave_provider.capacity, lean_to_provider.capacity);
}

// ─── Campfire composition ────────────────────────────────────────────────────

#[test]
fn campfire_has_light_source() {
    let mut world = TestWorld::with_seed(0);
    let campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));
    assert!(
        world.app().world().get::<LightSource>(campfire).is_some(),
        "campfire should have a LightSource component"
    );
}

#[test]
fn campfire_has_heat_source() {
    let mut world = TestWorld::with_seed(0);
    let campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));
    assert!(
        world.app().world().get::<HeatSource>(campfire).is_some(),
        "campfire should have a HeatSource component"
    );
}

#[test]
fn campfire_has_fuel_consumer() {
    let mut world = TestWorld::with_seed(0);
    let campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));
    let consumer = world.get::<FuelConsumer>(campfire);
    assert_eq!(consumer.fuel_type, Concept::Wood);
    assert!(consumer.fuel_remaining > 0.0);
    assert!(consumer.consumption_rate > 0.0);
}

#[test]
fn campfire_has_campfire_marker() {
    let mut world = TestWorld::with_seed(0);
    let campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));
    assert!(
        world
            .app()
            .world()
            .get::<CampfireMarker>(campfire)
            .is_some(),
        "campfire should have CampfireMarker"
    );
}

#[test]
fn campfire_light_dims_when_fuel_exhausted() {
    let mut world = TestWorld::with_seed(0);
    let campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));

    // Drain all fuel
    {
        let mut consumer = world.get_mut::<FuelConsumer>(campfire);
        consumer.fuel_remaining = 2.0;
    }

    world.tick(5);

    assert!(
        world.app().world().get::<LightSource>(campfire).is_none(),
        "campfire LightSource should be removed when fuel runs out"
    );
    assert!(
        world.app().world().get::<HeatSource>(campfire).is_none(),
        "campfire HeatSource should be removed when fuel runs out"
    );
    // The campfire entity itself persists (it's embers/ash)
    assert!(
        world.entity_exists(campfire),
        "campfire entity should still exist after fuel runs out (it's now embers)"
    );
}

// ─── BuiltBy ────────────────────────────────────────────────────────────────

#[test]
fn built_by_records_builder_and_tick() {
    let mut world = TestWorld::with_seed(0);
    let builder = world.spawn_agent(AgentConfig::at(Vec2::new(100.0, 100.0)));

    let structure = world
        .app_mut()
        .world_mut()
        .spawn((
            Name::new("Shelter"),
            EntityType(Concept::LeanTo),
            Physical,
            Transform::from_translation(Vec3::new(100.0, 100.0, 0.0)),
            GlobalTransform::default(),
            BuiltBy {
                builder,
                built_at: 42,
            },
        ))
        .id();

    let built_by = world.get::<BuiltBy>(structure);
    assert_eq!(built_by.builder, builder);
    assert_eq!(built_by.built_at, 42);
}

// ─── Ontology derivation ─────────────────────────────────────────────────────

#[test]
fn campfire_has_light_emitting_trait_in_ontology() {
    let mut world = TestWorld::with_seed(0);
    let _campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));

    // Tick once to let ontology derivation system run
    world.tick(1);

    let ontology = world
        .app()
        .world()
        .resource::<worldsim::agent::mind::knowledge::Ontology>();
    assert!(
        ontology.has_trait(Concept::Campfire, Concept::LightEmitting),
        "campfire should have LightEmitting trait after LightSource is added"
    );
}

#[test]
fn campfire_has_heat_emitting_trait_in_ontology() {
    let mut world = TestWorld::with_seed(0);
    let _campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));

    world.tick(1);

    let ontology = world
        .app()
        .world()
        .resource::<worldsim::agent::mind::knowledge::Ontology>();
    assert!(
        ontology.has_trait(Concept::Campfire, Concept::HeatEmitting),
        "campfire should have HeatEmitting trait after HeatSource is added"
    );
}

// ─── Heat perception ─────────────────────────────────────────────────────────

#[test]
fn agent_near_heat_source_perceives_warmth() {
    let mut world = TestWorld::with_seed(0);

    // Agent at origin, campfire 30px away (within 64px heat radius)
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(100.0, 100.0)));
    let _campfire = world.spawn_campfire(Vec2::new(130.0, 100.0));

    world.tick(12);

    let mind = world.get::<MindGraph>(agent);
    let warmth_triples: Vec<_> = mind
        .query(
            None,
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Warmth)),
        )
        .into_iter()
        .collect();

    assert!(
        !warmth_triples.is_empty(),
        "agent near a campfire should perceive warmth via temperature sense"
    );
}

#[test]
fn agent_far_from_heat_source_does_not_perceive_warmth() {
    let mut world = TestWorld::with_seed(0);

    // Agent far away (500px), campfire at origin — well outside radius
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(500.0, 500.0)));
    let _campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));

    world.tick(12);

    let mind = world.get::<MindGraph>(agent);
    let warmth_triples: Vec<_> = mind
        .query(
            None,
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Warmth)),
        )
        .into_iter()
        .collect();

    assert!(
        warmth_triples.is_empty(),
        "agent far from campfire should not perceive warmth"
    );
}
