use crate::menu::{AppState, SimConfig};
use crate::world::environment::BaseColor;
use bevy::prelude::*;
use noise::{NoiseFn, Simplex};
use std::collections::HashMap;

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<TileMap>()
            .register_type::<Tile>()
            .register_type::<TileType>()
            .insert_resource(WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT))
            .add_systems(OnEnter(AppState::InSim), setup_map);
    }
}

// World size constants
pub const TILE_SIZE: f32 = 16.0;
pub const CHUNK_SIZE: u32 = 16;
pub const MAP_CHUNKS_X: u32 = 13;
pub const MAP_CHUNKS_Y: u32 = 13;
// Derived constants
pub const WORLD_WIDTH: u32 = MAP_CHUNKS_X * CHUNK_SIZE;
pub const WORLD_HEIGHT: u32 = MAP_CHUNKS_Y * CHUNK_SIZE;

/// Default seed for terrain generation. Stable across runs for reproducibility.
pub const DEFAULT_TERRAIN_SEED: u32 = 1337;

// Marker component for the tile map parent entity
#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct TileMap;

#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct Tile {
    pub x: i32,
    pub y: i32,
    pub tile_type: TileType,
    /// Absolute elevation, range 0.0..=255.0.
    /// 0.0 = ocean floor, 64.0 = sea level, 255.0 = highest peaks.
    pub elevation: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum TileType {
    Grass,
    Dirt,
    Gravel,
    Rock,
    Sand,
    Water,
    ShallowWater,
}

impl TileType {
    /// Whether this tile is a water tile (deep or shallow).
    pub fn is_water(&self) -> bool {
        matches!(self, TileType::Water | TileType::ShallowWater)
    }

    /// Whether agents can traverse this tile at all.
    pub fn is_walkable(&self) -> bool {
        !matches!(self, TileType::Water)
    }

    /// Movement speed multiplier for traversing this tile (1.0 = full speed).
    /// Returns 0.0 for impassable tiles.
    pub fn speed_multiplier(&self) -> f32 {
        match self {
            TileType::Grass => 1.0,
            TileType::Sand => 0.8,
            TileType::Dirt => 0.6,
            TileType::Gravel => 0.5,
            TileType::Rock => 0.4,
            TileType::ShallowWater => 0.3,
            TileType::Water => 0.0,
        }
    }

    /// Render color for this tile type.
    pub fn color(&self) -> Color {
        match self {
            TileType::Grass => Color::srgb(0.34, 0.72, 0.30),
            TileType::Dirt => Color::srgb(0.55, 0.40, 0.26),
            TileType::Gravel => Color::srgb(0.58, 0.54, 0.48),
            TileType::Rock => Color::srgb(0.50, 0.48, 0.46),
            TileType::Sand => Color::srgb(0.88, 0.80, 0.55),
            TileType::ShallowWater => Color::srgb(0.40, 0.65, 0.85),
            TileType::Water => Color::srgb(0.15, 0.30, 0.70),
        }
    }
}

#[derive(Clone, Reflect)]
pub struct Chunk {
    pub x: i32,
    pub y: i32,
    pub tiles: Vec<TileType>,
    pub elevations: Vec<f32>,
}

impl Chunk {
    pub fn new(x: i32, y: i32) -> Self {
        let size = (CHUNK_SIZE * CHUNK_SIZE) as usize;
        Self {
            x,
            y,
            tiles: vec![TileType::Grass; size],
            elevations: vec![0.0; size],
        }
    }

    pub fn get_tile(&self, tx: u32, ty: u32) -> Option<TileType> {
        if tx >= CHUNK_SIZE || ty >= CHUNK_SIZE {
            return None;
        }
        Some(self.tiles[(ty * CHUNK_SIZE + tx) as usize])
    }

    pub fn set_tile(&mut self, tx: u32, ty: u32, tile: TileType) {
        if tx < CHUNK_SIZE && ty < CHUNK_SIZE {
            self.tiles[(ty * CHUNK_SIZE + tx) as usize] = tile;
        }
    }

    pub fn get_elevation(&self, tx: u32, ty: u32) -> Option<f32> {
        if tx >= CHUNK_SIZE || ty >= CHUNK_SIZE {
            return None;
        }
        Some(self.elevations[(ty * CHUNK_SIZE + tx) as usize])
    }

    pub fn set_elevation(&mut self, tx: u32, ty: u32, elevation: f32) {
        if tx < CHUNK_SIZE && ty < CHUNK_SIZE {
            self.elevations[(ty * CHUNK_SIZE + tx) as usize] = elevation;
        }
    }
}

#[derive(Resource)]
pub struct WorldMap {
    pub width: u32,
    pub height: u32,
    pub chunks: HashMap<IVec2, Chunk>,
}

impl WorldMap {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            chunks: HashMap::new(),
        }
    }

    pub fn get_tile(&self, x: u32, y: u32) -> Option<TileType> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let chunk_x = (x / CHUNK_SIZE) as i32;
        let chunk_y = (y / CHUNK_SIZE) as i32;
        let local_x = x % CHUNK_SIZE;
        let local_y = y % CHUNK_SIZE;

        self.chunks
            .get(&IVec2::new(chunk_x, chunk_y))
            .and_then(|chunk| chunk.get_tile(local_x, local_y))
    }

    pub fn set_tile(&mut self, x: u32, y: u32, tile: TileType) {
        if x >= self.width || y >= self.height {
            return;
        }
        let chunk_x = (x / CHUNK_SIZE) as i32;
        let chunk_y = (y / CHUNK_SIZE) as i32;
        let local_x = x % CHUNK_SIZE;
        let local_y = y % CHUNK_SIZE;

        if let Some(chunk) = self.chunks.get_mut(&IVec2::new(chunk_x, chunk_y)) {
            chunk.set_tile(local_x, local_y, tile);
        }
    }

    /// Check if a world position is within bounds
    pub fn in_bounds(&self, pos: Vec2) -> bool {
        pos.x >= 0.0
            && pos.y >= 0.0
            && pos.x < (self.width as f32 * TILE_SIZE)
            && pos.y < (self.height as f32 * TILE_SIZE)
    }

    /// Convert world position to tile coordinates
    pub fn world_to_tile(&self, pos: Vec2) -> (u32, u32) {
        (
            (pos.x / TILE_SIZE).floor().max(0.0) as u32,
            (pos.y / TILE_SIZE).floor().max(0.0) as u32,
        )
    }

    /// Convert tile coordinates to world position (center of tile)
    pub fn tile_to_world(&self, x: i32, y: i32) -> Vec2 {
        Vec2::new(
            x as f32 * TILE_SIZE + TILE_SIZE / 2.0,
            y as f32 * TILE_SIZE + TILE_SIZE / 2.0,
        )
    }

    /// Look up the tile type at a world position, if any.
    pub fn tile_at(&self, pos: Vec2) -> Option<TileType> {
        if !self.in_bounds(pos) {
            return None;
        }
        let (tx, ty) = self.world_to_tile(pos);
        self.get_tile(tx, ty)
    }

    /// Check if a world position is walkable (in bounds and not impassable terrain).
    pub fn is_walkable(&self, pos: Vec2) -> bool {
        self.tile_at(pos).is_some_and(|t| t.is_walkable())
    }

    /// Movement speed multiplier at a world position. Returns 0.0 for blocked or out-of-bounds.
    pub fn speed_at(&self, pos: Vec2) -> f32 {
        self.tile_at(pos)
            .map(|t| t.speed_multiplier())
            .unwrap_or(0.0)
    }

    /// Get pixel bounds of the map
    pub fn pixel_bounds(&self) -> (f32, f32) {
        (
            self.width as f32 * TILE_SIZE,
            self.height as f32 * TILE_SIZE,
        )
    }

    /// Elevation of a tile (0.0..=255.0). Returns `None` if out of bounds.
    pub fn elevation_at(&self, x: u32, y: u32) -> Option<f32> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let chunk_x = (x / CHUNK_SIZE) as i32;
        let chunk_y = (y / CHUNK_SIZE) as i32;
        let local_x = x % CHUNK_SIZE;
        let local_y = y % CHUNK_SIZE;
        self.chunks
            .get(&IVec2::new(chunk_x, chunk_y))
            .and_then(|chunk| chunk.get_elevation(local_x, local_y))
    }

    /// Store an elevation value for a tile. Silently ignored if out of bounds.
    pub fn set_elevation(&mut self, x: u32, y: u32, elevation: f32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let chunk_x = (x / CHUNK_SIZE) as i32;
        let chunk_y = (y / CHUNK_SIZE) as i32;
        let local_x = x % CHUNK_SIZE;
        let local_y = y % CHUNK_SIZE;
        if let Some(chunk) = self.chunks.get_mut(&IVec2::new(chunk_x, chunk_y)) {
            chunk.set_elevation(local_x, local_y, elevation);
        }
    }

    /// Signed slope between two tile positions: (to_elevation - from_elevation) / distance.
    /// Positive = uphill, negative = downhill, zero = flat.
    pub fn slope(&self, from: (u32, u32), to: (u32, u32)) -> f32 {
        let from_e = self.elevation_at(from.0, from.1).unwrap_or(0.0);
        let to_e = self.elevation_at(to.0, to.1).unwrap_or(0.0);
        let dx = to.0 as f32 - from.0 as f32;
        let dy = to.1 as f32 - from.1 as f32;
        let dist = (dx * dx + dy * dy).sqrt().max(1.0);
        (to_e - from_e) / dist
    }

    /// Returns `true` when moving from `from` to `to` goes uphill.
    pub fn is_uphill(&self, from: (u32, u32), to: (u32, u32)) -> bool {
        self.slope(from, to) > 0.0
    }
}

/// Shape of the island mask. Lower = smoother, wider transition; higher = sharper edges.
const ISLAND_FALLOFF_EXPONENT: f64 = 1.8;

/// Strength of low-frequency noise added to the radial distance, producing gentle bays.
/// Kept low so the coastline stays smooth instead of forming tentacle peninsulas.
const COAST_NOISE_STRENGTH: f64 = 0.07;

/// Mix factor for the island mask in `sample()`. 0.0 = no island shape (pure
/// noise terrain edge-to-edge, current playtest default). 1.0 = full island
/// with deep-water corners and a beach ring. Flip up for a tropical-island
/// scenario; the radial-falloff math is still in `sample()`, just dormant.
const ISLAND_MASK_STRENGTH: f64 = 0.0;

/// Noise fields for terrain generation.
///
/// Combines fBm rolling hills with domain warping for organic shapes,
/// a low-frequency mountain mask (~5 % of the map), ridged multifractal
/// noise in mountain zones for sharp peaks, and a radial island mask
/// with noisy coastline so the world reads as a single connected island.
struct TerrainNoise {
    elevation: Simplex,
    warp_x: Simplex,
    warp_y: Simplex,
    mountain_mask: Simplex,
    ridge: Simplex,
    coast: Simplex,
    width: u32,
    height: u32,
}

impl TerrainNoise {
    fn new(seed: u32, width: u32, height: u32) -> Self {
        Self {
            elevation: Simplex::new(seed),
            warp_x: Simplex::new(seed.wrapping_add(3)),
            warp_y: Simplex::new(seed.wrapping_add(4)),
            mountain_mask: Simplex::new(seed.wrapping_add(5)),
            ridge: Simplex::new(seed.wrapping_add(6)),
            coast: Simplex::new(seed.wrapping_add(7)),
            width,
            height,
        }
    }

    /// Returns an elevation value in roughly the [-1.0, 1.0] range.
    ///
    /// Smooth fBm shapes plus rare mountain ridges, biased by an additive
    /// island mask. The bias is small enough that center tiles read as grass
    /// by default, sloping into dirt/gravel/rock only where the (tightly
    /// thresholded) mountain mask fires.
    fn sample(&self, x: u32, y: u32) -> f64 {
        const BASE: f64 = 0.030;
        let bx = x as f64 * BASE;
        let by = y as f64 * BASE;

        const WARP_STRENGTH: f64 = 0.10;
        let wx = bx + self.warp_x.get([bx * 0.5, by * 0.5]) * WARP_STRENGTH;
        let wy = by + self.warp_y.get([bx * 0.5 + 50.0, by * 0.5 + 50.0]) * WARP_STRENGTH;

        let hills = (self.elevation.get([wx, wy]) * 0.85
            + self.elevation.get([wx * 2.1, wy * 2.1]) * 0.15)
            .clamp(-1.0, 1.0);

        // Mountain mask: Minecraft-style — chunky 50-tile features at low
        // threshold so rocky terrain reliably covers a meaningful share of
        // the map. Combined with the lower biome thresholds, this gives
        // dramatic slope-to-rock transitions instead of subtle peaks.
        const MASK_FREQ: f64 = 0.020;
        const MASK_THRESHOLD: f64 = 0.10;
        let raw_mask = self.mountain_mask.get([bx * MASK_FREQ, by * MASK_FREQ]);
        let mask = ((raw_mask - MASK_THRESHOLD) / (1.0 - MASK_THRESHOLD)).clamp(0.0, 1.0);
        // Mountain mask: rare dramatic peaks when noise happens to fire.
        // Reliable baseline rock/gravel comes from `place_rocky_outcrops`,
        // so this can stay rare without breaking resource gathering.
        let peak = self.ridge.get([wx * 1.2, wy * 1.2]).max(0.0);
        let mountain_boost = mask * peak * 1.5;

        let cx = self.width as f64 / 2.0;
        let cy = self.height as f64 / 2.0;
        let dx = (x as f64 - cx) / cx;
        let dy = (y as f64 - cy) / cy;
        const COAST_FREQ: f64 = 0.010;
        let coast_offset = self
            .coast
            .get([x as f64 * COAST_FREQ, y as f64 * COAST_FREQ])
            * COAST_NOISE_STRENGTH;
        let dist = ((dx * dx + dy * dy).sqrt() + coast_offset).clamp(0.0, 1.0);
        let falloff = (1.0 - dist.powf(ISLAND_FALLOFF_EXPONENT)).max(0.0);

        // Island mask: applied at strength `ISLAND_MASK_STRENGTH`. At 0 the
        // bias term zeroes out and we get plain noise terrain edge-to-edge
        // (mountains can appear anywhere). At 1.0 the full island shape
        // returns: center stays grass, corners forced into deep water,
        // mountains gated to interior.
        let land_bias = (falloff * 1.10 - 0.95) * ISLAND_MASK_STRENGTH;
        let mountain_gate = 1.0 - ISLAND_MASK_STRENGTH + ISLAND_MASK_STRENGTH * falloff;
        let base = hills * 0.50 + land_bias;
        (base + mountain_boost * mountain_gate).clamp(-1.0, 1.0)
    }
}

/// Elevation thresholds (in noise output space, roughly [-1.0, 1.0])
/// that decide which biome a tile belongs to. Subtle land variation
/// (low noise → mostly grass with occasional dirt patches) — reliable
/// stone comes from `place_rocky_outcrops`, not from this gradient.
/// Loose water bands so ponds form naturally without the island mask.
mod biome {
    pub const DEEP_WATER_MAX: f64 = -0.45;
    pub const SHALLOW_WATER_MAX: f64 = -0.32;
    pub const SAND_MAX: f64 = -0.22;
    pub const DIRT_MIN: f64 = 0.35;
    pub const GRAVEL_MIN: f64 = 0.45;
    pub const ROCK_MIN: f64 = 0.55;
}

/// Classify a tile from elevation noise value.
///
/// Water → Sand → Grass → Dirt (foothills) → Gravel (scree) → Rock (peak).
fn classify_tile(elevation: f64) -> TileType {
    if elevation < biome::DEEP_WATER_MAX {
        TileType::Water
    } else if elevation < biome::SHALLOW_WATER_MAX {
        TileType::ShallowWater
    } else if elevation < biome::SAND_MAX {
        TileType::Sand
    } else if elevation > biome::ROCK_MIN {
        TileType::Rock
    } else if elevation > biome::GRAVEL_MIN {
        TileType::Gravel
    } else if elevation > biome::DIRT_MIN {
        TileType::Dirt
    } else {
        TileType::Grass
    }
}

/// Generate a `width x height` terrain grid using layered simplex noise,
/// then overlay a procedural winding river and scattered rocky outcrops.
///
/// The island-mask path inside `TerrainNoise::sample` is gated by
/// `ISLAND_MASK_STRENGTH = 0` so the playtest map has usable terrain
/// edge-to-edge. Flip it on for a tropical-island scenario.
pub fn generate_terrain(width: u32, height: u32, seed: u32) -> Vec<TileType> {
    let noise = TerrainNoise::new(seed, width, height);
    let mut tiles = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let e = noise.sample(x, y);
            tiles.push(classify_tile(e));
        }
    }
    carve_river(&mut tiles, width, height, seed);
    place_rocky_outcrops(&mut tiles, width, height, seed);
    tiles
}

/// How many rocky outcrops to place per 256×256 map area. Scales with map
/// area so the per-tile density stays consistent at any map size.
const ROCKY_OUTCROP_DENSITY_PER_AREA: f64 = 6.0 / (256.0 * 256.0);
const ROCKY_OUTCROP_RADIUS_MIN: i32 = 2;
const ROCKY_OUTCROP_RADIUS_MAX: i32 = 4;

/// Scatters small rocky outcrops across the terrain. Each outcrop is a Rock
/// tile at the center surrounded by Gravel within a small radius. Skips water
/// tiles so outcrops form on dry land. Deterministic per seed.
fn place_rocky_outcrops(tiles: &mut [TileType], width: u32, height: u32, seed: u32) {
    use rand::SeedableRng;
    use rand::seq::IteratorRandom;
    use rand_chacha::ChaCha8Rng;

    let area = (width as f64) * (height as f64);
    let count = (area * ROCKY_OUTCROP_DENSITY_PER_AREA).round() as usize;
    let mut rng = ChaCha8Rng::seed_from_u64(seed.wrapping_add(202) as u64);

    for _ in 0..count {
        // Pick a random non-water tile as the outcrop center.
        let Some(idx) = (0..tiles.len())
            .filter(|&i| !tiles[i].is_water())
            .choose(&mut rng)
        else {
            return;
        };
        let cx = (idx as u32) % width;
        let cy = (idx as u32) / width;

        let radius = rand::Rng::random_range(
            &mut rng,
            ROCKY_OUTCROP_RADIUS_MIN..=ROCKY_OUTCROP_RADIUS_MAX,
        );

        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let nx = cx as i32 + dx;
                let ny = cy as i32 + dy;
                if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                    continue;
                }
                let dist_sq = dx * dx + dy * dy;
                if dist_sq > radius * radius {
                    continue;
                }
                let i = (ny as u32 * width + nx as u32) as usize;
                if tiles[i].is_water() {
                    continue;
                }
                // Center tile (and immediate neighbours) become Rock; outer
                // ring becomes Gravel. Gives each outcrop a clear "core +
                // skirt" structure.
                tiles[i] = if dist_sq <= 1 {
                    TileType::Rock
                } else {
                    TileType::Gravel
                };
            }
        }
    }
}

/// Water tiles are capped at this elevation (sea level). Rivers and lakes sit at or below it.
pub const SEA_LEVEL: f32 = 64.0;

/// Screen pixels per elevation unit used to fake 3D relief by shifting
/// tile sprites vertically. At 0.15, a peak at elevation 255 sits ~29 px
/// (nearly two tiles) above a sea-level tile.
pub const ELEVATION_LIFT: f32 = 0.15;

/// Generate per-tile elevation values in the 0.0..=255.0 range.
///
/// Uses the same noise seed as terrain generation so elevation naturally
/// correlates with biome type. Water tiles are capped at [`SEA_LEVEL`].
pub fn generate_elevations(tiles: &[TileType], width: u32, height: u32, seed: u32) -> Vec<f32> {
    debug_assert_eq!(
        tiles.len(),
        (width * height) as usize,
        "tiles length must equal width * height"
    );
    let noise = TerrainNoise::new(seed, width, height);
    tiles
        .iter()
        .enumerate()
        .map(|(i, &tile_type)| {
            let x = (i as u32) % width;
            let y = (i as u32) / width;
            let raw_elev = noise.sample(x, y);
            // Map from roughly [-1, 1] to [0, 255].
            let elevation = ((raw_elev + 1.0) / 2.0 * 255.0).clamp(0.0, 255.0) as f32;
            if tile_type.is_water() {
                elevation.min(SEA_LEVEL)
            } else {
                elevation
            }
        })
        .collect()
}

/// Compute a hillshade brightness multiplier for tile `(x, y)`.
///
/// Samples the elevation gradient using the east and north neighbours,
/// then dots it against a fake sun direction from the north-west.
/// Returns a multiplier in `[0.6, 1.2]` — values above 1.0 are sun-facing,
/// values below 1.0 are in shadow.
fn hillshade(elevations: &[f32], width: u32, height: u32, x: u32, y: u32) -> f32 {
    let idx = (y * width + x) as usize;
    let elev = elevations[idx];
    let right = if x + 1 < width {
        elevations[(y * width + x + 1) as usize]
    } else {
        elev
    };
    let up = if y + 1 < height {
        elevations[((y + 1) * width + x) as usize]
    } else {
        elev
    };
    let gradient_x = right - elev;
    let gradient_y = up - elev;
    // Fake sun from the north-west, normalized diagonal.
    const LIGHT_X: f32 = -0.707;
    const LIGHT_Y: f32 = -0.707;
    let dot = gradient_x * LIGHT_X + gradient_y * LIGHT_Y;
    (1.0 - dot * 0.015).clamp(0.75, 1.15)
}

/// Mix a biome base color with elevation: high = lighter, low = darker.
fn tile_base_color(tile_type: TileType, elevation: f32) -> Color {
    let srgba = tile_type.color().to_srgba();
    // -0.15 (valley) to +0.15 (peak), centered at mid-elevation (128).
    let factor = ((elevation - 128.0) / 128.0).clamp(-1.0, 1.0) * 0.15;
    if factor >= 0.0 {
        // Lighten toward white.
        Color::srgba(
            (srgba.red + (1.0 - srgba.red) * factor).clamp(0.0, 1.0),
            (srgba.green + (1.0 - srgba.green) * factor).clamp(0.0, 1.0),
            (srgba.blue + (1.0 - srgba.blue) * factor).clamp(0.0, 1.0),
            srgba.alpha,
        )
    } else {
        // Darken toward black.
        let darken = 1.0 + factor; // 0.7..1.0
        Color::srgba(
            (srgba.red * darken).clamp(0.0, 1.0),
            (srgba.green * darken).clamp(0.0, 1.0),
            (srgba.blue * darken).clamp(0.0, 1.0),
            srgba.alpha,
        )
    }
}

/// Darken an sRGB color by multiplying each channel by `factor` (0..1).
fn darken(color: Color, factor: f32) -> Color {
    let srgba = color.to_srgba();
    Color::srgba(
        (srgba.red * factor).clamp(0.0, 1.0),
        (srgba.green * factor).clamp(0.0, 1.0),
        (srgba.blue * factor).clamp(0.0, 1.0),
        srgba.alpha,
    )
}

/// Multiply an sRGB color by a scalar brightness factor.
fn apply_hillshade(color: Color, shade: f32) -> Color {
    let srgba = color.to_srgba();
    Color::srgba(
        (srgba.red * shade).clamp(0.0, 1.0),
        (srgba.green * shade).clamp(0.0, 1.0),
        (srgba.blue * shade).clamp(0.0, 1.0),
        srgba.alpha,
    )
}

/// Returns the center x-tile of the river at row `y` for the given terrain seed.
///
/// Uses the same multi-octave noise as [`carve_river`] so spawn placement and
/// carving always agree on which side of the river a tile is on.
pub fn river_center_x(y: u32, width: u32, seed: u32) -> u32 {
    let meander = Simplex::new(seed.wrapping_add(97));
    let ty = y as f64;
    // Two octaves: long slow meanders plus gentle wiggles. No high-frequency
    // octave — keeps the river smooth instead of jagged.
    let offset = meander.get([ty * 0.028, 0.0]) * 13.0 + meander.get([ty * 0.075, 100.0]) * 3.0;
    let base = (width / 2) as i32;
    base.saturating_add(offset as i32)
        .clamp(12, width as i32 - 12) as u32
}

/// Carves an organic winding river through the center of the tile grid.
///
/// The river's center line follows multi-octave simplex noise (not a sine
/// wave) for natural meanders. Width and bank thickness vary along its length.
/// Natural shallow "ford" sections emerge from a shallow-bias noise field,
/// with extra bias near y = height/4 and y = 3*height/4 so there are always
/// crossings. Tiles immediately outside the banks become sand shores.
fn carve_river(tiles: &mut [TileType], width: u32, height: u32, seed: u32) {
    let width_noise = Simplex::new(seed.wrapping_add(98));
    let shoal_noise = Simplex::new(seed.wrapping_add(99));
    let bank_l_noise = Simplex::new(seed.wrapping_add(100));
    let bank_r_noise = Simplex::new(seed.wrapping_add(101));

    let ford_centers = [height / 4, height * 3 / 4];

    for y in 0..height {
        let ty = y as f64;
        let cx = river_center_x(y, width, seed) as i32;

        // Variable core half-width: 1..3 (core = 3..7 tiles wide).
        let core_half =
            ((1.8 + width_noise.get([ty * 0.035, 0.0]) * 0.9).round() as i32).clamp(1, 3);

        // Asymmetric banks: 1..2 tiles per side.
        let bank_l = ((1.5 + bank_l_noise.get([ty * 0.06, 0.0]) * 0.6).round() as i32).clamp(1, 2);
        let bank_r = ((1.5 + bank_r_noise.get([ty * 0.06, 50.0]) * 0.6).round() as i32).clamp(1, 2);

        // Triangular bump kernel around each target ford row. At the center,
        // proximity = 1.0; it fades linearly to 0 over 4 rows. Combined with
        // per-tile 2D noise below, this gives ford zones a soft irregular
        // edge instead of a hard rectangular cut.
        let ford_proximity = ford_centers
            .iter()
            .map(|&fy| {
                let d = (y as i32 - fy as i32).abs() as f64;
                (1.0 - d / 4.0).max(0.0)
            })
            .fold(0.0_f64, f64::max);

        let left_edge = cx - core_half - bank_l;
        let right_edge = cx + core_half + bank_r;

        // Carve water core + shallow banks.
        for x in left_edge..=right_edge {
            if x < 0 || x >= width as i32 {
                continue;
            }
            let idx = (y * width + x as u32) as usize;
            let dx = x - cx;
            let is_core = dx.abs() <= core_half;

            if !is_core {
                // Bank: always shallow.
                tiles[idx] = TileType::ShallowWater;
                continue;
            }

            // Core tile: shallow if per-tile 2D noise + ford-proximity boost
            // crosses the threshold. Per-tile noise means shoals are small
            // scattered patches (not row-wide slabs), and the ford proximity
            // term guarantees the center rows of each ford zone are shallow
            // while letting the edges fade irregularly into deep water.
            let shallow_score =
                shoal_noise.get([x as f64 * 0.35, ty * 0.35]) + ford_proximity * 1.6;
            tiles[idx] = if shallow_score > 0.75 {
                TileType::ShallowWater
            } else {
                TileType::Water
            };
        }

        // Sand shores: one tile of beach just outside each bank, but only if
        // the noise-generated terrain there was land (don't overwrite water).
        for &sx in &[left_edge - 1, right_edge + 1] {
            if sx < 0 || sx >= width as i32 {
                continue;
            }
            let idx = (y * width + sx as u32) as usize;
            if !matches!(tiles[idx], TileType::Water | TileType::ShallowWater) {
                tiles[idx] = TileType::Sand;
            }
        }
    }
}

pub fn setup_map(
    mut commands: Commands,
    mut map_resource: ResMut<WorldMap>,
    sim_config: Option<Res<SimConfig>>,
) {
    let width = map_resource.width;
    let height = map_resource.height;

    // The main menu inserts SimConfig before transitioning into the sim. Falling
    // back to the default seed lets headless test apps that skip the menu still
    // generate a usable terrain.
    let seed = sim_config.map(|c| c.seed).unwrap_or(DEFAULT_TERRAIN_SEED);

    // Initialize chunks.
    for cy in 0..MAP_CHUNKS_Y {
        for cx in 0..MAP_CHUNKS_X {
            let chunk = Chunk::new(cx as i32, cy as i32);
            map_resource
                .chunks
                .insert(IVec2::new(cx as i32, cy as i32), chunk);
        }
    }

    // Generate terrain and elevation from noise, then store both in the map resource.
    let terrain = generate_terrain(width, height, seed);
    let elevations = generate_elevations(&terrain, width, height, seed);
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            map_resource.set_tile(x, y, terrain[idx]);
            map_resource.set_elevation(x, y, elevations[idx]);
        }
    }

    // Spawn tiles as children of a parent TileMap entity.
    // Each tile's color combines a biome tint from elevation and a hillshade shadow.
    commands
        .spawn((
            Name::new("TileMap"),
            TileMap,
            Transform::default(),
            Visibility::default(),
            DespawnOnExit(AppState::InSim),
        ))
        .with_children(|parent| {
            for cy in 0..MAP_CHUNKS_Y {
                for cx in 0..MAP_CHUNKS_X {
                    for ly in 0..CHUNK_SIZE {
                        for lx in 0..CHUNK_SIZE {
                            let x = cx * CHUNK_SIZE + lx;
                            let y = cy * CHUNK_SIZE + ly;
                            let idx = (y * width + x) as usize;
                            let tile_type = terrain[idx];
                            let elevation = elevations[idx];
                            let shade = hillshade(&elevations, width, height, x, y);
                            let color =
                                apply_hillshade(tile_base_color(tile_type, elevation), shade);

                            // Fake 3D: shift tile sprites up on the screen
                            // proportional to elevation above sea level. Lower
                            // grid rows render in front so hills can occlude
                            // what's behind them.
                            let lift = (elevation - SEA_LEVEL) * ELEVATION_LIFT;
                            let screen_y = y as f32 * TILE_SIZE + lift;
                            let z = -(y as f32) * 0.01;

                            // Darker "side face" of the cube, drawn directly
                            // below the lifted top. Makes the tile read as a
                            // block sticking up from the ground.
                            if lift > 0.0 {
                                let side_color = darken(color, 0.5);
                                parent.spawn((
                                    Name::new(format!("TileSide ({},{})", x, y)),
                                    Sprite {
                                        color: side_color,
                                        custom_size: Some(Vec2::new(TILE_SIZE, lift)),
                                        ..default()
                                    },
                                    BaseColor(side_color),
                                    Transform::from_translation(Vec3::new(
                                        x as f32 * TILE_SIZE,
                                        screen_y - TILE_SIZE * 0.5 - lift * 0.5,
                                        z - 0.005,
                                    )),
                                ));
                            }

                            parent.spawn((
                                Name::new(format!("Tile ({},{}) {:?}", x, y, tile_type)),
                                Tile {
                                    x: x as i32,
                                    y: y as i32,
                                    tile_type,
                                    elevation,
                                },
                                Sprite {
                                    color,
                                    custom_size: Some(Vec2::new(TILE_SIZE, TILE_SIZE)),
                                    ..default()
                                },
                                BaseColor(color),
                                Transform::from_translation(Vec3::new(
                                    x as f32 * TILE_SIZE,
                                    screen_y,
                                    z,
                                )),
                            ));
                        }
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // ─── Elevation data model (#188) ────────────────────────────────────────

    #[test]
    fn generate_elevations_covers_every_tile() {
        let terrain = generate_terrain(32, 32, 42);
        let elevations = generate_elevations(&terrain, 32, 32, 42);
        assert_eq!(elevations.len(), terrain.len());
    }

    #[test]
    fn elevation_values_clamped_to_0_255() {
        let terrain = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
        let elevations =
            generate_elevations(&terrain, WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
        for &e in &elevations {
            assert!((0.0..=255.0).contains(&e), "elevation {e} out of range");
        }
    }

    #[test]
    fn generated_elevation_has_meaningful_variation() {
        let terrain = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
        let elevations =
            generate_elevations(&terrain, WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
        let min = elevations.iter().cloned().fold(f32::INFINITY, f32::min);
        let max = elevations.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            max - min > 80.0,
            "expected at least 80 units of variation, got min={min:.1} max={max:.1}"
        );
    }

    #[test]
    fn elevation_generation_is_deterministic_with_seed() {
        let terrain = generate_terrain(32, 32, 7);
        let a = generate_elevations(&terrain, 32, 32, 7);
        let b = generate_elevations(&terrain, 32, 32, 7);
        assert_eq!(a, b);
    }

    #[test]
    fn water_tiles_elevation_capped_at_sea_level() {
        let terrain = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
        let elevations =
            generate_elevations(&terrain, WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
        for (i, (&tile_type, &elevation)) in terrain.iter().zip(elevations.iter()).enumerate() {
            if tile_type.is_water() {
                assert!(
                    elevation <= SEA_LEVEL,
                    "water tile {i} has elevation {elevation:.1} > SEA_LEVEL {SEA_LEVEL}"
                );
            }
        }
    }

    #[test]
    fn world_map_elevation_at_returns_stored_value() {
        let mut map = WorldMap::new(CHUNK_SIZE, CHUNK_SIZE);
        map.chunks.insert(IVec2::new(0, 0), Chunk::new(0, 0));
        map.set_elevation(3, 3, 128.0);
        assert_eq!(map.elevation_at(3, 3), Some(128.0));
    }

    #[test]
    fn world_map_elevation_at_returns_none_out_of_bounds() {
        let map = WorldMap::new(CHUNK_SIZE, CHUNK_SIZE);
        assert_eq!(map.elevation_at(CHUNK_SIZE + 1, 0), None);
    }

    #[test]
    fn slope_positive_for_uphill() {
        let mut map = WorldMap::new(CHUNK_SIZE, CHUNK_SIZE);
        map.chunks.insert(IVec2::new(0, 0), Chunk::new(0, 0));
        map.set_elevation(0, 0, 50.0);
        map.set_elevation(1, 0, 100.0);
        assert!(map.slope((0, 0), (1, 0)) > 0.0);
        assert!(map.is_uphill((0, 0), (1, 0)));
    }

    #[test]
    fn slope_negative_for_downhill() {
        let mut map = WorldMap::new(CHUNK_SIZE, CHUNK_SIZE);
        map.chunks.insert(IVec2::new(0, 0), Chunk::new(0, 0));
        map.set_elevation(0, 0, 100.0);
        map.set_elevation(1, 0, 50.0);
        assert!(map.slope((0, 0), (1, 0)) < 0.0);
        assert!(!map.is_uphill((0, 0), (1, 0)));
    }

    #[test]
    fn slope_zero_for_flat() {
        let mut map = WorldMap::new(CHUNK_SIZE, CHUNK_SIZE);
        map.chunks.insert(IVec2::new(0, 0), Chunk::new(0, 0));
        map.set_elevation(0, 0, 75.0);
        map.set_elevation(1, 0, 75.0);
        assert_eq!(map.slope((0, 0), (1, 0)), 0.0);
    }

    // ─── Elevation visualization (#193) ─────────────────────────────────────

    #[test]
    fn hillshade_flat_terrain_produces_shade_of_one() {
        let elevations = vec![100.0f32; 9]; // 3×3, all same
        let shade = hillshade(&elevations, 3, 3, 1, 1);
        assert!(
            (shade - 1.0).abs() < 1e-5,
            "flat terrain should have shade=1.0, got {shade}"
        );
    }

    #[test]
    fn hillshade_east_rising_slope_is_brighter_than_west_rising() {
        // East-rising: right neighbour is higher → gradient_x positive → sun-facing.
        let mut east = vec![100.0f32; 9];
        east[5] = 110.0; // (x=2, y=1) is higher than center (x=1, y=1)
        let shade_east = hillshade(&east, 3, 3, 1, 1);

        // West-rising: right neighbour is lower → gradient_x negative → in shadow.
        let mut west = vec![100.0f32; 9];
        west[5] = 90.0; // right is lower, so west side is higher
        let shade_west = hillshade(&west, 3, 3, 1, 1);

        assert!(
            shade_east > shade_west,
            "east-rising shade {shade_east} should exceed west-rising {shade_west}"
        );
    }

    #[test]
    fn hillshade_result_within_expected_range() {
        // Large gradient should still be clamped.
        let mut extreme = vec![0.0f32; 9];
        extreme[5] = 255.0;
        extreme[7] = 255.0;
        let shade = hillshade(&extreme, 3, 3, 1, 1);
        assert!(
            (0.75..=1.15).contains(&shade),
            "shade {shade} out of [0.75, 1.15]"
        );
    }

    #[test]
    fn tile_base_color_high_elevation_lighter_than_low() {
        let low = tile_base_color(TileType::Grass, 0.0).to_srgba();
        let high = tile_base_color(TileType::Grass, 255.0).to_srgba();
        // All channels should be >= the low-elevation version (lighter).
        assert!(high.red >= low.red);
        assert!(high.green >= low.green);
        assert!(high.blue >= low.blue);
    }

    // ─── Existing tests ──────────────────────────────────────────────────────

    #[test]
    fn all_land_types_are_walkable() {
        assert!(TileType::Grass.is_walkable());
        assert!(TileType::Sand.is_walkable());
        assert!(TileType::Dirt.is_walkable());
        assert!(TileType::Gravel.is_walkable());
        assert!(TileType::Rock.is_walkable());
        assert!(TileType::ShallowWater.is_walkable());
    }

    #[test]
    fn deep_water_is_not_walkable() {
        assert!(!TileType::Water.is_walkable());
    }

    #[test]
    fn speed_multipliers_form_expected_ordering() {
        // Grass is fastest, water is impassable, terrain in between slows agents.
        assert_eq!(TileType::Grass.speed_multiplier(), 1.0);
        assert_eq!(TileType::Water.speed_multiplier(), 0.0);
        assert!(TileType::Sand.speed_multiplier() < TileType::Grass.speed_multiplier());
        assert!(TileType::Dirt.speed_multiplier() < TileType::Sand.speed_multiplier());
        assert!(TileType::Gravel.speed_multiplier() < TileType::Dirt.speed_multiplier());
        assert!(TileType::Rock.speed_multiplier() < TileType::Gravel.speed_multiplier());
        assert!(TileType::ShallowWater.speed_multiplier() < TileType::Rock.speed_multiplier());
    }

    #[test]
    fn world_map_blocks_movement_on_water_tile() {
        let mut map = WorldMap::new(CHUNK_SIZE, CHUNK_SIZE);
        map.chunks.insert(IVec2::new(0, 0), Chunk::new(0, 0));
        map.set_tile(5, 5, TileType::Water);

        let water_pos = map.tile_to_world(5, 5);
        let grass_pos = map.tile_to_world(0, 0);

        assert!(!map.is_walkable(water_pos));
        assert!(map.is_walkable(grass_pos));
    }

    #[test]
    fn speed_at_returns_terrain_multiplier() {
        let mut map = WorldMap::new(CHUNK_SIZE, CHUNK_SIZE);
        map.chunks.insert(IVec2::new(0, 0), Chunk::new(0, 0));
        map.set_tile(3, 3, TileType::Rock);
        map.set_tile(4, 4, TileType::Water);

        assert_eq!(map.speed_at(map.tile_to_world(3, 3)), 0.4);
        assert_eq!(map.speed_at(map.tile_to_world(4, 4)), 0.0);
        assert_eq!(map.speed_at(map.tile_to_world(0, 0)), 1.0);
    }

    #[test]
    fn generated_terrain_has_visible_rocky_outcrops() {
        // Rocky terrain (Gravel + Rock) must be visible in the world, not
        // a handful of stray tiles — otherwise stone gathering breaks and
        // the world reads as flat grass. Threshold sized for the deterministic
        // outcrop pass (~6 outcrops × ~9 tiles each on a 208x208 map, minus
        // overlap with water).
        let tiles = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
        let unique: HashSet<TileType> = tiles.iter().copied().collect();
        for t in [
            TileType::Grass,
            TileType::Sand,
            TileType::Dirt,
            TileType::Gravel,
        ] {
            assert!(unique.contains(&t), "expected {t:?} tiles");
        }
        let rocky = tiles
            .iter()
            .filter(|&&t| matches!(t, TileType::Gravel | TileType::Rock))
            .count();
        assert!(
            rocky >= 30,
            "expected visible rocky outcrops (>=30 Gravel+Rock), got {rocky}"
        );
    }

    #[test]
    fn generated_terrain_is_deterministic_for_seed() {
        let a = generate_terrain(32, 32, 42);
        let b = generate_terrain(32, 32, 42);
        assert_eq!(a, b);
    }

    #[test]
    fn generated_terrain_changes_with_seed() {
        let a = generate_terrain(32, 32, 1);
        let b = generate_terrain(32, 32, 2);
        assert_ne!(a, b);
    }

    #[test]
    fn river_runs_through_center_of_map() {
        let tiles = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
        // Every row should have a Water or ShallowWater tile somewhere in the center third.
        let center_range = (WORLD_WIDTH / 3)..(WORLD_WIDTH * 2 / 3);
        let has_water_in_center = (0..WORLD_HEIGHT).all(|y| {
            center_range.clone().any(|x| {
                let t = tiles[(y * WORLD_WIDTH + x) as usize];
                t == TileType::Water || t == TileType::ShallowWater
            })
        });
        assert!(
            has_water_in_center,
            "river should pass through center third of every row"
        );
    }

    #[test]
    fn river_has_passable_ford_near_target_rows() {
        // Near y = height/4 and y = 3*height/4 at least one row must be fully
        // shallow at the river center (a crossing exists).
        let tiles = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
        for &fy in &[WORLD_HEIGHT / 4, WORLD_HEIGHT * 3 / 4] {
            let ford_zone = (fy.saturating_sub(3))..=(fy + 3).min(WORLD_HEIGHT - 1);
            let has_ford = ford_zone.clone().any(|y| {
                let cx = river_center_x(y, WORLD_WIDTH, DEFAULT_TERRAIN_SEED) as usize;
                tiles[y as usize * WORLD_WIDTH as usize + cx] == TileType::ShallowWater
            });
            assert!(has_ford, "expected a ShallowWater crossing near y={fy}");
        }
    }

    #[test]
    fn river_contains_both_deep_water_and_shallows() {
        // Sanity: the river must have deep water somewhere (it's a real barrier)
        // and shallows somewhere (it has crossings).
        let tiles = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
        let (mut deep, mut shallow) = (0, 0);
        for y in 0..WORLD_HEIGHT {
            let cx = river_center_x(y, WORLD_WIDTH, DEFAULT_TERRAIN_SEED) as usize;
            match tiles[y as usize * WORLD_WIDTH as usize + cx] {
                TileType::Water => deep += 1,
                TileType::ShallowWater => shallow += 1,
                _ => {}
            }
        }
        assert!(
            deep > WORLD_HEIGHT as usize / 2,
            "expected mostly deep water, got {deep}"
        );
        assert!(
            shallow >= 2,
            "expected at least 2 shallow rows for crossings, got {shallow}"
        );
    }

    #[test]
    fn river_has_sand_shores() {
        // Tiles immediately outside the river banks should be Sand shores
        // (at least somewhere — not strictly every row, because noise-generated
        // water in the area would block the sand conversion).
        let tiles = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
        let sand_count = tiles.iter().filter(|&&t| t == TileType::Sand).count();
        assert!(
            sand_count >= 20,
            "expected plenty of sand shores along the river, got {sand_count}"
        );
    }

    #[test]
    fn river_center_x_stays_in_bounds() {
        for y in 0..WORLD_HEIGHT {
            let cx = river_center_x(y, WORLD_WIDTH, DEFAULT_TERRAIN_SEED);
            assert!(
                (12..WORLD_WIDTH - 12).contains(&cx),
                "river_center_x({y}) = {cx} out of bounds"
            );
        }
    }

    // ─── Island shape (#659) ────────────────────────────────────────────────

    #[test]
    fn map_size_is_208x208() {
        assert_eq!(WORLD_WIDTH, 208);
        assert_eq!(WORLD_HEIGHT, 208);
    }

    #[test]
    #[ignore = "diagnostic — run with --include-ignored to see tile counts"]
    fn debug_tile_distribution() {
        // Diagnostic: prints tile counts at the default seed so we can see
        // whether the noise actually produces visible gravel/rock patches.
        let tiles = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
        let mut counts = std::collections::HashMap::new();
        for &t in &tiles {
            *counts.entry(format!("{t:?}")).or_insert(0_usize) += 1;
        }
        let total = tiles.len();
        let mut pairs: Vec<_> = counts.into_iter().collect();
        pairs.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
        eprintln!("tile counts at seed {DEFAULT_TERRAIN_SEED} on {WORLD_WIDTH}x{WORLD_HEIGHT}:");
        for (t, c) in &pairs {
            let pct = 100.0 * (*c as f64) / total as f64;
            eprintln!("  {t}: {c} ({pct:.1}%)");
        }
    }

    #[test]
    #[ignore = "island mask disabled — see ISLAND_MASK_STRENGTH"]
    fn island_corners_are_water() {
        let tiles = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
        let w = WORLD_WIDTH;
        let h = WORLD_HEIGHT;
        for &(x, y) in &[(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
            let idx = (y * w + x) as usize;
            assert!(
                tiles[idx].is_water(),
                "corner ({x}, {y}) should be water but was {:?}",
                tiles[idx]
            );
        }
    }

    #[test]
    #[ignore = "island mask disabled — see ISLAND_MASK_STRENGTH"]
    fn island_edges_are_mostly_water() {
        let tiles = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
        let w = WORLD_WIDTH;
        let h = WORLD_HEIGHT;
        let mut water_edge = 0;
        let mut total_edge = 0;
        for x in 0..w {
            for &y in &[0, h - 1] {
                if tiles[(y * w + x) as usize].is_water() {
                    water_edge += 1;
                }
                total_edge += 1;
            }
        }
        for y in 0..h {
            for &x in &[0, w - 1] {
                if tiles[(y * w + x) as usize].is_water() {
                    water_edge += 1;
                }
                total_edge += 1;
            }
        }
        let ratio = water_edge as f64 / total_edge as f64;
        assert!(
            ratio > 0.95,
            "expected >95% of edge tiles to be water, got {:.2}%",
            ratio * 100.0
        );
    }

    #[test]
    #[ignore = "island mask disabled — see ISLAND_MASK_STRENGTH"]
    fn island_interior_has_substantial_land() {
        let tiles = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
        let w = WORLD_WIDTH;
        let h = WORLD_HEIGHT;
        // Sample a 100x100 box around the center.
        let cx = w / 2;
        let cy = h / 2;
        let mut land = 0;
        let mut total = 0;
        for dy in -50i32..=50 {
            for dx in -50i32..=50 {
                let x = (cx as i32 + dx) as u32;
                let y = (cy as i32 + dy) as u32;
                if !tiles[(y * w + x) as usize].is_water() {
                    land += 1;
                }
                total += 1;
            }
        }
        let ratio = land as f64 / total as f64;
        assert!(
            ratio > 0.6,
            "expected >60% land in central 100x100 box, got {:.2}%",
            ratio * 100.0
        );
    }

    #[test]
    #[ignore = "island mask disabled — see ISLAND_MASK_STRENGTH"]
    fn coastline_has_visible_sand_ring() {
        let tiles = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
        let sand_count = tiles
            .iter()
            .filter(|&&t| matches!(t, TileType::Sand))
            .count();
        // A 512x512 island should have hundreds of sand tiles along its coast.
        assert!(
            sand_count > 500,
            "expected hundreds of sand tiles for the coast, got {sand_count}"
        );
    }

    #[test]
    #[ignore = "island mask disabled — see ISLAND_MASK_STRENGTH"]
    fn coastline_is_irregular_not_circular() {
        // Walk inward from each cardinal edge and record the distance to the
        // first land tile. A round island would give equal distances; an
        // irregular coast gives variation.
        let tiles = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
        let w = WORLD_WIDTH;
        let h = WORLD_HEIGHT;
        let mid_x = w / 2;
        let mid_y = h / 2;

        let mut samples = Vec::new();
        // Sample 16 rays around the half-perimeter to measure coast distance.
        for i in 0..16 {
            let frac = i as f32 / 16.0;
            let y = (frac * h as f32) as u32;
            // Walk in from x=0
            for x in 0..mid_x {
                if !tiles[(y * w + x) as usize].is_water() {
                    samples.push(x);
                    break;
                }
            }
            // Walk in from x=w-1
            for x in (mid_x..w).rev() {
                if !tiles[(y * w + x) as usize].is_water() {
                    samples.push(w - 1 - x);
                    break;
                }
            }
            let x = (frac * w as f32) as u32;
            for y in 0..mid_y {
                if !tiles[(y * w + x) as usize].is_water() {
                    samples.push(y);
                    break;
                }
            }
            for y in (mid_y..h).rev() {
                if !tiles[(y * w + x) as usize].is_water() {
                    samples.push(h - 1 - y);
                    break;
                }
            }
        }

        assert!(samples.len() > 30, "got only {} samples", samples.len());
        let mean = samples.iter().sum::<u32>() as f64 / samples.len() as f64;
        let variance = samples
            .iter()
            .map(|&v| {
                let d = v as f64 - mean;
                d * d
            })
            .sum::<f64>()
            / samples.len() as f64;
        let stddev = variance.sqrt();
        // A round island would give stddev ~0; an irregular coast >> 0.
        assert!(
            stddev > 8.0,
            "coastline looks too round (stddev = {stddev:.2} tiles)"
        );
    }
}
