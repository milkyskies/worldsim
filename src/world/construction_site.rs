//! Construction site spawning.
//!
//! Reads: Concept (target recipe), recipe requirements
//! Writes: Construction site entities (ItemSlots + Becomes + EntityType + Transform + optional Affordance + optional BuiltBy)
//! Upstream: execution system (Build action on_complete → SpawnRequest::Site)
//! Downstream: becomes_system (transforms site → finished entity when slots fill, carries BuiltBy forward),
//!             labor_accumulation_system (increments labor counter for Construct actions),
//!             perception (observers see the site as a perceivable world entity)
//!
//! A construction site is built entirely from existing primitives:
//! - `ItemSlots` with one `Construction` slot per recipe component
//! - `Becomes { target, trigger }` from the substrate (SlotsFilled or All([SlotsFilled, LaborAccumulated]))
//! - Standard perceivable-entity components (Transform, Physical, EntityType)
//! - Optional `Affordance { Construct }` when the site requires labor (enables GOAP planning)
//!
//! There is intentionally no "construction-specific" logic. Filling the slots
//! is the trigger; the `becomes_system` handles the rest.

use crate::agent::actions::ActionType;
use crate::agent::affordance::Affordance;
use crate::agent::inventory::EntityType;
use crate::agent::item_slots::{ItemSlots, Slot};
use crate::agent::mind::knowledge::Concept;
use crate::constants::actions::construct::INTERACTION_DISTANCE;
use crate::palette::{Palette, PaletteColor};
use crate::world::becomes::{Becomes, BecomesTrigger};
use crate::world::map::TILE_SIZE;
use crate::world::property::BuiltBy;
use bevy::prelude::*;

/// Marker for the placeholder visual sprite attached to a construction
/// site. Lets the sync system track which sites already have a visual
/// without re-spawning one every frame.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct ConstructionSiteVisual;

/// Marker for construction site entities. Lets queries narrow to "sites only"
/// without having to read `EntityType` and compare against `ConstructionSite`.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct ConstructionSiteMarker;

/// Spawn a construction site at `position` that will become `target` when its
/// Construction slots are filled (and optional labor is accumulated).
///
/// `requirements` is the list of `(material, required_quantity)` tuples derived
/// from the target concept's `Requires` recipe triples. One Construction slot is
/// created per requirement.
///
/// `initial_items` are deposited into the matching slots immediately. Used by
/// the Build action when the agent already has the materials in hand. If
/// `initial_items` fully satisfies all requirements AND no labor is required,
/// the next `becomes_system` pass will transform the site into the finished entity.
///
/// `labor_required` optionally adds a `LaborAccumulated` condition that agents
/// must satisfy via the `Construct` action. When `Some(n)`, the site's trigger
/// becomes `All([SlotsFilled, LaborAccumulated { required: n, current: 0 }])` and
/// an `Affordance { Construct }` component is added so the GOAP planner can find
/// this site as a Construct target.
///
/// `started_tick` records when the site was placed (used by composite triggers
/// like `All([SlotsFilled, AfterTicks(N)])` for cooking-style processes).
///
/// `builder` is the agent who placed the site. When `Some`, a `BuiltBy`
/// component is attached so the finished entity (after `becomes_system` fires)
/// carries the world-truth record of who built it. The builder's MindGraph
/// also receives a `(Self, Owns, finished_entity)` triple at transformation time.
pub fn spawn_construction_site_headless(
    commands: &mut Commands,
    target: Concept,
    position: Vec2,
    requirements: &[(Concept, u32)],
    initial_items: &[(Concept, u32)],
    labor_required: Option<u32>,
    started_tick: u64,
    builder: Option<Entity>,
) -> Entity {
    let mut item_slots = ItemSlots {
        slots: requirements
            .iter()
            .map(|(material, required)| Slot::construction(*material, *required))
            .collect(),
    };

    for (material, qty) in initial_items {
        item_slots.deposit(*material, *qty, None);
    }

    let trigger = match labor_required {
        None => BecomesTrigger::SlotsFilled,
        Some(required) => BecomesTrigger::All(vec![
            BecomesTrigger::SlotsFilled,
            BecomesTrigger::LaborAccumulated {
                required,
                current: 0,
            },
        ]),
    };

    let mut entity_cmd = commands.spawn((
        Name::new(format!("Site<{target:?}>")),
        EntityType(Concept::ConstructionSite),
        ConstructionSiteMarker,
        crate::world::Physical,
        Transform::from_translation(position.extend(1.0)),
        GlobalTransform::default(),
        // Visibility plumbing so the placeholder sprite the visual
        // sync system attaches as a child actually renders. Without
        // these the child sprites are silently culled.
        Visibility::default(),
        InheritedVisibility::default(),
        ViewVisibility::default(),
        item_slots,
        Becomes::new(target, trigger, started_tick),
    ));

    if labor_required.is_some() {
        entity_cmd.insert(Affordance {
            action_type: ActionType::Construct,
            cost: crate::constants::actions::construct::BASE_COST,
            distance: INTERACTION_DISTANCE,
            risk: 0.0,
        });
    }

    if let Some(builder) = builder {
        entity_cmd.insert(BuiltBy {
            builder,
            built_at: started_tick,
        });
    }

    entity_cmd.id()
}

/// Attach a placeholder visual to any construction site that doesn't
/// have one yet. Sites are spawned headless (no Sprite) by the action
/// pipeline, then this Update-time pass walks through and gives each
/// new site a sandy outlined square so the player can actually see
/// where they placed it. When the site transforms into the finished
/// entity (lean-to / house / chest) the visual is despawned with its
/// parent, so no cleanup needed here.
pub fn sync_construction_site_visuals(
    mut commands: Commands,
    palette: Res<Palette>,
    sites: Query<(Entity, &Children), With<ConstructionSiteMarker>>,
    existing_visuals: Query<&ConstructionSiteVisual>,
) {
    let footprint = Vec2::new(TILE_SIZE * 1.2, TILE_SIZE * 1.0);
    let body_color = palette.srgb(PaletteColor::SkinDeep);
    let outline_color = palette.srgb(PaletteColor::LeafForest);

    for (site_entity, children) in sites.iter() {
        // Skip if any child is already a visual.
        if children.iter().any(|c| existing_visuals.contains(c)) {
            continue;
        }
        commands.entity(site_entity).with_children(|parent| {
            // Outline rim — slightly larger box behind the fill so the
            // edge reads at small zoom levels.
            parent.spawn((
                ConstructionSiteVisual,
                Sprite {
                    color: outline_color,
                    custom_size: Some(footprint + Vec2::splat(2.0)),
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, 0.0, -0.05)),
            ));
            // Sandy fill — represents the "in-progress" feel.
            parent.spawn((
                ConstructionSiteVisual,
                Sprite {
                    color: body_color,
                    custom_size: Some(footprint),
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
            ));
        });
    }
}
