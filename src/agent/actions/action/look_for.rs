//! LookFor action - goal-directed search for a specific concept.
//!
//! Reads: MindGraph (Produces hints, Explored timestamps, visible entities),
//! WorldMap, LegCompleteContext (search_filter)
//! Writes: LegResult (next target or complete)
//! Upstream: brains::rational (fallback proposal when an urgency has a
//! concept-filtered satisfier but no known instance), nervous_system::execution (dispatch)
//! Downstream: nervous_system::execution (runs the action, calls on_leg_complete)

use crate::agent::actions::ActionType;
use crate::agent::actions::action::explore::{
    pick_explore_target, sample_walkable_scored, staleness_penalty,
};
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::motor::{
    ActionPrimitive, Behavior, IntensityPolicy, Intent, TargetSelector,
};
use crate::agent::actions::registry::{Action, ActionKind, LegCompleteContext, LegResult};
use crate::agent::brains::thinking::SearchFilter;
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use crate::world::map::{CHUNK_SIZE, WorldMap};
use bevy::math::IVec2;
use bevy::prelude::Vec2;

pub struct LookForAction;

impl Action for LookForAction {
    fn action_type(&self) -> ActionType {
        ActionType::LookFor
    }

    fn name(&self) -> &'static str {
        "LookFor"
    }

    fn default_behavior(&self) -> Behavior {
        Behavior::new(
            ActionPrimitive::Locomote,
            TargetSelector::UnknownArea,
            IntensityPolicy::Normal,
            Intent::Goal,
        )
    }

    fn kind(&self) -> ActionKind {
        // Ambient, same as Explore. The "I found it" termination happens
        // one level up: when perception writes a Contains triple the
        // planner builds a real Harvest/Walk/Eat plan, and its urgency
        // outranks LookFor in arbitration.
        ActionKind::Ambient
    }

    fn cost(&self) -> f32 {
        3.0
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(Channel::Locomotion, 0.4),
            ChannelUsage::new(Channel::Focus, 0.15),
            ChannelUsage::new(Channel::Awareness, 0.2),
        ];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        Some(Posture::Moving)
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("looking for something")
    }

    fn on_leg_complete(&self, ctx: &mut LegCompleteContext) -> LegResult {
        match pick_look_for_target(
            ctx.agent_position,
            ctx.mind,
            ctx.world_map,
            ctx.current_tick,
            ctx.search_filter,
            ctx.rng,
        ) {
            Some(pos) => LegResult::NextLeg(pos),
            None => LegResult::Complete,
        }
    }
}

/// Concept-hint-aware target picker.
///
/// Scores random walkable samples by a combination of:
///   * **Hint bonus** — chunks with a visible entity whose concept
///     `Produces` an item passing the filter get a large negative
///     score.
///   * **Staleness penalty** — recently-`Explored` chunks get the same
///     `1000 / (age + 1)` penalty `pick_explore_target` uses.
///   * **Distance** — small tiebreaker (added by `sample_walkable_scored`).
///
/// Falls through to `pick_explore_target` when no filter is set or no
/// hint-matching chunk can be found, so the action never stalls.
pub fn pick_look_for_target(
    current_pos: Vec2,
    mind: &MindGraph,
    world_map: &WorldMap,
    current_tick: u64,
    filter: Option<SearchFilter>,
    rng: &mut dyn rand::RngCore,
) -> Option<Vec2> {
    let Some(filter) = filter else {
        return pick_explore_target(current_pos, mind, world_map, current_tick, rng);
    };
    if filter.is_empty() {
        return pick_explore_target(current_pos, mind, world_map, current_tick, rng);
    }

    let hint_chunks = collect_hint_chunks(mind, &filter);

    let picked = sample_walkable_scored(current_pos, world_map, 16, rng, |_pos, chunk| {
        let mut score = staleness_penalty(mind, chunk, current_tick);
        if hint_chunks.iter().any(|c| *c == chunk) {
            score -= 2000.0;
        }
        score
    });

    picked.or_else(|| pick_explore_target(current_pos, mind, world_map, current_tick, rng))
}

/// Chunks that likely contain something matching the filter.
///
/// Walks the MindGraph for `Entity LocatedAt Tile` triples whose
/// subject concept is known to `Produces` an `Item` that passes the
/// filter. Returns the `(chunk_x, chunk_y)` of each match.
fn collect_hint_chunks(mind: &MindGraph, filter: &SearchFilter) -> Vec<IVec2> {
    let mut chunks = Vec::new();

    let mut source_concepts: Vec<Concept> = Vec::new();
    for triple in mind.query(None, Some(Predicate::Produces), None) {
        if let (Node::Concept(source), Value::Item(produced, _)) = (&triple.subject, &triple.object)
            && item_matches_filter(mind, *produced, filter)
        {
            source_concepts.push(*source);
        }
    }

    if source_concepts.is_empty() {
        return chunks;
    }

    for triple in mind.query(None, Some(Predicate::LocatedAt), None) {
        let Node::Entity(entity) = &triple.subject else {
            continue;
        };
        let Value::Tile((tx, ty)) = &triple.object else {
            continue;
        };
        let entity_node = Node::Entity(*entity);
        // `is_a` walks the ontology once per call; with K source
        // concepts that's K walks per entity. The `any` short-circuits
        // on the first match, so worst case dominates only when no
        // concept matches.
        let is_source = source_concepts.iter().any(|c| mind.is_a(&entity_node, *c));
        if is_source {
            chunks.push(IVec2::new(
                tx.div_euclid(CHUNK_SIZE as i32),
                ty.div_euclid(CHUNK_SIZE as i32),
            ));
        }
    }

    chunks
}

fn item_matches_filter(mind: &MindGraph, item: Concept, filter: &SearchFilter) -> bool {
    if let Some(c) = filter.isa
        && !mind.is_a(&Node::Concept(item), c)
    {
        return false;
    }
    if let Some(t) = filter.trait_ {
        let has_trait = !mind
            .query(
                Some(&Node::Concept(item)),
                Some(Predicate::HasTrait),
                Some(&Value::Concept(t)),
            )
            .is_empty();
        if !has_trait {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mind::knowledge::{Metadata, Ontology, Triple};
    use crate::world::map::TileType;
    use crate::world::spatial_index::world_pos_to_chunk;
    use bevy::prelude::Entity;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn walkable_map() -> WorldMap {
        let size = CHUNK_SIZE * 4;
        let mut map = WorldMap::new(size, size);
        for cx in 0..4i32 {
            for cy in 0..4i32 {
                map.chunks
                    .entry(IVec2::new(cx, cy))
                    .or_insert_with(|| crate::world::map::Chunk::new(cx, cy));
            }
        }
        for x in 0..size {
            for y in 0..size {
                map.set_tile(x, y, TileType::Grass);
            }
        }
        map
    }

    fn make_ontology() -> Ontology {
        crate::agent::mind::knowledge::setup_ontology()
    }

    fn mind_with_berry_bush_in_chunk(chunk_x: i32, chunk_y: i32) -> (MindGraph, Entity) {
        let mut mind = MindGraph::new(make_ontology());
        let bush = Entity::from_bits(42);
        let meta = Metadata::semantic(0);
        mind.assert(Triple::with_meta(
            Node::Concept(Concept::BerryBush),
            Predicate::Produces,
            Value::Item(Concept::Berry, 1),
            meta.clone(),
        ));
        mind.assert(Triple::with_meta(
            Node::Entity(bush),
            Predicate::IsA,
            Value::Concept(Concept::BerryBush),
            meta.clone(),
        ));
        let tile_x = chunk_x * CHUNK_SIZE as i32 + 2;
        let tile_y = chunk_y * CHUNK_SIZE as i32 + 2;
        mind.assert(Triple::with_meta(
            Node::Entity(bush),
            Predicate::LocatedAt,
            Value::Tile((tile_x, tile_y)),
            meta,
        ));
        (mind, bush)
    }

    #[test]
    fn look_for_target_picker_biases_toward_produces_hints() {
        let map = walkable_map();
        let (mind, _bush) = mind_with_berry_bush_in_chunk(2, 2);

        let mut hits_in_hint_chunk = 0;
        for seed in 0..40u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let target = pick_look_for_target(
                Vec2::new(10.0, 10.0),
                &mind,
                &map,
                0,
                Some(SearchFilter::concept(Concept::Food)),
                &mut rng,
            )
            .expect("picker should always find a walkable target on an all-grass map");

            if world_pos_to_chunk(target) == IVec2::new(2, 2) {
                hits_in_hint_chunk += 1;
            }
        }

        assert!(
            hits_in_hint_chunk >= 30,
            "hint chunk should win >= 75% of the time; got {hits_in_hint_chunk}/40"
        );
    }

    #[test]
    fn look_for_target_picker_falls_back_to_explore_with_no_hints() {
        let map = walkable_map();
        let mind = MindGraph::new(make_ontology());
        let mut rng = StdRng::seed_from_u64(0);

        let target = pick_look_for_target(
            Vec2::new(10.0, 10.0),
            &mind,
            &map,
            0,
            Some(SearchFilter::concept(Concept::Food)),
            &mut rng,
        );

        assert!(
            target.is_some(),
            "picker must fall through to Explore's staleness scorer when no hints exist"
        );
    }
}
