//! Perception: multi-sense detection of nearby entities and environmental signals.
//!
//! Reads: Transform, Vision, LightLevel, Physical entities, body state components, TickCount, SpatialIndex, HeatSource, SoundSource
//! Writes: VisibleObjects (entity list), PerceptionCache (chunk-bucket query cache), MindGraph (triples tagged with source_sense), SimEvent::{EntityPerceived, WarmthPerceived, SoundPerceived}
//! Upstream: world::map (tile/chunk data), world::environment (LightLevel), world::sense_sources, agent body state
//! Downstream: brain_system (reads VisibleObjects), knowledge (MindGraph updated with percepts), SimEvent consumers

use crate::agent::Agent;
use crate::agent::events::SimEventKind;
use crate::agent::mind::knowledge::{
    CardinalDirection, Concept, Metadata, MindGraph, Node, Predicate, Sense, Triple, Value,
};
use crate::core::GameLog;
use crate::core::tick::TickCount;
use crate::world::environment::LightLevel;
use crate::world::map::{CHUNK_SIZE, TILE_SIZE};
use crate::world::property::HeatSource;
use crate::world::sense_sources::SoundSource;
use crate::world::spatial_index::{SpatialIndex, chunk_radius_for, world_pos_to_chunk};
use bevy::prelude::*;
use smallvec::SmallVec;
use std::collections::HashMap;

// ═══════════════════════════════════════════════════════════════════════════
// VISION COMPONENTS
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Component, Reflect, Default)]
#[reflect(Component)]
#[require(PerceptionCache)]
pub struct Vision {
    pub range: f32,
}

#[derive(Component, Default)]
pub struct VisibleObjects {
    /// Every entity the agent can see this tick — typed or untyped.
    /// Use this when the consumer needs every visible thing regardless
    /// of `EntityType` (e.g. `write_perceptions_to_mind` writes
    /// per-entity belief triples for everything in view). Use
    /// [`Self::by_concept`] / [`Self::iter_by_concept`] instead when
    /// the consumer filters by concept-level trait — same data, lets
    /// each `mind.has_trait` query run once per concept instead of
    /// once per entity.
    pub entities: Vec<Entity>,
    /// Visible entities grouped by their world `EntityType` concept.
    /// Built once per tick by `update_visual_perception`. Entities
    /// without an `EntityType` component are absent from these
    /// buckets — they still appear in [`Self::entities`].
    pub by_concept: HashMap<Concept, SmallVec<[Entity; 4]>>,
}

impl VisibleObjects {
    /// Iterate visible entities whose concept matches `predicate`.
    /// Lets consumers (recognition, social perception) walk only the
    /// concept buckets they care about — typically "is this concept an
    /// agent species" via `mind.has_trait(_, Sentient)` — without
    /// re-checking every visible entity.
    pub fn iter_by_concept<'a>(
        &'a self,
        mut predicate: impl FnMut(Concept) -> bool + 'a,
    ) -> impl Iterator<Item = Entity> + 'a {
        self.by_concept
            .iter()
            .filter(move |(c, _)| predicate(**c))
            .flat_map(|(_, ents)| ents.iter().copied())
    }
}

/// Per-agent cache of the raw `SpatialIndex::entities_near` result, keyed by
/// `(center_chunk, chunk_radius)` — the exact pair that determines which chunk
/// buckets the query scans. Despawns are absorbed by the precise-distance pass
/// (failed `Transform` fetch); spawns are bounded by `SAFETY_REFRESH_TICKS`.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct PerceptionCache {
    last_chunk: Option<IVec2>,
    last_chunk_radius: i32,
    last_query_tick: u64,
    cached: Vec<Entity>,
}

/// Maximum ticks the cache may serve a stale chunk-bucket result before being forced
/// to re-query. Bounds the latency for a newly spawned nearby entity to enter
/// perception when the agent itself hasn't moved between chunks. At 60 ticks/second
/// this is 0.5s real time.
const SAFETY_REFRESH_TICKS: u64 = 30;

impl PerceptionCache {
    fn is_stale(&self, agent_chunk: IVec2, chunk_radius: i32, now: u64) -> bool {
        // Empty cache: re-query every tick. Cheap (HashMap miss on empty chunk buckets)
        // and avoids locking in a transient empty result — e.g. on the very first frame
        // before `update_spatial_index` has populated the index, or for an agent in an
        // empty zone where any new neighbor would otherwise wait the full safety window.
        self.cached.is_empty()
            || self.last_chunk != Some(agent_chunk)
            || self.last_chunk_radius != chunk_radius
            || now.saturating_sub(self.last_query_tick) >= SAFETY_REFRESH_TICKS
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// VISUAL PERCEPTION — Detect entities in range
// ═══════════════════════════════════════════════════════════════════════════

pub fn update_visual_perception(
    mut agents: Query<
        (
            Entity,
            &Transform,
            &Vision,
            &mut VisibleObjects,
            &mut PerceptionCache,
        ),
        With<Agent>,
    >,
    transforms: Query<&Transform, With<crate::world::Physical>>,
    entity_types: Query<&crate::agent::inventory::EntityType>,
    spatial_index: Res<SpatialIndex>,
    light_level: Res<LightLevel>,
    mut _game_log: ResMut<GameLog>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
    mut previous_buf: Local<Vec<Entity>>,
) {
    let _start = std::time::Instant::now();

    for (agent_entity, agent_transform, vision, mut visible_objects, mut cache) in agents.iter_mut()
    {
        // Swap the previous-tick visible list out without allocating; both buffers stabilise
        // at their max size after a warmup tick or two.
        std::mem::swap(&mut *previous_buf, &mut visible_objects.entities);
        visible_objects.entities.clear();
        for bucket in visible_objects.by_concept.values_mut() {
            bucket.clear();
        }

        let agent_pos = agent_transform.translation.truncate();
        let view_range = vision.range * light_level.0;

        let agent_chunk = world_pos_to_chunk(agent_pos);
        let chunk_radius = chunk_radius_for(view_range);
        if cache.is_stale(agent_chunk, chunk_radius, tick.current) {
            cache.cached = spatial_index.entities_near(agent_pos, view_range);
            cache.last_chunk = Some(agent_chunk);
            cache.last_chunk_radius = chunk_radius;
            cache.last_query_tick = tick.current;
        }

        // Precise distance pass against the cached candidates. Despawned entities fall
        // out here because `transforms.get` returns Err for them.
        for &entity in &cache.cached {
            if entity == agent_entity {
                continue;
            }

            if let Ok(target_transform) = transforms.get(entity) {
                let target_pos = target_transform.translation.truncate();
                if agent_pos.distance(target_pos) <= view_range {
                    visible_objects.entities.push(entity);
                    if let Ok(entity_type) = entity_types.get(entity) {
                        visible_objects
                            .by_concept
                            .entry(entity_type.0)
                            .or_default()
                            .push(entity);
                    }
                }
            }
        }

        visible_objects
            .by_concept
            .retain(|_, ents| !ents.is_empty());

        // Emit EntityPerceived for newly visible entities
        for &entity in &visible_objects.entities {
            if !previous_buf.contains(&entity) {
                sim_events.write(crate::agent::events::SimEvent::single(
                    tick.current,
                    agent_entity,
                    SimEventKind::EntityPerceived {
                        agent: agent_entity,
                        target: entity,
                    },
                ));
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// BODY STATE PERCEPTION
// ═══════════════════════════════════════════════════════════════════════════

pub fn update_body_perception(
    mut agents: Query<
        (
            Entity,
            &crate::agent::body::needs::Consciousness,
            &Transform,
            &mut MindGraph,
        ),
        With<Agent>,
    >,
    tick: Res<TickCount>,
) {
    let current_time = tick.current;

    for (_entity, consciousness, transform, mut mind) in agents.iter_mut() {
        // Rule 1: Location
        let pos = transform.translation.truncate();
        let tile_x = (pos.x / TILE_SIZE).floor() as i32;
        let tile_y = (pos.y / TILE_SIZE).floor() as i32;
        mind.perceive_self(
            Predicate::LocatedAt,
            Value::Tile((tile_x, tile_y)),
            current_time,
        );

        // Rule 2: Explored Areas (Semantic Memory)
        let chunk_x = (pos.x / (CHUNK_SIZE as f32 * TILE_SIZE)).floor() as i32;
        let chunk_y = (pos.y / (CHUNK_SIZE as f32 * TILE_SIZE)).floor() as i32;

        mind.assert(Triple::with_meta(
            Node::Chunk((chunk_x, chunk_y)),
            Predicate::Explored,
            Value::Boolean(true),
            Metadata::semantic(current_time),
        ));

        // Rule 4: Consciousness
        let is_awake = consciousness.alertness > 0.2;
        let trait_val = if is_awake {
            Concept::Awake
        } else {
            Concept::Asleep
        };

        // Remove stale
        let old_trait = if is_awake {
            Concept::Asleep
        } else {
            Concept::Awake
        };
        mind.remove(
            &Node::Self_,
            Predicate::HasTrait,
            &Value::Concept(old_trait),
        );

        mind.perceive_self(Predicate::HasTrait, Value::Concept(trait_val), current_time);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// EXTERNAL PERCEPTION
// ═══════════════════════════════════════════════════════════════════════════

pub fn write_perceptions_to_mind(
    mut agents: Query<(Entity, &Name, &Transform, &VisibleObjects, &mut MindGraph), With<Agent>>,
    transforms: Query<&Transform>,
    inventories: Query<&crate::agent::item_slots::ItemSlots>,
    entity_types: Query<&crate::agent::inventory::EntityType>,
    becomes_components: Query<&crate::world::becomes::Becomes>,
    lame_entities: Query<(), With<crate::agent::Lame>>,
    tick: Res<TickCount>,
) {
    let current_time = tick.current;

    for (agent_entity, _, agent_transform, visible, mut mind) in agents.iter_mut() {
        let agent_pos = agent_transform.translation.truncate();

        for &entity in &visible.entities {
            let confidence = calc_confidence(agent_pos, transforms.get(entity).ok());

            // 1. Perceive Location
            if let Ok(transform) = transforms.get(entity) {
                let pos = transform.translation.truncate();
                let tile_x = (pos.x / TILE_SIZE).floor() as i32;
                let tile_y = (pos.y / TILE_SIZE).floor() as i32;

                mind.perceive_entity(
                    entity,
                    Predicate::LocatedAt,
                    Value::Tile((tile_x, tile_y)),
                    current_time,
                    confidence,
                );
            }

            // 2. Perceive Inventory
            if let Ok(inventory) = inventories.get(entity) {
                perceive_inventory(
                    entity,
                    inventory,
                    &mut mind,
                    current_time,
                    confidence,
                    false,
                );
            }

            // 3. Perceive Type
            if let Ok(entity_type) = entity_types.get(entity) {
                mind.perceive_entity(
                    entity,
                    Predicate::IsA,
                    Value::Concept(entity_type.0),
                    current_time,
                    confidence,
                );
            }

            // 4. Perceive Becomes rule (#61): if the entity has a world `Becomes`
            // component, the observer learns "this thing will turn into that thing".
            // This is the agent's *belief* about a transformation rule, not the
            // rule itself — the world component fires regardless of who knows.
            if let Ok(becomes) = becomes_components.get(entity) {
                mind.perceive_entity(
                    entity,
                    Predicate::Becomes,
                    Value::Concept(becomes.target),
                    current_time,
                    confidence,
                );
            }

            // 5. Perceive Lame status. Predator target enumeration reads
            // this trait to weigh wounded prey above healthy prey.
            if lame_entities.get(entity).is_ok() {
                mind.perceive_entity(
                    entity,
                    Predicate::HasTrait,
                    Value::Concept(Concept::Lame),
                    current_time,
                    confidence,
                );
            }
        }

        // 4. Perceive Self Inventory
        if let Ok(self_inventory) = inventories.get(agent_entity) {
            perceive_inventory(
                agent_entity,
                self_inventory,
                &mut mind,
                current_time,
                1.0,
                true,
            );
        }
    }
}

// --- HELPERS ---

fn calc_confidence(agent_pos: Vec2, targeted_transform: Option<&Transform>) -> f32 {
    targeted_transform.map_or(0.5, |t| {
        let dist = agent_pos.distance(t.translation.truncate());
        (1.0 - (dist / 256.0).min(1.0)).max(0.3)
    })
}

fn perceive_inventory(
    entity: Entity,
    inventory: &crate::agent::item_slots::ItemSlots,
    mind: &mut MindGraph,
    time: u64,
    confidence: f32,
    is_self: bool,
) {
    let subject_node = if is_self {
        Node::Self_
    } else {
        Node::Entity(entity)
    };
    let mut observed_concepts = std::collections::HashSet::new();

    // 1. Record what IS there.
    // Other entities use Semantic (same as IsA) so the belief persists
    // until overwritten by fresh observation.
    for (concept, qty) in inventory.group_by_concept() {
        observed_concepts.insert(concept);
        let meta = if is_self {
            Metadata::perception_with_conf(time, confidence)
        } else {
            Metadata::semantic(time)
        };
        mind.assert(Triple::with_meta(
            subject_node.clone(),
            Predicate::Contains,
            Value::Item(concept, qty),
            meta,
        ));
    }

    // 2. Clear what IS NOT there (but used to be).
    //
    // For self-inventory, drop the stale belief — "I no longer have apples"
    // is cleanly expressed by an absent Contains triple.
    //
    // For other entities, replace it with an explicit
    // `Contains(concept, 0)` zero-quantity belief. The agent saw the entity
    // *and* saw it didn't have the thing — first-person negative evidence,
    // not absence of evidence. Without this, the planner's
    // `is_known_empty` check can't tell "I saw it run dry" apart from
    // "I haven't perceived its inventory yet," and the type-level
    // `Produces` fallback re-includes depleted resources every cycle.
    let beliefs = mind.query(Some(&subject_node), Some(Predicate::Contains), None);
    let stale: Vec<_> = beliefs
        .into_iter()
        .filter_map(|t| {
            if let Value::Item(c, q) = t.object {
                // Already-zero beliefs are stable: skipping them avoids a
                // remove + re-assert loop every perception tick for every
                // previously-depleted entity in view.
                if q != 0 && !observed_concepts.contains(&c) {
                    return Some((c, q));
                }
            }
            None
        })
        .collect();

    for (concept, old_qty) in stale {
        mind.remove(
            &subject_node,
            Predicate::Contains,
            &Value::Item(concept, old_qty),
        );
        if !is_self {
            mind.assert(Triple::with_meta(
                subject_node.clone(),
                Predicate::Contains,
                Value::Item(concept, 0),
                Metadata::semantic(time),
            ));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// WATER PERCEPTION — Detect water tiles in vision range
// ═══════════════════════════════════════════════════════════════════════════

/// Water tiles are static terrain — scan infrequently (every 30 ticks per agent).
pub fn perceive_water_tiles(
    mut agents: Query<(Entity, &Transform, &Vision, &mut MindGraph), With<Agent>>,
    world_map: Res<crate::world::map::WorldMap>,
    light_level: Res<LightLevel>,
    tick: Res<TickCount>,
) {
    let current_time = tick.current;

    for (entity, transform, vision, mut mind) in agents.iter_mut() {
        if !tick.should_run(entity, 30) {
            continue;
        }

        let pos = transform.translation.truncate();
        let view_range = vision.range * light_level.0;
        let tile_range = (view_range / TILE_SIZE).ceil() as i32;

        let center_tx = (pos.x / TILE_SIZE).floor() as i32;
        let center_ty = (pos.y / TILE_SIZE).floor() as i32;

        for dx in -tile_range..=tile_range {
            for dy in -tile_range..=tile_range {
                let tx = center_tx + dx;
                let ty = center_ty + dy;
                if tx < 0 || ty < 0 {
                    continue;
                }

                let tile_world = world_map.tile_to_world(tx, ty);
                if pos.distance(tile_world) > view_range {
                    continue;
                }

                if let Some(tile_type) = world_map.get_tile(tx as u32, ty as u32)
                    && tile_type.is_water()
                {
                    mind.assert(Triple::with_meta(
                        Node::Tile((tx, ty)),
                        Predicate::HasTrait,
                        Value::Concept(Concept::Drinkable),
                        Metadata::semantic(current_time),
                    ));
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// GRASS PERCEPTION — Detect grazable tiles for herbivores
// ═══════════════════════════════════════════════════════════════════════════

/// Grass tiles are static terrain — scan infrequently (every 30 ticks per agent).
///
/// Gated to herbivores so the planner only considers grazing for species that
/// would actually do it. Carnivores and omnivores never get `HasTrait Grazable`
/// asserted, keeping their MindGraph free of useless noise and their rational
/// brain from enumerating grass tiles as food candidates.
pub fn perceive_grass_tiles(
    mut agents: Query<
        (
            Entity,
            &Transform,
            &Vision,
            &crate::agent::body::species::SpeciesProfile,
            &mut MindGraph,
        ),
        With<Agent>,
    >,
    world_map: Res<crate::world::map::WorldMap>,
    light_level: Res<LightLevel>,
    tick: Res<TickCount>,
) {
    use crate::agent::body::species::Diet;
    use crate::world::map::TileType;

    let current_time = tick.current;

    for (entity, transform, vision, species, mut mind) in agents.iter_mut() {
        if !matches!(species.diet, Diet::Herbivore) {
            continue;
        }
        if !tick.should_run(entity, 30) {
            continue;
        }

        let pos = transform.translation.truncate();
        let view_range = vision.range * light_level.0;
        let tile_range = (view_range / TILE_SIZE).ceil() as i32;

        let center_tx = (pos.x / TILE_SIZE).floor() as i32;
        let center_ty = (pos.y / TILE_SIZE).floor() as i32;

        for dx in -tile_range..=tile_range {
            for dy in -tile_range..=tile_range {
                let tx = center_tx + dx;
                let ty = center_ty + dy;
                if tx < 0 || ty < 0 {
                    continue;
                }

                let tile_world = world_map.tile_to_world(tx, ty);
                if pos.distance(tile_world) > view_range {
                    continue;
                }

                if let Some(TileType::Grass) = world_map.get_tile(tx as u32, ty as u32) {
                    mind.assert(Triple::with_meta(
                        Node::Tile((tx, ty)),
                        Predicate::HasTrait,
                        Value::Concept(Concept::Grazable),
                        Metadata::semantic(current_time),
                    ));
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// DANGER PERCEPTION — Contextual threat assessment produces Fear
// ═══════════════════════════════════════════════════════════════════════════

/// Base fear intensity for a single dangerous entity, before contextual modulation.
/// Preserves the previous hardcoded value for a typical healthy, unarmed, calm agent.
const BASE_THREAT: f32 = 0.8;

/// Computes how threatening a single dangerous entity is *to this particular agent*,
/// producing a fear intensity in `[0.0, 1.0]`.
///
/// The factors reflect the acceptance criteria from #29:
/// - Combat capability: weapons and health reduce fear
/// - Personality: neuroticism amplifies fear, emotional stability dampens it
/// - Desperation: high unmet physical needs reduce fear so other urgencies can
///   compete in arbitration (a starving agent is more willing to approach danger)
///
/// Not yet modelled (documented as TODOs in the issue): allies vs. threats count
/// and escape-route analysis. Those require perception of packmates/terrain that
/// isn't wired through this system yet.
fn assess_threat(
    personality: &crate::agent::psyche::personality::Personality,
    needs: &crate::agent::body::needs::PhysicalNeeds,
    body_health: f32,
    items: Option<&crate::agent::item_slots::ItemSlots>,
) -> f32 {
    // Neuroticism amplifies fear; emotional stability dampens it.
    // 0.0 neuroticism → 0.7×, 0.5 (default) → 1.0×, 1.0 → 1.3×.
    let personality_mod = 0.7 + personality.traits.neuroticism * 0.6;

    // Low health amplifies perceived threat — a wounded agent has more to lose.
    // Full health → 1.0×, zero health → 1.4×.
    let health_loss = (1.0 - body_health).clamp(0.0, 1.0);
    let health_mod = 1.0 + health_loss * 0.4;

    // Holding a weapon reduces perceived threat. For now Stick is the only
    // weapon-capable item; extend when more are added.
    let armed_mod = match items {
        Some(slots) if slots.count(Concept::Stick) > 0 => 0.6,
        _ => 1.0,
    };

    // Desperation (high hunger or thirst) reduces fear. This lets the
    // arbitration layer pick food/water even when a threat is visible.
    // No desperation → 1.0×, fully desperate → 0.5×.
    let desperation = needs.hunger_urgency().max(needs.hydration.deficit());
    let desperation_mod = 1.0 - desperation * 0.5;

    (BASE_THREAT * personality_mod * health_mod * armed_mod * desperation_mod).clamp(0.0, 1.0)
}

pub fn react_to_danger(
    mut agents: Query<
        (
            &VisibleObjects,
            &mut MindGraph,
            &mut crate::agent::psyche::emotions::EmotionalState,
            &crate::agent::psyche::personality::Personality,
            &crate::agent::body::needs::PhysicalNeeds,
            Option<&crate::agent::biology::body::Body>,
            Option<&crate::agent::item_slots::ItemSlots>,
        ),
        With<Agent>,
    >,
    tick: Res<TickCount>,
) {
    use crate::agent::brains::emotional::add_entity_emotion;
    use crate::agent::mind::knowledge::Source;
    use crate::agent::psyche::emotions::{Emotion, EmotionType};

    let current_tick = tick.current;
    // Cadence-gate the per-entity Fear writes. Every tick is too noisy
    // (300 agents × N visible threats × 60Hz of mind ops) and the
    // entity-Fear scalar moves slowly anyway.
    let do_per_entity_writes = current_tick.is_multiple_of(PER_ENTITY_FEAR_WRITE_PERIOD);

    for (visible, mut mind, mut emotions, personality, needs, body, items) in agents.iter_mut() {
        // Trait checks are concept-level (every Wolf is Dangerous-or-not the
        // same way), so iterate by_concept and pay one `has_trait` per
        // visible concept instead of one per visible entity.
        let visible_dangerous: Vec<Entity> = visible
            .by_concept
            .iter()
            .filter(|(concept, _)| mind.has_trait(&Node::Concept(**concept), Concept::Dangerous))
            .flat_map(|(_, ents)| ents.iter().copied())
            .collect();

        let audible_dangerous_count = mind
            .query(
                None,
                Some(Predicate::HasTrait),
                Some(&Value::Concept(Concept::Dangerous)),
            )
            .iter()
            .filter(|t| matches!(t.subject, Node::Direction(_)))
            .count();

        let total_dangerous = visible_dangerous.len() + audible_dangerous_count;
        if total_dangerous == 0 {
            continue;
        }
        let dangerous_count = visible_dangerous.len() + audible_dangerous_count.min(2);

        let body_health = body.map_or(1.0, |b| b.overall_health());
        let per_threat = assess_threat(personality, needs, body_health, items);
        let fear_intensity = (per_threat * dangerous_count as f32).clamp(0.0, 1.0);

        let current_fear: f32 = emotions
            .active_emotions
            .iter()
            .filter(|e| e.emotion_type == EmotionType::Fear)
            .map(|e| e.intensity)
            .sum();
        if current_fear < fear_intensity {
            let additional_fear = (fear_intensity - current_fear).max(0.1);
            emotions.add_emotion(Emotion::new(EmotionType::Fear, additional_fear));
        }

        if !do_per_entity_writes {
            continue;
        }
        let per_entity_delta = (per_threat * PER_ENTITY_FEAR_FRACTION).max(PER_ENTITY_FEAR_FLOOR);
        for &entity in &visible_dangerous {
            add_entity_emotion(
                &mut mind,
                entity,
                EmotionType::Fear,
                per_entity_delta,
                current_tick,
                Source::Experienced,
            );
        }
    }
}

/// Run per-entity Fear accumulation every Nth tick. At 60 ticks/sec,
/// 15 = ~4 writes per real-second per agent, which still feels live
/// while cutting per-tick cost.
const PER_ENTITY_FEAR_WRITE_PERIOD: u64 = 15;
const PER_ENTITY_FEAR_FRACTION: f32 = 0.2;
const PER_ENTITY_FEAR_FLOOR: f32 = 0.05;

// ═══════════════════════════════════════════════════════════════════════════
// TEMPERATURE PERCEPTION — Detect heat sources without line-of-sight
// ═══════════════════════════════════════════════════════════════════════════

/// Base range for temperature perception (world pixels).
/// HeatSource.range is the source's emission radius; this is the agent's
/// maximum detection distance (whichever is smaller wins).
const TEMPERATURE_SENSE_RANGE: f32 = 64.0;

/// Heat sources are stable (campfires don't move), so scan every 10 ticks per agent.
const TEMPERATURE_SCAN_INTERVAL: u64 = 10;

pub fn perceive_temperature(
    mut agents: Query<(Entity, &Transform, &mut MindGraph), With<Agent>>,
    heat_sources: Query<(Entity, &Transform, &HeatSource)>,
    spatial_index: Res<SpatialIndex>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
) {
    let current_time = tick.current;

    for (agent_entity, agent_transform, mut mind) in agents.iter_mut() {
        if !tick.should_run(agent_entity, TEMPERATURE_SCAN_INTERVAL) {
            continue;
        }

        let agent_pos = agent_transform.translation.truncate();

        for candidate in spatial_index.entities_near(agent_pos, TEMPERATURE_SENSE_RANGE) {
            if candidate == agent_entity {
                continue;
            }

            let Ok((source_entity, source_transform, heat)) = heat_sources.get(candidate) else {
                continue;
            };

            let source_pos = source_transform.translation.truncate();
            let distance = agent_pos.distance(source_pos);
            let effective_range = heat.radius.min(TEMPERATURE_SENSE_RANGE);

            if distance > effective_range {
                continue;
            }

            // Confidence scales with proximity and intensity: close + hot = high confidence
            let distance_factor = 1.0 - (distance / effective_range).clamp(0.0, 1.0);
            let confidence = (distance_factor * heat.intensity * 0.6).clamp(0.1, 0.6);

            // Write warmth perception as a tile-level trait (no entity identification)
            let tile_x = (source_pos.x / TILE_SIZE).floor() as i32;
            let tile_y = (source_pos.y / TILE_SIZE).floor() as i32;

            mind.perceive_via_sense(
                Node::Tile((tile_x, tile_y)),
                Predicate::HasTrait,
                Value::Concept(Concept::Warmth),
                current_time,
                confidence,
                Sense::Temperature,
            );

            // Also write directional warmth
            let dir = source_pos - agent_pos;
            if dir.length_squared() > 0.01 {
                let cardinal = CardinalDirection::from_vec2(dir);
                mind.perceive_via_sense(
                    Node::Direction(cardinal),
                    Predicate::HasTrait,
                    Value::Concept(Concept::Warmth),
                    current_time,
                    confidence * 0.7,
                    Sense::Temperature,
                );
            }

            sim_events.write(crate::agent::events::SimEvent::single(
                current_time,
                agent_entity,
                SimEventKind::WarmthPerceived {
                    agent: agent_entity,
                    source: source_entity,
                },
            ));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// HEARING PERCEPTION — Detect sounds without line-of-sight
// ═══════════════════════════════════════════════════════════════════════════

/// Base range for hearing perception (world pixels).
const HEARING_SENSE_RANGE: f32 = 512.0;

/// Map SoundKind to the Concept used in MindGraph triples.
fn sound_kind_to_concept(kind: crate::world::sense_sources::SoundKind) -> Concept {
    use crate::world::sense_sources::SoundKind;
    match kind {
        SoundKind::Howl => Concept::Howl,
        SoundKind::AlarmCall => Concept::AlarmCall,
        SoundKind::Scream => Concept::Scream,
        SoundKind::Combat => Concept::CombatSound,
    }
}

pub fn perceive_hearing(
    mut agents: Query<(Entity, &Transform, &mut MindGraph), With<Agent>>,
    sound_sources: Query<(Entity, &Transform, &SoundSource)>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
) {
    let current_time = tick.current;

    // SoundSource is transient (1-tick lifetime) and typically rare. Iterate
    // the query directly instead of via spatial index — avoids the 1-tick lag
    // from PostUpdate spatial index updates.
    for (agent_entity, agent_transform, mut mind) in agents.iter_mut() {
        let agent_pos = agent_transform.translation.truncate();

        for (source_entity, source_transform, sound) in sound_sources.iter() {
            if source_entity == agent_entity {
                continue;
            }

            let source_pos = source_transform.translation.truncate();
            let distance = agent_pos.distance(source_pos);
            let effective_range = HEARING_SENSE_RANGE * sound.intensity;

            if distance > effective_range {
                continue;
            }

            // Hearing confidence is low — direction only, no entity identification
            let distance_factor = 1.0 - (distance / effective_range).clamp(0.0, 1.0);
            let confidence = (distance_factor * 0.5).clamp(0.1, 0.5);

            let dir = source_pos - agent_pos;
            if dir.length_squared() < 0.01 {
                continue;
            }

            let cardinal = CardinalDirection::from_vec2(dir);
            let sound_concept = sound_kind_to_concept(sound.kind);

            mind.perceive_via_sense(
                Node::Direction(cardinal),
                Predicate::ProducedSound,
                Value::Concept(sound_concept),
                current_time,
                confidence,
                Sense::Hearing,
            );

            if sound.kind.is_threatening() {
                mind.perceive_via_sense(
                    Node::Direction(cardinal),
                    Predicate::HasTrait,
                    Value::Concept(Concept::Dangerous),
                    current_time,
                    confidence * 0.6,
                    Sense::Hearing,
                );
            }

            sim_events.write(crate::agent::events::SimEvent::single(
                current_time,
                agent_entity,
                SimEventKind::SoundPerceived {
                    agent: agent_entity,
                    source: source_entity,
                    kind: sound.kind,
                },
            ));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ALARM EMISSION — Fleeing agents broadcast their distress
// ═══════════════════════════════════════════════════════════════════════════

/// Inserts a transient [`SoundSource`] on every agent who started a
/// Flee action this tick. `perceive_hearing` writes direction-based
/// Dangerous beliefs into listeners; that belief feeds existing fear
/// → herd flees together off one alarm.
pub fn emit_alarm_calls(
    mut commands: Commands,
    mut events: MessageReader<crate::agent::events::SimEvent>,
) {
    for event in events.read() {
        if let SimEventKind::ActionStarted { agent, action, .. } = &event.kind
            && matches!(action, crate::agent::actions::ActionType::Flee)
        {
            commands
                .entity(*agent)
                .insert(crate::world::sense_sources::SoundSource {
                    kind: crate::world::sense_sources::SoundKind::AlarmCall,
                    intensity: 1.0,
                });
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SOUND SOURCE CLEANUP — Remove transient SoundSource after one perception tick
// ═══════════════════════════════════════════════════════════════════════════

pub fn cleanup_sound_sources(mut commands: Commands, sources: Query<Entity, With<SoundSource>>) {
    for entity in sources.iter() {
        commands.entity(entity).remove::<SoundSource>();
    }
}

#[cfg(test)]
mod threat_tests {
    use super::*;
    use crate::agent::body::needs::PhysicalNeeds;
    use crate::agent::item_slots::ItemSlots;
    use crate::agent::psyche::personality::{Personality, PersonalityTraits};

    fn default_needs() -> PhysicalNeeds {
        PhysicalNeeds::full()
            .with_metabolism(crate::agent::body::metabolism::Metabolism::well_fed())
    }

    fn personality_with_neuroticism(neuroticism: f32) -> Personality {
        Personality {
            traits: PersonalityTraits {
                neuroticism,
                ..Default::default()
            },
        }
    }

    #[test]
    fn calm_healthy_unarmed_agent_matches_previous_hardcoded_fear() {
        let personality = personality_with_neuroticism(0.5);
        let needs = default_needs();
        let score = assess_threat(&personality, &needs, 1.0, None);
        // 0.8 × 1.0 × 1.0 × 1.0 × 1.0 = 0.8
        assert!((score - 0.8).abs() < 1e-4, "expected 0.8, got {score}");
    }

    #[test]
    fn armed_agent_feels_less_fear_than_unarmed() {
        let personality = personality_with_neuroticism(0.5);
        let needs = default_needs();

        let unarmed = assess_threat(&personality, &needs, 1.0, None);

        let mut slots = ItemSlots::agent_carry();
        slots.add(Concept::Stick, 1);
        let armed = assess_threat(&personality, &needs, 1.0, Some(&slots));

        assert!(
            armed < unarmed,
            "armed ({armed}) should be less than unarmed ({unarmed})"
        );
    }

    #[test]
    fn neurotic_agent_feels_more_fear_than_stable_agent() {
        let stable = assess_threat(
            &personality_with_neuroticism(0.0),
            &default_needs(),
            1.0,
            None,
        );
        let neurotic = assess_threat(
            &personality_with_neuroticism(1.0),
            &default_needs(),
            1.0,
            None,
        );
        assert!(
            neurotic > stable,
            "neurotic ({neurotic}) should exceed stable ({stable})"
        );
    }

    #[test]
    fn wounded_agent_feels_more_fear_than_healthy_one() {
        let personality = personality_with_neuroticism(0.5);
        let healthy = assess_threat(&personality, &default_needs(), 1.0, None);
        let wounded = assess_threat(&personality, &default_needs(), 0.2, None);
        assert!(
            wounded > healthy,
            "wounded ({wounded}) should exceed healthy ({healthy})"
        );
    }

    #[test]
    fn desperate_agent_feels_less_fear_so_other_urgencies_can_win() {
        let personality = personality_with_neuroticism(0.5);
        let calm_full = assess_threat(&personality, &default_needs(), 1.0, None);
        let starving = assess_threat(
            &personality,
            &default_needs()
                .with_metabolism(crate::agent::body::metabolism::Metabolism::at_urgency(0.95)),
            1.0,
            None,
        );
        assert!(
            starving < calm_full,
            "starving ({starving}) should be less than calm_full ({calm_full})"
        );
    }

    #[test]
    fn threat_score_clamped_to_unit_interval() {
        // Max-anxiety, max-wounded, unarmed, calm → should still clamp to ≤1.0
        let personality = personality_with_neuroticism(1.0);
        let needs = default_needs();
        let score = assess_threat(&personality, &needs, 0.0, None);
        assert!(score <= 1.0, "score {score} should be clamped to ≤1.0");
        assert!(score >= 0.0, "score {score} should be non-negative");
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    fn entity(id: u32) -> Entity {
        Entity::from_bits(id as u64)
    }

    fn primed(chunk: IVec2, chunk_radius: i32, tick: u64) -> PerceptionCache {
        PerceptionCache {
            last_chunk: Some(chunk),
            last_chunk_radius: chunk_radius,
            last_query_tick: tick,
            // A non-empty cache exercises the steady-state hit path; the "empty
            // forces re-query" branch has its own test below.
            cached: vec![entity(1)],
        }
    }

    #[test]
    fn unpopulated_cache_is_always_stale() {
        let cache = PerceptionCache::default();
        assert!(cache.is_stale(IVec2::ZERO, 2, 0));
    }

    #[test]
    fn same_chunk_and_radius_within_safety_window_is_fresh() {
        let cache = primed(IVec2::new(3, 4), 2, 100);
        assert!(!cache.is_stale(IVec2::new(3, 4), 2, 100));
        assert!(!cache.is_stale(IVec2::new(3, 4), 2, 100 + SAFETY_REFRESH_TICKS - 1));
    }

    #[test]
    fn chunk_change_invalidates() {
        let cache = primed(IVec2::new(3, 4), 2, 100);
        assert!(cache.is_stale(IVec2::new(4, 4), 2, 101));
    }

    #[test]
    fn chunk_radius_change_invalidates() {
        // Light dropping (or vision shrinking) can move chunk_radius_for from 2 → 1.
        let cache = primed(IVec2::new(3, 4), 2, 100);
        assert!(cache.is_stale(IVec2::new(3, 4), 1, 101));
    }

    #[test]
    fn safety_refresh_fires_at_threshold() {
        let cache = primed(IVec2::new(3, 4), 2, 100);
        assert!(cache.is_stale(IVec2::new(3, 4), 2, 100 + SAFETY_REFRESH_TICKS));
    }

    #[test]
    fn empty_cached_list_invalidates_even_within_safety_window() {
        // Reproduces the pre-fix bug: on the first tick the spatial index has not
        // yet been populated, so the query returns []. Without this branch the
        // cache would happily serve [] for the next 30 ticks.
        let mut cache = primed(IVec2::new(3, 4), 2, 100);
        cache.cached.clear();
        assert!(cache.is_stale(IVec2::new(3, 4), 2, 101));
    }
}
