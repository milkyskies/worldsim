//! Wood log spawning logic.
//!
//! Reads: ItemSlots, ResourceRegeneration, WorldMap (biome tiles via spawn_config)
//! Writes: WoodLog entities (EntityType, ItemSlots, Affordance, HarvestableComponent, ResourceRegeneration)
//! Upstream: world::spawn_config (layout), world::apple_tree (ResourceRegeneration)
//! Downstream: world::spawner (registered and synced each frame)

use crate::agent::inventory::EntityType;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::Concept;
use crate::world::apple_tree::ResourceRegeneration;
use crate::world::map::TILE_SIZE;
use crate::world::property::HarvestableComponent;
use bevy::prelude::*;

/// Marker component for wood visual pieces on a log.
#[derive(Component)]
pub struct VisualWoodPiece;

/// Marker component used to target wood log queries without scanning all entities.
#[derive(Component)]
pub struct WoodLogMarker;

/// Spawns a Wood Log with an ItemSlots inventory containing wood.
pub fn spawn_wood_log(commands: &mut Commands, position: Vec2, wood: u32) -> Entity {
    let mut inventory = ItemSlots::agent_carry();
    if wood > 0 {
        inventory.add(Concept::Wood, wood);
    }

    let log_size = Vec2::new(TILE_SIZE * 1.4, TILE_SIZE * 0.5);
    let log_color = Color::srgb(0.4, 0.25, 0.1);

    commands
        .spawn((
            Name::new("Wood Log"),
            EntityType(Concept::WoodLog),
            WoodLogMarker,
            crate::world::Physical,
            Transform::from_translation(position.extend(1.0)),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
            inventory,
            crate::agent::affordance::Affordance {
                action_type: crate::agent::actions::ActionType::Harvest,
                cost: 4.0,
                distance: 24.0,
                risk: 0.0,
            },
            HarvestableComponent {
                yields: Concept::Wood,
                remaining: wood,
                max: 6,
                regrow_rate: 0.0,
            },
            ResourceRegeneration {
                timer: 0.0,
                interval: 45.0,
                item: Concept::Wood,
                max_amount: 6,
            },
        ))
        .with_children(|parent| {
            // Shadow — dark ellipse underneath the log.
            parent.spawn((
                Sprite {
                    color: Color::srgba(0.0, 0.0, 0.0, 0.35),
                    custom_size: Some(Vec2::new(log_size.x * 1.1, log_size.y * 0.5)),
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, -log_size.y * 0.35, -0.05)),
            ));

            parent.spawn((
                Sprite {
                    color: log_color,
                    custom_size: Some(log_size),
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
            ));

            if wood > 0 {
                use rand::Rng;
                let mut rng = rand::rng();
                let piece_color = Color::srgb(0.55, 0.35, 0.15);
                let piece_size = Vec2::new(6.0, 4.0);

                for _ in 0..wood.min(5) {
                    let x = rng.random_range(-log_size.x * 0.35..log_size.x * 0.35);
                    let y = rng.random_range(-log_size.y * 0.25..log_size.y * 0.25);

                    parent.spawn((
                        Sprite {
                            color: piece_color,
                            custom_size: Some(piece_size),
                            ..default()
                        },
                        Transform::from_translation(Vec3::new(x, y, 0.1)),
                        VisualWoodPiece,
                    ));
                }
            }
        })
        .id()
}

/// Syncs the visual wood piece count with the inventory count.
pub fn sync_wood_visuals(
    mut commands: Commands,
    log_query: Query<(Entity, &ItemSlots, &Children), (With<WoodLogMarker>, Changed<ItemSlots>)>,
    pieces_query: Query<Entity, With<VisualWoodPiece>>,
) {
    let log_size = Vec2::new(TILE_SIZE * 1.4, TILE_SIZE * 0.5);
    let piece_color = Color::srgb(0.55, 0.35, 0.15);
    let piece_size = Vec2::new(6.0, 4.0);

    for (log_entity, inventory, children) in log_query.iter() {
        let wood_count = inventory.count(Concept::Wood);

        let mut current_visuals = Vec::new();
        for child in children.iter() {
            if pieces_query.contains(child) {
                current_visuals.push(child);
            }
        }

        let diff = wood_count as i32 - current_visuals.len() as i32;

        if diff > 0 {
            use rand::Rng;
            let mut rng = rand::rng();
            for _ in 0..diff {
                let x = rng.random_range(-log_size.x * 0.35..log_size.x * 0.35);
                let y = rng.random_range(-log_size.y * 0.25..log_size.y * 0.25);
                commands.entity(log_entity).with_children(|parent| {
                    parent.spawn((
                        Sprite {
                            color: piece_color,
                            custom_size: Some(piece_size),
                            ..default()
                        },
                        Transform::from_translation(Vec3::new(x, y, 0.1)),
                        VisualWoodPiece,
                    ));
                });
            }
        } else if diff < 0 {
            for i in 0..diff.abs() {
                if let Some(&entity) = current_visuals.get(i as usize) {
                    commands.entity(entity).despawn();
                }
            }
        }
    }
}
