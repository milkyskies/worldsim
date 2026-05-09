//! Tree spawning logic (Apple Trees, etc.).

use crate::agent::inventory::EntityType;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::Concept;
use crate::palette::{Palette, PaletteColor};
use crate::world::map::TILE_SIZE;
use crate::world::property::HarvestableComponent;
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
pub fn spawn_apple_tree(
    commands: &mut Commands,
    palette: &Palette,
    position: Vec2,
    apples: u32,
) -> Entity {
    use rand::Rng;
    let mut rng = rand::rng();

    let mut inventory = ItemSlots::agent_carry();
    if apples > 0 {
        inventory.add(Concept::Apple, apples);
    }

    let leaf_size = Vec2::new(TILE_SIZE * 1.6, TILE_SIZE * 1.6);
    let trunk_size = Vec2::new(TILE_SIZE * 0.4, TILE_SIZE * 0.8);
    let trunk_color = palette.srgb(PaletteColor::SkinDeep);
    let leaf_color = palette.srgb(PaletteColor::LeafForest);

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
            HarvestableComponent {
                yields: Concept::Apple,
            },
            ResourceRegeneration {
                timer: 0.0,
                interval: 10.0, // 10 seconds per apple
                item: Concept::Apple,
                max_amount: 20,
            },
        ))
        .with_children(|parent| {
            // 0. SHADOW — dark ellipse at the base of the trunk.
            parent.spawn((
                Sprite {
                    color: palette.shadow(),
                    custom_size: Some(Vec2::new(TILE_SIZE * 1.4, TILE_SIZE * 0.35)),
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, -trunk_size.y * 0.5, -0.05)),
            ));

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
                        let apple_color = palette.srgb(PaletteColor::BloodFresh);
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
    palette: Res<Palette>,
    mut tree_query: Query<(&ItemSlots, &Children)>,
    mut leaves_query: Query<(Entity, &Children), With<VisualLeaves>>,
    apples_query: Query<Entity, With<VisualApple>>,
) {
    use rand::Rng;
    let mut rng = rand::rng();
    let leaf_size = Vec2::new(TILE_SIZE * 1.6, TILE_SIZE * 1.6);
    let apple_color = palette.srgb(PaletteColor::BloodFresh);
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
    mut query: Query<(&mut ItemSlots, &mut ResourceRegeneration)>,
    tick: Res<crate::core::tick::TickCount>,
) {
    let dt = tick.dt();

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

/// Linear-RGB lerp between `depleted` (fraction = 0) and `healthy` (fraction = 1).
/// `fraction` is clamped to `0.0..=1.0`. Public for unit tests of the
/// depletion-color logic without driving sprites.
pub fn depletion_color(fraction: f32, depleted: Color, healthy: Color) -> Color {
    let t = fraction.clamp(0.0, 1.0);
    let d = depleted.to_linear();
    let h = healthy.to_linear();
    Color::linear_rgba(
        d.red + (h.red - d.red) * t,
        d.green + (h.green - d.green) * t,
        d.blue + (h.blue - d.blue) * t,
        d.alpha + (h.alpha - d.alpha) * t,
    )
}

/// Depletion fraction `count / max`, clamped. A fraction of 0 means fully
/// depleted (the visual should be brown / dead-looking), 1 means full
/// regeneration (healthy leaf color).
pub fn depletion_fraction(count: u32, max: u32) -> f32 {
    if max == 0 {
        return 0.0;
    }
    (count as f32 / max as f32).clamp(0.0, 1.0)
}

/// Updates the leaf color of every Apple Tree to reflect its inventory level.
/// A full tree shows `LeafForest`; a depleted tree fades toward `SkinDeep`
/// (the same brown the trunk uses), so the canopy reads as bare/dying.
pub fn sync_tree_depletion_color(
    palette: Res<crate::palette::Palette>,
    trees: Query<(&ItemSlots, &ResourceRegeneration, &Children)>,
    mut leaves: Query<&mut Sprite, With<VisualLeaves>>,
) {
    let healthy = palette.srgb(crate::palette::PaletteColor::LeafForest);
    let depleted = palette.srgb(crate::palette::PaletteColor::SkinDeep);

    for (inventory, regen, children) in trees.iter() {
        if regen.item != Concept::Apple {
            continue;
        }
        let fraction = depletion_fraction(inventory.count(regen.item), regen.max_amount);
        let target = depletion_color(fraction, depleted, healthy);
        for child in children.iter() {
            if let Ok(mut sprite) = leaves.get_mut(child) {
                sprite.color = target;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depletion_fraction_zero_when_empty() {
        assert_eq!(depletion_fraction(0, 10), 0.0);
    }

    #[test]
    fn depletion_fraction_one_when_full() {
        assert_eq!(depletion_fraction(10, 10), 1.0);
    }

    #[test]
    fn depletion_fraction_zero_when_max_is_zero() {
        assert_eq!(depletion_fraction(5, 0), 0.0);
    }

    #[test]
    fn depletion_fraction_clamps_above_max() {
        assert_eq!(depletion_fraction(20, 10), 1.0);
    }

    #[test]
    fn depletion_color_at_zero_is_depleted_color() {
        let depleted = Color::linear_rgba(0.4, 0.2, 0.1, 1.0);
        let healthy = Color::linear_rgba(0.1, 0.7, 0.2, 1.0);
        let c = depletion_color(0.0, depleted, healthy).to_linear();
        assert!((c.red - 0.4).abs() < 1e-5);
        assert!((c.green - 0.2).abs() < 1e-5);
        assert!((c.blue - 0.1).abs() < 1e-5);
    }

    #[test]
    fn depletion_color_at_one_is_healthy_color() {
        let depleted = Color::linear_rgba(0.4, 0.2, 0.1, 1.0);
        let healthy = Color::linear_rgba(0.1, 0.7, 0.2, 1.0);
        let c = depletion_color(1.0, depleted, healthy).to_linear();
        assert!((c.red - 0.1).abs() < 1e-5);
        assert!((c.green - 0.7).abs() < 1e-5);
        assert!((c.blue - 0.2).abs() < 1e-5);
    }

    #[test]
    fn depletion_color_midpoint_is_average() {
        let depleted = Color::linear_rgba(0.0, 0.0, 0.0, 1.0);
        let healthy = Color::linear_rgba(1.0, 1.0, 1.0, 1.0);
        let c = depletion_color(0.5, depleted, healthy).to_linear();
        assert!((c.red - 0.5).abs() < 1e-5);
        assert!((c.green - 0.5).abs() < 1e-5);
        assert!((c.blue - 0.5).abs() < 1e-5);
    }
}
