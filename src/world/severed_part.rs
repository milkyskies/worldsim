//! Severed body parts as first-class world entities.
//!
//! Reads: nothing (pure spawn helpers)
//! Writes: SeveredPart component, spawn entities
//! Upstream: combat system (drops parts when a non-vital BodyPart HP hits 0)
//! Downstream: rendering (sprite), future harvest path (butcher parts for meat / bone),
//!             perception (agents see severed parts on the ground and react)
//!
//! Covers any non-vital `BodyPart` that gets destroyed in combat: limbs
//! (arms, legs), jaws, ears, mouths. Vital parts (head, torso) never end
//! up here — their destruction routes through the death path instead.
//!
//! Each severed part carries forensic data (owner entity, original part
//! name, severance tick) so future systems can power narrative ("your
//! friend's arm lies in the dirt"), butchering for meat and bone, or
//! emotional reactions from witnesses. For now they're cosmetic: they
//! exist, they render, they don't decay.

use bevy::prelude::*;

use crate::agent::biology::body::BodyNodeKind;
use crate::agent::inventory::EntityType;
use crate::agent::mind::knowledge::Concept;
use crate::world::Physical;

/// A severed anatomical part lying on the ground.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct SeveredPart {
    /// Which kind of body part this used to be. `kind.display_name()`
    /// gives the label for UI / logs.
    pub kind: BodyNodeKind,
    /// The entity that used to own this part. Kept as forensic data —
    /// future work can read it to drive narrative. May point at an entity
    /// that's since become a Corpse.
    pub owner: Entity,
    /// Tick at which the part hit the ground.
    pub severed_at_tick: u64,
}

/// Spawn a severed body part entity at the given world position.
pub fn spawn_severed_part(
    commands: &mut Commands,
    owner: Entity,
    kind: BodyNodeKind,
    position: Vec2,
    tick: u64,
) -> Entity {
    commands
        .spawn((
            Name::new(format!("severed {}", kind.display_name())),
            SeveredPart {
                kind,
                owner,
                severed_at_tick: tick,
            },
            EntityType(Concept::SeveredPart),
            Physical,
            Transform::from_translation(position.extend(0.8)),
            GlobalTransform::default(),
        ))
        .id()
}

pub struct SeveredPartPlugin;

impl Plugin for SeveredPartPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<SeveredPart>();
    }
}
