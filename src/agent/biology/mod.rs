pub mod body;
pub mod combat;

use crate::agent::Agent;
use crate::agent::body::species::SpeciesProfile;
use crate::core::GameLog;
use bevy::prelude::*;

pub struct BiologyPlugin;

impl Plugin for BiologyPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<body::Body>()
            .register_type::<body::BodyNode>()
            .register_type::<body::BodyNodeKind>()
            .register_type::<body::Injury>()
            .register_type::<body::InjuryType>()
            .add_systems(
                Update,
                (
                    setup_biology,
                    (body::process_starvation, body::check_death).chain(),
                    body::process_healing,
                    // Combat resolution runs after action execution so it
                    // can read this frame's ActionCompleted messages.
                    combat::resolve_combat_hits
                        .after(crate::agent::nervous_system::execution::tick_actions),
                    combat::bleed_system,
                    combat::severance_system.after(combat::resolve_combat_hits),
                ),
            );
    }
}

/// Attach a species-appropriate `Body` to any new agent that doesn't already
/// have one. Runs for every `Agent` entity — including deer and wolves — so
/// animal anatomy is a first-class part of the ECS and channel queries can
/// rely on it existing. Without a `SpeciesProfile`, defaults to the human
/// template (matches legacy behaviour where `Body::default()` was human).
fn setup_biology(
    mut commands: Commands,
    query: Query<(Entity, Option<&SpeciesProfile>), (Added<Agent>, Without<body::Body>)>,
    mut game_log: ResMut<GameLog>,
) {
    for (entity, species) in query.iter() {
        let body = species
            .map(|s| body::Body::for_species(s.species))
            .unwrap_or_default();
        commands.entity(entity).insert(body);
        game_log.log_debug(format!("Biology initialized for agent {:?}", entity));
    }
}
