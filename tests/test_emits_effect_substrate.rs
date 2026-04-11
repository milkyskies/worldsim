//! Integration tests for the `EmitsEffect` substrate (#230).
//!
//! Validates the general radial effect primitive: entities with an
//! `EmitsEffect` component apply their effect to nearby agents each tick.
//! Mirrors the test structure from `test_becomes_substrate.rs`.

use bevy::prelude::*;
use worldsim::agent::Agent;
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::psyche::emotions::{Emotion, EmotionType, EmotionalState};
use worldsim::core::tick::TickCount;
use worldsim::world::emits_effect::{EffectKind, EmitsEffect, emits_effect_system};

/// With 3600 tps, `dt = 3600.0 / 3600.0 = 1.0` per tick.
/// Effect math becomes direct: `StressPerSec(-10.0)` → `-10.0` per tick.
const TICKS_PER_SECOND: f32 = 3600.0;

fn test_app() -> App {
    let mut app = App::new();
    app.insert_resource(TickCount::new(TICKS_PER_SECOND));
    app.add_systems(Update, emits_effect_system);
    app
}

fn advance_tick(app: &mut App) {
    app.world_mut().resource_mut::<TickCount>().current += 1;
    app.update();
}

fn spawn_agent(app: &mut App, pos: Vec2, stress: f32, aerobic: f32) -> Entity {
    app.world_mut()
        .spawn((
            Agent,
            Transform::from_xyz(pos.x, pos.y, 0.0),
            PhysicalNeeds {
                stamina: worldsim::agent::body::needs::Stamina {
                    aerobic,
                    ..Default::default()
                },
                metabolism: worldsim::agent::body::metabolism::Metabolism::well_fed(),
                thirst: 0.0,
                health: 100.0,
                last_health_damage: None,
            },
            EmotionalState {
                stress_level: stress,
                ..Default::default()
            },
        ))
        .id()
}

fn spawn_emitter(app: &mut App, pos: Vec2, radius: f32, effect: EffectKind) -> Entity {
    app.world_mut()
        .spawn((
            EmitsEffect::new(radius, effect),
            Transform::from_xyz(pos.x, pos.y, 0.0),
        ))
        .id()
}

// ═══════════════════════════════════════════════════════════════════════════
// Core radius check
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn effect_applied_to_agent_inside_radius() {
    let mut app = test_app();
    let agent = spawn_agent(&mut app, Vec2::ZERO, 50.0, 50.0);
    // Emitter at (3, 0) with radius 5: agent at origin is 3 units away → inside.
    spawn_emitter(
        &mut app,
        Vec2::new(3.0, 0.0),
        5.0,
        EffectKind::StressPerSec(-10.0),
    );

    advance_tick(&mut app);

    let stress = app
        .world()
        .get::<EmotionalState>(agent)
        .unwrap()
        .stress_level;
    assert!(
        stress < 50.0,
        "Agent inside radius must have reduced stress; got {stress}"
    );
}

#[test]
fn effect_not_applied_to_agent_outside_radius() {
    let mut app = test_app();
    // Agent at (10, 0), emitter at origin with radius 5 → distance 10 > radius.
    let agent = spawn_agent(&mut app, Vec2::new(10.0, 0.0), 50.0, 50.0);
    spawn_emitter(&mut app, Vec2::ZERO, 5.0, EffectKind::StressPerSec(-10.0));

    advance_tick(&mut app);

    let stress = app
        .world()
        .get::<EmotionalState>(agent)
        .unwrap()
        .stress_level;
    assert_eq!(
        stress, 50.0,
        "Agent outside radius must be unaffected; got {stress}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// EffectKind variants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn composite_all_applies_every_subeffect() {
    let mut app = test_app();
    let agent = spawn_agent(&mut app, Vec2::ZERO, 50.0, 50.0);
    // Campfire-like: both stress reduction and stamina recovery in one tick.
    spawn_emitter(
        &mut app,
        Vec2::ZERO,
        5.0,
        EffectKind::All(vec![
            EffectKind::StressPerSec(-10.0),
            EffectKind::StaminaPerSec(10.0),
        ]),
    );

    advance_tick(&mut app);

    let world = app.world();
    let stress = world.get::<EmotionalState>(agent).unwrap().stress_level;
    let aerobic = world.get::<PhysicalNeeds>(agent).unwrap().stamina.aerobic;
    assert!(
        stress < 50.0,
        "All composite must apply the stress sub-effect; got {stress}"
    );
    assert!(
        aerobic > 50.0,
        "All composite must apply the stamina sub-effect; got {aerobic}"
    );
}

#[test]
fn negative_stress_value_reduces_stress() {
    // Comfort zone (campfire-style): negative StressPerSec decreases stress_level.
    let mut app = test_app();
    let agent = spawn_agent(&mut app, Vec2::ZERO, 50.0, 50.0);
    spawn_emitter(&mut app, Vec2::ZERO, 5.0, EffectKind::StressPerSec(-10.0));

    advance_tick(&mut app);

    // dt = 1.0, so stress change = -10.0 * 1.0 = -10.0 → expected 40.0.
    let stress = app
        .world()
        .get::<EmotionalState>(agent)
        .unwrap()
        .stress_level;
    assert!(
        (stress - 40.0).abs() < 0.001,
        "Negative StressPerSec must decrease stress_level (expected ~40.0); got {stress}"
    );
}

#[test]
fn positive_stress_value_increases_stress() {
    // Hostile zone: positive StressPerSec raises stress_level.
    let mut app = test_app();
    let agent = spawn_agent(&mut app, Vec2::ZERO, 10.0, 50.0);
    spawn_emitter(&mut app, Vec2::ZERO, 5.0, EffectKind::StressPerSec(20.0));

    advance_tick(&mut app);

    let stress = app
        .world()
        .get::<EmotionalState>(agent)
        .unwrap()
        .stress_level;
    assert!(
        stress > 10.0,
        "Positive StressPerSec must increase stress_level; got {stress}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Stacking and control cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn multiple_emitters_stack_linearly() {
    let mut app = test_app();
    let agent = spawn_agent(&mut app, Vec2::ZERO, 50.0, 50.0);
    // Two emitters, each draining 10 stress/sec. Combined: -20/sec.
    spawn_emitter(&mut app, Vec2::ZERO, 5.0, EffectKind::StressPerSec(-10.0));
    spawn_emitter(
        &mut app,
        Vec2::new(1.0, 0.0),
        5.0,
        EffectKind::StressPerSec(-10.0),
    );

    advance_tick(&mut app);

    // dt = 1.0 → each emitter: -10.0 * 1.0 = -10. Total = -20. Expected stress = 30.
    let stress = app
        .world()
        .get::<EmotionalState>(agent)
        .unwrap()
        .stress_level;
    assert!(
        (stress - 30.0).abs() < 0.001,
        "Two emitters must stack linearly (expected ~30.0); got {stress}"
    );
}

#[test]
fn agent_outside_all_emitters_unchanged() {
    let mut app = test_app();
    let agent = spawn_agent(&mut app, Vec2::new(100.0, 0.0), 50.0, 50.0);
    spawn_emitter(&mut app, Vec2::ZERO, 5.0, EffectKind::StressPerSec(-10.0));
    spawn_emitter(
        &mut app,
        Vec2::new(10.0, 0.0),
        5.0,
        EffectKind::StaminaPerSec(10.0),
    );

    advance_tick(&mut app);

    let world = app.world();
    let stress = world.get::<EmotionalState>(agent).unwrap().stress_level;
    let aerobic = world.get::<PhysicalNeeds>(agent).unwrap().stamina.aerobic;
    assert_eq!(
        stress, 50.0,
        "Stress must be unchanged when agent is outside all emitters"
    );
    assert_eq!(
        aerobic, 50.0,
        "Aerobic must be unchanged when agent is outside all emitters"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Determinism
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn effect_runs_in_deterministic_tick_order() {
    // Running the same scenario from two different starting ticks must produce
    // identical results — no hidden Time-based variance.
    let run = |start_tick: u64| -> f32 {
        let mut app = App::new();
        let mut tick = TickCount::new(TICKS_PER_SECOND);
        tick.current = start_tick;
        app.insert_resource(tick);
        app.add_systems(Update, emits_effect_system);

        let agent = spawn_agent(&mut app, Vec2::ZERO, 50.0, 50.0);
        spawn_emitter(&mut app, Vec2::ZERO, 5.0, EffectKind::StressPerSec(-10.0));

        advance_tick(&mut app);

        app.world()
            .get::<EmotionalState>(agent)
            .unwrap()
            .stress_level
    };

    let result_a = run(0);
    let result_b = run(1000);
    assert_eq!(
        result_a, result_b,
        "Effect outcome must be identical regardless of starting tick ({result_a} vs {result_b})"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Fear emotion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fear_per_sec_reduces_existing_fear() {
    let mut app = test_app();
    let agent = {
        let mut emotional = EmotionalState::default();
        emotional.add_emotion(Emotion::new(EmotionType::Fear, 1.0));
        app.world_mut()
            .spawn((
                Agent,
                Transform::from_xyz(0.0, 0.0, 0.0),
                PhysicalNeeds::default(),
                emotional,
            ))
            .id()
    };
    // Lantern: drains fear by 0.5/sec. dt = 1.0 → intensity drops 0.5.
    spawn_emitter(&mut app, Vec2::ZERO, 5.0, EffectKind::FearPerSec(-0.5));

    advance_tick(&mut app);

    let fear_intensity = app
        .world()
        .get::<EmotionalState>(agent)
        .unwrap()
        .active_emotions
        .iter()
        .find(|e| e.emotion_type == EmotionType::Fear)
        .map(|e| e.intensity)
        .unwrap_or(0.0);
    assert!(
        fear_intensity < 1.0,
        "FearPerSec(-) must reduce fear intensity; got {fear_intensity}"
    );
}
