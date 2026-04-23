//! Integration tests for the tile-based temperature grid (#644).
//!
//! Covers the behaviors the grid was built for: day/night ambient swings
//! pass through, two overlapping fires stack, residual heat fades after
//! an emitter is removed, `tick_warmth` reads from the grid (no
//! HeatSource scan), and sparse storage stays bounded.

use bevy::math::{IVec2, Vec2};
use worldsim::agent::body::need::Need;
use worldsim::constants::thermal::{DAY_AMBIENT_C, NIGHT_AMBIENT_C};
use worldsim::core::tick::TickCount;
use worldsim::core::time::GameTime;
use worldsim::testing::{AgentConfig, TestWorld};
use worldsim::world::field_grid_plugin::FieldGrids;
use worldsim::world::map::TILE_SIZE;

fn pos_to_tile(pos: Vec2) -> IVec2 {
    IVec2::new(
        (pos.x / TILE_SIZE).floor() as i32,
        (pos.y / TILE_SIZE).floor() as i32,
    )
}

fn sample_temp(world: &TestWorld, tile: IVec2) -> f32 {
    world
        .app()
        .world()
        .resource::<FieldGrids>()
        .temperature()
        .sample_tile(tile)
}

fn ambient(world: &TestWorld) -> f32 {
    world
        .app()
        .world()
        .resource::<FieldGrids>()
        .temperature()
        .ambient
}

/// Fast-forward the simulation's raw tick counter so the derived
/// GameTime lands on `hour`. Setting `GameTime.hours` directly doesn't
/// stick — `deterministic_tick` re-derives it from `TickCount.current`
/// every tick — so we have to move the tick counter instead.
fn jump_to_hour(world: &mut TestWorld, hour: u32) {
    let offset_hours = (hour + 24 - GameTime::START_HOUR as u32) % 24;
    let target_tick = offset_hours as u64 * GameTime::TICKS_PER_HOUR;
    world
        .app_mut()
        .world_mut()
        .resource_mut::<TickCount>()
        .current = target_tick;
    // One tick so deterministic_tick, update_light_level, and the
    // thermal-ambient system all see the new clock.
    world.tick(1);
}

/// With the game clock at noon, the temperature grid should track
/// `DAY_AMBIENT_C`. With the clock at midnight, it should track
/// `NIGHT_AMBIENT_C`. Proves day/night drives ambient via LightLevel.
#[test]
fn day_ambient_is_warmer_than_night_ambient() {
    let mut world = TestWorld::with_seed(0);

    jump_to_hour(&mut world, 12);
    let day = ambient(&world);

    jump_to_hour(&mut world, 0);
    let night = ambient(&world);

    assert!(
        (day - DAY_AMBIENT_C).abs() < 0.1,
        "noon ambient should match DAY_AMBIENT_C={DAY_AMBIENT_C}, got {day}"
    );
    assert!(
        (night - NIGHT_AMBIENT_C).abs() < 0.1,
        "midnight ambient should match NIGHT_AMBIENT_C={NIGHT_AMBIENT_C}, got {night}"
    );
}

/// Tile near a campfire heats well above ambient within a few ticks of
/// injection. Proves the emitter-injection system runs and the grid
/// accumulates deltas.
#[test]
fn campfire_heats_its_tile_above_ambient() {
    let mut world = TestWorld::with_seed(0);
    world.spawn_campfire(Vec2::new(0.0, 0.0));

    // Let the cell reach near-steady state (half-life ≈ 2 game-seconds,
    // so 10 game-seconds = 5 half-lives = ~97% of steady state).
    world.tick(600);

    let source_temp = sample_temp(&world, IVec2::new(0, 0));
    let baseline = ambient(&world);
    assert!(
        source_temp > baseline + 20.0,
        "cell on fire tile should be >20°C above ambient; got {source_temp:.1} vs ambient {baseline:.1}"
    );
}

/// Two overlapping campfires on the same tile produce a hotter cell
/// than one — injection is additive, so stacked emitters sum. The old
/// radial-scan path used `max()` and missed this.
#[test]
fn two_fires_stack_hotter_than_one() {
    let mut one = TestWorld::with_seed(0);
    one.spawn_campfire(Vec2::new(0.0, 0.0));
    one.tick(600);
    let one_fire_temp = sample_temp(&one, IVec2::new(0, 0));

    let mut two = TestWorld::with_seed(0);
    two.spawn_campfire(Vec2::new(0.0, 0.0));
    two.spawn_campfire(Vec2::new(0.0, 0.0));
    two.tick(600);
    let two_fire_temp = sample_temp(&two, IVec2::new(0, 0));

    assert!(
        two_fire_temp > one_fire_temp + 5.0,
        "two campfires on same tile should be meaningfully hotter than one; \
         got one={one_fire_temp:.1} vs two={two_fire_temp:.1}"
    );
}

/// After a campfire despawns, its heat should dissipate over a few
/// seconds of damping, not persist indefinitely. Also not vanish
/// instantly — residual warmth is the whole point of having a grid.
#[test]
fn residual_heat_fades_after_emitter_removed() {
    let mut world = TestWorld::with_seed(0);
    let campfire = world.spawn_campfire(Vec2::new(0.0, 0.0));
    world.tick(600); // let the cell heat up
    let hot = sample_temp(&world, IVec2::new(0, 0));
    let baseline = ambient(&world);
    assert!(hot > baseline + 20.0, "pre-condition: cell should be hot");

    world.app_mut().world_mut().entity_mut(campfire).despawn();
    world.tick(30); // a fraction of a half-life — still visibly warm

    let soon_after = sample_temp(&world, IVec2::new(0, 0));
    assert!(
        soon_after > baseline + 5.0,
        "residual heat should linger briefly after emitter despawn; \
         got {soon_after:.1} (ambient {baseline:.1})"
    );
    assert!(
        soon_after < hot,
        "residual heat must be decaying, not matching the pre-removal peak"
    );

    world.tick(1200); // several half-lives — should approach ambient
    let much_later = sample_temp(&world, IVec2::new(0, 0));
    // Recapture ambient: it may have drifted with the in-sim clock
    // advancing through dawn; the cell should track current ambient,
    // not the one we captured earlier in the test.
    let ambient_now = ambient(&world);
    assert!(
        (much_later - ambient_now).abs() < 1.0,
        "cell should be back near current ambient after several half-lives; \
         got {much_later:.1} (ambient now {ambient_now:.1})"
    );
}

/// `tick_warmth` reads the grid — no `HeatSource` entity should be
/// required for an agent to warm up if the cell is hot. Manually inject
/// into the grid, pin the agent on that tile, and confirm they recover
/// warmth.
#[test]
fn tick_warmth_reads_from_grid_not_entities() {
    let mut world = TestWorld::with_seed(0);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(0.0, 0.0),
        warmth: 0.2,
        ..Default::default()
    });
    world.tick(1); // settle phenotype

    let agent_tile = pos_to_tile(Vec2::new(0.0, 0.0));

    // Pin the agent and inject a big per-tick bump into the grid
    // each cycle — big enough that steady-state easily exceeds
    // `FULL_RECOVERY_C` so the agent's `tick_warmth` sampling sees
    // max recovery regardless of where relaxation is pulling.
    for _ in 0..600 {
        world
            .app_mut()
            .world_mut()
            .resource_mut::<FieldGrids>()
            .temperature_mut()
            .inject_at_tile(agent_tile, 2.0);
        world.get_mut::<bevy::prelude::Transform>(agent).translation =
            bevy::prelude::Vec3::new(0.0, 0.0, 0.0);
        world.tick(1);
    }

    let warmth = world.agent_warmth(agent);
    assert!(
        warmth > 0.35,
        "agent on a hot tile (no HeatSource entity) should still warm up from grid sampling; \
         got {warmth:.3} from start 0.2"
    );
}

/// A world with one campfire and one agent should use a bounded number
/// of chunks. Exact count is implementation-detail (depends on fire
/// position and diffusion ring expansion) but it must stay in single
/// digits — the grid is sparse on purpose.
#[test]
fn sparse_storage_stays_bounded() {
    let mut world = TestWorld::with_seed(0);
    world.spawn_campfire(Vec2::new(0.0, 0.0));
    world.tick(600);

    let active = world
        .app()
        .world()
        .resource::<FieldGrids>()
        .temperature()
        .active_chunks();

    assert!(
        active < 10,
        "single campfire should leave the grid nearly empty, not scatter chunks; \
         got {active} active chunks"
    );
}

/// Control: in a world with no emitters, the grid stores zero deltas
/// and every sample returns exactly the current ambient.
#[test]
fn empty_world_has_no_deltas_and_samples_return_ambient() {
    let mut world = TestWorld::with_seed(0);
    world.tick(60);

    let amb = ambient(&world);
    for tile in [
        IVec2::new(0, 0),
        IVec2::new(100, 0),
        IVec2::new(-50, 50),
        IVec2::new(500, 500),
    ] {
        assert_eq!(
            sample_temp(&world, tile),
            amb,
            "empty world should sample ambient exactly at every tile"
        );
    }
    assert_eq!(
        world
            .app()
            .world()
            .resource::<FieldGrids>()
            .temperature()
            .active_chunks(),
        0,
        "no emitters → no allocated chunks"
    );
}

/// Sanity: a cold, exposed agent at night has warmth drain, even with
/// no HeatSource entity — the grid reflects ambient cold, `tick_warmth`
/// drains. Verifies the new exposure path flows through the grid.
#[test]
fn exposed_agent_at_night_drains_warmth() {
    let mut world = TestWorld::with_seed(0);
    jump_to_hour(&mut world, 0); // midnight

    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(1000.0, 1000.0),
        warmth: 0.8,
        ..Default::default()
    });
    // Need mut here to test the pin path against AI movement.
    for _ in 0..200 {
        world.get_mut::<bevy::prelude::Transform>(agent).translation =
            bevy::prelude::Vec3::new(1000.0, 1000.0, 0.0);
        let mut needs = world.get_mut::<worldsim::agent::body::needs::PhysicalNeeds>(agent);
        if needs.warmth.value > 0.8 {
            needs.warmth = Need::new(0.8);
        }
        world.tick(1);
    }
    let after = world.agent_warmth(agent);
    assert!(
        after < 0.8,
        "exposed agent at midnight should cool; before=0.8, after={after:.3}"
    );
}
