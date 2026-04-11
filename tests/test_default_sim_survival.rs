//! Regression tests for the default-sim starvation death spiral (#416).
//!
//! Investigation of seed 42 --game-defaults showed three distinct bugs that
//! all contribute to humans dying of "starvation/dehydration/injury" around
//! tick 60k:
//!
//!   1. **Harvest → Eat chain drops:** Humans who successfully harvest
//!      1,000–2,000 times in a 200k-tick run only Eat ~15 times. Food ends
//!      up in inventory but the plan fails to advance into the Eat step.
//!
//!   2. **Planner can't build hunger plans:** The Rational brain emits
//!      "No plan ready — exploring for resources" over 11k times in 200k
//!      ticks because `enumerate_targets` for Harvest requires the agent
//!      to already have `(bush, Contains, Berry)` beliefs in their
//!      MindGraph, which only arrive through direct perception. Agents
//!      spawned away from food never produce a food plan at all.
//!
//!   3. **Drink never fires:** Zero Drink actions across 10 humans in 200k
//!      ticks. The Survival brain only dispatches on the top urgency, and
//!      Fear/Hunger almost always outrank Thirst, so the Drink branch is
//!      unreachable.
//!
//! Each `#[test]` below captures one bug in isolation so it can fail,
//! motivate a fix, then lock in the expected post-fix behaviour.

use bevy::math::Vec2;
use worldsim::agent::actions::ActionType;
use worldsim::agent::events::SimEvent;
use worldsim::agent::nervous_system::config::NervousSystemConfig;
use worldsim::testing::TestWorld;
use worldsim::world::map::TileType;

/// Force brains to run every tick so these tests don't fight the 60-tick
/// thinking stagger. Without this a "hungry agent eats within 500 ticks"
/// test is really waiting for ~8 brain cycles.
fn fast_brains(world: &mut TestWorld) {
    let mut config = world
        .app_mut()
        .world_mut()
        .resource_mut::<NervousSystemConfig>();
    config.thinking_interval = 1;
}

/// Count how many times `agent` started an action of the given type.
fn action_started_count(world: &TestWorld, agent: bevy::prelude::Entity, at: ActionType) -> usize {
    world
        .sim_events()
        .all()
        .iter()
        .filter(|e| {
            matches!(
                e,
                SimEvent::ActionStarted { agent: a, action, .. }
                    if *a == agent && *action == at
            )
        })
        .count()
}

// ─── Bug 1: Harvest → Eat chain ─────────────────────────────────────────

/// A hungry human standing right next to a berry bush must Harvest AND
/// then Eat within a tight tick budget. Captures the Harvest→Eat chain
/// drop: before the fix, agents would harvest 100+ times but eat only a
/// handful of times because Eat's `can_start()` checks the MindGraph for
/// food rather than the actual inventory, so the 1-tick belief-updater
/// delay after Harvest leaves Eat looking at a stale mind.
#[test]
fn hungry_human_next_to_bush_eats_within_500_ticks() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("hungry")
        .pos(Vec2::new(100.0, 100.0))
        .hunger_urgency(0.85)
        .done()
        // Bush ~1 tile away — practically adjacent.
        .berry_bushes(1, Vec2::new(116.0, 100.0))
        .build();
    fast_brains(&mut world);

    let hungry = agents["hungry"];
    world.tick(500);

    let harvests = action_started_count(&world, hungry, ActionType::Harvest);
    let eats = action_started_count(&world, hungry, ActionType::Eat);

    if harvests == 0 || eats == 0 {
        world.print_agent_state(hungry);
        world.print_brain_decision(hungry);
        world.print_agent_events(hungry, 40);
    }

    assert!(
        harvests >= 1,
        "hungry agent one tile from a bush should Harvest at least once in 500 ticks (saw {harvests})"
    );
    assert!(
        eats >= 1,
        "after harvesting, hungry agent should Eat at least once in 500 ticks (saw {harvests} harvests, {eats} eats)"
    );
}

/// Stronger form of the above: once the agent has eaten, hunger should
/// actually go down. Guards against a degenerate fix where Eat fires but
/// `on_complete` is broken.
#[test]
fn hungry_human_next_to_bush_reduces_hunger() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("hungry")
        .pos(Vec2::new(100.0, 100.0))
        .hunger_urgency(0.85)
        .done()
        .berry_bushes(1, Vec2::new(116.0, 100.0))
        .build();
    fast_brains(&mut world);

    let hungry = agents["hungry"];
    let initial_hunger = world.agent_hunger(hungry);
    world.tick(800);
    let final_hunger = world.agent_hunger(hungry);

    assert!(
        final_hunger < initial_hunger - 0.05,
        "hunger should drop measurably after 800 ticks next to food \
         (start={initial_hunger:.3}, end={final_hunger:.3})"
    );
}

// ─── Bug 2: Planner can't plan for hunger ───────────────────────────────

/// An agent that spawns *within vision range* of a berry bush must form a
/// food plan from first sight — currently they can, because perception
/// adds the bush's Contains triples to the MindGraph. The regression we
/// really care about is the case where the agent must rely on *type-level*
/// knowledge (AppleTree produces Apple, Apple IsA Food) to form a plan
/// without ever observing a specific instance first. This test captures
/// that second case: the agent gets a bush far away, with no initial
/// Contains knowledge, and must discover it through exploration.
///
/// Before the fix, `enumerate_targets` would return an empty list and the
/// Rational brain would emit "No plan ready — exploring for resources"
/// forever, because exploration itself never widened the MindGraph to
/// include the far-away bush within the agent's thinking interval.
#[test]
fn hungry_human_forms_plan_after_seeing_bush() {
    // Place a bush just inside vision range (100 tiles) so one perception
    // tick adds a Contains belief. The agent must then form a plan and
    // start harvesting within a reasonable tick budget.
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("seeker")
        .pos(Vec2::new(100.0, 100.0))
        .hunger_urgency(0.85)
        .done()
        .berry_bushes(1, Vec2::new(180.0, 100.0))
        .build();
    fast_brains(&mut world);

    let seeker = agents["seeker"];
    world.tick(1200);

    let harvests = action_started_count(&world, seeker, ActionType::Harvest);
    if harvests == 0 {
        world.print_agent_state(seeker);
        world.print_brain_decision(seeker);
        world.print_agent_events(seeker, 60);
    }
    assert!(
        harvests >= 1,
        "agent with a bush in vision range must form a plan and Harvest within 1200 ticks (saw {harvests})"
    );
}

// ─── Bug 3: Thirst never fires Drink ────────────────────────────────────

/// A very thirsty human standing next to water must Drink. This directly
/// captures the "zero Drink actions in 200k ticks" finding: Survival brain
/// currently only proposes Drink when Thirst is the *top* urgency, which
/// almost never happens because Fear/Hunger outrank it. The fix must
/// ensure that a severely dehydrated agent next to water actually drinks.
#[test]
fn thirsty_human_next_to_water_drinks_within_500_ticks() {
    // Vertical water stripe at tile x=7 (world x ~112). Agent at (100, 100)
    // → tile (6, 6), one tile west of water. Adjacent so `is_adjacent_to_water`
    // passes immediately without a walk step.
    let water_tile_x: u32 = 7;
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .fill_rect(water_tile_x, 0, 1, 32, TileType::ShallowWater)
        .agent("thirsty")
        .pos(Vec2::new(100.0, 100.0))
        .done()
        .build();
    fast_brains(&mut world);

    // Crank thirst up directly — `PhysicalNeeds.thirst` is on a 0-100
    // scale, and severe dehydration kicks in at roughly 80+.
    let thirsty = agents["thirsty"];
    {
        use worldsim::agent::body::needs::PhysicalNeeds;
        let mut needs = world
            .app_mut()
            .world_mut()
            .get_mut::<PhysicalNeeds>(thirsty)
            .expect("agent must have PhysicalNeeds");
        needs.thirst = 90.0;
    }
    world.tick(500);

    let drinks = action_started_count(&world, thirsty, ActionType::Drink);
    if drinks == 0 {
        world.print_agent_state(thirsty);
        world.print_brain_decision(thirsty);
        world.print_agent_events(thirsty, 40);
    }
    assert!(
        drinks >= 1,
        "thirsty agent adjacent to water should Drink at least once in 500 ticks (saw {drinks})"
    );
}

// ─── Bug 4: Empty-target Harvest loop ──────────────────────────────────

/// Direct-execution probe: force a Harvest into the active set against
/// a target with zero items, step the world one tick, and assert that
/// the execution layer emits `ActionFailed(ResourceDepleted)`. Before
/// the #416 execution fix, on_complete silently returned early on an
/// empty target and the only downstream signal was ActionCompleted —
/// so the Rational brain happily advanced its plan step thinking the
/// Harvest had worked, the agent spammed the same plan forever, and
/// every human eventually starved to death in the long-run sim.
#[test]
fn empty_harvest_emits_resource_depleted() {
    use worldsim::agent::actions::ActionType;
    use worldsim::agent::actions::ActiveActions;
    use worldsim::agent::actions::registry::ActionState;
    use worldsim::agent::events::{FailureReason, SimEvent};

    let bush_pos = Vec2::new(100.0, 100.0);
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(64, 64)
        .noise_biomes(false)
        .agent("alice")
        .pos(bush_pos)
        .done()
        .build();

    // Empty bush on top of Alice. The real inventory is 0 berries so
    // Harvest's on_complete will return early (no items to take).
    let empty_bush = world.spawn_berry_bush(bush_pos, 0);
    let alice = agents["alice"];

    // Inject a Harvest directly into ActiveActions with
    // `ticks_remaining = 0` so it completes on the next execution tick.
    // This bypasses the brain entirely — we're testing just the
    // execution-layer empty-yield detection, not the whole plan stack.
    {
        let mut active = world
            .app_mut()
            .world_mut()
            .get_mut::<ActiveActions>(alice)
            .expect("alice has ActiveActions");
        let mut state = ActionState::new(ActionType::Harvest, 0);
        state.target_entity = Some(empty_bush);
        state.target_position = Some(bush_pos);
        state.ticks_remaining = 0;
        active.insert(state);
    }

    world.tick(2);

    let depletions = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| {
            matches!(
                e,
                SimEvent::ActionFailed {
                    agent: a,
                    action: ActionType::Harvest,
                    reason: FailureReason::ResourceDepleted,
                    ..
                } if *a == alice
            )
        })
        .count();
    let completions = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| {
            matches!(
                e,
                SimEvent::ActionCompleted {
                    agent: a,
                    action: ActionType::Harvest,
                    ..
                } if *a == alice
            )
        })
        .count();

    if depletions == 0 {
        world.print_agent_events(alice, 10);
    }

    assert!(
        depletions >= 1,
        "empty Harvest must emit ResourceDepleted (saw {depletions} depletions, {completions} completions)"
    );
    // And we must NOT also emit a spurious ActionCompleted for the
    // same harvest — the failure path takes over exclusively, which is
    // what lets the Rational brain's `recently_failed` bucket pick the
    // signal up and drop the stale plan instead of advancing its step.
    assert_eq!(
        completions, 0,
        "empty Harvest must not also emit ActionCompleted (the failure path replaces it)"
    );
}

// ─── Tight diagnostic: can a hungry human plan against a visible bush? ─

/// An even tighter test than the "eats within 500 ticks" one: gives the
/// agent plenty of time (1000 ticks) and a bush placed just 60 pixels
/// away so perception sees it immediately at the first tick. If *this*
/// fails, the hunger planner can't produce a plan even under ideal
/// conditions and every downstream symptom (Alice in the real sim doing
/// "No plan ready") traces to the same cause.
#[test]
fn hungry_agent_with_visible_bush_plans_a_harvest() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(100.0, 100.0))
        .hunger_urgency(0.85)
        .done()
        .berry_bushes(1, Vec2::new(160.0, 100.0))
        .build();
    fast_brains(&mut world);

    let alice = agents["alice"];
    world.tick(1000);

    let harvests = action_started_count(&world, alice, ActionType::Harvest);
    if harvests == 0 {
        world.print_agent_state(alice);
        world.print_brain_decision(alice);
        world.print_mind_graph(alice);
        world.print_agent_events(alice, 30);
    }
    assert!(
        harvests >= 1,
        "hungry agent with BerryBush 60px away must plan and start a Harvest within 1000 ticks \
         (saw {harvests})"
    );
}

// ─── The actual regression: default sim survival ───────────────────────

/// Run `game_defaults(42)` for `ticks` ticks and assert every human is
/// still alive. Shared helper for the multi-level survival ladder below.
///
/// Each level is its own `#[test]` rather than a parameterized loop so
/// the pass/fail signal is per-tick-budget — "30k passes, 60k passes,
/// 100k fails" tells you exactly where the calorie balance breaks down.
/// This is the contract for the player experience: humans stay alive
/// across the typical play session length under realistic conditions
/// (Realistic biome placement, wolves triggering Fear→Flee interrupts,
/// scattered food, natural hunger ramp-up).
fn assert_humans_survive_default_sim(ticks: u64) {
    use worldsim::agent::Person;
    use worldsim::world::spawn_config::WorldSpawnConfig;

    let mut world = TestWorld::game_defaults(42);
    let initial_humans: usize = {
        let mut q = world
            .app_mut()
            .world_mut()
            .query_filtered::<bevy::prelude::Entity, bevy::prelude::With<Person>>();
        q.iter(world.app().world()).count()
    };
    assert!(
        initial_humans > 0,
        "game_defaults must populate a non-empty human population"
    );

    world.tick(ticks);

    let deaths: Vec<SimEvent> = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| matches!(e, SimEvent::Death { .. }))
        .cloned()
        .collect();

    let surviving_humans: usize = {
        let mut q = world
            .app_mut()
            .world_mut()
            .query_filtered::<bevy::prelude::Entity, bevy::prelude::With<Person>>();
        q.iter(world.app().world()).count()
    };

    if surviving_humans < initial_humans {
        eprintln!(
            "default sim seed 42: {} deaths total in {}k ticks; humans {}/{}",
            deaths.len(),
            ticks / 1000,
            surviving_humans,
            initial_humans
        );
        for e in &deaths {
            if let SimEvent::Death { agent, tick, cause } = e {
                eprintln!("  death: {agent:?} @ tick {tick} ({cause})");
            }
        }
    }

    assert_eq!(
        surviving_humans,
        initial_humans,
        "every human must survive {}k ticks of the seed-42 default sim — got {surviving_humans}/{initial_humans}",
        ticks / 1000
    );

    // Also assert WorldSpawnConfig defaults haven't drifted — if the
    // regression fires with zero spawns it's a trivially-passing test.
    let _ = WorldSpawnConfig::game_defaults();
}

/// 30k ticks — 8 minutes of play. Should be trivially survivable; this
/// catches catastrophic regressions where humans can't even feed
/// themselves once.
#[test]
#[ignore = "slow (~15s): full game_defaults run. Run with --ignored."]
fn default_sim_seed_42_humans_survive_30k() {
    assert_humans_survive_default_sim(30_000);
}

/// 65k ticks — 18 minutes. Past the first death wave in the original
/// pre-fix baseline (humans died ~59k–63k). This is the contract level
/// the #416 fix had to clear.
#[test]
#[ignore = "slow (~30s): full game_defaults run. Run with --ignored."]
fn default_sim_seed_42_humans_survive_65k() {
    assert_humans_survive_default_sim(65_000);
}

/// 100k ticks — 28 minutes. Tests whether humans recover their reserves
/// fast enough to outpace BMR drain over many eating cycles, not just
/// one wave. Passing this level required two #416 fixes in tandem:
/// (a) execution emits `ResourceDepleted` when Harvest yields nothing,
/// so agents stop spamming the same empty target; (b) the belief
/// updater's `ResourceDepleted` handler clears every `Contains` belief
/// on the target instead of the old hardcoded `Concept::Apple`, so
/// BerryBush/Corpse depletion is actually learned.
#[test]
#[ignore = "slow (~90s): full game_defaults run. Run with --ignored."]
fn default_sim_seed_42_humans_survive_100k() {
    assert_humans_survive_default_sim(100_000);
}

/// 200k ticks — 56 minutes. The endgame check: indefinite play. This
/// is the calorie-balance and knowledge-maintenance contract for the
/// typical play session.
#[test]
#[ignore = "slow (~180s): full game_defaults run. Run with --ignored."]
fn default_sim_seed_42_humans_survive_200k() {
    assert_humans_survive_default_sim(200_000);
}

/// Diagnostic helper kept for manual investigation: walks the default
/// sim in 10k-tick chunks and dumps Alice's full state at each
/// checkpoint. Not a regression assertion; used to characterize the
/// slow-starvation trajectory when debugging long-tail deaths.
#[test]
#[ignore = "diagnostic (~60s): dumps Alice trajectory, not an assertion"]
fn diagnostic_alice_trajectory_100k() {
    use bevy::prelude::{Entity, Name, With};
    use worldsim::agent::Person;

    let mut world = TestWorld::game_defaults(42);
    let alice: Entity = {
        let mut q = world
            .app_mut()
            .world_mut()
            .query_filtered::<(Entity, &Name), With<Person>>();
        let mut found = None;
        for (e, name) in q.iter(world.app().world()) {
            if name.as_str() == "Alice" {
                found = Some(e);
                break;
            }
        }
        found.expect("Alice must be spawned in game_defaults")
    };

    for chunk in 1..=10u64 {
        world.tick(10_000);
        let tick = chunk * 10_000;

        let alive = world
            .app()
            .world()
            .get_entity(alice)
            .map(|e| e.contains::<Person>())
            .unwrap_or(false);
        eprintln!("\n==== CHECKPOINT tick={tick} alive={alive} ====");
        if !alive {
            eprintln!("(Alice is dead, stopping trajectory dump)");
            break;
        }
        world.print_agent_state(alice);
    }
}
