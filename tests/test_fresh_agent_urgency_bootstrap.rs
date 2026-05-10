//! Regression test for #762: a freshly-spawned agent must have populated
//! `cns.urgencies` after one tick, regardless of where the entity ID lands
//! in the staggered `thinking_interval` cycle. Without the bootstrap
//! exception in `generate_urgency`, the brain's first arbitration sees an
//! empty urgency list and the agent idles for up to ~60 ticks.

use bevy::math::Vec2;
use worldsim::agent::nervous_system::cns::CentralNervousSystem;
use worldsim::testing::TestWorld;

#[test]
fn fresh_deer_has_urgencies_after_one_tick() {
    // Sweep multiple seeds so we exercise different `(tick + entity_id)`
    // alignments against the 60-tick stagger. Without the bootstrap
    // bypass, at least one of these seeds would land on a tick the
    // staggered loop skips.
    for seed in 0..16u64 {
        let mut world = TestWorld::with_seed(seed);
        let deer = world.spawn_deer(Vec2::new(40.0, 40.0));
        world.tick(1);

        let cns = world.get::<CentralNervousSystem>(deer);
        assert!(
            !cns.urgencies.is_empty(),
            "seed {seed}: fresh deer should have urgencies after 1 tick, got empty"
        );
    }
}
