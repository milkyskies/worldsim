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
        use FunctionalTag::*;
        Self {
            parts: vec![
                BodyNode::vital(BodyNodeKind::Head, 50.0).with_children(vec![
                    BodyNode::vital(BodyNodeKind::Brain, 30.0).with_tags(vec![Think]),
                    BodyNode::new(BodyNodeKind::LeftEye, 10.0).with_tags(vec![See]),
                    BodyNode::new(BodyNodeKind::RightEye, 10.0).with_tags(vec![See]),
                    BodyNode::new(BodyNodeKind::LeftEar, 5.0).with_tags(vec![Hear]),
                    BodyNode::new(BodyNodeKind::RightEar, 5.0).with_tags(vec![Hear]),
                    BodyNode::new(BodyNodeKind::Nose, 10.0).with_tags(vec![Smell]),
                    BodyNode::new(BodyNodeKind::Jaw, 30.0).with_tags(vec![Eat, Speak, Bite]),
                ]),
                BodyNode::vital(BodyNodeKind::Torso, 100.0)
                    .with_tags(vec![FullBody])
                    .with_children(torso_organs()),
                BodyNode::new(BodyNodeKind::LeftArm, 60.0).with_children(vec![
                    BodyNode::new(BodyNodeKind::LeftHand, 30.0).with_tags(vec![Grasp, Carry]),
                ]),
                BodyNode::new(BodyNodeKind::RightArm, 60.0).with_children(vec![
                    BodyNode::new(BodyNodeKind::RightHand, 30.0).with_tags(vec![Grasp, Carry]),
                ]),
                BodyNode::new(BodyNodeKind::LeftLeg, 70.0).with_children(vec![
                    BodyNode::new(BodyNodeKind::LeftFoot, 35.0).with_tags(vec![Stance]),
                ]),
                BodyNode::new(BodyNodeKind::RightLeg, 70.0).with_children(vec![
                    BodyNode::new(BodyNodeKind::RightFoot, 35.0).with_tags(vec![Stance]),
                ]),
            ],
        }
    }

    pub fn wolf() -> Self {
        use FunctionalTag::*;
        Self {
            parts: vec![
                BodyNode::vital(BodyNodeKind::Head, 50.0).with_children(vec![
                    BodyNode::vital(BodyNodeKind::Brain, 30.0).with_tags(vec![Think]),
                    BodyNode::new(BodyNodeKind::LeftEye, 8.0).with_tags(vec![See]),
                    BodyNode::new(BodyNodeKind::RightEye, 8.0).with_tags(vec![See]),
                    BodyNode::new(BodyNodeKind::LeftEar, 5.0).with_tags(vec![Hear]),
                    BodyNode::new(BodyNodeKind::RightEar, 5.0).with_tags(vec![Hear]),
                    BodyNode::new(BodyNodeKind::Nose, 8.0).with_tags(vec![Smell]),
                    BodyNode::new(BodyNodeKind::Jaw, 40.0).with_tags(vec![Eat, Speak, Bite, Grasp]),
                ]),
                BodyNode::vital(BodyNodeKind::Torso, 100.0)
                    .with_tags(vec![FullBody])
                    .with_children(torso_organs()),
                BodyNode::new(BodyNodeKind::LeftForeleg, 55.0).with_children(vec![
                    BodyNode::new(BodyNodeKind::LeftForepaw, 25.0).with_tags(vec![Stance]),
                ]),
                BodyNode::new(BodyNodeKind::RightForeleg, 55.0).with_children(vec![
                    BodyNode::new(BodyNodeKind::RightForepaw, 25.0).with_tags(vec![Stance]),
                ]),
                BodyNode::new(BodyNodeKind::LeftHindleg, 55.0).with_children(vec![
                    BodyNode::new(BodyNodeKind::LeftHindpaw, 25.0).with_tags(vec![Stance]),
                ]),
                BodyNode::new(BodyNodeKind::RightHindleg, 55.0).with_children(vec![
                    BodyNode::new(BodyNodeKind::RightHindpaw, 25.0).with_tags(vec![Stance]),
                ]),
            ],
        }
    }

    pub fn deer() -> Self {
        use FunctionalTag::*;
        Self {
            parts: vec![
                BodyNode::vital(BodyNodeKind::Head, 40.0).with_children(vec![
                    BodyNode::vital(BodyNodeKind::Brain, 25.0).with_tags(vec![Think]),
                    BodyNode::new(BodyNodeKind::LeftEye, 8.0).with_tags(vec![See]),
                    BodyNode::new(BodyNodeKind::RightEye, 8.0).with_tags(vec![See]),
                    BodyNode::new(BodyNodeKind::LeftEar, 5.0).with_tags(vec![Hear]),
                    BodyNode::new(BodyNodeKind::RightEar, 5.0).with_tags(vec![Hear]),
                    BodyNode::new(BodyNodeKind::Nose, 8.0).with_tags(vec![Smell]),
                    BodyNode::new(BodyNodeKind::Jaw, 20.0).with_tags(vec![Eat, Speak]),
                ]),
                BodyNode::vital(BodyNodeKind::Torso, 80.0)
                    .with_tags(vec![FullBody])
                    .with_children(torso_organs()),
                BodyNode::new(BodyNodeKind::LeftForeleg, 50.0).with_children(vec![
                    BodyNode::new(BodyNodeKind::LeftForehoof, 20.0).with_tags(vec![Stance]),
                ]),
                BodyNode::new(BodyNodeKind::RightForeleg, 50.0).with_children(vec![
                    BodyNode::new(BodyNodeKind::RightForehoof, 20.0).with_tags(vec![Stance]),
                ]),
                BodyNode::new(BodyNodeKind::LeftHindleg, 50.0).with_children(vec![
                    BodyNode::new(BodyNodeKind::LeftHindhoof, 20.0).with_tags(vec![Stance]),
                ]),
                BodyNode::new(BodyNodeKind::RightHindleg, 50.0).with_children(vec![
                    BodyNode::new(BodyNodeKind::RightHindhoof, 20.0).with_tags(vec![Stance]),
                ]),
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

    /// All nodes in the tree that carry the given tag.
    pub fn nodes_with_tag(&self, tag: FunctionalTag) -> Vec<&BodyNode> {
        let mut result = Vec::new();
        for part in &self.parts {
            if part.has_tag(tag) {
                result.push(part);
            }
            for child in &part.children {
                if child.has_tag(tag) {
                    result.push(child);
                }
            }
        }
        result
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

    /// Total intensity this body can deliver on `channel`, derived from
    /// functional tags via the mapping.
    pub fn channel_capacity(&self, channel: Channel, mapping: &TagChannelMapping) -> f32 {
        mapping.channel_capacity(self, channel)
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

    /// Respiration multiplier: average condition of both lungs in `[0, 1]`.
    pub fn lung_condition(&self) -> f32 {
        let left = self
            .node(BodyNodeKind::LeftLung)
            .map(|o| o.condition())
            .unwrap_or(1.0);
        let right = self
            .node(BodyNodeKind::RightLung)
            .map(|o| o.condition())
            .unwrap_or(1.0);
        (left + right) / 2.0
    }
}

/// Torso organ seed — heart and split lungs vital.
fn torso_organs() -> Vec<BodyNode> {
    use FunctionalTag::*;
    vec![
        BodyNode::vital(BodyNodeKind::Heart, 40.0).with_tags(vec![Pump]),
        BodyNode::vital(BodyNodeKind::LeftLung, 18.0).with_tags(vec![Breathe]),
        BodyNode::vital(BodyNodeKind::RightLung, 17.0).with_tags(vec![Breathe]),
        BodyNode::new(BodyNodeKind::Liver, 30.0).with_tags(vec![Filter]),
        BodyNode::new(BodyNodeKind::Stomach, 25.0).with_tags(vec![Digest]),
        BodyNode::new(BodyNodeKind::Gut, 25.0).with_tags(vec![Digest]),
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
    // Internal organs — head
    Brain,
    LeftEye,
    RightEye,
    LeftEar,
    RightEar,
    Nose,
    Jaw,
    // Internal organs — torso
    Heart,
    LeftLung,
    RightLung,
    Liver,
    Stomach,
    Gut,
    // Extremities (children of limbs)
    LeftHand,
    RightHand,
    LeftFoot,
    RightFoot,
    LeftForepaw,
    RightForepaw,
    LeftHindpaw,
    RightHindpaw,
    LeftForehoof,
    RightForehoof,
    LeftHindhoof,
    RightHindhoof,
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
            Self::Brain => "brain",
            Self::LeftEye => "left eye",
            Self::RightEye => "right eye",
            Self::LeftEar => "left ear",
            Self::RightEar => "right ear",
            Self::Nose => "nose",
            Self::Jaw => "jaw",
            Self::Heart => "heart",
            Self::LeftLung => "left lung",
            Self::RightLung => "right lung",
            Self::Liver => "liver",
            Self::Stomach => "stomach",
            Self::Gut => "gut",
            Self::LeftHand => "left hand",
            Self::RightHand => "right hand",
            Self::LeftFoot => "left foot",
            Self::RightFoot => "right foot",
            Self::LeftForepaw => "left forepaw",
            Self::RightForepaw => "right forepaw",
            Self::LeftHindpaw => "left hindpaw",
            Self::RightHindpaw => "right hindpaw",
            Self::LeftForehoof => "left forehoof",
            Self::RightForehoof => "right forehoof",
            Self::LeftHindhoof => "left hindhoof",
            Self::RightHindhoof => "right hindhoof",
        }
    }
}

// ─── FunctionalTag ─────────────────────────────────────────────────────────

/// What a body node *does* biologically. Tags are declarative labels that
/// describe function without specifying magnitude — the mapping layer (#452)
/// will derive channel capacities from tags. For now they coexist with the
/// existing `provides` channel declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum FunctionalTag {
    Think,
    See,
    Hear,
    Smell,
    Grasp,
    Stance,
    Speak,
    Eat,
    Bite,
    Carry,
    Breathe,
    Pump,
    Digest,
    Filter,
    FullBody,
}

// ─── TagChannelMapping ─────────────────────────────────────────────────────

/// Maps functional tags to the channels they contribute to and at what
/// per-node intensity. A node with tag `See` contributes `Awareness` at the
/// configured intensity per See-tagged node. This is the single source of
/// truth for tag-to-channel derivation.
///
/// Stored as a Bevy Resource so it can be overridden for modding.
#[derive(Resource, Debug, Clone, Reflect)]
pub struct TagChannelMapping {
    entries: Vec<TagChannelEntry>,
}

#[derive(Debug, Clone, Reflect)]
struct TagChannelEntry {
    tag: FunctionalTag,
    channel: Channel,
    intensity: f32,
}

impl Default for TagChannelMapping {
    fn default() -> Self {
        use FunctionalTag::*;
        Self {
            entries: vec![
                TagChannelEntry {
                    tag: Think,
                    channel: Channel::Focus,
                    intensity: 1.0,
                },
                TagChannelEntry {
                    tag: Think,
                    channel: Channel::Awareness,
                    intensity: 0.5,
                },
                TagChannelEntry {
                    tag: See,
                    channel: Channel::Awareness,
                    intensity: 0.15,
                },
                TagChannelEntry {
                    tag: Hear,
                    channel: Channel::Awareness,
                    intensity: 0.1,
                },
                TagChannelEntry {
                    tag: Grasp,
                    channel: Channel::Manipulation,
                    intensity: 0.5,
                },
                TagChannelEntry {
                    tag: Carry,
                    channel: Channel::Carry,
                    intensity: 0.25,
                },
                TagChannelEntry {
                    tag: Stance,
                    channel: Channel::Locomotion,
                    intensity: 0.5,
                },
                TagChannelEntry {
                    tag: Speak,
                    channel: Channel::Vocalization,
                    intensity: 1.0,
                },
                TagChannelEntry {
                    tag: Eat,
                    channel: Channel::Consumption,
                    intensity: 1.0,
                },
                TagChannelEntry {
                    tag: Bite,
                    channel: Channel::Bite,
                    intensity: 1.0,
                },
                TagChannelEntry {
                    tag: FullBody,
                    channel: Channel::FullBody,
                    intensity: 1.0,
                },
            ],
        }
    }
}

impl TagChannelMapping {
    /// Compute channel capacity for a single channel across the whole body tree.
    pub fn channel_capacity(&self, body: &Body, channel: Channel) -> f32 {
        let mut total = 0.0;
        for part in &body.parts {
            total += self.node_contribution(part, channel);
            for child in &part.children {
                total += self.node_contribution(child, channel);
            }
        }
        total
    }

    fn node_contribution(&self, node: &BodyNode, channel: Channel) -> f32 {
        let mut contrib = 0.0;
        for entry in &self.entries {
            if entry.channel == channel && node.has_tag(entry.tag) {
                contrib += entry.intensity;
            }
        }
        contrib * node.function_rate
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
    pub tags: Vec<FunctionalTag>,
    pub vital: bool,
    pub max_hp: f32,
    pub current_hp: f32,
    pub function_rate: f32,
    pub injuries: Vec<Injury>,
    pub children: Vec<BodyNode>,
}

impl BodyNode {
    pub fn new(kind: BodyNodeKind, max_hp: f32) -> Self {
        Self {
            kind,
            tags: Vec::new(),
            vital: false,
            max_hp,
            current_hp: max_hp,
            function_rate: 1.0,
            injuries: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn vital(kind: BodyNodeKind, max_hp: f32) -> Self {
        let mut node = Self::new(kind, max_hp);
        node.vital = true;
        node
    }

    pub fn with_tags(mut self, tags: Vec<FunctionalTag>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_children(mut self, children: Vec<BodyNode>) -> Self {
        self.children = children;
        self
    }

    pub fn has_tag(&self, tag: FunctionalTag) -> bool {
        self.tags.contains(&tag)
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
        let heart = BodyNode::vital(BodyNodeKind::Heart, 40.0);
        assert!((heart.condition() - 1.0).abs() < 1e-6);
        assert!(!heart.is_destroyed());
        assert!(heart.vital);
    }

    #[test]
    fn zero_hp_organ_is_destroyed() {
        let mut lung = BodyNode::vital(BodyNodeKind::LeftLung, 18.0);
        lung.current_hp = 0.0;
        lung.recalculate_function();
        assert_eq!(lung.condition(), 0.0);
        assert!(lung.is_destroyed());
    }

    #[test]
    fn organ_condition_clamps_to_unit_interval() {
        let mut gut = BodyNode::new(BodyNodeKind::Gut, 25.0);
        gut.current_hp = 100.0;
        assert!((gut.condition() - 1.0).abs() < 1e-6);
        gut.current_hp = -5.0;
        assert_eq!(gut.condition(), 0.0);
    }

    #[test]
    fn human_has_expected_children_in_head_and_torso() {
        let body = Body::human();

        let head = body.part(BodyNodeKind::Head).expect("human has a head");
        let head_kinds: Vec<BodyNodeKind> = head.children.iter().map(|o| o.kind).collect();
        assert_eq!(
            head_kinds,
            vec![
                BodyNodeKind::Brain,
                BodyNodeKind::LeftEye,
                BodyNodeKind::RightEye,
                BodyNodeKind::LeftEar,
                BodyNodeKind::RightEar,
                BodyNodeKind::Nose,
                BodyNodeKind::Jaw,
            ]
        );

        let torso = body.part(BodyNodeKind::Torso).expect("human has a torso");
        let torso_kinds: Vec<BodyNodeKind> = torso.children.iter().map(|o| o.kind).collect();
        assert_eq!(
            torso_kinds,
            vec![
                BodyNodeKind::Heart,
                BodyNodeKind::LeftLung,
                BodyNodeKind::RightLung,
                BodyNodeKind::Liver,
                BodyNodeKind::Stomach,
                BodyNodeKind::Gut,
            ]
        );

        let left_arm = body
            .part(BodyNodeKind::LeftArm)
            .expect("human has a left arm");
        assert_eq!(left_arm.children.len(), 1);
        assert_eq!(left_arm.children[0].kind, BodyNodeKind::LeftHand);
    }

    #[test]
    fn wolf_and_deer_also_carry_head_and_torso_organs() {
        for body in [Body::wolf(), Body::deer()] {
            assert!(body.node(BodyNodeKind::Brain).is_some());
            assert!(body.node(BodyNodeKind::Heart).is_some());
            assert!(body.node(BodyNodeKind::LeftLung).is_some());
            assert!(body.node(BodyNodeKind::RightLung).is_some());
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
    fn all_species_have_expected_child_count() {
        // Human: 7 head + 6 torso + 1 per limb (4 limbs) = 17
        let human = Body::human();
        let count: usize = human.parts.iter().map(|p| p.children.len()).sum();
        assert_eq!(count, 17, "human: 7 head + 6 torso + 4 extremities");

        // Wolf: 7 head + 6 torso + 1 per leg (4 legs) = 17
        let wolf = Body::wolf();
        let count: usize = wolf.parts.iter().map(|p| p.children.len()).sum();
        assert_eq!(count, 17, "wolf: 7 head + 6 torso + 4 paws");

        // Deer: 7 head + 6 torso + 1 per leg (4 legs) = 17
        let deer = Body::deer();
        let count: usize = deer.parts.iter().map(|p| p.children.len()).sum();
        assert_eq!(count, 17, "deer: 7 head + 6 torso + 4 hooves");
    }

    #[test]
    fn lung_condition_averages_both_lungs() {
        let healthy = Body::human();
        assert!((healthy.lung_condition() - 1.0).abs() < 1e-6);

        let mut damaged = Body::human();
        // Destroy left lung, leave right intact: average = 0.5
        damaged
            .node_mut(BodyNodeKind::LeftLung)
            .expect("humans have left lung")
            .current_hp = 0.0;
        assert!(
            (damaged.lung_condition() - 0.5).abs() < 1e-6,
            "one destroyed lung should report 0.5, got {}",
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

    #[test]
    fn destroying_one_eye_reduces_awareness() {
        let m = TagChannelMapping::default();
        let body = Body::human();
        let full_awareness = body.channel_capacity(Channel::Awareness, &m);

        let mut damaged = Body::human();
        damaged
            .node_mut(BodyNodeKind::LeftEye)
            .expect("humans have left eye")
            .current_hp = 0.0;
        damaged
            .node_mut(BodyNodeKind::LeftEye)
            .unwrap()
            .recalculate_function();
        let reduced = damaged.channel_capacity(Channel::Awareness, &m);
        assert!(
            reduced < full_awareness,
            "losing an eye should reduce awareness ({full_awareness} -> {reduced})"
        );
        let diff = full_awareness - reduced;
        assert!(
            (diff - 0.15).abs() < 1e-6,
            "left eye contributes 0.15 awareness, got diff {diff}"
        );
    }

    #[test]
    fn brain_injury_reduces_focus() {
        let m = TagChannelMapping::default();
        let body = Body::human();
        let full_focus = body.channel_capacity(Channel::Focus, &m);

        let mut damaged = Body::human();
        let brain = damaged
            .node_mut(BodyNodeKind::Brain)
            .expect("humans have brain");
        brain.current_hp = brain.max_hp * 0.5;
        brain.recalculate_function();
        let reduced = damaged.channel_capacity(Channel::Focus, &m);
        assert!(
            reduced < full_focus,
            "brain damage should reduce focus ({full_focus} -> {reduced})"
        );
    }

    #[test]
    fn losing_hand_halves_manipulation() {
        let m = TagChannelMapping::default();
        let body = Body::human();
        let full = body.channel_capacity(Channel::Manipulation, &m);
        assert!((full - 1.0).abs() < 1e-6);

        let mut damaged = Body::human();
        damaged
            .node_mut(BodyNodeKind::LeftHand)
            .expect("humans have left hand")
            .current_hp = 0.0;
        damaged
            .node_mut(BodyNodeKind::LeftHand)
            .unwrap()
            .recalculate_function();
        let reduced = damaged.channel_capacity(Channel::Manipulation, &m);
        assert!(
            (reduced - 0.5).abs() < 1e-6,
            "losing a hand should halve manipulation, got {reduced}"
        );
    }

    #[test]
    fn jaw_is_under_head_for_all_species() {
        for (name, body) in [
            ("human", Body::human()),
            ("wolf", Body::wolf()),
            ("deer", Body::deer()),
        ] {
            let head = body.part(BodyNodeKind::Head).unwrap();
            assert!(
                head.children.iter().any(|c| c.kind == BodyNodeKind::Jaw),
                "{name} jaw should be a child of Head"
            );
        }
    }

    #[test]
    fn head_wound_without_organ_damage_has_no_cognitive_effect() {
        let m = TagChannelMapping::default();
        let mut body = Body::human();
        let full_focus = body.channel_capacity(Channel::Focus, &m);
        let full_awareness = body.channel_capacity(Channel::Awareness, &m);

        // Damage head HP but don't touch children
        let head = body.part_mut(BodyNodeKind::Head).unwrap();
        head.current_hp = head.max_hp * 0.3;
        head.recalculate_function();

        // Head itself provides no channels, so cognitive channels unchanged
        assert!(
            (body.channel_capacity(Channel::Focus, &m) - full_focus).abs() < 1e-6,
            "head wound alone should not reduce focus"
        );
        assert!(
            (body.channel_capacity(Channel::Awareness, &m) - full_awareness).abs() < 1e-6,
            "head wound alone should not reduce awareness"
        );
    }

    // ─── Functional tag tests ──────────────────────────────────────────

    #[test]
    fn each_species_has_expected_tags_on_key_nodes() {
        use FunctionalTag::*;
        for (name, body) in [
            ("human", Body::human()),
            ("wolf", Body::wolf()),
            ("deer", Body::deer()),
        ] {
            let brain = body.node(BodyNodeKind::Brain).unwrap();
            assert!(brain.has_tag(Think), "{name} brain should have Think");

            let left_eye = body.node(BodyNodeKind::LeftEye).unwrap();
            assert!(left_eye.has_tag(See), "{name} left eye should have See");

            let left_ear = body.node(BodyNodeKind::LeftEar).unwrap();
            assert!(left_ear.has_tag(Hear), "{name} left ear should have Hear");

            let nose = body.node(BodyNodeKind::Nose).unwrap();
            assert!(nose.has_tag(Smell), "{name} nose should have Smell");

            let heart = body.node(BodyNodeKind::Heart).unwrap();
            assert!(heart.has_tag(Pump), "{name} heart should have Pump");

            let left_lung = body.node(BodyNodeKind::LeftLung).unwrap();
            assert!(
                left_lung.has_tag(Breathe),
                "{name} left lung should have Breathe"
            );

            let stomach = body.node(BodyNodeKind::Stomach).unwrap();
            assert!(stomach.has_tag(Digest), "{name} stomach should have Digest");

            let liver = body.node(BodyNodeKind::Liver).unwrap();
            assert!(liver.has_tag(Filter), "{name} liver should have Filter");

            let jaw = body.node(BodyNodeKind::Jaw).unwrap();
            assert!(jaw.has_tag(Eat), "{name} jaw should have Eat");
            assert!(jaw.has_tag(Speak), "{name} jaw should have Speak");
        }
    }

    #[test]
    fn nodes_with_tag_returns_both_eyes() {
        use FunctionalTag::*;
        let body = Body::human();
        let see_nodes = body.nodes_with_tag(See);
        assert_eq!(see_nodes.len(), 2, "human has two See nodes (L/R eyes)");
        let kinds: Vec<BodyNodeKind> = see_nodes.iter().map(|n| n.kind).collect();
        assert!(kinds.contains(&BodyNodeKind::LeftEye));
        assert!(kinds.contains(&BodyNodeKind::RightEye));
    }

    #[test]
    fn tags_survive_injury() {
        use FunctionalTag::*;
        let mut body = Body::human();
        let eye = body.node_mut(BodyNodeKind::LeftEye).unwrap();
        eye.current_hp = 0.0;
        eye.recalculate_function();
        assert!(
            body.node(BodyNodeKind::LeftEye).unwrap().has_tag(See),
            "destroyed eye still has See tag"
        );
    }

    #[test]
    fn wolf_jaw_has_bite_and_grasp_but_deer_jaw_does_not() {
        use FunctionalTag::*;
        let wolf = Body::wolf();
        let wolf_jaw = wolf.node(BodyNodeKind::Jaw).unwrap();
        assert!(wolf_jaw.has_tag(Bite));
        assert!(wolf_jaw.has_tag(Grasp));

        let deer = Body::deer();
        let deer_jaw = deer.node(BodyNodeKind::Jaw).unwrap();
        assert!(!deer_jaw.has_tag(Bite), "deer jaw should not have Bite");
        assert!(!deer_jaw.has_tag(Grasp), "deer jaw should not have Grasp");
    }

    #[test]
    fn human_hands_have_grasp_and_carry() {
        use FunctionalTag::*;
        let body = Body::human();
        let grasp_nodes = body.nodes_with_tag(Grasp);
        assert_eq!(
            grasp_nodes.len(),
            2,
            "human has two Grasp nodes (L/R hands)"
        );
        let carry_nodes = body.nodes_with_tag(Carry);
        assert_eq!(
            carry_nodes.len(),
            2,
            "human has two Carry nodes (L/R hands)"
        );
    }

    #[test]
    fn stance_nodes_match_species_locomotion() {
        use FunctionalTag::*;
        assert_eq!(
            Body::human().nodes_with_tag(Stance).len(),
            2,
            "human: 2 feet"
        );
        assert_eq!(Body::wolf().nodes_with_tag(Stance).len(), 4, "wolf: 4 paws");
        assert_eq!(
            Body::deer().nodes_with_tag(Stance).len(),
            4,
            "deer: 4 hooves"
        );
    }
}
