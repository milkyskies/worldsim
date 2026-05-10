//! Bug 5 (#496): Rest must self-terminate when aerobic stamina recovers.
//!
//! Before this fix, Rest was `ActionKind::Timed { duration_ticks: u32::MAX }`
//! with no completion predicate — it ran forever, accumulating thousands of
//! starts with zero completions. The only exit was preemption by a Moving
//! action, which left Rest lingering alongside Harvest/Eat/Wander.

use bevy::math::Vec2;
use worldsim::agent::Dazed;
use worldsim::agent::actions::{ActionType, ActiveActions};
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::events::{SimEvent, SimEventKind};
use worldsim::testing::TestWorld;

/// Rest must self-complete (ActionCompleted) when aerobic crosses the 0.95
/// fraction threshold, rather than lingering until preempted.
#[test]
fn rest_self_completes_when_aerobic_recovers() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("rester")
        .pos(Vec2::new(50.0, 50.0))
        // Start aerobic just below the 0.95 threshold so Rest completes
        // quickly before the brain can preempt with Wander.
        .stamina(90.0)
        .done()
        .build();
    let rester = agents["rester"];

    // Force Rest into the active set.
    {
        let mut active = world.get_mut::<ActiveActions>(rester);
        active.insert(
            worldsim::agent::actions::registry::ActionState::new(ActionType::Rest, 0)
                .with_duration(u32::MAX),
        );
    }

    // Daze so the brain doesn't preempt Rest before it self-completes.
    world
        .app_mut()
        .world_mut()
        .entity_mut(rester)
        .insert(Dazed {
            until_tick: u64::MAX,
        });

    // Tick enough for aerobic to recover from 0.90 to 0.95. Rest's effort
    // profile is mild, and aerobic recovers at ~0.3/s at rest intensity.
    // Should cross the threshold within a few hundred ticks.
    world.tick(2_000);

    // Check that we got an ActionCompleted Rest, not just ActionPreempted.
    let completed = world
        .sim_events()
        .all()
        .iter()
        .any(|e| matches!(e, SimEvent { kind: SimEventKind::ActionCompleted { agent, action: ActionType::Rest, .. }, .. } if *agent == rester));

    assert!(
        completed,
        "Rest should fire ActionCompleted (self-complete) when aerobic \
         recovers past 0.95; got only preemptions or no Rest events at all"
    );
}

/// Rest must NOT complete while aerobic is still well below threshold.
#[test]
fn rest_stays_active_while_aerobic_is_low() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("rester")
        .pos(Vec2::new(50.0, 50.0))
        .stamina(30.0)
        .done()
        .build();
    let rester = agents["rester"];

    {
        let mut active = world.get_mut::<ActiveActions>(rester);
        active.insert(
            worldsim::agent::actions::registry::ActionState::new(ActionType::Rest, 0)
                .with_duration(u32::MAX),
        );
    }

    // Daze so the brain doesn't replace Rest with a Sleep/Wander proposal.
    world
        .app_mut()
        .world_mut()
        .entity_mut(rester)
        .insert(Dazed {
            until_tick: u64::MAX,
        });

    // Tick a very short window — aerobic should still be recovering.
    world.tick(60);

    let aerobic_frac = world
        .get::<PhysicalNeeds>(rester)
        .stamina
        .aerobic_fraction();
    assert!(
        aerobic_frac < 0.95,
        "sanity: aerobic should still be low after 60 ticks; got {aerobic_frac:.3}"
    );

    // No ActionCompleted Rest should have fired.
    let completed = world
        .sim_events()
        .all()
        .iter()
        .any(|e| matches!(e, SimEvent { kind: SimEventKind::ActionCompleted { agent, action: ActionType::Rest, .. }, .. } if *agent == rester));

    assert!(
        !completed,
        "Rest must NOT self-complete while aerobic ({aerobic_frac:.3}) is below 0.95"
    );
}
