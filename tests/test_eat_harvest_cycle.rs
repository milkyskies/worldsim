//! #598: Eat must not complete without consuming food (the "dead zone" bug).
//!
//! Before the bite-aware satiation gate, `stomach ∈ [60, 80]` was a silent
//! dead zone: the 0.80 fraction gate still said "go ahead" but
//! `metabolism.eat` refused a 40-mass berry into a < 40 mass headroom. The
//! Eat action completed anyway (granted stamina, fired `ActionCompleted`),
//! but the guard `if metabolism.eat(macros) { inventory.remove(..) }` kept
//! the berry in the pouch. Next tick: still hungry, still has food,
//! stomach still < 0.80, brain re-proposes Eat, loop repeats forever.
//!
//! With the bite-aware gate, `Eat::satiation` reports fullness = 1.0 when
//! the next berry wouldn't fit, so both the survival brain and the
//! execution-layer gate refuse the action until digestion drops stomach
//! below 60 and the next bite fits again.

use bevy::math::Vec2;
use worldsim::agent::actions::ActionType;
use worldsim::agent::body::metabolism::Metabolism;
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::events::SimEvent;
use worldsim::agent::item_slots::ItemSlots;
use worldsim::agent::mind::knowledge::Concept;
use worldsim::testing::TestWorld;

/// Regression for #598. With stomach parked at 70/100 (dead zone for a
/// 40-mass berry), the old code would "complete" Eat every ~20 ticks
/// without removing any berry from inventory. The invariant we want is:
/// every `ActionCompleted { action: Eat }` corresponds to exactly one
/// berry leaving inventory.
#[test]
fn eat_completions_match_berries_consumed_in_dead_zone() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(50.0, 50.0))
        .hunger_urgency(0.9)
        .done()
        .build();
    let alice = agents["alice"];

    // Force alice into the dead zone: stomach 70/100, glucose/reserves
    // low so hunger_urgency stays high and the brain keeps proposing Eat.
    {
        let mut needs = world.get_mut::<PhysicalNeeds>(alice);
        needs.metabolism = Metabolism {
            stomach_carbs: 42.0,
            stomach_fat: 28.0,
            glucose: 5.0,
            reserves: 10.0,
        };
    }
    // 10 berries — plenty for a buggy loop to chew through phantom-wise.
    {
        let mut inv = world.get_mut::<ItemSlots>(alice);
        inv.add(Concept::Berry, 10);
    }

    let start_berries = world.get::<ItemSlots>(alice).count(Concept::Berry);
    assert_eq!(start_berries, 10);

    // 300 ticks = 15 Eat durations. Before the fix this produced ~15
    // phantom completions. Long enough that digestion also starts lowering
    // stomach, which legitimately unlocks the next bite — the assertion
    // accommodates that by pairing completions to consumptions, not
    // requiring zero completions.
    world.tick(300);

    let eat_completions = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| {
            matches!(
                e,
                SimEvent::ActionCompleted {
                    agent,
                    action: ActionType::Eat,
                    ..
                } if *agent == alice
            )
        })
        .count() as u32;

    let end_berries = world.get::<ItemSlots>(alice).count(Concept::Berry);
    let consumed = start_berries - end_berries;

    assert_eq!(
        eat_completions, consumed,
        "every Eat completion must consume one berry; got {eat_completions} \
         completions and {consumed} consumed (start={start_berries}, end={end_berries})"
    );
}

/// End-to-end harvest → eat cycle. A hungry agent standing on a berry
/// bush should harvest, eat, and end up with more food in her stomach
/// than she started with, while the bush's inventory drops.
#[test]
fn harvest_eat_cycle_drains_bush_and_raises_stomach() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(50.0, 50.0))
        .hunger_urgency(0.9)
        .done()
        .build();
    let alice = agents["alice"];
    world.spawn_berry_bush(Vec2::new(52.0, 50.0), 10);

    world.enable_fast_forward();

    let start_stomach = world
        .get::<PhysicalNeeds>(alice)
        .metabolism
        .stomach_fullness();

    // Enough for: walk to bush, multiple harvest actions, eat chain,
    // digestion cycle, and a second meal after the first dead-zone
    // interval would have elapsed.
    world.tick(3000);

    let end_stomach = world
        .get::<PhysicalNeeds>(alice)
        .metabolism
        .stomach_fullness();

    let harvested_from_bush = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| {
            matches!(
                e,
                SimEvent::ActionCompleted {
                    agent,
                    action: ActionType::Harvest,
                    ..
                } if *agent == alice
            )
        })
        .count();
    let ate_food = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| {
            matches!(
                e,
                SimEvent::ActionCompleted {
                    agent,
                    action: ActionType::Eat,
                    ..
                } if *agent == alice
            )
        })
        .count();

    assert!(
        harvested_from_bush > 0,
        "alice should have completed at least one Harvest against the bush"
    );
    assert!(ate_food > 0, "alice should have completed at least one Eat");
    assert!(
        end_stomach > start_stomach,
        "alice's stomach must rise after eating (start={start_stomach:.1}, end={end_stomach:.1})"
    );
}
