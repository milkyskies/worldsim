//! Emotion-driven eye-state for any creature whose silhouette has eye parts.
//!
//! Reads: ActiveActions, EmotionalState, SpriteBody, SilhouettePartLink (children)
//! Writes: Sprite (size + color) on eye-role silhouette children
//! Upstream: silhouette renderer attaches `SilhouettePartLink { role: Eye, base_size, base_color }`
//! Downstream: visual only - no simulation state effect
//!
//! The system walks every eye-role sprite, hops up to its `SpriteBody` parent
//! to find the owning agent, reads `ActiveActions + EmotionalState`, and picks
//! an `EyeState` that modulates the Sprite's `custom_size` against the
//! recorded `base_size`. Re-applying is idempotent so the system can run every
//! tick without drift.

use bevy::prelude::*;

use crate::agent::actions::registry::ActiveActions;
use crate::agent::actions::types::ActionType;
use crate::agent::psyche::emotions::{EmotionType, EmotionalState};
use crate::silhouette::{PartRole, SilhouettePartLink};
use crate::ui::sprite_animation::SpriteBody;

pub struct EyesPlugin;

impl Plugin for EyesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update_eye_states);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EyeState {
    Open,
    Wide,
    Squint,
    Closed,
}

fn pick_eye_state(active: &ActiveActions, emotions: &EmotionalState) -> EyeState {
    if active.contains(ActionType::Sleep) {
        return EyeState::Closed;
    }
    let fear = emotions
        .active_emotions
        .iter()
        .find(|e| e.emotion_type == EmotionType::Fear)
        .map(|e| e.intensity)
        .unwrap_or(0.0);
    if fear > 0.5 {
        return EyeState::Wide;
    }
    if emotions.stress_level > 70.0 {
        return EyeState::Squint;
    }
    EyeState::Open
}

fn update_eye_states(
    bodies: Query<&SpriteBody>,
    agents: Query<(&ActiveActions, &EmotionalState)>,
    mut eyes: Query<(&mut Sprite, &SilhouettePartLink, &ChildOf)>,
) {
    for (mut sprite, link, child_of) in eyes.iter_mut() {
        if link.role != PartRole::Eye {
            continue;
        }
        let Ok(body) = bodies.get(child_of.parent()) else {
            continue;
        };
        let Ok((active, emotions)) = agents.get(body.root) else {
            continue;
        };
        let state = pick_eye_state(active, emotions);
        let (sx, sy) = state_scale(state);
        sprite.color = link.base_color;
        sprite.custom_size = Some(Vec2::new(link.base_size.x * sx, link.base_size.y * sy));
    }
}

fn state_scale(state: EyeState) -> (f32, f32) {
    match state {
        EyeState::Open => (1.0, 1.0),
        EyeState::Wide => (1.4, 1.4),
        EyeState::Squint => (1.0, 0.4),
        EyeState::Closed => (1.0, 0.15),
    }
}
