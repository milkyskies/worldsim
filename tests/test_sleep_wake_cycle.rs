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
use worldsim::agent::events::{SimEvent, SimEventKind};
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
        .wakefulness(0.1)
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

/// Ticks the world up to `max_ticks` iterations and returns `true` as soon
/// as the sleeper exits the Sleep action. Each iteration advances one cycle
/// at the world's current `game_seconds_per_cycle` — so calling
/// `enable_fast_forward` before invoking this cuts wall-clock by 60×.
fn tick_until_wake(world: &mut TestWorld, sleeper: bevy::prelude::Entity, max_ticks: u64) -> bool {
    let step = world
        .app()
        .world()
        .resource::<worldsim::core::TickCount>()
        .game_seconds_per_cycle
        .max(1);
    for _ in 0..max_ticks {
        world.tick(step);
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
fn exhausted_agent_sleeps_and_then_wakes_once_rested() {
    // Regression for #352. Phase 1: low stamina drives the agent into Sleep.
    // Phase 2: wakefulness AND stamina both recover, triggering WakeUp.
    //
    // The rested-wake condition requires BOTH wakefulness >= 0.95 AND
    // aerobic_fraction >= 0.9. Wakefulness recovers at SLEEP_RESTORE_RATE
    // (0.00167/rate-sec): from 0.1 to 0.95 takes ~509 rate-seconds =
    // ~30500 ticks. Allow generous headroom.
    let (mut world, sleeper) = tired_sleeper();
    world.enable_fast_forward();

    let woke = tick_until_wake(&mut world, sleeper, 700);
    let aerobic = world.get::<PhysicalNeeds>(sleeper).stamina.aerobic;
    assert!(
        woke,
        "agent should leave Sleep after wakefulness and stamina recover; \
         aerobic = {aerobic:.1}",
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

/// Regression for the silent-Sleep-drop bug: in the observed #496 event log,
/// two `ActionStarted Sleep` events fired for the same agent within a few
/// hundred ticks of each other with no intervening `ActionPreempted`,
/// `ActionCompleted`, or `ActionFailed` for Sleep. That means Sleep left
/// `ActiveActions` invisibly, the brain re-proposed it, and it got admitted
/// again — which is what the flapping timeline in the 4-day trace showed.
///
/// This test fails if any two consecutive `ActionStarted Sleep` events for
/// the sleeper aren't separated by a terminating Sleep event.
#[test]
fn sleep_start_always_has_matching_terminator() {
    let (mut world, sleeper) = tired_sleeper();
    world.enable_fast_forward();

    // Tick long enough for several sleep-wake cycles to happen. One full
    // sleep cycle is roughly 7000 ticks; run 15k to cover the flap window
    // the 4-day trace captured.
    world.tick(15_000);

    let events: Vec<_> = world
        .sim_events()
        .all()
        .iter()
        .filter_map(|e| match e {
            SimEvent {
                tick,
                kind:
                    SimEventKind::ActionStarted {
                        agent,
                        action: ActionType::Sleep,
                        ..
                    },
                ..
            } if *agent == sleeper => Some(("Started", *tick)),
            SimEvent {
                tick,
                kind:
                    SimEventKind::ActionPreempted {
                        agent,
                        preempted_action: ActionType::Sleep,
                        ..
                    },
                ..
            } if *agent == sleeper => Some(("Preempted", *tick)),
            SimEvent {
                tick,
                kind:
                    SimEventKind::ActionCompleted {
                        agent,
                        action: ActionType::Sleep,
                        ..
                    },
                ..
            } if *agent == sleeper => Some(("Completed", *tick)),
            SimEvent {
                tick,
                kind:
                    SimEventKind::ActionFailed {
                        agent,
                        action: ActionType::Sleep,
                        ..
                    },
                ..
            } if *agent == sleeper => Some(("Failed", *tick)),
            _ => None,
        })
        .collect();

    // Walk the event list. Every "Started" must be either the first Sleep
    // event, or immediately preceded by a terminator (Preempted/Completed/
    // Failed). Two consecutive "Started"s indicate a silent drop.
    let mut prev: Option<(&str, u64)> = None;
    for (kind, tick) in &events {
        if *kind == "Started"
            && let Some((prev_kind, prev_tick)) = prev
            && prev_kind == "Started"
        {
            panic!(
                "silent Sleep drop: two ActionStarted Sleep events in a row \
                 without an intervening terminator (Preempted/Completed/Failed). \
                 previous start at tick {prev_tick}, next start at tick {tick}. \
                 full event list: {:?}",
                events
            );
        }
        prev = Some((*kind, *tick));
    }
}
