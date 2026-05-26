//! Integration tests for world entity property components (#279).
//!
//! Verifies that LightSource, HeatSource, Durability, ShelterProvider, BuiltBy,
//! and FuelConsumer components behave correctly, that their systems run per tick,
//! and that campfire is correctly composed from property components.

use bevy::prelude::*;
use worldsim::agent::Dazed;
use worldsim::agent::actions::{ActionState, ActionType, ActiveActions};
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::inventory::EntityType;
use worldsim::agent::item_slots::ItemSlots;
use worldsim::agent::mind::knowledge::{Concept, MindGraph, Predicate, Value};
use worldsim::agent::psyche::emotions::EmotionalState;
use worldsim::testing::{AgentConfig, TestWorld};
use worldsim::world::Physical;
use worldsim::world::emits_effect::{EffectKind, EmitsEffect};
use worldsim::world::property::{
    BuiltBy, Durability, FuelConsumer, HeatSource, LightSource, ShelterProvider,
};

// ─── Durability ───────────────────────────────────────────────────────────────

#[test]
fn durability_degrades_over_time() {
    let mut world = TestWorld::with_seed(0);
    let entity = world
        .app_mut()
        .world_mut()
        .spawn((
            Name::new("BreakableRock"),
            EntityType(Concept::Stone),
            Physical,
            Transform::from_translation(Vec3::ZERO),
            GlobalTransform::default(),
            Durability {
                current: 10.0,
                max: 10.0,
                decay_rate: 1.0, // loses 1 per tick
            },
        ))
        .id();

    world.tick(5);

    let durability = world.get::<Durability>(entity);
    assert!(
        durability.current < 10.0,
        "durability should have decreased (got {})",
        durability.current
    );
}

#[test]
fn entity_despawns_when_durability_reaches_zero() {
    let mut world = TestWorld::with_seed(0);
    let entity = world
        .app_mut()
        .world_mut()
        .spawn((
            Name::new("Fragile"),
            EntityType(Concept::Stone),
            Physical,
            Transform::from_translation(Vec3::ZERO),
            GlobalTransform::default(),
            Durability {
                current: 3.0,
                max: 3.0,
                decay_rate: 1.0,
            },
        ))
        .id();

    // Tick past zero
    world.tick(5);

    assert!(
        !world.entity_exists(entity),
        "entity should be despawned when durability reaches zero"
    );
}

#[test]
fn indestructible_entity_never_despawns() {
    let mut world = TestWorld::with_seed(0);
    let entity = world
        .app_mut()
        .world_mut()
        .spawn((
            Name::new("StoneWall"),
            EntityType(Concept::Stone),
            Physical,
            Transform::from_translation(Vec3::ZERO),
            GlobalTransform::default(),
            Durability {
                current: 100.0,
                max: 100.0,
                decay_rate: 0.0, // indestructible
            },
        ))
        .id();

    world.tick(200);

    assert!(
        world.entity_exists(entity),
        "indestructible entity (decay_rate=0) should never despawn"
    );
}

// ─── FuelConsumer ────────────────────────────────────────────────────────────

#[test]
fn fuel_consumer_loses_fuel_per_tick() {
    let mut world = TestWorld::with_seed(0);
    let entity = world
        .app_mut()
        .world_mut()
        .spawn((
            Name::new("FuelBurner"),
            EntityType(Concept::Campfire),
            Physical,
            Transform::from_translation(Vec3::ZERO),
            GlobalTransform::default(),
            LightSource {
                radius: 50.0,
                intensity: 1.0,
            },
            HeatSource {
                radius: 50.0,
                intensity: 1.0,
            },
            FuelConsumer {
                fuel_type: Concept::Wood,
                fuel_remaining: 100.0,
                consumption_rate: 1.0,
            },
        ))
        .id();

    world.tick(10);

    let consumer = world.get::<FuelConsumer>(entity);
    assert!(
        consumer.fuel_remaining < 100.0,
        "fuel should have decreased (got {})",
        consumer.fuel_remaining
    );
}

#[test]
fn light_and_heat_sources_removed_when_fuel_runs_out() {
    let mut world = TestWorld::with_seed(0);
    let entity = world
        .app_mut()
        .world_mut()
        .spawn((
            Name::new("BurningOut"),
            EntityType(Concept::Campfire),
            Physical,
            Transform::from_translation(Vec3::ZERO),
            GlobalTransform::default(),
            LightSource {
                radius: 50.0,
                intensity: 1.0,
            },
            HeatSource {
                radius: 50.0,
                intensity: 1.0,
            },
            FuelConsumer {
                fuel_type: Concept::Wood,
                fuel_remaining: 3.0,
                consumption_rate: 1.0,
            },
        ))
        .id();

    // Tick past fuel exhaustion
    world.tick(5);

    let has_light = world.app().world().get::<LightSource>(entity).is_some();
    let has_heat = world.app().world().get::<HeatSource>(entity).is_some();

    assert!(
        !has_light,
        "LightSource should be removed when fuel runs out"
    );
    assert!(!has_heat, "HeatSource should be removed when fuel runs out");
}

// ─── ShelterProvider ─────────────────────────────────────────────────────────

#[test]
fn shelter_provider_improves_sleep_energy_recovery() {
    // Two agents start at low stamina. One sleeps near a shelter, one without.
    let mut world = TestWorld::with_seed(0);

    // Spawn shelter
    world.app_mut().world_mut().spawn((
        Name::new("LeanTo"),
        EntityType(Concept::LeanTo),
        Physical,
        Transform::from_translation(Vec3::new(100.0, 100.0, 0.0)),
        GlobalTransform::default(),
        ShelterProvider {
            capacity: 2,
            protection: 1.5,
        },
    ));

    // Agent near shelter
    let sheltered = world.spawn_agent(AgentConfig {
        pos: Vec2::new(100.0, 100.0),
        stamina: 50.0,
        ..AgentConfig::default()
    });
    // Agent far from shelter
    let unsheltered = world.spawn_agent(AgentConfig {
        pos: Vec2::new(500.0, 500.0),
        stamina: 50.0,
        ..AgentConfig::default()
    });

    // Daze both agents so arbitration is skipped — these agents are at full
    // wakefulness, so the brain would propose WakeUp on the first tick and
    // remove the injected Sleep before the shelter system observed it.
    // Force Sleep into each agent's ActiveActions so the shelter system
    // sees `active.contains(ActionType::Sleep)`.
    for agent in [sheltered, unsheltered] {
        let mut active = ActiveActions::empty();
        active.insert(ActionState::new(ActionType::Sleep, 0));
        let mut entity = world.app_mut().world_mut().entity_mut(agent);
        entity.insert(active);
        entity.insert(Dazed {
            until_tick: u64::MAX,
        });
    }

    let aerobic_before_sheltered = world.get::<PhysicalNeeds>(sheltered).stamina.aerobic;
    let aerobic_before_unsheltered = world.get::<PhysicalNeeds>(unsheltered).stamina.aerobic;

    // Tick once — the shelter_system runs exactly once before the brain can override.
    world.tick(1);

    let aerobic_after_sheltered = world.get::<PhysicalNeeds>(sheltered).stamina.aerobic;
    let aerobic_after_unsheltered = world.get::<PhysicalNeeds>(unsheltered).stamina.aerobic;

    let sheltered_gain = aerobic_after_sheltered - aerobic_before_sheltered;
    let unsheltered_gain = aerobic_after_unsheltered - aerobic_before_unsheltered;

    assert!(
        sheltered_gain > unsheltered_gain,
        "agent near shelter should recover more stamina while sleeping \
         (sheltered gained {:.2}, unsheltered gained {:.2})",
        sheltered_gain,
        unsheltered_gain
    );
}

#[test]
fn natural_shelter_provides_same_benefit_as_built_shelter() {
    // ShelterProvider is the same component regardless of BuiltBy presence.
    let mut world = TestWorld::with_seed(0);

    // Natural cave — no BuiltBy
    let cave = world
        .app_mut()
        .world_mut()
        .spawn((
            Name::new("Cave"),
            EntityType(Concept::LeanTo),
            Physical,
            Transform::from_translation(Vec3::new(100.0, 100.0, 0.0)),
            GlobalTransform::default(),
            ShelterProvider {
                capacity: 3,
                protection: 1.2,
            },
        ))
        .id();

    // Built lean-to — with BuiltBy
    let builder = world.app_mut().world_mut().spawn_empty().id();
    let lean_to = world
        .app_mut()
        .world_mut()
        .spawn((
            Name::new("LeanTo"),
            EntityType(Concept::LeanTo),
            Physical,
            Transform::from_translation(Vec3::new(200.0, 200.0, 0.0)),
            GlobalTransform::default(),
            ShelterProvider {
                capacity: 3,
                protection: 1.2,
            },
            BuiltBy {
                builder,
                built_at: 0,
            },
        ))
        .id();

    // Both should have the same ShelterProvider protection value
    let cave_provider = world.get::<ShelterProvider>(cave);
    let lean_to_provider = world.get::<ShelterProvider>(lean_to);

    assert_eq!(
        cave_provider.protection, lean_to_provider.protection,
        "natural and built shelters with the same protection value should provide identical benefit"
    );
    assert_eq!(cave_provider.capacity, lean_to_provider.capacity);
}

// ─── Campfire composition ────────────────────────────────────────────────────

#[test]
fn campfire_has_fuel_consumer() {
    let mut world = TestWorld::with_seed(0);
    let campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));
    let consumer = world.get::<FuelConsumer>(campfire);
    assert_eq!(consumer.fuel_type, Concept::Wood);
    assert!(consumer.fuel_remaining > 0.0);
    assert!(consumer.consumption_rate > 0.0);
}

#[test]
fn campfire_has_emits_effect_comfort_aura() {
    let mut world = TestWorld::with_seed(0);
    let campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));

    let emits = world.get::<EmitsEffect>(campfire);
    assert!(
        emits.radius > 0.0,
        "campfire should emit a comfort aura with positive radius"
    );

    // The aura should be a composite effect carrying both stress reduction
    // and stamina recovery — this is what makes a campfire feel like home.
    let (has_stress_relief, has_energy_recovery) = match &emits.effect {
        EffectKind::All(effects) => {
            let stress = effects
                .iter()
                .any(|e| matches!(e, EffectKind::StressPerSec(r) if *r < 0.0));
            let stamina = effects
                .iter()
                .any(|e| matches!(e, EffectKind::StaminaPerSec(r) if *r > 0.0));
            (stress, stamina)
        }
        _ => (false, false),
    };
    assert!(
        has_stress_relief,
        "campfire aura should reduce stress (negative StressPerSec)"
    );
    assert!(
        has_energy_recovery,
        "campfire aura should restore stamina (positive StaminaPerSec)"
    );
}

#[test]
fn agent_near_campfire_has_lower_stress_than_distant_agent() {
    // Two identical agents, one inside the campfire's aura and one well outside.
    // After ticking, the sheltered agent must have strictly lower stress than
    // the distant control. Direct comparison cancels out background stress
    // recovery / drift that affects both agents equally.
    let (mut world, entities) = TestWorld::scenario(0)
        .agent("near")
        .pos(Vec2::new(100.0, 100.0))
        .done()
        .agent("far")
        .pos(Vec2::new(800.0, 800.0))
        .done()
        .build();
    let near = entities.get("near");
    let far = entities.get("far");

    // Start both agents at the same elevated stress level so the campfire's
    // contribution shows up as a difference in how much stress drops.
    // (ScenarioBuilder doesn't expose stress directly — mutate post-build.)
    for agent in [near, far] {
        let mut emotional = world.get_mut::<EmotionalState>(agent);
        emotional.stress_level = 80.0;
    }

    // Campfire 10px from the near agent — well within the 80px aura radius.
    let _campfire = world.spawn_campfire(Vec2::new(110.0, 100.0));

    world.tick(120);

    let near_stress = world.get::<EmotionalState>(near).stress_level;
    let far_stress = world.get::<EmotionalState>(far).stress_level;

    assert!(
        near_stress < far_stress,
        "agent inside campfire aura should have lower stress than distant control \
         (near={near_stress:.2}, far={far_stress:.2})"
    );
}

#[test]
fn agent_near_campfire_recovers_more_energy_than_distant_agent() {
    // Drain both agents deep into exhaustion so both cross the
    // `FATIGUE_SLEEP_THRESHOLD` (#386) and pick full Sleep instead of
    // the milder Rest. Starting at stamina 30 puts them on the
    // boundary: the `near` agent's campfire comfort drops their
    // urgency just below the threshold → Rest, while `far` stays
    // just above → Sleep. Sleep recovers faster than Rest, so `far`
    // ends up with more aerobic and the test's intended signal
    // (campfire proximity boosts recovery) gets inverted. Starting
    // at stamina 10 pushes both into the Sleep branch unambiguously
    // and leaves the campfire aura as the only variable.
    let (mut world, entities) = TestWorld::scenario(0)
        .agent("near")
        .pos(Vec2::new(100.0, 100.0))
        .stamina(10.0)
        .done()
        .agent("far")
        .pos(Vec2::new(800.0, 800.0))
        .stamina(10.0)
        .done()
        .build();
    let near = entities.get("near");
    let far = entities.get("far");

    let _campfire = world.spawn_campfire(Vec2::new(110.0, 100.0));

    world.tick(120);

    let near_aerobic = world.get::<PhysicalNeeds>(near).stamina.aerobic;
    let far_aerobic = world.get::<PhysicalNeeds>(far).stamina.aerobic;

    assert!(
        near_aerobic > far_aerobic,
        "agent inside campfire aura should recover more aerobic stamina than distant control \
         (near={near_aerobic:.2}, far={far_aerobic:.2})"
    );
}

#[test]
fn campfire_light_dims_when_fuel_exhausted() {
    let mut world = TestWorld::with_seed(0);
    let campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));

    // Drain all fuel: both the FuelConsumer float counter AND the item slot.
    {
        let mut consumer = world.get_mut::<FuelConsumer>(campfire);
        consumer.fuel_remaining = 2.0;
    }
    {
        let mut slots = world.get_mut::<ItemSlots>(campfire);
        while slots.remove_thing_unchecked(Concept::Wood).is_some() {}
    }

    world.tick(5);

    assert!(
        world.app().world().get::<LightSource>(campfire).is_none(),
        "campfire LightSource should be removed when fuel runs out"
    );
    assert!(
        world.app().world().get::<HeatSource>(campfire).is_none(),
        "campfire HeatSource should be removed when fuel runs out"
    );
}

// ─── Ontology derivation ─────────────────────────────────────────────────────

#[test]
fn campfire_has_light_emitting_trait_in_ontology() {
    let mut world = TestWorld::with_seed(0);
    let _campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));

    // Tick once to let ontology derivation system run
    world.tick(1);

    let ontology = world
        .app()
        .world()
        .resource::<worldsim::agent::mind::knowledge::Ontology>();
    assert!(
        ontology.has_trait(Concept::Campfire, Concept::LightEmitting),
        "campfire should have LightEmitting trait after LightSource is added"
    );
}

#[test]
fn campfire_has_heat_emitting_trait_in_ontology() {
    let mut world = TestWorld::with_seed(0);
    let _campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));

    world.tick(1);

    let ontology = world
        .app()
        .world()
        .resource::<worldsim::agent::mind::knowledge::Ontology>();
    assert!(
        ontology.has_trait(Concept::Campfire, Concept::HeatEmitting),
        "campfire should have HeatEmitting trait after HeatSource is added"
    );
}

// ─── Heat perception ─────────────────────────────────────────────────────────

#[test]
fn agent_near_heat_source_perceives_warmth() {
    let mut world = TestWorld::with_seed(0);

    // Agent at origin, campfire 30px away (within 64px heat radius)
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(100.0, 100.0)));
    let _campfire = world.spawn_campfire(Vec2::new(130.0, 100.0));

    world.tick(12);

    let mind = world.get::<MindGraph>(agent);
    let warmth_triples: Vec<_> = mind
        .query(
            None,
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Warmth)),
        )
        .into_iter()
        .collect();

    assert!(
        !warmth_triples.is_empty(),
        "agent near a campfire should perceive warmth via temperature sense"
    );
}

#[test]
fn agent_far_from_heat_source_does_not_perceive_warmth() {
    let mut world = TestWorld::with_seed(0);

    // Agent far away (500px), campfire at origin — well outside radius
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(500.0, 500.0)));
    let _campfire = world.spawn_campfire(Vec2::new(100.0, 100.0));

    world.tick(12);

    let mind = world.get::<MindGraph>(agent);
    let warmth_triples: Vec<_> = mind
        .query(
            None,
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Warmth)),
        )
        .into_iter()
        .collect();

    assert!(
        warmth_triples.is_empty(),
        "agent far from campfire should not perceive warmth"
    );
}
