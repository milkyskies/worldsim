//! Perception: multi-sense detection of nearby entities and environmental signals.
//!
//! Reads: Transform, Vision, LightLevel, Physical entities, body state components, TickCount, SpatialIndex, HeatSource, SoundSource
//! Writes: VisibleObjects (entity list), MindGraph (triples tagged with source_sense), SimEvent::{EntityPerceived, WarmthPerceived, SoundPerceived}
//! Upstream: world::map (tile/chunk data), world::environment (LightLevel), world::sense_sources, agent body state
//! Downstream: brain_system (reads VisibleObjects), knowledge (MindGraph updated with percepts), SimEvent consumers

use crate::agent::Agent;
use crate::agent::events::SimEventKind;
use crate::agent::mind::knowledge::{
    CardinalDirection, Concept, Metadata, MindGraph, Node, Predicate, Quantity, Sense, Triple,
    Value,
};
use crate::core::GameLog;
use crate::core::tick::TickCount;
use crate::world::environment::LightLevel;
use crate::world::map::{CHUNK_SIZE, TILE_SIZE};
use crate::world::property::HeatSource;
use crate::world::sense_sources::SoundSource;
use crate::world::spatial_index::SpatialIndex;
use bevy::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════
// VISION COMPONENTS
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Vision {
    pub range: f32,
}

#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct VisibleObjects {
    pub entities: Vec<Entity>,
}

// ═══════════════════════════════════════════════════════════════════════════
// VISUAL PERCEPTION — Detect entities in range
// ═══════════════════════════════════════════════════════════════════════════

pub fn update_visual_perception(
    mut agents: Query<(Entity, &Transform, &Vision, &mut VisibleObjects), With<Agent>>,
    transforms: Query<&Transform, With<crate::world::Physical>>,
    spatial_index: Res<SpatialIndex>,
    light_level: Res<LightLevel>,
    mut _game_log: ResMut<GameLog>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
) {
    let _start = std::time::Instant::now();

    for (agent_entity, agent_transform, vision, mut visible_objects) in agents.iter_mut() {
        let previous: Vec<Entity> = visible_objects.entities.clone();
        visible_objects.entities.clear();

        let agent_pos = agent_transform.translation.truncate();
        let view_range = vision.range * light_level.0;

        // Spatial index gives us only nearby candidates (O(k) instead of O(n)).
        // We still do a precise distance check since chunk buckets are coarser than view range.
        for entity in spatial_index.entities_near(agent_pos, view_range) {
            if entity == agent_entity {
                continue;
            }

            if let Ok(target_transform) = transforms.get(entity) {
                let target_pos = target_transform.translation.truncate();
                if agent_pos.distance(target_pos) <= view_range {
                    visible_objects.entities.push(entity);
                }
            }
        }

        // Emit EntityPerceived for newly visible entities
        for &entity in &visible_objects.entities {
            if !previous.contains(&entity) {
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
            &crate::agent::body::needs::PhysicalNeeds,
            &crate::agent::body::needs::Consciousness,
            Option<&crate::agent::biology::body::Body>,
            &Transform,
            &mut MindGraph,
        ),
        With<Agent>,
    >,
    tick: Res<TickCount>,
) {
    let current_time = tick.current;

    for (_entity, physical, consciousness, body, transform, mut mind) in agents.iter_mut() {
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

        // Rule 3: Stats — self-sensing is Exact. The agent's body tells the
        // mind the ground-truth value. Observed beliefs about other agents go
        // through the social perception writer at Qualitative precision.
        let exact = |v: f32| Value::Quantity(Quantity::Exact(v));
        mind.perceive_self(
            Predicate::Hunger,
            exact(physical.hunger_urgency() * 100.0),
            current_time,
        );
        // Thirst stores "how thirsty" (0 = hydrated, 100 = parched) so
        // downstream goal predicates like `(Self, Thirst, 0)` keep reading
        // correctly. The mind stores thirst as a 0..100 deficit for legacy
        // goal-predicate compatibility; hydration itself is a 0..1 Need.
        mind.perceive_self(
            Predicate::Thirst,
            exact(physical.hydration.deficit() * 100.0),
            current_time,
        );
        mind.perceive_self(
            Predicate::Stamina,
            exact(physical.stamina.aerobic),
            current_time,
        );

        let total_pain = body.map(|b| b.total_pain()).unwrap_or(0.0);
        if total_pain > 0.0 {
            mind.perceive_self(Predicate::Pain, exact(total_pain), current_time);
        }

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

    // 2. Clear what IS NOT there (but used to be)
    // We query what we *thought* we had and REMOVE any items no longer present
    let beliefs = mind.query(Some(&subject_node), Some(Predicate::Contains), None);
    let stale: Vec<_> = beliefs
        .into_iter()
        .filter_map(|t| {
            if let Value::Item(c, q) = t.object {
                // Item is stale if: we don't see it anymore OR it had qty but now doesn't
                if !observed_concepts.contains(&c) {
                    return Some((c, q));
                }
            }
            None
        })
        .collect();

    for (concept, old_qty) in stale {
        // Remove the stale belief entirely instead of setting qty to 0
        mind.remove(
            &subject_node,
            Predicate::Contains,
            &Value::Item(concept, old_qty),
        );
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
            &MindGraph,
            &mut crate::agent::psyche::emotions::EmotionalState,
            &crate::agent::psyche::personality::Personality,
            &crate::agent::body::needs::PhysicalNeeds,
            Option<&crate::agent::biology::body::Body>,
            Option<&crate::agent::item_slots::ItemSlots>,
        ),
        With<Agent>,
    >,
    entity_types: Query<&crate::agent::inventory::EntityType>,
) {
    use crate::agent::psyche::emotions::{Emotion, EmotionType};

    for (visible, mind, mut emotions, personality, needs, body, items) in agents.iter_mut() {
        // Count how many visible entities this agent considers dangerous.
        let dangerous_count = visible
            .entities
            .iter()
            .filter(|&&entity| {
                let Ok(entity_type) = entity_types.get(entity) else {
                    return false;
                };
                !mind
                    .query(
                        Some(&Node::Concept(entity_type.0)),
                        Some(Predicate::HasTrait),
                        Some(&Value::Concept(Concept::Dangerous)),
                    )
                    .is_empty()
            })
            .count();

        if dangerous_count == 0 {
            continue;
        }

        // Single contextual threat score, scaled up by the number of threats visible.
        let body_health = body.map_or(1.0, |b| b.overall_health());
        let per_threat = assess_threat(personality, needs, body_health, items);
        let fear_intensity = (per_threat * dangerous_count as f32).clamp(0.0, 1.0);

        // Only top up fear if we're not already scared enough — prevents
        // runaway accumulation from the same scene being perceived each tick.
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
    }
}

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
