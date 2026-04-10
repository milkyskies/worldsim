//! EmitsEffect: general substrate for entities that apply effects to nearby agents.
//!
//! Reads: EmitsEffect (component), Transform, TickCount
//! Writes: PhysicalNeeds (energy), EmotionalState (stress_level, fear), SimEvent::EffectApplied
//! Upstream: World entities with EmitsEffect (campfires, lanterns, hostile zones)
//! Downstream: Perception (agents experience effects without necessarily knowing why)

use bevy::prelude::*;

use crate::agent::Agent;
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::events::SimEvent;
use crate::agent::psyche::emotions::{Emotion, EmotionType, EmotionalState};
use crate::core::tick::TickCount;

/// World-truth component declaring "this entity applies an effect to agents
/// within `radius` each tick". Lives on the world entity, NOT in any agent's mind.
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct EmitsEffect {
    /// Radius (in world units) within which agents receive the effect.
    pub radius: f32,
    /// The effect applied each tick to every in-range agent.
    #[reflect(ignore)]
    pub effect: EffectKind,
}

impl Default for EmitsEffect {
    fn default() -> Self {
        Self {
            radius: 0.0,
            effect: EffectKind::StressPerSec(0.0),
        }
    }
}

impl EmitsEffect {
    pub fn new(radius: f32, effect: EffectKind) -> Self {
        Self { radius, effect }
    }
}

/// The per-tick effect applied to each in-range agent.
///
/// Values are **per-second rates**; the system scales by `dt` so simulation
/// speed does not change total effect.
///
/// Sign convention: positive values increase the stat, negative values decrease it.
/// - `StressPerSec(-0.5)` → campfire-style, stress drops 0.5/sec
/// - `StressPerSec(2.0)` → hostile zone, stress rises 2/sec
/// - `EnergyPerSec(2.0)` → campfire-style, energy recovers 2/sec
/// - `FearPerSec(-1.0)` → lantern, fear drops 1 intensity/sec
#[derive(Clone, Debug)]
pub enum EffectKind {
    /// Per-second change to `stress_level`. Negative decreases stress (comfort
    /// zones); positive increases stress (hostile zones, cursed ground).
    StressPerSec(f32),
    /// Per-second change to `energy`. Positive restores energy; negative drains it.
    EnergyPerSec(f32),
    /// Per-second change to fear-emotion intensity. Negative reduces fear (lanterns,
    /// safe havens); positive increases fear.
    FearPerSec(f32),
    /// Apply every sub-effect. Use for entities with multiple simultaneous effects,
    /// e.g. `All([StressPerSec(-0.5), EnergyPerSec(2.0)])` for a campfire.
    All(Vec<EffectKind>),
}

impl Default for EffectKind {
    fn default() -> Self {
        Self::StressPerSec(0.0)
    }
}

/// Apply a single `EffectKind` to one agent's body state. Called recursively for `All`.
fn apply_effect(
    effect: &EffectKind,
    dt: f32,
    physical: &mut PhysicalNeeds,
    emotional: &mut EmotionalState,
) {
    match effect {
        EffectKind::StressPerSec(rate) => {
            emotional.stress_level = (emotional.stress_level + rate * dt).clamp(0.0, 100.0);
        }
        EffectKind::EnergyPerSec(rate) => {
            physical.energy = (physical.energy + rate * dt).clamp(0.0, 100.0);
        }
        EffectKind::FearPerSec(rate) => {
            let delta = rate * dt;
            if delta > 0.0 {
                emotional.add_emotion(Emotion::new(EmotionType::Fear, delta));
            } else {
                emotional.drain_emotion(EmotionType::Fear, -delta);
            }
        }
        EffectKind::All(effects) => {
            for sub in effects {
                apply_effect(sub, dt, physical, emotional);
            }
        }
    }
}

/// Process all `EmitsEffect` entities each tick. For every agent within an
/// emitter's radius, apply the emitter's effect to that agent's body state.
///
/// Runs after `becomes_system` (entity transformations that may spawn new emitters)
/// and before perception (agents experience effects in the same tick they occur).
pub fn emits_effect_system(
    emitters: Query<(Entity, &EmitsEffect, &Transform)>,
    mut agents: Query<(Entity, &Transform, &mut PhysicalNeeds, &mut EmotionalState), With<Agent>>,
    tick: Res<TickCount>,
    mut sim_events: Option<MessageWriter<SimEvent>>,
) {
    let dt = tick.dt();

    for (emitter_entity, emits, emitter_transform) in emitters.iter() {
        let emitter_pos = emitter_transform.translation.truncate();

        for (agent_entity, agent_transform, mut physical, mut emotional) in agents.iter_mut() {
            let agent_pos = agent_transform.translation.truncate();
            if emitter_pos.distance(agent_pos) <= emits.radius {
                apply_effect(&emits.effect, dt, &mut physical, &mut emotional);
                if let Some(ref mut events) = sim_events {
                    events.write(SimEvent::EffectApplied {
                        agent: agent_entity,
                        tick: tick.current,
                        source: emitter_entity,
                    });
                }
            }
        }
    }
}
