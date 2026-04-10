//! Procedural sprite animation: wobble, bounce, tilt, squish derived from velocity and emotion.
//!
//! Reads: Transform (velocity via position delta), EmotionalState, CurrentActivity (sleep only), Time
//! Writes: Transform (visual offsets only — applied to root entity each frame)
//! Upstream: movement (Transform changes), emotions (EmotionalState), activity (sleep check)
//! Downstream: purely visual — no simulation systems read these offsets

use crate::agent::Agent;
use crate::agent::activity::CurrentActivity;
use crate::agent::psyche::emotions::EmotionalState;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use std::cmp::Ordering;
use std::collections::HashMap;

pub struct SpriteAnimationPlugin;

impl Plugin for SpriteAnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            animate_sprites.before(TransformSystems::Propagate),
        );
    }
}

/// Tracks the visual offset applied last frame so it can be subtracted before applying the new one.
/// This prevents accumulation — the logical position stays untouched.
#[derive(Component, Debug, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct SpriteAnimation {
    prev_y_offset: f32,
    prev_rotation: f32,
    prev_y_scale: f32,
    /// Per-entity phase offset so agents don't all sway in sync
    phase: f32,
}

impl SpriteAnimation {
    pub fn with_phase(phase: f32) -> Self {
        Self {
            prev_y_scale: 1.0,
            phase,
            ..Default::default()
        }
    }
}

// ── Emotion valence (simple, no personality needed) ─────────────────────────

/// Simple valence for an emotion type: positive emotions > 0, negative < 0.
/// Personality-weighted valence lives in the emotion module — this is a coarse visual signal.
fn simple_valence(emotion_type: crate::agent::psyche::emotions::EmotionType) -> f32 {
    use crate::agent::psyche::emotions::EmotionType;
    match emotion_type {
        EmotionType::Joy => 1.0,
        EmotionType::Surprise => 0.3,
        EmotionType::Sadness => -0.7,
        EmotionType::Fear => -1.0,
        EmotionType::Anger => -0.8,
        EmotionType::Disgust => -0.5,
    }
}

/// Returns (valence, intensity) of the dominant (highest intensity) active emotion.
/// Valence is in [-1.0, 1.0], intensity in [0.0, 1.0].
/// Returns (0.0, 0.0) if no emotions are active.
fn dominant_valence_intensity(emotions: &EmotionalState) -> (f32, f32) {
    emotions
        .active_emotions
        .iter()
        .max_by(|a, b| {
            a.intensity
                .partial_cmp(&b.intensity)
                .unwrap_or(Ordering::Equal)
        })
        .map(|e| (simple_valence(e.emotion_type), e.intensity))
        .unwrap_or((0.0, 0.0))
}

// ── Deterministic jitter ────────────────────────────────────────────────────

/// Cheap pseudo-random jitter from time using high-frequency sine waves.
/// Returns a value in [-amplitude, +amplitude].
fn jitter_offset(time: f32, phase: f32, amplitude: f32) -> f32 {
    let a = (time * 37.17 + phase).sin();
    let b = (time * 59.51 + phase * 1.7).sin();
    (a + b) * 0.5 * amplitude
}

// ── Main system ─────────────────────────────────────────────────────────────

fn animate_sprites(
    time: Res<Time>,
    mut query: Query<
        (
            Entity,
            &mut Transform,
            &mut SpriteAnimation,
            &CurrentActivity,
            &EmotionalState,
        ),
        With<Agent>,
    >,
    mut prev_positions: Local<HashMap<Entity, Vec2>>,
) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();

    // Track which entities are still alive for cleanup
    let mut alive_entities = Vec::with_capacity(query.iter().len());

    for (entity, mut transform, mut anim, activity, emotions) in query.iter_mut() {
        alive_entities.push(entity);

        // Undo previous frame's visual offset
        transform.translation.y -= anim.prev_y_offset;
        transform.rotation = transform.rotation * Quat::from_rotation_z(-anim.prev_rotation);
        if anim.prev_y_scale != 0.0 {
            transform.scale.y /= anim.prev_y_scale;
        }

        let phase = anim.phase;

        // Compute velocity from position delta
        let pos = transform.translation.truncate();
        let speed = if dt > 0.0 {
            prev_positions
                .get(&entity)
                .map(|prev| pos.distance(*prev) / dt)
                .unwrap_or(0.0)
        } else {
            0.0
        };
        prev_positions.insert(entity, pos);

        let (y_offset, rotation, y_scale) = if matches!(activity, CurrentActivity::Sleeping) {
            let breathing = (t * (std::f32::consts::TAU / 4.0) + phase).sin() * 0.02;
            (0.0, 0.0, 0.7 + breathing)
        } else {
            let (bob_freq, bob_amp, sway_amp) = if speed > 0.1 {
                (speed * 0.3, (speed * 0.02).min(3.0_f32), 0.0)
            } else {
                (0.0, 0.0, 2.0_f32.to_radians())
            };

            let sway_freq = std::f32::consts::TAU / 3.0;

            let (valence, intensity) = dominant_valence_intensity(emotions);
            let stress_freq_mult = 1.0 + emotions.stress_level * 0.003;

            let jitter_amount = if valence < 0.0 { intensity } else { 0.0 };
            let bounce_bonus = if valence > 0.0 { intensity * 0.5 } else { 0.0 };
            let subdued = if valence < 0.0 && intensity < 0.3 {
                1.0 - intensity
            } else {
                1.0
            };

            let bob = (t * bob_freq * stress_freq_mult + phase).sin()
                * (bob_amp + bob_amp * bounce_bonus)
                * subdued;
            let sway = (t * sway_freq * stress_freq_mult + phase).sin() * sway_amp * subdued;
            let jitter_y = if jitter_amount > 0.0 {
                jitter_offset(t, phase, jitter_amount)
            } else {
                0.0
            };

            (bob + jitter_y, sway, 1.0)
        };

        transform.translation.y += y_offset;
        transform.rotation = transform.rotation * Quat::from_rotation_z(rotation);
        transform.scale.y *= y_scale;

        anim.prev_y_offset = y_offset;
        anim.prev_rotation = rotation;
        anim.prev_y_scale = y_scale;
    }

    // Clean up stale entries for despawned entities
    if prev_positions.len() > alive_entities.len() {
        prev_positions.retain(|e, _| alive_entities.contains(e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::psyche::emotions::{Emotion, EmotionType};

    #[test]
    fn sleeping_produces_flattened_y_scale() {
        // The sleeping branch computes: 0.7 + sin(...) * 0.02
        // At any time t, the scale should be in [0.68, 0.72]
        for i in 0..100 {
            let t = i as f32 * 0.1;
            let phase = 0.0;
            let breathing = (t * (std::f32::consts::TAU / 4.0) + phase).sin() * 0.02;
            let scale = 0.7 + breathing;
            assert!(
                scale < 1.0 && scale > 0.0,
                "sleeping scale should be flattened, got {scale} at t={t}"
            );
        }
    }

    #[test]
    fn negative_valence_high_intensity_produces_jitter() {
        let mut scared = EmotionalState::default();
        scared.add_emotion(Emotion::new(EmotionType::Fear, 0.8));

        let (valence, intensity) = dominant_valence_intensity(&scared);
        assert!(valence < 0.0, "fear should have negative valence");
        assert!(intensity > 0.5, "high fear should have high intensity");

        let jitter_amount = if valence < 0.0 { intensity } else { 0.0 };
        assert!(jitter_amount > 0.0, "scared agent should have jitter");
    }

    #[test]
    fn positive_valence_high_intensity_increases_bounce() {
        let mut happy = EmotionalState::default();
        happy.add_emotion(Emotion::new(EmotionType::Joy, 0.8));

        let (valence, intensity) = dominant_valence_intensity(&happy);
        assert!(valence > 0.0, "joy should have positive valence");

        let bounce_bonus = intensity * 0.5;
        assert!(
            bounce_bonus > 0.0,
            "happy agent should have bounce bonus, got {bounce_bonus}"
        );
    }

    #[test]
    fn no_emotions_gives_neutral_valence() {
        let calm = EmotionalState::default();
        let (valence, intensity) = dominant_valence_intensity(&calm);
        assert_eq!(valence, 0.0);
        assert_eq!(intensity, 0.0);
    }

    #[test]
    fn dominant_emotion_wins() {
        let mut mixed = EmotionalState::default();
        mixed.add_emotion(Emotion::new(EmotionType::Joy, 0.3));
        mixed.add_emotion(Emotion::new(EmotionType::Fear, 0.9));

        let (valence, intensity) = dominant_valence_intensity(&mixed);
        assert!(valence < 0.0, "fear should dominate, got valence={valence}");
        assert!(
            (intensity - 0.9).abs() < 0.01,
            "dominant intensity should be 0.9, got {intensity}"
        );
    }

    #[test]
    fn jitter_offset_is_bounded() {
        let amp = 1.0;
        for i in 0..100 {
            let t = i as f32 * 0.1;
            let j = jitter_offset(t, 0.0, amp);
            assert!(
                j.abs() <= amp,
                "jitter should be bounded by amplitude, got {j} (max {amp})"
            );
        }
    }

    #[test]
    fn stationary_agent_gets_sway_not_bob() {
        let speed: f32 = 0.0;
        let (bob_freq, bob_amp, sway_amp) = if speed > 0.1 {
            (speed * 0.3, (speed * 0.02).min(3.0), 0.0)
        } else {
            (0.0, 0.0, 2.0_f32.to_radians())
        };

        assert_eq!(
            bob_freq, 0.0,
            "stationary agent should have no bob frequency"
        );
        assert_eq!(
            bob_amp, 0.0,
            "stationary agent should have no bob amplitude"
        );
        assert!(sway_amp > 0.0, "stationary agent should sway");
    }

    #[test]
    fn moving_agent_bob_scales_with_speed() {
        let slow_speed = 10.0;
        let fast_speed = 50.0;

        let slow_amp = (slow_speed * 0.02_f32).min(3.0);
        let fast_amp = (fast_speed * 0.02_f32).min(3.0);

        assert!(
            fast_amp > slow_amp,
            "faster agent should bob more (slow={slow_amp}, fast={fast_amp})"
        );
    }

    #[test]
    fn stress_increases_frequency() {
        let low_stress = 10.0;
        let high_stress = 80.0;

        let low_mult = 1.0 + low_stress * 0.003;
        let high_mult = 1.0 + high_stress * 0.003;

        assert!(
            high_mult > low_mult,
            "higher stress should increase frequency multiplier"
        );
    }
}
