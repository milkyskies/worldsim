//! Per-tick agent state logger: emits JSONL lines carrying field values resolved
//! from the ECS component tree.
//!
//! Reads: CLI-selected fields (via `FieldLoggerConfig`), every agent component referenced by a field path
//! Writes: `FieldLoggerBuffer` (collected JSONL lines + per-agent state)
//! Upstream: cli::CliArgs (via HeadlessConfig), headless::run_headless
//! Downstream: JSONL file / stderr / stdout, optional CSV post-processing

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use bevy::prelude::*;
use serde_json::{Map, Value, json};

use crate::agent::Agent;
use crate::agent::actions::{
    ActionRegistry, ActionState, ActiveActions, Channel, ChannelCapacities,
};
use crate::agent::biology::body::{Body, TagChannelMapping};
use crate::agent::body::contributions::{Contribution, ContributionKind, compute_contributions};
use crate::agent::body::needs::{Consciousness, PhysicalNeeds};
use crate::agent::body::species::{Species, SpeciesProfile};
use crate::agent::brains::history::BrainHistory;
use crate::agent::brains::plan_memory::{HeldPlan, PlanMemory, PlanState};
use crate::agent::brains::proposal::BrainState;
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value as MindValue};
use crate::agent::nervous_system::cns::CentralNervousSystem;
use crate::agent::nervous_system::urgency::UrgencySource;
use crate::agent::psyche::emotions::EmotionalState;
use crate::core::tick::TickCount;

// ============================================================================
// CONFIG TYPES
// ============================================================================

/// Where to send JSONL output. Default is stderr (matches existing debug tools).
#[derive(Debug, Clone)]
pub enum FieldLoggerOutput {
    Stderr,
    Stdout,
    File(PathBuf),
}

/// Which agents should be logged. `FieldLoggerConfig` may hold multiple of these
/// (repeatable CLI flag); any match wins.
#[derive(Debug, Clone)]
pub enum AgentSelector {
    /// Every agent in the world.
    All,
    /// All agents whose [`SpeciesProfile::species`] matches.
    Species(Species),
    /// Substring match against the agent's Name (case-insensitive).
    NamePattern(String),
    /// Exact match on name or Bevy entity id (e.g. `"Alice"` or `"19v0"`).
    Literal(String),
}

/// One path to resolve each tick, plus any per-field output modifiers.
#[derive(Debug, Clone)]
pub struct FieldSpec {
    pub path: String,
    pub delta: bool,
    /// When true, emit a `"<path>_why"` sibling containing the causal
    /// breakdown for this metric. Only valid for the four metabolism metrics
    /// that [`compute_contributions`] knows about — other paths ignore it.
    pub why: bool,
}

impl FieldSpec {
    /// Plain path with no modifiers set.
    pub fn path(p: impl Into<String>) -> Self {
        Self {
            path: p.into(),
            delta: false,
            why: false,
        }
    }
}

/// One change-detection rule: emit when this path's value has moved more than
/// `threshold` (or the path's default threshold when `None`) since the last
/// emitted line.
#[derive(Debug, Clone)]
pub struct OnChangeSpec {
    pub path: String,
    pub threshold: Option<f32>,
}

/// Output format for the collected buffer. `Jsonl` writes lines as-is; `Csv`
/// flattens nested objects into dotted-path columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldLoggerFormat {
    Jsonl,
    Csv,
}

/// Full configuration for the per-tick field logger.
#[derive(Resource, Debug, Clone)]
pub struct FieldLoggerConfig {
    pub agents: Vec<AgentSelector>,
    pub fields: Vec<FieldSpec>,
    pub output: FieldLoggerOutput,
    pub format: FieldLoggerFormat,
    pub every: u64,
    pub on_change: Vec<OnChangeSpec>,
}

impl FieldLoggerConfig {
    /// True when there is any work for the collection system to do — at least
    /// one field path and at least one agent selector.
    pub fn is_active(&self) -> bool {
        !self.fields.is_empty() && !self.agents.is_empty()
    }
}

// ============================================================================
// BUFFER & PER-AGENT STATE
// ============================================================================

/// Per-agent memory carried across ticks. Used by the emission decision
/// (`--log-on-change`) and the `:delta` modifier to compare against the last
/// emitted line for that agent.
#[derive(Debug, Default, Clone)]
pub struct AgentLogState {
    pub last_emit_tick: Option<u64>,
    pub last_values: HashMap<String, Value>,
}

/// Buffers JSONL lines produced during a run plus the per-agent state the
/// emission logic needs.
#[derive(Resource, Default)]
pub struct FieldLoggerBuffer {
    pub lines: Vec<String>,
    pub states: HashMap<Entity, AgentLogState>,
}

// ============================================================================
// KNOWN PATHS / PRESETS / WILDCARDS
// ============================================================================

/// Every field path the resolver understands. `--log-list-fields` prints this
/// set, wildcards expand against it, and the `field_resolver_handles_all_dotted_paths`
/// test iterates it.
pub const KNOWN_FIELD_PATHS: &[&str] = &[
    // Needs & vitals
    "needs.aerobic",
    "needs.anaerobic",
    "needs.glucose",
    "needs.stomach",
    "needs.reserves",
    "needs.hunger",
    "needs.hydration",
    "needs.wakefulness",
    "needs.health",
    "consciousness.alertness",
    // Actions & channels
    "actions",
    "actions.primary",
    "channels",
    "channels.locomotion",
    "channels.manipulation",
    "channels.consumption",
    "channels.vocalization",
    "channels.bite",
    "channels.carry",
    "channels.fullbody",
    "channels.focus",
    "channels.awareness",
    // Brain
    "brain.winner",
    "brain.powers",
    "brain.powers.survival",
    "brain.powers.emotional",
    "brain.powers.rational",
    "brain.proposals",
    "brain.proposals.count",
    // CNS
    "cns.urgencies",
    "cns.urgencies.hunger",
    "cns.urgencies.thirst",
    "cns.urgencies.stamina",
    "cns.urgencies.social",
    "cns.urgencies.fun",
    "cns.urgencies.fear",
    "cns.urgencies.pain",
    "cns.urgencies.curiosity",
    "cns.urgencies.territoriality",
    "cns.urgencies.sleepiness",
    "cns.sleep_wake_trigger",
    // Plans
    "plans",
    "plans.executing",
    "plans.count",
    // Mind
    "mind.size",
    // Position & emotions
    "position",
    "emotions.mood",
    "emotions.active",
];

/// Expand a preset name into the list of dotted paths it represents. Returns
/// `None` for unknown presets — callers should surface that as a user error.
pub fn expand_preset(name: &str) -> Option<Vec<&'static str>> {
    match name {
        "vitals" => Some(vec![
            "needs.aerobic",
            "needs.glucose",
            "needs.stomach",
            "needs.reserves",
            "needs.hunger",
            "needs.wakefulness",
            "needs.health",
        ]),
        "actions" => Some(vec!["actions", "channels", "brain.winner"]),
        "brain" => Some(vec![
            "brain.winner",
            "brain.powers",
            "cns.urgencies",
            "plans.executing",
        ]),
        "full" => {
            let mut out = Vec::new();
            out.extend(expand_preset("vitals").unwrap());
            out.extend(expand_preset("actions").unwrap());
            out.extend(expand_preset("brain").unwrap());
            Some(out)
        }
        _ => None,
    }
}

/// Expand a wildcard pattern like `needs.*` or `cns.urgencies.*` into the full
/// set of matching paths from [`KNOWN_FIELD_PATHS`]. A path like `needs.*`
/// matches any known path that starts with `needs.` and has no further dots
/// beyond the wildcard — `needs.glucose` matches but `brain.powers.survival`
/// would not match `brain.*`.
pub fn expand_wildcard(pattern: &str) -> Vec<&'static str> {
    let Some(prefix) = pattern.strip_suffix(".*") else {
        return Vec::new();
    };
    let depth = prefix.matches('.').count() + 2; // prefix.X has `depth` segments
    KNOWN_FIELD_PATHS
        .iter()
        .copied()
        .filter(|p| {
            p.starts_with(prefix)
                && p.len() > prefix.len()
                && p.as_bytes()[prefix.len()] == b'.'
                && p.matches('.').count() == depth - 1
        })
        .collect()
}

// ============================================================================
// PARSING (CLI → Config)
// ============================================================================

/// Parse one `--log-agent` value into an [`AgentSelector`].
pub fn parse_agent_selector(s: &str) -> AgentSelector {
    if s.eq_ignore_ascii_case("all") {
        return AgentSelector::All;
    }
    if let Some(rest) = s.strip_prefix("species:") {
        if let Some(sp) = parse_species(rest) {
            return AgentSelector::Species(sp);
        }
        return AgentSelector::Literal(rest.to_string());
    }
    if let Some(rest) = s.strip_prefix("name:") {
        return AgentSelector::NamePattern(rest.to_string());
    }
    AgentSelector::Literal(s.to_string())
}

fn parse_species(s: &str) -> Option<Species> {
    match s.to_ascii_lowercase().as_str() {
        "human" => Some(Species::Human),
        "deer" => Some(Species::Deer),
        "wolf" => Some(Species::Wolf),
        "rabbit" => Some(Species::Rabbit),
        "bird" => Some(Species::Bird),
        _ => None,
    }
}

/// Parse one `--log-field` value into a [`FieldSpec`]. Recognised modifier
/// suffixes: `:delta` (emit delta-since-last-emission) and `:why` (emit the
/// causal contributor breakdown for metabolism metrics). The `mind.knows:<X>`
/// path uses `:` as a concept separator, not a modifier, and is preserved
/// as-is.
pub fn parse_field_spec(raw: &str) -> Option<FieldSpec> {
    if raw.starts_with("mind.knows:") {
        return Some(FieldSpec {
            path: raw.to_string(),
            delta: false,
            why: false,
        });
    }
    if let Some((path, modifier)) = raw.rsplit_once(':') {
        match modifier {
            "delta" => {
                return Some(FieldSpec {
                    path: path.to_string(),
                    delta: true,
                    why: false,
                });
            }
            "why" => {
                return Some(FieldSpec {
                    path: path.to_string(),
                    delta: false,
                    why: true,
                });
            }
            _ => {}
        }
    }
    Some(FieldSpec {
        path: raw.to_string(),
        delta: false,
        why: false,
    })
}

/// Parse one `--log-on-change` value into an [`OnChangeSpec`]. A `:<float>`
/// suffix sets a per-field threshold override; otherwise the default threshold
/// applies at emission time.
pub fn parse_on_change_spec(raw: &str) -> OnChangeSpec {
    if let Some((path, tail)) = raw.rsplit_once(':')
        && let Ok(threshold) = tail.parse::<f32>()
    {
        return OnChangeSpec {
            path: path.to_string(),
            threshold: Some(threshold),
        };
    }
    OnChangeSpec {
        path: raw.to_string(),
        threshold: None,
    }
}

/// Expand a mixed list of field specs, preset names, and wildcard patterns
/// into the concrete sequence of resolvable field specs. Duplicates are
/// dropped in first-seen order so the output column order is stable.
pub fn expand_fields(explicit: &[String], presets: &[String]) -> Result<Vec<FieldSpec>, String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<FieldSpec> = Vec::new();

    let push = |spec: FieldSpec, seen: &mut HashSet<String>, out: &mut Vec<FieldSpec>| {
        let key = format!(
            "{}{}{}",
            spec.path,
            if spec.delta { ":delta" } else { "" },
            if spec.why { ":why" } else { "" }
        );
        if seen.insert(key) {
            out.push(spec);
        }
    };

    for preset in presets {
        let paths =
            expand_preset(preset).ok_or_else(|| format!("unknown --log-preset: {preset:?}"))?;
        for p in paths {
            push(FieldSpec::path(p), &mut seen, &mut out);
        }
    }

    for raw in explicit {
        if raw.ends_with(".*") {
            for p in expand_wildcard(raw) {
                push(FieldSpec::path(p), &mut seen, &mut out);
            }
            continue;
        }
        if let Some(spec) = parse_field_spec(raw) {
            push(spec, &mut seen, &mut out);
        }
    }

    Ok(out)
}

// ============================================================================
// FIELD RESOLVER
// ============================================================================

/// Resolve a single dotted-path field for a given agent entity, returning
/// `None` when the component or sub-field isn't present.
pub fn resolve_field(world: &World, entity: Entity, path: &str) -> Option<Value> {
    match path {
        // ─── PhysicalNeeds scalars ─────────────────────────────────────────
        "needs.aerobic" => world
            .get::<PhysicalNeeds>(entity)
            .map(|n| json!(n.stamina.aerobic)),
        "needs.anaerobic" => world
            .get::<PhysicalNeeds>(entity)
            .map(|n| json!(n.stamina.anaerobic)),
        "needs.glucose" => world
            .get::<PhysicalNeeds>(entity)
            .map(|n| json!(n.metabolism.glucose)),
        "needs.stomach" => world
            .get::<PhysicalNeeds>(entity)
            .map(|n| json!(n.metabolism.stomach_fullness())),
        "needs.reserves" => world
            .get::<PhysicalNeeds>(entity)
            .map(|n| json!(n.metabolism.reserves)),
        "needs.hunger" => world
            .get::<PhysicalNeeds>(entity)
            .map(|n| json!(n.hunger_urgency())),
        "needs.hydration" => world
            .get::<PhysicalNeeds>(entity)
            .map(|n| json!(n.hydration)),
        "needs.wakefulness" => world
            .get::<PhysicalNeeds>(entity)
            .map(|n| json!(n.wakefulness)),
        "needs.health" => Some(json!(
            world
                .get::<Body>(entity)
                .map_or(1.0, |b| b.overall_health())
                * 100.0
        )),
        "consciousness.alertness" => world
            .get::<Consciousness>(entity)
            .map(|c| json!(c.alertness)),

        // ─── Actions ───────────────────────────────────────────────────────
        "actions" => Some(actions_value(world, entity)),
        "actions.primary" => Some(actions_primary_value(world, entity)),

        // ─── Brain ─────────────────────────────────────────────────────────
        "brain.winner" => world.get::<BrainState>(entity).map(|b| match b.winner {
            Some(w) => json!(w.display_name()),
            None => Value::Null,
        }),
        "brain.powers" => world.get::<BrainState>(entity).map(|b| {
            json!({
                "survival": b.powers.survival,
                "emotional": b.powers.emotional,
                "rational": b.powers.rational,
            })
        }),
        "brain.powers.survival" => world
            .get::<BrainState>(entity)
            .map(|b| json!(b.powers.survival)),
        "brain.powers.emotional" => world
            .get::<BrainState>(entity)
            .map(|b| json!(b.powers.emotional)),
        "brain.powers.rational" => world
            .get::<BrainState>(entity)
            .map(|b| json!(b.powers.rational)),
        "brain.proposals" => world.get::<BrainState>(entity).map(|b| {
            Value::Array(
                b.proposals
                    .iter()
                    .map(|p| {
                        json!({
                            "brain": p.brain.display_name(),
                            "action": format!("{:?}", p.action.action_type),
                            "urgency": p.urgency,
                            "reason": p.reasoning,
                        })
                    })
                    .collect(),
            )
        }),
        "brain.proposals.count" => world
            .get::<BrainState>(entity)
            .map(|b| json!(b.proposals.len())),

        // ─── CNS ───────────────────────────────────────────────────────────
        "cns.urgencies" => world.get::<CentralNervousSystem>(entity).map(|cns| {
            Value::Array(
                cns.urgencies
                    .iter()
                    .take(5)
                    .map(|u| {
                        json!({
                            "source": format!("{:?}", u.source),
                            "value": u.value,
                        })
                    })
                    .collect(),
            )
        }),
        "cns.sleep_wake_trigger" => world
            .get::<CentralNervousSystem>(entity)
            .map(|cns| match cns.sleep_wake_trigger {
                Some(s) => json!(format!("{:?}", s)),
                None => Value::Null,
            }),

        // ─── Plans ─────────────────────────────────────────────────────────
        "plans" => world
            .get::<PlanMemory>(entity)
            .map(|m| Value::Array(m.plans.iter().map(plan_to_json).collect())),
        "plans.executing" => world.get::<PlanMemory>(entity).map(|m| {
            Value::Array(
                m.plans
                    .iter()
                    .filter(|p| p.state == PlanState::Executing)
                    .map(plan_to_json)
                    .collect(),
            )
        }),
        "plans.count" => world
            .get::<PlanMemory>(entity)
            .map(|m| json!(m.plans.len())),

        // ─── Mind ──────────────────────────────────────────────────────────
        "mind.size" => world.get::<MindGraph>(entity).map(|g| json!(g.len())),
        p if p.starts_with("mind.knows:") => {
            let concept_name = &p["mind.knows:".len()..];
            let concept = concept_by_name(concept_name)?;
            let mind = world.get::<MindGraph>(entity)?;
            let known = !mind
                .query(
                    Some(&Node::Self_),
                    Some(Predicate::Knows),
                    Some(&MindValue::Concept(concept)),
                )
                .is_empty()
                || !mind
                    .query(
                        None,
                        Some(Predicate::IsA),
                        Some(&MindValue::Concept(concept)),
                    )
                    .is_empty();
            Some(json!(known))
        }

        // ─── Position & Emotions ──────────────────────────────────────────
        "position" => world.get::<Transform>(entity).map(|t| {
            let p = t.translation.truncate();
            json!([p.x, p.y])
        }),
        "emotions.mood" => world
            .get::<EmotionalState>(entity)
            .map(|e| json!(e.current_mood)),
        "emotions.active" => world.get::<EmotionalState>(entity).map(|e| {
            Value::Array(
                e.active_emotions
                    .iter()
                    .map(|em| {
                        json!({
                            "type": format!("{:?}", em.emotion_type),
                            "intensity": em.intensity,
                            "fuel": em.fuel,
                        })
                    })
                    .collect(),
            )
        }),

        // ─── Channels (plural + per-channel) ───────────────────────────────
        "channels" => Some(channels_value(world, entity, None)),
        p if p.starts_with("channels.") => {
            let name = &p["channels.".len()..];
            let ch = channel_by_lowercase_name(name)?;
            Some(channels_value(world, entity, Some(ch)))
        }

        // ─── CNS urgencies per source ─────────────────────────────────────
        p if p.starts_with("cns.urgencies.") => {
            let name = &p["cns.urgencies.".len()..];
            let source = urgency_source_by_lowercase_name(name)?;
            let cns = world.get::<CentralNervousSystem>(entity)?;
            let value = cns
                .urgencies
                .iter()
                .find(|u| u.source == source)
                .map(|u| u.value)
                .unwrap_or(0.0);
            Some(json!(value))
        }

        _ => None,
    }
}

fn plan_to_json(plan: &HeldPlan) -> Value {
    let steps: Vec<String> = plan
        .steps
        .iter()
        .map(|s| format!("{:?}", s.action_type))
        .collect();
    json!({
        "id": plan.id.0,
        "state": format!("{:?}", plan.state),
        "step": plan.current_step,
        "steps": steps,
        "commitment": plan.commitment,
        "priority": plan.goal.priority,
    })
}

fn actions_primary_value(world: &World, entity: Entity) -> Value {
    let Some(active) = world.get::<ActiveActions>(entity) else {
        return Value::Null;
    };
    let Some(registry) = world.get_resource::<ActionRegistry>() else {
        return Value::Null;
    };
    let Some(state) = active.primary(registry) else {
        return Value::Null;
    };
    one_action_json(world, entity, state)
}

fn actions_value(world: &World, entity: Entity) -> Value {
    let Some(active) = world.get::<ActiveActions>(entity) else {
        return Value::Null;
    };
    let actions: Vec<Value> = active
        .iter()
        .map(|state| one_action_json(world, entity, state))
        .collect();
    Value::Array(actions)
}

fn one_action_json(world: &World, entity: Entity, state: &ActionState) -> Value {
    let brain = world
        .get::<BrainHistory>(entity)
        .and_then(|h| h.active.get(&state.action_type).copied())
        .map(|b| b.display_name())
        .unwrap_or("?");
    let reason = world
        .get::<BrainState>(entity)
        .and_then(|bs| {
            bs.proposals
                .iter()
                .find(|p| p.action.action_type == state.action_type)
                .map(|p| p.reasoning.clone())
        })
        .unwrap_or_default();
    let target = state.target_entity.and_then(|e| {
        world
            .get::<Name>(e)
            .map(|n| n.as_str().to_string())
            .or_else(|| Some(format!("{e:?}")))
    });
    json!({
        "type": format!("{:?}", state.action_type),
        "brain": brain,
        "reason": reason,
        "target": target,
    })
}

fn channels_value(world: &World, entity: Entity, filter: Option<Channel>) -> Value {
    let Some(active) = world.get::<ActiveActions>(entity) else {
        return Value::Null;
    };
    let Some(registry) = world.get_resource::<ActionRegistry>() else {
        return Value::Null;
    };
    let body = world.get::<Body>(entity);
    let physical = world.get::<PhysicalNeeds>(entity);
    let consciousness = world.get::<Consciousness>(entity);
    let default_mapping = TagChannelMapping::default();
    let mapping = world
        .get_resource::<TagChannelMapping>()
        .unwrap_or(&default_mapping);
    let caps = ChannelCapacities::compute(body, physical, consciousness, mapping);

    let mut per_channel: Vec<(Channel, f32, Vec<String>)> =
        Channel::ALL.iter().map(|c| (*c, 0.0, Vec::new())).collect();
    for state in active.iter() {
        let Some(def) = registry.get(state.action_type) else {
            continue;
        };
        for usage in def.body_channels() {
            if let Some(slot) = per_channel.iter_mut().find(|(c, _, _)| *c == usage.channel) {
                slot.1 += usage.intensity;
                slot.2.push(format!("{:?}", state.action_type));
            }
        }
    }

    let to_entry = |(ch, load, holders): &(Channel, f32, Vec<String>)| {
        (
            channel_lowercase_name(*ch).to_string(),
            json!({
                "load": load,
                "cap": caps.get(*ch),
                "holders": holders,
            }),
        )
    };

    match filter {
        None => {
            let mut map = Map::new();
            for entry in &per_channel {
                let (k, v) = to_entry(entry);
                map.insert(k, v);
            }
            Value::Object(map)
        }
        Some(filter_ch) => {
            let entry = per_channel
                .iter()
                .find(|(c, _, _)| *c == filter_ch)
                .unwrap();
            let (_, v) = to_entry(entry);
            v
        }
    }
}

fn channel_lowercase_name(ch: Channel) -> &'static str {
    match ch {
        Channel::Locomotion => "locomotion",
        Channel::Manipulation => "manipulation",
        Channel::Consumption => "consumption",
        Channel::Vocalization => "vocalization",
        Channel::Bite => "bite",
        Channel::Carry => "carry",
        Channel::FullBody => "fullbody",
        Channel::Focus => "focus",
        Channel::Awareness => "awareness",
    }
}

fn channel_by_lowercase_name(s: &str) -> Option<Channel> {
    Channel::ALL
        .iter()
        .copied()
        .find(|c| channel_lowercase_name(*c) == s.to_ascii_lowercase())
}

/// Look up a [`Concept`] by case-insensitive name. Matches the `Debug` name
/// of every variant in the enum. Users pass the identifier as it appears in
/// the knowledge system (`AppleTree`, `BerryBush`, etc.) — case-insensitive.
fn concept_by_name(name: &str) -> Option<Concept> {
    let needle = name.to_ascii_lowercase();
    // One entry per Concept variant. Kept in sync with `Concept` — adding a
    // variant there requires adding it here so `mind.knows:<X>` can resolve it.
    const CONCEPTS: &[Concept] = &[
        Concept::Thing,
        Concept::Physical,
        Concept::Abstract,
        Concept::Person,
        Concept::Animal,
        Concept::Plant,
        Concept::Object,
        Concept::Food,
        Concept::Resource,
        Concept::Apple,
        Concept::AppleTree,
        Concept::Berry,
        Concept::BerryBush,
        Concept::Wood,
        Concept::WoodLog,
        Concept::Water,
        Concept::Stone,
        Concept::StoneNode,
        Concept::Stick,
        Concept::Meat,
        Concept::Corpse,
        Concept::SeveredPart,
        Concept::RottenApple,
        Concept::RottenBerry,
        Concept::Campfire,
        Concept::LeanTo,
        Concept::Ash,
        Concept::ConstructionSite,
        Concept::Safety,
        Concept::Warmth,
        Concept::Light,
        Concept::LargeLeaves,
        Concept::Deer,
        Concept::Wolf,
        Concept::Howl,
        Concept::AlarmCall,
        Concept::Scream,
        Concept::CombatSound,
        Concept::Edible,
        Concept::Drinkable,
        Concept::Grazable,
        Concept::Prey,
        Concept::Territory,
        Concept::Dangerous,
        Concept::Safe,
        Concept::Friendly,
        Concept::Hostile,
        Concept::Neutral,
        Concept::Sentient,
        Concept::Harvestable,
        Concept::Awake,
        Concept::Asleep,
        Concept::Unreachable,
        Concept::LightEmitting,
        Concept::HeatEmitting,
        Concept::ShelterProviding,
        Concept::Flammable,
        Concept::FuelConsuming,
        Concept::Degradable,
        Concept::ManMade,
        Concept::SocialAction,
        Concept::ViolentAction,
        Concept::SurvivalAction,
        Concept::MovementAction,
        Concept::HappyMood,
        Concept::SadMood,
        Concept::AngryMood,
        Concept::FearfulMood,
        Concept::NeutralMood,
        Concept::Stranger,
        Concept::Acquaintance,
        Concept::Friend,
        Concept::Rival,
        Concept::Enemy,
    ];
    CONCEPTS
        .iter()
        .copied()
        .find(|c| format!("{c:?}").to_ascii_lowercase() == needle)
}

fn urgency_source_by_lowercase_name(s: &str) -> Option<UrgencySource> {
    match s.to_ascii_lowercase().as_str() {
        "hunger" => Some(UrgencySource::Hunger),
        "thirst" => Some(UrgencySource::Thirst),
        "stamina" => Some(UrgencySource::Stamina),
        "social" => Some(UrgencySource::Social),
        "fun" => Some(UrgencySource::Fun),
        "fear" => Some(UrgencySource::Fear),
        "pain" => Some(UrgencySource::Pain),
        "curiosity" => Some(UrgencySource::Curiosity),
        "territoriality" => Some(UrgencySource::Territoriality),
        "sleepiness" => Some(UrgencySource::Sleepiness),
        _ => None,
    }
}

// ============================================================================
// CHANGE DETECTION
// ============================================================================

/// Default change-detection threshold for a given dotted path. Normalized [0,1]
/// metrics use 0.05; raw stats (glucose, aerobic, hydration, stomach, reserves)
/// use 1.0. List/struct fields fall through to structural equality so the
/// threshold is unused — returned as 0.0 to signal "no numeric window".
pub fn default_change_threshold(path: &str) -> f32 {
    match path {
        "needs.hunger"
        | "needs.wakefulness"
        | "consciousness.alertness"
        | "emotions.mood"
        | "brain.powers.survival"
        | "brain.powers.emotional"
        | "brain.powers.rational" => 0.05,
        p if p.starts_with("cns.urgencies.") => 0.05,
        "needs.aerobic" | "needs.anaerobic" | "needs.glucose" | "needs.stomach"
        | "needs.reserves" | "needs.hydration" | "needs.health" => 1.0,
        _ => 0.0,
    }
}

/// Returns true if `current` differs from `previous` beyond the threshold for
/// this path. Numeric leaves use the threshold; nested / list values use exact
/// structural equality. A missing `previous` always counts as changed.
pub fn value_changed(
    current: Option<&Value>,
    previous: Option<&Value>,
    threshold_override: Option<f32>,
    path: &str,
) -> bool {
    let Some(current) = current else { return false };
    let Some(previous) = previous else {
        return true;
    };
    if current == previous {
        return false;
    }
    let (Some(a), Some(b)) = (current.as_f64(), previous.as_f64()) else {
        // Non-scalar → any structural difference already triggered above.
        return true;
    };
    let threshold = threshold_override.unwrap_or_else(|| default_change_threshold(path)) as f64;
    if threshold <= 0.0 {
        // Structural equality path — any numeric difference counts.
        return (a - b).abs() > 0.0;
    }
    (a - b).abs() >= threshold
}

// ============================================================================
// EMISSION DECISION
// ============================================================================

/// Decide whether to emit a line this tick for a given agent given their
/// previous state and the current field values. Implements the OR rule
/// documented in issue #490: `--log-every` heartbeat OR `--log-on-change`
/// change detection.
pub fn decide_emit(
    tick: u64,
    state: &AgentLogState,
    config: &FieldLoggerConfig,
    values: &HashMap<String, Value>,
) -> bool {
    let has_every = config.every > 1;
    let has_on_change = !config.on_change.is_empty();

    if !has_every && !has_on_change {
        return true; // default: every tick
    }

    let heartbeat_hit = has_every && tick > 0 && tick.is_multiple_of(config.every);
    let change_hit = has_on_change
        && config.on_change.iter().any(|spec| {
            value_changed(
                values.get(&spec.path),
                state.last_values.get(&spec.path),
                spec.threshold,
                &spec.path,
            )
        });

    match (has_every, has_on_change) {
        (true, false) => heartbeat_hit,
        (false, true) => change_hit,
        (true, true) => heartbeat_hit || change_hit,
        (false, false) => unreachable!(),
    }
}

// ============================================================================
// LINE BUILDER (nest flat path → JSON object)
// ============================================================================

/// Build a single JSONL line for one agent/tick. Flat dotted paths are folded
/// into a nested object so `needs.glucose` lands at `.needs.glucose`, while
/// compound paths like `actions` carry their resolved structure as-is.
pub fn build_line(
    tick: u64,
    agent_name: &str,
    entity: Entity,
    values: &HashMap<String, Value>,
    why_values: &HashMap<String, Value>,
    config: &FieldLoggerConfig,
    state: &AgentLogState,
) -> String {
    let mut root = Map::new();
    root.insert("tick".to_string(), json!(tick));
    root.insert("agent".to_string(), json!(agent_name));
    root.insert("agent_id".to_string(), json!(format!("{entity:?}")));

    // Build a stable ordering by walking `config.fields`, so columns in the
    // line match the user's flag order — reproducible for CSV export.
    let mut fields_root = Map::new();
    for spec in &config.fields {
        let Some(value) = values.get(&spec.path) else {
            continue;
        };
        insert_dotted(&mut fields_root, &spec.path, value.clone());
        if spec.delta {
            let delta = delta_value(spec, values, state);
            let delta_path = format!("{}_delta", spec.path);
            insert_dotted(&mut fields_root, &delta_path, delta);
        }
        if spec.why
            && let Some(breakdown) = why_values.get(&spec.path)
        {
            let why_path = format!("{}_why", spec.path);
            insert_dotted(&mut fields_root, &why_path, breakdown.clone());
        }
    }
    for (k, v) in fields_root {
        root.insert(k, v);
    }

    serde_json::to_string(&Value::Object(root)).unwrap_or_else(|_| "{}".to_string())
}

/// Map a field path to the [`ContributionKind`] whose breakdown explains it,
/// or `None` when `:why` doesn't apply.
pub fn contribution_kind_for_path(path: &str) -> Option<ContributionKind> {
    match path {
        "needs.glucose" => Some(ContributionKind::Glucose),
        "needs.aerobic" => Some(ContributionKind::Stamina),
        "needs.hydration" => Some(ContributionKind::Hydration),
        "needs.stomach" => Some(ContributionKind::Stomach),
        _ => None,
    }
}

/// Serialize a contribution list to a JSON value suitable for inlining as a
/// `<path>_why` sibling.
pub fn contributions_to_json(contribs: &[Contribution]) -> Value {
    let net: f32 = contribs.iter().map(|c| c.rate).sum();
    json!({
        "contributors": contribs
            .iter()
            .map(|c| json!({ "source": c.source, "rate": c.rate }))
            .collect::<Vec<_>>(),
        "net": net,
    })
}

fn delta_value(spec: &FieldSpec, values: &HashMap<String, Value>, state: &AgentLogState) -> Value {
    let current = values.get(&spec.path).and_then(Value::as_f64);
    let previous = state.last_values.get(&spec.path).and_then(Value::as_f64);
    match (current, previous) {
        (Some(a), Some(b)) => json!(a - b),
        _ => Value::Null,
    }
}

fn insert_dotted(root: &mut Map<String, Value>, path: &str, value: Value) {
    let segments: Vec<&str> = path.split('.').collect();
    if segments.len() == 1 {
        root.insert(segments[0].to_string(), value);
        return;
    }
    let mut cursor = root;
    for seg in &segments[..segments.len() - 1] {
        let entry = cursor
            .entry(seg.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        cursor = entry.as_object_mut().unwrap();
    }
    cursor.insert(segments.last().unwrap().to_string(), value);
}

// ============================================================================
// COLLECTION SYSTEM
// ============================================================================

/// Exclusive system that evaluates all configured fields for all matched
/// agents each tick and appends emitted lines to [`FieldLoggerBuffer`].
pub fn collect_field_log(world: &mut World) {
    let Some(config) = world.get_resource::<FieldLoggerConfig>().cloned() else {
        return;
    };
    if !config.is_active() {
        return;
    }
    let tick = world.resource::<TickCount>().current;

    let matched = matched_agents(world, &config.agents);
    if matched.is_empty() {
        return;
    }

    // Phase 1: resolve values (and optional :why breakdowns) for every matched
    // agent. Only the immutable world borrow is needed here.
    let mut resolved: Vec<(
        Entity,
        String,
        HashMap<String, Value>,
        HashMap<String, Value>,
    )> = Vec::new();
    for (entity, name) in &matched {
        let mut values = HashMap::new();
        let mut why_values = HashMap::new();
        for spec in &config.fields {
            if let Some(v) = resolve_field(world, *entity, &spec.path) {
                values.insert(spec.path.clone(), v);
            }
            if spec.why
                && let Some(kind) = contribution_kind_for_path(&spec.path)
            {
                let contribs = compute_contributions(world, *entity, kind);
                why_values.insert(spec.path.clone(), contributions_to_json(&contribs));
            }
        }
        resolved.push((*entity, name.clone(), values, why_values));
    }

    // Phase 2: decide emission + build lines + update state (mutable borrow).
    let mut buffer = world.resource_mut::<FieldLoggerBuffer>();
    let mut lines_to_push = Vec::new();
    for (entity, name, values, why_values) in resolved {
        let state = buffer.states.entry(entity).or_default();
        if !decide_emit(tick, state, &config, &values) {
            continue;
        }
        let line = build_line(tick, &name, entity, &values, &why_values, &config, state);
        state.last_emit_tick = Some(tick);
        state.last_values = values;
        lines_to_push.push(line);
    }
    buffer.lines.extend(lines_to_push);
}

/// Snapshot of (entity, display-name) pairs matching the selector set.
fn matched_agents(world: &mut World, selectors: &[AgentSelector]) -> Vec<(Entity, String)> {
    let mut query =
        world.query_filtered::<(Entity, Option<&Name>, Option<&SpeciesProfile>), With<Agent>>();
    let mut out = Vec::new();
    for (entity, name, profile) in query.iter(world) {
        let display = name
            .map(|n| n.as_str().to_string())
            .unwrap_or_else(|| format!("{entity:?}"));
        let species = profile.map(|p| p.species);
        if agent_matches(selectors, entity, &display, species) {
            out.push((entity, display));
        }
    }
    out
}

fn agent_matches(
    selectors: &[AgentSelector],
    entity: Entity,
    name: &str,
    species: Option<Species>,
) -> bool {
    selectors.iter().any(|sel| match sel {
        AgentSelector::All => true,
        AgentSelector::Species(s) => species == Some(*s),
        AgentSelector::NamePattern(p) => {
            name.to_ascii_lowercase().contains(&p.to_ascii_lowercase())
        }
        AgentSelector::Literal(s) => {
            name.eq_ignore_ascii_case(s) || format!("{entity:?}").eq_ignore_ascii_case(s)
        }
    })
}

// ============================================================================
// OUTPUT (JSONL + CSV dump)
// ============================================================================

/// Write the collected buffer to the configured output. When `format` is
/// `Csv`, the JSONL buffer is post-processed into a flat CSV whose header is
/// the union of all dotted leaf paths seen in the buffer, in first-seen order.
pub fn dump_field_log(buffer: &FieldLoggerBuffer, config: &FieldLoggerConfig) {
    if buffer.lines.is_empty() {
        return;
    }
    let payload = match config.format {
        FieldLoggerFormat::Jsonl => buffer.lines.join("\n") + "\n",
        FieldLoggerFormat::Csv => match jsonl_to_csv(&buffer.lines) {
            Ok(csv) => csv,
            Err(e) => {
                eprintln!("field-log: csv conversion failed: {e}");
                return;
            }
        },
    };
    match &config.output {
        FieldLoggerOutput::Stderr => {
            eprint!("{payload}");
        }
        FieldLoggerOutput::Stdout => {
            print!("{payload}");
        }
        FieldLoggerOutput::File(path) => {
            if let Err(e) = std::fs::write(path, payload) {
                eprintln!("field-log: could not write {}: {e}", path.display());
            }
        }
    }
}

/// Convert a buffer of JSONL lines into a CSV string. Nested objects are
/// flattened into dotted-path column names; arrays are serialized back to JSON
/// strings so the CSV stays a rectangular grid.
pub fn jsonl_to_csv(lines: &[String]) -> Result<String, String> {
    let mut headers: Vec<String> = Vec::new();
    let mut rows: Vec<BTreeMap<String, String>> = Vec::new();

    for line in lines {
        let value: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let mut flat: BTreeMap<String, String> = BTreeMap::new();
        flatten_for_csv("", &value, &mut flat);
        for key in flat.keys() {
            if !headers.contains(key) {
                headers.push(key.clone());
            }
        }
        rows.push(flat);
    }

    let mut out = String::new();
    out.push_str(&headers.join(","));
    out.push('\n');
    for row in &rows {
        let values: Vec<String> = headers
            .iter()
            .map(|h| row.get(h).cloned().unwrap_or_default())
            .map(csv_escape)
            .collect();
        out.push_str(&values.join(","));
        out.push('\n');
    }
    Ok(out)
}

fn flatten_for_csv(prefix: &str, value: &Value, out: &mut BTreeMap<String, String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let child = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten_for_csv(&child, v, out);
            }
        }
        Value::Array(_) => {
            out.insert(prefix.to_string(), value.to_string());
        }
        Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        Value::Number(n) => {
            out.insert(prefix.to_string(), n.to_string());
        }
        Value::Bool(b) => {
            out.insert(prefix.to_string(), b.to_string());
        }
        Value::Null => {
            out.insert(prefix.to_string(), String::new());
        }
    }
}

fn csv_escape(s: String) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

// ============================================================================
// HELPERS
// ============================================================================

/// Print the fully expanded field list (after preset/wildcard expansion) to
/// stdout and return. Used by `--log-list-fields` for dry-run inspection.
pub fn print_expanded_field_list(fields: &[FieldSpec]) {
    for spec in fields {
        if spec.delta {
            println!("{}:delta", spec.path);
        } else {
            println!("{}", spec.path);
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestWorld;

    fn cfg_with(fields: Vec<&str>) -> FieldLoggerConfig {
        FieldLoggerConfig {
            agents: vec![AgentSelector::All],
            fields: fields.into_iter().map(FieldSpec::path).collect(),
            output: FieldLoggerOutput::Stderr,
            format: FieldLoggerFormat::Jsonl,
            every: 1,
            on_change: Vec::new(),
        }
    }

    fn human_test_world() -> TestWorld {
        use crate::testing::AgentConfig;
        let mut world = TestWorld::with_seed(7);
        world.spawn_agent(AgentConfig {
            pos: bevy::math::Vec2::new(50.0, 50.0),
            name: Some("Alice".to_string()),
            ..AgentConfig::default()
        });
        world
    }

    fn alice(world: &mut TestWorld) -> Entity {
        world.find_agent("Alice").expect("Alice should exist")
    }

    // ─── Resolver ─────────────────────────────────────────────────────────

    #[test]
    fn field_resolver_handles_all_dotted_paths() {
        let mut world = human_test_world();
        world.tick(1);
        let entity = alice(&mut world);
        let ecs = world.app().world();
        for path in KNOWN_FIELD_PATHS {
            let resolved = resolve_field(ecs, entity, path);
            assert!(
                resolved.is_some(),
                "resolver returned None for documented path {path:?}"
            );
        }
    }

    #[test]
    fn unknown_paths_return_none() {
        let mut world = human_test_world();
        world.tick(1);
        let entity = alice(&mut world);
        let ecs = world.app().world();
        assert!(resolve_field(ecs, entity, "does.not.exist").is_none());
        assert!(resolve_field(ecs, entity, "channels.jetpack").is_none());
    }

    // ─── Presets & wildcards ─────────────────────────────────────────────

    #[test]
    fn preset_vitals_expands_to_expected_fields() {
        let paths = expand_preset("vitals").unwrap();
        assert_eq!(
            paths,
            vec![
                "needs.aerobic",
                "needs.glucose",
                "needs.stomach",
                "needs.reserves",
                "needs.hunger",
                "needs.wakefulness",
                "needs.health",
            ]
        );
    }

    #[test]
    fn preset_full_is_union_of_other_presets() {
        let full = expand_preset("full").unwrap();
        for preset in ["vitals", "actions", "brain"] {
            for p in expand_preset(preset).unwrap() {
                assert!(full.contains(&p), "full missing {p} from {preset}");
            }
        }
    }

    #[test]
    fn wildcard_needs_star_expands_to_all_children() {
        let paths = expand_wildcard("needs.*");
        for expected in [
            "needs.aerobic",
            "needs.anaerobic",
            "needs.glucose",
            "needs.stomach",
            "needs.reserves",
            "needs.hunger",
            "needs.hydration",
            "needs.wakefulness",
            "needs.health",
        ] {
            assert!(paths.contains(&expected), "missing {expected}");
        }
        // Wildcard must not reach into deeper namespaces like brain.powers.
        assert!(!paths.iter().any(|p| p.starts_with("brain.")));
    }

    #[test]
    fn wildcard_urgencies_star_expands_to_each_source() {
        let paths = expand_wildcard("cns.urgencies.*");
        assert!(paths.contains(&"cns.urgencies.hunger"));
        assert!(paths.contains(&"cns.urgencies.sleepiness"));
    }

    // ─── Flag parsing ────────────────────────────────────────────────────

    #[test]
    fn parse_agent_selector_handles_all_species_and_literal() {
        assert!(matches!(parse_agent_selector("all"), AgentSelector::All));
        assert!(matches!(
            parse_agent_selector("species:Human"),
            AgentSelector::Species(Species::Human)
        ));
        assert!(matches!(
            parse_agent_selector("name:alic"),
            AgentSelector::NamePattern(_)
        ));
        assert!(matches!(
            parse_agent_selector("Alice"),
            AgentSelector::Literal(_)
        ));
    }

    #[test]
    fn parse_field_spec_recognizes_delta_modifier() {
        let spec = parse_field_spec("needs.glucose:delta").unwrap();
        assert_eq!(spec.path, "needs.glucose");
        assert!(spec.delta);
        let spec = parse_field_spec("needs.glucose").unwrap();
        assert!(!spec.delta);
    }

    #[test]
    fn parse_on_change_spec_accepts_threshold_override() {
        let spec = parse_on_change_spec("needs.aerobic:2.0");
        assert_eq!(spec.path, "needs.aerobic");
        assert_eq!(spec.threshold, Some(2.0));
        let spec = parse_on_change_spec("needs.aerobic");
        assert!(spec.threshold.is_none());
    }

    // ─── Change thresholds ───────────────────────────────────────────────

    #[test]
    fn change_threshold_defaults_and_overrides() {
        // Normalized field: 0.05 window
        assert!(!value_changed(
            Some(&json!(0.5)),
            Some(&json!(0.52)),
            None,
            "needs.hunger",
        ));
        assert!(value_changed(
            Some(&json!(0.5)),
            Some(&json!(0.6)),
            None,
            "needs.hunger",
        ));
        // Raw stat: 1.0 window
        assert!(!value_changed(
            Some(&json!(100.0)),
            Some(&json!(100.5)),
            None,
            "needs.glucose",
        ));
        assert!(value_changed(
            Some(&json!(100.0)),
            Some(&json!(98.0)),
            None,
            "needs.glucose",
        ));
        // Override beats default
        assert!(value_changed(
            Some(&json!(10.0)),
            Some(&json!(8.0)),
            Some(1.0),
            "needs.aerobic",
        ));
        assert!(!value_changed(
            Some(&json!(10.0)),
            Some(&json!(8.0)),
            Some(5.0),
            "needs.aerobic",
        ));
    }

    // ─── Emission rules ──────────────────────────────────────────────────

    #[test]
    fn log_every_n_samples_correctly() {
        let config = FieldLoggerConfig {
            every: 10,
            ..cfg_with(vec!["needs.glucose"])
        };
        let state = AgentLogState::default();
        let values = HashMap::from([("needs.glucose".to_string(), json!(100.0))]);
        assert!(!decide_emit(1, &state, &config, &values));
        assert!(decide_emit(10, &state, &config, &values));
        assert!(decide_emit(20, &state, &config, &values));
        assert!(!decide_emit(25, &state, &config, &values));
    }

    #[test]
    fn log_on_change_suppresses_unchanged_ticks() {
        let config = FieldLoggerConfig {
            on_change: vec![OnChangeSpec {
                path: "needs.glucose".to_string(),
                threshold: None,
            }],
            ..cfg_with(vec!["needs.glucose"])
        };
        let mut state = AgentLogState::default();
        state
            .last_values
            .insert("needs.glucose".to_string(), json!(100.0));
        let same = HashMap::from([("needs.glucose".to_string(), json!(100.5))]);
        let diff = HashMap::from([("needs.glucose".to_string(), json!(95.0))]);
        assert!(!decide_emit(42, &state, &config, &same));
        assert!(decide_emit(42, &state, &config, &diff));
    }

    #[test]
    fn log_every_and_on_change_together_use_or_rule() {
        let config = FieldLoggerConfig {
            every: 10,
            on_change: vec![OnChangeSpec {
                path: "needs.glucose".to_string(),
                threshold: None,
            }],
            ..cfg_with(vec!["needs.glucose"])
        };
        let mut state = AgentLogState::default();
        state
            .last_values
            .insert("needs.glucose".to_string(), json!(100.0));

        let unchanged = HashMap::from([("needs.glucose".to_string(), json!(100.2))]);
        let changed = HashMap::from([("needs.glucose".to_string(), json!(80.0))]);

        // Tick 5: not a heartbeat, unchanged → no emit.
        assert!(!decide_emit(5, &state, &config, &unchanged));
        // Tick 10: heartbeat hits even though unchanged → emit.
        assert!(decide_emit(10, &state, &config, &unchanged));
        // Tick 7: not heartbeat, but the value changed → emit.
        assert!(decide_emit(7, &state, &config, &changed));
    }

    // ─── Line / delta ────────────────────────────────────────────────────

    #[test]
    fn jsonl_output_is_valid_per_line() {
        let config = cfg_with(vec!["needs.glucose", "needs.hunger", "position", "actions"]);
        let mut world = human_test_world();
        world.app_mut().insert_resource(config.clone());
        world.app_mut().init_resource::<FieldLoggerBuffer>();
        world
            .app_mut()
            .add_systems(bevy::app::Last, collect_field_log);
        world.tick(5);
        let buffer = world.app().world().resource::<FieldLoggerBuffer>();
        assert!(!buffer.lines.is_empty());
        for line in &buffer.lines {
            let v: Value = serde_json::from_str(line).expect("valid json line");
            assert!(v.get("tick").is_some());
            assert!(v.get("agent").is_some());
            assert!(v.get("needs").is_some());
        }
    }

    #[test]
    fn delta_modifier_computes_against_last_emission() {
        let mut state = AgentLogState::default();
        state
            .last_values
            .insert("needs.glucose".to_string(), json!(90.0));
        let values = HashMap::from([("needs.glucose".to_string(), json!(92.5))]);
        let why_values = HashMap::new();
        let config = FieldLoggerConfig {
            fields: vec![FieldSpec {
                path: "needs.glucose".to_string(),
                delta: true,
                why: false,
            }],
            ..cfg_with(vec![])
        };
        let line = build_line(
            42,
            "Alice",
            Entity::from_raw_u32(1).unwrap(),
            &values,
            &why_values,
            &config,
            &state,
        );
        let v: Value = serde_json::from_str(&line).unwrap();
        let delta = v.pointer("/needs/glucose_delta").unwrap().as_f64().unwrap();
        assert!((delta - 2.5).abs() < 1e-6, "delta was {delta}");
    }

    #[test]
    fn delta_is_null_on_first_emission() {
        let state = AgentLogState::default();
        let values = HashMap::from([("needs.glucose".to_string(), json!(100.0))]);
        let why_values = HashMap::new();
        let config = FieldLoggerConfig {
            fields: vec![FieldSpec {
                path: "needs.glucose".to_string(),
                delta: true,
                why: false,
            }],
            ..cfg_with(vec![])
        };
        let line = build_line(
            1,
            "Alice",
            Entity::from_raw_u32(1).unwrap(),
            &values,
            &why_values,
            &config,
            &state,
        );
        let v: Value = serde_json::from_str(&line).unwrap();
        assert!(v.pointer("/needs/glucose_delta").unwrap().is_null());
    }

    #[test]
    fn why_modifier_parses_and_inlines_contributor_breakdown() {
        let spec = parse_field_spec("needs.glucose:why").unwrap();
        assert_eq!(spec.path, "needs.glucose");
        assert!(spec.why);
        assert!(!spec.delta);
        assert_eq!(
            contribution_kind_for_path("needs.glucose"),
            Some(ContributionKind::Glucose)
        );
        assert_eq!(
            contribution_kind_for_path("needs.aerobic"),
            Some(ContributionKind::Stamina)
        );
        assert!(contribution_kind_for_path("needs.hunger").is_none());

        let contribs = vec![
            Contribution {
                source: "BMR".into(),
                rate: -0.5,
            },
            Contribution {
                source: "Walk".into(),
                rate: -0.2,
            },
        ];
        let v = contributions_to_json(&contribs);
        assert!((v["net"].as_f64().unwrap() - -0.7).abs() < 1e-6);
        assert_eq!(v["contributors"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn mind_knows_concept_resolves_to_bool() {
        let mut world = human_test_world();
        world.tick(1);
        let entity = alice(&mut world);
        let ecs = world.app().world();
        let result = resolve_field(ecs, entity, "mind.knows:Apple");
        assert!(result.is_some(), "should return bool, not None");
        assert!(result.unwrap().is_boolean());
    }

    #[test]
    fn mind_knows_parses_as_path_not_modifier() {
        let spec = parse_field_spec("mind.knows:AppleTree").unwrap();
        assert_eq!(spec.path, "mind.knows:AppleTree");
        assert!(!spec.delta);
        assert!(!spec.why);
    }

    // ─── CSV ─────────────────────────────────────────────────────────────

    #[test]
    fn csv_export_flattens_nested_fields() {
        let lines = vec![
            json!({
                "tick": 1,
                "agent": "alice",
                "needs": {"glucose": 100.0, "hunger": 0.3},
            })
            .to_string(),
            json!({
                "tick": 2,
                "agent": "alice",
                "needs": {"glucose": 99.5, "hunger": 0.31},
            })
            .to_string(),
        ];
        let csv = jsonl_to_csv(&lines).unwrap();
        let mut iter = csv.lines();
        let header = iter.next().unwrap();
        assert!(header.contains("needs.glucose"));
        assert!(header.contains("needs.hunger"));
        assert!(header.contains("tick"));
        assert!(header.contains("agent"));
        let row1 = iter.next().unwrap();
        assert!(row1.contains("100"));
        assert!(row1.contains("alice"));
    }

    // ─── Agent selectors ─────────────────────────────────────────────────

    #[test]
    fn log_agent_species_selector_matches_all_of_species() {
        use crate::testing::AgentConfig;
        let mut world = TestWorld::with_seed(11);
        world.spawn_agent(AgentConfig {
            pos: bevy::math::Vec2::new(50.0, 50.0),
            name: Some("Alice".to_string()),
            ..AgentConfig::default()
        });
        world.spawn_agent(AgentConfig {
            pos: bevy::math::Vec2::new(60.0, 50.0),
            name: Some("Bob".to_string()),
            ..AgentConfig::default()
        });
        world.spawn_deer(bevy::math::Vec2::new(70.0, 50.0));
        world.tick(1);

        let selectors = vec![AgentSelector::Species(Species::Human)];
        let matched = matched_agents(world.app_mut().world_mut(), &selectors);

        let names: Vec<String> = matched.iter().map(|(_, n)| n.clone()).collect();
        assert!(names.iter().any(|n| n == "Alice"), "{names:?}");
        assert!(names.iter().any(|n| n == "Bob"), "{names:?}");
        assert!(
            !names.iter().any(|n| n.to_lowercase().contains("deer")),
            "species:Human must not match deer — got {names:?}"
        );
    }

    #[test]
    fn log_agent_literal_matches_by_name_or_entity_id() {
        let mut world = human_test_world();
        world.tick(1);
        let entity = alice(&mut world);
        let id_str = format!("{entity:?}");

        let by_name = matched_agents(
            world.app_mut().world_mut(),
            &[AgentSelector::Literal("Alice".to_string())],
        );
        assert_eq!(by_name.len(), 1);
        let by_id = matched_agents(
            world.app_mut().world_mut(),
            &[AgentSelector::Literal(id_str)],
        );
        assert_eq!(by_id.len(), 1);
    }
}
