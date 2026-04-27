//! Integration tests for the perception spatial-query cache (#368).
//!
//! Verifies that despawned entities never leak through the cache and that
//! entities spawned inside an unchanged chunk-bucket key still become visible
//! within the safety-refresh window.

use bevy::prelude::*;
use worldsim::agent::mind::perception::VisibleObjects;
use worldsim::testing::TestWorld;

// Spatial index updates in PostUpdate, so the first FixedUpdate perception sees an
// empty index — the cache picks up real entities on the second cycle once the empty
// cache forces a re-query.
const WARMUP_TICKS: u64 = 2;

#[test]
fn despawned_entity_never_appears_after_one_more_tick() {
    let (mut world, agent) = TestWorld::solo_agent(42);

    world.tick(WARMUP_TICKS);
    let visible_initial: Vec<Entity> = world.get::<VisibleObjects>(agent).entities.clone();
    assert!(
        !visible_initial.is_empty(),
        "agent should see the pre-spawned berry bushes (got {} visible)",
        visible_initial.len()
    );

    // Despawn a visible entity. The cache key (chunk + chunk_radius) is unchanged,
    // so the cached candidate list still contains the despawned entity. The precise
    // distance pass must catch it via the failed Transform fetch.
    let despawned = visible_initial[0];
    world.app_mut().world_mut().despawn(despawned);
    world.tick(1);

    assert!(
        !world
            .get::<VisibleObjects>(agent)
            .entities
            .contains(&despawned),
        "despawned entity must not appear in VisibleObjects"
    );
}

#[test]
fn by_concept_bucket_partitions_entities_with_entity_type() {
    use worldsim::agent::inventory::EntityType;

    let (mut world, agent) = TestWorld::solo_agent(42);
    world.tick(WARMUP_TICKS);

    let visible = world.get::<VisibleObjects>(agent);
    let entities_with_type: Vec<Entity> = visible
        .entities
        .iter()
        .copied()
        .filter(|e| world.app().world().get::<EntityType>(*e).is_some())
        .collect();

    assert!(
        !entities_with_type.is_empty(),
        "test setup should have visible entities with EntityType"
    );

    // Every entity-with-EntityType in `entities` is in exactly one bucket.
    for entity in &entities_with_type {
        let entity_type = world
            .app()
            .world()
            .get::<EntityType>(*entity)
            .expect("filtered above");
        let bucket = visible
            .by_concept
            .get(&entity_type.0)
            .expect("entity's concept must have a bucket");
        assert!(
            bucket.contains(entity),
            "entity {entity:?} of concept {:?} must be in its bucket",
            entity_type.0
        );
    }

    // Buckets contain only entities listed in `entities` (no stale state).
    for ents in visible.by_concept.values() {
        for e in ents {
            assert!(
                visible.entities.contains(e),
                "bucket entry {e:?} must also be in entities list"
            );
        }
    }
}

#[test]
fn entity_spawned_into_cached_bucket_appears_within_safety_refresh() {
    let (mut world, agent) = TestWorld::solo_agent(42);
    world.tick(WARMUP_TICKS);

    // Spawn a fresh entity well within the agent's view range. The agent has
    // not moved chunks since its first perception tick, so the cache key is
    // unchanged — only the safety refresh should surface this entity.
    let new_bush = world.spawn_berry_bush(Vec2::new(58.0, 50.0), 5);

    // 60 ticks is comfortably past the 30-tick safety refresh ceiling.
    world.tick(60);
    assert!(
        world
            .get::<VisibleObjects>(agent)
            .entities
            .contains(&new_bush),
        "newly spawned nearby entity must appear within the safety-refresh window"
    );
}
