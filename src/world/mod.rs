pub mod apple_tree;
pub mod becomes;
pub mod berry_bush;
pub mod campfire;
pub mod construction_site;
pub mod deer;
pub mod environment;
pub mod human;
pub mod map;
pub mod spawn;
pub mod spawn_config;
pub mod spawn_placement;
pub mod spatial_index;
pub mod spawner;
pub mod stone_node;
pub mod wolf;
pub mod wood_log;

use bevy::prelude::*;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Physical>()
            .register_type::<becomes::Becomes>()
            .register_type::<construction_site::ConstructionSiteMarker>()
            .add_plugins(map::MapPlugin)
            .add_plugins(environment::EnvironmentPlugin)
            .add_plugins(spatial_index::SpatialIndexPlugin)
            .add_plugins(spawner::SpawnerPlugin);
    }
}

#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Physical;
