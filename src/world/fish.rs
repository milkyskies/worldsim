//! Fish: aquatic agents that swim in water bodies.
//!
//! Reads: SpeciesProfile (Minnow/Pike), Ontology
//! Writes: Fish/Minnow/Pike/Schooling/FishHeading components, MindGraph (innate water knowledge)
//! Upstream: world::spawner (calls spawn_minnow/spawn_pike), world::map (water tile placement)
//! Downstream: world::fish_movement (Boids + wander steers fish each tick)
//!
//! Fish carry the full agent stack so they remain "real agents" — same brain,
//! mind, perception, drives — but their `SpeciesProfile.max_plan_depth = 1`
//! caps the planner at single-action plans, so in practice the swim-movement
//! system drives them. Schooling fish (Minnow) carry a `Schooling` component
//! that Boids reads; solitary fish (Pike) don't.

use crate::agent::biology::body::BodyNodeKind;
use crate::agent::body::genetics::founder::random_genome;
use crate::agent::body::species::{Species, SpeciesProfile};
use crate::agent::mind::knowledge::{Concept, MindGraph, Ontology};
use crate::agent::naming::{minnow_name, pike_name};
use crate::agent::{Agent, Alive, inventory::EntityType, item_slots::ItemSlots};
use crate::markings::{Markings, apply_markings};
use crate::palette::PaletteColor;
use crate::silhouette::{CreatureSilhouette, PartRole, Shape, SilhouettePart};
use bevy::prelude::*;
use rand::Rng;

/// Marker for any fish entity — used by the swim system to scope its query
/// without caring which species the fish is.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Fish;

/// Schooling forage fish.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Minnow;

/// Solitary freshwater predator. Eats minnows.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Pike;

/// Boids steering parameters. Attached only to species that school
/// (Minnow). The swim system filters on `With<Schooling>` so solitary
/// species (Pike) automatically opt out of flocking.
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct Schooling {
    /// World-pixel radius within which neighbours are considered school-mates.
    pub neighbour_radius: f32,
    /// Personal-space radius — closer neighbours push back.
    pub separation_radius: f32,
    /// Weight on the "steer away from crowded neighbours" rule.
    pub separation_weight: f32,
    /// Weight on the "match average heading of neighbours" rule.
    pub alignment_weight: f32,
    /// Weight on the "drift toward neighbour centroid" rule.
    pub cohesion_weight: f32,
}

impl Default for Schooling {
    fn default() -> Self {
        Self {
            neighbour_radius: 64.0,
            separation_radius: 14.0,
            separation_weight: 1.6,
            alignment_weight: 1.0,
            cohesion_weight: 0.8,
        }
    }
}

/// Per-fish swim state. Heading is the unit vector the fish is currently
/// facing; speed is in world pixels per tick.
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct FishHeading {
    pub heading: Vec2,
    pub speed: f32,
}

impl FishHeading {
    pub fn new(heading: Vec2, speed: f32) -> Self {
        let len = heading.length();
        let unit = if len > 1e-4 { heading / len } else { Vec2::X };
        Self {
            heading: unit,
            speed,
        }
    }
}

/// Per-individual cosmetic + cognitive jitter. A school of minnows uses the
/// same `Schooling` defaults and the same body color but each fish picks its
/// own size scale and speed jitter, so they're individually distinct without
/// looking like a different species. Built once at spawn from the per-species
/// [`FishVariantSpec`]. Genome-driven markings handle the per-individual
/// shade variation on top of the species color.
#[derive(Component, Clone, Debug)]
pub struct FishVariant {
    /// Multiplier on the species silhouette. Minnow ~0.85..1.15, Pike ~0.9..1.25.
    pub size_scale: f32,
    /// Speed jitter — multiplies `SpeciesProfile.base_speed` so two
    /// minnows aren't bit-identical.
    pub speed_jitter: f32,
}

/// Per-species variation knobs the spawner draws against. Body color is a
/// single canonical pick per species so a school reads as one species at a
/// glance — only size and speed vary per individual. Adding a new fish means
/// adding another spec.
struct FishVariantSpec {
    size_min: f32,
    size_max: f32,
    speed_jitter_min: f32,
    speed_jitter_max: f32,
}

const MINNOW_SPEC: FishVariantSpec = FishVariantSpec {
    size_min: 0.85,
    size_max: 1.15,
    speed_jitter_min: 0.9,
    speed_jitter_max: 1.1,
};

const PIKE_SPEC: FishVariantSpec = FishVariantSpec {
    size_min: 0.9,
    size_max: 1.25,
    speed_jitter_min: 0.95,
    speed_jitter_max: 1.15,
};

/// Canonical body color for every minnow. Light grey-silver reads as a
/// shimmery shoaling fish against blue water; markings add per-individual
/// shading on top.
const MINNOW_BODY_COLOR: PaletteColor = PaletteColor::FurWhite;

/// Canonical body color for every pike. Dark olive reads as a predator
/// silhouette in the water.
const PIKE_BODY_COLOR: PaletteColor = PaletteColor::FurSlate;

fn roll_variant<R: Rng>(spec: &FishVariantSpec, rng: &mut R) -> FishVariant {
    FishVariant {
        size_scale: rng.random_range(spec.size_min..spec.size_max),
        speed_jitter: rng.random_range(spec.speed_jitter_min..spec.speed_jitter_max),
    }
}

/// Canonical minnow silhouette - small slender body, simple tail fin, single
/// eye. Two-tone so schooling fish read as a shimmery cloud against water.
pub fn minnow_silhouette(body_color: PaletteColor) -> CreatureSilhouette {
    CreatureSilhouette {
        parts: vec![
            // Body — slender ellipse.
            SilhouettePart {
                body_node: Some(BodyNodeKind::Torso),
                shape: Shape::Ellipse,
                size: Vec2::new(7.0, 2.6),
                offset: Vec2::ZERO,
                rotation: 0.0,
                color: body_color,
                z_bias: 0,
                role: PartRole::Body,
                tint_with_environment: false,
            },
            // Tail fin — small triangle at the back.
            SilhouettePart {
                body_node: None,
                shape: Shape::Triangle,
                size: Vec2::new(2.5, 2.2),
                offset: Vec2::new(-4.0, 0.0),
                rotation: std::f32::consts::FRAC_PI_2,
                color: body_color,
                z_bias: 0,
                role: PartRole::Tail,
                tint_with_environment: false,
            },
            // Eye — single dark dot near the head.
            SilhouettePart {
                body_node: None,
                shape: Shape::Circle,
                size: Vec2::new(0.7, 0.7),
                offset: Vec2::new(2.6, 0.5),
                rotation: 0.0,
                color: PaletteColor::FurBlack,
                z_bias: 2,
                role: PartRole::Eye,
                tint_with_environment: false,
            },
        ],
        shadow_size: Vec2::new(7.0, 2.0),
        shadow_offset_y: -2.0,
        hop_phase: 0.0,
    }
}

/// Canonical pike silhouette - long lean body with pronounced snout, sharper
/// tail. Reads as a dark torpedo shape.
pub fn pike_silhouette(body_color: PaletteColor) -> CreatureSilhouette {
    CreatureSilhouette {
        parts: vec![
            // Body — long ellipse.
            SilhouettePart {
                body_node: Some(BodyNodeKind::Torso),
                shape: Shape::Ellipse,
                size: Vec2::new(14.0, 3.4),
                offset: Vec2::ZERO,
                rotation: 0.0,
                color: body_color,
                z_bias: 0,
                role: PartRole::Body,
                tint_with_environment: false,
            },
            // Snout — pointier head end.
            SilhouettePart {
                body_node: Some(BodyNodeKind::Head),
                shape: Shape::Triangle,
                size: Vec2::new(3.0, 2.6),
                offset: Vec2::new(7.5, 0.0),
                rotation: -std::f32::consts::FRAC_PI_2,
                color: body_color,
                z_bias: 1,
                role: PartRole::Snout,
                tint_with_environment: false,
            },
            // Tail fin — split fan.
            SilhouettePart {
                body_node: None,
                shape: Shape::Triangle,
                size: Vec2::new(3.4, 3.0),
                offset: Vec2::new(-8.0, 0.0),
                rotation: std::f32::consts::FRAC_PI_2,
                color: body_color,
                z_bias: 0,
                role: PartRole::Tail,
                tint_with_environment: false,
            },
            // Eye.
            SilhouettePart {
                body_node: None,
                shape: Shape::Circle,
                size: Vec2::new(0.9, 0.9),
                offset: Vec2::new(5.5, 0.6),
                rotation: 0.0,
                color: PaletteColor::FurBlack,
                z_bias: 2,
                role: PartRole::Eye,
                tint_with_environment: false,
            },
        ],
        shadow_size: Vec2::new(14.0, 3.0),
        shadow_offset_y: -2.0,
        hop_phase: 0.0,
    }
}

/// Add the innate fish knowledge: water is drinkable + survival-trait
/// associations. Predator fear is wired separately for prey species.
fn add_fish_innate_knowledge(mind: &mut MindGraph) {
    use crate::agent::mind::knowledge::{Metadata, Node, Predicate, Triple, Value};
    let meta = Metadata::default();
    // Fish know land creatures are dangerous (humans grab them, wolves hunt
    // them in the shallows). This reuses the existing fear-via-knowledge
    // pathway so a fish that perceives a Person flees just like a deer does.
    mind.assert(Triple::with_meta(
        Node::Concept(Concept::Person),
        Predicate::HasTrait,
        Value::Concept(Concept::Dangerous),
        meta.clone(),
    ));
}

/// Add the minnow-specific fear of pikes — without this they ignore the
/// predator that's literally hunting them.
fn add_minnow_innate_knowledge(mind: &mut MindGraph) {
    use crate::agent::mind::knowledge::{Metadata, Node, Predicate, Triple, Value};
    add_fish_innate_knowledge(mind);
    let meta = Metadata::default();
    mind.assert(Triple::with_meta(
        Node::Concept(Concept::Pike),
        Predicate::HasTrait,
        Value::Concept(Concept::Dangerous),
        meta,
    ));
}

/// Spawn a Minnow at `position`. Fish entities carry the full agent stack so
/// they remain real agents — perception, mind, brain, drives — but the
/// `Schooling` component opts them into Boids steering on top of the swim
/// wander baseline.
pub fn spawn_minnow<R: Rng>(
    commands: &mut Commands,
    ontology: Ontology,
    position: Vec2,
    index: usize,
    rng: &mut R,
) -> Entity {
    let species_profile = SpeciesProfile::minnow();
    let variant = roll_variant(&MINNOW_SPEC, rng);
    let inventory = ItemSlots::agent_carry();
    let genome = random_genome(rng, Species::Minnow);
    let markings = Markings::from_genome(&genome);

    let silhouette = apply_markings(minnow_silhouette(MINNOW_BODY_COLOR), &markings);

    let mut mind = MindGraph::new(ontology);
    add_minnow_innate_knowledge(&mut mind);

    let heading = random_unit_vec(rng);
    let speed = species_profile.base_speed * variant.speed_jitter;

    commands
        .spawn((
            Name::new(minnow_name(index)),
            Agent,
            Alive,
            // Fish-specific markers + per-tick state grouped in one slot to
            // stay under Bevy's 16-element bundle cap. `MovementAnimationGait::Glide`
            // suppresses the bouncing hop animation that's the default for
            // land creatures so fish read as swimming rather than hopping.
            (
                Fish,
                Minnow,
                Schooling::default(),
                FishHeading::new(heading, speed),
                variant,
                crate::ui::sprite_animation::MovementAnimationGait::Glide,
            ),
            EntityType(Concept::Minnow),
            species_profile,
            crate::world::Physical,
            crate::agent::TargetPosition::default(),
            crate::agent::movement::MovementState::default(),
            inventory,
            genome,
            Transform::from_translation(position.extend(3.0)),
            GlobalTransform::default(),
        ))
        .insert((
            crate::agent::affordance::Affordance::default(),
            mind,
            crate::agent::mind::perception::Vision {
                range: SpeciesProfile::minnow().vision_range,
            },
            crate::agent::mind::perception::VisibleObjects::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
            crate::ui::sprite_animation::VisualOffset::default(),
            markings,
            silhouette,
        ))
        .insert((
            crate::agent::mind::memory::WorkingMemory::default(),
            crate::agent::brains::rational::RationalBrain,
            crate::agent::brains::plan_memory::PlanMemory::default(),
            crate::agent::brains::proposal::BrainState::default(),
            crate::agent::nervous_system::cns::CentralNervousSystem::default(),
            crate::agent::body::needs::PhysicalNeeds::default(),
            crate::agent::body::needs::Consciousness::default(),
            crate::agent::body::needs::PsychologicalDrives::default(),
            crate::agent::actions::ActiveActions::default(),
            crate::agent::psyche::emotions::EmotionalState::default(),
            crate::agent::skills::Skills::default(),
        ))
        .id()
}

/// Spawn a Pike at `position`. Pikes are solitary, so they get no `Schooling`
/// component — the Boids system filters them out and they fall back to the
/// wander baseline, occasionally striking at minnows in vision range.
pub fn spawn_pike<R: Rng>(
    commands: &mut Commands,
    ontology: Ontology,
    position: Vec2,
    index: usize,
    rng: &mut R,
) -> Entity {
    let species_profile = SpeciesProfile::pike();
    let variant = roll_variant(&PIKE_SPEC, rng);
    let inventory = ItemSlots::agent_carry();
    let genome = random_genome(rng, Species::Pike);
    let markings = Markings::from_genome(&genome);

    let silhouette = apply_markings(pike_silhouette(PIKE_BODY_COLOR), &markings);

    let mut mind = MindGraph::new(ontology);
    add_fish_innate_knowledge(&mut mind);

    let heading = random_unit_vec(rng);
    let speed = species_profile.base_speed * variant.speed_jitter;

    commands
        .spawn((
            Name::new(pike_name(index)),
            Agent,
            Alive,
            // Fish-specific markers grouped under one bundle slot.
            // `MovementAnimationGait::Glide` suppresses the bouncing hop animation.
            (
                Fish,
                Pike,
                FishHeading::new(heading, speed),
                variant,
                crate::ui::sprite_animation::MovementAnimationGait::Glide,
            ),
            EntityType(Concept::Pike),
            species_profile,
            crate::world::Physical,
            crate::agent::TargetPosition::default(),
            crate::agent::movement::MovementState::default(),
            inventory,
            genome,
            Transform::from_translation(position.extend(3.0)),
            GlobalTransform::default(),
        ))
        .insert((
            crate::agent::affordance::Affordance::default(),
            mind,
            crate::agent::mind::perception::Vision {
                range: SpeciesProfile::pike().vision_range,
            },
            crate::agent::mind::perception::VisibleObjects::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
            crate::ui::sprite_animation::VisualOffset::default(),
            markings,
            silhouette,
        ))
        .insert((
            crate::agent::mind::memory::WorkingMemory::default(),
            crate::agent::brains::rational::RationalBrain,
            crate::agent::brains::plan_memory::PlanMemory::default(),
            crate::agent::brains::proposal::BrainState::default(),
            crate::agent::nervous_system::cns::CentralNervousSystem::default(),
            crate::agent::body::needs::PhysicalNeeds::default(),
            crate::agent::body::needs::Consciousness::default(),
            crate::agent::body::needs::PsychologicalDrives::default(),
            crate::agent::actions::ActiveActions::default(),
            crate::agent::psyche::emotions::EmotionalState::default(),
            crate::agent::skills::Skills::default(),
        ))
        .id()
}

fn random_unit_vec<R: Rng>(rng: &mut R) -> Vec2 {
    let theta = rng.random_range(0.0..std::f32::consts::TAU);
    Vec2::new(theta.cos(), theta.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minnow_profile_has_extreme_low_cognition() {
        let p = SpeciesProfile::minnow();
        assert_eq!(p.species, Species::Minnow);
        assert_eq!(p.max_plan_depth, 1, "minnow must be plan-depth 1");
        assert!(p.memory_capacity <= 30, "minnow memory must be tiny");
        assert!(p.memory_decay_rate >= 0.7, "minnow memory must decay fast");
        assert!(
            p.rational_weight <= 0.05,
            "minnow rational influence must be near-zero"
        );
        assert!(
            p.survival_weight >= 0.9,
            "minnow must be survival-dominated"
        );
    }

    #[test]
    fn pike_profile_has_predator_traits() {
        let p = SpeciesProfile::pike();
        assert_eq!(p.species, Species::Pike);
        assert_eq!(p.max_plan_depth, 1, "pike must be plan-depth 1");
        assert!(matches!(
            p.diet,
            crate::agent::body::species::Diet::Carnivore
        ));
        assert!(
            p.base_speed > SpeciesProfile::minnow().base_speed,
            "pike must out-burst a minnow"
        );
    }

    #[test]
    fn schooling_default_is_balanced_and_positive() {
        let s = Schooling::default();
        assert!(s.neighbour_radius > 0.0);
        assert!(s.separation_radius > 0.0);
        assert!(s.separation_radius < s.neighbour_radius);
        for w in [s.separation_weight, s.alignment_weight, s.cohesion_weight] {
            assert!(w > 0.0, "boids weight must be positive: {w}");
        }
    }

    #[test]
    fn fish_heading_normalizes_input() {
        let h = FishHeading::new(Vec2::new(3.0, 4.0), 2.0);
        assert!((h.heading.length() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn fish_heading_handles_zero_vector() {
        let h = FishHeading::new(Vec2::ZERO, 1.0);
        assert!((h.heading.length() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn variation_produces_distinct_fish_within_one_species_color() {
        // A school's individual fish must vary in size and speed (so they
        // don't read as a clone army) but the species color is canonical
        // — only `MINNOW_BODY_COLOR` is rendered for every minnow.
        use rand::SeedableRng;
        use rand_chacha::ChaCha8Rng;
        let mut rng = ChaCha8Rng::seed_from_u64(42);
        let variants: Vec<FishVariant> = (0..16)
            .map(|_| roll_variant(&MINNOW_SPEC, &mut rng))
            .collect();
        let unique_sizes: std::collections::HashSet<_> = variants
            .iter()
            .map(|v| (v.size_scale * 1000.0) as i32)
            .collect();
        assert!(
            unique_sizes.len() > 1,
            "16 minnows must not all share one size"
        );
        let unique_speeds: std::collections::HashSet<_> = variants
            .iter()
            .map(|v| (v.speed_jitter * 1000.0) as i32)
            .collect();
        assert!(
            unique_speeds.len() > 1,
            "16 minnows must not all share one speed jitter"
        );
    }
}
