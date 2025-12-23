use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;

// ═══════════════════════════════════════════════════════════════════════════
// NODES — What can be subject or object in a triple
// ═══════════════════════════════════════════════════════════════════════════

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
    Area(String),
    /// A remembered event
    Event(u64),
    /// The agent who owns this MindGraph (self-reference)
    Self_,
    /// An action type (e.g. Wave, Eat)
    Action(crate::agent::actions::ActionType),
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
    Water,
    Stone,
    Stick,

    // ─── Animal types ───
    Deer,

    // ─── Traits/Properties (adjectives) ───
    Edible, // Items that can be eaten (Apple, Berry, Meat)
    Prey,   // Creatures that can be hunted (Deer, Rabbit) → yields Meat
    Dangerous,
    Safe,
    Friendly,
    Hostile,
    Neutral,
    Sentient,
    Harvestable,
    Awake,
    Asleep,

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

    // ─── Temporal ───
    RegenerationRate, // (AppleTree, RegenerationRate, 10.0)
    LastObserved,     // (Tree42, LastObserved, 50000)

    // ─── Agent state ───
    Hunger,      // (Self, Hunger, Int)
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
}

impl Predicate {
    pub fn is_functional(&self) -> bool {
        matches!(
            self,
            Predicate::LocatedAt
                | Predicate::Hunger
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
    Text(String),       // For names and other text
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
        }
    }

    pub fn experience(timestamp: u64) -> Self {
        Self {
            source: Source::Experienced,
            memory_type: MemoryType::Semantic, // Learned from direct interaction
            timestamp,
            confidence: 1.0,
            informant: None,
            evidence: Vec::new(),
            salience: 0.0,
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
// MINDGRAPH INDEX
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Default, Clone, Reflect)]
pub struct MindGraphIndex {
    // Indices for O(1) lookup
    // Using simple vectors of indices into the main triples vector
}

#[derive(Component, Clone, Reflect)]
#[reflect(Component)]
#[derive(Default)]
pub struct MindGraph {
    /// Shared universal truths (read-only, Arc for cheap clone)
    #[reflect(ignore)]
    pub ontology: Ontology,

    /// Shared cultural/social knowledge blocks
    #[reflect(ignore)]
    pub shared_knowledge: Vec<Arc<Vec<Triple>>>,

    /// All personal knowledge
    pub triples: Vec<Triple>,

    /// Indices for fast lookup of LOCAL triples
    #[reflect(ignore)]
    pub by_subject: HashMap<Node, Vec<usize>>,
    #[reflect(ignore)]
    pub by_subject_pred: HashMap<(Node, Predicate), usize>,
    #[reflect(ignore)]
    pub by_predicate: HashMap<Predicate, Vec<usize>>,
}

impl MindGraph {
    pub fn new(ontology: Ontology) -> Self {
        Self {
            ontology,
            shared_knowledge: Vec::new(),
            triples: Vec::new(),
            by_subject: HashMap::new(),
            by_subject_pred: HashMap::new(),
            by_predicate: HashMap::new(),
        }
    }

    pub fn add_shared_knowledge(&mut self, knowledge: Arc<Vec<Triple>>) {
        self.shared_knowledge.push(knowledge);
    }

    /// Helper to rebuild indices
    pub fn rebuild_indices(&mut self) {
        self.by_subject.clear();
        self.by_subject_pred.clear();
        self.by_predicate.clear();

        for (i, triple) in self.triples.iter().enumerate() {
            self.by_subject
                .entry(triple.subject.clone())
                .or_default()
                .push(i);

            self.by_predicate
                .entry(triple.predicate)
                .or_default()
                .push(i);

            if triple.predicate.is_functional() {
                self.by_subject_pred
                    .insert((triple.subject.clone(), triple.predicate), i);
            }
        }
    }

    pub fn add(&mut self, triple: Triple) {
        let idx = self.triples.len();

        // Update indices
        self.by_subject
            .entry(triple.subject.clone())
            .or_default()
            .push(idx);

        self.by_predicate
            .entry(triple.predicate)
            .or_default()
            .push(idx);

        if triple.predicate.is_functional() {
            self.by_subject_pred
                .insert((triple.subject.clone(), triple.predicate), idx);
        }

        self.triples.push(triple);
    }

    pub fn remove(&mut self, subject: &Node, predicate: Predicate, object: &Value) {
        let initial_len = self.triples.len();
        self.triples.retain(|t| {
            !(t.subject == *subject && t.predicate == predicate && t.object == *object)
        });

        if self.triples.len() != initial_len {
            self.rebuild_indices();
        }
    }

    pub fn assert(&mut self, triple: Triple) {
        // Special case: Contains + Item should replace by concept, not exact value
        if triple.predicate == Predicate::Contains {
            if let Value::Item(concept, _) = &triple.object {
                let concept_copy = *concept;
                let initial_len = self.triples.len();
                self.triples.retain(|t| {
                    !(t.subject == triple.subject
                        && t.predicate == Predicate::Contains
                        && matches!(&t.object, Value::Item(c, _) if *c == concept_copy))
                });
                if self.triples.len() != initial_len {
                    self.rebuild_indices();
                }
            }
        }
        // If functional, remove old variant
        else if triple.predicate.is_functional() {
            if let Some(&_old_idx) = self
                .by_subject_pred
                .get(&(triple.subject.clone(), triple.predicate))
            {
                // Remove old one.
                self.triples
                    .retain(|t| !(t.subject == triple.subject && t.predicate == triple.predicate));
                self.rebuild_indices();
            }
        } else {
            // Non-functional: Check for exact duplicate
            if let Some(existing) = self.triples.iter_mut().find(|t| {
                t.subject == triple.subject
                    && t.predicate == triple.predicate
                    && t.object == triple.object
            }) {
                existing.meta.timestamp = triple.meta.timestamp;
                existing.meta.confidence = triple.meta.confidence;
                return;
            }
        }

        self.add(triple);
    }

    pub fn get(&self, subject: &Node, predicate: Predicate) -> Option<&Value> {
        // 1. Check local knowledge (fastest)
        if let Some(&idx) = self.by_subject_pred.get(&(subject.clone(), predicate))
            && let Some(triple) = self.triples.get(idx)
        {
            return Some(&triple.object);
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
        // Ontology doesn't have a value index for triples yet, but simple find is okay for now
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

        // If we have indices and specific queries, use them for LOCAL triples
        let local_iter: Box<dyn Iterator<Item = &Triple>> =
            if let (Some(sub), Some(pred)) = (subject, predicate) {
                if let Some(&idx) = self.by_subject_pred.get(&(sub.clone(), pred)) {
                    if let Some(t) = self.triples.get(idx).filter(|t| matcher(t)) {
                        Box::new(std::iter::once(t))
                    } else {
                        Box::new(std::iter::empty())
                    }
                } else if let Some(indices) = self.by_subject.get(sub) {
                    Box::new(
                        indices
                            .iter()
                            .filter_map(|&i| self.triples.get(i))
                            .filter(|t| matcher(t)),
                    )
                } else {
                    Box::new(std::iter::empty())
                }
            } else if let Some(sub) = subject {
                if let Some(indices) = self.by_subject.get(sub) {
                    Box::new(
                        indices
                            .iter()
                            .filter_map(|&i| self.triples.get(i))
                            .filter(|t| matcher(t)),
                    )
                } else {
                    Box::new(std::iter::empty())
                }
            } else if let Some(pred) = predicate {
                if let Some(indices) = self.by_predicate.get(&pred) {
                    Box::new(
                        indices
                            .iter()
                            .filter_map(|&i| self.triples.get(i))
                            .filter(|t| matcher(t)),
                    )
                } else {
                    Box::new(std::iter::empty())
                }
            } else {
                // Scan all
                Box::new(self.triples.iter().filter(|t| matcher(t)))
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

    add(c(Wood), IsA, v(Resource));
    add(c(Water), IsA, v(Resource));

    // ─── Properties (HasTrait) ───
    add(c(Food), HasTrait, v(Edible));
    add(c(Person), HasTrait, v(Sentient));
    add(c(Animal), HasTrait, v(Sentient));
    add(c(Plant), HasTrait, v(Harvestable));
    add(c(AppleTree), HasTrait, v(Harvestable));
    add(c(BerryBush), HasTrait, v(Harvestable));

    // ─── Action Categorization ───
    use crate::agent::actions::ActionType;
    let act = |a: ActionType| Node::Action(a);
    let val_act = |c: Concept| Value::Concept(c);

    add(act(ActionType::Wave), IsA, val_act(SocialAction));
    add(act(ActionType::Talk), IsA, val_act(SocialAction));
    add(act(ActionType::Attack), IsA, val_act(ViolentAction));
    add(act(ActionType::Flee), IsA, val_act(ViolentAction));
    add(act(ActionType::Eat), IsA, val_act(SurvivalAction));
    add(act(ActionType::Sleep), IsA, val_act(SurvivalAction));
    add(act(ActionType::Walk), IsA, val_act(MovementAction));
    add(act(ActionType::Wander), IsA, val_act(MovementAction));
    add(act(ActionType::Harvest), IsA, val_act(SurvivalAction));

    // ─── Action Emotional Triggers ───
    // Base emotions triggered by actions
    add(
        act(ActionType::Attack),
        TriggersEmotion,
        Value::Emotion(crate::agent::psyche::emotions::EmotionType::Fear, 0.8),
    );
    add(
        act(ActionType::Wave),
        TriggersEmotion,
        Value::Emotion(crate::agent::psyche::emotions::EmotionType::Joy, 0.5),
    );
    add(
        act(ActionType::Eat),
        TriggersEmotion,
        Value::Emotion(crate::agent::psyche::emotions::EmotionType::Joy, 0.3),
    );
    // Harvesting is satisfying
    add(
        act(ActionType::Harvest),
        TriggersEmotion,
        Value::Emotion(crate::agent::psyche::emotions::EmotionType::Joy, 0.2),
    );

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
}
