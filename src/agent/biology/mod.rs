pub mod body;

use crate::agent::Person;
use crate::core::GameLog;
use bevy::prelude::*;

pub struct BiologyPlugin;

impl Plugin for BiologyPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<body::Body>()
            .register_type::<body::BodyPart>()
            .register_type::<body::Injury>()
            .register_type::<body::InjuryType>()
            .add_systems(
                Update,
                (
                    setup_biology,
                    (body::process_starvation, body::check_death).chain(),
                    body::process_healing,
                ),
            );
    }
}

// Automatically add Biology components to any new Person
fn setup_biology(
    mut commands: Commands,
    query: Query<Entity, Added<Person>>,
    mut game_log: ResMut<GameLog>,
) {
    for entity in query.iter() {
        commands.entity(entity).insert(body::Body::default());
        game_log.log_debug(format!("Biology initialized for Person {:?}", entity));
    }
}
