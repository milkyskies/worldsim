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
use worldsim::agent::events::{SimEvent, SimEventKind};
use worldsim::agent::mind::knowledge::{AgentName, MindGraph};
use worldsim::agent::mind::recognition::init_relationship_dimensions;
use worldsim::agent::mind::social_identity::SocialIdentity;
use worldsim::testing::TestWorld;

fn introduce(world: &mut TestWorld, observer: Entity, target: Entity, name: &str, affection: f32) {
    if let Some(mut social) = world
        .app_mut()
        .world_mut()
        .get_mut::<SocialIdentity>(observer)
    {
        social.introduce(target, AgentName(name.to_string()), 0);
    }
    let mut mind = world.get_mut::<MindGraph>(observer);
    init_relationship_dimensions(&mut mind, target, 0, affection);
}

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
    introduce(&mut world, deer_a, deer_b, "Deer 2", 0.8);
    introduce(&mut world, deer_b, deer_a, "Deer 1", 0.8);

    // Tick once to let `develop_phenotype_system` run (Added<Genome>) so the
    // drives it computes from the genome don't clobber our test-authored
    // values below. After this tick, direct drive writes are stable.
    world.tick(1);

    // Crank both deer to maximum loneliness.
    {
        let mut drives = world.get_mut::<PsychologicalDrives>(deer_a);
        drives.companionship.set(0.0);
    }
    {
        let mut drives = world.get_mut::<PsychologicalDrives>(deer_b);
        drives.companionship.set(0.0);
    }

    let start_companionship = world
        .app()
        .world()
        .get::<PsychologicalDrives>(deer_a)
        .unwrap()
        .companionship
        .value;

    world.tick(200);

    let end_companionship = world
        .app()
        .world()
        .get::<PsychologicalDrives>(deer_a)
        .unwrap()
        .companionship
        .value;

    assert!(
        end_companionship > start_companionship,
        "deer A's companionship satisfaction should rise in the presence of visible kin (start {start_companionship:.2}, end {end_companionship:.2})"
    );
}

/// A lonely deer with a visible kin should drift closer to them over
/// time. Post-#642 the tile-based scorer picks a tile near the kin;
/// Walk's target is a position, not an entity, so we assert on
/// end-distance, not on the target_entity field.
#[test]
fn lonely_deer_with_visible_kin_walks_toward_them() {
    let mut world = TestWorld::with_seed(42);

    // Spawn two deer close enough that perception sees them even at
    // dawn light. Deer vision is 80px and the simulation starts at 6am
    // (light ≈ 0.65), so effective range is ~52px — put them 20px apart
    // to stay safely in-range all day.
    let lonely = world.spawn_deer(Vec2::new(40.0, 40.0));
    let friend_pos = Vec2::new(60.0, 40.0);
    let friend = world.spawn_deer(friend_pos);

    // Introduce them at high affection so the flock-walk picks the
    // friend over any random deer that might wander in.
    introduce(&mut world, lonely, friend, "Deer 2", 0.9);
    introduce(&mut world, friend, lonely, "Deer 1", 0.9);

    // Tick once to let `develop_phenotype_system` run, so our direct drive
    // mutation below isn't overwritten by the genome→drives pipeline.
    world.tick(1);

    let start_pos = world
        .app()
        .world()
        .get::<bevy::prelude::Transform>(lonely)
        .unwrap()
        .translation
        .truncate();

    // Pin the lonely deer's social drive low (=deficit high) AND the
    // friend's social drive high (=no deficit) each tick. Without
    // pinning the friend, both deer drift toward each other and the
    // test can't measure one-sided convergence.
    for _ in 0..200 {
        {
            let mut drives = world.get_mut::<PsychologicalDrives>(lonely);
            drives.companionship.set(0.0);
        }
        {
            let mut drives = world.get_mut::<PsychologicalDrives>(friend);
            drives.companionship.set(1.0);
        }
        // Pin the friend's transform too so even if something else
        // moves them (wander, rational plan), the measurement baseline
        // stays fixed.
        world
            .get_mut::<bevy::prelude::Transform>(friend)
            .translation = bevy::prelude::Vec3::new(friend_pos.x, friend_pos.y, 0.0);
        world.tick(1);
    }

    let end_pos = world
        .app()
        .world()
        .get::<bevy::prelude::Transform>(lonely)
        .unwrap()
        .translation
        .truncate();
    let start_dist = start_pos.distance(friend_pos);
    let end_dist = end_pos.distance(friend_pos);

    assert!(
        end_dist < start_dist - 5.0,
        "lonely deer should drift toward visible kin; start_dist={start_dist:.1}, end_dist={end_dist:.1}"
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
        drives.companionship.set(0.0);
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
            SimEvent { kind: SimEventKind::ActionStarted { agent, action: ActionType::Walk, target: Some(_), .. }, .. } if *agent == alone
        )
    });

    assert!(
        !started_targeted_walk,
        "isolated deer should not propose a flock-walk — there's nothing to walk toward"
    );
}
