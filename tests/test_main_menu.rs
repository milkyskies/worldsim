//! Integration tests for the main menu and simulation startup flow.
//!
//! Verifies that the chosen seed actually plumbs through `MapPlugin` and that
//! `OnEnter(AppState::InSim)` produces real terrain data — not just an
//! in-memory `SimConfig`.

use bevy::prelude::*;
use worldsim::menu::{AppState, SimConfig, SimMode};
use worldsim::world::map::{
    DEFAULT_TERRAIN_SEED, MapPlugin, WORLD_HEIGHT, WORLD_WIDTH, WorldMap, generate_terrain,
};

fn build_test_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(bevy::state::app::StatesPlugin);
    app.init_state::<AppState>();
    app.add_plugins(MapPlugin);
    app
}

fn enter_in_sim(app: &mut App) {
    let mut next = app.world_mut().resource_mut::<NextState<AppState>>();
    next.set(AppState::InSim);
    // First update commits the state transition and runs OnEnter systems.
    app.update();
    // Second update lets command flushes from setup_map settle.
    app.update();
}

#[test]
fn debug_sim_terrain_uses_seed_from_sim_config() {
    let chosen_seed: u32 = 12345;

    let mut app = build_test_app();
    app.insert_resource(SimConfig {
        mode: SimMode::Debug,
        seed: chosen_seed,
        world_name: "Seeded Debug".into(),
    });

    enter_in_sim(&mut app);

    let map = app.world().resource::<WorldMap>();
    let expected = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, chosen_seed);

    // The chosen seed must have actually been used: every tile in the populated
    // WorldMap should match what `generate_terrain` produces for that seed.
    let mut compared = 0usize;
    for y in 0..WORLD_HEIGHT {
        for x in 0..WORLD_WIDTH {
            let actual = map
                .get_tile(x, y)
                .unwrap_or_else(|| panic!("WorldMap missing tile at ({x},{y})"));
            let idx = (y * WORLD_WIDTH + x) as usize;
            assert_eq!(actual, expected[idx], "tile mismatch at ({x},{y})");
            compared += 1;
        }
    }
    assert_eq!(compared, (WORLD_WIDTH * WORLD_HEIGHT) as usize);
}

#[test]
fn debug_sim_with_explicit_seed_differs_from_default_seed() {
    // Sanity check: the test above only catches a regression if the chosen
    // seed actually produces a different terrain than DEFAULT_TERRAIN_SEED.
    let chosen_seed: u32 = 12345;
    assert_ne!(chosen_seed, DEFAULT_TERRAIN_SEED);

    let with_chosen = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, chosen_seed);
    let with_default = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
    assert_ne!(
        with_chosen, with_default,
        "chosen seed should produce a distinct terrain from the default seed"
    );
}

#[test]
fn map_plugin_does_not_run_setup_until_in_sim_state() {
    let mut app = build_test_app();
    app.insert_resource(SimConfig {
        mode: SimMode::Debug,
        seed: 42,
        world_name: "Pending".into(),
    });

    // Sit in the default MainMenu state across a couple of updates.
    app.update();
    app.update();

    let map = app.world().resource::<WorldMap>();
    // setup_map populates chunks; if it ran prematurely we'd see chunks here.
    assert!(
        map.chunks.is_empty(),
        "WorldMap should remain unpopulated while still on the main menu"
    );
}

#[test]
fn returning_to_main_menu_despawns_tilemap() {
    use worldsim::world::map::TileMap;

    let mut app = build_test_app();
    app.insert_resource(SimConfig {
        mode: SimMode::Debug,
        seed: 99,
        world_name: "Cleanup".into(),
    });

    enter_in_sim(&mut app);

    // The tilemap parent (with all its tile children) is now alive.
    {
        let mut q = app.world_mut().query::<&TileMap>();
        let count = q.iter(app.world()).count();
        assert_eq!(count, 1, "expected one TileMap parent after entering InSim");
    }

    // Player chooses Main Menu from the pause menu.
    {
        let mut next = app.world_mut().resource_mut::<NextState<AppState>>();
        next.set(AppState::MainMenu);
    }
    app.update();
    app.update();

    {
        let mut q = app.world_mut().query::<&TileMap>();
        let count = q.iter(app.world()).count();
        assert_eq!(
            count, 0,
            "TileMap should be despawned by DespawnOnExit when leaving InSim"
        );
    }
}

#[test]
fn re_entering_in_sim_after_main_menu_spawns_a_fresh_tilemap() {
    use worldsim::world::map::TileMap;

    let mut app = build_test_app();
    app.insert_resource(SimConfig {
        mode: SimMode::Debug,
        seed: 7,
        world_name: "First".into(),
    });

    enter_in_sim(&mut app);

    // Quit back to menu...
    {
        let mut next = app.world_mut().resource_mut::<NextState<AppState>>();
        next.set(AppState::MainMenu);
    }
    app.update();
    app.update();

    // ...then start a fresh sim with a different seed.
    app.insert_resource(SimConfig {
        mode: SimMode::Debug,
        seed: 31,
        world_name: "Second".into(),
    });
    enter_in_sim(&mut app);

    let mut q = app.world_mut().query::<&TileMap>();
    let count = q.iter(app.world()).count();
    assert_eq!(
        count, 1,
        "expected exactly one TileMap after re-entering InSim — old one should have been cleaned up"
    );
}
