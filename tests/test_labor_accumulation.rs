//! Integration tests for labor accumulation (#218).
//!
//! Validates the `LaborAccumulated` trigger, `labor_accumulation_system`, and
//! the `Construct` action's interaction with the `Becomes` substrate.

use bevy::prelude::*;
use worldsim::agent::actions::ActionType;
use worldsim::agent::actions::registry::{ActionState, ActiveActions};
use worldsim::agent::item_slots::{ItemSlots, Slot};
use worldsim::agent::mind::knowledge::Concept;
use worldsim::world::becomes::{
    Becomes, BecomesTrigger, becomes_system, labor_accumulation_system,
};
use worldsim::world::construction_site::spawn_construction_site_headless;

// ═══════════════════════════════════════════════════════════════════════════
// TEST APP HELPERS
// ═══════════════════════════════════════════════════════════════════════════

use worldsim::agent::events::SimEvent;
use worldsim::core::tick::TickCount;

fn labor_test_app() -> App {
    let mut app = App::new();
    let mut tick = TickCount::new(1.0);
    tick.current = 0;
    app.insert_resource(tick);
    app.add_event::<SimEvent>();
    // Labor must run before becomes so a threshold crossing fires in the same tick.
    app.add_systems(
        Update,
        (
            labor_accumulation_system,
            becomes_system.after(labor_accumulation_system),
        ),
    );
    app
}

/// Spawn a minimal construction site entity with a pure `LaborAccumulated` trigger.
fn spawn_labor_site(app: &mut App, required: u32) -> Entity {
    app.world_mut()
        .spawn((
            Name::new("LaborSite"),
            worldsim::world::Physical,
            Transform::from_xyz(0.0, 0.0, 1.0),
            GlobalTransform::default(),
            Becomes::new(
                Concept::Campfire,
                BecomesTrigger::LaborAccumulated {
                    required,
                    current: 0,
                },
                0,
            ),
        ))
        .id()
}

/// Spawn an agent entity with a `Construct` action targeting `site`.
fn spawn_constructor(app: &mut App, site: Entity) -> Entity {
    let mut active = ActiveActions::empty();
    active.insert(ActionState::new(ActionType::Construct, 0).with_target_entity(site));
    app.world_mut().spawn(active).id()
}

/// Read `LaborAccumulated.current` from a `Becomes` component, panicking if
/// the trigger tree contains no `LaborAccumulated` node.
fn labor_current(app: &App, site: Entity) -> u32 {
    let becomes = app
        .world()
        .entity(site)
        .get::<Becomes>()
        .expect("Site must have Becomes component");
    becomes
        .trigger
        .labor_current()
        .expect("Becomes trigger tree must contain LaborAccumulated")
}

// ═══════════════════════════════════════════════════════════════════════════
// TEST: SINGLE CONSTRUCTOR INCREMENTS BY 1 PER TICK
// ═══════════════════════════════════════════════════════════════════════════

/// A site with one active constructor must gain exactly 1 labor tick per
/// simulation tick.
#[test]
fn labor_accumulator_increments_per_active_constructor() {
    let mut app = labor_test_app();
    let site = spawn_labor_site(&mut app, 10);
    spawn_constructor(&mut app, site);

    app.update();
    assert_eq!(
        labor_current(&app, site),
        1,
        "Expected 1 labor after 1 tick"
    );

    app.update();
    assert_eq!(
        labor_current(&app, site),
        2,
        "Expected 2 labor after 2 ticks"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// TEST: MULTIPLE CONSTRUCTORS ADD LINEARLY
// ═══════════════════════════════════════════════════════════════════════════

/// Two agents both running Construct on the same site contribute 2 labor per
/// tick — linear scaling, no cap.
#[test]
fn multi_agent_labor_adds_linearly() {
    let mut app = labor_test_app();
    let site = spawn_labor_site(&mut app, 100);
    spawn_constructor(&mut app, site);
    spawn_constructor(&mut app, site);

    app.update();
    assert_eq!(
        labor_current(&app, site),
        2,
        "Two constructors must contribute 2 labor per tick"
    );

    app.update();
    assert_eq!(labor_current(&app, site), 4, "After 2 ticks: 2×2 = 4 labor");
}

// ═══════════════════════════════════════════════════════════════════════════
// TEST: WALKING AWAY PAUSES COUNTER; COUNTER PERSISTS ON RESUME
// ═══════════════════════════════════════════════════════════════════════════

/// When the agent stops Constructing (walks away), the labor counter freezes.
/// When they return and resume, it picks up from where it left off.
#[test]
fn walking_away_pauses_labor_state_persists() {
    let mut app = labor_test_app();
    let site = spawn_labor_site(&mut app, 100);
    let agent = spawn_constructor(&mut app, site);

    // Tick 1: agent constructs → labor = 1
    app.update();
    assert_eq!(labor_current(&app, site), 1);

    // Agent walks away — clear Construct from active actions.
    {
        let mut active = app.world_mut().get_mut::<ActiveActions>(agent).unwrap();
        active.remove(ActionType::Construct);
    }

    // Tick 2 and 3: no constructor → labor stays at 1
    app.update();
    assert_eq!(
        labor_current(&app, site),
        1,
        "Counter must not advance while idle"
    );
    app.update();
    assert_eq!(labor_current(&app, site), 1, "Counter must stay frozen");

    // Agent returns — re-add Construct to active actions.
    {
        let mut active = app.world_mut().get_mut::<ActiveActions>(agent).unwrap();
        active.insert(ActionState::new(ActionType::Construct, 0).with_target_entity(site));
    }

    // Tick 4: agent constructs again → labor = 2
    app.update();
    assert_eq!(
        labor_current(&app, site),
        2,
        "Counter must resume from saved state"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// TEST: COMPOSITE ALL REQUIRES BOTH SLOTS AND LABOR
// ═══════════════════════════════════════════════════════════════════════════

/// A site with `All([SlotsFilled, LaborAccumulated])` must NOT transform when
/// only materials are present and must NOT transform when only labor is done.
/// It MUST transform only when both conditions are met simultaneously.
#[test]
fn composite_all_requires_both_slots_and_labor() {
    let mut app = labor_test_app();

    // Site with composite All trigger: needs 1 wood AND 3 labor ticks.
    let site = app
        .world_mut()
        .spawn((
            Name::new("CompositeSite"),
            worldsim::world::Physical,
            Transform::from_xyz(0.0, 0.0, 1.0),
            GlobalTransform::default(),
            ItemSlots {
                slots: vec![Slot::construction(Concept::Wood, 1)],
            },
            Becomes::new(
                Concept::Campfire,
                BecomesTrigger::All(vec![
                    BecomesTrigger::SlotsFilled,
                    BecomesTrigger::LaborAccumulated {
                        required: 3,
                        current: 0,
                    },
                ]),
                0,
            ),
        ))
        .id();

    // Fill the material slot immediately.
    {
        let mut entity_mut = app.world_mut().entity_mut(site);
        let mut slots = entity_mut.get_mut::<ItemSlots>().unwrap();
        assert!(slots.deposit(Concept::Wood, 1, None));
    }

    // Tick 1: slots filled, labor = 0 → must NOT transform yet.
    spawn_constructor(&mut app, site);
    // Don't advance tick yet — just check that slots alone don't trigger.
    // Remove the constructor so labor doesn't pile up unintentionally:
    // Actually, we want to verify slots-only. Spawn a *separate* app for
    // that case and keep this one for the full progression.
    app.update();
    // After tick 1: slots filled, labor = 1 → still < 3, no transform.
    assert!(
        app.world().get_entity(site).is_ok(),
        "Site must NOT transform when labor < required (1 < 3)"
    );
    assert_eq!(labor_current(&app, site), 1);

    // Tick 2: labor = 2 → still not enough.
    app.update();
    assert!(
        app.world().get_entity(site).is_ok(),
        "Site must NOT transform when labor < required (2 < 3)"
    );

    // Tick 3: labor = 3 → both conditions met → must transform.
    app.update();
    assert!(
        app.world().get_entity(site).is_err(),
        "Site must transform once All([SlotsFilled, LaborAccumulated(3)]) fires"
    );
}

/// Verify that labor-only (without slots filled) does NOT trigger the composite.
#[test]
fn composite_all_labor_alone_does_not_transform() {
    let mut app = labor_test_app();

    let site = app
        .world_mut()
        .spawn((
            Name::new("LaborOnlySite"),
            worldsim::world::Physical,
            Transform::from_xyz(0.0, 0.0, 1.0),
            GlobalTransform::default(),
            ItemSlots {
                slots: vec![Slot::construction(Concept::Wood, 1)],
            },
            Becomes::new(
                Concept::Campfire,
                BecomesTrigger::All(vec![
                    BecomesTrigger::SlotsFilled,
                    BecomesTrigger::LaborAccumulated {
                        required: 1,
                        current: 0,
                    },
                ]),
                0,
            ),
        ))
        .id();

    // Add a constructor but leave slots EMPTY.
    spawn_constructor(&mut app, site);

    // Tick: labor accumulates to 1, but slots are empty → no transform.
    app.update();
    assert!(
        app.world().get_entity(site).is_ok(),
        "Site must NOT transform on labor-only when slots are not filled"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// TEST: IDLE AGENTS NEAR SITE DO NOT INCREMENT LABOR
// ═══════════════════════════════════════════════════════════════════════════

/// Agents that are physically near the site but are NOT running a Construct
/// action must not contribute any labor.
#[test]
fn labor_does_not_increment_for_proximate_idle_agents() {
    let mut app = labor_test_app();
    let site = spawn_labor_site(&mut app, 10);

    // Spawn an agent at the same position but with no Construct action (Idle).
    let mut idle_actions = ActiveActions::empty();
    idle_actions.insert(ActionState::new(ActionType::Idle, 0));
    app.world_mut()
        .spawn((idle_actions, Transform::from_xyz(0.0, 0.0, 0.0)));

    app.update();
    assert_eq!(
        labor_current(&app, site),
        0,
        "Idle agents must NOT contribute labor even when co-located with the site"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// TEST: LABOR IS CREDITED ONLY TO THE TARGETED SITE
// ═══════════════════════════════════════════════════════════════════════════

/// An agent constructing site X must not increment the labor counter on site Y.
#[test]
fn labor_only_counts_toward_the_targeted_site() {
    let mut app = labor_test_app();
    let site_x = spawn_labor_site(&mut app, 10);
    let site_y = spawn_labor_site(&mut app, 10);

    // Agent targets site_x only.
    spawn_constructor(&mut app, site_x);

    app.update();

    assert_eq!(labor_current(&app, site_x), 1, "Site X must get the labor");
    assert_eq!(
        labor_current(&app, site_y),
        0,
        "Site Y must be unaffected when not targeted"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// TEST: SPAWN HELPER CREATES LABOR SITE CORRECTLY
// ═══════════════════════════════════════════════════════════════════════════

/// `spawn_construction_site_headless` with `labor_required: Some(n)` must
/// produce a `BecomesTrigger::All([SlotsFilled, LaborAccumulated])` site.
#[test]
fn spawn_helper_creates_labor_site_with_correct_trigger() {
    let mut app = labor_test_app();

    let site = {
        let mut commands_queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut commands_queue, app.world());
        let id = spawn_construction_site_headless(
            &mut commands,
            Concept::Campfire,
            Vec2::ZERO,
            &[(Concept::Wood, 3)],
            &[],
            Some(5),
            0,
        );
        commands_queue.apply(app.world_mut());
        id
    };

    let becomes = app
        .world()
        .entity(site)
        .get::<Becomes>()
        .expect("Site must have Becomes");

    assert!(
        becomes.trigger.has_labor_accumulated(),
        "Labor site trigger tree must contain LaborAccumulated"
    );
    assert_eq!(
        becomes.trigger.labor_current().unwrap(),
        0,
        "Labor must start at 0"
    );
}

/// `spawn_construction_site_headless` with `labor_required: None` must produce
/// a `BecomesTrigger::SlotsFilled` site (backward-compatible behaviour).
#[test]
fn spawn_helper_without_labor_uses_slots_filled_trigger() {
    let mut app = labor_test_app();

    let site = {
        let mut commands_queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut commands_queue, app.world());
        let id = spawn_construction_site_headless(
            &mut commands,
            Concept::Campfire,
            Vec2::ZERO,
            &[(Concept::Wood, 3)],
            &[],
            None,
            0,
        );
        commands_queue.apply(app.world_mut());
        id
    };

    let becomes = app
        .world()
        .entity(site)
        .get::<Becomes>()
        .expect("Site must have Becomes");

    assert!(
        !becomes.trigger.has_labor_accumulated(),
        "Non-labor site must not contain LaborAccumulated trigger"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// TEST: LABOR SITE WITH FULL MATERIALS NEEDS LABOR BEFORE TRANSFORMING
// ═══════════════════════════════════════════════════════════════════════════

/// Even if a labor site's material slots are fully stocked, it must not
/// transform until the required labor has been accumulated.
#[test]
fn fully_stocked_labor_site_needs_labor_before_transforming() {
    use worldsim::core::tick::TickCount;
    let mut app = labor_test_app();

    let site = {
        let mut commands_queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut commands_queue, app.world());
        let id = spawn_construction_site_headless(
            &mut commands,
            Concept::Campfire,
            Vec2::ZERO,
            &[(Concept::Wood, 3)],
            &[(Concept::Wood, 3)], // fully stocked
            Some(2),               // requires 2 labor ticks
            0,
        );
        commands_queue.apply(app.world_mut());
        id
    };

    // Tick 1: slots filled but labor = 0 → no transform (no constructor).
    app.update();
    assert!(
        app.world().get_entity(site).is_ok(),
        "Fully-stocked labor site must NOT transform without labor"
    );

    // Add a constructor and run 2 more ticks.
    spawn_constructor(&mut app, site);

    app.world_mut().resource_mut::<TickCount>().current += 1;
    app.update();
    // labor = 1 → still not enough
    assert!(
        app.world().get_entity(site).is_ok(),
        "1 labor tick not enough"
    );

    app.world_mut().resource_mut::<TickCount>().current += 1;
    app.update();
    // labor = 2 → both conditions met → transforms
    assert!(
        app.world().get_entity(site).is_err(),
        "Site must transform once both slots and labor requirements are met"
    );
}
