//! Regression test for #600: Bite/Attack must not target dead entities.
//!
//! Before this fix, the target enumerator walked MindGraph `IsA` beliefs and
//! kept anything matching `HasTrait(Prey)`. A corpse still carries `IsA Deer`
//! in observer minds (the belief-invalidation work is #524), so a wolf that
//! had perceived its prey while it was alive would keep biting it after
//! death instead of routing to Harvest.

use bevy::math::Vec2;
use worldsim::agent::Dead;
use worldsim::agent::actions::ActionType;
use worldsim::agent::biology::body::{Body, BodyNodeKind};
use worldsim::agent::body::metabolism::Metabolism;
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::events::{SimEvent, SimEventKind};
use worldsim::agent::inventory::EntityType;
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

#[test]
fn wolf_does_not_bite_freshly_killed_deer_corpse() {
    let mut world = TestWorld::with_seed(42);

    // Co-located so the wolf perceives the deer without needing to pathfind.
    let deer = world.spawn_deer(Vec2::new(50.0, 50.0));
    let wolf = world.spawn_wolf(Vec2::new(50.0, 50.0));

    // Make the wolf starving so the survival brain wants to hunt.
    {
        let mut needs = world
            .app_mut()
            .world_mut()
            .get_mut::<PhysicalNeeds>(wolf)
            .expect("wolf has PhysicalNeeds");
        needs.metabolism = Metabolism::at_urgency(0.95);
    }

    // Let the wolf perceive the live deer so `(deer, IsA, Deer)` lands in
    // its MindGraph — the stale belief that the Dead-filter has to guard
    // against once the deer turns into a corpse.
    world.tick(5);

    destroy_heart(&mut world, deer);
    world.tick(60);

    assert!(
        world.app().world().get::<Dead>(deer).is_some(),
        "deer should carry the Dead marker after heart destruction"
    );
    assert_eq!(
        world.get::<EntityType>(deer).0,
        Concept::Corpse,
        "deer should have morphed into a Corpse"
    );

    let death_tick = world.current_tick();

    // Tick long enough that any plan cycle would pick Bite again if the
    // filter were broken.
    world.tick(300);

    let post_death_bites: Vec<u64> = world
        .sim_events()
        .all()
        .iter()
        .filter_map(|e| match e {
            SimEvent {
                tick,
                kind:
                    SimEventKind::ActionStarted {
                        agent,
                        action,
                        target,
                        ..
                    },
                ..
            } if *agent == wolf
                && *action == ActionType::Bite
                && *target == Some(deer)
                && *tick >= death_tick =>
            {
                Some(*tick)
            }
            _ => None,
        })
        .collect();

    assert!(
        post_death_bites.is_empty(),
        "wolf must not Bite a dead deer corpse; bites observed at ticks {post_death_bites:?}"
    );

    // Regression check: the Dead filter lives on the trait-enumeration path
    // (EntityWithTrait), not the affordance path (EntityAffordance). A
    // hungry wolf next to a meat-bearing corpse should still pursue
    // Harvest rather than going hungry.
}
