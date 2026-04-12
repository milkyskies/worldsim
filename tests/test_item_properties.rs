//! Integration tests for per-instance item properties (#286).
//!
//! Verifies that:
//! - Harvested apples get freshness = 1.0 and created_at stamped
//! - Freshness decays over time via the freshness_decay_system
//! - Freshness reaching 0 converts the concept to its rotten variant
//! - Properties are preserved through deposit/take transfers

use bevy::prelude::Vec2;
use worldsim::agent::item_slots::{
    Access, ItemSlots, Slot, SlotFilter, SlotRole, Thing, ThingProperties, perishable_decay_rate,
};
use worldsim::agent::mind::knowledge::Concept;
use worldsim::testing::{AgentConfig, TestWorld};

// ═══════════════════════════════════════════════════════════════════════════
// Freshness decay via system
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn freshness_decreases_after_100_ticks() {
    // Spawn a world with one agent. Directly give it a fresh apple (bypassing
    // the harvest action) and confirm freshness decays after 100 ticks.
    let mut world = TestWorld::new();
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(50.0, 50.0)));

    world
        .app_mut()
        .world_mut()
        .get_mut::<ItemSlots>(agent)
        .unwrap()
        .add_thing(Thing::fresh(Concept::Apple, 0));

    let freshness_before = world
        .app()
        .world()
        .get::<ItemSlots>(agent)
        .unwrap()
        .all_items()
        .find(|t| t.concept == Concept::Apple)
        .and_then(|t| t.properties.freshness)
        .unwrap();

    assert!((freshness_before - 1.0).abs() < 0.001, "Should start fresh");

    // The decay system fires every 100 ticks.
    world.tick(100);

    let freshness_after = world
        .app()
        .world()
        .get::<ItemSlots>(agent)
        .unwrap()
        .all_items()
        .find(|t| t.concept == Concept::Apple)
        .and_then(|t| t.properties.freshness)
        .unwrap();

    assert!(
        freshness_after < freshness_before,
        "Freshness should have decreased: before={freshness_before} after={freshness_after}"
    );

    let rate = perishable_decay_rate(Concept::Apple).unwrap();
    let expected = freshness_before - rate;
    assert!(
        (freshness_after - expected).abs() < 0.001,
        "Expected freshness={expected:.4}, got {freshness_after:.4}"
    );
}

#[test]
fn freshness_reaches_zero_and_converts_to_rotten_apple() {
    let mut world = TestWorld::new();
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(50.0, 50.0)));

    let rate = perishable_decay_rate(Concept::Apple).unwrap();

    // Place the apple just below the decay step so the next event tips it to 0
    world
        .app_mut()
        .world_mut()
        .get_mut::<ItemSlots>(agent)
        .unwrap()
        .add_thing(Thing {
            concept: Concept::Apple,
            properties: ThingProperties {
                freshness: Some(rate * 0.5),
                ..Default::default()
            },
        });

    world.tick(100);

    let slots = world.app().world().get::<ItemSlots>(agent).unwrap();
    assert_eq!(
        slots.count(Concept::RottenApple),
        1,
        "Apple should have become RottenApple"
    );
    assert_eq!(
        slots.count(Concept::Apple),
        0,
        "Original Apple concept should be gone"
    );

    let spoiled_events: Vec<_> = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| {
            matches!(
                e,
                worldsim::agent::events::SimEvent::ItemSpoiled {
                    from: Concept::Apple,
                    to: Concept::RottenApple,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(
        spoiled_events.len(),
        1,
        "One ItemSpoiled event should fire when apple rots"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Harvest stamps freshness
// ═══════════════════════════════════════════════════════════════════════════

/// Berries/apples sitting in a bush/tree's inventory are "still on the
/// plant" — they must not rot in place. The distinction is carried by
/// the Thing's `freshness` field: `Thing::new` (plant stock) has
/// `freshness = None`, while `Thing::fresh` (picked by Harvest) has
/// `freshness = Some(1.0)`. The decay system skips the `None` sentinel.
/// Before #416, `get_or_insert(1.0)` initialized plant stock to fresh
/// then immediately started decaying — so an agent walking up to a
/// "stocked" bush would often find only RottenBerry.
#[test]
fn berries_on_bush_do_not_rot() {
    let mut world = TestWorld::new();
    let bush = world.spawn_berry_bush(Vec2::new(50.0, 50.0), 5);

    // Tick past what would be ~full rot for a picked berry
    // (0.020 per 100 ticks → ~5000 ticks to zero). 10k ticks is double.
    world.tick(10_000);

    let slots = world.app().world().get::<ItemSlots>(bush).unwrap();
    assert_eq!(
        slots.count(Concept::Berry),
        5,
        "all 5 berries should still be on the bush after 10k ticks"
    );
    assert_eq!(
        slots.count(Concept::RottenBerry),
        0,
        "no berries on the bush should have turned into RottenBerry"
    );
}

#[test]
fn harvested_apple_has_freshness_one_and_created_at() {
    let (mut world, agents) = TestWorld::scenario(1)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(50.0, 50.0))
        .hunger_urgency(0.85)
        .done()
        .apple_trees(1, Vec2::new(60.0, 50.0))
        .build();

    let alice = agents["alice"];

    let mut harvested = false;
    for _ in 0..600u64 {
        world.tick(1);
        if world.item_count(alice, Concept::Apple) > 0 {
            harvested = true;
            break;
        }
    }

    assert!(harvested, "Alice should harvest an apple within 600 ticks");

    let slots = world.app().world().get::<ItemSlots>(alice).unwrap();
    let apple = slots
        .all_items()
        .find(|t| t.concept == Concept::Apple)
        .expect("Should have an apple");

    assert_eq!(
        apple.properties.freshness,
        Some(1.0),
        "Freshly harvested apple should have freshness 1.0"
    );
    assert!(
        apple.properties.created_at.is_some(),
        "Harvested apple should have created_at set"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Properties preserved through deposit and take
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn properties_preserved_through_remove_and_deposit_thing() {
    let mut agent = ItemSlots::agent_carry();
    agent.add_thing(Thing {
        concept: Concept::Apple,
        properties: ThingProperties {
            freshness: Some(0.73),
            created_at: Some(55),
            ..Default::default()
        },
    });

    let mut chest = ItemSlots {
        slots: vec![Slot {
            role: SlotRole::Free,
            filter: SlotFilter::Any,
            capacity: None,
            contents: Vec::new(),
            deposit_access: Access::Public,
            extract_access: Access::Public,
        }],
    };

    let thing = agent.remove_thing(Concept::Apple).unwrap();
    assert!(chest.deposit_thing(thing, None));

    let in_chest = chest
        .all_items()
        .find(|t| t.concept == Concept::Apple)
        .unwrap();
    assert_eq!(in_chest.properties.freshness, Some(0.73));
    assert_eq!(in_chest.properties.created_at, Some(55));

    let taken = chest.extract_thing(Concept::Apple).unwrap();
    assert_eq!(taken.properties.freshness, Some(0.73));
    assert_eq!(taken.properties.created_at, Some(55));
}
