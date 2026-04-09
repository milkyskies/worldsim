pub mod apple_tree;
pub mod berry_bush;
pub mod campfire;
pub mod deer;
pub mod environment;
pub mod human;
pub mod map;
pub mod spawn_config;
pub mod spawn_placement;
pub mod spawner;
pub mod stone_node;
pub mod wood_log;

use bevy::prelude::*;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Physical>()
            .add_plugins(map::MapPlugin)
            .add_plugins(environment::EnvironmentPlugin)
            .add_plugins(spawner::SpawnerPlugin);
    }
}

#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Physical;
