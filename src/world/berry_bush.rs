//! Berry Bush spawning logic.

use crate::agent::inventory::{EntityType, Inventory};
use crate::agent::mind::knowledge::Concept;
use crate::world::map::TILE_SIZE;
use bevy::prelude::*;

use super::apple_tree::ResourceRegeneration;

/// Marker component for berry bush leaf visuals.
#[derive(Component)]
pub struct VisualBushLeaves;

/// Marker component for berry visuals on bushes.
#[derive(Component)]
pub struct VisualBerry;

/// Spawns a Berry Bush with an Inventory
pub fn spawn_berry_bush(commands: &mut Commands, position: Vec2, berries: u32) -> Entity {
    use rand::Rng;
    let mut rng = rand::rng();

    let mut inventory = Inventory::default();
    if berries > 0 {
        inventory.add(Concept::Berry, berries);
    }

    // Bush Dimensions (smaller than tree)
    let bush_size = Vec2::new(TILE_SIZE * 1.0, TILE_SIZE * 0.8);
    let bush_color = Color::srgb(0.2, 0.5, 0.2); // Darker green

    // Spawn ECS Entity (Root Container)
    

    commands
        .spawn((
            Name::new("Berry Bush"),
            EntityType(Concept::BerryBush),
            crate::world::Physical,
            // Container Transform
            Transform::from_translation(position.extend(1.0)),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
            inventory,
            // Affordance: "You can Harvest me"
            crate::agent::affordance::Affordance {
                action_type: crate::agent::actions::ActionType::Harvest,
                cost: 3.0, // Easier to harvest than trees
                distance: 24.0,
                risk: 0.0,
            },
            ResourceRegeneration {
                timer: 0.0,
                interval: 8.0, // 8 seconds per berry (faster than apples)
                item: Concept::Berry,
                max_amount: 15,
            },
        ))
        .with_children(|parent| {
            // Bush body
            parent
                .spawn((
                    Sprite {
                        color: bush_color,
                        custom_size: Some(bush_size),
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
                    VisualBushLeaves,
                ))
                .with_children(|bush| {
                    // Berries (Visual only, data is in Inventory)
                    if berries > 0 {
                        let berry_color = Color::srgb(0.6, 0.1, 0.4); // Purple-ish berries
                        let berry_size = Vec2::new(3.0, 3.0);

                        for _ in 0..berries.min(8) {
                            let x = rng.random_range(-bush_size.x * 0.35..bush_size.x * 0.35);
                            let y = rng.random_range(-bush_size.y * 0.35..bush_size.y * 0.35);

                            bush.spawn((
                                Sprite {
                                    color: berry_color,
                                    custom_size: Some(berry_size),
                                    ..default()
                                },
                                Transform::from_translation(Vec3::new(x, y, 0.2)),
                                VisualBerry,
                            ));
                        }
                    }
                });
        })
        .id()
}

/// Syncs the visual berry count with the inventory count.
pub fn sync_berry_visuals(
    mut commands: Commands,
    mut bush_query: Query<(&Inventory, &Children)>,
    mut leaves_query: Query<(Entity, &Children), With<VisualBushLeaves>>,
    berries_query: Query<Entity, With<VisualBerry>>,
) {
    use rand::Rng;
    let mut rng = rand::rng();
    let bush_size = Vec2::new(TILE_SIZE * 1.0, TILE_SIZE * 0.8);
    let berry_color = Color::srgb(0.6, 0.1, 0.4);
    let berry_size = Vec2::new(3.0, 3.0);

    for (inventory, children) in bush_query.iter_mut() {
        let berry_count = inventory.count(Concept::Berry);

        // Find the bush leaves child
        for child in children.iter() {
            if let Ok((leaves_entity, leaf_children)) = leaves_query.get_mut(child) {
                // Count current visual berries
                let mut current_visuals = Vec::new();
                for leaf_child in leaf_children.iter() {
                    if berries_query.contains(leaf_child) {
                        current_visuals.push(leaf_child);
                    }
                }

                let diff = berry_count as i32 - current_visuals.len() as i32;

                if diff > 0 {
                    // Spawn more
                    for _ in 0..diff {
                        let x = rng.random_range(-bush_size.x * 0.35..bush_size.x * 0.35);
                        let y = rng.random_range(-bush_size.y * 0.35..bush_size.y * 0.35);

                        commands.entity(leaves_entity).with_children(|parent| {
                            parent.spawn((
                                Sprite {
                                    color: berry_color,
                                    custom_size: Some(berry_size),
                                    ..default()
                                },
                                Transform::from_translation(Vec3::new(x, y, 0.2)),
                                VisualBerry,
                            ));
                        });
                    }
                } else if diff < 0 {
                    // Despawn some
                    for i in 0..diff.abs() {
                        if let Some(&entity) = current_visuals.get(i as usize) {
                            commands.entity(entity).despawn();
                        }
                    }
                }
            }
        }
    }
}
