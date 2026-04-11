//! Shared logic-only spawn helper for human (Person) agents.
//!
//! Reads: Personality, Ontology, cultural knowledge triples
//! Writes: PersonCoreBundle, PersonPerceptionBundle, PersonBrainBundle
//! Upstream: world::human::spawn_person (real game), testing::spawn::spawn_test_person (TestWorld)
//! Downstream: brain pipeline (any system that queries Person logic components)
//!
//! Both spawn paths must produce identical brain-relevant components or
//! TestWorld humans drift from real-game humans (e.g. issue #306 where
//! TestWorld humans were decision-dead because the test path skipped
//! cultural knowledge and personality-derived drives).

use std::sync::Arc;

use bevy::prelude::*;

use crate::agent::actions::ActiveActions;
use crate::agent::affordance::Affordance;
use crate::agent::body::needs::{Consciousness, PhysicalNeeds, PsychologicalDrives};
use crate::agent::body::species::SpeciesProfile;
use crate::agent::brains::active_plan::ActivePlans;
use crate::agent::brains::history::BrainHistory;
use crate::agent::brains::proposal::BrainState;
use crate::agent::brains::rational::RationalBrain;
use crate::agent::inventory::EntityType;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::{Concept, MindGraph, Ontology, Triple};
use crate::agent::mind::memory::WorkingMemory;
use crate::agent::mind::perception::{VisibleObjects, Vision};
use crate::agent::mind::theory_of_mind::TheoryOfMind;
use crate::agent::movement::MovementState;
use crate::agent::nervous_system::cns::CentralNervousSystem;
use crate::agent::psyche::emotions::EmotionalState;
use crate::agent::psyche::personality::Personality;
use crate::agent::psyche::relationships::RelationshipHistory;
use crate::agent::skills::Skills;
use crate::agent::{Agent, Person, TargetPosition};
use crate::world::Physical;

/// Identity, body, and movement components. First of three bundles because
/// Bevy's Bundle tuple impl tops out at 12 elements.
#[derive(Bundle)]
pub struct PersonCoreBundle {
    pub name: Name,
    pub agent: Agent,
    pub person: Person,
    pub entity_type: EntityType,
    pub species: SpeciesProfile,
    pub physical: Physical,
    pub target_position: TargetPosition,
    pub movement_state: MovementState,
    pub inventory: ItemSlots,
    pub personality: Personality,
    pub transform: Transform,
    pub global_transform: GlobalTransform,
}

/// Affordance, mind graph, and perception state.
#[derive(Bundle)]
pub struct PersonPerceptionBundle {
    pub affordance: Affordance,
    pub mind: MindGraph,
    pub vision: Vision,
    pub visible: VisibleObjects,
}

/// Brains, drives, and the rest of the cognitive layer.
#[derive(Bundle)]
pub struct PersonBrainBundle {
    pub working_memory: WorkingMemory,
    pub rational_brain: RationalBrain,
    pub brain_state: BrainState,
    pub cns: CentralNervousSystem,
    pub physical_needs: PhysicalNeeds,
    pub consciousness: Consciousness,
    pub drives: PsychologicalDrives,
    pub commitments: crate::agent::commitment::Commitments,
    pub active_actions: ActiveActions,
    pub emotional: EmotionalState,
    pub brain_history: BrainHistory,
    pub active_plans: ActivePlans,
    pub relationships: RelationshipHistory,
    pub theory_of_mind: TheoryOfMind,
    pub skills: Skills,
}

/// Inputs that vary between spawn paths. Anything not in here is fixed
/// across all human agents (e.g. SpeciesProfile::human, Vision range).
pub struct PersonInit {
    /// Display name for the entity.
    pub name: String,
    /// World position to spawn at.
    pub position: Vec2,
    /// Personality traits. Drives are derived from these.
    pub personality: Personality,
    /// Initial physical needs (hunger, thirst, stamina, health).
    pub physical_needs: PhysicalNeeds,
    /// Override for the personality-derived social drive. `None` keeps the
    /// derived value, `Some(v)` clamps it to `v` (used by tests that want
    /// guaranteed-social agents without coupling to personality).
    pub social_drive_override: Option<f32>,
    /// Cultural knowledge triples shared across an agent's culture. The
    /// real spawner sources this from `create_cultural_knowledge(culture)`;
    /// the test spawner uses the default culture.
    pub cultural_knowledge: Arc<Vec<Triple>>,
    /// Per-agent knowledge triples to assert after cultural knowledge.
    pub extra_knowledge: Vec<Triple>,
}

/// Adds innate biological knowledge all humans have regardless of culture.
fn add_person_knowledge(mind: &mut MindGraph) {
    use crate::agent::mind::knowledge::{Metadata, Node, Predicate, Triple, Value};

    let meta = Metadata::default(); // Source::Intrinsic, confidence 1.0

    mind.assert(Triple::with_meta(
        Node::Action(crate::agent::actions::ActionType::Eat),
        Predicate::Satisfies,
        Value::Concept(Concept::Thing),
        meta.clone(),
    ));

    mind.assert(Triple::with_meta(
        Node::Concept(Concept::Wolf),
        Predicate::HasTrait,
        Value::Concept(Concept::Dangerous),
        meta,
    ));
}

/// Builds the three logic-only bundles for a Person agent. Both
/// `world::human::spawn_person` (real game) and
/// `testing::spawn::spawn_test_person` (TestWorld) call this — drift here
/// causes brain divergence between the two paths.
pub fn build_person_logic(
    init: PersonInit,
    ontology: Ontology,
) -> (PersonCoreBundle, PersonPerceptionBundle, PersonBrainBundle) {
    let mut mind = MindGraph::new(ontology);
    add_person_knowledge(&mut mind);
    mind.add_shared_knowledge(init.cultural_knowledge);
    for triple in init.extra_knowledge {
        mind.assert(triple);
    }

    let mut drives = PsychologicalDrives::from_personality(&init.personality.traits);
    if let Some(social) = init.social_drive_override {
        drives.social = social;
    }

    let core = PersonCoreBundle {
        name: Name::new(init.name),
        agent: Agent,
        person: Person,
        entity_type: EntityType(Concept::Person),
        species: SpeciesProfile::human(),
        physical: Physical,
        target_position: TargetPosition::default(),
        movement_state: MovementState::default(),
        inventory: ItemSlots::agent_carry(),
        personality: init.personality,
        transform: Transform::from_translation(init.position.extend(3.0)),
        global_transform: GlobalTransform::default(),
    };

    let perception = PersonPerceptionBundle {
        affordance: Affordance::default(),
        mind,
        vision: Vision { range: 100.0 },
        visible: VisibleObjects::default(),
    };

    let brain = PersonBrainBundle {
        working_memory: WorkingMemory::default(),
        rational_brain: RationalBrain::default(),
        brain_state: BrainState::default(),
        cns: CentralNervousSystem::default(),
        physical_needs: init.physical_needs,
        consciousness: Consciousness::default(),
        drives,
        commitments: crate::agent::commitment::Commitments::default(),
        active_actions: ActiveActions::default(),
        emotional: EmotionalState::default(),
        brain_history: BrainHistory::default(),
        active_plans: ActivePlans::default(),
        relationships: RelationshipHistory::default(),
        theory_of_mind: TheoryOfMind::default(),
        skills: Skills::default(),
    };

    (core, perception, brain)
}
