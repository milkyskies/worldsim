//! World components for entities that emit perceivable signals through non-visual senses.
//!
//! Reads: nothing (pure data components)
//! Writes: SoundSource, SoundKind (attached to world entities)
//! Upstream: action execution (adds transient SoundSource)
//! Downstream: perception systems (perceive_hearing)
//!
//! Note: HeatSource lives in world::property — it is a registered property component
//! that auto-derives ontology traits. Import it from there.

use bevy::prelude::*;

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
