//! Emotional brain: association-driven behavior based on feelings.
//!
//! Reads: EmotionalState, MindGraph, VisibleObjects, PsychologicalDrives, Engaged
//! Writes: BrainProposal
//! Upstream: perception (VisibleObjects), psyche (EmotionalState)
//! Downstream: brains::proposal (winner selection)
//!
//! Conversation continuation and turn-taking are handled by the
//! [`ConversePlugin`](crate::agent::engagement::converse::ConversePlugin).
//! The emotional brain only proposes the *initiation* of conversations
//! (`ActionType::InitiateConversation`); once registered, the plugin owns the
//! lifecycle.

use super::drift::{BEHAVIORS, DriftContext, propose_drift};
use super::proposal::{BrainProposal, BrainType, Intent};
use super::social_initiation::SocialInitiationCooldowns;
use crate::agent::actions::ActionType;
use crate::agent::body::needs::{PhysicalNeeds, PsychologicalDrives};
use crate::agent::engagement::Engaged;
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
use crate::world::map::TILE_SIZE;
use crate::world::spatial_index::world_pos_to_tile;
use bevy::prelude::*;

pub struct EmotionalInputs<'a> {
    pub emotions: &'a EmotionalState,
    pub mind: &'a MindGraph,
    pub visible: &'a VisibleObjects,
    pub visible_positions: &'a [(Entity, Vec2)],
    /// Parallel-indexed with `visible_positions`. `Some(concept)` for
    /// agents/things with an `EntityType` component; `None` for the rest.
    /// Lets the per-visible-entity loops do `mind.has_trait(&Node::Concept(_))`
    /// at the ontology-cache fast-path instead of `Node::Entity(_)` (which
    /// pays a wasted `(entity, IsA, ?)` indexed query before falling
    /// through to the same cache).
    pub visible_types: &'a [Option<Concept>],
    pub physical: &'a PhysicalNeeds,
    pub drives: Option<&'a PsychologicalDrives>,
    pub engaged: Option<&'a Engaged>,
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
    /// Parallel to `visible_positions`. `true` when the entity is in
    /// someone else's conversation — used to skip `ConversationFull`
    /// initiations.
    pub visible_engaged_converse: &'a [bool],
    /// Per-target `InitiateConversation` failure cooldowns; `None` until
    /// the agent records its first failure.
    pub social_cooldowns: Option<&'a SocialInitiationCooldowns>,
    pub current_tick: u64,
}

pub struct ClosestThreat<'a> {
    pub entity: Entity,
    pub pos: Vec2,
    /// Concept from the threat's `EntityType` component (Wolf, Person…),
    /// threaded through so threat-appraisal can read concept-level
    /// emotions without paying an indexed `(entity, IsA, ?)` lookup.
    pub type_concept: Option<Concept>,
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
            visible_types: self.visible_types,
            fields: self.fields,
        }
    }
}

pub fn emotional_brain_propose(inputs: &EmotionalInputs) -> Option<BrainProposal> {
    let mut best: Option<BrainProposal> = None;
    let mut best_urgency = 0.0f32;

    // `visible_positions` and `visible_types` are pre-built in
    // arbitrate_every_tick from a single Bevy query, parallel-indexed so
    // visible_positions[i] is the same agent as visible_types[i]. Iterate
    // them together to skip the per-call `(entity, IsA, ?)` lookup that
    // walking from `Node::Entity(_)` would cost.
    for (i, &(entity, _)) in inputs.visible_positions.iter().enumerate() {
        let entity_type = inputs.visible_types.get(i).and_then(|t| *t);
        if let Some((proposal, urgency)) = evaluate_entity_emotions(
            entity,
            entity_type,
            inputs.mind,
            inputs.action_registry,
            best_urgency,
        ) {
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
        let entity_anger = match threat.type_concept {
            Some(et) => entity_emotion_intensity_with_type(
                inputs.mind,
                threat.entity,
                et,
                EmotionType::Anger,
            ),
            None => entity_emotion_intensity(inputs.mind, threat.entity, EmotionType::Anger),
        };
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
    // engaged because a second engagement mid-chat is silly
    // (channel costs alone can't block it: InitiateConversation is Focus 0).
    if inputs.engaged.is_none()
        && inputs.self_concept == Some(Concept::Person)
        && let Some(d) = inputs.drives
        && let Some(proposal) =
            seek_social_initiation(d.companionship.deficit(), inputs, best_urgency)
    {
        best_urgency = proposal.urgency;
        best = Some(proposal);
    }

    // Reactive drift — score local tiles per drive, walk toward the best.
    if inputs.engaged.is_none() {
        let drift_ctx = DriftContext {
            agent_pos: inputs.agent_pos,
            self_concept: inputs.self_concept,
            physical: inputs.physical,
            drives: inputs.drives,
            mind: inputs.mind,
            visible: inputs.visible_positions,
            visible_types: inputs.visible_types,
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

    // Ambient drives (curiosity, territoriality) — not engagement-gated;
    // channel conflicts handle that (Explore Focus 0.15 coexists with
    // Converse Focus 0.6; Observe Focus 0.3 + Awareness 0.6 soft-conflicts).
    if let Some(proposal) = propose_curiosity(
        inputs.cns,
        inputs.mind,
        inputs.visible_positions,
        inputs.visible_types,
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
    mind: &MindGraph,
    visible_positions: &[(Entity, Vec2)],
    visible_types: &[Option<Concept>],
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
    // the berry bush it's seen a thousand times. Read agent-ness from
    // the entity's `EntityType` concept threaded through `visible_types`
    // (parallel-indexed with `visible_positions`) — saves three
    // MindGraph IsA queries per visible entity vs. the old path.
    let interesting_target = visible_positions
        .iter()
        .enumerate()
        .find(|(i, _)| {
            visible_types
                .get(*i)
                .and_then(|c| *c)
                .is_some_and(|c| mind.has_trait(&Node::Concept(c), Concept::Sentient))
        })
        .map(|(_, (e, _))| *e);
    if let Some(target) = interesting_target {
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

/// Affection weight for candidate ranking, expressed in tile units so a
/// maximally-fond partner outranks a stranger by roughly that many
/// tiles of distance.
const AFFECTION_RANK_WEIGHT: f32 = 6.0;

/// Propose `InitiateConversation` toward the best-scoring visible
/// person. Filters busy / unreachable / cooled-down candidates, then
/// picks the closest-and-fondest survivor. Strangers are eligible —
/// the first turn of any conversation is the greeting, owned by
/// `ConversePlugin`.
fn seek_social_initiation(
    social_drive: f32,
    inputs: &EmotionalInputs,
    min_urgency: f32,
) -> Option<BrainProposal> {
    if social_drive <= SOCIAL_SEEK_THRESHOLD {
        return None;
    }

    let urgency = social_drive * SOCIAL_SEEK_URGENCY_MULTIPLIER;
    if urgency <= min_urgency {
        return None;
    }

    let action = inputs
        .action_registry
        .get(ActionType::InitiateConversation)?;

    // Lazy: skip the per-tick MindGraph scan for the common "not
    // lonely / nobody around" path. Above the threshold and at this
    // point in the function we know the proposer is going to do real
    // work, so the scan is cheaper than threading the cache through.
    let unreachable_tiles =
        super::planner::collect_unreachable_tiles(inputs.mind, inputs.current_tick);

    let mut best: Option<(Entity, f32)> = None;
    for (i, &(entity, pos)) in inputs.visible_positions.iter().enumerate() {
        if inputs.visible_types.get(i).and_then(|c| *c) != Some(Concept::Person) {
            continue;
        }

        if inputs
            .visible_engaged_converse
            .get(i)
            .copied()
            .unwrap_or(false)
        {
            continue;
        }

        let tile_v = world_pos_to_tile(pos);
        if unreachable_tiles.contains(&(tile_v.x, tile_v.y)) {
            continue;
        }

        if inputs
            .social_cooldowns
            .is_some_and(|c| c.is_on_cooldown(entity, inputs.current_tick))
        {
            continue;
        }

        let affection = mind_affection(inputs.mind, entity);
        let distance = pos.distance(inputs.agent_pos) / TILE_SIZE;
        let score = -distance + AFFECTION_RANK_WEIGHT * affection;

        if best.map(|(_, prev)| score > prev).unwrap_or(true) {
            best = Some((entity, score));
        }
    }

    let (target, _) = best?;
    Some(BrainProposal {
        brain: BrainType::Emotional,
        action: action.to_template(Some(target)),
        urgency,
        intent: Intent::SatisfySocial,
        reasoning: format!("I want to chat with {target:?} (social: {social_drive:.2})"),
    })
}

fn mind_affection(mind: &MindGraph, entity: Entity) -> f32 {
    mind.get(&Node::Entity(entity), Predicate::Affection)
        .and_then(|v| v.as_quantity())
        .map(|q| q.point_estimate())
        .unwrap_or(0.0)
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
    for (concept, ents) in visible.by_concept.iter() {
        if !mind.has_trait(&Node::Concept(*concept), Concept::Dangerous) {
            continue;
        }
        for &e in ents {
            let Ok((t, _)) = transforms_and_types.get(e) else {
                continue;
            };
            let pos = t.translation.truncate();
            let d = pos.distance_squared(agent_pos);
            if best.map(|(_, _, prev)| d < prev).unwrap_or(true) {
                best = Some((e, pos, d));
            }
        }
    }
    best.map(|(e, pos, _)| (e, pos))
}

/// Per-emotion sums for one entity, including type-inherited contributions
/// (e.g. an entity-of-type-Wolf inherits the Concept-level Wolf feelings).
/// Public so the UI can read the agent's feelings toward any entity, not
/// just the three types `evaluate_entity_emotions` consumes.
/// Per-emotion sums toward `entity` including type-inherited contributions
/// (e.g. an entity-of-type-Wolf inherits the Concept-level Wolf feelings).
///
/// The "what type is this entity" question is resolved by walking the
/// agent's per-agent `(entity, IsA, ?)` MindGraph triples — i.e. the
/// agent's *belief* about what the entity is. Cold-path callers (UI,
/// debug inspection) want this. Hot-path callers inside arbitration
/// know the entity's `EntityType` component already and should use
/// [`entity_feelings_with_type`] to skip the per-call IsA-walk query.
pub fn entity_feelings(entity: Entity, mind: &MindGraph) -> Vec<(EmotionType, f32)> {
    entity_feelings_inner(entity, mind.all_types(&Node::Entity(entity)), mind)
}

/// Hot-path variant of [`entity_feelings`] for callers who already know
/// the entity's `EntityType` concept (typically threaded through from
/// the brain's per-tick visible-entity loop). Walks the ontology
/// hierarchy from `entity_type` directly instead of paying an indexed
/// `(entity, IsA, ?)` lookup per call.
pub fn entity_feelings_with_type(
    entity: Entity,
    entity_type: Concept,
    mind: &MindGraph,
) -> Vec<(EmotionType, f32)> {
    let mut types: Vec<Concept> = Vec::with_capacity(8);
    types.push(entity_type);
    types.extend(mind.all_types(&Node::Concept(entity_type)));
    entity_feelings_inner(entity, types, mind)
}

fn entity_feelings_inner(
    entity: Entity,
    type_concepts: Vec<Concept>,
    mind: &MindGraph,
) -> Vec<(EmotionType, f32)> {
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
    for concept in type_concepts {
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
/// Cold-path variant — falls back to walking the agent's IsA triples.
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

/// Hot-path variant of [`entity_emotion_intensity`]. Same answer, but the
/// caller supplies the entity's `EntityType` concept so we can walk the
/// ontology hierarchy directly from there.
pub fn entity_emotion_intensity_with_type(
    mind: &MindGraph,
    entity: Entity,
    entity_type: Concept,
    emotion_type: EmotionType,
) -> f32 {
    entity_feelings_with_type(entity, entity_type, mind)
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
fn collect_entity_feelings(
    entity: Entity,
    entity_type: Option<Concept>,
    mind: &MindGraph,
) -> (f32, f32, f32) {
    let feelings = match entity_type {
        Some(et) => entity_feelings_with_type(entity, et, mind),
        None => entity_feelings(entity, mind),
    };
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
    entity_type: Option<Concept>,
    mind: &MindGraph,
    action_registry: &crate::agent::actions::ActionRegistry,
    min_urgency: f32,
) -> Option<(BrainProposal, f32)> {
    let (fear, joy, anger) = collect_entity_feelings(entity, entity_type, mind);
    let mut best: Option<(BrainProposal, f32)> = None;
    let mut threshold = min_urgency;

    if fear > threshold
        && fear > FEAR_ENTITY_THRESHOLD
        && let Some(action) = action_registry.get(ActionType::InitiateFlee)
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
            let action = action_registry.get(ActionType::InitiateFlee)?;
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
            // Bite is a Hunt-engagement-internal beat post-#743; brain
            // proposals route every species through `DefendSelf` for
            // reactive combat. Predator-vs-prey aggression goes through
            // `InitiateHunt` instead, which is a different pathway.
            let _ = self_concept;
            let attack_action = ActionType::DefendSelf;
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
        && let Some(action) = action_registry.get(ActionType::InitiateFlee)
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
            visible_types: &[],
            physical: &PhysicalNeeds::default(),
            drives: None,
            engaged: None,
            self_concept: None,
            agent_pos: Vec2::ZERO,
            fields: &FieldGrids::default(),
            cns: &Default::default(),
            action_registry: &registry,
            personality: None,
            body: None,
            cornered: false,
            closest_threat: None,
            visible_engaged_converse: &[],
            social_cooldowns: None,
            current_tick: 0,
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
        let visible_positions = [(entity, Vec2::ZERO)];

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register_def(&crate::agent::actions::action::FLEE_DEF);

        let proposal = emotional_brain_propose(&EmotionalInputs {
            emotions: &state,
            mind: &mind,
            visible: &visible,
            visible_positions: &visible_positions,
            visible_types: &[None],
            physical: &PhysicalNeeds::default(),
            drives: None,
            engaged: None,
            self_concept: None,
            agent_pos: Vec2::ZERO,
            fields: &FieldGrids::default(),
            cns: &Default::default(),
            action_registry: &registry,
            personality: None,
            body: None,
            cornered: false,
            closest_threat: None,
            visible_engaged_converse: &[],
            social_cooldowns: None,
            current_tick: 0,
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
        let visible_positions = [(entity, Vec2::ZERO)];

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register_def(&crate::agent::actions::action::WALK_DEF);

        let proposal = emotional_brain_propose(&EmotionalInputs {
            emotions: &state,
            mind: &mind,
            visible: &visible,
            visible_positions: &visible_positions,
            visible_types: &[None],
            physical: &PhysicalNeeds::default(),
            drives: None,
            engaged: None,
            self_concept: None,
            agent_pos: Vec2::ZERO,
            fields: &FieldGrids::default(),
            cns: &Default::default(),
            action_registry: &registry,
            personality: None,
            body: None,
            cornered: false,
            closest_threat: None,
            visible_engaged_converse: &[],
            social_cooldowns: None,
            current_tick: 0,
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
            visible_types: &[],
            physical: &PhysicalNeeds::default(),
            drives: None,
            engaged: None,
            self_concept: None,
            agent_pos: Vec2::ZERO,
            fields: &FieldGrids::default(),
            cns: &Default::default(),
            action_registry: &registry,
            personality: None,
            body: None,
            cornered: false,
            closest_threat: None,
            visible_engaged_converse: &[],
            social_cooldowns: None,
            current_tick: 0,
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
            visible_types: &[],
            physical: &PhysicalNeeds::default(),
            drives: None,
            engaged: None,
            self_concept: None,
            agent_pos: Vec2::ZERO,
            fields: &FieldGrids::default(),
            cns: &Default::default(),
            action_registry: &registry,
            personality: None,
            body: None,
            cornered: false,
            closest_threat: None,
            visible_engaged_converse: &[],
            social_cooldowns: None,
            current_tick: 0,
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

    // ─── seek_social_initiation ─────────────────────────────────────────────

    use super::super::social_initiation::{
        SOCIAL_INITIATION_COOLDOWN_TICKS, SocialInitiationCooldowns,
    };

    fn social_registry() -> crate::agent::actions::ActionRegistry {
        let mut r = crate::agent::actions::ActionRegistry::default();
        r.register_def(&crate::agent::actions::action::INITIATE_CONVERSATION_DEF);
        r
    }

    /// Owns the borrowed-by-EmotionalInputs scaffolding so each test
    /// can declare just the fields the social proposer reads.
    struct SocialFixture {
        emotions: EmotionalState,
        mind: MindGraph,
        visible: VisibleObjects,
        physical: PhysicalNeeds,
        cns: crate::agent::nervous_system::cns::CentralNervousSystem,
        fields: FieldGrids,
        registry: crate::agent::actions::ActionRegistry,
    }

    impl SocialFixture {
        fn new(mind: MindGraph) -> Self {
            Self {
                emotions: EmotionalState::default(),
                mind,
                visible: VisibleObjects::default(),
                physical: PhysicalNeeds::default(),
                cns: Default::default(),
                fields: FieldGrids::default(),
                registry: social_registry(),
            }
        }

        fn inputs<'a>(
            &'a self,
            visible_positions: &'a [(Entity, Vec2)],
            visible_types: &'a [Option<Concept>],
            visible_engaged_converse: &'a [bool],
            social_cooldowns: Option<&'a SocialInitiationCooldowns>,
            current_tick: u64,
        ) -> EmotionalInputs<'a> {
            EmotionalInputs {
                emotions: &self.emotions,
                mind: &self.mind,
                visible: &self.visible,
                visible_positions,
                visible_types,
                physical: &self.physical,
                drives: None,
                engaged: None,
                self_concept: Some(Concept::Person),
                agent_pos: Vec2::ZERO,
                fields: &self.fields,
                cns: &self.cns,
                action_registry: &self.registry,
                personality: None,
                body: None,
                cornered: false,
                closest_threat: None,
                visible_engaged_converse,
                social_cooldowns,
                current_tick,
            }
        }
    }

    /// Lonely enough that the proposer fires past `SOCIAL_SEEK_THRESHOLD`.
    const LONELY_DRIVE: f32 = 1.0;

    #[test]
    fn social_initiation_skips_unreachable_in_conversation_and_picks_reachable() {
        let mut mind = MindGraph::default();
        // Mark tile (5, 5) Unreachable for the agent.
        mind.assert(Triple::with_meta(
            Node::Tile((5, 5)),
            Predicate::HasTrait,
            Value::Concept(Concept::Unreachable),
            Metadata::experience(0),
        ));
        let fixture = SocialFixture::new(mind);

        let unreachable = Entity::from_bits(11);
        let busy = Entity::from_bits(12);
        let reachable = Entity::from_bits(13);
        let visible_positions = [
            (unreachable, Vec2::new(5.0 * TILE_SIZE, 5.0 * TILE_SIZE)),
            (busy, Vec2::new(TILE_SIZE, 0.0)),
            (reachable, Vec2::new(2.0 * TILE_SIZE, 0.0)),
        ];
        let visible_types = [
            Some(Concept::Person),
            Some(Concept::Person),
            Some(Concept::Person),
        ];
        let visible_engaged_converse = [false, true, false];
        let inputs = fixture.inputs(
            &visible_positions,
            &visible_types,
            &visible_engaged_converse,
            None,
            0,
        );

        let proposal = seek_social_initiation(LONELY_DRIVE, &inputs, 0.0)
            .expect("should propose toward the reachable, available person");
        assert_eq!(proposal.action.target_entity, Some(reachable));
    }

    #[test]
    fn social_initiation_returns_none_when_all_candidates_filtered() {
        let mut mind = MindGraph::default();
        mind.assert(Triple::with_meta(
            Node::Tile((5, 5)),
            Predicate::HasTrait,
            Value::Concept(Concept::Unreachable),
            Metadata::experience(0),
        ));
        let fixture = SocialFixture::new(mind);

        let unreachable = Entity::from_bits(11);
        let busy = Entity::from_bits(12);
        let visible_positions = [
            (unreachable, Vec2::new(5.0 * TILE_SIZE, 5.0 * TILE_SIZE)),
            (busy, Vec2::new(TILE_SIZE, 0.0)),
        ];
        let visible_types = [Some(Concept::Person), Some(Concept::Person)];
        let visible_engaged_converse = [false, true];
        let inputs = fixture.inputs(
            &visible_positions,
            &visible_types,
            &visible_engaged_converse,
            None,
            0,
        );

        assert!(
            seek_social_initiation(LONELY_DRIVE, &inputs, 0.0).is_none(),
            "no candidates → no proposal, no spam"
        );
    }

    #[test]
    fn social_initiation_skips_target_inside_failure_cooldown() {
        let fixture = SocialFixture::new(MindGraph::default());
        let only_candidate = Entity::from_bits(7);

        let visible_positions = [(only_candidate, Vec2::new(TILE_SIZE, 0.0))];
        let visible_types = [Some(Concept::Person)];
        let visible_engaged_converse = [false];

        let mut cooldowns = SocialInitiationCooldowns::default();
        cooldowns.record(only_candidate, 100);

        // Inside the cooldown window: skip even though it is the only
        // candidate (rather than re-spamming an already-failed target).
        let inputs = fixture.inputs(
            &visible_positions,
            &visible_types,
            &visible_engaged_converse,
            Some(&cooldowns),
            100,
        );
        assert!(
            seek_social_initiation(LONELY_DRIVE, &inputs, 0.0).is_none(),
            "cooled-down target must not be retried"
        );

        // After the cooldown window: candidate is re-eligible.
        let inputs = fixture.inputs(
            &visible_positions,
            &visible_types,
            &visible_engaged_converse,
            Some(&cooldowns),
            100 + SOCIAL_INITIATION_COOLDOWN_TICKS,
        );
        let proposal = seek_social_initiation(LONELY_DRIVE, &inputs, 0.0)
            .expect("expired cooldown should let the target through again");
        assert_eq!(proposal.action.target_entity, Some(only_candidate));
    }

    #[test]
    fn social_initiation_prefers_closer_and_fonder_candidate() {
        let close_stranger = Entity::from_bits(20);
        let far_friend = Entity::from_bits(21);

        // Close stranger sits 2 tiles away with no affection. Far friend
        // sits 5 tiles away with affection 1.0 — AFFECTION_RANK_WEIGHT
        // (6.0) means the friend wins by ~3 tile-units.
        let mut mind = MindGraph::default();
        crate::agent::mind::recognition::init_relationship_dimensions(
            &mut mind, far_friend, 0, 1.0,
        );
        let fixture = SocialFixture::new(mind);

        let visible_positions = [
            (close_stranger, Vec2::new(2.0 * TILE_SIZE, 0.0)),
            (far_friend, Vec2::new(5.0 * TILE_SIZE, 0.0)),
        ];
        let visible_types = [Some(Concept::Person), Some(Concept::Person)];
        let visible_engaged_converse = [false, false];
        let inputs = fixture.inputs(
            &visible_positions,
            &visible_types,
            &visible_engaged_converse,
            None,
            0,
        );

        let proposal =
            seek_social_initiation(LONELY_DRIVE, &inputs, 0.0).expect("should propose someone");
        assert_eq!(
            proposal.action.target_entity,
            Some(far_friend),
            "affection should outweigh a few extra tiles of distance"
        );
    }
}
