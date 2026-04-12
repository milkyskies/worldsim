//! Integration tests for the deer/wolf flocking drive (#260).
//!
//! Verifies the complete loop:
//!   1. Deer have `PsychologicalDrives` (without it the drive system has
//!      nothing to act on).
//!   2. The proximity-decay system reduces social drive when an
//!      affection-rated conspecific is visible.
//!   3. A separated, lonely deer with a known herd-mate visible proposes a
//!      Walk toward that herd-mate via the emotional brain.
//!
//! These tests exercise the *mechanism*. Statistical "do herds form
//! across many seeds" tests are deliberately omitted — they belong in a
//! separate emergence harness, not the unit-test runner.

use bevy::prelude::*;
use worldsim::agent::actions::ActionType;
use worldsim::agent::body::needs::PsychologicalDrives;
use worldsim::agent::events::SimEvent;
use worldsim::agent::mind::knowledge::MindGraph;
use worldsim::agent::mind::recognition::initialize_relationship_with_affection;
use worldsim::testing::TestWorld;

/// Deer must spawn with `PsychologicalDrives` so the urgency / decay
/// pipeline has a field to act on. Before #260 they had no drives at all.
#[test]
fn deer_spawns_with_psychological_drives() {
    let mut world = TestWorld::with_seed(42);
    let deer = world.spawn_deer(Vec2::new(40.0, 40.0));
    let drives = world.app().world().get::<PsychologicalDrives>(deer);
    assert!(
        drives.is_some(),
        "deer should ship with PsychologicalDrives so social drive can decay"
    );
}

/// Two deer who know each other (herd-mates with kin-level affection)
/// should drive each other's social loneliness down toward zero when in
/// vision range. The decay system runs every 10 ticks; 200 ticks is
/// plenty of cycles for a kin-affection deer-pair to satisfy each other.
#[test]
fn visible_kin_decay_social_drive() {
    let mut world = TestWorld::with_seed(42);

    // Spawn two deer close enough to see each other.
    let deer_a = world.spawn_deer(Vec2::new(40.0, 40.0));
    let deer_b = world.spawn_deer(Vec2::new(60.0, 40.0));

    // Mutually introduce them at kin-level affection.
    {
        let mut mind_a = world.get_mut::<MindGraph>(deer_a);
        initialize_relationship_with_affection(&mut mind_a, deer_b, "Deer 2", 0, 0.8);
    }
    {
        let mut mind_b = world.get_mut::<MindGraph>(deer_b);
        initialize_relationship_with_affection(&mut mind_b, deer_a, "Deer 1", 0, 0.8);
    }

    // Tick once to let `develop_phenotype_system` run (Added<Genome>) so the
    // drives it computes from the genome don't clobber our test-authored
    // values below. After this tick, direct drive writes are stable.
    world.tick(1);

    // Crank both deer to maximum loneliness.
    {
        let mut drives = world.get_mut::<PsychologicalDrives>(deer_a);
        drives.companionship = 0.0;
    }
    {
        let mut drives = world.get_mut::<PsychologicalDrives>(deer_b);
        drives.companionship = 0.0;
    }

    let start_companionship = world
        .app()
        .world()
        .get::<PsychologicalDrives>(deer_a)
        .unwrap()
        .companionship;

    world.tick(200);

    let end_companionship = world
        .app()
        .world()
        .get::<PsychologicalDrives>(deer_a)
        .unwrap()
        .companionship;

    assert!(
        end_companionship > start_companionship,
        "deer A's companionship satisfaction should rise in the presence of visible kin (start {start_companionship:.2}, end {end_companionship:.2})"
    );
}

/// A lonely deer with a visible kin should propose a `Walk` action via the
/// emotional brain. We cap the social drive high and assert at least one
/// `ActionStarted { action: Walk }` event fires for the lonely deer.
#[test]
fn lonely_deer_with_visible_kin_walks_toward_them() {
    let mut world = TestWorld::with_seed(42);

    // Spawn two deer close enough that perception sees the other deer
    // immediately. Vision range for deer is 128px.
    let lonely = world.spawn_deer(Vec2::new(40.0, 40.0));
    let friend = world.spawn_deer(Vec2::new(120.0, 40.0));

    // Introduce them at high affection so the flock-walk picks the
    // friend over any random deer that might wander in.
    {
        let mut mind = world.get_mut::<MindGraph>(lonely);
        initialize_relationship_with_affection(&mut mind, friend, "Deer 2", 0, 0.9);
    }
    {
        let mut mind = world.get_mut::<MindGraph>(friend);
        initialize_relationship_with_affection(&mut mind, lonely, "Deer 1", 0, 0.9);
    }

    // Tick once to let `develop_phenotype_system` run, so our direct drive
    // mutation below isn't overwritten by the genome→drives pipeline.
    world.tick(1);

    // Pin the lonely deer's social drive high so the urgency dominates
    // for the duration of the test (otherwise the proximity decay we
    // just enabled will erase it before the brain even runs).
    {
        let mut drives = world.get_mut::<PsychologicalDrives>(lonely);
        drives.companionship = 0.0;
    }

    // Tick enough for: perception (60), thinking interval, action
    // admission. The thinking interval defaults to ~60 ticks, so 200 is
    // ample for at least one decision cycle.
    world.tick(200);

    let started_walk_for_lonely = world.sim_events().all().iter().any(|ev| {
        matches!(
            ev,
            SimEvent::ActionStarted {
                agent,
                action: ActionType::Walk,
                target: Some(_),
                ..
            } if *agent == lonely
        )
    });

    assert!(
        started_walk_for_lonely,
        "lonely deer should start a Walk action toward a target (its kin)"
    );
}

/// A solitary lonely deer (no visible conspecific anywhere) should NOT
/// trigger the flock-walk path. This is the negative case for
/// `seek_flock_proximity` — without a target, the function returns None
/// and the brain falls through to whatever else it would have done.
#[test]
fn lonely_deer_alone_does_not_flock_walk() {
    let mut world = TestWorld::with_seed(42);

    // Single deer in the middle of nowhere. Vision range 128px, so we
    // need to be sure no other deer is anywhere nearby.
    let alone = world.spawn_deer(Vec2::new(400.0, 400.0));

    // Tick once so `develop_phenotype_system` runs before we touch drives.
    world.tick(1);

    {
        let mut drives = world.get_mut::<PsychologicalDrives>(alone);
        drives.companionship = 0.0;
    }

    world.tick(200);

    // We don't assert "no Walk ever" because Wander is also Movement and
    // can fire as an idle wander. The specific assertion is that no
    // Walk event has a target_entity set — flock-walk is the only thing
    // that targets an entity for a non-Person agent without inventory or
    // resources to pursue.
    let started_targeted_walk = world.sim_events().all().iter().any(|ev| {
        matches!(
            ev,
            SimEvent::ActionStarted {
                agent,
                action: ActionType::Walk,
                target: Some(_),
                ..
            } if *agent == alone
        )
    });

    assert!(
        !started_targeted_walk,
        "isolated deer should not propose a flock-walk — there's nothing to walk toward"
    );
}
