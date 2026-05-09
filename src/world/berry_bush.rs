//! Berry Bush spawning logic.

use crate::agent::inventory::EntityType;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::Concept;
use crate::palette::{Palette, PaletteColor};
use crate::world::map::TILE_SIZE;
use crate::world::property::HarvestableComponent;
use bevy::prelude::*;

use super::apple_tree::{ResourceRegeneration, depletion_color, depletion_fraction};

/// Marker component for berry bush leaf visuals.
#[derive(Component)]
pub struct VisualBushLeaves;

/// Marker component for berry visuals on bushes.
#[derive(Component)]
pub struct VisualBerry;

/// Spawns a Berry Bush with an Inventory
pub fn spawn_berry_bush(
    commands: &mut Commands,
    palette: &Palette,
    position: Vec2,
    berries: u32,
) -> Entity {
    use rand::Rng;
    let mut rng = rand::rng();

    let mut inventory = ItemSlots::agent_carry();
    if berries > 0 {
        inventory.add(Concept::Berry, berries);
    }

    let bush_size = Vec2::new(TILE_SIZE * 1.0, TILE_SIZE * 0.8);
    let bush_color = palette.srgb(PaletteColor::LeafBush);

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
            HarvestableComponent {
                yields: Concept::Berry,
            },
            ResourceRegeneration {
                timer: 0.0,
                interval: 8.0, // 8 seconds per berry (faster than apples)
                item: Concept::Berry,
                max_amount: 15,
            },
        ))
        .with_children(|parent| {
            // Shadow — dark ellipse underneath the bush.
            parent.spawn((
                Sprite {
                    color: palette.shadow(),
                    custom_size: Some(Vec2::new(bush_size.x * 1.1, bush_size.y * 0.35)),
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, -bush_size.y * 0.5, -0.05)),
            ));

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
                        let berry_color = palette.srgb(PaletteColor::AccentBerry);
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
    palette: Res<Palette>,
    mut bush_query: Query<(&ItemSlots, &Children)>,
    mut leaves_query: Query<(Entity, &Children), With<VisualBushLeaves>>,
    berries_query: Query<Entity, With<VisualBerry>>,
) {
    use rand::Rng;
    let mut rng = rand::rng();
    let bush_size = Vec2::new(TILE_SIZE * 1.0, TILE_SIZE * 0.8);
    let berry_color = palette.srgb(PaletteColor::AccentBerry);
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

/// Updates the bush body color of every Berry Bush to reflect its inventory
/// level. A full bush shows `LeafBush`; a depleted bush fades toward
/// `SkinDeep` (brown), so empty bushes read as bare twigs.
pub fn sync_bush_depletion_color(
    palette: Res<Palette>,
    bushes: Query<(&ItemSlots, &ResourceRegeneration, &Children)>,
    mut leaves: Query<&mut Sprite, With<VisualBushLeaves>>,
) {
    let healthy = palette.srgb(PaletteColor::LeafBush);
    let depleted = palette.srgb(PaletteColor::SkinDeep);

    for (inventory, regen, children) in bushes.iter() {
        if regen.item != Concept::Berry {
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
    use crate::testing::TestWorld;
    use bevy::math::Vec2;

    fn bush_berry_count(world: &TestWorld, bush: bevy::ecs::entity::Entity) -> u32 {
        let inv = world
            .app()
            .world()
            .get::<ItemSlots>(bush)
            .expect("bush should have ItemSlots");
        inv.count(Concept::Berry)
    }

    #[test]
    fn empty_bush_regenerates_to_at_least_one_berry_over_time() {
        let mut world = TestWorld::with_seed(10);
        let bush = world.spawn_berry_bush(Vec2::new(50.0, 50.0), 0);
        assert_eq!(bush_berry_count(&world, bush), 0);

        // Default berry bush regen interval is 8.0 rate-units. At test
        // dt = 1/60, that is 480 ticks per +1 berry. Tick well past it.
        world.tick(700);

        let count = bush_berry_count(&world, bush);
        assert!(
            count >= 1,
            "expected at least 1 berry to regenerate after 700 ticks, got {count}"
        );
    }

    #[test]
    fn full_bush_does_not_regenerate_above_max() {
        let mut world = TestWorld::with_seed(11);
        // Spawn already at the cap (15 = berry-bush max_amount).
        let bush = world.spawn_berry_bush(Vec2::new(50.0, 50.0), 15);
        assert_eq!(bush_berry_count(&world, bush), 15);

        world.tick(2000);

        let count = bush_berry_count(&world, bush);
        assert_eq!(
            count, 15,
            "bush at max should not exceed max_amount, got {count}"
        );
    }

    #[test]
    fn fully_drained_bush_can_refill_to_max_given_enough_time() {
        let mut world = TestWorld::with_seed(12);
        let bush = world.spawn_berry_bush(Vec2::new(50.0, 50.0), 0);

        // Run long enough to refill from 0 to the cap (15 * 480 + slack).
        world.tick(15 * 480 + 200);

        let count = bush_berry_count(&world, bush);
        assert_eq!(count, 15, "depleted bush should refill back to max");
    }

    #[test]
    fn manually_depleted_bush_starts_regenerating_again() {
        let mut world = TestWorld::with_seed(13);
        let bush = world.spawn_berry_bush(Vec2::new(50.0, 50.0), 5);

        // Drain as if an agent harvested every berry.
        {
            let mut inv = world
                .app_mut()
                .world_mut()
                .get_mut::<ItemSlots>(bush)
                .unwrap();
            assert!(inv.remove(Concept::Berry, 5));
        }
        assert_eq!(bush_berry_count(&world, bush), 0);

        world.tick(700);
        let count = bush_berry_count(&world, bush);
        assert!(
            count >= 1,
            "drained bush should resume regenerating, got {count}"
        );
    }
}
