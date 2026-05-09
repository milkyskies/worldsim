pub mod apple_tree;
pub mod becomes;
pub mod berry_bush;
pub mod campfire;
pub mod construction_site;
pub mod corpse;
pub mod deer;
pub mod emits_effect;
pub mod environment;
pub mod field_grid;
pub mod field_grid_plugin;
pub mod house;
pub mod human;
pub mod lean_to;
pub mod liquid;
pub mod map;
pub mod property;
pub mod sapling;
pub mod sense_sources;
pub mod severed_part;
pub mod spatial_index;
pub mod spawn;
pub mod spawn_config;
pub mod spawn_placement;
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
            .register_type::<emits_effect::EmitsEffect>()
            .register_type::<construction_site::ConstructionSiteMarker>()
            .register_type::<sense_sources::SoundSource>()
            .add_plugins(map::MapPlugin)
            .add_plugins(environment::EnvironmentPlugin)
            .add_plugins(spatial_index::SpatialIndexPlugin)
            .add_plugins(spawner::SpawnerPlugin)
            .add_plugins(property::OntologyDerivationPlugin)
            .add_plugins(field_grid_plugin::FieldGridPlugin)
            .add_plugins(liquid::LiquidPlugin)
            .add_plugins(severed_part::SeveredPartPlugin);
    }
}

#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Physical;
