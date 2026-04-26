//! Emotional brain: association-driven behavior based on feelings.
//!
//! Reads: EmotionalState, MindGraph, VisibleObjects, PsychologicalDrives, InConversation
//! Writes: BrainProposal
//! Upstream: perception (VisibleObjects), psyche (EmotionalState)
//! Downstream: brains::proposal (winner selection)
//!
//! Conversation continuation and turn-taking are handled by the
//! [`CommunicationPlugin`](crate::agent::communication::CommunicationPlugin).
//! The emotional brain only proposes the *initiation* of conversations
//! (`ActionType::InitiateConversation`); once registered, the plugin owns the
//! lifecycle.

use super::drift::{BEHAVIORS, DriftContext, propose_drift};
use super::proposal::{BrainProposal, BrainType, Intent};
use crate::agent::actions::ActionType;
use crate::agent::body::needs::{PhysicalNeeds, PsychologicalDrives};
use crate::agent::mind::conversation::InConversation;
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use crate::agent::mind::perception::VisibleObjects;
use crate::agent::psyche::emotions::{EmotionType, EmotionalState};
use crate::constants::brains::emotional::{
    ANGER_ENTITY_THRESHOLD, ANGER_ENTITY_URGENCY_MULTIPLIER, FEAR_ENTITY_THRESHOLD,
    FEAR_ENTITY_URGENCY_MULTIPLIER, FEAR_GENERAL_THRESHOLD, FEAR_GENERAL_URGENCY_MULTIPLIER,
    FIGHT_RESPONSE_BASE_URGENCY, FIGHT_RESPONSE_COMMITMENT_MULTIPLIER,
    FLEE_RESPONSE_URGENCY_MULTIPLIER, JOY_ENTITY_THRESHOLD, JOY_ENTITY_URGENCY_MULTIPLIER,
    SOCIAL_SEEK_THRESHOLD, SOCIAL_SEEK_URGENCY_MULTIPLIER, STAND_GROUND_BASE_URGENCY,
};
use crate::world::field_grid_plugin::FieldGrids;
use bevy::prelude::*;

pub struct EmotionalInputs<'a> {
    pub emotions: &'a EmotionalState,
    pub mind: &'a MindGraph,
    pub visible: &'a VisibleObjects,
    pub visible_positions: &'a [(Entity, Vec2)],
    pub physical: &'a PhysicalNeeds,
    pub drives: Option<&'a PsychologicalDrives>,
    pub in_conversation: Option<&'a InConversation>,
    pub self_concept: Option<Concept>,
    pub agent_pos: Vec2,
    pub fields: &'a FieldGrids,
    pub cns: &'a crate::agent::nervous_system::cns::CentralNervousSystem,
    pub action_registry: &'a crate::agent::actions::ActionRegistry,
    /// Big Five traits, used by threat appraisal for boldness scoring.
    pub personality: Option<&'a crate::agent::psyche::personality::PersonalityTraits>,
    /// Defender body, used by threat appraisal for combat-power scoring.
    pub body: Option<&'a crate::agent::biology::body::Body>,
    /// True when `pick_flee_target` exhausted candidates last tick —
    /// drops the Fight threshold so trapped agents engage.
    pub cornered: bool,
    /// Pre-resolved closest visible Dangerous entity and its body, used
    /// by threat appraisal as the comparison target. None means "no
    /// pressing threat" → general-fear path runs instead.
    pub closest_threat: Option<ClosestThreat<'a>>,
}

pub struct ClosestThreat<'a> {
    pub entity: Entity,
    pub pos: Vec2,
    pub body: Option<&'a crate::agent::biology::body::Body>,
}

impl<'a> EmotionalInputs<'a> {
    /// Borrow the subset needed by tile-based scorers (drift, action-prep).
    /// Seven fields shared with `PreferenceContext`; avoids reconstructing
    /// them at every call site.
    pub fn preference_context(&self) -> crate::agent::actions::definition::PreferenceContext<'a> {
        crate::agent::actions::definition::PreferenceContext {
            agent_pos: self.agent_pos,
            self_concept: self.self_concept,
            physical: self.physical,
            drives: self.drives,
            mind: self.mind,
            visible: self.visible_positions,
            fields: self.fields,
        }
    }
}

pub fn emotional_brain_propose(inputs: &EmotionalInputs) -> Option<BrainProposal> {
    let mut best: Option<BrainProposal> = None;
    let mut best_urgency = 0.0f32;

    for &entity in &inputs.visible.entities {
        if let Some((proposal, urgency)) =
            evaluate_entity_emotions(entity, inputs.mind, inputs.action_registry, best_urgency)
        {
            best = Some(proposal);
            best_urgency = urgency;
        }
    }

    // Unified threat-appraisal path: when a Dangerous entity is visible,
    // run the full appraisal function (defender vs attacker power,
    // desperation, personality boldness, anger, cornered). The output
    // ThreatResponse maps to a single proposal — Flee, StandGround,
    // or Fight — replacing the three ad-hoc branches that used to live
    // here (general fear, general anger, starving-predator).
    if let Some(threat) = inputs.closest_threat.as_ref() {
        let entity_anger = entity_emotion_intensity(inputs.mind, threat.entity, EmotionType::Anger);
        let general_anger = inputs.emotions.get_emotion_intensity(EmotionType::Anger);
        let anger = entity_anger.max(general_anger);
        let ctx = super::threat_appraisal::ThreatAppraisalContext {
            physical: inputs.physical,
            body: inputs.body,
            personality: inputs.personality,
            anger,
            cornered: inputs.cornered,
            attacker_body: threat.body,
            dependents_nearby: 0,
            on_home_turf: false,
            prior_experience: 0.0,
        };
        if let Some(proposal) = appraise_threat_proposal(
            &ctx,
            threat,
            inputs.self_concept,
            inputs.action_registry,
            best_urgency,
        ) {
            best_urgency = proposal.urgency;
            best = Some(proposal);
        }
    } else if let Some(proposal) =
        check_general_fear(inputs.emotions, best_urgency, inputs.action_registry)
    {
        // No specific Dangerous entity visible but accumulated general
        // fear — still flee. Covers post-trauma fear, audible-alarm
        // fear, and other no-visible-threat cases.
        best_urgency = proposal.urgency;
        best = Some(proposal);
    }

    // Social seeking — conversation path (humans only). Gated on
    // in_conversation because a second conversation mid-chat is silly
    // (channel costs alone can't block it: InitiateConversation is Focus 0).
    if inputs.in_conversation.is_none()
        && inputs.self_concept == Some(Concept::Person)
        && let Some(d) = inputs.drives
        && let Some(proposal) = seek_social_initiation(
            d.companionship.deficit(),
            inputs.visible,
            inputs.mind,
            inputs.action_registry,
            best_urgency,
        )
    {
        best_urgency = proposal.urgency;
        best = Some(proposal);
    }

    // Reactive drift — score local tiles per drive, walk toward the best.
    if inputs.in_conversation.is_none() {
        let drift_ctx = DriftContext {
            agent_pos: inputs.agent_pos,
            self_concept: inputs.self_concept,
            physical: inputs.physical,
            drives: inputs.drives,
            mind: inputs.mind,
            visible: inputs.visible_positions,
            fields: inputs.fields,
        };
        for behavior in BEHAVIORS {
            if let Some(proposal) =
                propose_drift(behavior, &drift_ctx, inputs.action_registry, best_urgency)
            {
                best_urgency = proposal.urgency;
                best = Some(proposal);
            }
        }
    }

    // Ambient drives (curiosity, territoriality) — not in_conversation-gated;
    // channel conflicts handle that (Explore Focus 0.15 coexists with
    // Converse Focus 0.6; Observe Focus 0.3 + Awareness 0.6 soft-conflicts).
    if let Some(proposal) = propose_curiosity(
        inputs.cns,
        inputs.visible,
        inputs.mind,
        inputs.action_registry,
        best_urgency,
    ) {
        best_urgency = proposal.urgency;
        best = Some(proposal);
    }
    if let Some(proposal) = propose_patrol(inputs.cns, inputs.action_registry, best_urgency) {
        best = Some(proposal);
    }

    best
}

/// Propose `Observe` (if an agent is visible to watch) or `Explore`
/// (otherwise) when the agent's `Curiosity` urgency is active.
///
/// No thresholds. No arbitrary multipliers. `drives.curiosity` is a
/// real drainable state: it rises during unstimulating activity
/// (Idle/Sleep/Rest at ~+0.01/s) and drains from
/// Observe/Explore/Wander/Converse (via `RuntimeEffects::curiosity_per_sec`).
/// The proposal urgency follows the same `value * 40` pattern as
/// Social — comparable in weight, so the arbitrator picks whichever
/// drive is more pressing this moment.
///
/// When something interesting (another agent) is visible, Observe is
/// the specific satisfier. Otherwise the agent falls through to
/// Explore — go find something to look at.
fn propose_curiosity(
    cns: &crate::agent::nervous_system::cns::CentralNervousSystem,
    visible: &VisibleObjects,
    mind: &MindGraph,
    action_registry: &crate::agent::actions::ActionRegistry,
    min_urgency: f32,
) -> Option<BrainProposal> {
    use crate::agent::nervous_system::urgency::UrgencySource;
    let u = cns
        .urgencies
        .iter()
        .find(|u| matches!(u.source, UrgencySource::Curiosity | UrgencySource::Fun))?;
    // Match Social's 40× multiplier so curiosity competes on the same
    // scale as other psychological drives. The drive itself gates
    // firing — once drives.curiosity drains below the drive config's
    // `min_threshold`, no urgency is emitted and this whole path
    // returns None on its own.
    let urgency = u.value * 40.0;
    if urgency <= min_urgency {
        return None;
    }

    // Pick the most interesting visible entity: another agent beats a
    // static object. A curious creature watches moving things, not
    // the berry bush it's seen a thousand times. Filtering to agents
    // also keeps the target fresh because agents themselves move.
    let interesting_target = visible.entities.iter().find(|&&e| is_agent(mind, e));
    if let Some(&target) = interesting_target {
        let observe = action_registry.get(ActionType::Observe)?;
        let mut template = observe.to_template(None);
        template.target_entity = Some(target);
        template.escalate_intensity(u.value);
        return Some(BrainProposal {
            brain: BrainType::Emotional,
            action: template,
            urgency,
            intent: Intent::from_urgency_source(u.source),
            reasoning: format!("Curious — watching ({:.2})", u.value),
        });
    }
    let explore = action_registry.get(ActionType::Explore)?;
    let mut template = explore.to_template(None);
    template.escalate_intensity(u.value);
    Some(BrainProposal {
        brain: BrainType::Emotional,
        action: template,
        urgency,
        intent: Intent::from_urgency_source(u.source),
        reasoning: format!("Curious — exploring ({:.2})", u.value),
    })
}

/// True if the MindGraph says this entity is a Person, Deer, or Wolf —
/// i.e. another agent. Static objects (berry bushes, trees, rocks) are
/// filtered out so a curious agent doesn't freeze staring at a stump.
fn is_agent(mind: &MindGraph, entity: Entity) -> bool {
    const AGENT_CONCEPTS: &[Concept] = &[Concept::Person, Concept::Deer, Concept::Wolf];
    for concept in AGENT_CONCEPTS {
        if !mind
            .query(
                Some(&Node::Entity(entity)),
                Some(Predicate::IsA),
                Some(&Value::Concept(*concept)),
            )
            .is_empty()
        {
            return true;
        }
    }
    false
}

/// Propose `Wander` for Territoriality urgency. The agent paces a
/// short local loop to watch over familiar ground. Wander's random-
/// walkable-target picker already keeps movement local, so the effect
/// reads as patrolling without needing a dedicated patrol action.
fn propose_patrol(
    cns: &crate::agent::nervous_system::cns::CentralNervousSystem,
    action_registry: &crate::agent::actions::ActionRegistry,
    min_urgency: f32,
) -> Option<BrainProposal> {
    use crate::agent::nervous_system::urgency::UrgencySource;
    let u = cns
        .urgencies
        .iter()
        .find(|u| matches!(u.source, UrgencySource::Territoriality))?;
    let urgency = u.value * 100.0;
    if urgency <= min_urgency {
        return None;
    }
    let wander = action_registry.get(ActionType::Wander)?;
    Some(BrainProposal {
        brain: BrainType::Emotional,
        action: wander.to_template(None),
        urgency,
        intent: Intent::SatisfyTerritoriality,
        reasoning: format!("Patrolling territory ({:.2})", u.value),
    })
}

/// Propose `InitiateConversation` toward a visible person if social drive is
/// high enough. Skips strangers (the agent's recognition system handles those
/// separately) — for now we accept any visible Person concept.
fn seek_social_initiation(
    social_drive: f32,
    visible: &VisibleObjects,
    mind: &MindGraph,
    action_registry: &crate::agent::actions::ActionRegistry,
    min_urgency: f32,
) -> Option<BrainProposal> {
    if social_drive <= SOCIAL_SEEK_THRESHOLD {
        return None;
    }

    let urgency = social_drive * SOCIAL_SEEK_URGENCY_MULTIPLIER;
    if urgency <= min_urgency {
        return None;
    }

    let action = action_registry.get(ActionType::InitiateConversation)?;

    for &entity in &visible.entities {
        // Must be a person.
        let is_person = !mind
            .query(
                Some(&Node::Entity(entity)),
                Some(Predicate::IsA),
                Some(&Value::Concept(Concept::Person)),
            )
            .is_empty();
        if !is_person {
            continue;
        }
        return Some(BrainProposal {
            brain: BrainType::Emotional,
            action: action.to_template(Some(entity)),
            urgency,
            intent: Intent::SatisfySocial,
            reasoning: format!("I want to chat with {entity:?} (social: {social_drive:.2})"),
        });
    }

    None
}

/// Closest visible entity the agent considers `Dangerous`, with its
/// world position.
/// Find the closest visible Dangerous entity. Looks the trait up by the
/// entity's concept (`mind.has_trait(&Node::Concept(...))`) instead of
/// `mind.has_trait(&Node::Entity(...))` — both paths reach the same
/// answer, but the entity path pays an extra indexed MindGraph query to
/// walk `(entity, IsA, ?)` first, even when the concept is already
/// available from the entity's `EntityType` component. Saves a per-tick
/// MindGraph query per visible entity per agent inside arbitration.
pub fn find_closest_dangerous(
    visible: &VisibleObjects,
    mind: &MindGraph,
    transforms_and_types: &Query<(&Transform, Option<&crate::agent::inventory::EntityType>)>,
    agent_pos: Vec2,
) -> Option<(Entity, Vec2)> {
    let mut best: Option<(Entity, Vec2, f32)> = None;
    for &e in &visible.entities {
        let Ok((t, Some(entity_type))) = transforms_and_types.get(e) else {
            continue;
        };
        if !mind.has_trait(&Node::Concept(entity_type.0), Concept::Dangerous) {
            continue;
        }
        let pos = t.translation.truncate();
        let d = pos.distance_squared(agent_pos);
        if best.map(|(_, _, prev)| d < prev).unwrap_or(true) {
            best = Some((e, pos, d));
        }
    }
    best.map(|(e, pos, _)| (e, pos))
}

/// Per-emotion sums for one entity, including type-inherited contributions
/// (e.g. an entity-of-type-Wolf inherits the Concept-level Wolf feelings).
/// Public so the UI can read the agent's feelings toward any entity, not
/// just the three types `evaluate_entity_emotions` consumes.
pub fn entity_feelings(entity: Entity, mind: &MindGraph) -> Vec<(EmotionType, f32)> {
    let subject = Node::Entity(entity);
    let mut feelings: Vec<(EmotionType, f32)> = Vec::new();
    let mut collect = |subj: &Node| {
        for triple in mind.query(Some(subj), Some(Predicate::TriggersEmotion), None) {
            if let Value::Emotion(etype, intensity) = triple.object {
                feelings.push((etype, intensity));
            }
        }
    };
    collect(&subject);
    for concept in mind.all_types(&subject) {
        collect(&Node::Concept(concept));
    }
    feelings
}

/// All entities the agent has any `TriggersEmotion` triple for —
/// the keys for an "I feel about these entities" UI panel. Only
/// returns entity-level subjects; concept-level feelings (like "I
/// fear all Wolves in general") aren't enumerated here.
pub fn entities_with_feelings(mind: &MindGraph) -> Vec<Entity> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for triple in mind.query(None, Some(Predicate::TriggersEmotion), None) {
        if let Node::Entity(e) = triple.subject
            && seen.insert(e)
        {
            out.push(e);
        }
    }
    out
}

/// Sum of one emotion type toward an entity (entity-level + concept-level).
pub fn entity_emotion_intensity(
    mind: &MindGraph,
    entity: Entity,
    emotion_type: EmotionType,
) -> f32 {
    entity_feelings(entity, mind)
        .into_iter()
        .filter(|(t, _)| *t == emotion_type)
        .map(|(_, i)| i)
        .sum()
}

/// Sums into the existing (target, type) triple instead of forking a
/// new one each call. `MemoryType::Episodic` so entity-feelings about
/// no-longer-seen entities fade rather than persisting forever.
pub fn add_entity_emotion(
    mind: &mut MindGraph,
    target: Entity,
    emotion_type: EmotionType,
    delta: f32,
    tick: u64,
    source: crate::agent::mind::knowledge::Source,
) {
    use crate::agent::mind::knowledge::{MemoryType, Metadata, Triple};

    if delta <= 0.0 {
        return;
    }
    let subject = Node::Entity(target);

    let mut existing: Option<Value> = None;
    let mut existing_intensity: f32 = 0.0;
    for triple in mind.query(Some(&subject), Some(Predicate::TriggersEmotion), None) {
        if let Value::Emotion(t, i) = triple.object
            && t == emotion_type
        {
            existing = Some(Value::Emotion(t, i));
            existing_intensity = i;
            break;
        }
    }

    if let Some(old) = existing {
        mind.remove(&subject, Predicate::TriggersEmotion, &old);
    }

    let new_intensity = (existing_intensity + delta).clamp(0.0, MAX_ENTITY_EMOTION_INTENSITY);
    let mut meta = Metadata::experience(tick);
    meta.source = source;
    meta.memory_type = MemoryType::Episodic;
    mind.assert(Triple::with_meta(
        subject,
        Predicate::TriggersEmotion,
        Value::Emotion(emotion_type, new_intensity),
        meta,
    ));
}

/// Cap per-entity emotion intensity. Past this point further events
/// don't deepen the feeling — prevents anger toward one wolf from
/// dominating arbitration after dozens of hits.
pub const MAX_ENTITY_EMOTION_INTENSITY: f32 = 2.0;

/// (fear, joy, anger) intensities from direct and inherited associations.
fn collect_entity_feelings(entity: Entity, mind: &MindGraph) -> (f32, f32, f32) {
    let feelings = entity_feelings(entity, mind);
    let sum = |target: EmotionType| -> f32 {
        feelings
            .iter()
            .filter(|(e, _)| *e == target)
            .map(|(_, i)| i)
            .sum()
    };
    (
        sum(EmotionType::Fear),
        sum(EmotionType::Joy),
        sum(EmotionType::Anger),
    )
}

/// Returns the best (proposal, intensity) for a single entity, if above min_urgency.
fn evaluate_entity_emotions(
    entity: Entity,
    mind: &MindGraph,
    action_registry: &crate::agent::actions::ActionRegistry,
    min_urgency: f32,
) -> Option<(BrainProposal, f32)> {
    let (fear, joy, anger) = collect_entity_feelings(entity, mind);
    let mut best: Option<(BrainProposal, f32)> = None;
    let mut threshold = min_urgency;

    if fear > threshold
        && fear > FEAR_ENTITY_THRESHOLD
        && let Some(action) = action_registry.get(ActionType::Flee)
    {
        let mut template = action.to_template(Some(entity));
        template.escalate_intensity(fear);
        best = Some((
            BrainProposal {
                brain: BrainType::Emotional,
                action: template,
                urgency: fear * FEAR_ENTITY_URGENCY_MULTIPLIER,
                intent: Intent::SatisfySafety,
                reasoning: format!("I'm scared of {:?} (fear: {:.2})", entity, fear),
            },
            fear,
        ));
        threshold = fear;
    }

    if joy > threshold
        && joy > JOY_ENTITY_THRESHOLD
        && let Some(action) = action_registry.get(ActionType::Walk)
    {
        let mut template = action.to_template(Some(entity));
        template.escalate_intensity(joy);
        best = Some((
            BrainProposal {
                brain: BrainType::Emotional,
                action: template,
                urgency: joy * JOY_ENTITY_URGENCY_MULTIPLIER,
                intent: Intent::SatisfySocial,
                reasoning: format!("I like {:?} (joy: {:.2})", entity, joy),
            },
            joy,
        ));
        threshold = joy;
    }

    if anger > threshold
        && anger > ANGER_ENTITY_THRESHOLD
        && let Some(action) = action_registry.get(ActionType::Attack)
    {
        let mut template = action.to_template(Some(entity));
        template.escalate_intensity(anger);
        best = Some((
            BrainProposal {
                brain: BrainType::Emotional,
                action: template,
                urgency: anger * ANGER_ENTITY_URGENCY_MULTIPLIER,
                intent: Intent::SatisfySafety,
                reasoning: format!("I hate {:?}! (anger: {:.2})", entity, anger),
            },
            anger,
        ));
    }

    best
}

/// Convert a [`ThreatResponse`] into a concrete [`BrainProposal`] aimed
/// at the closest threat. Maps Flee → `Flee`, StandGround → `Idle`,
/// Fight → species-appropriate combat verb (Wolf → `Bite`, Person →
/// `DefendSelf`). Replaces the three ad-hoc proposal branches with one
/// dispatch driven entirely by the appraisal output.
fn appraise_threat_proposal(
    ctx: &super::threat_appraisal::ThreatAppraisalContext,
    threat: &ClosestThreat,
    self_concept: Option<Concept>,
    action_registry: &crate::agent::actions::ActionRegistry,
    best_urgency: f32,
) -> Option<BrainProposal> {
    use super::threat_appraisal::{ThreatResponse, appraise_threat};

    let response = appraise_threat(ctx);

    match response {
        ThreatResponse::Flee { urgency } => {
            let action = action_registry.get(ActionType::Flee)?;
            let proposal_urgency = urgency * FLEE_RESPONSE_URGENCY_MULTIPLIER;
            if proposal_urgency <= best_urgency {
                return None;
            }
            let mut template = action.to_template(Some(threat.entity));
            template.escalate_intensity(urgency);
            Some(BrainProposal {
                brain: BrainType::Emotional,
                action: template,
                urgency: proposal_urgency,
                intent: Intent::SatisfySafety,
                reasoning: format!("Threat appraisal → Flee (urgency {urgency:.2})"),
            })
        }
        ThreatResponse::StandGround => {
            let action = action_registry.get(ActionType::Idle)?;
            let proposal_urgency = STAND_GROUND_BASE_URGENCY.max(best_urgency + 0.1);
            if proposal_urgency <= best_urgency {
                return None;
            }
            let mut template = action.to_template(None);
            template.escalate_intensity(0.5);
            Some(BrainProposal {
                brain: BrainType::Emotional,
                action: template,
                urgency: proposal_urgency,
                intent: Intent::SatisfySafety,
                reasoning: "Threat appraisal → StandGround (cornered)".to_string(),
            })
        }
        ThreatResponse::Fight { commitment } => {
            let attack_action = match self_concept {
                Some(Concept::Wolf) => ActionType::Bite,
                _ => ActionType::DefendSelf,
            };
            let action = action_registry.get(attack_action)?;
            let proposal_urgency = (FIGHT_RESPONSE_BASE_URGENCY
                + commitment * FIGHT_RESPONSE_COMMITMENT_MULTIPLIER)
                .max(best_urgency + 0.1);
            if proposal_urgency <= best_urgency {
                return None;
            }
            let mut template = action.to_template(Some(threat.entity));
            template.escalate_intensity(commitment.max(0.5));
            Some(BrainProposal {
                brain: BrainType::Emotional,
                action: template,
                urgency: proposal_urgency,
                intent: Intent::SatisfySafety,
                reasoning: format!("Threat appraisal → Fight (commitment {commitment:.2})"),
            })
        }
    }
}

/// Responds to general (non-entity) fear.
fn check_general_fear(
    emotions: &EmotionalState,
    best_urgency: f32,
    action_registry: &crate::agent::actions::ActionRegistry,
) -> Option<BrainProposal> {
    let fear_level: f32 = emotions
        .active_emotions
        .iter()
        .filter(|e| e.emotion_type == EmotionType::Fear)
        .map(|e| e.intensity)
        .sum();

    if fear_level <= FEAR_GENERAL_THRESHOLD {
        return None;
    }

    let fear_urgency = fear_level * FEAR_GENERAL_URGENCY_MULTIPLIER;
    if fear_urgency > best_urgency
        && let Some(action) = action_registry.get(ActionType::Flee)
    {
        let mut template = action.to_template(None);
        template.escalate_intensity(fear_level);
        return Some(BrainProposal {
            brain: BrainType::Emotional,
            action: template,
            urgency: fear_urgency,
            intent: Intent::SatisfySafety,
            reasoning: format!("I'm terrified! (fear: {:.2})", fear_level),
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mind::knowledge::{Metadata, MindGraph, Node, Predicate, Triple, Value};
    use crate::agent::psyche::emotions::Emotion;

    fn setup_mind() -> MindGraph {
        MindGraph::default()
    }

    #[test]
    fn test_emotional_fear_response() {
        let mut state = EmotionalState::default();
        state.add_emotion(Emotion::new(EmotionType::Fear, 0.9));

        let mind = setup_mind();
        let visible = VisibleObjects::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register_def(&crate::agent::actions::action::FLEE_DEF);

        let proposal = emotional_brain_propose(&EmotionalInputs {
            emotions: &state,
            mind: &mind,
            visible: &visible,
            visible_positions: &[],
            physical: &PhysicalNeeds::default(),
            drives: None,
            in_conversation: None,
            self_concept: None,
            agent_pos: Vec2::ZERO,
            fields: &FieldGrids::default(),
            cns: &Default::default(),
            action_registry: &registry,
            personality: None,
            body: None,
            cornered: false,
            closest_threat: None,
        });

        assert!(proposal.is_some());
        let prop = proposal.unwrap();
        assert_eq!(prop.brain, BrainType::Emotional);
        assert_eq!(prop.action.name, "Flee");
        assert!(prop.urgency > 60.0);
    }

    #[test]
    fn test_emotional_entity_fear() {
        let state = EmotionalState::default();
        let entity = Entity::from_bits(42);
        let mut mind = setup_mind();

        mind.assert(Triple::with_meta(
            Node::Entity(entity),
            Predicate::TriggersEmotion,
            Value::Emotion(EmotionType::Fear, 0.8),
            Metadata::default(),
        ));

        let mut visible = VisibleObjects::default();
        visible.entities.push(entity);

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register_def(&crate::agent::actions::action::FLEE_DEF);

        let proposal = emotional_brain_propose(&EmotionalInputs {
            emotions: &state,
            mind: &mind,
            visible: &visible,
            visible_positions: &[],
            physical: &PhysicalNeeds::default(),
            drives: None,
            in_conversation: None,
            self_concept: None,
            agent_pos: Vec2::ZERO,
            fields: &FieldGrids::default(),
            cns: &Default::default(),
            action_registry: &registry,
            personality: None,
            body: None,
            cornered: false,
            closest_threat: None,
        });

        assert!(proposal.is_some());
        let prop = proposal.unwrap();
        assert!(prop.action.name.contains("Flee"));
    }

    #[test]
    fn test_emotional_entity_joy() {
        let state = EmotionalState::default();
        let entity = Entity::from_bits(42);
        let mut mind = setup_mind();

        mind.assert(Triple::with_meta(
            Node::Entity(entity),
            Predicate::TriggersEmotion,
            Value::Emotion(EmotionType::Joy, 0.6),
            Metadata::default(),
        ));

        let mut visible = VisibleObjects::default();
        visible.entities.push(entity);

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register_def(&crate::agent::actions::action::WALK_DEF);

        let proposal = emotional_brain_propose(&EmotionalInputs {
            emotions: &state,
            mind: &mind,
            visible: &visible,
            visible_positions: &[],
            physical: &PhysicalNeeds::default(),
            drives: None,
            in_conversation: None,
            self_concept: None,
            agent_pos: Vec2::ZERO,
            fields: &FieldGrids::default(),
            cns: &Default::default(),
            action_registry: &registry,
            personality: None,
            body: None,
            cornered: false,
            closest_threat: None,
        });

        assert!(proposal.is_some());
        let prop = proposal.unwrap();
        assert!(prop.action.name.contains("Walk"));
    }

    #[test]
    fn test_emotional_returns_none_when_idle_with_empty_registry() {
        let state = EmotionalState::default();
        let mind = setup_mind();
        let visible = VisibleObjects::default();

        let registry = crate::agent::actions::ActionRegistry::default();
        let proposal = emotional_brain_propose(&EmotionalInputs {
            emotions: &state,
            mind: &mind,
            visible: &visible,
            visible_positions: &[],
            physical: &PhysicalNeeds::default(),
            drives: None,
            in_conversation: None,
            self_concept: None,
            agent_pos: Vec2::ZERO,
            fields: &FieldGrids::default(),
            cns: &Default::default(),
            action_registry: &registry,
            personality: None,
            body: None,
            cornered: false,
            closest_threat: None,
        });

        assert!(proposal.is_none());
    }

    #[test]
    fn emotional_brain_flee_carries_safety_intent() {
        use crate::agent::actions::motor::{ActionPrimitive, Intent as MotorIntent};

        let mut state = EmotionalState::default();
        state.add_emotion(Emotion::new(EmotionType::Fear, 0.9));

        let mind = setup_mind();
        let visible = VisibleObjects::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register_def(&crate::agent::actions::action::FLEE_DEF);

        let proposal = emotional_brain_propose(&EmotionalInputs {
            emotions: &state,
            mind: &mind,
            visible: &visible,
            visible_positions: &[],
            physical: &PhysicalNeeds::default(),
            drives: None,
            in_conversation: None,
            self_concept: None,
            agent_pos: Vec2::ZERO,
            fields: &FieldGrids::default(),
            cns: &Default::default(),
            action_registry: &registry,
            personality: None,
            body: None,
            cornered: false,
            closest_threat: None,
        })
        .expect("should propose Flee");

        let behavior = &proposal.action.behavior;
        assert_eq!(
            behavior.primitive,
            ActionPrimitive::Locomote,
            "Flee should use the Locomote primitive"
        );
        assert_eq!(
            behavior.intent,
            MotorIntent::Safety,
            "Flee should carry Safety intent"
        );
    }
}
