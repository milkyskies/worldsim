//! Agent knowledge graph: MindGraph stores the agent's beliefs as subject-predicate-object triples.
//!
//! Reads: world observations (via perception), action outcomes (via belief_updater), shared knowledge (via conversation)
//! Writes: MindGraph (triple store), Ontology (concept hierarchy), Node, Predicate, Value types
//! Upstream: perception (observes world), belief_updater (processes action outcomes), conversation (receives shared triples)
//! Downstream: all brain systems, thinking (TriplePattern queries), belief_state, nervous_system::cns

use bevy::prelude::*;
use smallvec::SmallVec;
use std::collections::HashMap;
use std::sync::Arc;

// ═══════════════════════════════════════════════════════════════════════════
// NODES — What can be subject or object in a triple
// ═══════════════════════════════════════════════════════════════════════════

/// A named area (e.g., "Forest", "River Bank").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Reflect)]
pub struct AreaId(pub String);

impl std::fmt::Display for AreaId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A typed agent name — prevents accidental comparison against arbitrary strings.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Reflect)]
pub struct AgentName(pub String);

impl std::fmt::Display for AgentName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Cardinal/ordinal direction for imprecise perception (hearing, smell).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum CardinalDirection {
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

impl CardinalDirection {
    /// Convert a 2D offset vector into the nearest cardinal/ordinal direction.
    pub fn from_vec2(dir: Vec2) -> Self {
        let angle = dir.y.atan2(dir.x).to_degrees();
        // Normalize to [0, 360)
        let angle = if angle < 0.0 { angle + 360.0 } else { angle };
        match angle as u32 {
            0..=22 | 338..=360 => CardinalDirection::East,
            23..=67 => CardinalDirection::NorthEast,
            68..=112 => CardinalDirection::North,
            113..=157 => CardinalDirection::NorthWest,
            158..=202 => CardinalDirection::West,
            203..=247 => CardinalDirection::SouthWest,
            248..=292 => CardinalDirection::South,
            _ => CardinalDirection::SouthEast,
        }
    }
}

/// A node in the knowledge graph — can be a subject or object
#[derive(Debug, Clone, PartialEq, Eq, Hash, Reflect)]
pub enum Node {
    /// A specific game entity (Tree42, Alice)
    Entity(Entity),
    /// Abstract concept (Food, Friendly)
    Concept(Concept),
    /// A tile location
    Tile((i32, i32)),
    /// A 16x16 chunk coordinate
    Chunk((i32, i32)),
    /// A named area (e.g., "Forest", "River")
    Area(AreaId),
    /// A remembered event
    Event(u64),
    /// The agent who owns this MindGraph (self-reference)
    Self_,
    /// An action type (e.g. Wave, Eat)
    Action(crate::agent::actions::ActionType),
    /// A cardinal/ordinal direction (for non-precise perception like hearing)
    Direction(CardinalDirection),
}

// ═══════════════════════════════════════════════════════════════════════════
// CONCEPTS — Unified enum for all describable things
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Default)]
pub enum Concept {
    // ─── Base categories ───
    #[default]
    Thing, // Root of everything
    Physical, // Has physical presence
    Abstract, // Ideas, concepts

    // ─── Entity types (nouns) ───
    Person,
    Animal,
    Plant,
    Object,

    // ─── Resource types ───
    Food,
    Resource,
    Apple,
    AppleTree,
    Berry,
    BerryBush,
    Wood,
    WoodLog,
    Water,
    Stone,
    StoneNode,
    Stick,

    // ─── Rotten / spoiled variants ───
    RottenApple,
    RottenBerry,

    // ─── Buildable entity types ───
    Campfire,
    LeanTo,

    // ─── Remains / byproducts ───
    Ash,

    // ─── Transformation intermediates ───
    /// A partially-built world entity that becomes a finished structure when its
    /// Construction slots are filled. Sites are the first concrete use of the
    /// `Becomes` substrate (#61).
    ConstructionSite,

    // ─── Abstract needs / states ───
    Safety,
    Warmth,
    Light,

    // ─── Plant materials ───
    LargeLeaves,

    // ─── Animal types ───
    Deer,
    Wolf,

    // ─── Sound kinds (perceived via hearing) ───
    Howl,
    AlarmCall,
    Scream,
    CombatSound,

    // ─── Traits/Properties (adjectives) ───
    Edible,    // Items that can be eaten (Apple, Berry, Meat)
    Drinkable, // Tiles/items that can provide water (ShallowWater, Water)
    Grazable,  // Tiles that can be grazed on (Grass) — drifting herbivore forage
    Prey,      // Creatures that can be hunted (Deer, Rabbit) → yields Meat
    Territory, // A tile the agent claims as its own (marked intrinsically at spawn)
    Dangerous,
    Safe,
    Friendly,
    Hostile,
    Neutral,
    Sentient,
    Harvestable,
    Awake,
    Asleep,

    // ─── Property traits (auto-derived from ECS components via define_property_component!) ───
    LightEmitting,    // Entity emits light (e.g. campfire, torch)
    HeatEmitting,     // Entity emits heat (e.g. campfire, fire)
    ShelterProviding, // Entity provides shelter from weather (e.g. lean-to, cave)
    Flammable,        // Entity can catch fire and burn
    FuelConsuming,    // Entity consumes fuel to function (e.g. campfire)
    Degradable,       // Entity degrades over time and despawns at zero durability
    ManMade,          // Entity was built by an agent (vs spawned by world generation)

    // ─── Action categories ───
    SocialAction,
    ViolentAction,
    SurvivalAction,
    MovementAction,

    // ─── Apparent Moods (visible expressions) ───
    HappyMood,
    SadMood,
    AngryMood,
    FearfulMood,
    NeutralMood,

    // ─── Relationship Categories ───
    Stranger,     // Never met
    Acquaintance, // Met but not close
    Friend,       // High affection + trust
    Rival,        // Competition
    Enemy,        // Active hostility
}

// ═══════════════════════════════════════════════════════════════════════════
// PREDICATES — Relationships between nodes
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum Predicate {
    // ─── Classification ───
    IsA,      // (Apple, IsA, Food)
    HasTrait, // (Wolf, HasTrait, Dangerous)

    // ─── Spatial ───
    LocatedAt, // (Tree42, LocatedAt, Tile(5,3))
    Contains,  // (Tree42, Contains, Apple(3))

    // ─── Action Semantics ───
    Affords,   // (AppleTree, Affords, Harvest)
    Produces,  // (AppleTree, Produces, Apple)
    Consumes,  // (Eat, Consumes, Food)
    Satisfies, // (Eat, Satisfies, Hunger)
    Requires,  // (Harvest, Requires, AtLocation)
    Provides,  // (Campfire, Provides, Warmth)
    BuildTime, // (Campfire, BuildTime, 120) — ticks to construct
    Becomes,   // (ConstructionSite_42, Becomes, Campfire) — observed transformation rule

    // ─── Temporal ───
    RegenerationRate, // (AppleTree, RegenerationRate, 10.0)
    LastObserved,     // (Tree42, LastObserved, 50000)

    // ─── Agent state ───
    Hunger,      // (Self, Hunger, Int)
    Thirst,      // (Self, Thirst, Int)
    Energy,      // (Self, Energy, Int)
    Pain,        // (Self, Pain, Int)
    SocialDrive, // (Self, SocialDrive, Int) - 0 = satisfied, 100 = lonely

    // ─── Episodic Memory ───
    Actor,       // (Event42, Actor, Bob)
    Action,      // (Event42, Action, Attack)
    Target,      // (Event42, Target, Self_)
    Result,      // (Event42, Result, Damaged)
    Timestamp,   // (Event42, Timestamp, 1000)
    FeltEmotion, // (Event42, FeltEmotion, Fear(0.8))

    // ─── Social ───
    Relationship, // (Self, Relationship, Bob) → Attitude(0.7)
    TrustsFor,    // (Bob, TrustsFor, FoodKnowledge)
    Knows,        // (Self, Knows, Entity) - have we met?
    Introduced,   // (Self, Introduced, Entity) - exchanged names?
    NameOf,       // (Entity, NameOf, String) - what's their name?

    // ─── Relationship Dimensions ───
    Trust,        // (Entity, Trust, Float) - 0.0 to 1.0
    Affection,    // (Entity, Affection, Float) - 0.0 to 1.0
    Respect,      // (Entity, Respect, Float) - 0.0 to 1.0
    PowerBalance, // (Entity, PowerBalance, Float) - -1.0 to 1.0

    // ─── Social Perception ───
    Doing,          // (Entity, Doing, Action) - current activity
    AppearsMood,    // (Entity, AppearsMood, Concept) - visible mood
    AppearsInjured, // (Entity, AppearsInjured, Boolean)
    Heading,        // (Entity, Heading, Tile) - movement direction

    // ─── Exploration ───
    Explored, // (Tile(x,y), Explored, Timestamp) - agent has seen this tile

    // ─── Emotional ───
    TriggersEmotion, // (Wolf, TriggersEmotion, Fear(0.6))

    // ─── Sensory ───
    ProducedSound, // (Direction::North, ProducedSound, Concept(Howl)) — heard a sound
    EmitsHeat,     // (Tile(x,y), EmitsHeat, Float(intensity)) — felt warmth
}

impl Predicate {
    pub fn is_functional(&self) -> bool {
        matches!(
            self,
            Predicate::LocatedAt
                | Predicate::Hunger
                | Predicate::Thirst
                | Predicate::Energy
                | Predicate::RegenerationRate
                | Predicate::LastObserved
                | Predicate::Actor
                | Predicate::Action
                | Predicate::Target
                | Predicate::Result
                | Predicate::Timestamp
                // Relationship dimensions are functional (one value per entity)
                | Predicate::Trust
                | Predicate::Affection
                | Predicate::Respect
                | Predicate::PowerBalance
                | Predicate::NameOf
                // Social perception (one current state per entity)
                | Predicate::Doing
                | Predicate::AppearsMood
                | Predicate::AppearsInjured
                | Predicate::Heading
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// VALUES — What predicates evaluate to
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Reflect)]
pub enum Value {
    Boolean(bool), // Truth value
    Int(i32),
    Float(f32),
    Concept(Concept),
    Entity(Entity),
    Tile((i32, i32)),
    Action(crate::agent::actions::ActionType),
    Emotion(crate::agent::psyche::emotions::EmotionType, f32),
    Item(Concept, u32), // (Apple, 5) - quantity
    Attitude(f32),      // -1.0 to 1.0 (hate to love)
    Text(AgentName),    // For agent names
}

impl Value {
    pub fn as_concept(&self) -> Option<Concept> {
        match self {
            Value::Concept(c) => Some(*c),
            _ => None,
        }
    }

    pub fn as_entity(&self) -> Option<Entity> {
        match self {
            Value::Entity(e) => Some(*e),
            _ => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// MEMORY TYPES
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Default)]
pub enum MemoryType {
    /// Universal truths (laws of physics, logic)
    /// Never decays. Shared across all agents.
    #[default]
    Intrinsic,

    /// Knowledge taught by culture/parents
    /// Slow decay. Agents start with this.
    Cultural,

    /// Personal beliefs inferred from experience
    /// Medium decay. "Bob is hostile", "Trees have apples"
    Semantic,

    /// Specific remembered events
    /// Faster decay (unless emotionally significant)
    Episodic,

    /// Skills and procedural knowledge
    /// Very slow decay. "I can forge swords"
    Procedural,

    /// Current perceptions
    /// Instant decay when out of sight
    Perception,
}

// ═══════════════════════════════════════════════════════════════════════════
// METADATA — Information about the knowledge
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, Default)]
pub enum Source {
    #[default]
    Intrinsic, // Laws of the universe
    Cultural,     // Taught by society
    Communicated, // Someone told me
    Hearsay,      // Learned from conversation (less trustworthy)
    Observed,     // I saw it happen (to others)
    Experienced,  // I did it / it happened to me
    Inferred,     // I deduced from patterns
    Perception,   // I see it right now
}

// ═══════════════════════════════════════════════════════════════════════════
// SENSE — Which perceptual channel produced a triple
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum Sense {
    /// Requires line-of-sight, high precision, medium range (~256px)
    Sight,
    /// No line-of-sight needed, short range (~64px), detects warmth zones
    Temperature,
    /// No line-of-sight needed, long range (~512px), direction only
    Hearing,
    /// Future: wind-dependent, medium range, no direction
    Smell,
}

#[derive(Debug, Clone, Reflect)]
pub struct Metadata {
    /// How did I learn this?
    pub source: Source,

    /// What kind of memory is this?
    pub memory_type: MemoryType,

    /// When did I learn this? (game time in ms)
    pub timestamp: u64,

    /// How confident am I? (0.0 to 1.0)
    pub confidence: f32,

    /// Who told me? (for communicated knowledge)
    pub informant: Option<Entity>,

    /// What events support this belief?
    pub evidence: Vec<u64>,

    /// How emotionally significant? (affects decay)
    pub salience: f32,

    /// Which sense produced this triple (None for non-perceptual knowledge)
    pub source_sense: Option<Sense>,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            source: Source::Intrinsic,
            memory_type: MemoryType::Intrinsic,
            timestamp: 0,
            confidence: 1.0,
            informant: None,
            evidence: Vec::new(),
            salience: 0.0,
            source_sense: None,
        }
    }
}

impl Metadata {
    pub fn perception(timestamp: u64) -> Self {
        Self {
            source: Source::Perception,
            memory_type: MemoryType::Perception,
            timestamp,
            confidence: 1.0,
            informant: None,
            evidence: Vec::new(),
            salience: 0.0,
            source_sense: None,
        }
    }

    pub fn perception_sense_conf(timestamp: u64, confidence: f32, sense: Sense) -> Self {
        Self {
            source: Source::Perception,
            memory_type: MemoryType::Perception,
            timestamp,
            confidence,
            informant: None,
            evidence: Vec::new(),
            salience: 0.0,
            source_sense: Some(sense),
        }
    }

    pub fn semantic(timestamp: u64) -> Self {
        Self {
            source: Source::Inferred,
            memory_type: MemoryType::Semantic,
            timestamp,
            confidence: 1.0,
            informant: None,
            evidence: Vec::new(),
            salience: 0.0,
            source_sense: None,
        }
    }

    pub fn perception_with_conf(timestamp: u64, confidence: f32) -> Self {
        Self {
            source: Source::Perception,
            memory_type: MemoryType::Perception,
            timestamp,
            confidence,
            informant: None,
            evidence: Vec::new(),
            salience: 0.0,
            source_sense: None,
        }
    }

    pub fn experience(timestamp: u64) -> Self {
        Self {
            source: Source::Experienced,
            memory_type: MemoryType::Semantic,
            timestamp,
            confidence: 1.0,
            informant: None,
            evidence: Vec::new(),
            salience: 0.0,
            source_sense: None,
        }
    }

    pub fn inference(timestamp: u64, confidence: f32) -> Self {
        Self {
            source: Source::Inferred,
            memory_type: MemoryType::Semantic,
            timestamp,
            confidence,
            informant: None,
            evidence: Vec::new(),
            salience: 0.0,
            source_sense: None,
        }
    }

    /// Knowledge learned from another agent during a conversation.
    /// Confidence starts lower than direct experience and the informant is
    /// recorded so consumers can weight by trust.
    pub fn hearsay(timestamp: u64, informant: Entity) -> Self {
        Self {
            source: Source::Hearsay,
            memory_type: MemoryType::Semantic,
            timestamp,
            confidence: 0.7,
            informant: Some(informant),
            evidence: Vec::new(),
            salience: 0.0,
            source_sense: None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// TRIPLE — A single piece of knowledge
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Reflect)]
pub struct Triple {
    pub subject: Node,
    pub predicate: Predicate,
    pub object: Value,
    pub meta: Metadata,
}

impl Triple {
    pub fn new(subject: Node, predicate: Predicate, object: Value) -> Self {
        Self {
            subject,
            predicate,
            object,
            meta: Metadata::default(),
        }
    }

    pub fn with_meta(subject: Node, predicate: Predicate, object: Value, meta: Metadata) -> Self {
        Self {
            subject,
            predicate,
            object,
            meta,
        }
    }
}

// NOTE: Ontology is now defined in the ONTOLOGY section below with caching support

// ═══════════════════════════════════════════════════════════════════════════
// MINDGRAPH — Triple store with subject / predicate / (subject,predicate) indexes
// ═══════════════════════════════════════════════════════════════════════════

/// SmallVec sizing:
/// - subject/predicate fan-out is usually small per agent — 8 covers the common case.
/// - (subject, predicate) pairs are tighter still — 4 covers almost everything.
type IdxList = SmallVec<[usize; 8]>;
type SubjPredIdxList = SmallVec<[usize; 4]>;

#[derive(Component, Clone, Reflect, Default)]
#[reflect(Component)]
pub struct MindGraph {
    /// Shared universal truths (read-only, Arc for cheap clone)
    #[reflect(ignore)]
    pub ontology: Ontology,

    /// Shared cultural/social knowledge blocks
    #[reflect(ignore)]
    pub shared_knowledge: Vec<Arc<Vec<Triple>>>,

    /// Canonical per-agent triple storage. `None` = tombstoned slot.
    /// Indices into this vector are stable across assert/remove (only `compact()`
    /// rewrites them), which lets indexes store bare `usize` ids.
    triples: Vec<Option<Triple>>,

    /// Number of tombstoned slots — reclaimed by `compact()`.
    tombstone_count: usize,

    /// Subject → live triple ids.
    #[reflect(ignore)]
    by_subject: HashMap<Node, IdxList>,
    /// Predicate → live triple ids.
    #[reflect(ignore)]
    by_predicate: HashMap<Predicate, IdxList>,
    /// (Subject, Predicate) → live triple ids. Most brain queries hit this one.
    #[reflect(ignore)]
    by_subject_predicate: HashMap<(Node, Predicate), SubjPredIdxList>,
}

impl MindGraph {
    pub fn new(ontology: Ontology) -> Self {
        Self {
            ontology,
            shared_knowledge: Vec::new(),
            triples: Vec::new(),
            tombstone_count: 0,
            by_subject: HashMap::new(),
            by_predicate: HashMap::new(),
            by_subject_predicate: HashMap::new(),
        }
    }

    pub fn add_shared_knowledge(&mut self, knowledge: Arc<Vec<Triple>>) {
        self.shared_knowledge.push(knowledge);
    }

    // ─── Accessors ──────────────────────────────────────────────────────────

    /// Number of LIVE personal triples (excludes tombstones).
    pub fn len(&self) -> usize {
        self.triples.len() - self.tombstone_count
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Total number of slots in the triple vector, including tombstones.
    /// Exposed for compaction heuristics and tests.
    pub fn total_slots(&self) -> usize {
        self.triples.len()
    }

    pub fn tombstone_count(&self) -> usize {
        self.tombstone_count
    }

    /// Iterate over live personal triples only.
    pub fn iter(&self) -> impl Iterator<Item = &Triple> {
        self.triples.iter().filter_map(|slot| slot.as_ref())
    }

    /// Resolve a slice of slot ids into live triples, skipping any that have
    /// been tombstoned out from under us. Used by indexed query paths.
    fn live_at<'a>(&'a self, ids: &'a [usize]) -> impl Iterator<Item = &'a Triple> + 'a {
        ids.iter()
            .filter_map(|&i| self.triples.get(i).and_then(|s| s.as_ref()))
    }

    // ─── Index bookkeeping ──────────────────────────────────────────────────

    fn index_insert(&mut self, idx: usize, subject: &Node, predicate: Predicate) {
        self.by_subject
            .entry(subject.clone())
            .or_default()
            .push(idx);
        self.by_predicate.entry(predicate).or_default().push(idx);
        self.by_subject_predicate
            .entry((subject.clone(), predicate))
            .or_default()
            .push(idx);
    }

    fn index_remove(&mut self, idx: usize, subject: &Node, predicate: Predicate) {
        if let Some(list) = self.by_subject.get_mut(subject) {
            list.retain(|i| *i != idx);
            if list.is_empty() {
                self.by_subject.remove(subject);
            }
        }
        if let Some(list) = self.by_predicate.get_mut(&predicate) {
            list.retain(|i| *i != idx);
            if list.is_empty() {
                self.by_predicate.remove(&predicate);
            }
        }
        let key = (subject.clone(), predicate);
        if let Some(list) = self.by_subject_predicate.get_mut(&key) {
            list.retain(|i| *i != idx);
            if list.is_empty() {
                self.by_subject_predicate.remove(&key);
            }
        }
    }

    /// Tombstone the slot at `idx`. Assumes it is currently live.
    fn tombstone(&mut self, idx: usize) {
        if let Some(slot) = self.triples.get_mut(idx)
            && let Some(triple) = slot.take()
        {
            self.tombstone_count += 1;
            self.index_remove(idx, &triple.subject, triple.predicate);
        }
    }

    /// Rebuild indexes and drop tombstoned slots. Triple ids are invalidated.
    pub fn compact(&mut self) {
        if self.tombstone_count == 0 {
            return;
        }
        let mut new_triples: Vec<Option<Triple>> = Vec::with_capacity(self.len());
        for slot in self.triples.drain(..) {
            if slot.is_some() {
                new_triples.push(slot);
            }
        }
        self.triples = new_triples;
        self.tombstone_count = 0;
        self.rebuild_indexes();
    }

    /// Rebuild the subject / predicate / (subject, predicate) indexes from the
    /// current triple vector.
    fn rebuild_indexes(&mut self) {
        self.by_subject.clear();
        self.by_predicate.clear();
        self.by_subject_predicate.clear();
        for (i, slot) in self.triples.iter().enumerate() {
            if let Some(triple) = slot {
                self.by_subject
                    .entry(triple.subject.clone())
                    .or_default()
                    .push(i);
                self.by_predicate
                    .entry(triple.predicate)
                    .or_default()
                    .push(i);
                self.by_subject_predicate
                    .entry((triple.subject.clone(), triple.predicate))
                    .or_default()
                    .push(i);
            }
        }
    }

    // ─── Mutations ──────────────────────────────────────────────────────────

    /// Append a triple unconditionally. Use `assert` for deduplicated writes.
    pub fn add(&mut self, triple: Triple) {
        let idx = self.triples.len();
        self.index_insert(idx, &triple.subject, triple.predicate);
        self.triples.push(Some(triple));
    }

    /// Retain only triples for which `f` returns true. Matching triples are
    /// tombstoned in place — indexes stay consistent without a full rebuild.
    /// Returns the number of triples that were forgotten.
    pub fn retain<F>(&mut self, mut f: F) -> usize
    where
        F: FnMut(&Triple) -> bool,
    {
        let mut removed = 0;
        // Index loop — avoids borrowing `self.triples` while we call `self.tombstone`.
        for i in 0..self.triples.len() {
            let drop = match &self.triples[i] {
                Some(triple) => !f(triple),
                None => false,
            };
            if drop {
                self.tombstone(i);
                removed += 1;
            }
        }
        removed
    }

    pub fn remove(&mut self, subject: &Node, predicate: Predicate, object: &Value) {
        let Some(list) = self.by_subject_predicate.get(&(subject.clone(), predicate)) else {
            return;
        };
        let mut to_remove: SmallVec<[usize; 4]> = SmallVec::new();
        for &idx in list {
            if let Some(Some(triple)) = self.triples.get(idx)
                && triple.object == *object
            {
                to_remove.push(idx);
            }
        }
        for idx in to_remove {
            self.tombstone(idx);
        }
    }

    pub fn assert(&mut self, triple: Triple) {
        let key = (triple.subject.clone(), triple.predicate);

        // Contains + Item replaces by concept, not exact value — different
        // quantities of the same concept share one slot. If the quantity
        // didn't change, update metadata in place to avoid a tombstone churn
        // (hot path: every perception re-asserts the same inventory).
        if triple.predicate == Predicate::Contains
            && let Value::Item(concept, _) = &triple.object
        {
            let concept_copy = *concept;
            if let Some(idx) = self.find_existing_id(
                &key,
                |t| matches!(&t.object, Value::Item(c, _) if *c == concept_copy),
            ) {
                // Safe: find_existing_id only returns live ids.
                let existing = self.triples[idx].as_mut().expect("live slot");
                if existing.object == triple.object {
                    existing.meta.timestamp = triple.meta.timestamp;
                    existing.meta.confidence = triple.meta.confidence;
                    return;
                }
                self.tombstone(idx);
            }
            self.add(triple);
            return;
        }

        // Functional predicates have at most one object per subject. If the
        // value hasn't changed, just refresh metadata — tombstoning an
        // unchanged Hunger/Thirst fact every tick would dominate decay cost.
        if triple.predicate.is_functional() {
            if let Some(idx) = self.find_existing_id(&key, |_| true) {
                let existing = self.triples[idx].as_mut().expect("live slot");
                if existing.object == triple.object {
                    existing.meta.timestamp = triple.meta.timestamp;
                    existing.meta.confidence = triple.meta.confidence;
                    return;
                }
                self.tombstone(idx);
            }
            self.add(triple);
            return;
        }

        // Non-functional: exact-match dedupe against the (subject, predicate)
        // bucket; update metadata in place when we find a hit.
        if let Some(idx) = self.find_existing_id(&key, |t| t.object == triple.object) {
            let existing = self.triples[idx].as_mut().expect("live slot");
            existing.meta.timestamp = triple.meta.timestamp;
            existing.meta.confidence = triple.meta.confidence;
            return;
        }

        self.add(triple);
    }

    /// First id in the (subject, predicate) bucket whose live triple passes
    /// the predicate. Returns `None` if the bucket is missing or no entry
    /// matches. Kept private — only `assert` cares about "first match".
    fn find_existing_id<F>(&self, key: &(Node, Predicate), mut pred: F) -> Option<usize>
    where
        F: FnMut(&Triple) -> bool,
    {
        self.by_subject_predicate
            .get(key)?
            .iter()
            .copied()
            .find(|&idx| matches!(self.triples.get(idx), Some(Some(t)) if pred(t)))
    }

    // ─── Reads ──────────────────────────────────────────────────────────────

    pub fn get(&self, subject: &Node, predicate: Predicate) -> Option<&Value> {
        // 1. Check local knowledge (fastest)
        if let Some(list) = self.by_subject_predicate.get(&(subject.clone(), predicate)) {
            for &idx in list {
                if let Some(Some(triple)) = self.triples.get(idx) {
                    return Some(&triple.object);
                }
            }
        }

        // 2. Check shared knowledge
        for shared in &self.shared_knowledge {
            if let Some(triple) = shared
                .iter()
                .find(|t| t.subject == *subject && t.predicate == predicate)
            {
                return Some(&triple.object);
            }
        }

        // 3. Fallback to ontology
        self.ontology
            .triples
            .iter()
            .find(|t| t.subject == *subject && t.predicate == predicate)
            .map(|t| &t.object)
    }

    pub fn query(
        &self,
        subject: Option<&Node>,
        predicate: Option<Predicate>,
        object: Option<&Value>,
    ) -> Vec<&Triple> {
        let matcher = |t: &Triple| {
            subject.is_none_or(|s| t.subject == *s)
                && predicate.is_none_or(|p| t.predicate == p)
                && object.is_none_or(|o| t.object == *o)
        };

        // Pick the tightest index for LOCAL triples.
        // Pick the tightest index that fits the query pattern. None means
        // "no useful index" — fall back to a live-triple scan.
        let ids: Option<&[usize]> = match (subject, predicate) {
            (Some(sub), Some(pred)) => self
                .by_subject_predicate
                .get(&(sub.clone(), pred))
                .map(|v| v.as_slice()),
            (Some(sub), None) => self.by_subject.get(sub).map(|v| v.as_slice()),
            (None, Some(pred)) => self.by_predicate.get(&pred).map(|v| v.as_slice()),
            (None, None) => None,
        };
        let local_iter: Box<dyn Iterator<Item = &Triple>> = match (ids, subject, predicate) {
            (Some(ids), _, _) => Box::new(self.live_at(ids).filter(|t| matcher(t))),
            // (None, None) — no index usable, walk everything live.
            (None, None, None) => Box::new(self.iter().filter(move |t| matcher(t))),
            // Subject or predicate specified but bucket missing → empty.
            _ => Box::new(std::iter::empty()),
        };

        // Combine sources: Ontology -> Shared -> Local
        self.ontology
            .triples
            .iter()
            .filter(|t| matcher(t))
            .chain(
                self.shared_knowledge
                    .iter()
                    .flat_map(|vec| vec.iter().filter(|t| matcher(t))),
            )
            .chain(local_iter)
            .collect()
    }

    // ─── Diagnostics / inspection ──────────────────────────────────────────

    pub fn by_subject_len(&self) -> usize {
        self.by_subject.len()
    }

    pub fn by_predicate_len(&self) -> usize {
        self.by_predicate.len()
    }

    pub fn by_subject_predicate_len(&self) -> usize {
        self.by_subject_predicate.len()
    }

    // ─── Inheritance queries ───

    pub fn is_a(&self, subject: &Node, target: Concept) -> bool {
        if let Node::Concept(c) = subject {
            // Check ontology cache first (O(1))
            if self.ontology.is_a(*c, target) {
                return true;
            }
        }

        // Check local/shared (rare for IsA to be dynamic, but possible)
        if self.has(subject, Predicate::IsA, &Value::Concept(target)) {
            return true;
        }

        // Recursive check
        let parents = self.query(Some(subject), Some(Predicate::IsA), None);
        for triple in parents {
            if let Value::Concept(parent) = &triple.object
                && self.is_a(&Node::Concept(*parent), target)
            {
                return true;
            }
        }

        false
    }

    pub fn has_trait(&self, subject: &Node, trait_: Concept) -> bool {
        if let Node::Concept(c) = subject {
            // Check ontology cache first (O(1))
            if self.ontology.has_trait(*c, trait_) {
                return true;
            }
        }

        // Check local override/additions
        if self.has(subject, Predicate::HasTrait, &Value::Concept(trait_)) {
            return true;
        }

        // Inherited from parents
        let parents = self.query(Some(subject), Some(Predicate::IsA), None);
        for triple in parents {
            if let Value::Concept(parent) = &triple.object
                && self.has_trait(&Node::Concept(*parent), trait_)
            {
                return true;
            }
        }
        false
    }

    pub fn all_types(&self, subject: &Node) -> Vec<Concept> {
        let mut result = vec![];
        let mut queue = vec![subject.clone()];
        let mut visited = Vec::new();

        if let Node::Concept(c) = subject {
            result.extend(self.ontology.get_parents(*c));
        }

        while let Some(current) = queue.pop() {
            if visited.contains(&current) {
                continue;
            }
            visited.push(current.clone());

            for triple in self.query(Some(&current), Some(Predicate::IsA), None) {
                if let Value::Concept(parent) = &triple.object
                    && !result.contains(parent)
                {
                    result.push(*parent);
                    queue.push(Node::Concept(*parent));
                }
            }
        }
        result
    }

    pub fn has(&self, subject: &Node, predicate: Predicate, object: &Value) -> bool {
        !self
            .query(Some(subject), Some(predicate), Some(object))
            .is_empty()
    }

    // ─── Quantity-aware helpers ───

    /// Check if subject contains any amount of the given concept.
    /// Handles Item(concept, N) where N > 0.
    pub fn has_any(&self, subject: &Node, concept: Concept) -> bool {
        self.count_of(subject, concept) > 0
    }

    /// Get the count of a concept that subject contains.
    /// Returns 0 if not found or if Value is not an Item.
    pub fn count_of(&self, subject: &Node, concept: Concept) -> u32 {
        for triple in self.query(Some(subject), Some(Predicate::Contains), None) {
            if let Value::Item(c, count) = &triple.object
                && *c == concept
            {
                return *count;
            }
        }
        0
    }

    /// Check if subject contains any items at all (any concept, any count > 0).
    pub fn has_any_items(&self, subject: &Node) -> bool {
        for triple in self.query(Some(subject), Some(Predicate::Contains), None) {
            if let Value::Item(_, count) = &triple.object
                && *count > 0
            {
                return true;
            }
        }
        false
    }

    /// Get the confidence that subject contains the given concept.
    /// Returns 0.0 if not found, or the triple's confidence if found with count > 0.
    pub fn confidence_of(&self, subject: &Node, concept: Concept) -> f32 {
        for triple in self.query(Some(subject), Some(Predicate::Contains), None) {
            if let Value::Item(c, count) = &triple.object
                && *c == concept
                && *count > 0
            {
                return triple.meta.confidence;
            }
        }
        0.0
    }

    pub fn perceive_self(&mut self, predicate: Predicate, object: Value, timestamp: u64) {
        self.assert(Triple::with_meta(
            Node::Self_,
            predicate,
            object,
            Metadata::perception(timestamp),
        ));
    }

    pub fn perceive_entity(
        &mut self,
        entity: Entity,
        predicate: Predicate,
        object: Value,
        timestamp: u64,
        confidence: f32,
    ) {
        self.assert(Triple::with_meta(
            Node::Entity(entity),
            predicate,
            object,
            Metadata::perception_with_conf(timestamp, confidence),
        ));
    }

    /// Like `perceive_entity` but tags the triple with the originating sense.
    pub fn perceive_via_sense(
        &mut self,
        subject: Node,
        predicate: Predicate,
        object: Value,
        timestamp: u64,
        confidence: f32,
        sense: Sense,
    ) {
        self.assert(Triple::with_meta(
            subject,
            predicate,
            object,
            Metadata::perception_sense_conf(timestamp, confidence, sense),
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ONTOLOGY — Shared universal truths with precomputed caches
// ═══════════════════════════════════════════════════════════════════════════

use std::collections::HashSet;

#[derive(Resource, Clone)]
pub struct Ontology {
    pub triples: Arc<Vec<Triple>>,
    /// Cache: Concept -> all traits it has (inherited)
    pub trait_cache: Arc<HashMap<Concept, HashSet<Concept>>>,
    /// Cache: Concept -> direct parents (IsA)
    pub parent_cache: Arc<HashMap<Concept, Vec<Concept>>>,
}

impl Default for Ontology {
    fn default() -> Self {
        Self {
            triples: Arc::new(Vec::new()),
            trait_cache: Arc::new(HashMap::new()),
            parent_cache: Arc::new(HashMap::new()),
        }
    }
}

impl Ontology {
    /// Check if concept has trait (O(1) via cache)
    pub fn has_trait(&self, concept: Concept, trait_: Concept) -> bool {
        self.trait_cache
            .get(&concept)
            .is_some_and(|traits| traits.contains(&trait_))
    }

    /// Check if concept is-a parent (O(1) via cache)
    pub fn is_a(&self, concept: Concept, parent: Concept) -> bool {
        if concept == parent {
            return true;
        }
        self.parent_cache
            .get(&concept)
            .is_some_and(|parents| parents.contains(&parent))
    }

    /// Get direct parents of concept
    pub fn get_parents(&self, concept: Concept) -> Vec<Concept> {
        self.parent_cache.get(&concept).cloned().unwrap_or_default()
    }

    /// Assert that a concept has a trait, rebuilding caches if the triple is new.
    /// Idempotent — calling twice with the same arguments is a no-op.
    pub fn ensure_trait(&mut self, concept: Concept, trait_: Concept) {
        if self.has_trait(concept, trait_) {
            return;
        }
        let triple = Triple::new(
            Node::Concept(concept),
            Predicate::HasTrait,
            Value::Concept(trait_),
        );
        let mut triples = (*self.triples).clone();
        triples.push(triple);
        self.triples = Arc::new(triples);
        self.build_caches();
    }

    /// Assert that a concept produces another concept (e.g. BerryBush → Berry),
    /// rebuilding caches if the triple is new.
    /// Idempotent — calling twice with the same arguments is a no-op.
    pub fn ensure_production(&mut self, producer: Concept, product: Concept) {
        let already_exists = self.triples.iter().any(|t| {
            t.subject == Node::Concept(producer)
                && t.predicate == Predicate::Produces
                && t.object == Value::Concept(product)
        });
        if already_exists {
            return;
        }
        let triple = Triple::new(
            Node::Concept(producer),
            Predicate::Produces,
            Value::Concept(product),
        );
        let mut triples = (*self.triples).clone();
        triples.push(triple);
        self.triples = Arc::new(triples);
        // No cache rebuild needed — production triples don't affect trait/parent caches
    }

    /// Build caches from triples
    fn build_caches(&mut self) {
        let mut parent_map: HashMap<Concept, Vec<Concept>> = HashMap::new();
        let mut trait_map: HashMap<Concept, HashSet<Concept>> = HashMap::new();

        // Build parent_cache
        for triple in self.triples.iter() {
            if triple.predicate == Predicate::IsA
                && let (Node::Concept(child), Value::Concept(parent)) =
                    (&triple.subject, &triple.object)
            {
                parent_map.entry(*child).or_default().push(*parent);
            }
        }

        // Build trait_cache (direct)
        for triple in self.triples.iter() {
            if triple.predicate == Predicate::HasTrait
                && let (Node::Concept(concept), Value::Concept(trait_)) =
                    (&triple.subject, &triple.object)
            {
                trait_map.entry(*concept).or_default().insert(*trait_);
            }
        }

        // Helper for inheritance
        fn collect_traits(
            concept: Concept,
            parent_map: &HashMap<Concept, Vec<Concept>>,
            trait_map: &HashMap<Concept, HashSet<Concept>>,
        ) -> HashSet<Concept> {
            let mut result = HashSet::new();
            if let Some(traits) = trait_map.get(&concept) {
                result.extend(traits.iter().cloned());
            }
            if let Some(parents) = parent_map.get(&concept) {
                for parent in parents {
                    result.extend(collect_traits(*parent, parent_map, trait_map));
                }
            }
            result
        }

        // Build full trait cache
        let all_concepts: HashSet<Concept> =
            parent_map.keys().chain(trait_map.keys()).cloned().collect();
        let mut full_trait_map = HashMap::new();

        for concept in all_concepts {
            let traits = collect_traits(concept, &parent_map, &trait_map);
            if !traits.is_empty() {
                full_trait_map.insert(concept, traits);
            }
        }

        self.parent_cache = Arc::new(parent_map);
        self.trait_cache = Arc::new(full_trait_map);
    }
}

pub fn setup_ontology() -> Ontology {
    // println!("Running setup_ontology...");
    let mut triples = Vec::new();
    let mut add = |s: Node, p: Predicate, o: Value| {
        triples.push(Triple::new(s, p, o));
    };

    // Helper closures
    use Concept::*;
    use Predicate::*;
    let c = |con: Concept| Node::Concept(con);
    let v = |con: Concept| Value::Concept(con);

    // ─── Category hierarchy (IsA) ───
    add(c(Person), IsA, v(Physical));
    add(c(Animal), IsA, v(Physical));
    add(c(Plant), IsA, v(Physical));
    add(c(Object), IsA, v(Physical));
    add(c(Food), IsA, v(Physical));
    add(c(Resource), IsA, v(Physical));

    add(c(Apple), IsA, v(Food));
    add(c(Apple), IsA, v(Resource));
    add(c(Apple), IsA, v(Plant));

    add(c(Berry), IsA, v(Food));
    add(c(Berry), IsA, v(Resource));
    add(c(Berry), IsA, v(Plant));

    add(c(AppleTree), IsA, v(Plant));
    add(c(BerryBush), IsA, v(Plant));

    add(c(Deer), IsA, v(Animal));

    add(c(Wolf), IsA, v(Animal));
    add(c(Wolf), HasTrait, v(Dangerous)); // All agents with ontology know wolves are dangerous

    add(c(Wood), IsA, v(Resource));
    add(c(Water), IsA, v(Resource));
    add(c(Stone), IsA, v(Resource));

    add(c(WoodLog), IsA, v(Object));
    add(c(WoodLog), IsA, v(Resource));

    add(c(StoneNode), IsA, v(Object));
    add(c(StoneNode), IsA, v(Resource));

    // ─── Properties (HasTrait) ───
    add(c(Food), HasTrait, v(Edible));
    add(c(Water), HasTrait, v(Drinkable));
    add(c(Person), HasTrait, v(Sentient));
    add(c(Animal), HasTrait, v(Sentient));
    // AppleTree and BerryBush inherit Harvestable from Plant via IsA.
    // WoodLog and StoneNode receive it at spawn time via
    // derive_ontology_harvestable_component (they are not Plants).
    add(c(Plant), HasTrait, v(Harvestable));

    // ─── Universal production facts (all agents know these) ───
    add(c(WoodLog), Produces, Value::Item(Wood, 1));
    add(c(StoneNode), Produces, Value::Item(Stone, 1));

    // ─── Action Categorization ───
    use crate::agent::actions::ActionType;
    let act = |a: ActionType| Node::Action(a);
    let val_act = |c: Concept| Value::Concept(c);

    add(act(ActionType::Wave), IsA, val_act(SocialAction));
    add(act(ActionType::Converse), IsA, val_act(SocialAction));
    add(act(ActionType::Attack), IsA, val_act(ViolentAction));
    add(act(ActionType::Bite), IsA, val_act(ViolentAction));
    add(act(ActionType::Flee), IsA, val_act(ViolentAction));
    add(act(ActionType::Eat), IsA, val_act(SurvivalAction));
    add(act(ActionType::Sleep), IsA, val_act(SurvivalAction));
    add(act(ActionType::Walk), IsA, val_act(MovementAction));
    add(act(ActionType::Wander), IsA, val_act(MovementAction));
    add(act(ActionType::Harvest), IsA, val_act(SurvivalAction));
    add(act(ActionType::Drink), IsA, val_act(SurvivalAction));
    add(act(ActionType::Graze), IsA, val_act(SurvivalAction));

    let mut ontology = Ontology {
        triples: Arc::new(triples),
        trait_cache: Arc::new(HashMap::new()),
        parent_cache: Arc::new(HashMap::new()),
    };
    ontology.build_caches();
    ontology
}

// NOTE: Perception systems are now consolidated in cognition/perception.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contains_item_replacement() {
        // Test that asserting (Self, Contains, Apple(0)) replaces (Self, Contains, Apple(5))
        let mut mind = MindGraph::default();

        // Start with 5 apples
        mind.assert(Triple::new(
            Node::Self_,
            Predicate::Contains,
            Value::Item(Concept::Apple, 5),
        ));

        // Verify we have 5 apples
        let results = mind.query(Some(&Node::Self_), Some(Predicate::Contains), None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].object, Value::Item(Concept::Apple, 5));

        // Now eat some - update to 2 apples
        mind.assert(Triple::new(
            Node::Self_,
            Predicate::Contains,
            Value::Item(Concept::Apple, 2),
        ));

        // Should replace, not add - still only 1 triple
        let results = mind.query(Some(&Node::Self_), Some(Predicate::Contains), None);
        assert_eq!(results.len(), 1, "Should replace, not add duplicate");
        assert_eq!(results[0].object, Value::Item(Concept::Apple, 2));

        // Eat the rest - update to 0 apples
        mind.assert(Triple::new(
            Node::Self_,
            Predicate::Contains,
            Value::Item(Concept::Apple, 0),
        ));

        // Should still be 1 triple, now with 0
        let results = mind.query(Some(&Node::Self_), Some(Predicate::Contains), None);
        assert_eq!(results.len(), 1, "Should replace, not add");
        assert_eq!(results[0].object, Value::Item(Concept::Apple, 0));
    }

    #[test]
    fn test_contains_different_items_separate() {
        // Test that different item types don't interfere with each other
        let mut mind = MindGraph::default();

        // Add apples and sticks
        mind.assert(Triple::new(
            Node::Self_,
            Predicate::Contains,
            Value::Item(Concept::Apple, 3),
        ));
        mind.assert(Triple::new(
            Node::Self_,
            Predicate::Contains,
            Value::Item(Concept::Stick, 2),
        ));

        // Should have 2 separate triples
        let results = mind.query(Some(&Node::Self_), Some(Predicate::Contains), None);
        assert_eq!(results.len(), 2, "Different items should be separate");

        // Update apples
        mind.assert(Triple::new(
            Node::Self_,
            Predicate::Contains,
            Value::Item(Concept::Apple, 1),
        ));

        // Still 2 triples, apples updated, sticks unchanged
        let results = mind.query(Some(&Node::Self_), Some(Predicate::Contains), None);
        assert_eq!(results.len(), 2);

        let apple_count = results
            .iter()
            .find_map(|t| match &t.object {
                Value::Item(Concept::Apple, qty) => Some(*qty),
                _ => None,
            })
            .unwrap();
        assert_eq!(apple_count, 1);

        let stick_count = results
            .iter()
            .find_map(|t| match &t.object {
                Value::Item(Concept::Stick, qty) => Some(*qty),
                _ => None,
            })
            .unwrap();
        assert_eq!(stick_count, 2);
    }

    // ─── query() — pattern matching with wildcards (#20) ──────────────────────

    fn three_entity_world() -> (MindGraph, Entity, Entity, Entity) {
        let mut mind = MindGraph::default();
        let agent = Entity::from_bits(1);
        let tree = Entity::from_bits(2);
        let bush = Entity::from_bits(3);

        mind.add(Triple::new(
            Node::Entity(agent),
            Predicate::Contains,
            Value::Item(Concept::Apple, 1),
        ));
        mind.add(Triple::new(
            Node::Entity(tree),
            Predicate::Contains,
            Value::Item(Concept::Apple, 5),
        ));
        mind.add(Triple::new(
            Node::Entity(bush),
            Predicate::Contains,
            Value::Item(Concept::Berry, 3),
        ));
        mind.add(Triple::new(
            Node::Entity(tree),
            Predicate::LocatedAt,
            Value::Tile((4, 4)),
        ));

        (mind, agent, tree, bush)
    }

    #[test]
    fn query_fully_specified_matches_only_the_target_triple() {
        let (mind, _agent, tree, _bush) = three_entity_world();

        let results = mind.query(
            Some(&Node::Entity(tree)),
            Some(Predicate::Contains),
            Some(&Value::Item(Concept::Apple, 5)),
        );

        assert_eq!(
            results.len(),
            1,
            "fully-specified query must match exactly one triple"
        );
        assert_eq!(results[0].subject, Node::Entity(tree));
    }

    #[test]
    fn query_fully_specified_does_not_match_unintended_triples() {
        // Regression for #20: a specific query must not match other entities.
        let (mind, _agent, _tree, bush) = three_entity_world();

        let results = mind.query(
            Some(&Node::Entity(bush)),
            Some(Predicate::Contains),
            Some(&Value::Item(Concept::Apple, 5)),
        );

        assert!(results.is_empty());
    }

    #[test]
    fn query_wildcard_subject_matches_any_subject() {
        let (mind, _agent, _tree, _bush) = three_entity_world();

        let results = mind.query(
            None,
            Some(Predicate::Contains),
            Some(&Value::Item(Concept::Apple, 5)),
        );

        assert_eq!(results.len(), 1, "only the tree holds Apple(5)");
        assert!(matches!(results[0].subject, Node::Entity(_)));
    }

    #[test]
    fn query_wildcard_predicate_matches_any_predicate() {
        let (mind, _agent, tree, _bush) = three_entity_world();

        let results = mind.query(Some(&Node::Entity(tree)), None, None);

        assert_eq!(results.len(), 2);
        let predicates: Vec<_> = results.iter().map(|t| t.predicate).collect();
        assert!(predicates.contains(&Predicate::Contains));
        assert!(predicates.contains(&Predicate::LocatedAt));
    }

    #[test]
    fn query_wildcard_object_matches_any_object() {
        let (mind, _agent, tree, _bush) = three_entity_world();

        let results = mind.query(Some(&Node::Entity(tree)), Some(Predicate::Contains), None);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].object, Value::Item(Concept::Apple, 5));
    }

    #[test]
    fn query_wildcard_object_does_not_leak_across_subjects() {
        // Even with object wildcard, the subject filter must hold strictly.
        let (mind, _agent, tree, bush) = three_entity_world();

        let tree_results = mind.query(Some(&Node::Entity(tree)), Some(Predicate::Contains), None);
        let bush_results = mind.query(Some(&Node::Entity(bush)), Some(Predicate::Contains), None);

        assert_eq!(tree_results.len(), 1);
        assert_eq!(bush_results.len(), 1);
        assert_ne!(tree_results[0].object, bush_results[0].object);
    }

    // ─── #197 indexing regression tests ───────────────────────────────────

    /// Build a graph with N random-ish triples so index and linear-scan paths
    /// both have to handle a non-trivial workload.
    fn populated_graph(n: usize) -> MindGraph {
        let mut mind = MindGraph::default();
        for i in 0..n {
            let entity = Entity::from_bits(1000 + i as u64);
            // Three facts per entity — Contains, LocatedAt, IsA
            mind.add(Triple::new(
                Node::Entity(entity),
                Predicate::Contains,
                Value::Item(Concept::Apple, (i % 10) as u32),
            ));
            mind.add(Triple::new(
                Node::Entity(entity),
                Predicate::LocatedAt,
                Value::Tile((i as i32, (i * 3) as i32)),
            ));
            mind.add(Triple::new(
                Node::Entity(entity),
                Predicate::IsA,
                Value::Concept(Concept::AppleTree),
            ));
        }
        mind
    }

    /// Reference linear scan over local triples only — used to prove the
    /// indexed `query()` returns the same results as an O(n) walk.
    fn linear_scan<'a>(
        mind: &'a MindGraph,
        subject: Option<&Node>,
        predicate: Option<Predicate>,
        object: Option<&Value>,
    ) -> Vec<&'a Triple> {
        mind.iter()
            .filter(|t| subject.is_none_or(|s| t.subject == *s))
            .filter(|t| predicate.is_none_or(|p| t.predicate == p))
            .filter(|t| object.is_none_or(|o| t.object == *o))
            .collect()
    }

    fn sort_by_ptr(mut v: Vec<&Triple>) -> Vec<*const Triple> {
        v.sort_by_key(|t| *t as *const _ as usize);
        v.into_iter().map(|t| t as *const _).collect()
    }

    #[test]
    fn query_by_subject_matches_linear_scan() {
        let mind = populated_graph(200);
        let target = Node::Entity(Entity::from_bits(1042));

        let indexed = mind.query(Some(&target), None, None);
        let reference = linear_scan(&mind, Some(&target), None, None);

        assert_eq!(indexed.len(), reference.len());
        assert_eq!(sort_by_ptr(indexed), sort_by_ptr(reference));
    }

    #[test]
    fn query_by_subject_predicate_matches_linear_scan() {
        let mind = populated_graph(200);
        let target = Node::Entity(Entity::from_bits(1017));

        let indexed = mind.query(Some(&target), Some(Predicate::Contains), None);
        let reference = linear_scan(&mind, Some(&target), Some(Predicate::Contains), None);

        assert_eq!(indexed.len(), reference.len());
        assert_eq!(sort_by_ptr(indexed), sort_by_ptr(reference));
    }

    #[test]
    fn query_by_predicate_matches_linear_scan() {
        let mind = populated_graph(50);

        let indexed = mind.query(None, Some(Predicate::LocatedAt), None);
        let reference = linear_scan(&mind, None, Some(Predicate::LocatedAt), None);

        assert_eq!(indexed.len(), 50);
        assert_eq!(indexed.len(), reference.len());
        assert_eq!(sort_by_ptr(indexed), sort_by_ptr(reference));
    }

    #[test]
    fn query_with_all_none_returns_all_live_triples() {
        let mind = populated_graph(10);
        let all = mind.query(None, None, None);
        // 10 entities × 3 triples each = 30 (ontology is empty by default)
        assert_eq!(all.len(), 30);
    }

    #[test]
    fn assert_updates_all_indexes() {
        let mut mind = MindGraph::default();
        let e = Entity::from_bits(1);
        mind.assert(Triple::new(
            Node::Entity(e),
            Predicate::IsA,
            Value::Concept(Concept::Food),
        ));

        assert_eq!(mind.by_subject_len(), 1);
        assert_eq!(mind.by_predicate_len(), 1);
        assert_eq!(mind.by_subject_predicate_len(), 1);

        // All three indexes resolve the same triple.
        assert_eq!(mind.query(Some(&Node::Entity(e)), None, None).len(), 1);
        assert_eq!(mind.query(None, Some(Predicate::IsA), None).len(), 1);
        assert_eq!(
            mind.query(Some(&Node::Entity(e)), Some(Predicate::IsA), None)
                .len(),
            1
        );
    }

    #[test]
    fn remove_tombstones_rather_than_compacting() {
        let mut mind = MindGraph::default();
        let e = Entity::from_bits(1);
        mind.add(Triple::new(
            Node::Entity(e),
            Predicate::Contains,
            Value::Item(Concept::Apple, 3),
        ));
        mind.add(Triple::new(
            Node::Entity(e),
            Predicate::Contains,
            Value::Item(Concept::Berry, 1),
        ));
        assert_eq!(mind.len(), 2);
        assert_eq!(mind.total_slots(), 2);

        mind.remove(
            &Node::Entity(e),
            Predicate::Contains,
            &Value::Item(Concept::Apple, 3),
        );

        // Live count drops; slot count does not.
        assert_eq!(mind.len(), 1);
        assert_eq!(mind.total_slots(), 2);
        assert_eq!(mind.tombstone_count(), 1);

        // Tombstoned triple is invisible to every query path.
        let by_both = mind.query(Some(&Node::Entity(e)), Some(Predicate::Contains), None);
        assert_eq!(by_both.len(), 1);
        assert_eq!(by_both[0].object, Value::Item(Concept::Berry, 1));

        let by_pred = mind.query(None, Some(Predicate::Contains), None);
        assert_eq!(by_pred.len(), 1);

        let by_all = mind.query(None, None, None);
        assert_eq!(by_all.len(), 1);
    }

    #[test]
    fn remove_updates_indexes() {
        let mut mind = MindGraph::default();
        let e = Entity::from_bits(1);
        mind.add(Triple::new(
            Node::Entity(e),
            Predicate::Contains,
            Value::Item(Concept::Apple, 3),
        ));
        mind.remove(
            &Node::Entity(e),
            Predicate::Contains,
            &Value::Item(Concept::Apple, 3),
        );

        // With no more (subject, predicate) entries left, those index buckets
        // should be empty so they don't leak memory.
        assert_eq!(mind.by_subject_len(), 0);
        assert_eq!(mind.by_predicate_len(), 0);
        assert_eq!(mind.by_subject_predicate_len(), 0);
    }

    #[test]
    fn compact_reclaims_tombstoned_slots() {
        let mut mind = MindGraph::default();
        for i in 1..=10 {
            mind.add(Triple::new(
                Node::Entity(Entity::from_bits(i)),
                Predicate::IsA,
                Value::Concept(Concept::Food),
            ));
        }
        for i in 1..=5 {
            mind.remove(
                &Node::Entity(Entity::from_bits(i)),
                Predicate::IsA,
                &Value::Concept(Concept::Food),
            );
        }
        assert_eq!(mind.len(), 5);
        assert_eq!(mind.total_slots(), 10);

        mind.compact();
        assert_eq!(mind.len(), 5);
        assert_eq!(mind.total_slots(), 5);
        assert_eq!(mind.tombstone_count(), 0);

        // Indexes survived the rebuild — queries still work.
        let results = mind.query(None, Some(Predicate::IsA), None);
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn retain_tombstones_filtered_triples() {
        let mut mind = MindGraph::default();
        for i in 1..=6 {
            mind.add(Triple::new(
                Node::Entity(Entity::from_bits(i)),
                Predicate::IsA,
                Value::Concept(if i % 2 == 0 {
                    Concept::Food
                } else {
                    Concept::Resource
                }),
            ));
        }

        // Forget everything that isn't Food.
        let removed = mind.retain(|t| matches!(t.object, Value::Concept(c) if c == Concept::Food));

        assert_eq!(removed, 3);
        assert_eq!(mind.len(), 3);
        assert_eq!(mind.tombstone_count(), 3);

        let survivors = mind.query(None, Some(Predicate::IsA), None);
        assert_eq!(survivors.len(), 3);
        assert!(
            survivors
                .iter()
                .all(|t| matches!(t.object, Value::Concept(Concept::Food)))
        );
    }

    #[test]
    fn query_skips_tombstoned_entries_in_all_index_paths() {
        let mut mind = populated_graph(20);
        // Tombstone all Contains triples via retain().
        let removed = mind.retain(|t| t.predicate != Predicate::Contains);
        assert_eq!(removed, 20);

        // Every access path must agree that Contains is gone.
        assert!(mind.query(None, Some(Predicate::Contains), None).is_empty());
        for i in 0..20 {
            let e = Entity::from_bits(1000 + i);
            assert!(
                mind.query(Some(&Node::Entity(e)), Some(Predicate::Contains), None)
                    .is_empty()
            );
            // by_subject path also returns only LocatedAt + IsA.
            assert_eq!(mind.query(Some(&Node::Entity(e)), None, None).len(), 2);
        }
    }

    #[test]
    fn functional_assert_replaces_existing_via_indexes() {
        let mut mind = MindGraph::default();
        mind.assert(Triple::new(Node::Self_, Predicate::Hunger, Value::Int(50)));
        mind.assert(Triple::new(Node::Self_, Predicate::Hunger, Value::Int(80)));

        // Only one live value; the old one is tombstoned.
        assert_eq!(mind.len(), 1);
        assert_eq!(
            mind.get(&Node::Self_, Predicate::Hunger),
            Some(&Value::Int(80))
        );
    }

    #[test]
    fn get_prefers_live_triple_after_tombstone() {
        let mut mind = MindGraph::default();
        mind.assert(Triple::new(Node::Self_, Predicate::Hunger, Value::Int(50)));
        mind.remove(&Node::Self_, Predicate::Hunger, &Value::Int(50));
        // Nothing live should survive.
        assert_eq!(mind.get(&Node::Self_, Predicate::Hunger), None);
    }

    #[test]
    fn indexed_query_handles_long_live_lists() {
        // Stress the (subject, predicate) index with many triples under the
        // same key — forces SmallVec to spill to heap and still work.
        let mut mind = MindGraph::default();
        let subject = Node::Entity(Entity::from_bits(1));
        for i in 0..100 {
            mind.add(Triple::new(
                subject.clone(),
                Predicate::HasTrait,
                Value::Concept(match i % 4 {
                    0 => Concept::Dangerous,
                    1 => Concept::Safe,
                    2 => Concept::Edible,
                    _ => Concept::Friendly,
                }),
            ));
        }

        let all = mind.query(Some(&subject), Some(Predicate::HasTrait), None);
        assert_eq!(all.len(), 100);

        let indexed = mind.query(
            Some(&subject),
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Dangerous)),
        );
        assert_eq!(indexed.len(), 25);
    }
}
