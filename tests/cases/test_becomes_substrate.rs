//! Integration tests for the `Becomes` substrate (#61).
//!
//! Validates the general transformation primitive: world entities with a
//! `Becomes` component transform into target concept entities when their
//! trigger fires. Construction sites are exercised as the first concrete use.

use bevy::prelude::*;
use worldsim::agent::inventory::EntityType;
use worldsim::agent::item_slots::{ItemSlots, Slot};
use worldsim::agent::mind::knowledge::Concept;
use worldsim::core::tick::TickCount;
use worldsim::world::becomes::{Becomes, BecomesTrigger, becomes_system};
use worldsim::world::campfire::CampfireMarker;
use worldsim::world::construction_site::{
    ConstructionSiteMarker, spawn_construction_site_headless,
};

/// Build a minimal Bevy app that runs only the `becomes_system`.
/// Tests step it manually with `app.update()`.
fn becomes_test_app(starting_tick: u64) -> App {
    let mut app = App::new();
    app.insert_resource(TickCount {
        current: starting_tick,
        ..Default::default()
    });
    app.add_plugins(worldsim::palette::PalettePlugin);
    app.add_systems(Update, becomes_system);
    app
}

fn advance_tick(app: &mut App) {
    app.world_mut().resource_mut::<TickCount>().current += 1;
    app.update();
}

// ═══════════════════════════════════════════════════════════════════════════
// SubsTRATE: SlotsFilled trigger
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn slots_filled_trigger_transforms_entity() {
    let mut app = becomes_test_app(0);

    // Spawn a site with a single Construction slot pre-filled.
    let mut item_slots = ItemSlots {
        slots: vec![Slot::construction(Concept::Wood, 3)],
    };
    item_slots.deposit(Concept::Wood, 3, None);

    let site_entity = app
        .world_mut()
        .spawn((
            EntityType(Concept::ConstructionSite),
            ConstructionSiteMarker,
            worldsim::world::Physical,
            Transform::from_xyz(40.0, 60.0, 1.0),
            item_slots,
            Becomes::new(Concept::Campfire, BecomesTrigger::SlotsFilled, 0),
        ))
        .id();

    app.update();

    // Source site is gone.
    assert!(
        app.world().get_entity(site_entity).is_err(),
        "Source site must be despawned after transformation"
    );

    // A campfire was spawned.
    let mut camps = app.world_mut().query::<(&CampfireMarker, &Transform)>();
    let mut found = None;
    for (_, transform) in camps.iter(app.world()) {
        found = Some(transform.translation);
    }
    let camp_translation = found.expect("A campfire must exist after transformation");
    assert_eq!(camp_translation.x, 40.0, "Campfire must spawn at site x");
    assert_eq!(camp_translation.y, 60.0, "Campfire must spawn at site y");
}

#[test]
fn partial_slots_do_not_trigger_transformation() {
    let mut app = becomes_test_app(0);

    let mut item_slots = ItemSlots {
        slots: vec![Slot::construction(Concept::Wood, 3)],
    };
    item_slots.deposit(Concept::Wood, 1, None);

    let site_entity = app
        .world_mut()
        .spawn((
            EntityType(Concept::ConstructionSite),
            ConstructionSiteMarker,
            worldsim::world::Physical,
            Transform::from_xyz(0.0, 0.0, 0.0),
            item_slots,
            Becomes::new(Concept::Campfire, BecomesTrigger::SlotsFilled, 0),
        ))
        .id();

    app.update();

    assert!(
        app.world().get_entity(site_entity).is_ok(),
        "Partially filled site must NOT be despawned"
    );

    let mut camps = app.world_mut().query::<&CampfireMarker>();
    assert_eq!(
        camps.iter(app.world()).count(),
        0,
        "No campfire should be spawned from a partial site"
    );
}

#[test]
fn site_transforms_after_being_topped_up() {
    let mut app = becomes_test_app(0);

    let mut item_slots = ItemSlots {
        slots: vec![Slot::construction(Concept::Wood, 3)],
    };
    item_slots.deposit(Concept::Wood, 1, None);

    let site_entity = app
        .world_mut()
        .spawn((
            EntityType(Concept::ConstructionSite),
            ConstructionSiteMarker,
            worldsim::world::Physical,
            Transform::from_xyz(0.0, 0.0, 0.0),
            item_slots,
            Becomes::new(Concept::Campfire, BecomesTrigger::SlotsFilled, 0),
        ))
        .id();

    app.update();
    assert!(
        app.world().get_entity(site_entity).is_ok(),
        "Site must persist while partial"
    );

    // Simulate another agent depositing the remaining materials.
    {
        let mut entity_mut = app.world_mut().entity_mut(site_entity);
        let mut slots = entity_mut.get_mut::<ItemSlots>().unwrap();
        assert!(slots.deposit(Concept::Wood, 2, None));
    }

    app.update();

    assert!(
        app.world().get_entity(site_entity).is_err(),
        "Site must transform once topped up"
    );
    let mut camps = app.world_mut().query::<&CampfireMarker>();
    assert_eq!(
        camps.iter(app.world()).count(),
        1,
        "Exactly one campfire must be spawned"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// SubsTRATE: AfterTicks trigger
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn after_ticks_trigger_fires_at_deadline() {
    let mut app = becomes_test_app(100);

    let entity = app
        .world_mut()
        .spawn((
            EntityType(Concept::ConstructionSite),
            ConstructionSiteMarker,
            worldsim::world::Physical,
            Transform::from_xyz(10.0, 20.0, 0.0),
            Becomes::new(Concept::Campfire, BecomesTrigger::AfterTicks(3), 100),
        ))
        .id();

    // Tick 100 (age 0): not fired
    app.update();
    assert!(app.world().get_entity(entity).is_ok());

    // Tick 101 (age 1): not fired
    advance_tick(&mut app);
    assert!(app.world().get_entity(entity).is_ok());

    // Tick 102 (age 2): not fired
    advance_tick(&mut app);
    assert!(app.world().get_entity(entity).is_ok());

    // Tick 103 (age 3): deadline reached → fires
    advance_tick(&mut app);
    assert!(
        app.world().get_entity(entity).is_err(),
        "Entity must transform once the AfterTicks deadline is hit"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// CONSTRUCTION SITE: spawn helper produces correct slot configuration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn spawn_helper_creates_one_slot_per_requirement() {
    let mut app = becomes_test_app(0);

    let site = {
        let mut commands_queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut commands_queue, app.world());
        let id = spawn_construction_site_headless(
            &mut commands,
            Concept::Campfire,
            Vec2::new(10.0, 20.0),
            &[(Concept::Wood, 3)],
            &[],
            None,
            42,
            None,
        );
        commands_queue.apply(app.world_mut());
        id
    };

    let entity = app.world().entity(site);
    let slots = entity.get::<ItemSlots>().expect("Site must have ItemSlots");
    assert_eq!(
        slots.slots.len(),
        1,
        "One Construction slot per requirement"
    );
    assert_eq!(slots.count(Concept::Wood), 0, "No initial items");

    let becomes = entity
        .get::<Becomes>()
        .expect("Site must have a Becomes component");
    assert_eq!(becomes.target, Concept::Campfire);
    assert_eq!(becomes.started_tick, 42);
}

#[test]
fn spawn_helper_deposits_initial_items() {
    let mut app = becomes_test_app(0);

    let site = {
        let mut commands_queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut commands_queue, app.world());
        let id = spawn_construction_site_headless(
            &mut commands,
            Concept::Campfire,
            Vec2::ZERO,
            &[(Concept::Wood, 3)],
            &[(Concept::Wood, 2)],
            None,
            0,
            None,
        );
        commands_queue.apply(app.world_mut());
        id
    };

    let slots = app
        .world()
        .entity(site)
        .get::<ItemSlots>()
        .expect("ItemSlots present");
    assert_eq!(
        slots.count(Concept::Wood),
        2,
        "Initial items must be deposited"
    );
}

#[test]
fn full_initial_items_cause_immediate_transform_on_next_tick() {
    let mut app = becomes_test_app(7);

    let site = {
        let mut commands_queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut commands_queue, app.world());
        let id = spawn_construction_site_headless(
            &mut commands,
            Concept::Campfire,
            Vec2::new(5.0, 5.0),
            &[(Concept::Wood, 3)],
            &[(Concept::Wood, 3)], // exactly enough
            None,
            7,
            None,
        );
        commands_queue.apply(app.world_mut());
        id
    };

    app.update();

    assert!(
        app.world().get_entity(site).is_err(),
        "Fully-stocked site should transform on first system tick"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// MULTI-MATERIAL SITE: requires every slot to fill
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn multi_material_site_requires_all_slots_to_fill() {
    let mut app = becomes_test_app(0);

    let site_entity = app
        .world_mut()
        .spawn((
            EntityType(Concept::ConstructionSite),
            ConstructionSiteMarker,
            worldsim::world::Physical,
            Transform::from_xyz(0.0, 0.0, 0.0),
            ItemSlots {
                slots: vec![
                    Slot::construction(Concept::Wood, 3),
                    Slot::construction(Concept::Stone, 2),
                ],
            },
            Becomes::new(Concept::Campfire, BecomesTrigger::SlotsFilled, 0),
        ))
        .id();

    // Fill only the wood slot.
    {
        let mut entity_mut = app.world_mut().entity_mut(site_entity);
        let mut slots = entity_mut.get_mut::<ItemSlots>().unwrap();
        assert!(slots.deposit(Concept::Wood, 3, None));
    }
    app.update();
    assert!(
        app.world().get_entity(site_entity).is_ok(),
        "Site must persist while any slot is unfilled"
    );

    // Fill the stone slot.
    {
        let mut entity_mut = app.world_mut().entity_mut(site_entity);
        let mut slots = entity_mut.get_mut::<ItemSlots>().unwrap();
        assert!(slots.deposit(Concept::Stone, 2, None));
    }
    app.update();
    assert!(
        app.world().get_entity(site_entity).is_err(),
        "Site must transform once every Construction slot is filled"
    );
}
