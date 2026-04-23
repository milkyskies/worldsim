//! Reactive drift: tile-based local-gradient sampling.
//!
//! Each active drive scores the agent's 9×9 tile neighborhood by summing
//! perceived-entity pulls and field samples; the agent walks toward the
//! highest-scored tile. Adding a new drive is one `DriveBehavior` entry
//! plus one scorer function.

use bevy::math::{IVec2, Vec2};
use bevy::prelude::Entity;

use crate::agent::actions::{ActionRegistry, ActionType};
use crate::agent::body::needs::{PhysicalNeeds, PsychologicalDrives};
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use crate::agent::nervous_system::urgency::UrgencySource;
use crate::constants::brains::emotional::{
    SOCIAL_SEEK_THRESHOLD, SOCIAL_SEEK_URGENCY_MULTIPLIER, WARMTH_SEEK_THRESHOLD,
    WARMTH_SEEK_URGENCY_MULTIPLIER,
};
use crate::constants::thermal::COMFORT_MIN_C;
use crate::world::field_grid_plugin::FieldGrids;
use crate::world::spatial_index::{tile_center_px, world_pos_to_tile};

use super::proposal::{BrainProposal, BrainType, Intent};

/// 4 tiles = 64px radius = 9×9 = 81 tiles scored per active drive.
const DRIFT_RADIUS_TILES: i32 = 4;

/// How strongly a positive field delta (above the comfort floor) pulls
/// compared to entity proximity. Tuned so a single nearby campfire
/// entity outranks a single warm tile — entities are the first-order
/// signal, field values are the corrective fine-grain layer.
const FIELD_PULL_WEIGHT: f32 = 0.05;

pub(super) struct DriftContext<'a> {
    pub agent_pos: Vec2,
    pub self_concept: Option<Concept>,
    pub physical: &'a PhysicalNeeds,
    pub drives: Option<&'a PsychologicalDrives>,
    pub mind: &'a MindGraph,
    /// (entity, world position) for every currently visible entity,
    /// pre-resolved so the per-tile scoring loop doesn't hit the ECS.
    pub visible: &'a [(Entity, Vec2)],
    pub fields: &'a FieldGrids,
}

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
    let mut best: Option<(IVec2, f32)> = None;
    for dy in -DRIFT_RADIUS_TILES..=DRIFT_RADIUS_TILES {
        for dx in -DRIFT_RADIUS_TILES..=DRIFT_RADIUS_TILES {
            if dx == 0 && dy == 0 {
                continue;
            }
            let tile = center_tile + IVec2::new(dx, dy);
            let score = (behavior.score_tile)(&inputs, tile);
            if score <= 0.0 {
                continue;
            }
            match best {
                Some((_, s)) if score <= s => {}
                _ => best = Some((tile, score)),
            }
        }
    }

    let (target_tile, best_score) = best?;
    // Don't propose a move when the current tile is already at-or-better.
    // Keeps the agent from oscillating once they arrive.
    if best_score <= center_score {
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
