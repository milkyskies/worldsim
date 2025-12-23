use crate::agent::Agent;
use crate::agent::mind::knowledge::{Concept, Metadata, MindGraph, Node, Predicate, Triple, Value};
use crate::core::GameLog;
use crate::core::tick::TickCount;
use crate::world::environment::LightLevel;
use crate::world::map::{CHUNK_SIZE, TILE_SIZE};
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
    physical_entities: Query<(Entity, &Transform), With<crate::world::Physical>>,
    light_level: Res<LightLevel>,
    mut _game_log: ResMut<GameLog>,
) {
    let _start = std::time::Instant::now();

    for (agent_entity, agent_transform, vision, mut visible_objects) in agents.iter_mut() {
        visible_objects.entities.clear();

        let agent_pos = agent_transform.translation.truncate();
        let view_range = vision.range * light_level.0;

        for (entity, target_transform) in physical_entities.iter() {
            if entity == agent_entity {
                continue;
            }

            let target_pos = target_transform.translation.truncate();
            if agent_pos.distance(target_pos) <= view_range {
                visible_objects.entities.push(entity);
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

        // Rule 3: Stats
        mind.perceive_self(
            Predicate::Hunger,
            Value::Int(physical.hunger as i32),
            current_time,
        );
        mind.perceive_self(
            Predicate::Energy,
            Value::Int(physical.energy as i32),
            current_time,
        );

        let total_pain = body.map(|b| b.total_pain()).unwrap_or(0.0);
        if total_pain > 0.0 {
            mind.perceive_self(Predicate::Pain, Value::Int(total_pain as i32), current_time);
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
    inventories: Query<&crate::agent::inventory::Inventory>,
    entity_types: Query<&crate::agent::inventory::EntityType>,
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
    inventory: &crate::agent::inventory::Inventory,
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

    // 1. Record what IS there
    for item in &inventory.items {
        if item.quantity > 0 {
            observed_concepts.insert(item.concept);
            mind.assert(Triple::with_meta(
                subject_node.clone(),
                Predicate::Contains,
                Value::Item(item.concept, item.quantity),
                Metadata::perception_with_conf(time, confidence),
            ));
        }
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
// DANGER PERCEPTION — Trigger Fear when seeing dangerous entities
// ═══════════════════════════════════════════════════════════════════════════

// TODO: Stop hard coding
pub fn react_to_danger(
    mut agents: Query<
        (
            &VisibleObjects,
            &MindGraph,
            &mut crate::agent::psyche::emotions::EmotionalState,
        ),
        With<Agent>,
    >,
    entity_types: Query<&crate::agent::inventory::EntityType>,
) {
    use crate::agent::psyche::emotions::{Emotion, EmotionType};

    for (visible, mind, mut emotions) in agents.iter_mut() {
        let mut danger_level: f32 = 0.0;

        for &entity in &visible.entities {
            // Get what type of entity this is
            if let Ok(entity_type) = entity_types.get(entity) {
                let concept = entity_type.0;

                // Check if this agent knows this concept is Dangerous
                // Query: (Concept, HasTrait, Dangerous)
                let danger_triples = mind.query(
                    Some(&Node::Concept(concept)),
                    Some(Predicate::HasTrait),
                    Some(&Value::Concept(Concept::Dangerous)),
                );

                if !danger_triples.is_empty() {
                    // This entity is dangerous! Accumulate danger
                    danger_level += 0.8; // Each dangerous entity adds 0.8 fear
                }
            }
        }

        // If we saw something dangerous, add Fear emotion
        if danger_level > 0.0 {
            let fear_intensity = danger_level.min(1.0); // Cap at 1.0

            // Check current fear level
            let current_fear: f32 = emotions
                .active_emotions
                .iter()
                .filter(|e| e.emotion_type == EmotionType::Fear)
                .map(|e| e.intensity)
                .sum();

            // Only add fear if we're not already scared enough
            if current_fear < fear_intensity {
                let additional_fear = (fear_intensity - current_fear).max(0.1);
                emotions.add_emotion(Emotion::new(EmotionType::Fear, additional_fear));
            }
        }
    }
}
