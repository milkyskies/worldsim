//! After a Harvest empties the target, the agent must not immediately
//! re-propose Harvest on the same target. Before this fix, a small window
//! between the successful harvest and the perception/belief-update tick
//! let the rational brain spawn a doomed follow-up plan — visible as
//! "agent keeps tapping an empty bush".

use bevy::prelude::*;
use worldsim::agent::actions::ActionType;
use worldsim::agent::events::{SimEvent, SimEventKind};
use worldsim::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use worldsim::testing::TestWorld;
use worldsim::world::apple_tree::ResourceRegeneration;

/// Hungry alice next to a 1-berry bush whose regrowth is disabled. The
/// bush can only be harvested once; after that any subsequent Harvest
/// attempt is the #629 spin.
#[test]
fn agent_does_not_respawn_harvest_on_emptied_bush_without_regrowth() {
    let bush_pos = Vec2::new(52.0, 50.0);

    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(50.0, 50.0))
        .hunger_urgency(0.9)
        .done()
        .build();
    let alice = agents["alice"];
    let bush = world.spawn_berry_bush(bush_pos, 1);

    // Freeze regrowth: remove the ResourceRegeneration component so the
    // bush stays empty after the first harvest. This isolates the #629
    // bug (stale belief after empty-ing) from regrowth noise.
    world
        .app_mut()
        .world_mut()
        .entity_mut(bush)
        .remove::<ResourceRegeneration>();

    world.enable_fast_forward();
    world.tick(2000);

    let bush_items = world
        .app()
        .world()
        .get::<worldsim::agent::item_slots::ItemSlots>(bush)
        .expect("bush has ItemSlots");
    let bush_berries = bush_items.count(Concept::Berry);
    assert_eq!(
        bush_berries, 0,
        "bush should be empty after hungry alice harvests it"
    );

    let harvest_starts_on_bush = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| {
            matches!(
                e,
                SimEvent {
                    kind: SimEventKind::ActionStarted {
                        action: ActionType::Harvest,
                        target: Some(t),
                        ..
                    },
                    ..
                } if *t == bush && e.involves(alice)
            )
        })
        .count();

    // The test passes at any value ≤ 2: one legitimate harvest that
    // empties the bush, plus at most one follow-up attempt that trips
    // the ResourceDepleted detection. Any more than that is a spin.
    assert!(
        harvest_starts_on_bush <= 2,
        "alice kept harvesting the same empty bush: {harvest_starts_on_bush} \
         Harvest starts against {bush:?}. She should stop after the \
         emptying harvest (with at most one follow-up that trips the \
         ResourceDepleted detection)."
    );

    let mind = world
        .app()
        .world()
        .get::<MindGraph>(alice)
        .expect("alice has MindGraph");
    let stale_positive_contains: Vec<_> = mind
        .query(Some(&Node::Entity(bush)), Some(Predicate::Contains), None)
        .into_iter()
        .filter(|t| matches!(t.object, Value::Item(_, qty) if qty > 0))
        .collect();
    assert!(
        stale_positive_contains.is_empty(),
        "bush is empty but alice still believes it contains: {stale_positive_contains:?}"
    );
}
