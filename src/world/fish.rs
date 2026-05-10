//! Aquatic agents. Carry the full agent stack but `max_plan_depth = 1`
//! caps GOAP at single-action plans, so locomotion is driven by
//! `world::fish_movement::swim_fish` (Boids for schooling, wander +
//! shore-avoid for everyone).

use crate::agent::biology::body::BodyNodeKind;
use crate::agent::body::genetics::founder::random_genome;
use crate::agent::body::species::{Species, SpeciesProfile};
use crate::agent::mind::knowledge::{Concept, MindGraph, Ontology};
use crate::agent::naming::{minnow_name, pike_name};
use crate::agent::{Agent, Alive, inventory::EntityType, item_slots::ItemSlots};
use crate::markings::{Markings, apply_markings};
use crate::palette::PaletteColor;
use crate::silhouette::{CreatureSilhouette, PartRole, Shape, SilhouettePart};
use crate::ui::sprite_animation::MovementAnimationGait;
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

/// Boids steering parameters. Present only on schooling species — the
/// swim system filters on `With<Schooling>` so solitary species opt out.
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct Schooling {
    pub neighbour_radius: f32,
    pub separation_radius: f32,
    pub separation_weight: f32,
    pub alignment_weight: f32,
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

/// Per-fish swim state — unit heading + speed in world pixels per tick.
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

/// Per-individual size + speed jitter so a school doesn't read as a
/// clone army. Body color is canonical per species; markings handle the
/// per-individual shading.
#[derive(Component, Clone, Debug)]
pub struct FishVariant {
    pub size_scale: f32,
    pub speed_jitter: f32,
}

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

const MINNOW_BODY_COLOR: PaletteColor = PaletteColor::FurWhite;
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

fn add_fish_innate_knowledge(mind: &mut MindGraph) {
    use crate::agent::mind::knowledge::{Metadata, Node, Predicate, Triple, Value};
    mind.assert(Triple::with_meta(
        Node::Concept(Concept::Person),
        Predicate::HasTrait,
        Value::Concept(Concept::Dangerous),
        Metadata::default(),
    ));
}

fn add_minnow_innate_knowledge(mind: &mut MindGraph) {
    use crate::agent::mind::knowledge::{Metadata, Node, Predicate, Triple, Value};
    add_fish_innate_knowledge(mind);
    mind.assert(Triple::with_meta(
        Node::Concept(Concept::Pike),
        Predicate::HasTrait,
        Value::Concept(Concept::Dangerous),
        Metadata::default(),
    ));
}

pub fn spawn_minnow<R: Rng>(
    commands: &mut Commands,
    ontology: Ontology,
    position: Vec2,
    index: usize,
    rng: &mut R,
) -> Entity {
    let species_profile = SpeciesProfile::minnow();
    let variant = roll_variant(&MINNOW_SPEC, rng);
    let genome = random_genome(rng, Species::Minnow);
    let markings = Markings::from_genome(&genome);

    let silhouette = apply_markings(minnow_silhouette(MINNOW_BODY_COLOR), &markings);

    let mut mind = MindGraph::new(ontology);
    add_minnow_innate_knowledge(&mut mind);

    let heading = random_unit_vec(rng);
    let speed = species_profile.base_speed * variant.speed_jitter;
    let vision_range = species_profile.vision_range;

    commands
        .spawn((
            Name::new(minnow_name(index)),
            Agent,
            Alive,
            // Fish markers grouped under one bundle slot to stay under
            // Bevy's 16-element cap.
            (
                Fish,
                Minnow,
                Schooling::default(),
                FishHeading::new(heading, speed),
                variant,
                MovementAnimationGait::Glide,
            ),
            EntityType(Concept::Minnow),
            species_profile,
            crate::world::Physical,
            crate::agent::TargetPosition::default(),
            crate::agent::movement::MovementState::default(),
            ItemSlots::agent_carry(),
            genome,
            Transform::from_translation(position.extend(3.0)),
            GlobalTransform::default(),
        ))
        .insert((
            crate::agent::affordance::Affordance::default(),
            mind,
            crate::agent::mind::perception::Vision {
                range: vision_range,
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

pub fn spawn_pike<R: Rng>(
    commands: &mut Commands,
    ontology: Ontology,
    position: Vec2,
    index: usize,
    rng: &mut R,
) -> Entity {
    let species_profile = SpeciesProfile::pike();
    let variant = roll_variant(&PIKE_SPEC, rng);
    let genome = random_genome(rng, Species::Pike);
    let markings = Markings::from_genome(&genome);

    let silhouette = apply_markings(pike_silhouette(PIKE_BODY_COLOR), &markings);

    let mut mind = MindGraph::new(ontology);
    add_fish_innate_knowledge(&mut mind);

    let heading = random_unit_vec(rng);
    let speed = species_profile.base_speed * variant.speed_jitter;
    let vision_range = species_profile.vision_range;

    commands
        .spawn((
            Name::new(pike_name(index)),
            Agent,
            Alive,
            (
                Fish,
                Pike,
                FishHeading::new(heading, speed),
                variant,
                MovementAnimationGait::Glide,
            ),
            EntityType(Concept::Pike),
            species_profile,
            crate::world::Physical,
            crate::agent::TargetPosition::default(),
            crate::agent::movement::MovementState::default(),
            ItemSlots::agent_carry(),
            genome,
            Transform::from_translation(position.extend(3.0)),
            GlobalTransform::default(),
        ))
        .insert((
            crate::agent::affordance::Affordance::default(),
            mind,
            crate::agent::mind::perception::Vision {
                range: vision_range,
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
