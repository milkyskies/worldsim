//! Data-driven creature silhouette: every species is a list of part primitives,
//! not a hand-rolled child sprite tree.
//!
//! Reads: CreatureSilhouette (component, attached at spawn), Palette (resource)
//! Writes: child sprite hierarchy under each silhouette-tagged entity
//! Upstream: world::{wolf,deer,human} *_silhouette() builders attach the component
//! Downstream: future markings (#694), injury overlays (#695), eyes (#697) read
//!             the same data and modulate or layer onto these parts

use bevy::prelude::*;

use crate::agent::biology::body::BodyNodeKind;
use crate::palette::{Palette, PaletteColor};
use crate::ui::sprite_animation::{GroundShadow, SpriteBody};
use crate::world::environment::{AgentBodySprite, BaseColor};

pub struct SilhouettePlugin;

impl Plugin for SilhouettePlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<crate::palette::PalettePlugin>() {
            app.add_plugins(crate::palette::PalettePlugin);
        }
        app.add_systems(Update, render_added_silhouettes);
    }
}

/// Composable description of how a creature is drawn. Per-species builder
/// functions (`wolf_silhouette`, `deer_silhouette`, `human_silhouette`) return
/// the canonical "average" silhouette; future systems mutate or layer onto it.
#[derive(Component, Clone, Debug)]
pub struct CreatureSilhouette {
    pub parts: Vec<SilhouettePart>,
    pub shadow_size: Vec2,
    pub shadow_offset_y: f32,
    pub hop_phase: f32,
}

impl CreatureSilhouette {
    pub fn with_hop_phase(mut self, phase: f32) -> Self {
        self.hop_phase = phase;
        self
    }
}

#[derive(Clone, Debug)]
pub struct SilhouettePart {
    /// Anatomical link for injury/scar overlays. Decorative parts leave None.
    pub body_node: Option<BodyNodeKind>,
    pub shape: Shape,
    pub size: Vec2,
    pub offset: Vec2,
    pub rotation: f32,
    pub color: PaletteColor,
    pub z_bias: i8,
    pub role: PartRole,
    /// When true the rendered sprite gets `BaseColor` + `AgentBodySprite`
    /// markers so `apply_sprite_lighting` modulates it on the agent path
    /// (gentler night dimming than terrain). Default false matches the
    /// pre-silhouette behavior for non-human creatures.
    pub tint_with_environment: bool,
}

/// Locked shape vocabulary. Currently every variant renders as a colored
/// rectangle via `Sprite` (matching pre-silhouette rendering); a follow-up
/// can lift any subset to mesh-based primitives without changing the data
/// model or any caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Ellipse,
    Circle,
    Triangle,
    Capsule,
    Teardrop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartRole {
    Body,
    Limb,
    Ear,
    Eye,
    Snout,
    Tail,
    Marking,
}

fn render_added_silhouettes(
    mut commands: Commands,
    palette: Res<Palette>,
    query: Query<(Entity, &CreatureSilhouette), Added<CreatureSilhouette>>,
) {
    for (entity, silhouette) in query.iter() {
        commands.entity(entity).with_children(|root| {
            spawn_shadow(root, entity, silhouette, &palette);
            spawn_sprite_body(root, entity, silhouette, &palette);
        });
    }
}

fn spawn_shadow(
    root: &mut ChildSpawnerCommands,
    entity: Entity,
    silhouette: &CreatureSilhouette,
    palette: &Palette,
) {
    let y = silhouette.shadow_offset_y;
    root.spawn((
        GroundShadow::new(entity, Vec2::new(0.0, y)),
        Sprite {
            color: palette.shadow(),
            custom_size: Some(silhouette.shadow_size),
            ..default()
        },
        Transform::from_translation(Vec3::new(0.0, y, -0.05)),
    ));
}

fn spawn_sprite_body(
    root: &mut ChildSpawnerCommands,
    entity: Entity,
    silhouette: &CreatureSilhouette,
    palette: &Palette,
) {
    root.spawn((
        SpriteBody::new(entity, silhouette.hop_phase),
        Transform::default(),
        GlobalTransform::default(),
        Visibility::default(),
        InheritedVisibility::default(),
        ViewVisibility::default(),
    ))
    .with_children(|body| {
        for (i, part) in silhouette.parts.iter().enumerate() {
            spawn_part(body, part, palette, i);
        }
    });
}

fn spawn_part(
    body: &mut ChildSpawnerCommands,
    part: &SilhouettePart,
    palette: &Palette,
    index: usize,
) {
    let color = palette.srgb(part.color);
    // z_bias picks the layer (body=0, head=1, ears/eyes=2, ...); index is a
    // tiny tiebreaker so spawn order is stable within a layer.
    let z = (part.z_bias as f32) * 0.1 + (index as f32) * 0.001;
    let transform = Transform {
        translation: Vec3::new(part.offset.x, part.offset.y, z),
        rotation: Quat::from_rotation_z(part.rotation),
        ..default()
    };
    let sprite = Sprite {
        color,
        custom_size: Some(part.size),
        ..default()
    };
    if part.tint_with_environment {
        body.spawn((sprite, BaseColor(color), AgentBodySprite, transform));
    } else {
        body.spawn((sprite, transform));
    }
}
