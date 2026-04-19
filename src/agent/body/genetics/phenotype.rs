//! Phenotype component: traits derived from Genome + SpeciesProfile.
//!
//! Reads: Genome, SpeciesProfile (at spawn, via Added<Genome> trigger)
//! Writes: Phenotype (inserted), Personality (overwritten), Vision (overwritten)
//! Upstream: genetics::genome (Genome), body::species (SpeciesProfile)
//! Downstream: nervous_system::execution (speed multiplier),
//!             mind::perception (vision range), nervous_system (Personality)

use bevy::prelude::*;

use crate::agent::body::genetics::genome::{
    AEROBIC_CAPACITY_START, AGREEABLENESS_START, ANAEROBIC_CAPACITY_START, BMR_START,
    CONSCIENTIOUSNESS_START, DIGESTION_START, EXTRAVERSION_START, Genome, LOCI_PER_TRAIT, N_LOCI,
    NEUROTICISM_START, OPENNESS_START, SLEEP_EFFICIENCY_START, SPEED_START, VISION_START,
};
use crate::agent::body::needs::{PsychologicalDrives, SocialDriveOverride};
use crate::agent::body::species::SpeciesProfile;
use crate::agent::events::{SimEvent, SimEventKind};
use crate::agent::mind::perception::Vision;
use crate::agent::psyche::personality::{Personality, PersonalityTraits};
use crate::core::tick::TickCount;

/// Heritability for physical traits (speed, vision, metabolism, endurance).
///
/// Twin studies put physical heritability in the 0.6–0.8 range; 0.7 is the
/// conventional midpoint. Higher than personality because physical traits have
/// fewer environmental buffering pathways.
const H2_PHYSICAL: f32 = 0.7;

/// Heritability for Big Five personality traits.
///
/// Meta-analyses (e.g. Vukasović & Bratko 2015) consistently find ~0.4–0.5.
/// Using 0.5 means half of a trait score comes from genetics, half from the
/// species-neutral baseline — leaving room for environmental development in
/// future iterations.
const H2_PERSONALITY: f32 = 0.5;

/// Traits derived from the agent's genome, used by locomotion, perception, and drive systems.
///
/// Physical fields are multipliers on the species baseline (1.0 = exactly average).
/// Personality fields are 0..1 Big Five trait scores (0.5 = neutral).
///
/// Computed once at spawn by [`develop_phenotype_system`] and never modified
/// until reproduction wires in inheritance (issue #311).
#[derive(Component, Clone, Reflect, Debug, serde::Serialize)]
#[reflect(Component)]
pub struct Phenotype {
    // Physical multipliers (1.0 = species baseline)
    pub speed: f32,
    pub vision: f32,
    /// Digestion rate multiplier. High = fast digester (stomach empties
    /// quickly, glucose available sooner). Low = slow but fuel-efficient.
    pub digestion: f32,
    /// Base metabolic rate multiplier. High = burns more at rest but
    /// recovers stamina faster. Low = fuel-efficient, slow recovery.
    pub bmr: f32,
    /// Aerobic capacity multiplier. Scales aerobic stamina pool size.
    /// High = sustains effort longer (marathoner). Low = tires quickly.
    pub aerobic_capacity: f32,
    /// Anaerobic capacity multiplier. Scales anaerobic stamina pool size.
    /// High = longer sprints (sprinter). Low = burns out fast.
    pub anaerobic_capacity: f32,
    /// Sleep efficiency multiplier. Scales wakefulness restore rate during
    /// sleep. High = short sleeper (recovers fast). Low = needs more sleep.
    pub sleep_efficiency: f32,
    // Personality scores (0..1, centered at 0.5)
    pub openness: f32,
    pub conscientiousness: f32,
    pub extraversion: f32,
    pub agreeableness: f32,
    pub neuroticism: f32,
}

impl Default for Phenotype {
    fn default() -> Self {
        Self {
            speed: 1.0,
            vision: 1.0,
            digestion: 1.0,
            bmr: 1.0,
            aerobic_capacity: 1.0,
            anaerobic_capacity: 1.0,
            sleep_efficiency: 1.0,
            openness: 0.5,
            conscientiousness: 0.5,
            extraversion: 0.5,
            agreeableness: 0.5,
            neuroticism: 0.5,
        }
    }
}

impl Phenotype {
    pub fn from_genome(genome: &Genome) -> Self {
        Self {
            speed: develop_physical(genome.locus_sum(SPEED_START)),
            vision: develop_physical(genome.locus_sum(VISION_START)),
            digestion: develop_physical(genome.locus_sum(DIGESTION_START)),
            bmr: develop_physical(genome.locus_sum(BMR_START)),
            aerobic_capacity: develop_physical(genome.locus_sum(AEROBIC_CAPACITY_START)),
            anaerobic_capacity: develop_physical(genome.locus_sum(ANAEROBIC_CAPACITY_START)),
            sleep_efficiency: develop_physical(genome.locus_sum(SLEEP_EFFICIENCY_START)),
            openness: develop_personality(genome.locus_sum(OPENNESS_START)),
            conscientiousness: develop_personality(genome.locus_sum(CONSCIENTIOUSNESS_START)),
            extraversion: develop_personality(genome.locus_sum(EXTRAVERSION_START)),
            agreeableness: develop_personality(genome.locus_sum(AGREEABLENESS_START)),
            neuroticism: develop_personality(genome.locus_sum(NEUROTICISM_START)),
        }
    }
}

impl Genome {
    /// Build a genome whose phenotype development yields traits matching `target`.
    ///
    /// Inverse of [`Phenotype::from_genome`]. Used by tests and scenario builders
    /// that want an agent with specific trait values without touching genome loci
    /// directly.
    ///
    /// Because H2 < 1.0 dampens genetic influence, target values saturate near the
    /// edges of the representable range: physical multipliers in roughly
    /// `[0.65, 1.35]` (H2_PHYSICAL = 0.7) and personality scores in roughly
    /// `[0.25, 0.75]` (H2_PERSONALITY = 0.5). Targets outside those ranges are
    /// clamped to the nearest representable value.
    pub fn from_phenotype(target: &Phenotype) -> Self {
        let mut maternal = [0.0_f32; N_LOCI];
        let mut paternal = [0.0_f32; N_LOCI];

        fill_physical(&mut maternal, &mut paternal, SPEED_START, target.speed);
        fill_physical(&mut maternal, &mut paternal, VISION_START, target.vision);
        fill_physical(
            &mut maternal,
            &mut paternal,
            DIGESTION_START,
            target.digestion,
        );
        fill_physical(&mut maternal, &mut paternal, BMR_START, target.bmr);
        fill_physical(
            &mut maternal,
            &mut paternal,
            AEROBIC_CAPACITY_START,
            target.aerobic_capacity,
        );
        fill_physical(
            &mut maternal,
            &mut paternal,
            ANAEROBIC_CAPACITY_START,
            target.anaerobic_capacity,
        );
        fill_physical(
            &mut maternal,
            &mut paternal,
            SLEEP_EFFICIENCY_START,
            target.sleep_efficiency,
        );

        fill_personality(
            &mut maternal,
            &mut paternal,
            OPENNESS_START,
            target.openness,
        );
        fill_personality(
            &mut maternal,
            &mut paternal,
            CONSCIENTIOUSNESS_START,
            target.conscientiousness,
        );
        fill_personality(
            &mut maternal,
            &mut paternal,
            EXTRAVERSION_START,
            target.extraversion,
        );
        fill_personality(
            &mut maternal,
            &mut paternal,
            AGREEABLENESS_START,
            target.agreeableness,
        );
        fill_personality(
            &mut maternal,
            &mut paternal,
            NEUROTICISM_START,
            target.neuroticism,
        );

        Self { maternal, paternal }
    }
}

/// Maximum |tanh| we let the inverse reach, to keep `atanh` finite.
const INVERSE_CLAMP: f32 = 0.999;

/// Fill `LOCI_PER_TRAIT` loci on both haplotypes so the forward pass reproduces
/// the physical `target` multiplier (centered at 1.0).
fn fill_physical(
    maternal: &mut [f32; N_LOCI],
    paternal: &mut [f32; N_LOCI],
    start: usize,
    target: f32,
) {
    // Forward: target = 1.0 + H2_PHYSICAL * 0.5 * tanh(locus_sum / 2.0)
    let tanh_arg = ((target - 1.0) / (H2_PHYSICAL * 0.5)).clamp(-INVERSE_CLAMP, INVERSE_CLAMP);
    let locus_sum = 2.0 * tanh_arg.atanh();
    fill_loci(maternal, paternal, start, locus_sum);
}

/// Fill `LOCI_PER_TRAIT` loci on both haplotypes so the forward pass reproduces
/// the personality `target` score (centered at 0.5).
fn fill_personality(
    maternal: &mut [f32; N_LOCI],
    paternal: &mut [f32; N_LOCI],
    start: usize,
    target: f32,
) {
    // Forward: target = 0.5 + H2_PERSONALITY * 0.5 * tanh(locus_sum / 2.0)
    let tanh_arg = ((target - 0.5) / (H2_PERSONALITY * 0.5)).clamp(-INVERSE_CLAMP, INVERSE_CLAMP);
    let locus_sum = 2.0 * tanh_arg.atanh();
    fill_loci(maternal, paternal, start, locus_sum);
}

fn fill_loci(
    maternal: &mut [f32; N_LOCI],
    paternal: &mut [f32; N_LOCI],
    start: usize,
    locus_sum: f32,
) {
    // locus_sum is the total across 2 haplotypes × LOCI_PER_TRAIT loci.
    let per_locus = locus_sum / (2.0 * LOCI_PER_TRAIT as f32);
    for i in start..start + LOCI_PER_TRAIT {
        maternal[i] = per_locus;
        paternal[i] = per_locus;
    }
}

/// Map a raw locus sum to a physical trait multiplier centered at 1.0.
///
/// `locus_sum` is the sum of 2×LOCI_PER_TRAIT values, typically in (-4.0, 4.0)
/// for founder populations. The result is a multiplier in roughly (0.65, 1.35)
/// for one-sigma genomes, shrunk toward 1.0 by the heritability mix.
fn develop_physical(locus_sum: f32) -> f32 {
    // genetic_value: multiplier in (0.5, 1.5), centered at 1.0
    let genetic_value = 1.0 + 0.5 * (locus_sum / 2.0).tanh();
    // Blend with species mean (1.0) by heritability
    (1.0 - H2_PHYSICAL) * 1.0 + H2_PHYSICAL * genetic_value
}

/// Map a raw locus sum to a personality trait score in [0, 1] centered at 0.5.
fn develop_personality(locus_sum: f32) -> f32 {
    // genetic_value in (0.0, 1.0), centered at 0.5
    let genetic_value = 0.5 + 0.5 * (locus_sum / 2.0).tanh();
    // Blend with neutral personality (0.5) by heritability
    (1.0 - H2_PERSONALITY) * 0.5 + H2_PERSONALITY * genetic_value
}

/// Runs once per entity when a [`Genome`] is added (at spawn).
///
/// Inserts (or overwrites):
/// - [`Phenotype`] derived from the genome
/// - [`Vision`] with range scaled by `species.vision_range * phenotype.vision`
/// - [`Personality`] derived from the phenotype's personality fields
/// - [`PsychologicalDrives`] derived from the personality (so drives stay in
///   sync with genes)
///
/// If a [`SocialDriveOverride`] component is present, the derived drives have
/// their `social` field replaced with the override — tests use this to force
/// agents into loneliness regardless of their extraversion genome.
///
/// Emits [`SimEvent::PhenotypeDeveloped`] for observability.
pub fn develop_phenotype_system(
    mut commands: Commands,
    query: Query<
        (
            Entity,
            &Genome,
            &SpeciesProfile,
            Option<&SocialDriveOverride>,
        ),
        Added<Genome>,
    >,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<SimEvent>,
) {
    for (entity, genome, species, social_override) in query.iter() {
        let phenotype = Phenotype::from_genome(genome);

        let vision_range = species.vision_range * phenotype.vision;

        let personality = Personality {
            traits: PersonalityTraits {
                openness: phenotype.openness,
                conscientiousness: phenotype.conscientiousness,
                extraversion: phenotype.extraversion,
                agreeableness: phenotype.agreeableness,
                neuroticism: phenotype.neuroticism,
            },
        };

        let mut drives = PsychologicalDrives::from_personality(&personality.traits);
        if let Some(SocialDriveOverride(v)) = social_override {
            // Override stores legacy "social drive" semantics
            // (0 = satisfied, 1 = lonely). Invert to the new
            // satisfaction-based `companionship` field.
            drives.companionship.set(1.0 - *v);
        }

        sim_events.write(SimEvent::single(
            tick.current,
            entity,
            SimEventKind::PhenotypeDeveloped {
                agent: entity,
                phenotype: phenotype.clone(),
            },
        ));

        commands.entity(entity).insert((
            phenotype,
            Vision {
                range: vision_range,
            },
            personality,
            drives,
        ));
    }
}

/// Scale stamina pool sizes by genetic aerobic/anaerobic capacity at spawn.
///
/// Runs once when [`Phenotype`] is added (same frame as `develop_phenotype_system`).
/// High aerobic_capacity agents get larger aerobic reserves; high anaerobic_capacity
/// agents get larger sprint reserves. Both pools start full at the new max.
pub fn apply_stamina_genetics_system(
    mut query: Query<(&Phenotype, &mut crate::agent::body::needs::PhysicalNeeds), Added<Phenotype>>,
) {
    for (phenotype, mut physical) in query.iter_mut() {
        let aero_frac = physical.stamina.aerobic_fraction();
        physical.stamina.aerobic_max *= phenotype.aerobic_capacity;
        physical.stamina.aerobic = physical.stamina.aerobic_max * aero_frac;

        let anaero_frac = physical.stamina.anaerobic_fraction();
        physical.stamina.anaerobic_max *= phenotype.anaerobic_capacity;
        physical.stamina.anaerobic = physical.stamina.anaerobic_max * anaero_frac;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::body::genetics::genome::{Genome, N_PHYSICAL_LOCI};

    #[test]
    fn neutral_genome_produces_baseline_phenotype() {
        let g = Genome::default();
        let p = Phenotype::from_genome(&g);
        // All physical traits should be exactly 1.0 (species baseline)
        assert!((p.speed - 1.0).abs() < 1e-6, "speed={}", p.speed);
        assert!((p.vision - 1.0).abs() < 1e-6, "vision={}", p.vision);
        assert!(
            (p.digestion - 1.0).abs() < 1e-6,
            "digestion={}",
            p.digestion
        );
        assert!((p.bmr - 1.0).abs() < 1e-6, "bmr={}", p.bmr);
        assert!(
            (p.aerobic_capacity - 1.0).abs() < 1e-6,
            "aerobic_capacity={}",
            p.aerobic_capacity
        );
        assert!(
            (p.anaerobic_capacity - 1.0).abs() < 1e-6,
            "anaerobic_capacity={}",
            p.anaerobic_capacity
        );
        // All personality traits should be exactly 0.5 (neutral)
        assert!((p.openness - 0.5).abs() < 1e-6, "openness={}", p.openness);
        assert!((p.conscientiousness - 0.5).abs() < 1e-6);
        assert!((p.extraversion - 0.5).abs() < 1e-6);
        assert!((p.agreeableness - 0.5).abs() < 1e-6);
        assert!((p.neuroticism - 0.5).abs() < 1e-6);
    }

    #[test]
    fn positive_loci_produce_above_baseline_speed() {
        let mut g = Genome::default();
        // Set all 4 speed loci on both haplotypes to +1.0
        for i in 0..4 {
            g.maternal[i] = 1.0;
            g.paternal[i] = 1.0;
        }
        let p = Phenotype::from_genome(&g);
        assert!(
            p.speed > 1.0,
            "positive loci should produce speed > 1.0, got {}",
            p.speed
        );
    }

    #[test]
    fn negative_loci_produce_below_baseline_speed() {
        let mut g = Genome::default();
        for i in 0..4 {
            g.maternal[i] = -1.0;
            g.paternal[i] = -1.0;
        }
        let p = Phenotype::from_genome(&g);
        assert!(
            p.speed < 1.0,
            "negative loci should produce speed < 1.0, got {}",
            p.speed
        );
    }

    #[test]
    fn phenotype_speed_stays_in_reasonable_multiplier_range() {
        // Even with maximum possible locus values (+/- 10.0 per locus)
        let mut g = Genome::default();
        for i in 0..4 {
            g.maternal[i] = 10.0;
            g.paternal[i] = 10.0;
        }
        let p_max = Phenotype::from_genome(&g);
        for i in 0..4 {
            g.maternal[i] = -10.0;
            g.paternal[i] = -10.0;
        }
        let p_min = Phenotype::from_genome(&g);
        // Bounded by tanh: max is 1.0 + 0.35 = 1.35, min is 1.0 - 0.35 = 0.65
        assert!(
            p_max.speed <= 1.36,
            "speed should be bounded above: {}",
            p_max.speed
        );
        assert!(
            p_min.speed >= 0.64,
            "speed should be bounded below: {}",
            p_min.speed
        );
    }

    #[test]
    fn personality_loci_do_not_affect_physical_traits() {
        let mut g = Genome::default();
        // Set all personality loci to max (use N_PHYSICAL_LOCI..N_LOCI
        // so the range stays correct when new physical traits are added).
        for i in N_PHYSICAL_LOCI..N_LOCI {
            g.maternal[i] = 10.0;
            g.paternal[i] = 10.0;
        }
        let p = Phenotype::from_genome(&g);
        assert!(
            (p.speed - 1.0).abs() < 1e-6,
            "personality loci bled into speed"
        );
        assert!(
            (p.vision - 1.0).abs() < 1e-6,
            "personality loci bled into vision"
        );
    }

    #[test]
    fn neutral_phenotype_round_trips_to_neutral_genome() {
        let target = Phenotype::default();
        let g = Genome::from_phenotype(&target);
        let back = Phenotype::from_genome(&g);
        assert!((back.speed - 1.0).abs() < 1e-5);
        assert!((back.openness - 0.5).abs() < 1e-5);
        assert!((back.conscientiousness - 0.5).abs() < 1e-5);
        // Zero target loci means the genome should also be all zeros.
        for v in g.maternal.iter().chain(g.paternal.iter()) {
            assert_eq!(*v, 0.0);
        }
    }

    #[test]
    fn mid_range_physical_target_round_trips() {
        let target = Phenotype {
            speed: 1.2,
            ..Phenotype::default()
        };
        let g = Genome::from_phenotype(&target);
        let back = Phenotype::from_genome(&g);
        assert!(
            (back.speed - 1.2).abs() < 1e-3,
            "speed target 1.2 round-tripped to {}",
            back.speed
        );
        // Other traits stay baseline.
        assert!((back.vision - 1.0).abs() < 1e-5);
        assert!((back.openness - 0.5).abs() < 1e-5);
    }

    #[test]
    fn mid_range_personality_target_round_trips() {
        let target = Phenotype {
            conscientiousness: 0.7,
            ..Phenotype::default()
        };
        let g = Genome::from_phenotype(&target);
        let back = Phenotype::from_genome(&g);
        assert!(
            (back.conscientiousness - 0.7).abs() < 1e-3,
            "conscientiousness target 0.7 round-tripped to {}",
            back.conscientiousness
        );
        assert!((back.openness - 0.5).abs() < 1e-5);
    }

    #[test]
    fn extreme_personality_target_saturates_near_limit() {
        // 0.0 is outside the representable range under H2_PERSONALITY = 0.5.
        // It should saturate near the lower limit (~0.25) rather than explode.
        let target = Phenotype {
            conscientiousness: 0.0,
            ..Phenotype::default()
        };
        let g = Genome::from_phenotype(&target);
        let back = Phenotype::from_genome(&g);
        assert!(
            back.conscientiousness < 0.3,
            "extreme target should saturate low, got {}",
            back.conscientiousness
        );
        assert!(
            back.conscientiousness > 0.2,
            "saturation shouldn't overshoot past the representable limit, got {}",
            back.conscientiousness
        );
    }
}
