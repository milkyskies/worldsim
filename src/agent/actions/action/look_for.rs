//! LookFor action - goal-directed search for a specific concept.

use crate::agent::actions::ActionType;
use crate::agent::actions::action::explore::pick_explore_target;
use crate::agent::actions::action::search_utils::{sample_walkable_scored, staleness_penalty};
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, LegCompleteContext, LegResult, TargetSource};
use crate::agent::brains::thinking::{SearchDomain, SearchFilter};
use crate::agent::mind::explored_tiles::ExploredTiles;
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use crate::world::map::{CHUNK_SIZE, WorldMap};
use bevy::math::IVec2;
use bevy::prelude::Vec2;

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Locomotion, 0.4),
    ChannelUsage::new(Channel::Focus, 0.15),
    ChannelUsage::new(Channel::Awareness, 0.2),
];

pub static LOOK_FOR_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::LookFor,
    kind: ActionKind::Ambient,
    target_source: TargetSource::None,
    base_cost: 3.0,
    primitive: ActionPrimitive::Locomote,
    target_selector: TargetSelector::UnknownArea,
    intensity: IntensityPolicy::Normal,
    intent: Intent::Goal,
    body_channels: CHANNELS,
    posture: Some(Posture::Moving),
    interruptible: true,
    start_log: Some("looking for something"),
    complete_log: None,
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks {
        on_leg_complete: Some(look_for_on_leg_complete),
        ..Hooks::EMPTY
    },
    recipe: None,
};

fn look_for_on_leg_complete(ctx: &mut LegCompleteContext) -> LegResult {
    match pick_look_for_target(
        ctx.agent_position,
        ctx.mind,
        ctx.explored,
        ctx.world_map,
        ctx.current_tick,
        ctx.search_filter,
        ctx.rng,
    ) {
        Some(pos) => LegResult::NextLeg(pos),
        None => LegResult::Complete,
    }
}

/// Concept-hint-aware target picker.
pub fn pick_look_for_target(
    current_pos: Vec2,
    mind: &MindGraph,
    explored: &ExploredTiles,
    world_map: &WorldMap,
    current_tick: u64,
    filter: Option<SearchFilter>,
    rng: &mut dyn rand::RngCore,
) -> Option<Vec2> {
    let Some(filter) = filter else {
        return pick_explore_target(current_pos, explored, world_map, current_tick, rng);
    };
    debug_assert!(
        !filter.is_empty(),
        "LookFor dispatched with an empty SearchFilter — derive_search_concept should never return an empty filter"
    );

    let hint_chunks = match filter.domain {
        SearchDomain::Inventory => collect_producer_hint_chunks(mind, &filter),
        SearchDomain::WorldTile | SearchDomain::WorldEntity => Vec::new(),
    };

    let picked = sample_walkable_scored(current_pos, world_map, 16, rng, |_pos, chunk| {
        let mut score = staleness_penalty(explored, chunk, current_tick);
        if hint_chunks.contains(&chunk) {
            score -= 2000.0;
        }
        score
    });

    picked.or_else(|| pick_explore_target(current_pos, explored, world_map, current_tick, rng))
}

fn collect_producer_hint_chunks(mind: &MindGraph, filter: &SearchFilter) -> Vec<IVec2> {
    let mut chunks = Vec::new();

    let mut source_concepts: Vec<Concept> = Vec::new();
    for triple in mind.query(None, Some(Predicate::Produces), None) {
        if let (Node::Concept(source), Value::Item(produced, _)) = (&triple.subject, &triple.object)
            && filter.matches(*produced, mind)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mind::knowledge::{Metadata, Ontology, Triple, setup_ontology};
    use crate::world::map::WorldMap;
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
        map
    }

    fn make_ontology() -> Ontology {
        setup_ontology()
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
                &ExploredTiles::default(),
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
    fn world_tile_domain_ignores_producer_hints() {
        let map = walkable_map();
        let (mind, _bush) = mind_with_berry_bush_in_chunk(2, 2);

        let mut hits_in_bush_chunk = 0;
        for seed in 0..40u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let target = pick_look_for_target(
                Vec2::new(10.0, 10.0),
                &mind,
                &ExploredTiles::default(),
                &map,
                0,
                Some(SearchFilter::tile_trait(Concept::Drinkable)),
                &mut rng,
            )
            .expect("picker should always find a walkable target");

            if world_pos_to_chunk(target) == IVec2::new(2, 2) {
                hits_in_bush_chunk += 1;
            }
        }

        assert!(
            hits_in_bush_chunk <= 15,
            "WorldTile search must not cluster on producer chunks; \
             got {hits_in_bush_chunk}/40 in the bush chunk"
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
            &ExploredTiles::default(),
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
