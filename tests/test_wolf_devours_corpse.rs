//! Wolves should feed in place from a fresh corpse via the Devour action.
//!
//! Before this change wolves could only consume the "first cut" 1-meat
//! deposit Bite gives the killer; the 10 meat sitting in the corpse's
//! `ItemSlots` was unreachable because Harvest gates on Manipulation 0.9
//! (two-handed) which a single wolf jaw can't satisfy. Devour bypasses the
//! Harvest hop entirely — the wolf's metabolism is fed straight from the
//! corpse's slots, one bite per Devour completion.

use bevy::math::Vec2;
use worldsim::agent::actions::ActionType;
use worldsim::agent::biology::body::{Body, BodyNodeKind};
use worldsim::agent::body::metabolism::Metabolism;
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::events::{SimEvent, SimEventKind};
use worldsim::agent::inventory::EntityType;
use worldsim::agent::item_slots::ItemSlots;
use worldsim::agent::mind::knowledge::Concept;
use worldsim::testing::TestWorld;

fn destroy_heart(world: &mut TestWorld, entity: bevy::prelude::Entity) {
    world
        .app_mut()
        .world_mut()
        .get_mut::<Body>(entity)
        .expect("entity has Body")
        .node_mut(BodyNodeKind::Heart)
        .expect("body has Heart")
        .current_hp = 0.0;
}

#[ignore = "TODO #745: InitiateHunt's EntityWithTrait(Prey) enumeration picks up corpses that retain a stale Prey belief, so the planner builds a Hunt plan against the corpse instead of a Devour. Needs the prey enumeration to hard-exclude Carrion / the Devour planner migration."]
#[test]
fn hungry_wolf_devours_meat_from_nearby_corpse() {
    let mut world = TestWorld::with_seed(42);

    // Co-located so the wolf perceives the deer immediately and we don't
    // depend on pathfinding / approach behaviour to trigger the feed.
    let deer = world.spawn_deer(Vec2::new(50.0, 50.0));
    let wolf = world.spawn_wolf(Vec2::new(50.0, 50.0));

    // Starving wolf — survival brain should reach for the nearest food.
    {
        let mut needs = world
            .app_mut()
            .world_mut()
            .get_mut::<PhysicalNeeds>(wolf)
            .expect("wolf has PhysicalNeeds");
        needs.metabolism = Metabolism::at_urgency(0.95);
    }

    // Tick a few so the wolf perceives the live deer and the planner has
    // a clean MindGraph snapshot to work from.
    world.tick(5);

    // Kill the deer in place — preserves the entity ID, swaps EntityType
    // to Corpse, deposits DEFAULT_CORPSE_MEAT into its ItemSlots.
    destroy_heart(&mut world, deer);
    world.tick(60);
    assert_eq!(
        world.get::<EntityType>(deer).0,
        Concept::Corpse,
        "deer should have morphed into a Corpse"
    );

    let meat_before = world.get::<ItemSlots>(deer).count(Concept::Meat);
    assert!(
        meat_before > 0,
        "fresh corpse should hold meat for scavengers"
    );

    let devour_arming_tick = world.current_tick();
    world.tick(600);

    let devour_completes: u32 = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| {
            matches!(
                e,
                SimEvent { tick, kind: SimEventKind::ActionCompleted { agent, action, .. }, .. } if *agent == wolf && *action == ActionType::Devour && *tick >= devour_arming_tick
            )
        })
        .count() as u32;

    assert!(
        devour_completes > 0,
        "starving wolf should have completed at least one Devour against the corpse"
    );

    let meat_after = world.get::<ItemSlots>(deer).count(Concept::Meat);
    assert!(
        meat_after < meat_before,
        "corpse meat must drop as the wolf devours; before={meat_before}, after={meat_after}"
    );
    assert_eq!(
        meat_before - meat_after,
        devour_completes,
        "each Devour completion should remove exactly one meat from the corpse"
    );
}
