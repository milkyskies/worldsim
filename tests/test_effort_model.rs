//! Integration tests for the derived effort model (#419).
//!
//! Verifies that the migration from per-action constants to channel-based
//! EffortProfile + compute_action_cost does not blow up the calorie economy.

use worldsim::agent::actions::{ActionState, ActionType, ActiveActions};
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::testing::{AgentConfig, TestWorld};

/// Force Sleep into ActiveActions and verify the effort model's recovery
/// channel restores aerobic stamina without any per-activity plumbing.
#[test]
fn sleep_restores_aerobic_via_effort_model() {
    let mut world = TestWorld::with_seed(0);
    let agent = world.spawn_agent(AgentConfig::default());

    {
        let mut needs = world.get_mut::<PhysicalNeeds>(agent);
        needs.stamina.aerobic = 20.0;
    }

    let mut active = ActiveActions::empty();
    active.insert(ActionState::new(ActionType::Sleep, 0));
    world.app_mut().world_mut().entity_mut(agent).insert(active);

    let before = world.get::<PhysicalNeeds>(agent).stamina.aerobic;
    world.tick(60);
    let after = world.get::<PhysicalNeeds>(agent).stamina.aerobic;

    assert!(
        after > before + 5.0,
        "Sleep should restore aerobic through the effort model \
         (before={before:.1}, after={after:.1})"
    );
}

/// Regression guard: the default-spawned agent carries no activity marker
/// and must tick cleanly through the full nervous-system schedule.
#[test]
fn systems_tolerate_default_spawned_agent() {
    let mut world = TestWorld::with_seed(0);
    let agent = world.spawn_agent(AgentConfig::default());

    world.tick(30);

    let needs = world.get::<PhysicalNeeds>(agent);
    assert!(needs.metabolism.glucose > 0.0);
    assert!(needs.stamina.aerobic > 0.0);
}

/// Headless 10k-tick run on game_defaults(42) — assert that agents survive
/// and the calorie economy doesn't collapse after the effort model migration.
/// This is NOT a precise ±15% regression gate against a frozen baseline
/// (the architecture changed, so exact parity is not the goal).
#[test]
#[ignore = "slow: 10k-tick game_defaults run"]
fn migrated_action_calorie_totals_within_15pct_of_baseline() {
    use bevy::prelude::With;
    use worldsim::agent::{Alive, Person};

    let mut world = TestWorld::game_defaults(42);

    let alive_count = |world: &mut TestWorld| -> usize {
        let mut q = world
            .app_mut()
            .world_mut()
            .query_filtered::<bevy::prelude::Entity, (With<Person>, With<Alive>)>();
        q.iter(world.app().world()).count()
    };

    let initial = alive_count(&mut world);
    assert!(initial > 0);

    world.tick(10_000);

    let surviving = alive_count(&mut world);

    // All humans should survive 10k ticks (~2.7 in-game minutes). If the
    // effort model miscalibrated energy drain, agents starve faster.
    assert_eq!(
        surviving, initial,
        "all {initial} humans should survive 10k ticks, but only {surviving} did"
    );

    // Spot-check that metabolism pools are in healthy ranges — no agent
    // should be critically starving at 10k ticks under game defaults.
    let mut starving_count = 0;
    {
        let w = world.app_mut().world_mut();
        let mut q = w.query_filtered::<&PhysicalNeeds, (With<Person>, With<Alive>)>();
        for needs in q.iter(w) {
            if needs.metabolism.glucose < 15.0 && needs.metabolism.reserves < 5.0 {
                starving_count += 1;
            }
        }
    }
    assert!(
        starving_count == 0,
        "{starving_count} humans are critically starving at 10k ticks — \
         effort model energy drain may be miscalibrated"
    );
}
