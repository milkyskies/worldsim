//! Stone node spawning logic.
//!
//! Reads: ItemSlots, ResourceRegeneration, WorldMap (biome tiles via spawn_config)
//! Writes: StoneNode entities (EntityType, ItemSlots, Affordance, ResourceRegeneration)
//! Upstream: world::spawn_config (layout), world::apple_tree (ResourceRegeneration)
//! Downstream: world::spawner (registered and synced each frame)

use crate::agent::inventory::EntityType;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::Concept;
use crate::world::apple_tree::ResourceRegeneration;
use crate::world::map::TILE_SIZE;
use bevy::prelude::*;

/// Marker component for stone chunk visuals on a stone node.
#[derive(Component)]
pub struct VisualStoneChunk;

/// Marker component used to target stone node queries without scanning all entities.
#[derive(Component)]
pub struct StoneNodeMarker;

/// Spawns a Stone Node with an ItemSlots inventory containing stone.
pub fn spawn_stone_node(commands: &mut Commands, position: Vec2, stones: u32) -> Entity {
    let mut inventory = ItemSlots::agent_carry();
    if stones > 0 {
        inventory.add(Concept::Stone, stones);
    }

    let base_size = Vec2::new(TILE_SIZE * 1.2, TILE_SIZE * 0.7);
    let base_color = Color::srgb(0.45, 0.45, 0.45);

    commands
        .spawn((
            Name::new("Stone Node"),
            EntityType(Concept::StoneNode),
            StoneNodeMarker,
            crate::world::Physical,
            Transform::from_translation(position.extend(1.0)),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
            inventory,
            crate::agent::affordance::Affordance {
                action_type: crate::agent::actions::ActionType::Harvest,
                cost: 6.0,
                distance: 28.0,
                risk: 0.0,
            },
            ResourceRegeneration {
                timer: 0.0,
                interval: 60.0,
                item: Concept::Stone,
                max_amount: 8,
            },
        ))
        .with_children(|parent| {
            parent.spawn((
                Sprite {
                    color: base_color,
                    custom_size: Some(base_size),
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
            ));

            if stones > 0 {
                use rand::Rng;
                let chunk_color = Color::srgb(0.6, 0.6, 0.62);
                let chunk_size = Vec2::new(5.0, 4.0);
                let mut rng = rand::rng();

                for _ in 0..stones.min(6) {
                    let x = rng.random_range(-base_size.x * 0.35..base_size.x * 0.35);
                    let y = rng.random_range(-base_size.y * 0.3..base_size.y * 0.3);

                    parent.spawn((
                        Sprite {
                            color: chunk_color,
                            custom_size: Some(chunk_size),
                            ..default()
                        },
                        Transform::from_translation(Vec3::new(x, y, 0.1)),
                        VisualStoneChunk,
                    ));
                }
            }
        })
        .id()
}

/// Syncs the visual stone chunk count with the inventory count.
pub fn sync_stone_visuals(
    mut commands: Commands,
    node_query: Query<(Entity, &ItemSlots, &Children), (With<StoneNodeMarker>, Changed<ItemSlots>)>,
    chunks_query: Query<Entity, With<VisualStoneChunk>>,
) {
    let base_size = Vec2::new(TILE_SIZE * 1.2, TILE_SIZE * 0.7);
    let chunk_color = Color::srgb(0.6, 0.6, 0.62);
    let chunk_size = Vec2::new(5.0, 4.0);

    for (node_entity, inventory, children) in node_query.iter() {
        let stone_count = inventory.count(Concept::Stone);

        let mut current_visuals = Vec::new();
        for child in children.iter() {
            if chunks_query.contains(child) {
                current_visuals.push(child);
            }
        }

        let diff = stone_count as i32 - current_visuals.len() as i32;

        if diff > 0 {
            use rand::Rng;
            let mut rng = rand::rng();
            for _ in 0..diff {
                let x = rng.random_range(-base_size.x * 0.35..base_size.x * 0.35);
                let y = rng.random_range(-base_size.y * 0.3..base_size.y * 0.3);
                commands.entity(node_entity).with_children(|parent| {
                    parent.spawn((
                        Sprite {
                            color: chunk_color,
                            custom_size: Some(chunk_size),
                            ..default()
                        },
                        Transform::from_translation(Vec3::new(x, y, 0.1)),
                        VisualStoneChunk,
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
