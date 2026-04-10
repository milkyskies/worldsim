//! Procedural sprite animation: hop-based movement like indie pixel art games.
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

/// Tracks the visual offset applied last frame so it can be undone cleanly.
#[derive(Component, Debug, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct SpriteAnimation {
    prev_y_offset: f32,
    prev_x_scale: f32,
    prev_y_scale: f32,
    /// Per-entity phase offset so agents don't all hop in sync
    phase: f32,
}

impl SpriteAnimation {
    pub fn with_phase(phase: f32) -> Self {
        Self {
            prev_x_scale: 1.0,
            prev_y_scale: 1.0,
            phase,
            ..Default::default()
        }
    }
}

// ── Hop curve ───────────────────────────────────────────────────────────────

/// Hop height from a phase angle. Always >= 0 (sprite only goes UP).
/// Uses abs(sin()) so the sprite lifts off, lands, lifts off, lands.
fn hop_height(phase_angle: f32) -> f32 {
    phase_angle.sin().abs()
}

/// Landing squish: when the sprite is near the ground (hop_height near 0),
/// squash Y and stretch X briefly. Returns (x_scale, y_scale).
fn landing_squish(hop: f32, squish_amount: f32) -> (f32, f32) {
    // hop is 0 at landing, 1 at peak. Invert for squish strength.
    let ground_proximity = 1.0 - hop;
    // Only squish when very close to ground (bottom 20% of hop)
    let squish = if ground_proximity > 0.8 {
        (ground_proximity - 0.8) * 5.0 * squish_amount // ramp from 0 to squish_amount
    } else {
        0.0
    };
    (1.0 + squish * 0.5, 1.0 - squish) // wider + shorter
}

// ── Emotion valence ─────────────────────────────────────────────────────────

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
            Option<&CurrentActivity>,
            Option<&EmotionalState>,
        ),
        With<Agent>,
    >,
    mut prev_positions: Local<HashMap<Entity, Vec2>>,
) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();

    let mut alive_entities = Vec::with_capacity(query.iter().len());

    for (entity, mut transform, mut anim, activity, emotions) in query.iter_mut() {
        alive_entities.push(entity);

        // Undo previous frame's visual offset
        transform.translation.y -= anim.prev_y_offset;
        if anim.prev_x_scale != 0.0 {
            transform.scale.x /= anim.prev_x_scale;
        }
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

        let is_sleeping = matches!(activity, Some(CurrentActivity::Sleeping));

        let default_emotions = EmotionalState::default();
        let emo = emotions.unwrap_or(&default_emotions);
        let (valence, intensity) = dominant_valence_intensity(emo);

        let (y_offset, x_scale, y_scale) = if is_sleeping {
            // Sleeping: flatten + breathe
            let breathing = (t * (std::f32::consts::TAU / 4.0) + phase).sin() * 0.02;
            (0.0, 1.0, 0.7 + breathing)
        } else if speed > 0.1 {
            // Moving: HOP! Frequency and height scale with speed
            let hop_freq = 8.0 + speed * 0.04; // faster = faster hops
            let hop_amp = 3.0 + (speed * 0.03).min(4.0); // faster = higher hops, cap at 7px

            // Emotion modifiers
            let amp_bonus = if valence > 0.0 { intensity * 0.4 } else { 0.0 };
            let freq_bonus = if valence < 0.0 { intensity * 3.0 } else { 0.0 }; // scared = frantic
            let stress_freq = emo.stress_level * 0.02;

            let final_freq = hop_freq + freq_bonus + stress_freq;
            let final_amp = hop_amp * (1.0 + amp_bonus);

            let hop = hop_height(t * final_freq + phase);
            let y = hop * final_amp;
            let (sx, sy) = landing_squish(hop, 0.3);

            // Scared jitter on top of hops
            let jitter_y = if valence < 0.0 && intensity > 0.2 {
                jitter_offset(t, phase, intensity * 0.8)
            } else {
                0.0
            };

            (y + jitter_y, sx, sy)
        } else {
            // Idle: very gentle, slow micro-hops (breathing-like bounce)
            let idle_freq = 2.0;
            let idle_amp = 1.0;

            let amp_bonus = if valence > 0.0 { intensity * 0.5 } else { 0.0 };

            let hop = hop_height(t * idle_freq + phase);
            let y = hop * idle_amp * (1.0 + amp_bonus);
            let (sx, sy) = landing_squish(hop, 0.15);

            let jitter_y = if valence < 0.0 && intensity > 0.2 {
                jitter_offset(t, phase, intensity * 0.5)
            } else {
                0.0
            };

            (y + jitter_y, sx, sy)
        };

        // Apply
        transform.translation.y += y_offset;
        transform.scale.x *= x_scale;
        transform.scale.y *= y_scale;

        anim.prev_y_offset = y_offset;
        anim.prev_x_scale = x_scale;
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
    fn hop_height_is_always_non_negative() {
        for i in 0..100 {
            let angle = i as f32 * 0.1;
            let h = hop_height(angle);
            assert!(h >= 0.0, "hop height must never go below ground, got {h}");
        }
    }

    #[test]
    fn hop_height_reaches_zero_and_one() {
        // At 0 and PI, sin = 0 so hop = 0 (landing)
        assert!((hop_height(0.0)).abs() < 0.001);
        assert!((hop_height(std::f32::consts::PI)).abs() < 0.001);
        // At PI/2, sin = 1 so hop = 1 (peak)
        assert!((hop_height(std::f32::consts::FRAC_PI_2) - 1.0).abs() < 0.001);
    }

    #[test]
    fn landing_squish_squashes_at_ground() {
        let (sx, sy) = landing_squish(0.0, 0.3);
        assert!(sx > 1.0, "should stretch X on landing, got {sx}");
        assert!(sy < 1.0, "should squash Y on landing, got {sy}");
    }

    #[test]
    fn no_squish_at_peak() {
        let (sx, sy) = landing_squish(1.0, 0.3);
        assert!((sx - 1.0).abs() < 0.001, "no X stretch at peak, got {sx}");
        assert!((sy - 1.0).abs() < 0.001, "no Y squash at peak, got {sy}");
    }

    #[test]
    fn sleeping_produces_flattened_y_scale() {
        for i in 0..100 {
            let t = i as f32 * 0.1;
            let breathing = (t * (std::f32::consts::TAU / 4.0)).sin() * 0.02;
            let scale = 0.7 + breathing;
            assert!(
                scale < 1.0 && scale > 0.0,
                "sleeping scale should be flattened, got {scale}"
            );
        }
    }

    #[test]
    fn scared_agent_gets_jitter() {
        let mut scared = EmotionalState::default();
        scared.add_emotion(Emotion::new(EmotionType::Fear, 0.8));

        let (valence, intensity) = dominant_valence_intensity(&scared);
        assert!(valence < 0.0);
        assert!(intensity > 0.2);
        // Jitter should be nonzero at various times
        let mut any_nonzero = false;
        for i in 0..50 {
            let t = i as f32 * 0.1;
            let j = jitter_offset(t, 0.0, intensity * 0.8);
            if j.abs() > 0.01 {
                any_nonzero = true;
                break;
            }
        }
        assert!(any_nonzero, "scared agent should produce nonzero jitter");
    }

    #[test]
    fn happy_agent_hops_higher() {
        let calm = EmotionalState::default();
        let mut happy = EmotionalState::default();
        happy.add_emotion(Emotion::new(EmotionType::Joy, 0.8));

        let (calm_val, calm_int) = dominant_valence_intensity(&calm);
        let (happy_val, happy_int) = dominant_valence_intensity(&happy);

        let calm_bonus = if calm_val > 0.0 { calm_int * 0.4 } else { 0.0 };
        let happy_bonus = if happy_val > 0.0 {
            happy_int * 0.4
        } else {
            0.0
        };

        assert!(
            happy_bonus > calm_bonus,
            "happy agent should have higher hop bonus"
        );
    }

    #[test]
    fn jitter_offset_is_bounded() {
        let amp = 1.0;
        for i in 0..100 {
            let t = i as f32 * 0.1;
            let j = jitter_offset(t, 0.0, amp);
            assert!(j.abs() <= amp, "jitter must be bounded, got {j}");
        }
    }
}
