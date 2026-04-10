//! World components for entities that emit perceivable signals through non-visual senses.
//!
//! Reads: nothing (pure data components)
//! Writes: HeatSource, SoundSource, SoundKind (attached to world entities)
//! Upstream: campfire spawning, action execution (adds transient SoundSource)
//! Downstream: perception systems (perceive_temperature, perceive_hearing)

use bevy::prelude::*;

/// An entity that emits heat, perceivable through the temperature sense.
/// Does not require line-of-sight — warmth passes through walls and around corners.
#[derive(Component, Reflect, Clone)]
#[reflect(Component)]
pub struct HeatSource {
    /// How far the heat can be felt (in world pixels).
    pub range: f32,
    /// Heat intensity (0.0 = barely warm, 1.0 = blazing).
    pub intensity: f32,
}

/// What kind of sound an entity is producing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum SoundKind {
    /// Wolf territorial/pack call
    Howl,
    /// Deer alarm stomp
    AlarmCall,
    /// Agent in distress
    Scream,
    /// Fighting sounds
    Combat,
}

impl SoundKind {
    /// Whether this sound indicates danger to listeners.
    pub fn is_threatening(self) -> bool {
        matches!(self, Self::Howl | Self::Combat | Self::Scream)
    }
}

/// A transient component for entities currently producing sound.
/// Added when an action produces sound (howling, fighting, screaming)
/// and removed after one perception tick. Event-like, not persistent state.
#[derive(Component, Reflect, Clone)]
#[reflect(Component)]
pub struct SoundSource {
    /// What kind of sound is being produced.
    pub kind: SoundKind,
    /// Affects effective hearing range (0.0 = whisper, 1.0 = loud).
    pub intensity: f32,
}
