//! Property components for world entities — auto-derived ontology traits.
//!
//! Reads:  [`EntityType`] on newly-spawned entities
//! Writes: [`Ontology`] (`HasTrait` and `Produces` triples)
//! Upstream: world spawners (apple_tree, campfire, etc.)
//! Downstream: agent perception, culture knowledge seeding

use bevy::prelude::*;

use crate::agent::inventory::EntityType;
use crate::agent::mind::knowledge::{Concept, Ontology};

// ─── Marker traits ──────────────────────────────────────────────────────────

/// Sealed marker trait: every property component created via
/// `define_property_component!` implements this automatically.
///
/// Any system that feeds a component into agent-facing reasoning can bound on
/// `IsRegisteredProperty` to get a compile-time guarantee that the corresponding
/// ontology triple will be registered.  A raw `#[derive(Component)]` struct
/// WITHOUT the macro will fail to satisfy this bound.
pub trait IsRegisteredProperty: Component {}

/// Maps a property component type to the [`Concept`] trait it asserts in the
/// ontology when added to an entity.
pub trait OntologyTrait: Component {
    fn concept_trait() -> Concept;
}

// ─── Macro ──────────────────────────────────────────────────────────────────

/// Define a property component that automatically registers an ontology trait.
///
/// Usage:
/// ```rust
/// define_property_component! {
///     /// Doc comment
///     pub struct LightSource {
///         pub radius: f32,
///         pub intensity: f32,
///     } => Concept::LightEmitting
/// }
/// ```
///
/// The macro generates:
/// 1. The struct with `#[derive(Component, Reflect, Debug, Clone)]` and `#[reflect(Component)]`
/// 2. `impl IsRegisteredProperty` — compile-time proof of registration
/// 3. `impl OntologyTrait` — maps to the concept trait
/// 4. A derive system `derive_ontology_<snake_name>` — fires on `Added<Self>`,
///    calls `ontology.ensure_trait(entity_type, concept_trait)` for every new entity
#[macro_export]
macro_rules! define_property_component {
    (
        $(#[$meta:meta])*
        pub struct $name:ident {
            $(pub $field:ident : $ftype:ty),* $(,)?
        } => $concept:expr
    ) => {
        $(#[$meta])*
        #[derive(Component, Reflect, Debug, Clone)]
        #[reflect(Component)]
        pub struct $name {
            $(pub $field: $ftype,)*
        }

        impl $crate::world::property::IsRegisteredProperty for $name {}

        impl $crate::world::property::OntologyTrait for $name {
            fn concept_trait() -> $crate::agent::mind::knowledge::Concept {
                $concept
            }
        }

        ::paste::paste! {
            pub fn [<derive_ontology_ $name:snake>](
                query: Query<&$crate::agent::inventory::EntityType, Added<$name>>,
                mut ontology: ResMut<$crate::agent::mind::knowledge::Ontology>,
            ) {
                for entity_type in query.iter() {
                    ontology.ensure_trait(entity_type.0, $concept);
                }
            }
        }
    };
}

// ─── Property components ─────────────────────────────────────────────────────

define_property_component! {
    /// Emits light in a radius. Perceived via visual sense.
    pub struct LightSource {
        pub radius: f32,
        pub intensity: f32,
    } => Concept::LightEmitting
}

define_property_component! {
    /// Emits heat in a radius. Affects warmth comfort of nearby agents.
    pub struct HeatSource {
        pub radius: f32,
        pub intensity: f32,
    } => Concept::HeatEmitting
}

define_property_component! {
    /// Provides shelter from weather. Reduces exposure to rain/cold when nearby.
    pub struct ShelterProvider {
        pub capacity: u32,
        pub protection: f32,
    } => Concept::ShelterProviding
}

define_property_component! {
    /// Can catch fire and burn. Consumed when ignited.
    pub struct Flammable {
        pub burn_time: f32,
    } => Concept::Flammable
}

define_property_component! {
    /// Consumes fuel to maintain function (e.g. campfire burns wood).
    /// When fuel_remaining reaches zero, LightSource and HeatSource are removed.
    pub struct FuelConsumer {
        pub fuel_type: Concept,
        pub fuel_remaining: f32,
        pub consumption_rate: f32,
    } => Concept::FuelConsuming
}

define_property_component! {
    /// Degrades over time. At zero durability, entity is despawned.
    /// Set decay_rate to 0.0 for indestructible entities (e.g. stone walls).
    pub struct Durability {
        pub current: f32,
        pub max: f32,
        pub decay_rate: f32,
    } => Concept::Degradable
}

// Harvestable is handled specially: it also derives a Produces triple from its
// `yields` field.  Written manually (not via the macro) because the macro only
// handles simple trait derivation; field-based derivation is the exception.

/// Marks an entity as harvestable and declares what it produces. Auto-derives
/// both the `Harvestable` trait and a `Produces` triple in the ontology.
///
/// Inventory count and regeneration live in `ItemSlots` + `ResourceRegeneration`.
#[derive(Component, Reflect, Debug, Clone)]
#[reflect(Component)]
pub struct HarvestableComponent {
    pub yields: Concept,
}

impl IsRegisteredProperty for HarvestableComponent {}

impl OntologyTrait for HarvestableComponent {
    fn concept_trait() -> Concept {
        Concept::Harvestable
    }
}

pub fn derive_ontology_harvestable_component(
    query: Query<(&EntityType, &HarvestableComponent), Added<HarvestableComponent>>,
    mut ontology: ResMut<Ontology>,
) {
    for (entity_type, harvestable) in query.iter() {
        ontology.ensure_trait(entity_type.0, Concept::Harvestable);
        ontology.ensure_production(entity_type.0, harvestable.yields);
    }
}

/// Marks an entity as having been built by an agent (vs spawned by world generation).
/// Used for ownership, territory, and knowledge ("Alice built this").
/// Natural entities (trees, caves) do not have this component.
#[derive(Component, Reflect, Debug, Clone)]
#[reflect(Component)]
pub struct BuiltBy {
    pub builder: Entity,
    /// Tick at which the entity was constructed.
    pub built_at: u64,
}

impl IsRegisteredProperty for BuiltBy {}

impl OntologyTrait for BuiltBy {
    fn concept_trait() -> Concept {
        Concept::ManMade
    }
}

pub fn derive_ontology_built_by(
    query: Query<&EntityType, Added<BuiltBy>>,
    mut ontology: ResMut<Ontology>,
) {
    for entity_type in query.iter() {
        ontology.ensure_trait(entity_type.0, Concept::ManMade);
    }
}

// ─── Systems ─────────────────────────────────────────────────────────────────

/// Ticks all [`FuelConsumer`] entities. Decrements fuel per tick.
///
/// When `fuel_remaining` hits zero, the system tries to auto-reload from
/// the entity's [`ItemSlots`] fuel slot. If a matching item is found, it is
/// consumed and `fuel_remaining` is refilled by [`FUEL_PER_WOOD`]. If no
/// items remain, the entity's light, heat, and comfort aura are removed and
/// a [`Becomes`] component targeting Ash is inserted so the entity
/// transforms on the next tick.
pub fn fuel_system(
    mut commands: Commands,
    mut query: Query<(
        Entity,
        &mut FuelConsumer,
        Option<&mut crate::agent::item_slots::ItemSlots>,
    )>,
) {
    use crate::world::becomes::{Becomes, BecomesTrigger};
    use crate::world::campfire::FUEL_PER_WOOD;
    use crate::world::emits_effect::EmitsEffect;

    for (entity, mut consumer, slots) in query.iter_mut() {
        if consumer.fuel_remaining <= 0.0 {
            continue;
        }
        consumer.fuel_remaining -= consumer.consumption_rate;
        if consumer.fuel_remaining <= 0.0 {
            consumer.fuel_remaining = 0.0;

            let reloaded = slots.is_some_and(|mut slots| {
                slots.remove_thing_unchecked(consumer.fuel_type).is_some()
            });

            if reloaded {
                consumer.fuel_remaining = FUEL_PER_WOOD;
            } else {
                commands.entity(entity).remove::<LightSource>();
                commands.entity(entity).remove::<HeatSource>();
                commands.entity(entity).remove::<EmitsEffect>();
                commands.entity(entity).insert(Becomes::new(
                    Concept::Ash,
                    BecomesTrigger::AfterTicks(0),
                    0,
                ));
            }
        }
    }
}

/// Ticks all [`Durability`] entities. Decrements current by decay_rate per tick.
/// Despawns the entity when current reaches zero.
pub fn durability_system(mut commands: Commands, mut query: Query<(Entity, &mut Durability)>) {
    for (entity, mut durability) in query.iter_mut() {
        if durability.decay_rate <= 0.0 {
            continue;
        }
        durability.current -= durability.decay_rate;
        if durability.current <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// Applies a shelter quality bonus to sleeping agents within range of a [`ShelterProvider`].
///
/// The effort model already restores aerobic stamina while Sleep is active.
/// This system adds an additional recovery bonus scaled by the shelter's
/// quality multiplier when the sleeping agent is near one.
pub fn shelter_system(
    shelter_providers: Query<(&Transform, &ShelterProvider)>,
    mut agents: Query<
        (
            &Transform,
            &mut crate::agent::body::needs::PhysicalNeeds,
            &crate::agent::actions::ActiveActions,
        ),
        With<crate::agent::Agent>,
    >,
) {
    use crate::agent::actions::ActionType;

    for (agent_transform, mut needs, active) in agents.iter_mut() {
        if !active.contains(ActionType::Sleep) {
            continue;
        }
        let agent_pos = agent_transform.translation.truncate();

        // Find the best nearby shelter (highest quality within capacity).
        let best_quality = shelter_providers
            .iter()
            .filter(|(shelter_transform, _)| {
                let shelter_pos = shelter_transform.translation.truncate();
                // Use TILE_SIZE * 3 as the "within shelter" range — close enough to benefit.
                agent_pos.distance(shelter_pos) <= crate::world::map::TILE_SIZE * 3.0
            })
            .map(|(_, provider)| provider.protection)
            .fold(0.0_f32, f32::max);

        if best_quality > 0.0 {
            // Add bonus aerobic recovery: quality acts as multiplier on a small base bonus.
            // Base bonus is 0.1 aerobic/tick at quality 1.0, scales linearly.
            // Shelters aid sustained rest (aerobic), not sprint reserves.
            needs.stamina.adjust_aerobic(best_quality * 0.1);
        }
    }
}

// ─── Plugin ──────────────────────────────────────────────────────────────────

/// Registers all ontology-derivation systems generated by `define_property_component!`.
///
/// Each system fires on `Added<Component>` so derivation works for entities
/// spawned at startup AND mid-game.  `ensure_trait`/`ensure_production` are
/// idempotent, so multiple spawns of the same entity type are safe.
pub struct OntologyDerivationPlugin;

impl Plugin for OntologyDerivationPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<LightSource>()
            .register_type::<HeatSource>()
            .register_type::<ShelterProvider>()
            .register_type::<Flammable>()
            .register_type::<FuelConsumer>()
            .register_type::<Durability>()
            .register_type::<BuiltBy>()
            .register_type::<HarvestableComponent>()
            .add_systems(
                Update,
                (
                    derive_ontology_light_source,
                    derive_ontology_heat_source,
                    derive_ontology_shelter_provider,
                    derive_ontology_flammable,
                    derive_ontology_fuel_consumer,
                    derive_ontology_durability,
                    derive_ontology_built_by,
                    derive_ontology_harvestable_component,
                    fuel_system,
                    durability_system,
                    shelter_system,
                ),
            );
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mind::knowledge::{Ontology, setup_ontology};

    fn empty_ontology() -> Ontology {
        setup_ontology()
    }

    // ── ensure_trait ────────────────────────────────────────────────────────

    #[test]
    fn ensure_trait_adds_missing_trait() {
        let mut ontology = empty_ontology();
        assert!(!ontology.has_trait(Concept::Campfire, Concept::LightEmitting));
        ontology.ensure_trait(Concept::Campfire, Concept::LightEmitting);
        assert!(ontology.has_trait(Concept::Campfire, Concept::LightEmitting));
    }

    #[test]
    fn ensure_trait_is_idempotent() {
        let mut ontology = empty_ontology();
        let count_before = ontology.triples.len();
        ontology.ensure_trait(Concept::Campfire, Concept::LightEmitting);
        let count_after_first = ontology.triples.len();
        ontology.ensure_trait(Concept::Campfire, Concept::LightEmitting); // duplicate
        let count_after_second = ontology.triples.len();
        assert_eq!(count_before + 1, count_after_first);
        assert_eq!(
            count_after_first, count_after_second,
            "duplicate call must not add a second triple"
        );
    }

    // ── ensure_production ───────────────────────────────────────────────────

    #[test]
    fn ensure_production_adds_produces_triple() {
        let mut ontology = empty_ontology();
        ontology.ensure_production(Concept::BerryBush, Concept::Berry);
        let exists = ontology.triples.iter().any(|t| {
            t.subject == crate::agent::mind::knowledge::Node::Concept(Concept::BerryBush)
                && t.predicate == crate::agent::mind::knowledge::Predicate::Produces
                && t.object == crate::agent::mind::knowledge::Value::Concept(Concept::Berry)
        });
        assert!(
            exists,
            "Produces triple should exist after ensure_production"
        );
    }

    #[test]
    fn ensure_production_is_idempotent() {
        let mut ontology = empty_ontology();
        ontology.ensure_production(Concept::BerryBush, Concept::Berry);
        let count = ontology.triples.len();
        ontology.ensure_production(Concept::BerryBush, Concept::Berry); // duplicate
        assert_eq!(
            count,
            ontology.triples.len(),
            "duplicate call must not add a second triple"
        );
    }

    // ── Trait impls ─────────────────────────────────────────────────────────

    #[test]
    fn light_source_implements_is_registered_property() {
        fn requires_registered<T: IsRegisteredProperty>() {}
        requires_registered::<LightSource>();
    }

    #[test]
    fn heat_source_concept_trait_is_heat_emitting() {
        assert_eq!(HeatSource::concept_trait(), Concept::HeatEmitting);
    }

    #[test]
    fn harvestable_component_implements_is_registered_property() {
        fn requires_registered<T: IsRegisteredProperty>() {}
        requires_registered::<HarvestableComponent>();
    }
}
