//! TestWorld: a Bevy `App` configured with all simulation logic plugins but no rendering or input.
//!
//! Reads: AgentPlugin, agent components, knowledge ontology, world map types
//! Writes: TestWorld (App wrapper exposing spawn/tick/inspect APIs), SimEventLog (auto-collected event history)
//! Upstream: testing::config (AgentConfig), testing::spawn (logic-only spawners)
//! Downstream: integration tests (scenario, brain, knowledge, planner, perception)

use bevy::math::IVec2;
use bevy::prelude::*;

use crate::agent::Agent;
use crate::agent::AgentPlugin;
use crate::agent::actions::{ActionRegistry, ActionType, ActiveActions};
use crate::agent::biology::body::Body;
use crate::agent::body::needs::{Consciousness, PhysicalNeeds, PsychologicalDrives};
use crate::agent::brains::proposal::BrainState;
use crate::agent::events::SimEvent;
use crate::agent::mind::conversation::{ConversationManager, InConversation};
use crate::agent::mind::knowledge::{
    Concept, MindGraph, Node as MindNode, Ontology, Predicate, Value, setup_ontology,
};
use crate::agent::psyche::emotions::EmotionalState;
use crate::core::tick::TickCount;
use crate::core::{GameLog, GameTime};
use crate::testing::config::AgentConfig;
use crate::testing::spawn::{
    spawn_test_apple_tree, spawn_test_berry_bush, spawn_test_deer, spawn_test_person,
    spawn_test_stone_node, spawn_test_wolf, spawn_test_wood_log,
};
use crate::world::environment::LightLevel;
use crate::world::map::{
    CHUNK_SIZE, Chunk, DEFAULT_TERRAIN_SEED, MAP_CHUNKS_X, MAP_CHUNKS_Y, WORLD_HEIGHT, WORLD_WIDTH,
    WorldMap, generate_terrain,
};
use crate::world::spatial_index::SpatialIndexPlugin;
use crate::world::spawn_config::{SpawnLayout, WorldSpawnConfig};

/// Default test world dimensions in tiles. Large enough for typical scenarios but
/// small enough that map construction is cheap (a few KB).
const DEFAULT_MAP_TILES: u32 = 64;

// ─── SimEvent history ──────────────────────────────────────────────────────

/// Resource that accumulates every SimEvent emitted during a TestWorld run.
///
/// Populated automatically by `collect_sim_events_into_log`. Available via
/// `TestWorld::print_recent_events` and `TestWorld::print_agent_events`.
#[derive(Resource, Default)]
pub struct SimEventLog {
    events: Vec<SimEvent>,
}

impl SimEventLog {
    fn push(&mut self, event: SimEvent) {
        self.events.push(event);
    }

    fn events_since(
        &self,
        current_tick: u64,
        last_n_ticks: u64,
    ) -> impl Iterator<Item = &SimEvent> {
        let cutoff = current_tick.saturating_sub(last_n_ticks);
        self.events
            .iter()
            .filter(move |e| sim_event_tick(e) >= cutoff)
    }

    /// Read-only access to all collected events. Tests use this to assert
    /// specific SimEvent variants fired during a run.
    pub fn all(&self) -> &[SimEvent] {
        &self.events
    }
}

/// Bevy system that drains incoming SimEvents into `SimEventLog`.
fn collect_sim_events_into_log(mut reader: MessageReader<SimEvent>, mut log: ResMut<SimEventLog>) {
    for event in reader.read() {
        log.push(event.clone());
    }
}

// ─── Formatting helpers ────────────────────────────────────────────────────

/// Extract the primary tick from any SimEvent variant.
fn sim_event_tick(event: &SimEvent) -> u64 {
    match event {
        SimEvent::Decision { tick, .. }
        | SimEvent::ActionStarted { tick, .. }
        | SimEvent::ActionCompleted { tick, .. }
        | SimEvent::ActionPreempted { tick, .. }
        | SimEvent::ActionFailed { tick, .. }
        | SimEvent::PlanAbandoned { tick, .. }
        | SimEvent::ConversationStarted { tick, .. }
        | SimEvent::ConversationEnded { tick, .. }
        | SimEvent::ConversationJoined { tick, .. }
        | SimEvent::ConversationLeft { tick, .. }
        | SimEvent::ConversationAbandoned { tick, .. }
        | SimEvent::RelationshipChanged { tick, .. }
        | SimEvent::EmotionTriggered { tick, .. }
        | SimEvent::Death { tick, .. }
        | SimEvent::EntityPerceived { tick, .. }
        | SimEvent::StrangerDetected { tick, .. }
        | SimEvent::KnowledgeShared { tick, .. }
        | SimEvent::WarmthPerceived { tick, .. }
        | SimEvent::SoundPerceived { tick, .. }
        | SimEvent::TheoryOfMindUpdated { tick, .. }
        | SimEvent::ItemSpoiled { tick, .. }
        | SimEvent::EffectApplied { tick, .. }
        | SimEvent::LaborContributed { tick, .. }
        | SimEvent::SkillChanged { tick, .. }
        | SimEvent::CombatHit { tick, .. }
        | SimEvent::CombatMissed { tick, .. }
        | SimEvent::PartSevered { tick, .. }
        | SimEvent::PhenotypeDeveloped { tick, .. } => *tick,
    }
}

/// Returns true if this SimEvent involves the given entity as a primary participant.
fn sim_event_involves(event: &SimEvent, agent: Entity) -> bool {
    match event {
        SimEvent::Decision { agent: a, .. }
        | SimEvent::ActionStarted { agent: a, .. }
        | SimEvent::ActionCompleted { agent: a, .. }
        | SimEvent::ActionPreempted { agent: a, .. }
        | SimEvent::ActionFailed { agent: a, .. }
        | SimEvent::PlanAbandoned { agent: a, .. }
        | SimEvent::EmotionTriggered { agent: a, .. }
        | SimEvent::Death { agent: a, .. }
        | SimEvent::EntityPerceived { agent: a, .. }
        | SimEvent::StrangerDetected { agent: a, .. } => *a == agent,

        SimEvent::RelationshipChanged {
            agent: a, other, ..
        } => *a == agent || *other == agent,

        SimEvent::KnowledgeShared {
            speaker, listener, ..
        } => *speaker == agent || *listener == agent,

        SimEvent::ConversationStarted { participants, .. }
        | SimEvent::ConversationEnded { participants, .. } => participants.contains(&agent),

        SimEvent::ConversationJoined { joiner, .. } => *joiner == agent,
        SimEvent::ConversationLeft { leaver, .. } => *leaver == agent,

        SimEvent::ConversationAbandoned {
            abandoner,
            abandoned,
            ..
        } => *abandoner == agent || *abandoned == agent,

        SimEvent::WarmthPerceived { agent: a, .. } | SimEvent::SoundPerceived { agent: a, .. } => {
            *a == agent
        }

        SimEvent::TheoryOfMindUpdated {
            agent: a, about, ..
        } => *a == agent || *about == agent,

        SimEvent::ItemSpoiled { agent: a, .. }
        | SimEvent::EffectApplied { agent: a, .. }
        | SimEvent::LaborContributed { agent: a, .. }
        | SimEvent::SkillChanged { agent: a, .. }
        | SimEvent::PhenotypeDeveloped { agent: a, .. } => *a == agent,

        SimEvent::CombatHit {
            attacker, defender, ..
        } => *attacker == agent || *defender == agent,
        SimEvent::CombatMissed {
            attacker, defender, ..
        } => *attacker == agent || *defender == agent,
        SimEvent::PartSevered { entity, .. } => *entity == agent,
    }
}

/// One-line description of a SimEvent for terminal output.
fn format_sim_event(event: &SimEvent) -> String {
    match event {
        SimEvent::Decision {
            agent,
            tick,
            winner,
            chosen_actions,
            powers,
            ..
        } => format!(
            "[t{tick}] Decision  agent={agent:?} winner={winner:?} actions={chosen_actions:?} \
             powers=(S:{:.2} E:{:.2} R:{:.2})",
            powers.survival, powers.emotional, powers.rational
        ),

        SimEvent::ActionStarted {
            agent,
            tick,
            action,
            target,
        } => {
            if let Some(t) = target {
                format!("[t{tick}] ActionStarted   agent={agent:?} action={action:?} target={t:?}")
            } else {
                format!("[t{tick}] ActionStarted   agent={agent:?} action={action:?}")
            }
        }

        SimEvent::ActionCompleted {
            agent,
            tick,
            action,
            target,
        } => {
            let tgt = target.map(|t| format!(" target={t:?}")).unwrap_or_default();
            format!("[t{tick}] ActionCompleted agent={agent:?} action={action:?}{tgt}")
        }

        SimEvent::ActionPreempted {
            agent,
            tick,
            preempted_action,
        } => {
            format!("[t{tick}] ActionPreempted agent={agent:?} preempted={preempted_action:?}")
        }

        SimEvent::ActionFailed {
            agent,
            tick,
            action,
            reason,
        } => {
            format!("[t{tick}] ActionFailed    agent={agent:?} action={action:?} reason={reason:?}")
        }

        SimEvent::PlanAbandoned {
            agent,
            tick,
            action,
            intent,
        } => {
            format!(
                "[t{tick}] PlanAbandoned    agent={agent:?} action={action:?} intent={intent:?}"
            )
        }

        SimEvent::ConversationStarted {
            participants,
            tick,
            conversation_id,
        } => {
            format!(
                "[t{tick}] ConversationStarted  id={conversation_id} participants={participants:?}"
            )
        }

        SimEvent::ConversationEnded {
            participants,
            tick,
            conversation_id,
        } => {
            format!(
                "[t{tick}] ConversationEnded    id={conversation_id} participants={participants:?}"
            )
        }

        SimEvent::ConversationJoined {
            joiner,
            tick,
            conversation_id,
        } => {
            format!("[t{tick}] ConversationJoined   id={conversation_id} joiner={joiner:?}")
        }

        SimEvent::ConversationLeft {
            leaver,
            tick,
            conversation_id,
        } => {
            format!("[t{tick}] ConversationLeft     id={conversation_id} leaver={leaver:?}")
        }

        SimEvent::ConversationAbandoned {
            abandoner,
            abandoned,
            tick,
        } => {
            format!(
                "[t{tick}] ConversationAbandoned abandoner={abandoner:?} abandoned={abandoned:?}"
            )
        }

        SimEvent::RelationshipChanged {
            agent,
            other,
            tick,
            dimension,
            old_value,
            new_value,
        } => format!(
            "[t{tick}] RelationshipChanged agent={agent:?} other={other:?} \
             dim={dimension:?} {old_value:.3}->{new_value:.3}"
        ),

        SimEvent::EmotionTriggered {
            agent,
            tick,
            emotion,
            intensity,
        } => {
            format!(
                "[t{tick}] EmotionTriggered   agent={agent:?} emotion={emotion:?} \
                 intensity={intensity:.3}"
            )
        }

        SimEvent::Death { agent, tick, cause } => {
            format!("[t{tick}] Death             agent={agent:?} cause={cause}")
        }

        SimEvent::EntityPerceived {
            agent,
            tick,
            target,
        } => {
            format!("[t{tick}] EntityPerceived   agent={agent:?} target={target:?}")
        }

        SimEvent::StrangerDetected {
            agent,
            tick,
            stranger,
        } => {
            format!("[t{tick}] StrangerDetected  agent={agent:?} stranger={stranger:?}")
        }

        SimEvent::KnowledgeShared {
            speaker,
            listener,
            tick,
            triple_count,
        } => {
            format!(
                "[t{tick}] KnowledgeShared   speaker={speaker:?} listener={listener:?} \
                 triples={triple_count}"
            )
        }

        SimEvent::WarmthPerceived {
            agent,
            tick,
            source,
        } => {
            format!("[t{tick}] WarmthPerceived  agent={agent:?} source={source:?}")
        }

        SimEvent::SoundPerceived {
            agent,
            tick,
            source,
            kind,
        } => {
            format!("[t{tick}] SoundPerceived   agent={agent:?} source={source:?} kind={kind:?}")
        }

        SimEvent::TheoryOfMindUpdated {
            agent,
            about,
            tick,
            source,
            belief_count,
        } => {
            format!(
                "[t{tick}] TheoryOfMindUpdated agent={agent:?} about={about:?} \
                 source={source:?} beliefs={belief_count}"
            )
        }

        SimEvent::ItemSpoiled {
            agent,
            tick,
            from,
            to,
        } => {
            format!("[t{tick}] ItemSpoiled    agent={agent:?} {from:?} -> {to:?}")
        }

        SimEvent::EffectApplied {
            agent,
            tick,
            source,
        } => {
            format!("[t{tick}] EffectApplied     agent={agent:?} source={source:?}")
        }

        SimEvent::LaborContributed { agent, tick, site } => {
            format!("[t{tick}] LaborContributed  agent={agent:?} site={site:?}")
        }

        SimEvent::SkillChanged {
            agent,
            tick,
            skill,
            old_value,
            new_value,
        } => {
            format!(
                "[t{tick}] SkillChanged      agent={agent:?} skill={skill:?} \
                 {old_value:.3}->{new_value:.3}"
            )
        }

        SimEvent::CombatHit {
            attacker,
            defender,
            tick,
            part_kind,
            damage,
            injury_type,
        } => {
            format!(
                "[t{tick}] CombatHit         {attacker:?} -> {defender:?} \
                 {} {damage:.1} ({injury_type:?})",
                part_kind.display_name()
            )
        }

        SimEvent::CombatMissed {
            attacker,
            defender,
            tick,
        } => {
            format!("[t{tick}] CombatMissed      {attacker:?} -> {defender:?} (dodged)")
        }

        SimEvent::PartSevered {
            entity,
            tick,
            part_kind,
        } => {
            format!(
                "[t{tick}] PartSevered       {entity:?} lost {}",
                part_kind.display_name()
            )
        }

        SimEvent::PhenotypeDeveloped {
            agent,
            tick,
            phenotype,
        } => {
            format!(
                "[t{tick}] PhenotypeDeveloped agent={agent:?} speed={:.3} \
                 vision={:.3} bmr={:.3} aerobic={:.3}",
                phenotype.speed, phenotype.vision, phenotype.bmr, phenotype.aerobic_capacity,
            )
        }
    }
}

fn format_triple(triple: &crate::agent::mind::knowledge::Triple) -> String {
    format!(
        "{:?} --{:?}--> {:?}",
        triple.subject, triple.predicate, triple.object
    )
}

fn entity_name(world: &World, entity: Entity) -> String {
    world
        .get::<Name>(entity)
        .map(|n| n.as_str().to_string())
        .unwrap_or_else(|| format!("{entity:?}"))
}

fn print_section_header(title: &str, name: &str, entity: Entity, tick: u64) {
    eprintln!("══════════════════════════════════════════════════");
    eprintln!("  {title} — {name} [{entity:?}] at tick {tick}");
    eprintln!("══════════════════════════════════════════════════");
}

fn print_section_footer() {
    eprintln!("──────────────────────────────────────────────────");
}

enum ContributionKind {
    Glucose,
    Stamina,
    Hydration,
    Stomach,
}

fn dump_contributions_headless(
    world: &World,
    agent: Entity,
    label: &str,
    unit: &str,
    kind: ContributionKind,
) {
    use crate::agent::actions::ActionRegistry;
    use crate::agent::activity::{ActivityConfig, CurrentActivity};

    let mut contribs: Vec<(String, f32)> = Vec::new();
    let cfg = world.get_resource::<ActivityConfig>();

    match kind {
        ContributionKind::Glucose => {
            if let Some(cfg) = cfg {
                let bmr = cfg.base.effects.glucose_drain;
                if bmr != 0.0 {
                    contribs.push(("BMR (base metabolic rate)".into(), -bmr));
                }
            }
            if let (Some(active), Some(reg)) = (
                world.get::<ActiveActions>(agent),
                world.get_resource::<ActionRegistry>(),
            ) {
                use crate::agent::actions::registry::ActionKind;
                use crate::agent::body::effort::{self, DEFAULT_BODY_MASS, compute_action_cost};
                use crate::agent::body::species::SpeciesProfile;
                use crate::agent::movement::effective_intensity;

                let body_mass = world
                    .get::<SpeciesProfile>(agent)
                    .map(|s| s.mass_kg)
                    .unwrap_or(DEFAULT_BODY_MASS);
                let stamina = world
                    .get::<PhysicalNeeds>(agent)
                    .map(|p| p.stamina.clone())
                    .unwrap_or_default();

                for state in active.iter() {
                    let Some(action) = reg.get(state.action_type) else {
                        continue;
                    };
                    let primitive = action.motor_primitive();
                    let intensity = if matches!(action.kind(), ActionKind::Movement)
                        && state.locomotion_intensity > 0.0
                    {
                        effective_intensity(state.locomotion_intensity, &stamina)
                    } else {
                        action.default_behavior().intensity.resolve()
                    };
                    let profile = primitive.effort_profile().scaled(intensity);
                    let cost = compute_action_cost(&profile, body_mass);
                    if cost.energy != 0.0 {
                        let reserves = world
                            .get::<PhysicalNeeds>(agent)
                            .map(|p| p.metabolism.reserves)
                            .unwrap_or(0.0);
                        let gluc_frac =
                            effort::effective_glucose_fraction(profile.peak_intensity(), reserves);
                        contribs
                            .push((format!("{:?}", state.action_type), -cost.energy * gluc_frac));
                    }
                }
            }
        }
        ContributionKind::Stamina => {
            if let (Some(active), Some(reg)) = (
                world.get::<ActiveActions>(agent),
                world.get_resource::<ActionRegistry>(),
            ) {
                use crate::agent::actions::registry::ActionKind;
                use crate::agent::body::effort::{DEFAULT_BODY_MASS, compute_action_cost};
                use crate::agent::body::species::SpeciesProfile;
                use crate::agent::movement::effective_intensity;

                let body_mass = world
                    .get::<SpeciesProfile>(agent)
                    .map(|s| s.mass_kg)
                    .unwrap_or(DEFAULT_BODY_MASS);
                let stamina = world
                    .get::<PhysicalNeeds>(agent)
                    .map(|p| p.stamina.clone())
                    .unwrap_or_default();

                for state in active.iter() {
                    let Some(action) = reg.get(state.action_type) else {
                        continue;
                    };
                    let primitive = action.motor_primitive();
                    let intensity = if matches!(action.kind(), ActionKind::Movement)
                        && state.locomotion_intensity > 0.0
                    {
                        effective_intensity(state.locomotion_intensity, &stamina)
                    } else {
                        action.default_behavior().intensity.resolve()
                    };
                    let profile = primitive.effort_profile().scaled(intensity);
                    let cost = compute_action_cost(&profile, body_mass);
                    if cost.aerobic_drain != 0.0 {
                        contribs.push((format!("{:?}", state.action_type), -cost.aerobic_drain));
                    }
                }
            }
        }
        ContributionKind::Hydration => {
            if let Some(cfg) = cfg {
                let base = cfg.base.effects.hydration_change;
                if base != 0.0 {
                    contribs.push(("baseline".into(), base));
                }
                if let Some(activity) = world.get::<CurrentActivity>(agent) {
                    let activity_delta = cfg.get(activity).effects.hydration_change;
                    if activity_delta != 0.0 {
                        contribs.push((format!("{:?}", activity), activity_delta));
                    }
                }
            }
        }
        ContributionKind::Stomach => {
            use crate::agent::body::metabolism::{DIGEST_CARB_RATE, DIGEST_FAT_RATE};
            use crate::agent::body::needs::PhysicalNeeds;
            if let Some(needs) = world.get::<PhysicalNeeds>(agent) {
                let m = &needs.metabolism;
                if m.stomach_carbs > 0.0 {
                    contribs.push((
                        "digestion: carbs → glucose".into(),
                        -DIGEST_CARB_RATE.min(m.stomach_carbs),
                    ));
                }
                if m.stomach_fat > 0.0 {
                    contribs.push((
                        "digestion: fat → reserves".into(),
                        -DIGEST_FAT_RATE.min(m.stomach_fat),
                    ));
                }
            }
            if let (Some(active), Some(reg)) = (
                world.get::<ActiveActions>(agent),
                world.get_resource::<ActionRegistry>(),
            ) {
                for state in active.iter() {
                    let Some(action) = reg.get(state.action_type) else {
                        continue;
                    };
                    let rate = action.runtime_effects().stomach_carbs_per_sec;
                    if rate != 0.0 {
                        contribs.push((format!("{:?}: carbs in", state.action_type), rate));
                    }
                }
            }
        }
    }

    if contribs.is_empty() {
        eprintln!("  (no active contributors to {})", label);
    } else {
        for (src, rate) in &contribs {
            eprintln!("  {:+.2}{}  {}", rate, unit, src);
        }
        let net: f32 = contribs.iter().map(|(_, r)| r).sum();
        eprintln!("  ----");
        eprintln!("  net {:+.2}{}", net, unit);
    }
}

/// A lightweight headless simulation harness. Wraps a Bevy `App` configured with
/// the same logic plugins as the real game (`AgentPlugin` and friends) but
/// without rendering, windowing, input, UI, or world spawn population.
///
/// The seed parameter is captured for forward compatibility with deterministic
/// RNG once the simulation is refactored to use a seeded RNG resource. Today,
/// individual tests should rely on explicit `AgentConfig` values for
/// reproducibility rather than implicit randomness.
pub struct TestWorld {
    app: App,
    seed: u64,
}

impl TestWorld {
    /// Creates a new TestWorld with seed 0.
    pub fn new() -> Self {
        Self::with_seed(0)
    }

    /// Creates a new TestWorld with the given RNG seed.
    pub fn with_seed(seed: u64) -> Self {
        Self::with_seed_and_map(
            seed,
            make_walkable_map(DEFAULT_MAP_TILES, DEFAULT_MAP_TILES),
        )
    }

    /// Creates a TestWorld backed by the 128×128 noise terrain used by the normal
    /// windowed game. Useful when you need realistic biomes for spawn placement
    /// (e.g. settlement detection, forest biomes for apple trees).
    ///
    /// Does **not** populate entities — use [`TestWorld::game_defaults`] for a
    /// fully-populated world, or call [`TestWorld::apply_spawn_layout`] manually.
    pub(crate) fn with_game_map(seed: u64) -> Self {
        Self::with_seed_and_map(seed, make_game_map())
    }

    /// Creates a TestWorld populated with the same algorithm and entity counts
    /// as the normal windowed game launch. Identical to running:
    /// `cargo run -- --headless --game-defaults --seed <seed>`
    ///
    /// Uses the 128×128 noise map and the Realistic placement algorithm
    /// (settlement detection, biome clustering, herd grouping).
    pub fn game_defaults(seed: u64) -> Self {
        let mut world = Self::with_game_map(seed);
        let config = WorldSpawnConfig {
            seed,
            ..WorldSpawnConfig::game_defaults()
        };
        let layout = {
            let map = world.app().world().resource::<WorldMap>();
            config.compute_layout(map)
        };
        world.apply_spawn_layout(&layout);
        world
    }

    /// Creates a new TestWorld with the given seed and a pre-built `WorldMap`.
    /// Used by `ScenarioBuilder::build()` to inject a custom map.
    pub(super) fn with_seed_and_map(seed: u64, map: WorldMap) -> Self {
        let mut app = App::new();

        // MinimalPlugins gives us TaskPool, Time, ScheduleRunner — no rendering.
        app.add_plugins(MinimalPlugins);

        // TransformPlugin runs `propagate_transforms` in PostUpdate so that
        // `GlobalTransform` tracks `Transform`. Without this, every entity's
        // `GlobalTransform` is stuck at the identity (origin) forever and any
        // system that reads `GlobalTransform` for a world position sees
        // `(0, 0, 0)` — which is how the brain's target enumeration reported
        // every harvestable resource as being at tile `(0, 0)`, turning every
        // Harvest plan into a `Walk → PathBlocked { target_tile: (0, 0) }`
        // loop that ultimately starved the default sim (#416). Agents
        // navigate on `Transform` so they *appear* to move; only systems
        // reading `GlobalTransform` saw the bug.
        app.add_plugins(bevy::transform::TransformPlugin);

        // Resources normally provided by plugins we deliberately exclude:
        // - SpawnerPlugin (Ontology, plus startup population we don't want)
        // - MapPlugin (WorldMap, plus tile sprite spawning)
        // - EnvironmentPlugin (LightLevel, plus ClearColor manipulation)
        // - CorePlugin (TickCount/GameLog/GameTime, plus keyboard time controls)
        app.insert_resource(setup_ontology());
        app.insert_resource(map);
        app.insert_resource(LightLevel(1.0));
        app.insert_resource(TickCount::new(60.0));
        app.insert_resource(GameLog::new(100));
        app.init_resource::<GameTime>();
        app.insert_resource(crate::core::SimRng::from_seed(seed));
        app.add_plugins(SpatialIndexPlugin);

        // SimEvent history — collected automatically each tick.
        app.init_resource::<SimEventLog>();
        app.add_systems(Last, collect_sim_events_into_log);

        // Replace tick_system with a deterministic per-update tick advancer so
        // each `app.update()` advances exactly one logical tick regardless of
        // wall-clock delta. Running in PreUpdate guarantees the tick is
        // committed before any Update systems (e.g. decay_relationships) read it,
        // which prevents ordering-dependent flakes.
        app.add_systems(PreUpdate, deterministic_tick);

        // Adds biology, brains, nervous_system, mind systems, action registry,
        // conversation manager, relationship config, etc.
        app.add_plugins(AgentPlugin);

        // Resource regeneration (berry bushes, apple trees, etc). SpawnerPlugin
        // is excluded because it also runs startup spawning, but we still need
        // the Update system that refills depleted resources over time.
        app.add_systems(Update, crate::world::apple_tree::regenerate_resources);

        // Ontology derivation + world entity property systems (fuel, durability, shelter).
        app.add_plugins(crate::world::property::OntologyDerivationPlugin);

        Self { app, seed }
    }

    /// Begin building a composable test scenario. Returns a `ScenarioBuilder`
    /// that lets you configure the map, agents, groups, relationships, and
    /// resources before calling `.build()`.
    ///
    /// ```ignore
    /// let (mut world, agents) = TestWorld::scenario(42)
    ///     .map_size(32, 32)
    ///     .noise_biomes(false)
    ///     .agent("alice").pos(Vec2::new(50.0, 50.0)).hunger_urgency(0.8).done()
    ///     .berry_bushes(2, Vec2::new(60.0, 50.0))
    ///     .build();
    /// let alice = agents["alice"];
    /// ```
    pub fn scenario(seed: u64) -> crate::testing::scenario::ScenarioBuilder {
        crate::testing::scenario::ScenarioBuilder::new(seed)
    }

    /// Convenience preset: one agent on a small flat map with two nearby berry bushes.
    pub fn solo_agent(seed: u64) -> (Self, Entity) {
        let (world, agents) = Self::scenario(seed)
            .map_size(32, 32)
            .noise_biomes(false)
            .agent("agent")
            .pos(Vec2::new(50.0, 50.0))
            .done()
            .berry_bushes(2, Vec2::new(60.0, 50.0))
            .build();
        (world, agents["agent"])
    }

    /// Convenience preset: two socially-driven strangers on a small flat map.
    pub fn two_strangers(seed: u64) -> (Self, Entity, Entity) {
        let (world, agents) = Self::scenario(seed)
            .map_size(32, 32)
            .noise_biomes(false)
            .agent("a")
            .pos(Vec2::new(50.0, 50.0))
            .social_drive(0.8)
            .done()
            .agent("b")
            .pos(Vec2::new(52.0, 50.0))
            .social_drive(0.8)
            .done()
            .build();
        (world, agents["a"], agents["b"])
    }

    /// The seed this TestWorld was created with.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    // ─── Spawning ──────────────────────────────────────────────────────────

    /// Spawns a Person agent with the given config.
    pub fn spawn_agent(&mut self, config: AgentConfig) -> Entity {
        let ontology = self.app.world().resource::<Ontology>().clone();
        spawn_test_person(self.app.world_mut(), ontology, config)
    }

    /// Spawns `n` agents in a small grid centered on `near`. Useful for crowd
    /// scenarios; returns the entities in spawn order.
    pub fn spawn_agent_cluster(&mut self, n: usize, near: Vec2) -> Vec<Entity> {
        // Lay out as a square grid with ~16 px spacing.
        let side = (n as f32).sqrt().ceil() as usize;
        let spacing = 16.0;
        let mut entities = Vec::with_capacity(n);
        for i in 0..n {
            let row = (i / side) as f32;
            let col = (i % side) as f32;
            let offset = Vec2::new(col * spacing, row * spacing);
            let center_offset = Vec2::new(side as f32 * spacing * 0.5, side as f32 * spacing * 0.5);
            let pos = near + offset - center_offset;
            entities.push(self.spawn_agent(AgentConfig::at(pos)));
        }
        entities
    }

    /// Spawns a deer (animal agent) with the species-baseline genome.
    pub fn spawn_deer(&mut self, pos: Vec2) -> Entity {
        self.spawn_deer_with(pos, crate::agent::body::genetics::genome::Genome::default())
    }

    /// Spawns a deer with a caller-provided genome (or builder).
    ///
    /// Accepts any `Into<Genome>` — typically a fluent builder:
    /// ```ignore
    /// world.spawn_deer_with(pos, physical().speed(1.3));
    /// world.spawn_deer_with(pos, personality().openness(0.75));
    /// ```
    pub fn spawn_deer_with(
        &mut self,
        pos: Vec2,
        genome: impl Into<crate::agent::body::genetics::genome::Genome>,
    ) -> Entity {
        let ontology = self.app.world().resource::<Ontology>().clone();
        spawn_test_deer(self.app.world_mut(), ontology, pos, genome.into())
    }

    /// Spawns a wolf at the given position with the species-baseline genome.
    pub fn spawn_wolf(&mut self, pos: Vec2) -> Entity {
        self.spawn_wolf_with(pos, crate::agent::body::genetics::genome::Genome::default())
    }

    /// Spawns a wolf with a caller-provided genome (or builder).
    pub fn spawn_wolf_with(
        &mut self,
        pos: Vec2,
        genome: impl Into<crate::agent::body::genetics::genome::Genome>,
    ) -> Entity {
        let ontology = self.app.world().resource::<Ontology>().clone();
        spawn_test_wolf(self.app.world_mut(), ontology, pos, genome.into())
    }

    /// Spawns a pack of wolves at the given positions and sets up mutual pack bonds.
    ///
    /// All wolves in the returned list know each other as high-trust friends,
    /// mirroring the bonds established by `setup_wolf_pack_bonds` in the real game.
    pub fn spawn_wolf_pack(&mut self, positions: &[Vec2]) -> Vec<Entity> {
        use crate::agent::body::genetics::genome::Genome;
        use crate::agent::mind::knowledge::{Concept, Metadata, Node, Predicate, Triple, Value};

        let ontology = self.app.world().resource::<Ontology>().clone();
        let entities: Vec<Entity> = positions
            .iter()
            .map(|&pos| {
                spawn_test_wolf(
                    self.app.world_mut(),
                    ontology.clone(),
                    pos,
                    Genome::default(),
                )
            })
            .collect();

        let meta = Metadata::default();
        let world = self.app.world_mut();
        for &wolf in &entities {
            let packmates: Vec<Entity> = entities.iter().filter(|&&e| e != wolf).copied().collect();
            let mut mind = world.get_mut::<MindGraph>(wolf).unwrap();
            for packmate in packmates {
                mind.assert(Triple::with_meta(
                    Node::Entity(packmate),
                    Predicate::IsA,
                    Value::Concept(Concept::Friend),
                    meta.clone(),
                ));
                mind.assert(Triple::with_meta(
                    Node::Entity(packmate),
                    Predicate::Trust,
                    Value::Float(0.9),
                    meta.clone(),
                ));
                mind.assert(Triple::with_meta(
                    Node::Entity(packmate),
                    Predicate::Affection,
                    Value::Float(0.8),
                    meta.clone(),
                ));
            }
        }

        entities
    }

    /// Spawns a berry bush at the given position with the specified berry count.
    pub fn spawn_berry_bush(&mut self, pos: Vec2, berries: u32) -> Entity {
        spawn_test_berry_bush(self.app.world_mut(), pos, berries)
    }

    /// Spawns an apple tree at the given position with the specified apple count.
    pub fn spawn_apple_tree(&mut self, pos: Vec2, apples: u32) -> Entity {
        spawn_test_apple_tree(self.app.world_mut(), pos, apples)
    }

    /// Spawns a stone node at the given position with the specified stone count.
    pub fn spawn_stone_node(&mut self, pos: Vec2, stones: u32) -> Entity {
        spawn_test_stone_node(self.app.world_mut(), pos, stones)
    }

    /// Spawns a wood log at the given position with the specified wood count.
    pub fn spawn_wood_log(&mut self, pos: Vec2, wood: u32) -> Entity {
        spawn_test_wood_log(self.app.world_mut(), pos, wood)
    }

    /// Spawns a campfire (logic-only) at the given position. Includes LightSource and HeatSource.
    pub fn spawn_campfire(&mut self, pos: Vec2) -> Entity {
        self.app
            .world_mut()
            .spawn(crate::world::campfire::campfire_components(pos))
            .id()
    }

    /// Spawns a bare entity with a SoundSource at the given position.
    /// The SoundSource is transient and will be cleaned up after one perception tick.
    pub fn spawn_sound_source(
        &mut self,
        pos: Vec2,
        kind: crate::world::sense_sources::SoundKind,
        intensity: f32,
    ) -> Entity {
        self.app
            .world_mut()
            .spawn((
                crate::world::Physical,
                crate::world::sense_sources::SoundSource { kind, intensity },
                Transform::from_translation(pos.extend(0.0)),
                GlobalTransform::default(),
            ))
            .id()
    }

    /// Spawns all entities from a layout using the test-compatible (logic-only,
    /// no-visuals) spawners. Counterpart to [`crate::world::spawner::apply_layout`]
    /// which uses the full visual spawners.
    ///
    /// Spawns all entities from a layout using the test-compatible (logic-only,
    /// no-visuals) spawners. Human personalities are left at the neutral default
    /// (0.5 on all traits); genome-derived variation only applies after a tick
    /// once `develop_phenotype_system` runs.
    pub fn apply_spawn_layout(&mut self, layout: &SpawnLayout) {
        for &pos in &layout.human_positions {
            self.spawn_agent(AgentConfig::at(pos));
        }
        for &pos in &layout.second_human_positions {
            self.spawn_agent(AgentConfig::at(pos));
        }
        for herd in &layout.deer_herds {
            let members: Vec<Entity> = herd.iter().map(|&pos| self.spawn_deer(pos)).collect();
            if members.len() > 1 {
                crate::testing::spawn::introduce_kin(self, &members, 0.8);
            }
        }
        for pack in &layout.wolf_packs {
            let members = self.spawn_wolf_pack(pack);
            if members.len() > 1 {
                crate::testing::spawn::introduce_kin(self, &members, 0.8);
            }
        }
        for &(pos, berries) in &layout.berry_bush_positions {
            self.spawn_berry_bush(pos, berries);
        }
        for &(pos, apples) in &layout.apple_tree_positions {
            self.spawn_apple_tree(pos, apples);
        }
        for &(pos, stones) in &layout.stone_node_positions {
            self.spawn_stone_node(pos, stones);
        }
        for &(pos, wood) in &layout.wood_log_positions {
            self.spawn_wood_log(pos, wood);
        }
    }

    /// Sets a tile type at the given tile coordinates.
    pub fn set_tile(&mut self, x: u32, y: u32, tile: crate::world::map::TileType) {
        self.app
            .world_mut()
            .resource_mut::<WorldMap>()
            .set_tile(x, y, tile);
    }

    // ─── Simulation ────────────────────────────────────────────────────────

    /// Advances the simulation by `n` ticks. Each tick is one full Bevy `update()`
    /// pass with all logic systems running.
    pub fn tick(&mut self, n: u64) {
        for _ in 0..n {
            self.app.update();
        }
    }

    /// Returns the current tick count.
    pub fn current_tick(&self) -> u64 {
        self.app.world().resource::<TickCount>().current
    }

    /// Borrows the SimEventLog for assertion in tests.
    /// Use this to check that specific SimEvent variants were emitted.
    pub fn sim_events(&self) -> &SimEventLog {
        self.app.world().resource::<SimEventLog>()
    }

    // ─── Inspection ────────────────────────────────────────────────────────

    /// Returns the underlying Bevy `App` for advanced introspection. Prefer the
    /// typed helpers below for common queries.
    pub fn app(&self) -> &App {
        &self.app
    }

    /// Returns the underlying Bevy `App` for advanced mutation. Prefer the typed
    /// helpers below for common operations.
    pub fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    /// Borrows a component from an entity. Panics if missing — tests should know
    /// what they spawned.
    pub fn get<T: Component>(&self, entity: Entity) -> &T {
        self.app.world().get::<T>(entity).unwrap_or_else(|| {
            panic!(
                "entity {entity:?} missing component {}",
                std::any::type_name::<T>()
            )
        })
    }

    /// Mutably borrows a component from an entity. Panics if missing.
    pub fn get_mut<T: Component<Mutability = bevy::ecs::component::Mutable>>(
        &mut self,
        entity: Entity,
    ) -> Mut<'_, T> {
        let type_name = std::any::type_name::<T>();
        self.app
            .world_mut()
            .get_mut::<T>(entity)
            .unwrap_or_else(|| panic!("entity {entity:?} missing component {type_name}"))
    }

    /// Returns true if the entity still exists in the world.
    pub fn entity_exists(&self, entity: Entity) -> bool {
        self.app.world().get_entity(entity).is_ok()
    }

    /// Returns the Euclidean distance between two entities' Transforms in 2D.
    /// Panics if either entity lacks a Transform.
    pub fn distance(&self, a: Entity, b: Entity) -> f32 {
        let pos_a = self.get::<Transform>(a).translation.truncate();
        let pos_b = self.get::<Transform>(b).translation.truncate();
        pos_a.distance(pos_b)
    }

    /// Returns all agent entities currently in the world.
    pub fn all_agents(&mut self) -> Vec<Entity> {
        let world = self.app.world_mut();
        let mut query = world.query_filtered::<Entity, With<Agent>>();
        query.iter(world).collect()
    }

    /// Finds an agent entity by name (case-insensitive). Returns `None` if no
    /// agent with that name exists.
    pub fn find_agent_by_name(&mut self, name: &str) -> Option<Entity> {
        let world = self.app.world_mut();
        let mut query = world.query_filtered::<(Entity, &Name), With<Agent>>();
        query
            .iter(world)
            .find(|(_, n)| n.as_str().eq_ignore_ascii_case(name))
            .map(|(e, _)| e)
    }

    /// Finds an agent entity by its Bevy entity-id string (e.g. `"0v0"`,
    /// `"19v0"`). This format matches `format!("{entity:?}")` so it lines up
    /// with the `agent_id` field in the JSONL event log. Returns `None` if
    /// no agent with that id exists.
    pub fn find_agent_by_entity_id(&mut self, id: &str) -> Option<Entity> {
        let world = self.app.world_mut();
        let mut query = world.query_filtered::<Entity, With<Agent>>();
        query
            .iter(world)
            .find(|e| format!("{e:?}").eq_ignore_ascii_case(id))
    }

    /// Convenience: try entity-id lookup first (fast, exact), then fall back
    /// to name lookup. Used by all inspection CLI commands so users can pass
    /// either form and get the same behavior.
    pub fn find_agent(&mut self, selector: &str) -> Option<Entity> {
        self.find_agent_by_entity_id(selector)
            .or_else(|| self.find_agent_by_name(selector))
    }

    // ─── Convenience queries ───────────────────────────────────────────────

    /// True if `agent` has at least one Knows triple about `other`.
    pub fn agent_knows(&self, agent: Entity, other: Entity) -> bool {
        let mind = self.get::<MindGraph>(agent);
        !mind
            .query(
                Some(&MindNode::Self_),
                Some(Predicate::Knows),
                Some(&Value::Entity(other)),
            )
            .is_empty()
            || !mind
                .query(Some(&MindNode::Entity(other)), Some(Predicate::Knows), None)
                .is_empty()
    }

    /// Returns the trust value `agent` has toward `other`, or 0.0 if no triple exists.
    pub fn agent_trust(&self, agent: Entity, other: Entity) -> f32 {
        let mind = self.get::<MindGraph>(agent);
        mind.query(Some(&MindNode::Entity(other)), Some(Predicate::Trust), None)
            .into_iter()
            .find_map(|t| match &t.object {
                Value::Float(f) => Some(*f),
                _ => None,
            })
            .unwrap_or(0.0)
    }

    /// Returns the agent's hunger value (0.0–100.0).
    /// Hunger urgency 0..1 derived from the agent's metabolism pools.
    /// 0.0 = fully sated, 1.0 = every pool empty.
    pub fn agent_hunger(&self, agent: Entity) -> f32 {
        self.get::<PhysicalNeeds>(agent).hunger_urgency()
    }

    /// Returns the agent's thirst value (0.0–100.0).
    pub fn agent_thirst(&self, agent: Entity) -> f32 {
        100.0 - self.get::<PhysicalNeeds>(agent).hydration
    }

    /// Returns the agent's aerobic stamina value (0.0–aerobic_max).
    /// This is the primary "how tired" fatigue value; anaerobic is the
    /// sprint reserve, accessed separately if needed.
    pub fn agent_aerobic(&self, agent: Entity) -> f32 {
        self.get::<PhysicalNeeds>(agent).stamina.aerobic
    }

    /// Returns the agent's anaerobic (sprint) reserve.
    pub fn agent_anaerobic(&self, agent: Entity) -> f32 {
        self.get::<PhysicalNeeds>(agent).stamina.anaerobic
    }

    /// Returns true if the entity carries any of the given concept in its inventory.
    pub fn has_item(&self, entity: Entity, concept: Concept) -> bool {
        self.app
            .world()
            .get::<crate::agent::item_slots::ItemSlots>(entity)
            .map(|inv| inv.has(concept))
            .unwrap_or(false)
    }

    /// Returns the count of `concept` in the entity's inventory, or 0 if missing.
    pub fn item_count(&self, entity: Entity, concept: Concept) -> u32 {
        self.app
            .world()
            .get::<crate::agent::item_slots::ItemSlots>(entity)
            .map(|inv| inv.count(concept))
            .unwrap_or(0)
    }

    /// Returns the action type the agent is currently executing. Returns
    /// `Some(Idle)` when the agent has no active action. With parallel
    /// channels, this reports the *primary* (highest-intensity) running action.
    pub fn current_action(&self, agent: Entity) -> Option<ActionType> {
        let world = self.app.world();
        let active = world.get::<ActiveActions>(agent)?;
        let registry = world.resource::<ActionRegistry>();
        Some(
            active
                .primary(registry)
                .map(|s| s.action_type)
                .unwrap_or(ActionType::Idle),
        )
    }

    /// Returns true if the agent is currently in a conversation.
    pub fn in_conversation(&self, agent: Entity) -> bool {
        self.app
            .world()
            .get::<crate::agent::mind::conversation::InConversation>(agent)
            .is_some()
    }

    /// Returns the number of active (non-ended) conversations in the world.
    pub fn active_conversation_count(&self) -> usize {
        self.app
            .world()
            .resource::<crate::agent::mind::conversation::ConversationManager>()
            .active_conversations()
            .count()
    }

    /// Returns true if the action registry contains an entry for the given action.
    /// Useful for catching test setup mistakes.
    pub fn has_registered_action(&self, action: ActionType) -> bool {
        self.app
            .world()
            .resource::<ActionRegistry>()
            .get(action)
            .is_some()
    }

    // ─── Text inspection (output goes to stderr) ───────────────────────────

    /// Print full agent state to stderr: position, current action, brain winner,
    /// physical needs, psychological drives, consciousness, emotional state, and body.
    pub fn print_agent_state(&self, agent: Entity) {
        let world = self.app.world();
        let tick = world.resource::<TickCount>().current;
        let name = entity_name(world, agent);
        print_section_header("Agent state", &name, agent, tick);

        // Position
        if let Some(tf) = world.get::<Transform>(agent) {
            let pos = tf.translation.truncate();
            eprintln!("  Position:  ({:.1}, {:.1})", pos.x, pos.y);
        }

        // Current action
        if let Some(active) = world.get::<ActiveActions>(agent) {
            let registry = world.resource::<ActionRegistry>();
            if let Some(primary) = active.primary(registry) {
                eprintln!("  Action:    {:?}", primary.action_type);
            } else {
                eprintln!("  Action:    Idle");
            }
            let running: Vec<_> = active
                .iter()
                .map(|s| format!("{:?}", s.action_type))
                .collect();
            if running.len() > 1 {
                eprintln!("  All running: [{}]", running.join(", "));
            }
        }

        // Brain winner
        if let Some(brain) = world.get::<BrainState>(agent) {
            eprintln!(
                "  Brain:     winner={:?}  S:{:.2} E:{:.2} R:{:.2}",
                brain.winner, brain.powers.survival, brain.powers.emotional, brain.powers.rational
            );
        }

        // Physical needs — show the underlying metabolism pools, not just
        // the abstract hunger_urgency() roll-up. The roll-up hides whether
        // an agent is actually starving (low glucose+reserves) vs just
        // running on an empty stomach with full backup.
        if let Some(needs) = world.get::<PhysicalNeeds>(agent) {
            let m = &needs.metabolism;
            let starving = if m.is_starving() { "  STARVING" } else { "" };
            eprintln!(
                "  Vitals:    health={:.1}  thirst={:.1}  stamina(a/an)={:.1}/{:.1}",
                needs.health,
                100.0 - needs.hydration,
                needs.stamina.aerobic,
                needs.stamina.anaerobic
            );
            eprintln!(
                "  Metabolism: stomach(c/f)={:.1}/{:.1}  glucose={:.1}/100  reserves={:.0}/500  hunger={:.2}{}",
                m.stomach_carbs,
                m.stomach_fat,
                m.glucose,
                m.reserves,
                needs.hunger_urgency(),
                starving
            );
            if let Some(src) = needs.last_health_damage {
                eprintln!("  Last damage: {:?}", src);
            }
        }

        // Inventory
        if let Some(inv) = world.get::<crate::agent::item_slots::ItemSlots>(agent) {
            let items: Vec<String> = inv
                .group_by_concept()
                .into_iter()
                .map(|(c, q)| format!("{c:?}×{q}"))
                .collect();
            if items.is_empty() {
                eprintln!("  Inventory: (empty)");
            } else {
                eprintln!("  Inventory: [{}]", items.join(", "));
            }
        }

        // Self-inventory beliefs as they appear in the MindGraph. These
        // are what the Rational planner's `self_contains_food()`
        // precondition actually queries, not the ItemSlots component —
        // when the two disagree it means perception or belief-update
        // drift. Silent divergence here was the final Alice-eats-nothing
        // pathology in #416.
        if let Some(mind_graph) = world.get::<crate::agent::mind::knowledge::MindGraph>(agent) {
            use crate::agent::mind::knowledge::{Node, Predicate, Value};
            let triples = mind_graph.query(Some(&Node::Self_), Some(Predicate::Contains), None);
            let items: Vec<String> = triples
                .iter()
                .filter_map(|t| match &t.object {
                    Value::Item(c, q) => Some(format!("{c:?}×{q}")),
                    _ => None,
                })
                .collect();
            if items.is_empty() {
                eprintln!("  MindInv:   (empty)");
            } else {
                eprintln!("  MindInv:   [{}]", items.join(", "));
            }
        }

        // Current CNS goal
        if let Some(cns) =
            world.get::<crate::agent::nervous_system::cns::CentralNervousSystem>(agent)
        {
            match &cns.current_goal {
                Some(goal) => {
                    let conds: Vec<String> = goal
                        .conditions
                        .iter()
                        .map(|c| {
                            format!(
                                "{:?}={:?}",
                                c.predicate.map(|p| format!("{p:?}")).unwrap_or_default(),
                                c.object
                            )
                        })
                        .collect();
                    eprintln!(
                        "  Goal:      {} (priority {:.2})",
                        if conds.is_empty() {
                            "(empty)".to_string()
                        } else {
                            conds.join(", ")
                        },
                        goal.priority
                    );
                }
                None => eprintln!("  Goal:      (none)"),
            }
            // Top urgencies for context
            let top: Vec<String> = cns
                .urgencies
                .iter()
                .take(3)
                .map(|u| format!("{:?}={:.2}", u.source, u.value))
                .collect();
            if !top.is_empty() {
                eprintln!("  Urgency:   [{}]", top.join(", "));
            }
        }

        // Plan memory
        if let Some(memory) = world.get::<crate::agent::brains::plan_memory::PlanMemory>(agent) {
            if memory.plans.is_empty() {
                eprintln!("  Plans:     (none)");
            } else {
                eprintln!("  Plans:");
                for plan in memory.plans.iter() {
                    let cur_action = plan.current();
                    let cur = cur_action
                        .map(|a| format!("{:?}", a.action_type))
                        .unwrap_or_else(|| "(finished)".to_string());
                    let target = cur_action
                        .and_then(|a| {
                            a.target_entity
                                .map(|e| format!(" tgt={:?}", e))
                                .or_else(|| {
                                    a.target_position
                                        .map(|p| format!(" pos=({:.0},{:.0})", p.x, p.y))
                                })
                        })
                        .unwrap_or_default();
                    let intent = plan
                        .goal
                        .conditions
                        .iter()
                        .find_map(|c| c.predicate.map(|p| format!("{p:?}")))
                        .unwrap_or_else(|| "?".to_string());
                    eprintln!(
                        "    {:?} {:?} step {}/{}: {}{}  (goal={}, prio={:.2}, commit={:.2})",
                        plan.id,
                        plan.state,
                        plan.current_step,
                        plan.steps.len(),
                        cur,
                        target,
                        intent,
                        plan.goal.priority,
                        plan.commitment,
                    );
                    // Print remaining steps so we can see the full plan
                    // shape (Harvest→Eat, Walk→Drink, etc.).
                    if plan.steps.len() > 1 {
                        let steps: Vec<String> = plan
                            .steps
                            .iter()
                            .enumerate()
                            .map(|(i, s)| {
                                let marker = if i == plan.current_step { ">" } else { " " };
                                format!("{}{:?}", marker, s.action_type)
                            })
                            .collect();
                        eprintln!("      steps: [{}]", steps.join(", "));
                    }
                }
            }
        }

        // Recent action summary — what's the agent been doing in the last
        // 2000 ticks? Critical for spotting "Alice harvests 178 times but
        // eats 0 times after tick 26000" patterns.
        {
            let log = world.resource::<SimEventLog>();
            const WINDOW: u64 = 2000;
            let cutoff = tick.saturating_sub(WINDOW);
            let mut started: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            let mut failed: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            let mut last_eat_tick: Option<u64> = None;
            let mut last_harvest_tick: Option<u64> = None;
            for event in log.all() {
                let event_tick = sim_event_tick(event);
                match event {
                    SimEvent::ActionStarted {
                        agent: a, action, ..
                    } if *a == agent => {
                        if event_tick >= cutoff {
                            *started.entry(format!("{action:?}")).or_insert(0) += 1;
                        }
                        match action {
                            crate::agent::actions::ActionType::Eat => {
                                last_eat_tick = Some(event_tick);
                            }
                            crate::agent::actions::ActionType::Harvest => {
                                last_harvest_tick = Some(event_tick);
                            }
                            _ => {}
                        }
                    }
                    SimEvent::ActionFailed {
                        agent: a, action, ..
                    } if *a == agent && event_tick >= cutoff => {
                        *failed.entry(format!("{action:?}")).or_insert(0) += 1;
                    }
                    _ => {}
                }
            }
            if !started.is_empty() {
                let mut entries: Vec<(String, usize)> = started.into_iter().collect();
                entries.sort_by(|a, b| b.1.cmp(&a.1));
                let summary: Vec<String> =
                    entries.iter().map(|(k, v)| format!("{k}×{v}")).collect();
                eprintln!("  Recent({} ticks): [{}]", WINDOW, summary.join(", "));
            }
            if !failed.is_empty() {
                let mut entries: Vec<(String, usize)> = failed.into_iter().collect();
                entries.sort_by(|a, b| b.1.cmp(&a.1));
                let summary: Vec<String> =
                    entries.iter().map(|(k, v)| format!("{k}×{v}")).collect();
                eprintln!("  Failed({} ticks): [{}]", WINDOW, summary.join(", "));
            }
            let format_ago = |t: u64| {
                if t > tick {
                    "?".to_string()
                } else {
                    format!("{} ticks ago", tick - t)
                }
            };
            eprintln!(
                "  Last eat:  {}",
                last_eat_tick
                    .map(format_ago)
                    .unwrap_or_else(|| "(never)".to_string())
            );
            eprintln!(
                "  Last harv: {}",
                last_harvest_tick
                    .map(format_ago)
                    .unwrap_or_else(|| "(never)".to_string())
            );
        }

        // Resource knowledge breakdown — counts of each known entity type
        // and how many of them have a stocked Contains belief.
        if let Some(mind_graph) = world.get::<crate::agent::mind::knowledge::MindGraph>(agent) {
            use crate::agent::mind::knowledge::{Node, Predicate, Value};
            use std::collections::HashMap;
            let mut by_type: HashMap<String, (usize, usize)> = HashMap::new();
            // First pass: collect known entities by IsA-concept.
            let mut entity_type: HashMap<bevy::prelude::Entity, String> = HashMap::new();
            for triple in mind_graph.query(None, Some(Predicate::IsA), None) {
                if let (Node::Entity(e), Value::Concept(c)) =
                    (triple.subject.clone(), &triple.object)
                {
                    entity_type.insert(e, format!("{c:?}"));
                }
            }
            // Second pass: figure out which of those have a stocked Contains.
            let mut stocked: std::collections::HashSet<bevy::prelude::Entity> = Default::default();
            for triple in mind_graph.query(None, Some(Predicate::Contains), None) {
                if let Node::Entity(e) = triple.subject
                    && matches!(triple.object, Value::Item(_, q) if q > 0)
                {
                    stocked.insert(e);
                }
            }
            for (e, type_name) in &entity_type {
                let entry = by_type.entry(type_name.clone()).or_insert((0, 0));
                entry.0 += 1;
                if stocked.contains(e) {
                    entry.1 += 1;
                }
            }
            if !by_type.is_empty() {
                let mut summary: Vec<String> = by_type
                    .iter()
                    .map(|(t, (total, stocked))| {
                        if *stocked > 0 || *total > 5 {
                            format!("{}×{}({}stocked)", t, total, stocked)
                        } else {
                            format!("{}×{}", t, total)
                        }
                    })
                    .collect();
                summary.sort();
                eprintln!("  Knows:     [{}]", summary.join(", "));
            }

            // Distance breakdown to known food sources. The agent might
            // "know" 5 BerryBushes but if they're all 200 tiles away across
            // unwalkable terrain, that knowledge is useless. Sort by distance
            // and tag each with its stocked Contains belief so it's obvious
            // when the agent only remembers depleted bushes.
            let agent_pos = world
                .get::<bevy::prelude::Transform>(agent)
                .map(|t| t.translation.truncate());
            if let Some(agent_pos) = agent_pos {
                let mut food_entries: Vec<(f32, String, bool)> = Vec::new();
                for (e, type_name) in &entity_type {
                    if !matches!(
                        type_name.as_str(),
                        "BerryBush" | "AppleTree" | "Berry" | "Apple"
                    ) {
                        continue;
                    }
                    let Some(tf) = world.get::<bevy::prelude::Transform>(*e) else {
                        continue;
                    };
                    let dist = tf.translation.truncate().distance(agent_pos);
                    food_entries.push((dist, type_name.clone(), stocked.contains(e)));
                }
                food_entries.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                if !food_entries.is_empty() {
                    let summary: Vec<String> = food_entries
                        .iter()
                        .take(6)
                        .map(|(d, t, s)| {
                            let mark = if *s { "+" } else { "-" };
                            format!("{t}@{d:.0}{mark}")
                        })
                        .collect();
                    eprintln!("  Food dist: [{}] (+stocked, -empty)", summary.join(", "));
                }
            }
        }

        // Recent path-blocked targets — when an agent is geographically
        // trapped, this surfaces the actual tiles the pathfinder rejected.
        // Pair this with `Failed[Explore×N]` from the recent action summary
        // to spot the "agent keeps trying to walk through the same wall"
        // pattern that drives long-tail starvation deaths.
        {
            let log = world.resource::<SimEventLog>();
            const WINDOW: u64 = 2000;
            let cutoff = tick.saturating_sub(WINDOW);
            let mut blocked: std::collections::HashMap<(i32, i32), usize> =
                std::collections::HashMap::new();
            for event in log.all() {
                if let SimEvent::ActionFailed {
                    agent: a,
                    tick: et,
                    reason: crate::agent::events::FailureReason::PathBlocked { target_tile },
                    ..
                } = event
                    && *a == agent
                    && *et >= cutoff
                {
                    *blocked.entry(*target_tile).or_insert(0) += 1;
                }
            }
            if !blocked.is_empty() {
                let mut entries: Vec<((i32, i32), usize)> = blocked.into_iter().collect();
                entries.sort_by(|a, b| b.1.cmp(&a.1));
                let summary: Vec<String> = entries
                    .iter()
                    .take(6)
                    .map(|((tx, ty), n)| format!("({tx},{ty})×{n}"))
                    .collect();
                eprintln!("  Blocked({} ticks): [{}]", WINDOW, summary.join(", "));
            }
        }

        // Consciousness
        if let Some(con) = world.get::<Consciousness>(agent) {
            eprintln!("  Alertness: {:.2}", con.alertness);
        }

        // Psychological drives
        if let Some(drives) = world.get::<PsychologicalDrives>(agent) {
            eprintln!(
                "  Drives:    social={:.2}  fun={:.2}  curiosity={:.2}  status={:.2}  security={:.2}  autonomy={:.2}",
                1.0 - drives.companionship,
                1.0 - drives.enjoyment,
                1.0 - drives.stimulation,
                1.0 - drives.esteem,
                1.0 - drives.safety,
                1.0 - drives.autonomy
            );
        }

        // Emotional state. Fuel is load-bearing when intensity saturates
        // at 1.0: stuck-max-anger looks identical to one-tick-max-anger
        // without it.
        if let Some(emo) = world.get::<EmotionalState>(agent) {
            eprintln!(
                "  Emotions:  mood={:.3}  stress={:.1}  active=[{}]",
                emo.current_mood,
                emo.stress_level,
                emo.active_emotions
                    .iter()
                    .map(|e| format!("{:?}(i={:.2},f={:.1})", e.emotion_type, e.intensity, e.fuel))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        // Body
        if let Some(body) = world.get::<Body>(agent) {
            eprintln!("  Body:");
            for part in body.parts() {
                let injury_str = if part.injuries.is_empty() {
                    String::new()
                } else {
                    format!(
                        "  injuries=[{}]",
                        part.injuries
                            .iter()
                            .map(|i| format!("{:?}({:.2})", i.injury_type, i.severity))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                eprintln!(
                    "    {:<10}  hp={:.0}/{:.0}  fn={:.2}{}",
                    part.name(),
                    part.current_hp,
                    part.max_hp,
                    part.function_rate,
                    injury_str
                );
            }
        }

        print_section_footer();
    }

    /// Print the last brain decision for `agent` to stderr: all proposals, their
    /// urgency and reasoning, the power levels, and the winner.
    pub fn print_brain_decision(&self, agent: Entity) {
        let world = self.app.world();
        let tick = world.resource::<TickCount>().current;
        let name = entity_name(world, agent);
        print_section_header("Brain decision", &name, agent, tick);

        let Some(brain) = world.get::<BrainState>(agent) else {
            eprintln!("  (no BrainState component)");
            print_section_footer();
            return;
        };

        eprintln!(
            "  Powers:  Survival={:.2}  Emotional={:.2}  Rational={:.2}",
            brain.powers.survival, brain.powers.emotional, brain.powers.rational
        );
        eprintln!("  Winner:  {:?}", brain.winner);

        if brain.proposals.is_empty() {
            eprintln!("  Proposals: (none this tick — brain thinking interval not reached)");
        } else {
            eprintln!("  Proposals ({}):", brain.proposals.len());
            for p in brain.proposals.iter() {
                eprintln!(
                    "    [{:?}]  urgency={:.2}  action={:?}",
                    p.brain, p.urgency, p.action.action_type
                );
                if let Some(target) = p.action.target_entity {
                    eprintln!("             target={target:?}");
                }
                eprintln!("             reason=\"{}\"", p.reasoning);
            }
        }

        if brain.chosen_actions.is_empty() {
            eprintln!("  Chosen actions: (none)");
        } else {
            eprintln!("  Chosen actions:");
            for a in &brain.chosen_actions {
                eprintln!("    {:?}", a.action_type);
            }
        }

        print_section_footer();
    }

    /// Print the full MindGraph for `agent` to stderr: all triples across the
    /// ontology, shared knowledge blocks, and personal knowledge.
    pub fn print_mind_graph(&self, agent: Entity) {
        let world = self.app.world();
        let tick = world.resource::<TickCount>().current;
        let name = entity_name(world, agent);
        print_section_header("MindGraph", &name, agent, tick);

        let Some(mind) = world.get::<MindGraph>(agent) else {
            eprintln!("  (no MindGraph component)");
            print_section_footer();
            return;
        };

        eprintln!("  Ontology ({} triples):", mind.ontology.triples.len());
        for triple in mind.ontology.triples.iter() {
            eprintln!("    {}", format_triple(triple));
        }

        let shared_total: usize = mind.shared_knowledge.iter().map(|v| v.len()).sum();
        if shared_total > 0 {
            eprintln!("  Shared knowledge ({shared_total} triples):");
            for block in &mind.shared_knowledge {
                for triple in block.iter() {
                    eprintln!("    {}", format_triple(triple));
                }
            }
        }

        eprintln!("  Personal knowledge ({} triples):", mind.len());
        for triple in mind.iter() {
            eprintln!(
                "    {}  [conf={:.2} age={}]",
                format_triple(triple),
                triple.meta.confidence,
                tick.saturating_sub(triple.meta.timestamp)
            );
        }

        print_section_footer();
    }

    /// Print all relationships (Trust, Affection, Respect) that `agent` holds
    /// toward other entities.
    pub fn print_relationships(&self, agent: Entity) {
        let world = self.app.world();
        let tick = world.resource::<TickCount>().current;
        let name = entity_name(world, agent);
        print_section_header("Relationships", &name, agent, tick);

        let Some(mind) = world.get::<MindGraph>(agent) else {
            eprintln!("  (no MindGraph component)");
            print_section_footer();
            return;
        };

        // Single pass: collect (trust, affection, respect, knows) per Entity subject.
        #[derive(Default)]
        struct RelEntry {
            trust: Option<f32>,
            affection: Option<f32>,
            respect: Option<f32>,
            knows: bool,
        }
        let mut by_entity: std::collections::HashMap<Entity, RelEntry> =
            std::collections::HashMap::new();
        for pred in [
            Predicate::Trust,
            Predicate::Affection,
            Predicate::Respect,
            Predicate::Knows,
        ] {
            for triple in mind.query(None, Some(pred), None) {
                if let MindNode::Entity(e) = &triple.subject {
                    let entry = by_entity.entry(*e).or_default();
                    match (pred, &triple.object) {
                        (Predicate::Trust, Value::Float(f)) => entry.trust = Some(*f),
                        (Predicate::Affection, Value::Float(f)) => entry.affection = Some(*f),
                        (Predicate::Respect, Value::Float(f)) => entry.respect = Some(*f),
                        (Predicate::Knows, _) => entry.knows = true,
                        _ => {}
                    }
                }
            }
        }

        if by_entity.is_empty() {
            eprintln!("  (no relationship entries)");
        } else {
            for (other, rel) in &by_entity {
                let other_name = entity_name(world, *other);
                eprintln!(
                    "  {other_name} [{other:?}]  knows={knows}  trust={trust}  affection={affection}  respect={respect}",
                    knows = rel.knows,
                    trust = rel
                        .trust
                        .map(|f| format!("{f:.3}"))
                        .unwrap_or_else(|| "-".to_string()),
                    affection = rel
                        .affection
                        .map(|f| format!("{f:.3}"))
                        .unwrap_or_else(|| "-".to_string()),
                    respect = rel
                        .respect
                        .map(|f| format!("{f:.3}"))
                        .unwrap_or_else(|| "-".to_string()),
                );
            }
        }

        print_section_footer();
    }

    /// Print the current conversation state for `agent` to stderr (if any).
    pub fn print_conversation(&self, agent: Entity) {
        let world = self.app.world();
        let tick = world.resource::<TickCount>().current;
        let name = entity_name(world, agent);
        print_section_header("Conversation", &name, agent, tick);

        let in_conv = world.get::<InConversation>(agent);
        let manager = world.resource::<ConversationManager>();

        let Some(in_conv) = in_conv else {
            eprintln!("  (agent is not currently in a conversation)");
            print_section_footer();
            return;
        };

        let others: Vec<String> = manager
            .conversations
            .get(&in_conv.conversation_id)
            .map(|conv| {
                conv.participants
                    .iter()
                    .filter(|e| **e != agent)
                    .map(|e| format!("{} [{e:?}]", entity_name(world, *e)))
                    .collect()
            })
            .unwrap_or_default();

        eprintln!(
            "  conversation_id={}  others=[{}]",
            in_conv.conversation_id,
            others.join(", ")
        );

        if let Some(conv) = manager.conversations.get(&in_conv.conversation_id) {
            eprintln!(
                "  State: {:?}  started=t{}  last_turn=t{}  turn_index={}  turns={}",
                conv.state,
                conv.started_at,
                conv.last_turn_at,
                conv.turn,
                conv.turns.len()
            );
            for (i, turn) in conv.turns.iter().enumerate() {
                let speaker_name = entity_name(world, turn.speaker);
                eprintln!(
                    "  Turn {i}: [{speaker_name}] intent={:?}  topic={:?}  triples={}",
                    turn.intent,
                    turn.topic,
                    turn.content.len()
                );
            }
        }

        print_section_footer();
    }

    /// Search the agent's full MindGraph (ontology + shared + personal) for
    /// triples whose subject, predicate, or object Debug representation contains
    /// `query` (case-insensitive substring match). Returns formatted strings.
    pub fn query_knowledge(&self, agent: Entity, query: &str) -> Vec<String> {
        let Some(mind) = self.app.world().get::<MindGraph>(agent) else {
            return Vec::new();
        };

        let query_lower = query.to_lowercase();

        let all_triples = mind
            .ontology
            .triples
            .iter()
            .chain(mind.shared_knowledge.iter().flat_map(|v| v.iter()))
            .chain(mind.iter());

        all_triples
            .filter(|t| {
                let subject = format!("{:?}", t.subject).to_lowercase();
                let predicate = format!("{:?}", t.predicate).to_lowercase();
                let object = format!("{:?}", t.object).to_lowercase();
                subject.contains(&query_lower)
                    || predicate.contains(&query_lower)
                    || object.contains(&query_lower)
            })
            .map(format_triple)
            .collect()
    }

    /// Print all SimEvents that occurred in the last `last_n_ticks` ticks to stderr.
    pub fn print_recent_events(&self, last_n_ticks: u64) {
        let world = self.app.world();
        let tick = world.resource::<TickCount>().current;
        let log = world.resource::<SimEventLog>();
        let events: Vec<_> = log.events_since(tick, last_n_ticks).collect();

        eprintln!("══════════════════════════════════════════════════");
        eprintln!(
            "  SimEvents — last {last_n_ticks} ticks (tick {tick})  [{} events]",
            events.len()
        );
        eprintln!("══════════════════════════════════════════════════");
        if events.is_empty() {
            eprintln!("  (none)");
        } else {
            for event in events {
                eprintln!("  {}", format_sim_event(event));
            }
        }
        print_section_footer();
    }

    /// Print what the agent currently perceives: every entity in
    /// VisibleObjects with name, kind, and distance. Mirrors the
    /// Perception tab in the interactive panel.
    pub fn print_perception(&self, agent: Entity) {
        use crate::agent::inventory::EntityType;
        use crate::agent::mind::perception::VisibleObjects;

        let world = self.app.world();
        let tick = world.resource::<TickCount>().current;
        let name = entity_name(world, agent);
        print_section_header("Perception", &name, agent, tick);

        let Some(visible) = world.get::<VisibleObjects>(agent) else {
            eprintln!("  (this entity has no Vision/VisibleObjects)");
            print_section_footer();
            return;
        };
        let agent_pos = world
            .get::<bevy::prelude::Transform>(agent)
            .map(|t| t.translation.truncate());
        if visible.entities.is_empty() {
            eprintln!("  (sees nothing)");
            print_section_footer();
            return;
        }

        let mut rows: Vec<(f32, String, String)> = Vec::new();
        for &other in &visible.entities {
            let n = world
                .get::<bevy::prelude::Name>(other)
                .map(|n| n.to_string())
                .unwrap_or_else(|| format!("{:?}", other));
            let kind = world
                .get::<EntityType>(other)
                .map(|t| format!("{:?}", t.0))
                .unwrap_or_else(|| "?".into());
            let dist = match (agent_pos, world.get::<bevy::prelude::Transform>(other)) {
                (Some(a), Some(t)) => a.distance(t.translation.truncate()),
                _ => f32::INFINITY,
            };
            rows.push((dist, n, kind));
        }
        rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        for (dist, n, kind) in rows {
            let dist_str = if dist.is_finite() {
                format!("{:.0}", dist)
            } else {
                "?".into()
            };
            eprintln!("  {:<20}  {:<14}  dist={}", n, kind, dist_str);
        }
        print_section_footer();
    }

    /// Print body-channel occupancy for the agent to stderr: each channel
    /// with its current load, capacity, and which running actions are
    /// claiming it.
    pub fn print_channels(&self, agent: Entity) {
        use crate::agent::actions::ActionRegistry;
        use crate::agent::actions::channel::{Channel, ChannelCapacities};
        use crate::agent::biology::body::Body;
        use crate::agent::body::needs::PhysicalNeeds;

        let world = self.app.world();
        let tick = world.resource::<TickCount>().current;
        let name = entity_name(world, agent);
        print_section_header("Channels", &name, agent, tick);

        let Some(active) = world.get::<ActiveActions>(agent) else {
            eprintln!("  (no ActiveActions)");
            print_section_footer();
            return;
        };
        let body = world.get::<Body>(agent);
        let physical = world.get::<PhysicalNeeds>(agent);
        let registry = world.resource::<ActionRegistry>();
        let capacities = ChannelCapacities::compute(body, physical);

        let mut per_channel: Vec<(Channel, f32, Vec<String>)> =
            Channel::ALL.iter().map(|c| (*c, 0.0, Vec::new())).collect();
        for state in active.iter() {
            let Some(def) = registry.get(state.action_type) else {
                continue;
            };
            for usage in def.body_channels() {
                let slot = per_channel
                    .iter_mut()
                    .find(|(c, _, _)| *c == usage.channel)
                    .unwrap();
                slot.1 += usage.intensity;
                slot.2.push(format!("{:?}", state.action_type));
            }
        }

        for (channel, load, holders) in per_channel {
            let cap = capacities.get(channel);
            let held = if holders.is_empty() {
                "-".to_string()
            } else {
                holders.join(", ")
            };
            eprintln!(
                "  {:<12}  load={:.2}  cap={:.2}  holders=[{}]",
                format!("{:?}", channel),
                load,
                cap,
                held
            );
        }
        print_section_footer();
    }

    /// Print a causal breakdown for one metric on this agent: every
    /// contributor's signed per-second rate, followed by the net rate.
    /// Supported metrics: "glucose", "stamina", "hydration", "mood".
    pub fn print_why(&self, agent: Entity, metric: &str) {
        let world = self.app.world();
        let tick = world.resource::<TickCount>().current;
        let name = entity_name(world, agent);
        print_section_header(&format!("Why {}", metric), &name, agent, tick);

        match metric {
            "glucose" => dump_contributions_headless(
                world,
                agent,
                "glucose",
                " /sec",
                ContributionKind::Glucose,
            ),
            "stamina" => dump_contributions_headless(
                world,
                agent,
                "stamina",
                " /sec",
                ContributionKind::Stamina,
            ),
            "hydration" => dump_contributions_headless(
                world,
                agent,
                "hydration",
                " /sec",
                ContributionKind::Hydration,
            ),
            "stomach" | "satiety" => dump_contributions_headless(
                world,
                agent,
                "stomach",
                " /sec",
                ContributionKind::Stomach,
            ),
            "mood" => {
                use crate::agent::psyche::emotions::EmotionalState;
                if let Some(emo) = world.get::<EmotionalState>(agent) {
                    eprintln!("  mood scalar: {:+.2}", emo.current_mood);
                    eprintln!("  stress:      {:.1}", emo.stress_level);
                    if emo.active_emotions.is_empty() {
                        eprintln!("  (no active emotions)");
                    } else {
                        for e in &emo.active_emotions {
                            eprintln!(
                                "  {:?}  intensity={:.2}  fuel={:.1}",
                                e.emotion_type, e.intensity, e.fuel
                            );
                        }
                    }
                } else {
                    eprintln!("  (no EmotionalState component)");
                }
            }
            other => {
                eprintln!(
                    "  unknown metric {:?}. try glucose / stamina / hydration / stomach / mood",
                    other
                );
            }
        }
        print_section_footer();
    }

    /// Print all SimEvents involving `agent` in the last `last_n_ticks` ticks to stderr.
    pub fn print_agent_events(&self, agent: Entity, last_n_ticks: u64) {
        let world = self.app.world();
        let tick = world.resource::<TickCount>().current;
        let name = entity_name(world, agent);
        let log = world.resource::<SimEventLog>();
        let events: Vec<_> = log
            .events_since(tick, last_n_ticks)
            .filter(|e| sim_event_involves(e, agent))
            .collect();

        eprintln!("══════════════════════════════════════════════════");
        eprintln!(
            "  Agent SimEvents — {name} [{agent:?}]  last {last_n_ticks} ticks  [{} events]",
            events.len()
        );
        eprintln!("══════════════════════════════════════════════════");
        if events.is_empty() {
            eprintln!("  (none)");
        } else {
            for event in events {
                eprintln!("  {}", format_sim_event(event));
            }
        }
        print_section_footer();
    }
}

impl Default for TestWorld {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Builds the 128×128 noise terrain map used by the normal windowed game.
/// Applies the same `DEFAULT_TERRAIN_SEED` and `generate_terrain` algorithm,
/// so settlement detection and biome-based spawning produce identical results
/// to a real game run.
fn make_game_map() -> WorldMap {
    let mut map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    for cy in 0..MAP_CHUNKS_Y as i32 {
        for cx in 0..MAP_CHUNKS_X as i32 {
            map.chunks.insert(IVec2::new(cx, cy), Chunk::new(cx, cy));
        }
    }
    let terrain = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
    for y in 0..WORLD_HEIGHT {
        for x in 0..WORLD_WIDTH {
            map.set_tile(x, y, terrain[(y * WORLD_WIDTH + x) as usize]);
        }
    }
    map
}

/// Builds a fully walkable WorldMap of the given dimensions in tiles. Initializes
/// every chunk with grass so `is_walkable` returns true everywhere.
pub(super) fn make_walkable_map(width: u32, height: u32) -> WorldMap {
    let mut map = WorldMap::new(width, height);
    let chunks_x = width.div_ceil(CHUNK_SIZE);
    let chunks_y = height.div_ceil(CHUNK_SIZE);
    for cy in 0..chunks_y as i32 {
        for cx in 0..chunks_x as i32 {
            map.chunks.insert(IVec2::new(cx, cy), Chunk::new(cx, cy));
        }
    }
    map
}

/// Replaces `core::tick::tick_system` for tests: increments TickCount.current by
/// exactly one per update, regardless of real-time delta. Also drives GameTime.
fn deterministic_tick(mut tick: ResMut<TickCount>, mut game_time: ResMut<GameTime>) {
    if tick.paused {
        return;
    }
    tick.current += 1;
    game_time.update_from_tick(tick.current);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Person;

    #[test]
    fn new_world_starts_at_tick_zero() {
        let world = TestWorld::with_seed(42);
        assert_eq!(world.current_tick(), 0);
    }

    #[test]
    fn tick_advances_logical_tick_count() {
        let mut world = TestWorld::with_seed(42);
        world.tick(10);
        assert_eq!(world.current_tick(), 10);
    }

    #[test]
    fn spawn_agent_creates_person_with_logic_components() {
        let mut world = TestWorld::with_seed(42);
        let agent = world.spawn_agent(AgentConfig::default());

        // Core markers
        assert!(world.app().world().get::<Person>(agent).is_some());
        assert!(world.app().world().get::<Agent>(agent).is_some());

        // Logic components needed for the brain pipeline
        assert!(world.app().world().get::<MindGraph>(agent).is_some());
        assert!(world.app().world().get::<PhysicalNeeds>(agent).is_some());
        assert!(
            world
                .app()
                .world()
                .get::<crate::agent::brains::proposal::BrainState>(agent)
                .is_some()
        );
    }

    #[test]
    fn spawn_agent_uses_config_values() {
        let mut world = TestWorld::with_seed(42);
        let agent = world.spawn_agent(AgentConfig {
            pos: Vec2::new(50.0, 75.0),
            metabolism: crate::agent::body::metabolism::Metabolism::at_urgency(0.8),
            stamina: 25.0,
            ..Default::default()
        });

        assert!((world.agent_hunger(agent) - 0.8).abs() < 1e-4);
        assert_eq!(world.agent_aerobic(agent), 25.0);
        let transform = world.get::<Transform>(agent);
        assert_eq!(transform.translation.x, 50.0);
        assert_eq!(transform.translation.y, 75.0);
    }

    #[test]
    fn spawn_agent_cluster_returns_n_agents_near_center() {
        let mut world = TestWorld::with_seed(42);
        let center = Vec2::new(100.0, 100.0);
        let agents = world.spawn_agent_cluster(9, center);
        assert_eq!(agents.len(), 9);

        // All agents should be within a small radius of the center.
        for agent in &agents {
            let pos = world.get::<Transform>(*agent).translation.truncate();
            assert!(pos.distance(center) < 50.0);
        }
    }

    #[test]
    fn spawn_berry_bush_starts_with_berry_inventory() {
        let mut world = TestWorld::with_seed(42);
        let bush = world.spawn_berry_bush(Vec2::new(10.0, 10.0), 5);
        assert_eq!(world.item_count(bush, Concept::Berry), 5);
    }

    #[test]
    fn spawn_apple_tree_starts_with_apple_inventory() {
        let mut world = TestWorld::with_seed(42);
        let tree = world.spawn_apple_tree(Vec2::new(20.0, 20.0), 7);
        assert_eq!(world.item_count(tree, Concept::Apple), 7);
    }

    #[test]
    fn spawn_stone_node_starts_with_stone_inventory() {
        let mut world = TestWorld::with_seed(42);
        let node = world.spawn_stone_node(Vec2::new(30.0, 30.0), 5);
        assert_eq!(world.item_count(node, Concept::Stone), 5);
    }

    #[test]
    fn spawn_wood_log_starts_with_wood_inventory() {
        let mut world = TestWorld::with_seed(42);
        let log = world.spawn_wood_log(Vec2::new(40.0, 40.0), 4);
        assert_eq!(world.item_count(log, Concept::Wood), 4);
    }

    #[test]
    fn spawn_deer_creates_agent_with_dangerous_person_belief() {
        let mut world = TestWorld::with_seed(42);
        let deer = world.spawn_deer(Vec2::new(40.0, 40.0));
        assert!(world.app().world().get::<Agent>(deer).is_some());
        assert!(
            world
                .app()
                .world()
                .get::<crate::world::deer::Deer>(deer)
                .is_some()
        );

        // The deer should know persons are dangerous (loaded at spawn).
        let mind = world.get::<MindGraph>(deer);
        let dangerous = mind.query(
            Some(&MindNode::Concept(Concept::Person)),
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Dangerous)),
        );
        assert!(
            !dangerous.is_empty(),
            "deer should believe Person is Dangerous"
        );
    }

    #[test]
    fn distance_returns_euclidean_distance_between_entities() {
        let mut world = TestWorld::with_seed(42);
        let a = world.spawn_agent(AgentConfig::at(Vec2::new(0.0, 0.0)));
        let b = world.spawn_agent(AgentConfig::at(Vec2::new(3.0, 4.0)));
        assert_eq!(world.distance(a, b), 5.0);
    }

    #[test]
    fn entity_exists_reflects_world_state() {
        let mut world = TestWorld::with_seed(42);
        let agent = world.spawn_agent(AgentConfig::default());
        assert!(world.entity_exists(agent));

        world.app_mut().world_mut().despawn(agent);
        assert!(!world.entity_exists(agent));
    }

    #[test]
    fn all_agents_returns_only_agent_marker_entities() {
        let mut world = TestWorld::with_seed(42);
        let person = world.spawn_agent(AgentConfig::default());
        let deer = world.spawn_deer(Vec2::new(20.0, 20.0));
        let _bush = world.spawn_berry_bush(Vec2::new(30.0, 30.0), 3);

        let agents = world.all_agents();
        assert_eq!(agents.len(), 2);
        assert!(agents.contains(&person));
        assert!(agents.contains(&deer));
    }

    #[test]
    fn registered_actions_include_core_action_set() {
        let world = TestWorld::with_seed(42);
        for action in [
            ActionType::Eat,
            ActionType::Sleep,
            ActionType::Walk,
            ActionType::Harvest,
            ActionType::Wander,
            ActionType::Converse,
        ] {
            assert!(
                world.has_registered_action(action),
                "expected {action:?} to be registered"
            );
        }
    }

    #[test]
    fn config_with_pre_loaded_knowledge_is_applied_to_mind() {
        use crate::agent::mind::knowledge::{Metadata, Triple};

        let mut world = TestWorld::with_seed(42);
        let agent = world.spawn_agent(AgentConfig {
            knowledge: vec![Triple::with_meta(
                MindNode::Concept(Concept::AppleTree),
                Predicate::Produces,
                Value::Item(Concept::Apple, 1),
                Metadata::semantic(0),
            )],
            ..Default::default()
        });

        let mind = world.get::<MindGraph>(agent);
        let triples = mind.query(
            Some(&MindNode::Concept(Concept::AppleTree)),
            Some(Predicate::Produces),
            None,
        );
        assert!(
            !triples.is_empty(),
            "pre-loaded AppleTree-produces-Apple knowledge should be present"
        );
    }

    #[test]
    fn ticking_runs_brain_pipeline_without_panicking() {
        // This is the smoke test that proves the full system stack is wired up.
        // A bare agent with default needs should be tickable for many frames
        // without any system panicking on missing resources or components.
        let mut world = TestWorld::with_seed(42);
        let _ = world.spawn_agent(AgentConfig {
            metabolism: crate::agent::body::metabolism::Metabolism::at_urgency(0.5),
            ..Default::default()
        });
        world.spawn_apple_tree(Vec2::new(20.0, 20.0), 10);
        world.tick(30);
        assert_eq!(world.current_tick(), 30);
    }

    // ─── Inspection method tests ──────────────────────────────────────────

    #[test]
    fn print_agent_state_does_not_panic() {
        let mut world = TestWorld::with_seed(42);
        let agent = world.spawn_agent(AgentConfig {
            metabolism: crate::agent::body::metabolism::Metabolism::at_urgency(0.6),
            stamina: 40.0,
            ..Default::default()
        });
        world.tick(5);
        // Should not panic; output goes to stderr.
        world.print_agent_state(agent);
    }

    #[test]
    fn print_brain_decision_does_not_panic() {
        let mut world = TestWorld::with_seed(42);
        let agent = world.spawn_agent(AgentConfig {
            metabolism: crate::agent::body::metabolism::Metabolism::at_urgency(0.8),
            ..Default::default()
        });
        world.spawn_apple_tree(Vec2::new(20.0, 20.0), 10);
        // Tick past the brain thinking_interval (60 ticks) so a decision is made.
        world.tick(65);
        world.print_brain_decision(agent);
    }

    #[test]
    fn print_mind_graph_does_not_panic() {
        let mut world = TestWorld::with_seed(42);
        let agent = world.spawn_agent(AgentConfig::default());
        world.tick(5);
        world.print_mind_graph(agent);
    }

    #[test]
    fn print_relationships_does_not_panic() {
        let mut world = TestWorld::with_seed(42);
        let agent = world.spawn_agent(AgentConfig::at(Vec2::new(10.0, 10.0)));
        world.spawn_agent(AgentConfig::at(Vec2::new(12.0, 10.0)));
        world.tick(50);
        world.print_relationships(agent);
    }

    #[test]
    fn print_conversation_does_not_panic_when_not_in_conversation() {
        let mut world = TestWorld::with_seed(42);
        let agent = world.spawn_agent(AgentConfig::default());
        world.print_conversation(agent);
    }

    #[test]
    fn query_knowledge_returns_matching_triples() {
        use crate::agent::mind::knowledge::{Metadata, Triple};

        let mut world = TestWorld::with_seed(42);
        let agent = world.spawn_agent(AgentConfig {
            knowledge: vec![Triple::with_meta(
                MindNode::Concept(Concept::AppleTree),
                Predicate::Produces,
                Value::Item(Concept::Apple, 1),
                Metadata::semantic(0),
            )],
            ..Default::default()
        });

        let results = world.query_knowledge(agent, "Apple");
        assert!(
            !results.is_empty(),
            "query for 'Apple' should match AppleTree-Produces-Apple triple"
        );
    }

    #[test]
    fn query_knowledge_returns_empty_for_no_match() {
        let mut world = TestWorld::with_seed(42);
        let agent = world.spawn_agent(AgentConfig::default());
        let results = world.query_knowledge(agent, "xyzzy_no_match");
        assert!(results.is_empty());
    }

    #[test]
    fn print_recent_events_does_not_panic_with_no_events() {
        let world = TestWorld::with_seed(42);
        world.print_recent_events(10);
    }

    #[test]
    fn print_recent_events_shows_events_after_ticking() {
        let mut world = TestWorld::with_seed(42);
        let agent = world.spawn_agent(AgentConfig {
            metabolism: crate::agent::body::metabolism::Metabolism::at_urgency(0.5),
            ..Default::default()
        });
        world.spawn_apple_tree(Vec2::new(20.0, 20.0), 10);
        world.tick(100);

        // Should not panic and the log should have events.
        let log = world.app().world().resource::<SimEventLog>();
        assert!(
            !log.events.is_empty(),
            "SimEventLog should collect events after ticking"
        );

        world.print_recent_events(100);
        world.print_agent_events(agent, 100);
    }

    #[test]
    fn print_agent_events_filters_to_agent() {
        let mut world = TestWorld::with_seed(42);
        let agent_a = world.spawn_agent(AgentConfig::at(Vec2::new(0.0, 0.0)));
        let _agent_b = world.spawn_agent(AgentConfig::at(Vec2::new(200.0, 200.0)));
        world.tick(100);

        // Should not panic.
        world.print_agent_events(agent_a, 100);
    }

    // ─── game_defaults tests ──────────────────────────────────────────────

    #[test]
    fn game_defaults_spawns_correct_agent_count() {
        let mut world = TestWorld::game_defaults(42);
        let game_config = WorldSpawnConfig::game_defaults();
        // The second human group is best-effort: it only spawns when a
        // suitable opposite-bank settlement is found. Assert within a range
        // so the test tolerates seeds where the second settlement fails.
        let total = world.all_agents().len();
        let min = game_config.humans + game_config.deer + game_config.wolves;
        let max = min + game_config.second_humans;
        assert!(
            total >= min && total <= max,
            "expected {min}..={max} agents, got {total}"
        );
    }

    #[test]
    fn game_defaults_humans_match_game_config() {
        let mut world = TestWorld::game_defaults(42);
        let game_config = WorldSpawnConfig::game_defaults();
        let all = world.all_agents();
        let humans: Vec<_> = all
            .iter()
            .filter(|&&e| world.app().world().get::<crate::agent::Person>(e).is_some())
            .collect();
        // Humans = first group + (possibly) second group across the river.
        let min = game_config.humans;
        let max = game_config.humans + game_config.second_humans;
        assert!(
            humans.len() >= min && humans.len() <= max,
            "expected {min}..={max} humans, got {}",
            humans.len()
        );
    }

    #[test]
    fn game_defaults_same_seed_produces_same_positions() {
        let mut world_a = TestWorld::game_defaults(42);
        let mut world_b = TestWorld::game_defaults(42);

        let agents_a = world_a.all_agents();
        let agents_b = world_b.all_agents();

        let positions_a: Vec<_> = agents_a
            .iter()
            .map(|&e| world_a.get::<Transform>(e).translation)
            .collect();
        let positions_b: Vec<_> = agents_b
            .iter()
            .map(|&e| world_b.get::<Transform>(e).translation)
            .collect();

        assert_eq!(positions_a, positions_b);
    }

    #[test]
    fn apply_spawn_layout_places_all_entity_types() {
        use crate::world::spawn_config::{SpawnAlgorithm, WorldSpawnConfig};
        let mut world = TestWorld::new();
        let config = WorldSpawnConfig {
            humans: 2,
            deer: 1,
            wolves: 0,
            berry_bushes: 3,
            apple_trees: 2,
            seed: 7,
            spawn_algorithm: SpawnAlgorithm::Uniform,
            ..WorldSpawnConfig::game_defaults()
        };
        let layout = {
            let map = world.app().world().resource::<WorldMap>();
            config.compute_layout(map)
        };
        world.apply_spawn_layout(&layout);

        let agents = world.all_agents();
        // 2 humans + 1 deer = 3 agents
        assert_eq!(agents.len(), 3);
    }

    #[test]
    fn wolf_has_innate_prey_knowledge() {
        let mut world = TestWorld::with_seed(42);
        let wolf = world.spawn_wolf(Vec2::new(40.0, 40.0));

        let mind = world.get::<MindGraph>(wolf);

        // Wolves know deer are prey and yield meat. The "meat is food" link
        // lives in the shared ontology, so the wolf doesn't need to assert
        // it directly — the planner walks the IsA chain through Meat → Food.
        let prey = mind.query(
            Some(&MindNode::Concept(Concept::Deer)),
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Prey)),
        );
        assert!(
            !prey.is_empty(),
            "wolf should know Deer HasTrait Prey intrinsically"
        );

        let produces = mind.query(
            Some(&MindNode::Concept(Concept::Deer)),
            Some(Predicate::Produces),
            Some(&Value::Item(Concept::Meat, 1)),
        );
        assert!(
            !produces.is_empty(),
            "wolf should know Deer Produces Meat intrinsically"
        );

        assert!(
            mind.is_a(&MindNode::Concept(Concept::Meat), Concept::Food),
            "shared ontology should classify Meat IsA Food"
        );
    }

    #[test]
    fn wolf_has_no_triggers_emotion_triples() {
        let mut world = TestWorld::with_seed(42);
        let wolf = world.spawn_wolf(Vec2::new(40.0, 40.0));

        let mind = world.get::<MindGraph>(wolf);
        let triples = mind.query(None, Some(Predicate::TriggersEmotion), None);
        assert!(
            triples.is_empty(),
            "wolf MindGraph should contain no TriggersEmotion triples — emotions emerge from drives, not scripts"
        );
    }

    #[test]
    fn wolf_knows_humans_are_dangerous() {
        let mut world = TestWorld::with_seed(42);
        let wolf = world.spawn_wolf(Vec2::new(40.0, 40.0));

        let mind = world.get::<MindGraph>(wolf);
        let triples = mind.query(
            Some(&MindNode::Concept(Concept::Person)),
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Dangerous)),
        );
        assert!(
            !triples.is_empty(),
            "wolf should have innate belief that humans are dangerous"
        );
    }

    #[test]
    fn wolf_marks_spawn_tile_as_territory() {
        use crate::world::map::TILE_SIZE;

        let pos = Vec2::new(80.0, 64.0);
        let mut world = TestWorld::with_seed(42);
        let wolf = world.spawn_wolf(pos);

        let expected_tile = ((pos.x / TILE_SIZE) as i32, (pos.y / TILE_SIZE) as i32);
        let mind = world.get::<MindGraph>(wolf);
        let triples = mind.query(
            Some(&MindNode::Tile(expected_tile)),
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Territory)),
        );
        assert!(
            !triples.is_empty(),
            "wolf should mark its spawn tile as territory"
        );
    }

    // ─── Genetics tests ────────────────────────────────────────────────────────

    #[test]
    fn deer_has_genome_at_spawn_and_phenotype_after_tick() {
        use crate::agent::body::genetics::genome::Genome;
        use crate::agent::body::genetics::phenotype::Phenotype;

        let mut world = TestWorld::with_seed(42);
        let deer = world.spawn_deer(Vec2::new(40.0, 40.0));

        // Genome is present at spawn time.
        assert!(
            world.app().world().get::<Genome>(deer).is_some(),
            "spawned deer should have Genome component"
        );

        // Phenotype is computed by develop_phenotype_system on the first tick.
        world.tick(1);
        assert!(
            world.app().world().get::<Phenotype>(deer).is_some(),
            "deer should have Phenotype after the first tick"
        );
    }

    #[test]
    fn neutral_genome_produces_baseline_phenotype_on_tick() {
        use crate::agent::body::genetics::phenotype::Phenotype;

        let mut world = TestWorld::with_seed(42);
        let deer = world.spawn_deer(Vec2::new(40.0, 40.0));

        // Neutral (default) genome → develop_phenotype_system writes the
        // species-baseline phenotype on tick 1.
        world.tick(1);

        let phenotype = world.get::<Phenotype>(deer);
        assert!(
            (phenotype.speed - 1.0).abs() < 1e-5,
            "neutral genome should produce speed=1.0, got {}",
            phenotype.speed
        );
        assert!(
            (phenotype.vision - 1.0).abs() < 1e-5,
            "neutral genome should produce vision=1.0, got {}",
            phenotype.vision
        );
    }

    #[test]
    fn positive_speed_genome_makes_deer_faster_than_baseline() {
        use crate::agent::body::genetics::builder::physical;
        use crate::agent::body::genetics::phenotype::Phenotype;

        let mut world = TestWorld::with_seed(42);

        let slow_deer = world.spawn_deer(Vec2::new(40.0, 40.0));
        let fast_deer = world.spawn_deer_with(Vec2::new(60.0, 60.0), physical().speed(1.3));

        world.tick(1);

        let fast_phenotype = world.get::<Phenotype>(fast_deer);
        let slow_phenotype = world.get::<Phenotype>(slow_deer);

        assert!(
            fast_phenotype.speed > slow_phenotype.speed,
            "fast deer (speed={}) should be faster than slow deer (speed={})",
            fast_phenotype.speed,
            slow_phenotype.speed
        );
    }

    #[test]
    fn personality_is_derived_from_genome_after_tick() {
        use crate::agent::body::genetics::builder::personality;
        use crate::agent::psyche::personality::Personality;

        let mut world = TestWorld::with_seed(42);

        let deer = world.spawn_deer_with(Vec2::new(40.0, 40.0), personality().openness(0.75));
        world.tick(1);

        let p = world.get::<Personality>(deer);
        assert!(
            p.traits.openness > 0.6,
            "high openness genome should produce openness > 0.6, got {}",
            p.traits.openness
        );
    }

    #[test]
    fn drives_are_derived_from_genome_personality() {
        use crate::agent::body::genetics::builder::personality;
        use crate::agent::body::needs::PsychologicalDrives;

        let mut world = TestWorld::with_seed(42);

        let deer = world.spawn_deer_with(Vec2::new(40.0, 40.0), personality().extraversion(0.75));
        world.tick(1);

        let drives = world.get::<PsychologicalDrives>(deer);
        assert!(
            drives.companionship < 0.4,
            "extrovert genome should yield low baseline companionship (waking up lonely), got {}",
            drives.companionship
        );
    }

    #[test]
    fn wolf_pack_bonds_established_at_spawn() {
        let mut world = TestWorld::with_seed(42);
        let wolves = world.spawn_wolf_pack(&[Vec2::new(40.0, 40.0), Vec2::new(50.0, 50.0)]);
        let (wolf_a, wolf_b) = (wolves[0], wolves[1]);

        // wolf_a should know wolf_b as a friend with high trust
        let mind_a = world.get::<MindGraph>(wolf_a);
        let trust = mind_a.query(
            Some(&MindNode::Entity(wolf_b)),
            Some(Predicate::Trust),
            None,
        );
        assert!(
            !trust.is_empty(),
            "wolf_a should have a Trust triple for wolf_b"
        );

        // wolf_b should know wolf_a as a friend
        let mind_b = world.get::<MindGraph>(wolf_b);
        let friend = mind_b.query(
            Some(&MindNode::Entity(wolf_a)),
            Some(Predicate::IsA),
            Some(&Value::Concept(Concept::Friend)),
        );
        assert!(!friend.is_empty(), "wolf_b should know wolf_a as a Friend");
    }
}
