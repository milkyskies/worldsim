//! Species-agnostic body model.
//!
//! A `Body` is a flat `Vec<BodyPart>`. Each part declares which action
//! [`Channel`]s it provides and at what intensity — capability lives in the
//! anatomy, not in a hardcoded struct. That lets a wolf's jaws offer
//! Consumption + Bite while a human's arm offers Manipulation + Carry, without
//! the action system knowing anything about species.
//!
//! Reads: PhysicalNeeds (for healing boost)
//! Writes: Body (healing/scarring), PhysicalNeeds (starvation damage)
//! Upstream: BiologyPlugin (auto-spawn), per-species spawners
//! Downstream: channel::ChannelCapacities (capability queries),
//!             movement::calculate_speed (injury penalty), UI/debug

use crate::agent::actions::channel::Channel;
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::body::species::Species;
use crate::core::GameLog;
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
                BodyPart::vital("head", 50.0, vec![(Channel::Cognition, 1.0)]),
                BodyPart::vital("torso", 100.0, vec![(Channel::FullBody, 1.0)]),
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
                BodyPart::vital("head", 50.0, vec![(Channel::Cognition, 0.6)]),
                BodyPart::vital("torso", 100.0, vec![(Channel::FullBody, 1.0)]),
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
                BodyPart::vital("head", 40.0, vec![(Channel::Cognition, 0.4)]),
                BodyPart::vital("torso", 80.0, vec![(Channel::FullBody, 1.0)]),
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
}

#[derive(Debug, Clone, Copy, PartialEq, Reflect)]
pub enum InjuryType {
    Cut,
    Bruise,
    Fracture,
    Burn,
    Infection,
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
        }
    }

    pub fn vital(name: impl Into<String>, max_hp: f32, provides: Vec<(Channel, f32)>) -> Self {
        let mut part = Self::new(name, max_hp, provides);
        part.vital = true;
        part
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

pub fn process_healing(mut query: Query<(&mut Body, Option<&PhysicalNeeds>)>, time: Res<Time>) {
    let dt = time.delta_secs();
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

/// Starvation and dehydration system - applies damage if hunger or thirst is critical
pub fn process_starvation(time: Res<Time>, mut query: Query<&mut PhysicalNeeds>) {
    let dt = time.delta_secs();

    for mut physical in query.iter_mut() {
        if physical.hunger >= 90.0 {
            let health_damage = dt * 0.2;
            physical.health = (physical.health - health_damage).clamp(0.0, 100.0);
        }

        if physical.thirst >= 90.0 {
            let health_damage = dt * 0.3;
            physical.health = (physical.health - health_damage).clamp(0.0, 100.0);
        }
    }
}

pub fn check_death(
    mut commands: Commands,
    query: Query<(Entity, &PhysicalNeeds, Option<&Name>)>,
    mut game_log: ResMut<GameLog>,
    tick: Res<crate::core::tick::TickCount>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
) {
    for (entity, physical, name) in query.iter() {
        if physical.health <= 0.0 {
            let name_str = name.map(|n| n.as_str()).unwrap_or("Unknown Entity");
            game_log.event(&format!(
                "{} died of starvation/dehydration/injury!",
                name_str
            ));
            sim_events.write(crate::agent::events::SimEvent::Death {
                agent: entity,
                tick: tick.current,
                cause: "starvation/dehydration/injury".to_string(),
            });
            commands.entity(entity).despawn();
        }
    }
}
