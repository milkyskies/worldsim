//! Fish swim movement: Boids steering for schooling fish, wander for solitary.
//!
//! Reads: Fish, FishHeading, Schooling, SpeciesProfile, WorldMap, SpatialIndex, TickCount
//! Writes: Transform, FishHeading
//! Upstream: world::fish (spawn components), world::map (water tiles), world::spatial_index
//! Downstream: rendering reads Transform + sprite_animation
//!
//! Drives all fish entities directly, bypassing the GOAP-driven Walk action.
//! Fish carry the full agent stack but `max_plan_depth = 1` means the planner
//! basically never produces multi-step plans for them — so locomotion is
//! handled here as reactive steering rather than goal-directed pathing. When
//! a fish acquires a real planned action in the future this system will need
//! to defer to it.

use bevy::prelude::*;
use rand::Rng;
use rand_chacha::ChaCha8Rng;
use std::collections::HashMap;

use crate::core::{SimRng, TickCount};
use crate::world::fish::{Fish, FishHeading, Schooling};
use crate::world::map::{TILE_SIZE, TileType, WorldMap};
use crate::world::spatial_index::SpatialIndex;

/// World-pixel maximum heading change per tick. Lower = smoother turns,
/// higher = jerky. Calibrated for fish — they don't pivot on a dime.
const TURN_RATE_PER_TICK: f32 = 0.18;

/// Faster heading-blend rate used when the shore-avoid force is active.
/// Snappier than the normal turn so a fish heading toward a wall actually
/// rotates away before it gets there — the calm gliding turn rate is too
/// slow to handle tight river meanders.
const EMERGENCY_TURN_RATE: f32 = 0.6;

/// Position-delta squared below which the fish is considered "stuck" — no
/// candidate position passed the water check this tick. One pixel² is well
/// below any meaningful per-tick swim step.
const STUCK_EPSILON_SQ: f32 = 0.25;

/// Strength of the random-wander component added every tick. Keeps a
/// solitary fish from sitting still and gives schools a touch of life
/// inside their flocking forces.
const WANDER_WEIGHT: f32 = 0.35;

/// World-pixel lookahead used to detect whether the fish is about to leave
/// the water body. If the tile at `position + heading * lookahead` is land,
/// the fish steers back inward instead of swimming through the bank.
const WATER_LOOKAHEAD: f32 = 12.0;

/// Strength of the "steer back into water" force when the lookahead lands
/// on dry land. Should dominate other forces so fish never beach themselves.
const SHORE_AVOID_WEIGHT: f32 = 4.0;

pub struct FishMovementPlugin;

impl Plugin for FishMovementPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Fish>()
            .register_type::<crate::world::fish::Minnow>()
            .register_type::<crate::world::fish::Pike>()
            .register_type::<FishHeading>()
            .register_type::<Schooling>()
            .add_systems(FixedUpdate, swim_fish);
    }
}

/// Snapshot of a fish's pose for one tick. Built up-front so we can
/// borrow `&mut FishHeading + &mut Transform` while still computing
/// each fish's neighbour forces against everyone else's pre-tick state.
#[derive(Clone, Copy)]
struct FishSnapshot {
    pos: Vec2,
    heading: Vec2,
}

/// Per-tick fish steering. Schooling fish run Boids against their nearby
/// school-mates; solitary fish only wander. All fish are clamped to water
/// tiles via a lookahead shore-avoid force.
pub fn swim_fish(
    mut fish_q: Query<(Entity, &mut FishHeading, &mut Transform, Option<&Schooling>), With<Fish>>,
    map: Res<WorldMap>,
    spatial: Res<SpatialIndex>,
    tick: Res<TickCount>,
    mut sim_rng: ResMut<SimRng>,
) {
    // Snapshot every fish's pose first so neighbour lookups don't need a
    // second `Query<&Transform, &FishHeading>` (which would conflict with
    // the mut query and panic at runtime).
    let snapshots: HashMap<Entity, FishSnapshot> = fish_q
        .iter()
        .map(|(e, h, t, _)| {
            (
                e,
                FishSnapshot {
                    pos: t.translation.truncate(),
                    heading: h.heading,
                },
            )
        })
        .collect();

    let rng = sim_rng.inner_mut();
    for (entity, mut heading, mut transform, schooling) in fish_q.iter_mut() {
        let pos = transform.translation.truncate();
        let mut steer = Vec2::ZERO;

        if let Some(school) = schooling {
            steer += boids_steer(entity, pos, heading.heading, school, &spatial, &snapshots);
        }

        steer += wander_steer(rng) * WANDER_WEIGHT;
        let shore = shore_avoid_steer(pos, heading.heading, &map);
        steer += shore;

        // Tick-stepped heading update: blend steer into heading at the
        // normal turn rate, but bump to an emergency rate when shore-avoid
        // is firing so the fish actually rotates away from the bank before
        // it reaches it. Without this the slow Boids-friendly turn rate
        // lets fish sail straight into shallows during the heading lerp.
        let desired = (heading.heading + steer).normalize_or(heading.heading);
        let turn_rate = if shore == Vec2::ZERO {
            TURN_RATE_PER_TICK
        } else {
            EMERGENCY_TURN_RATE
        };
        heading.heading = lerp_unit(heading.heading, desired, turn_rate);

        // Step position along heading. Axis-slide lets fish glide along a
        // shore when the diagonal candidate is land but one cardinal axis
        // still leaves them in water.
        let step = heading.heading * heading.speed;
        let candidate = pos + step;
        let next_pos = if is_water(&map, candidate) {
            candidate
        } else {
            let only_x = pos + Vec2::new(step.x, 0.0);
            let only_y = pos + Vec2::new(0.0, step.y);
            if is_water(&map, only_x) {
                only_x
            } else if is_water(&map, only_y) {
                only_y
            } else {
                pos
            }
        };

        // Stuck escape: if we couldn't move at all this tick (every fallback
        // hit land), the fish is wedged into a concave shore — flip the
        // heading 180° immediately so next tick steps outward instead of
        // grinding into the same wall. Without this fish snag on river
        // dead-ends and oscillate forever between failed candidates.
        let stuck = (next_pos - pos).length_squared() < STUCK_EPSILON_SQ;
        if stuck {
            heading.heading = -heading.heading;
        }

        transform.translation.x = next_pos.x;
        transform.translation.y = next_pos.y;

        let _ = tick.current; // reserved for future tick-aware behaviour
    }
}

/// Reynolds (1987) Boids steering: separation, alignment, cohesion. Returns
/// a single steering force in heading units; the caller blends it into the
/// fish's heading at the global turn rate.
fn boids_steer(
    self_entity: Entity,
    self_pos: Vec2,
    self_heading: Vec2,
    school: &Schooling,
    spatial: &SpatialIndex,
    snapshots: &HashMap<Entity, FishSnapshot>,
) -> Vec2 {
    let neighbours = spatial.entities_near(self_pos, school.neighbour_radius);
    let mut sep = Vec2::ZERO;
    let mut align = Vec2::ZERO;
    let mut cohesion_centroid = Vec2::ZERO;
    let mut neighbour_count: usize = 0;

    let radius_sq = school.neighbour_radius * school.neighbour_radius;
    let sep_radius_sq = school.separation_radius * school.separation_radius;

    for other in neighbours {
        if other == self_entity {
            continue;
        }
        let Some(snap) = snapshots.get(&other) else {
            continue;
        };
        let to_other = snap.pos - self_pos;
        let dist_sq = to_other.length_squared();
        if dist_sq > radius_sq || dist_sq <= 1e-4 {
            continue;
        }

        neighbour_count += 1;
        align += snap.heading;
        cohesion_centroid += snap.pos;

        if dist_sq < sep_radius_sq {
            // Repulsion strength scales inversely with distance — closer
            // neighbours push back harder.
            sep -= to_other / dist_sq.max(1e-4);
        }
    }

    if neighbour_count == 0 {
        return Vec2::ZERO;
    }

    let alignment = (align / neighbour_count as f32 - self_heading) * school.alignment_weight;
    let cohesion = (cohesion_centroid / neighbour_count as f32 - self_pos).normalize_or_zero()
        * school.cohesion_weight;
    let separation = sep * school.separation_weight;

    alignment + cohesion + separation
}

fn wander_steer(rng: &mut ChaCha8Rng) -> Vec2 {
    let theta = rng.random_range(0.0..std::f32::consts::TAU);
    Vec2::new(theta.cos(), theta.sin())
}

/// Steers the fish back toward the water if the tile a short distance ahead
/// is land. Strong weight so it dominates Boids when a school's centroid
/// drifts toward shore.
fn shore_avoid_steer(pos: Vec2, heading: Vec2, map: &WorldMap) -> Vec2 {
    let lookahead = pos + heading * WATER_LOOKAHEAD;
    if is_water(map, lookahead) {
        return Vec2::ZERO;
    }
    // Probe eight directions (cardinals + diagonals) one-and-a-half tiles
    // out so we find escape routes in concave corners that a 4-direction
    // scan would miss. Probe distance must exceed one tile so the probe
    // actually crosses into a neighbouring tile.
    let probe_dist = TILE_SIZE * 1.5;
    let inv = std::f32::consts::FRAC_1_SQRT_2;
    let dirs = [
        Vec2::X,
        -Vec2::X,
        Vec2::Y,
        -Vec2::Y,
        Vec2::new(inv, inv),
        Vec2::new(-inv, inv),
        Vec2::new(inv, -inv),
        Vec2::new(-inv, -inv),
    ];
    // Pick the probe most opposite the fish's current heading among those
    // that land in water — that's the deepest direction back into the body.
    // Fall back to a u-turn if none find water.
    let mut best: Option<Vec2> = None;
    let mut best_score = f32::NEG_INFINITY;
    for dir in dirs {
        let probe = pos + dir * probe_dist;
        if !is_water(map, probe) {
            continue;
        }
        let score = -dir.dot(heading); // higher = more opposite
        if score > best_score {
            best_score = score;
            best = Some(dir);
        }
    }
    let target = best.unwrap_or(-heading);
    target * SHORE_AVOID_WEIGHT
}

fn is_water(map: &WorldMap, pos: Vec2) -> bool {
    matches!(
        map.tile_at(pos),
        Some(TileType::Water | TileType::ShallowWater)
    )
}

/// Spherical-style lerp on the unit circle: rotate `from` toward `to` by at
/// most `rate` of the angular gap. Returns a unit vector.
fn lerp_unit(from: Vec2, to: Vec2, rate: f32) -> Vec2 {
    let blended = from * (1.0 - rate) + to * rate;
    let len = blended.length();
    if len < 1e-4 { from } else { blended / len }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::fish::Schooling;
    use crate::world::map::{CHUNK_SIZE, Chunk, TILE_SIZE};
    use bevy::math::IVec2;

    fn empty_water_map() -> WorldMap {
        let mut map = WorldMap::new(CHUNK_SIZE, CHUNK_SIZE);
        map.chunks.insert(IVec2::ZERO, Chunk::new(0, 0));
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                map.set_tile(x, y, TileType::Water);
            }
        }
        map
    }

    #[test]
    fn shore_avoid_returns_zero_inside_water() {
        let map = empty_water_map();
        // Center of an all-water 16x16 map.
        let pos = Vec2::splat(CHUNK_SIZE as f32 * TILE_SIZE * 0.5);
        let force = shore_avoid_steer(pos, Vec2::X, &map);
        assert_eq!(force, Vec2::ZERO);
    }

    #[test]
    fn shore_avoid_pushes_back_when_heading_into_land() {
        let mut map = empty_water_map();
        // Wall off the eastern half so the lookahead reliably lands on grass.
        for x in 12..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                map.set_tile(x, y, TileType::Grass);
            }
        }
        // Place the fish at the center of the rightmost water tile (column
        // 11), heading east into the land wall. Lookahead = 12 px puts the
        // probe inside column 12 = grass.
        let pos = Vec2::new(11.5 * TILE_SIZE, 8.0 * TILE_SIZE);
        let force = shore_avoid_steer(pos, Vec2::X, &map);
        assert!(
            force != Vec2::ZERO,
            "shore avoid must fire when heading into land"
        );
        assert!(
            force.x < 0.0,
            "must steer away from land on the east; got {force:?}"
        );
    }

    #[test]
    fn lerp_unit_partial_step_lies_between() {
        let from = Vec2::X;
        let to = Vec2::Y;
        let mid = lerp_unit(from, to, 0.5);
        assert!(mid.x > 0.0 && mid.x < 1.0);
        assert!(mid.y > 0.0 && mid.y < 1.0);
        assert!((mid.length() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn lerp_unit_zero_rate_returns_origin() {
        let from = Vec2::new(1.0, 0.0);
        let to = Vec2::new(0.0, 1.0);
        let r = lerp_unit(from, to, 0.0);
        assert!((r - from).length() < 1e-4);
    }

    #[test]
    fn schooling_default_separation_inside_neighbour_radius() {
        let s = Schooling::default();
        assert!(
            s.separation_radius < s.neighbour_radius,
            "separation radius must be tighter than neighbour radius"
        );
    }
}
