//! Tree spawning logic (Apple Trees, etc.).

use crate::agent::inventory::{EntityType, Inventory};
use crate::agent::mind::knowledge::Concept;
use crate::world::map::TILE_SIZE;
use bevy::prelude::*;

/// Marker component for tree leaf visuals.
#[derive(Component)]
pub struct VisualLeaves;

/// Marker component for apple visuals on trees.
#[derive(Component)]
pub struct VisualApple;

/// Component for resources that regenerate over time.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct ResourceRegeneration {
    pub timer: f32,
    pub interval: f32,
    pub item: Concept,
    pub max_amount: u32,
}

/// Spawns an Apple Tree with an Inventory
pub fn spawn_apple_tree(commands: &mut Commands, position: Vec2, apples: u32) -> Entity {
    use rand::Rng;
    let mut rng = rand::rng();

    let mut inventory = Inventory::default();
    if apples > 0 {
        inventory.add(Concept::Apple, apples);
    }

    // Tree Dimensions (Twice as big as before)
    let leaf_size = Vec2::new(TILE_SIZE * 1.6, TILE_SIZE * 1.6);
    let trunk_size = Vec2::new(TILE_SIZE * 0.4, TILE_SIZE * 0.8);
    let trunk_color = Color::srgb(0.55, 0.27, 0.07); // SaddleBrown
    let leaf_color = Color::srgb(0.13, 0.55, 0.13); // ForestGreen

    // Spawn ECS Entity (Root Container)
    

    commands
        .spawn((
            Name::new("Apple Tree"),
            EntityType(Concept::AppleTree), // What this entity IS
            crate::world::Physical,
            // Container Transform
            Transform::from_translation(position.extend(1.0)),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
            inventory, // Unified Inventory!
            // Affordance: "You can Harvest me"
            crate::agent::affordance::Affordance {
                action_type: crate::agent::actions::ActionType::Harvest,
                cost: 5.0,
                distance: 32.0, // Increased interaction distance due to size
                risk: 0.0,
            },
            ResourceRegeneration {
                timer: 0.0,
                interval: 10.0, // 10 seconds per apple
                item: Concept::Apple,
                max_amount: 20,
            },
        ))
        .with_children(|parent| {
            // 1. TRUNK
            parent.spawn((
                Sprite {
                    color: trunk_color,
                    custom_size: Some(trunk_size),
                    ..default()
                },
                // Position trunk at the bottom
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
            ));

            // 2. LEAVES (The main "tree" part)
            parent
                .spawn((
                    Sprite {
                        color: leaf_color,
                        custom_size: Some(leaf_size),
                        ..default()
                    },
                    // Position leaves above trunk
                    Transform::from_translation(Vec3::new(0.0, trunk_size.y * 0.8, 0.1)),
                    VisualLeaves,
                ))
                .with_children(|leaves| {
                    // 3. APPLES (Visual only, data is in Inventory)
                    if apples > 0 {
                        let apple_color = Color::srgb(0.8, 0.2, 0.2);
                        let apple_size = Vec2::new(4.0, 4.0);

                        for _ in 0..apples.min(10) {
                            // Limit visual apples so it doesn't look too crowded
                            let x = rng.random_range(-leaf_size.x * 0.4..leaf_size.x * 0.4);
                            let y = rng.random_range(-leaf_size.y * 0.4..leaf_size.y * 0.4);

                            leaves.spawn((
                                Sprite {
                                    color: apple_color,
                                    custom_size: Some(apple_size),
                                    ..default()
                                },
                                Transform::from_translation(Vec3::new(x, y, 0.2)),
                                VisualApple,
                            ));
                        }
                    }
                });
        })
        .id()
}

/// Syncs the visual apple count with the inventory count.
pub fn sync_apple_visuals(
    mut commands: Commands,
    mut tree_query: Query<(&Inventory, &Children)>,
    mut leaves_query: Query<(Entity, &Children), With<VisualLeaves>>,
    apples_query: Query<Entity, With<VisualApple>>,
) {
    use rand::Rng;
    let mut rng = rand::rng();
    let leaf_size = Vec2::new(TILE_SIZE * 1.6, TILE_SIZE * 1.6);
    let apple_color = Color::srgb(0.8, 0.2, 0.2);
    let apple_size = Vec2::new(4.0, 4.0);

    for (inventory, children) in tree_query.iter_mut() {
        let apple_count = inventory.count(Concept::Apple);

        // Find the leaves child
        for child in children.iter() {
            if let Ok((leaves_entity, leaf_children)) = leaves_query.get_mut(child) {
                // Count current visual apples
                let mut current_visuals = Vec::new();
                for leaf_child in leaf_children.iter() {
                    if apples_query.contains(leaf_child) {
                        current_visuals.push(leaf_child);
                    }
                }

                let diff = apple_count as i32 - current_visuals.len() as i32;

                if diff > 0 {
                    // Spawn more
                    for _ in 0..diff {
                        let x = rng.random_range(-leaf_size.x * 0.4..leaf_size.x * 0.4);
                        let y = rng.random_range(-leaf_size.y * 0.4..leaf_size.y * 0.4);

                        commands.entity(leaves_entity).with_children(|parent| {
                            parent.spawn((
                                Sprite {
                                    color: apple_color,
                                    custom_size: Some(apple_size),
                                    ..default()
                                },
                                Transform::from_translation(Vec3::new(x, y, 0.2)),
                                VisualApple,
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

/// Regenerates resources (e.g., apples on trees) over time.
pub fn regenerate_resources(
    mut query: Query<(&mut Inventory, &mut ResourceRegeneration)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();

    for (mut inventory, mut regen) in query.iter_mut() {
        let current = inventory.count(regen.item);
        if current < regen.max_amount {
            regen.timer += dt;
            if regen.timer >= regen.interval {
                regen.timer = 0.0;
                inventory.add(regen.item, 1);
            }
        }
    }
}
