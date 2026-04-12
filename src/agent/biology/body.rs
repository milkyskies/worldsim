//! Species-agnostic body model.
//!
//! A `Body` is a tree of [`BodyNode`]s. Each node declares which action
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

use crate::agent::actions::channel::Channel;
use crate::agent::body::metabolism::STARVATION_DAMAGE_PER_SEC;
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::body::species::Species;
use crate::agent::mind::knowledge::Concept;
use crate::agent::{Alive, Dead};
use crate::core::GameLog;
use crate::world::becomes::{Becomes, BecomesMode, BecomesTrigger};
use bevy::prelude::*;

// ─── Body ──────────────────────────────────────────────────────────────────

/// A tree of anatomical nodes. Root-level nodes are the outermost structural
/// parts (head, torso, limbs); each can have children (organs, future: bones,
/// tendons). The shape is species-defined.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct Body {
    #[reflect(ignore)]
    pub parts: Vec<BodyNode>,
}

impl Default for Body {
    fn default() -> Self {
        Self::human()
    }
}

impl Body {
    pub fn human() -> Self {
        Self {
            parts: vec![
                BodyNode::vital(
                    BodyNodeKind::Head,
                    50.0,
                    vec![(Channel::Focus, 1.0), (Channel::Awareness, 1.0)],
                )
                .with_children(head_organs()),
                BodyNode::vital(BodyNodeKind::Torso, 100.0, vec![(Channel::FullBody, 1.0)])
                    .with_children(torso_organs()),
                BodyNode::new(
                    BodyNodeKind::LeftArm,
                    60.0,
                    vec![(Channel::Manipulation, 0.5), (Channel::Carry, 0.25)],
                ),
                BodyNode::new(
                    BodyNodeKind::RightArm,
                    60.0,
                    vec![(Channel::Manipulation, 0.5), (Channel::Carry, 0.25)],
                ),
                BodyNode::new(
                    BodyNodeKind::LeftLeg,
                    70.0,
                    vec![(Channel::Locomotion, 0.5)],
                ),
                BodyNode::new(
                    BodyNodeKind::RightLeg,
                    70.0,
                    vec![(Channel::Locomotion, 0.5)],
                ),
                BodyNode::new(
                    BodyNodeKind::Mouth,
                    30.0,
                    vec![(Channel::Consumption, 1.0), (Channel::Vocalization, 1.0)],
                ),
            ],
        }
    }

    pub fn wolf() -> Self {
        Self {
            parts: vec![
                BodyNode::vital(
                    BodyNodeKind::Head,
                    50.0,
                    vec![(Channel::Focus, 0.6), (Channel::Awareness, 0.8)],
                )
                .with_children(head_organs()),
                BodyNode::vital(BodyNodeKind::Torso, 100.0, vec![(Channel::FullBody, 1.0)])
                    .with_children(torso_organs()),
                BodyNode::new(
                    BodyNodeKind::LeftForeleg,
                    55.0,
                    vec![(Channel::Locomotion, 0.3)],
                ),
                BodyNode::new(
                    BodyNodeKind::RightForeleg,
                    55.0,
                    vec![(Channel::Locomotion, 0.3)],
                ),
                BodyNode::new(
                    BodyNodeKind::LeftHindleg,
                    55.0,
                    vec![(Channel::Locomotion, 0.3)],
                ),
                BodyNode::new(
                    BodyNodeKind::RightHindleg,
                    55.0,
                    vec![(Channel::Locomotion, 0.3)],
                ),
                BodyNode::new(
                    BodyNodeKind::Jaws,
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

    pub fn deer() -> Self {
        Self {
            parts: vec![
                BodyNode::vital(
                    BodyNodeKind::Head,
                    40.0,
                    vec![(Channel::Focus, 0.4), (Channel::Awareness, 1.0)],
                )
                .with_children(head_organs()),
                BodyNode::vital(BodyNodeKind::Torso, 80.0, vec![(Channel::FullBody, 1.0)])
                    .with_children(torso_organs()),
                BodyNode::new(
                    BodyNodeKind::LeftForeleg,
                    50.0,
                    vec![(Channel::Locomotion, 0.3)],
                ),
                BodyNode::new(
                    BodyNodeKind::RightForeleg,
                    50.0,
                    vec![(Channel::Locomotion, 0.3)],
                ),
                BodyNode::new(
                    BodyNodeKind::LeftHindleg,
                    50.0,
                    vec![(Channel::Locomotion, 0.3)],
                ),
                BodyNode::new(
                    BodyNodeKind::RightHindleg,
                    50.0,
                    vec![(Channel::Locomotion, 0.3)],
                ),
                BodyNode::new(
                    BodyNodeKind::Mouth,
                    25.0,
                    vec![(Channel::Consumption, 1.0), (Channel::Vocalization, 0.3)],
                ),
            ],
        }
    }

    pub fn for_species(species: Species) -> Self {
        match species {
            Species::Human => Self::human(),
            Species::Wolf => Self::wolf(),
            Species::Deer | Species::Rabbit => Self::deer(),
            Species::Bird => Self::human(),
        }
    }

    /// Root-level nodes only (head, torso, limbs).
    pub fn parts(&self) -> impl Iterator<Item = &BodyNode> {
        self.parts.iter()
    }

    /// Find a root-level node by kind. For combat hit location, UI rendering.
    pub fn part(&self, kind: BodyNodeKind) -> Option<&BodyNode> {
        self.parts.iter().find(|p| p.kind == kind)
    }

    pub fn part_mut(&mut self, kind: BodyNodeKind) -> Option<&mut BodyNode> {
        self.parts.iter_mut().find(|p| p.kind == kind)
    }

    /// Find any node anywhere in the tree by kind.
    pub fn node(&self, kind: BodyNodeKind) -> Option<&BodyNode> {
        self.parts.iter().find_map(|p| p.find(kind))
    }

    pub fn node_mut(&mut self, kind: BodyNodeKind) -> Option<&mut BodyNode> {
        self.parts.iter_mut().find_map(|p| p.find_mut(kind))
    }

    /// Sum of pain across every node in the tree.
    pub fn total_pain(&self) -> f32 {
        self.parts.iter().map(BodyNode::tree_pain).sum()
    }

    /// Any vital node at critically low function.
    pub fn is_incapacitated(&self) -> bool {
        for part in &self.parts {
            if part.vital && part.function_rate < 0.2 {
                return true;
            }
            for child in &part.children {
                if child.vital && child.function_rate < 0.2 {
                    return true;
                }
            }
        }
        false
    }

    /// Total intensity this body can deliver on `channel`, summed across
    /// every node in the tree (root + children).
    pub fn channel_capacity(&self, channel: Channel) -> f32 {
        let mut total = 0.0;
        for part in &self.parts {
            if let Some(v) = part.channel_intensity(channel) {
                total += v;
            }
            for child in &part.children {
                if let Some(v) = child.channel_intensity(channel) {
                    total += v;
                }
            }
        }
        total
    }

    /// True when any vital node has been reduced to zero HP.
    pub fn any_vital_organ_destroyed(&self) -> bool {
        for part in &self.parts {
            if part.vital && part.is_destroyed() {
                return true;
            }
            for child in &part.children {
                if child.vital && child.is_destroyed() {
                    return true;
                }
            }
        }
        false
    }

    /// Derive the digestive-organ multipliers the metabolism tick consumes.
    pub fn organ_mods(&self) -> crate::agent::body::metabolism::OrganMods {
        crate::agent::body::metabolism::OrganMods {
            stomach: self
                .node(BodyNodeKind::Stomach)
                .map(|o| o.condition())
                .unwrap_or(1.0),
            liver: self
                .node(BodyNodeKind::Liver)
                .map(|o| o.condition())
                .unwrap_or(1.0),
            gut: self
                .node(BodyNodeKind::Gut)
                .map(|o| o.condition())
                .unwrap_or(1.0),
        }
    }

    /// Respiration multiplier: lung condition in `[0, 1]`.
    pub fn lung_condition(&self) -> f32 {
        self.node(BodyNodeKind::Lungs)
            .map(|o| o.condition())
            .unwrap_or(1.0)
    }
}

/// Head organ seed — brain (vital), eyes, ears, nose.
fn head_organs() -> Vec<BodyNode> {
    vec![
        BodyNode::vital(BodyNodeKind::Brain, 30.0, vec![]),
        BodyNode::new(BodyNodeKind::Eyes, 10.0, vec![]),
        BodyNode::new(BodyNodeKind::Ears, 10.0, vec![]),
        BodyNode::new(BodyNodeKind::Nose, 10.0, vec![]),
    ]
}

/// Torso organ seed — heart and lungs vital.
fn torso_organs() -> Vec<BodyNode> {
    vec![
        BodyNode::vital(BodyNodeKind::Heart, 40.0, vec![]),
        BodyNode::vital(BodyNodeKind::Lungs, 35.0, vec![]),
        BodyNode::new(BodyNodeKind::Liver, 30.0, vec![]),
        BodyNode::new(BodyNodeKind::Stomach, 25.0, vec![]),
        BodyNode::new(BodyNodeKind::Gut, 25.0, vec![]),
    ]
}

// ─── BodyNodeKind ──────────────────────────────────────────────────────────

/// Typed identifier for every anatomical node any species can carry.
/// Merges the former `BodyPartKind` (structural) and `OrganKind` (internal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum BodyNodeKind {
    // Structural
    Head,
    Torso,
    LeftArm,
    RightArm,
    LeftLeg,
    RightLeg,
    LeftForeleg,
    RightForeleg,
    LeftHindleg,
    RightHindleg,
    Mouth,
    Jaws,
    // Internal organs
    Brain,
    Eyes,
    Ears,
    Nose,
    Heart,
    Lungs,
    Liver,
    Stomach,
    Gut,
}

impl BodyNodeKind {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Head => "head",
            Self::Torso => "torso",
            Self::LeftArm => "left arm",
            Self::RightArm => "right arm",
            Self::LeftLeg => "left leg",
            Self::RightLeg => "right leg",
            Self::LeftForeleg => "front left leg",
            Self::RightForeleg => "front right leg",
            Self::LeftHindleg => "back left leg",
            Self::RightHindleg => "back right leg",
            Self::Mouth => "mouth",
            Self::Jaws => "jaws",
            Self::Brain => "brain",
            Self::Eyes => "eyes",
            Self::Ears => "ears",
            Self::Nose => "nose",
            Self::Heart => "heart",
            Self::Lungs => "lungs",
            Self::Liver => "liver",
            Self::Stomach => "stomach",
            Self::Gut => "gut",
        }
    }
}

// ─── Injury ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Reflect)]
pub enum InjuryType {
    Cut,
    Bruise,
    Fracture,
    Burn,
    Infection,
    Pierce,
    Slash,
    Crush,
}

#[derive(Debug, Clone, Reflect)]
pub struct Injury {
    pub injury_type: InjuryType,
    pub severity: f32,
    pub pain: f32,
    pub healed_amount: f32,
    pub bleed_rate: f32,
}

impl Injury {
    pub fn effective_bleed(&self) -> f32 {
        self.bleed_rate.max(0.0)
    }
}

pub const CLOT_DECAY_PER_SEC: f32 = 1.0 / 300.0;

// ─── BodyNode ──────────────────────────────────────────────────────────────

/// A single anatomical node. Replaces both the former `BodyPart` and `Organ`
/// types. Every node uniformly has HP, vital flag, channel contributions,
/// injury list, and function rate. Nodes can nest arbitrarily deep via
/// `children`.
#[derive(Debug, Clone, Reflect)]
pub struct BodyNode {
    pub kind: BodyNodeKind,
    #[reflect(ignore)]
    pub provides: Vec<(Channel, f32)>,
    pub vital: bool,
    pub max_hp: f32,
    pub current_hp: f32,
    pub function_rate: f32,
    pub injuries: Vec<Injury>,
    pub children: Vec<BodyNode>,
}

impl BodyNode {
    pub fn new(kind: BodyNodeKind, max_hp: f32, provides: Vec<(Channel, f32)>) -> Self {
        Self {
            kind,
            provides,
            vital: false,
            max_hp,
            current_hp: max_hp,
            function_rate: 1.0,
            injuries: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn vital(kind: BodyNodeKind, max_hp: f32, provides: Vec<(Channel, f32)>) -> Self {
        let mut node = Self::new(kind, max_hp, provides);
        node.vital = true;
        node
    }

    pub fn with_children(mut self, children: Vec<BodyNode>) -> Self {
        self.children = children;
        self
    }

    pub fn name(&self) -> &'static str {
        self.kind.display_name()
    }

    /// Fractional HP in [0, 1].
    pub fn condition(&self) -> f32 {
        if self.max_hp <= 0.0 {
            0.0
        } else {
            (self.current_hp / self.max_hp).clamp(0.0, 1.0)
        }
    }

    pub fn is_destroyed(&self) -> bool {
        self.current_hp <= 0.0
    }

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
        let hp_factor = if self.max_hp > 0.0 {
            self.current_hp / self.max_hp
        } else {
            0.0
        };

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

    /// Total pain including all descendants.
    fn tree_pain(&self) -> f32 {
        let mut pain = self.total_pain();
        for child in &self.children {
            pain += child.tree_pain();
        }
        pain
    }

    /// Find a node by kind in this subtree (self + descendants).
    pub fn find(&self, kind: BodyNodeKind) -> Option<&BodyNode> {
        if self.kind == kind {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(kind))
    }

    pub fn find_mut(&mut self, kind: BodyNodeKind) -> Option<&mut BodyNode> {
        if self.kind == kind {
            return Some(self);
        }
        self.children.iter_mut().find_map(|c| c.find_mut(kind))
    }
}

// ─── Healing system ────────────────────────────────────────────────────────

fn heal_duration_seconds(kind: InjuryType) -> f32 {
    const MINUTE: f32 = 60.0;
    match kind {
        InjuryType::Bruise => 3.0 * MINUTE,
        InjuryType::Cut => 3.0 * MINUTE,
        InjuryType::Slash => 4.0 * MINUTE,
        InjuryType::Crush => 5.0 * MINUTE,
        InjuryType::Burn => 7.0 * MINUTE,
        InjuryType::Pierce => 10.0 * MINUTE,
        InjuryType::Infection => 10.0 * MINUTE,
        InjuryType::Fracture => 20.0 * MINUTE,
    }
}

/// Heal a single node: advance injury healing, apply scar damage, regen HP.
fn heal_node(node: &mut BodyNode, dt: f32, condition_mult: f32) {
    let mut fully_healed_indices = Vec::new();

    for (i, injury) in node.injuries.iter_mut().enumerate() {
        if injury.healed_amount < 1.0 {
            let duration = heal_duration_seconds(injury.injury_type).max(1.0);
            let rate = condition_mult / duration;
            injury.healed_amount += rate * dt;
            if injury.healed_amount >= 1.0 {
                injury.healed_amount = 1.0;
                fully_healed_indices.push(i);
            }
        }
    }

    for index in fully_healed_indices.iter().rev() {
        let severity = node.injuries[*index].severity;
        let scar_damage = severity * 2.0;
        node.max_hp = (node.max_hp - scar_damage).max(1.0);
        node.current_hp = node.current_hp.min(node.max_hp);
        node.injuries.remove(*index);
    }

    if node.current_hp < node.max_hp {
        node.current_hp += 1.0 * dt;
        node.current_hp = node.current_hp.min(node.max_hp);
    }

    node.recalculate_function();
}

pub fn process_healing(
    mut query: Query<(&mut Body, Option<&PhysicalNeeds>), With<Alive>>,
    tick: Res<crate::core::tick::TickCount>,
) {
    let dt = tick.dt();

    for (mut body, needs) in query.iter_mut() {
        let condition_mult = if let Some(physical) = needs
            && physical.stamina.aerobic > 80.0
        {
            2.0
        } else {
            1.0
        };

        for part in body.parts.iter_mut() {
            heal_node(part, dt, condition_mult);
            for child in part.children.iter_mut() {
                heal_node(child, dt, condition_mult);
            }
        }
    }
}

// ─── Starvation / death ────────────────────────────────────────────────────

pub fn process_starvation(
    tick: Res<crate::core::tick::TickCount>,
    mut query: Query<&mut PhysicalNeeds, With<Alive>>,
) {
    use crate::agent::body::needs::HealthDamageSource;
    let dt = tick.dt();

    for mut physical in query.iter_mut() {
        if physical.metabolism.is_starving() {
            let health_damage = dt * STARVATION_DAMAGE_PER_SEC;
            physical.health = (physical.health - health_damage).clamp(0.0, 100.0);
            physical.last_health_damage = Some(HealthDamageSource::Starvation);
        }

        if physical.hydration <= 10.0 {
            let health_damage = dt * 0.3;
            physical.health = (physical.health - health_damage).clamp(0.0, 100.0);
            physical.last_health_damage = Some(HealthDamageSource::Dehydration);
        }
    }
}

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
    commands
        .entity(entity)
        .remove::<Alive>()
        .insert(Dead)
        .insert(Becomes {
            target: Concept::Corpse,
            trigger: BecomesTrigger::AfterTicks(0),
            started_tick: current_tick,
            mode: BecomesMode::InPlace,
        });
}

pub fn check_death(
    mut commands: Commands,
    query: Query<(Entity, &PhysicalNeeds, Option<&Name>), With<Alive>>,
    mut game_log: ResMut<GameLog>,
    tick: Res<crate::core::tick::TickCount>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
) {
    for (entity, physical, name) in query.iter() {
        if physical.health <= 0.0 {
            let cause = physical
                .last_health_damage
                .map(|s| s.as_cause())
                .unwrap_or("unknown health drain");
            die(
                &mut commands,
                entity,
                cause,
                tick.current,
                &mut game_log,
                &mut sim_events,
                name,
            );
        }
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_organ_is_fully_intact() {
        let heart = BodyNode::vital(BodyNodeKind::Heart, 40.0, vec![]);
        assert!((heart.condition() - 1.0).abs() < 1e-6);
        assert!(!heart.is_destroyed());
        assert!(heart.vital);
    }

    #[test]
    fn zero_hp_organ_is_destroyed() {
        let mut lungs = BodyNode::vital(BodyNodeKind::Lungs, 35.0, vec![]);
        lungs.current_hp = 0.0;
        lungs.recalculate_function();
        assert_eq!(lungs.condition(), 0.0);
        assert!(lungs.is_destroyed());
    }

    #[test]
    fn organ_condition_clamps_to_unit_interval() {
        let mut gut = BodyNode::new(BodyNodeKind::Gut, 25.0, vec![]);
        gut.current_hp = 100.0;
        assert!((gut.condition() - 1.0).abs() < 1e-6);
        gut.current_hp = -5.0;
        assert_eq!(gut.condition(), 0.0);
    }

    #[test]
    fn human_has_expected_organs_in_head_and_torso() {
        let body = Body::human();

        let head = body.part(BodyNodeKind::Head).expect("human has a head");
        let head_kinds: Vec<BodyNodeKind> = head.children.iter().map(|o| o.kind).collect();
        assert_eq!(
            head_kinds,
            vec![
                BodyNodeKind::Brain,
                BodyNodeKind::Eyes,
                BodyNodeKind::Ears,
                BodyNodeKind::Nose,
            ]
        );

        let torso = body.part(BodyNodeKind::Torso).expect("human has a torso");
        let torso_kinds: Vec<BodyNodeKind> = torso.children.iter().map(|o| o.kind).collect();
        assert_eq!(
            torso_kinds,
            vec![
                BodyNodeKind::Heart,
                BodyNodeKind::Lungs,
                BodyNodeKind::Liver,
                BodyNodeKind::Stomach,
                BodyNodeKind::Gut,
            ]
        );

        let left_arm = body
            .part(BodyNodeKind::LeftArm)
            .expect("human has a left arm");
        assert!(left_arm.children.is_empty());
    }

    #[test]
    fn wolf_and_deer_also_carry_head_and_torso_organs() {
        for body in [Body::wolf(), Body::deer()] {
            assert!(body.node(BodyNodeKind::Brain).is_some());
            assert!(body.node(BodyNodeKind::Heart).is_some());
            assert!(body.node(BodyNodeKind::Lungs).is_some());
            assert!(body.node(BodyNodeKind::Stomach).is_some());
        }
    }

    #[test]
    fn body_node_lookup_returns_full_hp_on_fresh_body() {
        let body = Body::human();
        let stomach = body
            .node(BodyNodeKind::Stomach)
            .expect("humans have a stomach");
        assert!((stomach.condition() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn any_vital_organ_destroyed_tracks_heart_hp() {
        let mut body = Body::human();
        assert!(!body.any_vital_organ_destroyed());

        body.node_mut(BodyNodeKind::Heart)
            .expect("humans have a heart")
            .current_hp = 0.0;
        assert!(body.any_vital_organ_destroyed());
    }

    #[test]
    fn destroying_non_vital_organ_does_not_trip_vital_predicate() {
        let mut body = Body::human();
        body.node_mut(BodyNodeKind::Liver)
            .expect("humans have a liver")
            .current_hp = 0.0;
        assert!(!body.any_vital_organ_destroyed());
    }

    #[test]
    fn organs_iterator_walks_head_and_torso_organs() {
        for body in [Body::human(), Body::wolf(), Body::deer()] {
            let count: usize = body.parts.iter().map(|p| p.children.len()).sum();
            assert_eq!(count, 9, "every species carries 4 head + 5 torso organs");
        }
    }

    #[test]
    fn lung_condition_tracks_lung_organ_hp() {
        let healthy = Body::human();
        assert!((healthy.lung_condition() - 1.0).abs() < 1e-6);

        let mut damaged = Body::human();
        let lungs = damaged
            .node_mut(BodyNodeKind::Lungs)
            .expect("humans have lungs");
        lungs.current_hp = lungs.max_hp * 0.25;
        assert!(
            (damaged.lung_condition() - 0.25).abs() < 1e-6,
            "quarter-hp lungs should report 0.25, got {}",
            damaged.lung_condition()
        );
    }

    #[test]
    fn organ_mods_reflects_digestive_organ_condition() {
        let body = Body::human();
        let mods = body.organ_mods();
        assert!((mods.stomach - 1.0).abs() < 1e-6);
        assert!((mods.liver - 1.0).abs() < 1e-6);
        assert!((mods.gut - 1.0).abs() < 1e-6);

        let mut damaged = Body::human();
        let stomach = damaged
            .node_mut(BodyNodeKind::Stomach)
            .expect("humans have a stomach");
        stomach.current_hp = stomach.max_hp * 0.5;
        let mods = damaged.organ_mods();
        assert!(
            (mods.stomach - 0.5).abs() < 1e-6,
            "half-hp stomach should report ~0.5, got {}",
            mods.stomach
        );
        assert!((mods.liver - 1.0).abs() < 1e-6);
        assert!((mods.gut - 1.0).abs() < 1e-6);
    }
}
