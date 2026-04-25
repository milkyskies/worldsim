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
            .register_type::<body::FunctionalTag>()
            .register_type::<body::TagChannelMapping>()
            .register_type::<body::Injury>()
            .register_type::<body::InjuryType>()
            .init_resource::<body::TagChannelMapping>()
            .add_systems(
                FixedUpdate,
                (
                    setup_biology,
                    (
                        body::process_deprivation,
                        body::check_death,
                        body::process_healing,
                    )
                        .chain(),
                    combat::resolve_combat_hits
                        .after(crate::agent::nervous_system::execution::tick_actions),
                    combat::bleed_system,
                    combat::severance_system.after(combat::resolve_combat_hits),
                    derive_lameness.after(combat::resolve_combat_hits),
                    expire_dazed,
                ),
            );
    }
}

/// Toggle the [`crate::agent::Lame`] component based on leg-node HP.
/// Runs after combat resolution so a fresh leg injury is reflected in
/// the same tick. Emits `LamenessChanged` only on transition so the
/// event log doesn't churn every tick.
fn derive_lameness(
    mut commands: Commands,
    bodies: Query<(Entity, &body::Body, Option<&crate::agent::Lame>), With<Agent>>,
    tick: Res<crate::core::tick::TickCount>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
) {
    for (entity, body, current_lame) in bodies.iter() {
        let now_lame = body.is_lame();
        let was_lame = current_lame.is_some();
        if now_lame == was_lame {
            continue;
        }
        if now_lame {
            commands.entity(entity).insert(crate::agent::Lame);
        } else {
            commands.entity(entity).remove::<crate::agent::Lame>();
        }
        sim_events.write(crate::agent::events::SimEvent::single(
            tick.current,
            entity,
            crate::agent::events::SimEventKind::LamenessChanged {
                agent: entity,
                lame: now_lame,
            },
        ));
    }
}

/// Drop the [`crate::agent::Dazed`] component once its `until_tick` has
/// passed. Brain proposal layer reads `Dazed` and skips the agent's
/// proposal cycle while it's set.
fn expire_dazed(
    mut commands: Commands,
    dazed: Query<(Entity, &crate::agent::Dazed)>,
    tick: Res<crate::core::tick::TickCount>,
) {
    for (entity, daze) in dazed.iter() {
        if tick.current >= daze.until_tick {
            commands.entity(entity).remove::<crate::agent::Dazed>();
        }
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
