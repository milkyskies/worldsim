//! Species-agnostic body model.
//!
//! A `Body` is a flat `Vec<BodyPart>`. Each part declares which action
//! [`Channel`]s it provides and at what intensity — capability lives in the
//! anatomy, not in a hardcoded struct. That lets a wolf's jaws offer
//! Consumption + Bite while a human's arm offers Manipulation + Carry, without
//! the action system knowing anything about species.
//!
//! Reads: PhysicalNeeds (for healing boost + starvation gradient)
//! Writes: Body (healing/scarring), PhysicalNeeds (starvation damage)
//! Upstream: BiologyPlugin (auto-spawn), per-species spawners
//! Downstream: channel::ChannelCapacities (capability queries),
//!             movement::calculate_speed (injury penalty), UI/debug

use crate::agent::Agent;
use crate::agent::actions::channel::Channel;
use crate::agent::body::metabolism::STARVATION_DAMAGE_PER_SEC;
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::body::species::Species;
use crate::agent::mind::knowledge::Concept;
use crate::core::GameLog;
use crate::world::becomes::{Becomes, BecomesMode, BecomesTrigger};
use bevy::prelude::*;

/// A collection of anatomical parts. The shape is species-defined — a human
/// has 7 parts (head, torso, 2 arms, 2 legs, mouth), a wolf has 7 but with
/// four legs and jaws, a future octopus will have 10 tentacles.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct Body {
    #[reflect(ignore)]
    pub parts: Vec<BodyPart>,
}

impl Default for Body {
    /// `Body::default()` returns the human anatomy so legacy tests and
    /// spawners that don't know about species still get a usable body.
    /// Prefer `Body::for_species` or the explicit factories in new code.
    fn default() -> Self {
        Self::human()
    }
}

impl Body {
    /// Human anatomy: two arms, two legs, mouth, head, torso. Per-part
    /// intensities add up to the legacy "fully functional human" baseline
    /// of 1.0 per core channel: each arm provides Manipulation 0.5 and
    /// Carry 0.25, each leg provides Locomotion 0.5, the mouth provides
    /// Consumption + Vocalization at 1.0, and head/torso are FullBody
    /// gates. Losing one arm halves Manipulation to 0.5 (still above the
    /// 0.4 wolf-jaw ceiling but below Harvest's 0.9 requirement — a
    /// one-armed human can't reliably pick berries).
    pub fn human() -> Self {
        Self {
            parts: vec![
                BodyPart::vital("head", 50.0, vec![(Channel::Cognition, 1.0)])
                    .with_organs(head_organs()),
                BodyPart::vital("torso", 100.0, vec![(Channel::FullBody, 1.0)])
                    .with_organs(torso_organs()),
                BodyPart::new(
                    "left arm",
                    60.0,
                    vec![(Channel::Manipulation, 0.5), (Channel::Carry, 0.25)],
                ),
                BodyPart::new(
                    "right arm",
                    60.0,
                    vec![(Channel::Manipulation, 0.5), (Channel::Carry, 0.25)],
                ),
                BodyPart::new("left leg", 70.0, vec![(Channel::Locomotion, 0.5)]),
                BodyPart::new("right leg", 70.0, vec![(Channel::Locomotion, 0.5)]),
                BodyPart::new(
                    "mouth",
                    30.0,
                    vec![(Channel::Consumption, 1.0), (Channel::Vocalization, 1.0)],
                ),
            ],
        }
    }

    /// Wolf anatomy: four legs (each 0.3 Locomotion, so the pack outruns
    /// humans at 1.2 total), jaws that do double-duty as manipulator, eater,
    /// howler and weapon. No arms — wolves fail any action that demands
    /// Manipulation above the jaw's 0.4 ceiling.
    pub fn wolf() -> Self {
        Self {
            parts: vec![
                BodyPart::vital("head", 50.0, vec![(Channel::Cognition, 0.6)])
                    .with_organs(head_organs()),
                BodyPart::vital("torso", 100.0, vec![(Channel::FullBody, 1.0)])
                    .with_organs(torso_organs()),
                BodyPart::new("front left leg", 55.0, vec![(Channel::Locomotion, 0.3)]),
                BodyPart::new("front right leg", 55.0, vec![(Channel::Locomotion, 0.3)]),
                BodyPart::new("back left leg", 55.0, vec![(Channel::Locomotion, 0.3)]),
                BodyPart::new("back right leg", 55.0, vec![(Channel::Locomotion, 0.3)]),
                BodyPart::new(
                    "jaws",
                    40.0,
                    vec![
                        (Channel::Manipulation, 0.4),
                        (Channel::Consumption, 1.0),
                        (Channel::Vocalization, 0.7),
                        (Channel::Bite, 1.0),
                    ],
                ),
            ],
        }
    }

    /// Deer anatomy: four legs (total Locomotion 1.2, fast prey), a grazing
    /// mouth that provides Consumption + weak Vocalization (grunts, alarm
    /// calls). No Manipulation, no Bite — deer cannot harvest or fight.
    pub fn deer() -> Self {
        Self {
            parts: vec![
                BodyPart::vital("head", 40.0, vec![(Channel::Cognition, 0.4)])
                    .with_organs(head_organs()),
                BodyPart::vital("torso", 80.0, vec![(Channel::FullBody, 1.0)])
                    .with_organs(torso_organs()),
                BodyPart::new("front left leg", 50.0, vec![(Channel::Locomotion, 0.3)]),
                BodyPart::new("front right leg", 50.0, vec![(Channel::Locomotion, 0.3)]),
                BodyPart::new("back left leg", 50.0, vec![(Channel::Locomotion, 0.3)]),
                BodyPart::new("back right leg", 50.0, vec![(Channel::Locomotion, 0.3)]),
                BodyPart::new(
                    "mouth",
                    25.0,
                    vec![(Channel::Consumption, 1.0), (Channel::Vocalization, 0.3)],
                ),
            ],
        }
    }

    /// Pick the right anatomy for a species. Rabbits reuse the deer shape for
    /// now; birds fall back to the human template until we have a wing
    /// anatomy. Adding a new species is a one-line match arm plus a factory.
    pub fn for_species(species: Species) -> Self {
        match species {
            Species::Human => Self::human(),
            Species::Wolf => Self::wolf(),
            Species::Deer | Species::Rabbit => Self::deer(),
            Species::Bird => Self::human(),
        }
    }

    pub fn parts(&self) -> impl Iterator<Item = &BodyPart> {
        self.parts.iter()
    }

    pub fn part(&self, name: &str) -> Option<&BodyPart> {
        self.parts.iter().find(|p| p.name == name)
    }

    pub fn part_mut(&mut self, name: &str) -> Option<&mut BodyPart> {
        self.parts.iter_mut().find(|p| p.name == name)
    }

    /// Sum of pain across every part, weighted by unhealed severity.
    /// Feeds the pain urgency signal.
    pub fn total_pain(&self) -> f32 {
        self.parts.iter().map(BodyPart::total_pain).sum()
    }

    /// Any vital part (head / torso) at critically low function. Locks out
    /// active channels — the agent falls through to Idle, passive healing
    /// continues.
    pub fn is_incapacitated(&self) -> bool {
        self.parts
            .iter()
            .filter(|p| p.vital)
            .any(|p| p.function_rate < 0.2)
    }

    /// Total intensity this body can deliver on `channel`, taking injury
    /// into account. Sums across every part that provides the channel, so
    /// losing a leg drops Locomotion proportionally and a quadruped (four
    /// 0.3 legs) outpaces a biped (two 0.5 legs). Intensities are declared
    /// per part with this additive semantic in mind — each human arm
    /// provides Manipulation 0.5, for example, so both arms together equal
    /// the legacy "one fully functional agent" baseline of 1.0.
    pub fn channel_capacity(&self, channel: Channel) -> f32 {
        self.parts
            .iter()
            .filter_map(|p| p.channel_intensity(channel))
            .sum()
    }

    /// Iterator over every organ across every body part. Downstream systems
    /// use this to fan a single pass over the whole anatomy without caring
    /// which part an organ lives in.
    pub fn organs(&self) -> impl Iterator<Item = &Organ> {
        self.parts.iter().flat_map(|p| p.organs.iter())
    }

    /// Look up the first organ of the given kind anywhere on the body.
    /// Returns `None` for anatomies that don't carry that organ — e.g. no
    /// species has more than one brain, but "find the lungs" might still
    /// miss on a future species that lacks them.
    pub fn organ(&self, kind: OrganKind) -> Option<&Organ> {
        self.organs().find(|o| o.kind == kind)
    }

    /// Mutable variant of [`Body::organ`]. Used by future systems (combat
    /// damage, disease, poison) that need to mutate a specific organ's HP.
    pub fn organ_mut(&mut self, kind: OrganKind) -> Option<&mut Organ> {
        self.parts
            .iter_mut()
            .flat_map(|p| p.organs.iter_mut())
            .find(|o| o.kind == kind)
    }

    /// True when any vital organ (brain, heart, lungs) has been reduced to
    /// zero HP. Future death-path work reads this to trigger `die()` when
    /// combat or disease destroys a life-critical organ. The data structure
    /// publishes the predicate; the policy of "what to do about it" lives
    /// in the systems that consume it.
    pub fn any_vital_organ_destroyed(&self) -> bool {
        self.organs().any(|o| o.vital && o.is_destroyed())
    }

    /// Derive the digestive-organ multipliers the metabolism tick consumes.
    /// Missing organs (e.g. a future species that doesn't carry one) default
    /// to `1.0` so the absence of data never degrades the pipeline — only
    /// *damaged* organs slow the metabolism.
    pub fn organ_mods(&self) -> crate::agent::body::metabolism::OrganMods {
        crate::agent::body::metabolism::OrganMods {
            stomach: self
                .organ(OrganKind::Stomach)
                .map(|o| o.condition())
                .unwrap_or(1.0),
            liver: self
                .organ(OrganKind::Liver)
                .map(|o| o.condition())
                .unwrap_or(1.0),
            gut: self
                .organ(OrganKind::Gut)
                .map(|o| o.condition())
                .unwrap_or(1.0),
        }
    }

    /// Respiration multiplier: lung condition in `[0, 1]`. Consumed by the
    /// activity-effects stamina recovery path — oxygen delivery gates how
    /// fast aerobic reserves refill, so damaged lungs slow recovery without
    /// making activities themselves cheaper.
    ///
    /// Missing lungs default to `1.0` for the same reason `organ_mods`
    /// does: absence of anatomy never degrades the pipeline.
    pub fn lung_condition(&self) -> f32 {
        self.organ(OrganKind::Lungs)
            .map(|o| o.condition())
            .unwrap_or(1.0)
    }
}

/// Head organ seed — brain (vital), eyes, ears, nose. Shared across every
/// species that has a head. HP values reflect rough fragility: the brain is
/// the most delicate vital organ, sensory organs are small and fragile but
/// non-vital at the data layer.
fn head_organs() -> Vec<Organ> {
    vec![
        Organ::vital(OrganKind::Brain, 30.0),
        Organ::new(OrganKind::Eyes, 10.0),
        Organ::new(OrganKind::Ears, 10.0),
        Organ::new(OrganKind::Nose, 10.0),
    ]
}

/// Torso organ seed — heart and lungs vital, liver / stomach / gut non-vital
/// at this layer (individual systems decide what partial damage means).
/// Shared across every species that has a torso.
fn torso_organs() -> Vec<Organ> {
    vec![
        Organ::vital(OrganKind::Heart, 40.0),
        Organ::vital(OrganKind::Lungs, 35.0),
        Organ::new(OrganKind::Liver, 30.0),
        Organ::new(OrganKind::Stomach, 25.0),
        Organ::new(OrganKind::Gut, 25.0),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Reflect)]
pub enum InjuryType {
    Cut,
    Bruise,
    Fracture,
    Burn,
    Infection,
}

/// Species-agnostic organ kinds carried inside a [`BodyPart`]. Each entry is
/// a seed point for future systems: combat hit locations (#334) target
/// specific organs, metabolism modulation (#351) reads digestive organ
/// condition, perception reads eyes / ears / nose, respiration reads lungs,
/// circulation reads heart, cognition reads brain, and so on.
///
/// Organs are intentionally minimal at this layer — they carry HP, a vital
/// flag, and nothing else. Downstream systems add multipliers on top of the
/// `condition()` accessor without ever changing this enum's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum OrganKind {
    // Head
    Brain,
    Eyes,
    Ears,
    Nose,
    // Torso
    Heart,
    Lungs,
    Liver,
    Stomach,
    Gut,
}

/// An anatomical organ living inside a [`BodyPart`]. Pure data — the only
/// behaviour is the derived `condition()` accessor that every downstream
/// system reads to scale its own effect (digestion rate, perception range,
/// stamina ceiling, etc.). See [`OrganKind`] for the rationale.
///
/// Construction-time invariants: `max_hp > 0.0`, `hp == max_hp` at spawn
/// (organs always start intact), `vital` set only for organs whose
/// destruction represents instant death in future death-path work.
#[derive(Debug, Clone, Reflect)]
pub struct Organ {
    pub kind: OrganKind,
    pub hp: f32,
    pub max_hp: f32,
    /// Losing a vital organ (heart, lungs, brain) represents a lethal wound
    /// in future combat/disease work. The data structure just carries the
    /// flag; actual death logic belongs in the systems that read it.
    pub vital: bool,
}

impl Organ {
    /// Construct an organ at full HP.
    pub fn new(kind: OrganKind, max_hp: f32) -> Self {
        Self {
            kind,
            hp: max_hp,
            max_hp,
            vital: false,
        }
    }

    /// Construct a vital organ at full HP (Brain, Heart, Lungs).
    pub fn vital(kind: OrganKind, max_hp: f32) -> Self {
        Self {
            kind,
            hp: max_hp,
            max_hp,
            vital: true,
        }
    }

    /// Fractional HP in [0, 1]. Every downstream consumer (metabolism
    /// modulation, perception, respiration, etc.) reads through this so the
    /// underlying `hp / max_hp` math stays in one place.
    pub fn condition(&self) -> f32 {
        if self.max_hp <= 0.0 {
            0.0
        } else {
            (self.hp / self.max_hp).clamp(0.0, 1.0)
        }
    }

    /// True when the organ has been reduced to zero HP. Kept separate from
    /// `condition() == 0.0` so future scar / destruction work can evolve
    /// the predicate without breaking callers.
    pub fn is_destroyed(&self) -> bool {
        self.hp <= 0.0
    }
}

#[derive(Debug, Clone, Reflect)]
pub struct Injury {
    pub injury_type: InjuryType,
    pub severity: f32,      // 0.0 to 1.0 (1.0 = Max severity)
    pub pain: f32,          // 0.0 to 10.0
    pub healed_amount: f32, // 0.0 to 1.0 (1.0 = Fully healed)
}

#[derive(Debug, Clone, Reflect)]
pub struct BodyPart {
    pub name: String,
    #[reflect(ignore)]
    pub provides: Vec<(Channel, f32)>,
    /// Losing a vital part (head / torso) incapacitates the whole body.
    pub vital: bool,
    pub max_hp: f32,
    pub current_hp: f32,
    pub function_rate: f32,
    pub injuries: Vec<Injury>,
    /// Organs nested inside this part. Head holds brain/eyes/ears/nose,
    /// torso holds heart/lungs/liver/stomach/gut, limbs stay empty for now
    /// (future: bones in arms, tendons in legs).
    pub organs: Vec<Organ>,
}

impl BodyPart {
    pub fn new(name: impl Into<String>, max_hp: f32, provides: Vec<(Channel, f32)>) -> Self {
        Self {
            name: name.into(),
            provides,
            vital: false,
            max_hp,
            current_hp: max_hp,
            function_rate: 1.0,
            injuries: Vec::new(),
            organs: Vec::new(),
        }
    }

    pub fn vital(name: impl Into<String>, max_hp: f32, provides: Vec<(Channel, f32)>) -> Self {
        let mut part = Self::new(name, max_hp, provides);
        part.vital = true;
        part
    }

    /// Builder: attach a set of organs to this part. Used in the species
    /// factories (`Body::human`, `Body::wolf`, etc.) to seed head and torso
    /// with their anatomical contents.
    pub fn with_organs(mut self, organs: Vec<Organ>) -> Self {
        self.organs = organs;
        self
    }

    /// Intensity this part offers for `channel` after injury scaling, or
    /// `None` if the part doesn't participate in that channel at all. Used
    /// by `Body::channel_capacity` and direct capability queries.
    pub fn channel_intensity(&self, channel: Channel) -> Option<f32> {
        self.provides
            .iter()
            .find(|(c, _)| *c == channel)
            .map(|(_, intensity)| intensity * self.function_rate)
    }

    pub fn add_injury(&mut self, injury: Injury) {
        self.current_hp = (self.current_hp - (injury.severity * 20.0)).max(0.0);
        self.injuries.push(injury);
        self.recalculate_function();
    }

    pub fn recalculate_function(&mut self) {
        let hp_factor = self.current_hp / self.max_hp;

        let mut injury_penalty = 0.0;
        for injury in &self.injuries {
            let heal_factor = 1.0 - injury.healed_amount;
            injury_penalty += injury.severity * heal_factor;
        }

        self.function_rate = (hp_factor - injury_penalty).clamp(0.0, 1.0);
    }

    pub fn total_pain(&self) -> f32 {
        self.injuries
            .iter()
            .map(|injury| injury.pain * (1.0 - injury.healed_amount))
            .sum()
    }
}

pub fn process_healing(
    mut query: Query<(&mut Body, Option<&PhysicalNeeds>)>,
    tick: Res<crate::core::tick::TickCount>,
) {
    let dt = tick.dt();
    let base_healing_speed = 0.05;

    for (mut body, needs) in query.iter_mut() {
        let mut healing_speed = base_healing_speed;

        if let Some(physical) = needs
            && physical.stamina.aerobic > 80.0
        {
            healing_speed *= 2.0;
        }

        for part in body.parts.iter_mut() {
            let mut fully_healed_indices = Vec::new();

            for (i, injury) in part.injuries.iter_mut().enumerate() {
                if injury.healed_amount < 1.0 {
                    injury.healed_amount += healing_speed * dt;
                    if injury.healed_amount >= 1.0 {
                        injury.healed_amount = 1.0;
                        fully_healed_indices.push(i);
                    }
                }
            }

            for index in fully_healed_indices.iter().rev() {
                let severity = part.injuries[*index].severity;
                let scar_damage = severity * 2.0;
                part.max_hp = (part.max_hp - scar_damage).max(1.0);

                part.injuries.remove(*index);
            }

            if part.current_hp < part.max_hp {
                part.current_hp += 1.0 * dt;
                part.current_hp = part.current_hp.min(part.max_hp);
            }

            part.recalculate_function();
        }
    }
}

/// Starvation and dehydration: gradient damage when the metabolism has nothing
/// left to burn, or thirst is at the critical threshold. Ticks glucose / fat
/// / stomach flow lives in `activity_effects` — this system only applies the
/// *consequences* of a depleted metabolism (HP damage).
///
/// The gradient: reserves mobilize to cover low glucose as a first line of
/// defense. Once reserves are empty AND glucose stays critical, HP damage
/// begins. Well-fed agents can fast for days; lean agents die faster.
pub fn process_starvation(
    tick: Res<crate::core::tick::TickCount>,
    mut query: Query<&mut PhysicalNeeds>,
) {
    let dt = tick.dt();

    for mut physical in query.iter_mut() {
        if physical.metabolism.is_starving() {
            let health_damage = dt * STARVATION_DAMAGE_PER_SEC;
            physical.health = (physical.health - health_damage).clamp(0.0, 100.0);
        }

        if physical.thirst >= 90.0 {
            let health_damage = dt * 0.3;
            physical.health = (physical.health - health_damage).clamp(0.0, 100.0);
        }
    }
}

/// Unified death path.
///
/// Every cause of death — starvation, combat, future disease / drowning / old
/// age — routes through this helper so corpses spawn via the same `Becomes
/// InPlace Corpse` substrate. Previously `check_death` called `despawn()`
/// directly, which skipped corpse spawning entirely (the bug that #356 was
/// supposed to fix but never did).
///
/// The `Becomes` component with `AfterTicks(0)` fires on the next tick of the
/// becomes system, which morphs the entity into a Corpse in place — preserving
/// entity ID, MindGraph, Body, and relationship references.
pub fn die(
    commands: &mut Commands,
    entity: Entity,
    cause: impl Into<String>,
    current_tick: u64,
    game_log: &mut GameLog,
    sim_events: &mut MessageWriter<crate::agent::events::SimEvent>,
    name: Option<&Name>,
) {
    let cause = cause.into();
    let name_str = name.map(|n| n.as_str()).unwrap_or("Unknown Entity");
    game_log.event(&format!("{} died of {}!", name_str, cause));
    sim_events.write(crate::agent::events::SimEvent::Death {
        agent: entity,
        tick: current_tick,
        cause: cause.clone(),
    });
    commands.entity(entity).insert(Becomes {
        target: Concept::Corpse,
        trigger: BecomesTrigger::AfterTicks(0),
        started_tick: current_tick,
        mode: BecomesMode::InPlace,
    });
}

pub fn check_death(
    mut commands: Commands,
    query: Query<(Entity, &PhysicalNeeds, Option<&Name>), (With<Agent>, Without<Becomes>)>,
    mut game_log: ResMut<GameLog>,
    tick: Res<crate::core::tick::TickCount>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
) {
    for (entity, physical, name) in query.iter() {
        if physical.health <= 0.0 {
            die(
                &mut commands,
                entity,
                "starvation/dehydration/injury",
                tick.current,
                &mut game_log,
                &mut sim_events,
                name,
            );
        }
    }
}

#[cfg(test)]
mod organ_tests {
    use super::*;

    /// A freshly constructed organ reports full condition and is not destroyed.
    #[test]
    fn fresh_organ_is_fully_intact() {
        let heart = Organ::vital(OrganKind::Heart, 40.0);
        assert!((heart.condition() - 1.0).abs() < 1e-6);
        assert!(!heart.is_destroyed());
        assert!(heart.vital);
    }

    /// Zero HP drives condition to zero and flags destruction, regardless of
    /// how `hp` was zeroed.
    #[test]
    fn zero_hp_organ_is_destroyed() {
        let mut lungs = Organ::vital(OrganKind::Lungs, 35.0);
        lungs.hp = 0.0;
        assert_eq!(lungs.condition(), 0.0);
        assert!(lungs.is_destroyed());
    }

    /// Condition clamps to [0, 1] even if `hp` somehow drifts out of range.
    #[test]
    fn organ_condition_clamps_to_unit_interval() {
        let mut gut = Organ::new(OrganKind::Gut, 25.0);
        gut.hp = 100.0; // above max — should clamp, not overflow the ratio
        assert!((gut.condition() - 1.0).abs() < 1e-6);
        gut.hp = -5.0;
        assert_eq!(gut.condition(), 0.0);
    }

    /// A fresh human body carries every expected organ at full HP in the
    /// expected parts — head holds brain/eyes/ears/nose, torso holds
    /// heart/lungs/liver/stomach/gut, limbs stay flat.
    #[test]
    fn human_has_expected_organs_in_head_and_torso() {
        let body = Body::human();

        let head = body.part("head").expect("human has a head");
        let head_kinds: Vec<OrganKind> = head.organs.iter().map(|o| o.kind).collect();
        assert_eq!(
            head_kinds,
            vec![
                OrganKind::Brain,
                OrganKind::Eyes,
                OrganKind::Ears,
                OrganKind::Nose
            ]
        );

        let torso = body.part("torso").expect("human has a torso");
        let torso_kinds: Vec<OrganKind> = torso.organs.iter().map(|o| o.kind).collect();
        assert_eq!(
            torso_kinds,
            vec![
                OrganKind::Heart,
                OrganKind::Lungs,
                OrganKind::Liver,
                OrganKind::Stomach,
                OrganKind::Gut,
            ]
        );

        let left_arm = body.part("left arm").expect("human has a left arm");
        assert!(
            left_arm.organs.is_empty(),
            "limbs carry no organs in MVP, got {:?}",
            left_arm.organs
        );
    }

    /// Wolves and deer share the same organ seed as humans — head and torso
    /// both stocked on both species.
    #[test]
    fn wolf_and_deer_also_carry_head_and_torso_organs() {
        for body in [Body::wolf(), Body::deer()] {
            assert!(body.organ(OrganKind::Brain).is_some());
            assert!(body.organ(OrganKind::Heart).is_some());
            assert!(body.organ(OrganKind::Lungs).is_some());
            assert!(body.organ(OrganKind::Stomach).is_some());
        }
    }

    /// `Body::organ` looks up the first organ of a kind anywhere on the body
    /// and returns a full-hp reading on a fresh anatomy.
    #[test]
    fn body_organ_lookup_returns_full_hp_on_fresh_body() {
        let body = Body::human();
        let stomach = body
            .organ(OrganKind::Stomach)
            .expect("humans have a stomach");
        assert!((stomach.condition() - 1.0).abs() < 1e-6);
    }

    /// `any_vital_organ_destroyed` is false on a fresh anatomy and true once
    /// a vital organ has been zeroed. Exercises the downstream predicate
    /// that future combat/disease death paths will read.
    #[test]
    fn any_vital_organ_destroyed_tracks_heart_hp() {
        let mut body = Body::human();
        assert!(!body.any_vital_organ_destroyed());

        body.organ_mut(OrganKind::Heart)
            .expect("humans have a heart")
            .hp = 0.0;
        assert!(body.any_vital_organ_destroyed());
    }

    /// Destroying a non-vital organ (liver, eyes, etc.) does NOT trip the
    /// vital-destroyed predicate — the flag is per-organ, not per-body-part.
    #[test]
    fn destroying_non_vital_organ_does_not_trip_vital_predicate() {
        let mut body = Body::human();
        body.organ_mut(OrganKind::Liver)
            .expect("humans have a liver")
            .hp = 0.0;
        assert!(
            !body.any_vital_organ_destroyed(),
            "a destroyed liver is serious but not instant death at the data layer"
        );
    }

    /// Every organ iterator walks head + torso on every species. Total count
    /// matches the head (4) + torso (5) seed.
    #[test]
    fn organs_iterator_walks_head_and_torso_organs() {
        for body in [Body::human(), Body::wolf(), Body::deer()] {
            let count = body.organs().count();
            assert_eq!(count, 9, "every species carries 4 head + 5 torso organs");
        }
    }

    /// Fresh lungs report full condition; damaging them scales `lung_condition`
    /// proportionally. Feeds the respiration bridge into activity_effects.
    #[test]
    fn lung_condition_tracks_lung_organ_hp() {
        let healthy = Body::human();
        assert!((healthy.lung_condition() - 1.0).abs() < 1e-6);

        let mut damaged = Body::human();
        let lungs = damaged
            .organ_mut(OrganKind::Lungs)
            .expect("humans have lungs");
        lungs.hp = lungs.max_hp * 0.25;
        assert!(
            (damaged.lung_condition() - 0.25).abs() < 1e-6,
            "quarter-hp lungs should report 0.25, got {}",
            damaged.lung_condition()
        );
    }

    /// A fresh body produces fully-intact organ mods (all 1.0). Degrading
    /// stomach / liver / gut HP drops the matching mod toward zero without
    /// touching the others. Exercises the #351 bridge from organ condition
    /// into [`metabolism::OrganMods`].
    #[test]
    fn organ_mods_reflects_digestive_organ_condition() {
        let body = Body::human();
        let mods = body.organ_mods();
        assert!((mods.stomach - 1.0).abs() < 1e-6);
        assert!((mods.liver - 1.0).abs() < 1e-6);
        assert!((mods.gut - 1.0).abs() < 1e-6);

        let mut damaged = Body::human();
        let stomach = damaged
            .organ_mut(OrganKind::Stomach)
            .expect("humans have a stomach");
        stomach.hp = stomach.max_hp * 0.5;
        let mods = damaged.organ_mods();
        assert!(
            (mods.stomach - 0.5).abs() < 1e-6,
            "half-hp stomach should report ~0.5, got {}",
            mods.stomach
        );
        assert!(
            (mods.liver - 1.0).abs() < 1e-6,
            "liver is untouched, should still report 1.0"
        );
        assert!(
            (mods.gut - 1.0).abs() < 1e-6,
            "gut is untouched, should still report 1.0"
        );
    }
}
