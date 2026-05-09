//! Saplings: young plants that grow into mature plants over time.
//!
//! Reads: TickCount, Sapling, Transform
//! Writes: Sapling entities (EntityType, Sapling marker), spawns mature plants
//!         via `spawn_apple_tree` / `spawn_berry_bush`, emits SimEvent::PlantMatured
//! Upstream: world::spawner (registers system + spawn helper exposed for layout)
//! Downstream: world::apple_tree, world::berry_bush (mature plants spawned in place)

use crate::agent::events::{SimEvent, SimEventKind};
use crate::agent::inventory::EntityType;
use crate::agent::mind::knowledge::Concept;
use crate::core::tick::TickCount;
use crate::palette::{Palette, PaletteColor};
use crate::world::apple_tree::spawn_apple_tree;
use crate::world::berry_bush::spawn_berry_bush;
use crate::world::map::TILE_SIZE;
use bevy::prelude::*;

/// A young plant that grows over time and is replaced in place by a mature
/// plant when its growth timer reaches `mature_at`.
///
/// `growth_timer` and `mature_at` are in the same rate-units used elsewhere
/// in the simulation: `tick.dt()` is added each FixedUpdate and a value of
/// `1.0` represents 60 game-seconds (1 game-minute).
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Sapling {
    pub growth_timer: f32,
    pub mature_at: f32,
    pub matures_into: Concept,
}

#[derive(Component)]
pub struct VisualSaplingStem;

/// Default time for a sapling to mature: 4 game-hours = 240 rate-units.
/// Slow enough to feel like a lifecycle, fast enough that a long sim run
/// will see saplings turn into trees.
pub const DEFAULT_MATURE_AT: f32 = 240.0;

/// Spawns a Sapling that will grow into `matures_into` (currently expects
/// `Concept::AppleTree` or `Concept::BerryBush`).
pub fn spawn_sapling(
    commands: &mut Commands,
    palette: &Palette,
    position: Vec2,
    matures_into: Concept,
    mature_at: f32,
) -> Entity {
    let stem_size = Vec2::new(TILE_SIZE * 0.15, TILE_SIZE * 0.4);
    let leaf_size = Vec2::new(TILE_SIZE * 0.5, TILE_SIZE * 0.5);
    let stem_color = palette.srgb(PaletteColor::SkinDeep);
    let leaf_color = palette.srgb(PaletteColor::LeafBright);

    commands
        .spawn((
            Name::new("Sapling"),
            EntityType(Concept::Sapling),
            crate::world::Physical,
            Transform::from_translation(position.extend(1.0)),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
            Sapling {
                growth_timer: 0.0,
                mature_at,
                matures_into,
            },
        ))
        .with_children(|parent| {
            parent.spawn((
                Sprite {
                    color: palette.shadow(),
                    custom_size: Some(Vec2::new(stem_size.x * 2.5, stem_size.y * 0.3)),
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, -stem_size.y * 0.5, -0.05)),
            ));
            parent.spawn((
                Sprite {
                    color: stem_color,
                    custom_size: Some(stem_size),
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
                VisualSaplingStem,
            ));
            parent.spawn((
                Sprite {
                    color: leaf_color,
                    custom_size: Some(leaf_size),
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, stem_size.y * 0.6, 0.1)),
            ));
        })
        .id()
}

/// Advances every sapling's growth timer by `tick.dt()`. When a sapling
/// reaches `mature_at`, it is despawned and replaced at the same position
/// by its mature plant (`AppleTree` or `BerryBush`), with a freshly empty
/// inventory — newly mature plants must regenerate their first crop.
pub fn grow_saplings(
    mut commands: Commands,
    palette: Res<Palette>,
    tick: Res<TickCount>,
    mut events: MessageWriter<SimEvent>,
    mut query: Query<(Entity, &mut Sapling, &Transform)>,
) {
    let dt = tick.dt();
    let current_tick = tick.current;

    let ready: Vec<(Entity, Vec2, Concept)> = query
        .iter_mut()
        .filter_map(|(entity, mut sapling, transform)| {
            sapling.growth_timer += dt;
            if sapling.growth_timer >= sapling.mature_at {
                Some((
                    entity,
                    transform.translation.truncate(),
                    sapling.matures_into,
                ))
            } else {
                None
            }
        })
        .collect();

    for (entity, pos, matures_into) in ready {
        commands.entity(entity).despawn();
        let mature = match matures_into {
            Concept::AppleTree => spawn_apple_tree(&mut commands, &palette, pos, 0),
            Concept::BerryBush => spawn_berry_bush(&mut commands, &palette, pos, 0),
            _ => continue,
        };
        events.write(SimEvent::new(
            current_tick,
            Vec::new(),
            SimEventKind::PlantMatured {
                mature,
                matured_into: matures_into,
            },
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::events::SimEventKind;
    use crate::agent::inventory::EntityType;
    use crate::testing::TestWorld;
    use bevy::math::Vec2;

    #[test]
    fn sapling_matures_into_apple_tree_after_mature_at_ticks() {
        let mut world = TestWorld::with_seed(1);
        let pos = Vec2::new(50.0, 50.0);
        let mature_at = 5.0;
        let sapling = world.spawn_sapling(pos, Concept::AppleTree, mature_at);

        // Each tick advances dt = 1/60 rate-units. Need mature_at / dt ticks
        // to cross the threshold; 60 * mature_at + slack covers it.
        world.tick((60.0 * mature_at) as u64 + 5);

        // Sapling must be gone, replaced in place by an AppleTree.
        assert!(
            world.app().world().get::<Sapling>(sapling).is_none(),
            "sapling should be despawned after maturing"
        );

        let app_world = world.app_mut().world_mut();
        let mut entity_types = app_world.query::<(&EntityType, &Transform)>();
        let found_tree_at_pos = entity_types.iter(app_world).any(|(etype, transform)| {
            etype.0 == Concept::AppleTree && (transform.translation.truncate() - pos).length() < 0.1
        });
        assert!(
            found_tree_at_pos,
            "expected an AppleTree at the sapling's position after maturing"
        );
    }

    #[test]
    fn sapling_matures_into_berry_bush_after_mature_at_ticks() {
        let mut world = TestWorld::with_seed(2);
        let pos = Vec2::new(40.0, 60.0);
        let mature_at = 3.0;
        world.spawn_sapling(pos, Concept::BerryBush, mature_at);

        world.tick((60.0 * mature_at) as u64 + 5);

        let app_world = world.app_mut().world_mut();
        let mut entity_types = app_world.query::<(&EntityType, &Transform)>();
        let bush_count = entity_types
            .iter(app_world)
            .filter(|(etype, transform)| {
                etype.0 == Concept::BerryBush
                    && (transform.translation.truncate() - pos).length() < 0.1
            })
            .count();
        assert_eq!(
            bush_count, 1,
            "expected exactly one BerryBush at sapling pos"
        );
    }

    #[test]
    fn sapling_does_not_mature_before_threshold() {
        let mut world = TestWorld::with_seed(3);
        let pos = Vec2::new(50.0, 50.0);
        let mature_at = 10.0;
        let sapling = world.spawn_sapling(pos, Concept::AppleTree, mature_at);

        // Advance halfway: 60 * (mature_at / 2) ticks.
        world.tick(60 * 5);

        let app_world = world.app().world();
        let s = app_world
            .get::<Sapling>(sapling)
            .expect("sapling should still exist before maturing");
        assert!(
            s.growth_timer < s.mature_at,
            "growth_timer should not yet have crossed mature_at"
        );
        assert!(
            s.growth_timer > 0.0,
            "growth_timer should advance over time"
        );
    }

    #[test]
    fn maturing_sapling_emits_plant_matured_event() {
        let mut world = TestWorld::with_seed(4);
        let pos = Vec2::new(50.0, 50.0);
        let mature_at = 2.0;
        world.spawn_sapling(pos, Concept::AppleTree, mature_at);

        world.tick((60.0 * mature_at) as u64 + 5);

        let events = world.sim_events();
        let matured = events.all().iter().find_map(|e| match &e.kind {
            SimEventKind::PlantMatured {
                mature,
                matured_into,
            } => Some((*mature, *matured_into)),
            _ => None,
        });
        let (mature, matured_into) = matured.expect("expected PlantMatured event");
        assert_eq!(matured_into, Concept::AppleTree);
        // The mature entity should exist with an AppleTree EntityType.
        let app_world = world.app().world();
        let etype = app_world
            .get::<EntityType>(mature)
            .expect("mature entity should have EntityType");
        assert_eq!(etype.0, Concept::AppleTree);
    }
}
