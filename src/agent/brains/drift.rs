//! Reactive drift: tile-based local-gradient sampling.
//!
//! Each active drive scores the agent's 9×9 tile neighborhood by summing
//! perceived-entity pulls and field samples; the agent walks toward the
//! highest-scored tile. Adding a new drive is one `DriveBehavior` entry
//! plus one scorer function.

use bevy::math::{IVec2, Vec2};

use crate::agent::actions::definition::PreferenceContext;
use crate::agent::actions::{ActionRegistry, ActionType};
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use crate::agent::nervous_system::urgency::UrgencySource;
use crate::constants::brains::emotional::{
    SOCIAL_SEEK_THRESHOLD, SOCIAL_SEEK_URGENCY_MULTIPLIER, WARMTH_SEEK_THRESHOLD,
    WARMTH_SEEK_URGENCY_MULTIPLIER,
};
use crate::constants::thermal::COMFORT_MIN_C;
use crate::world::field_grid_plugin::FieldGrids;
use crate::world::spatial_index::{tile_center_px, world_pos_to_tile};
use bevy::prelude::Entity;

use super::proposal::{BrainProposal, BrainType, Intent};

/// 4 tiles = 64px radius = 9×9 = 81 tiles scored per active drive.
pub(crate) const DRIFT_RADIUS_TILES: i32 = 4;

/// How strongly a positive field delta (above the comfort floor) pulls
/// compared to entity proximity. Tuned so a single nearby campfire
/// entity outranks a single warm tile — entities are the first-order
/// signal, field values are the corrective fine-grain layer.
pub(crate) const FIELD_PULL_WEIGHT: f32 = 0.05;

/// Drift shares its per-agent context with action-prep scoring
/// (`PreferenceContext` from `agent::actions::definition`). Keeping them
/// as one type avoids duplication.
pub(super) type DriftContext<'a> = PreferenceContext<'a>;

#[derive(Clone, Copy)]
pub(super) struct DriveBehavior {
    pub drive: UrgencySource,
    pub threshold: f32,
    pub multiplier: f32,
    pub intent: Intent,
    pub get_deficit: fn(&DriftContext) -> f32,
    pub score_tile: fn(&ScoringInputs, IVec2) -> f32,
    /// Filter + weight visible entities once per proposal. For Warmth
    /// every heat emitter contributes weight 1.0; for Social only
    /// conspecifics contribute and their weight is the affection
    /// predicate. Called once outside the 81-tile loop.
    pub collect_targets: fn(&DriftContext) -> Vec<(Vec2, f32)>,
}

pub(super) const BEHAVIORS: &[DriveBehavior] = &[
    DriveBehavior {
        drive: UrgencySource::Warmth,
        threshold: WARMTH_SEEK_THRESHOLD,
        multiplier: WARMTH_SEEK_URGENCY_MULTIPLIER,
        intent: Intent::SatisfyWarmth,
        get_deficit: get_deficit_warmth,
        score_tile: score_tile_warmth,
        collect_targets: collect_heat_emitters,
    },
    DriveBehavior {
        drive: UrgencySource::Social,
        threshold: SOCIAL_SEEK_THRESHOLD,
        multiplier: SOCIAL_SEEK_URGENCY_MULTIPLIER,
        intent: Intent::SatisfySocial,
        get_deficit: get_deficit_social,
        score_tile: score_tile_social,
        collect_targets: collect_conspecifics,
    },
];

/// Inputs the per-tile scorer needs after expensive filtering is done.
/// Separated so the scorer's hot inner loop doesn't touch the MindGraph.
pub(super) struct ScoringInputs<'a> {
    targets: &'a [(Vec2, f32)],
    fields: &'a FieldGrids,
}

pub(super) fn propose_drift(
    behavior: &DriveBehavior,
    ctx: &DriftContext,
    action_registry: &ActionRegistry,
    min_urgency: f32,
) -> Option<BrainProposal> {
    let deficit = (behavior.get_deficit)(ctx);
    if deficit <= behavior.threshold {
        return None;
    }
    let urgency = deficit * behavior.multiplier;
    if urgency <= min_urgency {
        return None;
    }

    let targets = (behavior.collect_targets)(ctx);
    // Social has no field component; Warmth does. If there are no
    // target entities AND (for drives without a field term) no other
    // signal, there's nothing to drift toward — skip the 81-tile scan.
    let has_field_signal = matches!(behavior.drive, UrgencySource::Warmth);
    if targets.is_empty() && !has_field_signal {
        return None;
    }

    let inputs = ScoringInputs {
        targets: &targets,
        fields: ctx.fields,
    };
    let center_tile = world_pos_to_tile(ctx.agent_pos);
    let center_score = (behavior.score_tile)(&inputs, center_tile);
    let (target_tile, best_score) =
        best_neighbor_tile(center_tile, |tile| (behavior.score_tile)(&inputs, tile))?;
    // Don't propose a move when the current tile is already at-or-better.
    // Keeps the agent from oscillating once they arrive.
    if best_score <= center_score || best_score <= 0.0 {
        return None;
    }

    let action = action_registry.get(ActionType::Walk)?;
    let mut template = action.to_template(None);
    template.target_position = Some(tile_center_px(target_tile));

    Some(BrainProposal {
        brain: BrainType::Emotional,
        action: template,
        urgency,
        intent: behavior.intent,
        reasoning: format!(
            "Drift {:?}: toward tile ({}, {}) (deficit {:.2})",
            behavior.drive, target_tile.x, target_tile.y, deficit
        ),
    })
}

fn get_deficit_warmth(ctx: &DriftContext) -> f32 {
    ctx.physical.warmth.deficit()
}

fn collect_heat_emitters(ctx: &DriftContext) -> Vec<(Vec2, f32)> {
    ctx.visible
        .iter()
        .filter(|(e, _)| ctx.mind.has_trait(&Node::Entity(*e), Concept::HeatEmitting))
        .map(|(_, pos)| (*pos, 1.0))
        .collect()
}

fn score_tile_warmth(inputs: &ScoringInputs, tile: IVec2) -> f32 {
    let center = tile_center_px(tile);
    let entity_pull: f32 = sum_inverse_distance(inputs.targets, center);
    let cell_temp = inputs.fields.temperature().sample_tile(tile);
    let field_pull = (cell_temp - COMFORT_MIN_C).max(0.0) * FIELD_PULL_WEIGHT;
    entity_pull + field_pull
}

fn get_deficit_social(ctx: &DriftContext) -> f32 {
    ctx.drives.map(|d| d.companionship.deficit()).unwrap_or(0.0)
}

fn collect_conspecifics(ctx: &DriftContext) -> Vec<(Vec2, f32)> {
    let Some(self_concept) = ctx.self_concept else {
        return Vec::new();
    };
    ctx.visible
        .iter()
        .filter(|(e, _)| is_conspecific(ctx.mind, *e, self_concept))
        .map(|(e, pos)| (*pos, read_affection(ctx.mind, *e)))
        .collect()
}

fn score_tile_social(inputs: &ScoringInputs, tile: IVec2) -> f32 {
    sum_inverse_distance(inputs.targets, tile_center_px(tile))
}

fn sum_inverse_distance(targets: &[(Vec2, f32)], at: Vec2) -> f32 {
    targets
        .iter()
        .map(|(pos, weight)| weight / (at.distance(*pos) + 1.0))
        .sum()
}

fn is_conspecific(mind: &MindGraph, entity: Entity, self_concept: Concept) -> bool {
    !mind
        .query(
            Some(&Node::Entity(entity)),
            Some(Predicate::IsA),
            Some(&Value::Concept(self_concept)),
        )
        .is_empty()
}

fn read_affection(mind: &MindGraph, entity: Entity) -> f32 {
    mind.get(&Node::Entity(entity), Predicate::Affection)
        .and_then(|v| v.as_quantity().map(|q| q.point_estimate()))
        .unwrap_or(0.5)
}

// ─── Low-level primitives exposed for action-prep scorers ───────────────

pub(crate) fn prep_collect_heat_emitters(ctx: &PreferenceContext) -> Vec<(Vec2, f32)> {
    collect_heat_emitters(ctx)
}

pub(crate) fn prep_collect_conspecifics(ctx: &PreferenceContext) -> Vec<(Vec2, f32)> {
    collect_conspecifics(ctx)
}

/// Pure inverse-distance sum at a tile center. Scorers pre-collect
/// targets once, then call this per tile.
pub(crate) fn prep_entity_pull(targets: &[(Vec2, f32)], tile: IVec2) -> f32 {
    sum_inverse_distance(targets, tile_center_px(tile))
}

/// Temperature-field contribution at a tile: amount by which the cell
/// exceeds the comfort floor, in the same units as `prep_entity_pull`.
pub(crate) fn prep_field_warmth(fields: &FieldGrids, tile: IVec2) -> f32 {
    let cell_temp = fields.temperature().sample_tile(tile);
    (cell_temp - COMFORT_MIN_C).max(0.0) * FIELD_PULL_WEIGHT
}

// ─── Action-prep pass (location_preference hook) ────────────────────────

/// Minimum score margin a neighbor tile must beat the current tile by
/// before we swap the action for a prep-walk. Prevents micro-oscillation
/// across tile boundaries.
const PREP_HYSTERESIS: f32 = 0.05;

/// Iterate the 9×9 tiles around `center` (excluding center itself) and
/// return the highest-scoring one. Shared between drift and action-prep.
fn best_neighbor_tile(center: IVec2, mut score: impl FnMut(IVec2) -> f32) -> Option<(IVec2, f32)> {
    let mut best: Option<(IVec2, f32)> = None;
    for dy in -DRIFT_RADIUS_TILES..=DRIFT_RADIUS_TILES {
        for dx in -DRIFT_RADIUS_TILES..=DRIFT_RADIUS_TILES {
            if dx == 0 && dy == 0 {
                continue;
            }
            let tile = center + IVec2::new(dx, dy);
            let s = score(tile);
            match best {
                Some((_, cur)) if s <= cur => {}
                _ => best = Some((tile, s)),
            }
        }
    }
    best
}

/// If the admitted proposal's action declares a `location_preference`
/// and a meaningfully-better neighbor tile exists, return a Walk
/// proposal toward that tile instead. Pass-through when no preference
/// is declared or when the current tile is already best.
///
/// The scorer is a batch `&[IVec2] -> Vec<f32>` so the scorer can
/// filter perceived entities once and score all candidate tiles
/// against the filtered set. Emergency semantics (e.g. exhausted
/// agent skipping prep) are expressed by the scorer returning
/// uniformly-zero scores — hysteresis then blocks any swap.
pub fn apply_location_preference(
    proposal: BrainProposal,
    ctx: &PreferenceContext,
    action_registry: &ActionRegistry,
) -> BrainProposal {
    let Some(action_def) = action_registry.get(proposal.action.action_type) else {
        return proposal;
    };
    let Some(scorer) = action_def.location_preference() else {
        return proposal;
    };

    let center_tile = world_pos_to_tile(ctx.agent_pos);
    let mut tiles: Vec<IVec2> =
        Vec::with_capacity(((2 * DRIFT_RADIUS_TILES + 1) * (2 * DRIFT_RADIUS_TILES + 1)) as usize);
    tiles.push(center_tile);
    for dy in -DRIFT_RADIUS_TILES..=DRIFT_RADIUS_TILES {
        for dx in -DRIFT_RADIUS_TILES..=DRIFT_RADIUS_TILES {
            if dx == 0 && dy == 0 {
                continue;
            }
            tiles.push(center_tile + IVec2::new(dx, dy));
        }
    }
    let scores = scorer(ctx, &tiles);
    let center_score = scores[0];
    let (best_idx, best_score) = scores
        .iter()
        .enumerate()
        .skip(1)
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, s)| (i, *s))
        .unwrap_or((0, center_score));

    if best_score <= center_score + PREP_HYSTERESIS {
        return proposal;
    }

    let best_tile = tiles[best_idx];
    let Some(walk) = action_registry.get(ActionType::Walk) else {
        return proposal;
    };
    let mut template = walk.to_template(None);
    template.target_position = Some(tile_center_px(best_tile));

    BrainProposal {
        action: template,
        reasoning: format!(
            "Prep for {:?}: walk to ({}, {}) before firing",
            proposal.action.action_type, best_tile.x, best_tile.y
        ),
        ..proposal
    }
}
