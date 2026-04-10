//! Integration tests for campfire fuel mechanics (#239).
//!
//! Verifies that campfires consume wood from their fuel slot over time,
//! transform to ash when fuel is fully exhausted, and that agents can
//! extend a campfire's life by depositing wood.

use bevy::prelude::*;
use worldsim::agent::item_slots::{ItemSlots, Thing};
use worldsim::agent::mind::knowledge::Concept;
use worldsim::testing::TestWorld;
use worldsim::world::campfire::{CampfireMarker, FUEL_PER_WOOD};
use worldsim::world::property::{FuelConsumer, LightSource};

/// A campfire with fuel in its slot should auto-reload when fuel_remaining
/// hits zero, consuming one wood item and resetting fuel_remaining.
#[test]
fn campfire_burns_one_unit_per_burn_interval() {
    let mut world = TestWorld::with_seed(0);
    let campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));

    // Drain the float counter to just above zero so it triggers
    // a reload from the slot within a few ticks.
    {
        let mut consumer = world.get_mut::<FuelConsumer>(campfire);
        consumer.fuel_remaining = 2.0;
    }

    let initial_wood = count_wood(&world, campfire);

    world.tick(5);

    let remaining_wood = count_wood(&world, campfire);
    let consumer = world.get::<FuelConsumer>(campfire);

    assert!(
        remaining_wood < initial_wood,
        "fuel slot should have lost a wood item after fuel_remaining ran out \
         (initial={initial_wood}, remaining={remaining_wood})"
    );
    assert!(
        consumer.fuel_remaining > 0.0,
        "fuel_remaining should have been refilled from the slot (got {})",
        consumer.fuel_remaining
    );
}

/// When both the fuel slot is empty AND fuel_remaining hits zero, the campfire
/// should lose its light, heat, and comfort aura, then transform to ash.
#[test]
fn campfire_with_no_fuel_transforms_to_ash() {
    let mut world = TestWorld::with_seed(0);
    let campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));

    // Drain everything: empty the fuel slot and set fuel_remaining low.
    {
        let mut slots = world.get_mut::<ItemSlots>(campfire);
        while slots.remove_thing_unchecked(Concept::Wood).is_some() {}
    }
    {
        let mut consumer = world.get_mut::<FuelConsumer>(campfire);
        consumer.fuel_remaining = 2.0;
    }

    // Tick enough for fuel to run out and Becomes to fire.
    world.tick(10);

    // The original campfire entity should be gone (despawned by becomes_system).
    assert!(
        !world.entity_exists(campfire),
        "campfire entity should be despawned after becoming ash"
    );
}

/// Depositing wood into a campfire's fuel slot extends its lifetime.
/// The campfire should auto-reload from the slot when fuel_remaining depletes.
#[test]
fn depositing_wood_extends_campfire_lifetime() {
    let mut world = TestWorld::with_seed(0);
    let campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));

    // Empty the fuel slot and set fuel_remaining to just 2 ticks.
    {
        let mut slots = world.get_mut::<ItemSlots>(campfire);
        while slots.remove_thing_unchecked(Concept::Wood).is_some() {}
    }
    {
        let mut consumer = world.get_mut::<FuelConsumer>(campfire);
        consumer.fuel_remaining = 2.0;
    }

    // Manually deposit one wood item into the fuel slot (simulating a Deposit action).
    {
        let mut slots = world.get_mut::<ItemSlots>(campfire);
        slots.deposit_thing(Thing::new(Concept::Wood), None);
    }

    // Tick past what would have been the death point without the deposit.
    world.tick(10);

    // Campfire should still be alive — the deposited wood extended its life.
    assert!(
        world.entity_exists(campfire),
        "campfire should still exist after refuelling"
    );
    assert!(
        world.app().world().get::<LightSource>(campfire).is_some(),
        "campfire should still have LightSource after refuelling"
    );
}

/// Two agents alternating wood deposits should keep a campfire alive indefinitely.
/// Each deposit arrives before the previous wood is fully consumed.
#[test]
fn multi_agent_refuelling_keeps_campfire_alive_indefinitely() {
    let mut world = TestWorld::with_seed(0);
    let campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));

    // Empty the fuel slot and set fuel low so it will need refuelling soon.
    {
        let mut slots = world.get_mut::<ItemSlots>(campfire);
        while slots.remove_thing_unchecked(Concept::Wood).is_some() {}
    }
    {
        let mut consumer = world.get_mut::<FuelConsumer>(campfire);
        consumer.fuel_remaining = 2.0;
    }

    // Simulate alternating deposits: deposit 1 wood, burn most of it,
    // deposit another before it runs out, repeat. Each wood item provides
    // FUEL_PER_WOOD ticks of fuel. Deposit well before that expires.
    let burn_interval = (FUEL_PER_WOOD * 0.8) as u64;
    for _ in 0..4 {
        {
            let mut slots = world.get_mut::<ItemSlots>(campfire);
            slots.deposit_thing(Thing::new(Concept::Wood), None);
        }
        world.tick(burn_interval);
    }

    assert!(
        world.entity_exists(campfire),
        "campfire should survive indefinitely with regular refuelling"
    );
    assert!(
        world
            .app()
            .world()
            .get::<CampfireMarker>(campfire)
            .is_some(),
        "campfire should still be a campfire (not transformed)"
    );
}

fn count_wood(world: &TestWorld, entity: Entity) -> u32 {
    let slots = world.get::<ItemSlots>(entity);
    slots
        .slots
        .iter()
        .flat_map(|s| &s.contents)
        .filter(|t| t.concept == Concept::Wood)
        .count() as u32
}

/// The fuel slot has extract_access: None, so items cannot be taken back out.
#[test]
fn cannot_extract_wood_from_burning_fire() {
    let mut world = TestWorld::with_seed(0);
    let campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));

    // Attempt to remove wood through the normal extract path (respects access control).
    let mut slots = world.get_mut::<ItemSlots>(campfire);
    let extracted = slots.remove_thing(Concept::Wood);

    assert!(
        extracted.is_none(),
        "should not be able to extract wood from a burning campfire's fuel slot"
    );
}
