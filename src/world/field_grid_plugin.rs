//! Plugin + systems for the scalar temperature field.

use bevy::math::IVec2;
use bevy::prelude::*;

use crate::constants::thermal::{
    AMBIENT_RELAXATION_PER_SEC, DAY_AMBIENT_C, DIFFUSION_BLEND, DIFFUSION_PERIOD_TICKS,
    EQUILIBRIUM_EPSILON_C, INJECTION_RATE_AT_SOURCE_C_PER_SEC, LIGHT_AT_NIGHT, NIGHT_AMBIENT_C,
    PRUNE_PERIOD_TICKS,
};
use crate::core::tick::TickCount;
use crate::world::environment::LightLevel;
use crate::world::field_grid::{FIELD_CHUNK_SIZE, FieldGrid};
use crate::world::map::TILE_SIZE;
use crate::world::property::{HeatSource, ShelterProvider};
use crate::world::spatial_index::world_pos_to_tile;

#[derive(Resource)]
pub struct FieldGrids {
    temperature: FieldGrid,
}

impl FieldGrids {
    pub fn temperature(&self) -> &FieldGrid {
        &self.temperature
    }

    pub fn temperature_mut(&mut self) -> &mut FieldGrid {
        &mut self.temperature
    }
}

impl Default for FieldGrids {
    fn default() -> Self {
        Self {
            temperature: FieldGrid::new(DAY_AMBIENT_C),
        }
    }
}

pub struct FieldGridPlugin;

impl Plugin for FieldGridPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FieldGrids>().add_systems(
            FixedUpdate,
            (
                update_thermal_ambient,
                inject_heat_emitters,
                inject_shelter_providers,
                relax_fields_toward_ambient,
                diffuse_temperature,
                prune_equilibrated_chunks,
            )
                .chain()
                // Must see this tick's LightLevel, or day/night transitions
                // lag the grid by a frame.
                .after(crate::world::environment::update_light_level),
        );
    }
}

fn update_thermal_ambient(light: Res<LightLevel>, mut grids: ResMut<FieldGrids>) {
    let t = ((light.0 - LIGHT_AT_NIGHT) / (1.0 - LIGHT_AT_NIGHT)).clamp(0.0, 1.0);
    let ambient = NIGHT_AMBIENT_C + (DAY_AMBIENT_C - NIGHT_AMBIENT_C) * t;
    grids.temperature_mut().set_ambient(ambient);
}

fn inject_heat_emitters(
    tick: Res<TickCount>,
    mut grids: ResMut<FieldGrids>,
    sources: Query<(&Transform, &HeatSource)>,
) {
    let dt = tick.dt();
    let grid = grids.temperature_mut();
    for (transform, source) in sources.iter() {
        inject_radial(
            grid,
            transform.translation.truncate(),
            source.radius,
            source.intensity * INJECTION_RATE_AT_SOURCE_C_PER_SEC * dt,
        );
    }
}

/// Shelter's contribution to the temperature field — not active heat,
/// but a milder bump than a fire. Uses `protection` as the effective
/// radius and a fixed fraction of the full heat-emitter rate.
const SHELTER_INJECTION_FRACTION: f32 = 0.25;

fn inject_shelter_providers(
    tick: Res<TickCount>,
    mut grids: ResMut<FieldGrids>,
    shelters: Query<(&Transform, &ShelterProvider)>,
) {
    let dt = tick.dt();
    let grid = grids.temperature_mut();
    for (transform, shelter) in shelters.iter() {
        inject_radial(
            grid,
            transform.translation.truncate(),
            shelter.protection,
            SHELTER_INJECTION_FRACTION * INJECTION_RATE_AT_SOURCE_C_PER_SEC * dt,
        );
    }
}

fn inject_radial(grid: &mut FieldGrid, origin: Vec2, radius_px: f32, peak_per_tick: f32) {
    if peak_per_tick <= 0.0 || radius_px <= 0.0 {
        return;
    }
    let center_tile = world_pos_to_tile(origin);
    let radius_tiles = (radius_px / TILE_SIZE).ceil() as i32;
    for dy in -radius_tiles..=radius_tiles {
        for dx in -radius_tiles..=radius_tiles {
            let tile = center_tile + IVec2::new(dx, dy);
            let tile_center_px = Vec2::new(
                (tile.x as f32 + 0.5) * TILE_SIZE,
                (tile.y as f32 + 0.5) * TILE_SIZE,
            );
            let distance = origin.distance(tile_center_px);
            if distance >= radius_px {
                continue;
            }
            let falloff = 1.0 - (distance / radius_px);
            grid.inject_at_tile(tile, peak_per_tick * falloff);
        }
    }
}

fn relax_fields_toward_ambient(tick: Res<TickCount>, mut grids: ResMut<FieldGrids>) {
    let retain = (1.0 - AMBIENT_RELAXATION_PER_SEC * tick.dt()).max(0.0);
    for (_, chunk) in grids.temperature_mut().chunks_mut().iter_mut() {
        for delta in chunk.deltas_mut().iter_mut() {
            *delta *= retain;
        }
    }
}

/// Spatial diffusion: each cell blends toward the 4-neighbor average.
/// Iterates active chunks plus a one-chunk border ring so heat at the
/// edge of an active chunk can flow into previously-inactive neighbors
/// instead of evaporating at the boundary.
fn diffuse_temperature(tick: Res<TickCount>, mut grids: ResMut<FieldGrids>) {
    if !tick.current.is_multiple_of(DIFFUSION_PERIOD_TICKS) {
        return;
    }
    let grid = grids.temperature_mut();

    let mut relevant_chunks: std::collections::HashSet<IVec2> = std::collections::HashSet::new();
    for (coord, _) in grid.iter_chunks() {
        for dy in -1..=1 {
            for dx in -1..=1 {
                relevant_chunks.insert(coord + IVec2::new(dx, dy));
            }
        }
    }

    let mut updates: Vec<(IVec2, f32)> = Vec::new();
    for chunk_coord in &relevant_chunks {
        for local_y in 0..FIELD_CHUNK_SIZE {
            for local_x in 0..FIELD_CHUNK_SIZE {
                let world_tile = IVec2::new(
                    chunk_coord.x * FIELD_CHUNK_SIZE + local_x,
                    chunk_coord.y * FIELD_CHUNK_SIZE + local_y,
                );
                let current = grid.delta_at_tile(world_tile);
                let neighbor_avg = 0.25
                    * (grid.delta_at_tile(world_tile + IVec2::new(1, 0))
                        + grid.delta_at_tile(world_tile + IVec2::new(-1, 0))
                        + grid.delta_at_tile(world_tile + IVec2::new(0, 1))
                        + grid.delta_at_tile(world_tile + IVec2::new(0, -1)));
                let new_delta = current + (neighbor_avg - current) * DIFFUSION_BLEND;
                if (new_delta - current).abs() < 1e-5 {
                    continue;
                }
                updates.push((world_tile, new_delta));
            }
        }
    }

    for (tile, delta) in updates {
        grid.set_delta_at_tile(tile, delta);
    }
}

fn prune_equilibrated_chunks(tick: Res<TickCount>, mut grids: ResMut<FieldGrids>) {
    if !tick.current.is_multiple_of(PRUNE_PERIOD_TICKS) {
        return;
    }
    grids
        .temperature_mut()
        .prune_equilibrated(EQUILIBRIUM_EPSILON_C);
}
