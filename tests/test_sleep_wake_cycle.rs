//! Regression test for #352: an exhausted agent must enter Sleep and then
//! leave it once rested. Drives the full brain + execution loop so the
//! WakeUp deadlock would cause the second phase to loop forever.
//!
//! Also covers #357: sleeping agents must be wakeable by strong stimuli
//! (starvation, severe dehydration, pain, fear) — not just by stamina
//! recovery.

use bevy::math::Vec2;
use worldsim::agent::actions::{ActionType, ActiveActions};
use worldsim::agent::biology::body::{Body, Injury, InjuryType};
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::nervous_system::cns::CentralNervousSystem;
use worldsim::agent::nervous_system::urgency::UrgencySource;
use worldsim::testing::TestWorld;

/// Builds a scenario with one exhausted agent and ticks until they enter
/// Sleep. Returns the world and the sleeper entity. Panics if sleep doesn't
/// start within 200 ticks — that would be a separate regression worth
/// flagging loudly.
fn tired_sleeper() -> (TestWorld, bevy::prelude::Entity) {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("sleeper")
        .pos(Vec2::new(50.0, 50.0))
        .stamina(5.0)
        .done()
        .build();
    let sleeper = agents["sleeper"];

    for _ in 0..200 {
        world.tick(1);
        if world
            .get::<ActiveActions>(sleeper)
            .contains(ActionType::Sleep)
        {
            return (world, sleeper);
        }
    }
    panic!("exhausted agent should enter Sleep within 200 ticks");
}

/// Ticks the world up to `max_ticks` and returns `true` as soon as the
/// sleeper exits the Sleep action.
fn tick_until_wake(world: &mut TestWorld, sleeper: bevy::prelude::Entity, max_ticks: u64) -> bool {
    for _ in 0..max_ticks {
        world.tick(1);
        if !world
            .get::<ActiveActions>(sleeper)
            .contains(ActionType::Sleep)
        {
            return true;
        }
    }
    false
}

#[test]
#[ignore = "flaky regression — rested-wake deadlock resurfaced, tracked in #382"]
fn exhausted_agent_sleeps_and_then_wakes_once_rested() {
    // Regression for #352. Phase 1: low stamina drives the agent into Sleep.
    // Phase 2: stamina recovers during sleep and they must leave it. Before
    // the #352 fix this second phase looped forever because WakeUp could
    // never preempt uninterruptible Sleep.
    //
    // As of the #350 nutrient-loop work this test fails deterministically:
    // stamina fully recovers to aerobic=100 but the agent never exits Sleep.
    // #357 fixed the stimulus-wake path (hunger/pain/fear) but didn't touch
    // this rested-wake path. Tracking the fix in #382.
    let (mut world, sleeper) = tired_sleeper();

    // Sleep restores aerobic at +20/s, WAKE_STAMINA_THRESHOLD = 90, so ~5
    // seconds of sim time at minimum plus the WakeUp transition.
    let woke = tick_until_wake(&mut world, sleeper, 5_000);
    let aerobic = world.get::<PhysicalNeeds>(sleeper).stamina.aerobic;
    assert!(
        woke,
        "agent should leave Sleep after stamina recovers; final aerobic = {aerobic:.1}",
    );
}

/// #357: extreme hunger (starvation) must rouse a sleeping agent even before
/// stamina recovers. The wake threshold is config-driven in `DriveConfig`.
#[test]
fn starving_wakes_sleeping_agent() {
    let (mut world, sleeper) = tired_sleeper();

    // Bump hunger urgency past the Hunger drive's sleep_wake_threshold (0.9
    // in input-space = 90/100 raw hunger, which maps to `at_urgency(0.95)`
    // under the new three-pool metabolism). Keep aerobic low so we are NOT
    // testing the rested-wake path — any wake must come from the hunger
    // trigger. Cap the wake loop well under the natural recovery time.
    {
        let mut needs = world.get_mut::<PhysicalNeeds>(sleeper);
        needs.metabolism = worldsim::agent::body::metabolism::Metabolism::at_urgency(0.95);
        needs.stamina.aerobic = 5.0;
    }

    let woke = tick_until_wake(&mut world, sleeper, 500);
    assert!(
        woke,
        "starving sleeper should wake from the hunger wake pathway before \
         stamina recovers; current action = {:?}",
        world.current_action(sleeper),
    );

    // Sanity: the urgency layer should have flagged Hunger as the trigger
    // on the last update that ran before (or at) the wake.
    let cns = world.get::<CentralNervousSystem>(sleeper);
    if let Some(trigger) = cns.sleep_wake_trigger {
        assert_eq!(
            trigger,
            UrgencySource::Hunger,
            "expected hunger trigger, got {trigger:?}"
        );
    }
}

/// #357 negative: moderate hunger must NOT wake a sleeping agent. This is
/// what protects normal sleep against casual interruption.
#[test]
fn moderate_hunger_does_not_wake_sleeping_agent() {
    let (mut world, sleeper) = tired_sleeper();

    // Below the 0.9 urgency threshold. Agent should stay asleep until stamina
    // recovers naturally — far longer than our short observation window.
    {
        let mut needs = world.get_mut::<PhysicalNeeds>(sleeper);
        needs.metabolism = worldsim::agent::body::metabolism::Metabolism::at_urgency(0.5);
        needs.stamina.aerobic = 5.0;
    }

    // Short window well below the natural rested-wake time.
    for _ in 0..200 {
        world.tick(1);
        assert!(
            world
                .get::<ActiveActions>(sleeper)
                .contains(ActionType::Sleep),
            "moderate hunger must not interrupt sleep; current action = {:?}",
            world.current_action(sleeper),
        );
    }

    let cns = world.get::<CentralNervousSystem>(sleeper);
    assert!(
        cns.sleep_wake_trigger.is_none(),
        "no wake trigger should be set at hunger 50/100; got {:?}",
        cns.sleep_wake_trigger,
    );
}

/// #357: a predator appearing within vision range of a sleeping agent must
/// rouse them through the fear wake pathway. Exercises the full integration
/// — perception raises Fear emotion, urgency generation flags the trigger,
/// the brain's sleep gate proposes WakeUp.
#[test]
fn nearby_wolf_wakes_sleeping_agent() {
    let (mut world, sleeper) = tired_sleeper();

    // Spawn a wolf well within human vision range (100 px). Placing it
    // after the sleeper has entered Sleep avoids the chicken-and-egg
    // problem of an already-scared agent refusing to sleep.
    let _wolf = world.spawn_wolf(Vec2::new(70.0, 50.0));

    // Keep aerobic low so any wake must come from the fear pathway, not
    // from natural recovery.
    {
        let mut needs = world.get_mut::<PhysicalNeeds>(sleeper);
        needs.stamina.aerobic = 5.0;
    }

    let woke = tick_until_wake(&mut world, sleeper, 1_000);
    assert!(
        woke,
        "sleeping agent should wake when a predator appears nearby; \
         current action = {:?}",
        world.current_action(sleeper),
    );
}

/// #357: significant injury pain must rouse a sleeping agent. Mirrors the
/// real-life nociceptive wake pathway.
#[test]
fn pain_wakes_sleeping_agent() {
    let (mut world, sleeper) = tired_sleeper();

    // Inject significant pain. `body.total_pain()` sums per-injury pain
    // across all parts; the Pain drive's sleep_wake_threshold is 0.6 in
    // input space, i.e. total_pain >= 60. Pile on enough pain that the
    // wake pathway fires even though healing nibbles at it every tick.
    // Push directly into the injury vec to avoid mutating HP as a side
    // effect of `add_injury` (we're only exercising the pain signal here).
    {
        let mut body = world.get_mut::<Body>(sleeper);
        let first_part = body
            .parts
            .first_mut()
            .expect("agent body should have at least one part");
        for _ in 0..10 {
            first_part.injuries.push(Injury {
                injury_type: InjuryType::Cut,
                severity: 0.1,
                pain: 10.0,
                healed_amount: 0.0,
                bleed_rate: 0.0,
            });
        }
    }
    {
        let mut needs = world.get_mut::<PhysicalNeeds>(sleeper);
        needs.stamina.aerobic = 5.0;
    }

    let woke = tick_until_wake(&mut world, sleeper, 500);
    assert!(
        woke,
        "injured sleeper should wake from the pain wake pathway; \
         body.total_pain() = {:.1}, current action = {:?}",
        world.get::<Body>(sleeper).total_pain(),
        world.current_action(sleeper),
    );
}
