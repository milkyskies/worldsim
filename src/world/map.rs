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
            .add_systems(Startup, setup_map);
    }
}

// World size constants
pub const TILE_SIZE: f32 = 16.0;
pub const CHUNK_SIZE: u32 = 16;
pub const MAP_CHUNKS_X: u32 = 8;
pub const MAP_CHUNKS_Y: u32 = 8;
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum TileType {
    Grass,
    Forest,
    Rock,
    Sand,
    Water,
    ShallowWater,
}

impl TileType {
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
            TileType::Forest => 0.6,
            TileType::Rock => 0.4,
            TileType::ShallowWater => 0.3,
            TileType::Water => 0.0,
        }
    }

    /// Render color for this tile type.
    pub fn color(&self) -> Color {
        match self {
            TileType::Grass => Color::srgb(0.34, 0.72, 0.30),
            TileType::Forest => Color::srgb(0.15, 0.45, 0.18),
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
}

impl Chunk {
    pub fn new(x: i32, y: i32) -> Self {
        Self {
            x,
            y,
            tiles: vec![TileType::Grass; (CHUNK_SIZE * CHUNK_SIZE) as usize],
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
}

/// Sampled noise fields used to classify a tile.
struct TerrainNoise {
    elevation: Simplex,
    moisture: Simplex,
    detail: Simplex,
}

impl TerrainNoise {
    fn new(seed: u32) -> Self {
        Self {
            elevation: Simplex::new(seed),
            moisture: Simplex::new(seed.wrapping_add(1)),
            detail: Simplex::new(seed.wrapping_add(2)),
        }
    }

    /// Returns (elevation, moisture) in roughly the [-1.0, 1.0] range.
    fn sample(&self, x: u32, y: u32) -> (f64, f64) {
        // Base frequency — controls biome size relative to map.
        const BASE: f64 = 0.045;
        let nx = x as f64 * BASE;
        let ny = y as f64 * BASE;

        // Two octaves of elevation plus a high-frequency detail layer.
        let elevation = self.elevation.get([nx, ny]) * 0.65
            + self.elevation.get([nx * 2.1, ny * 2.1]) * 0.25
            + self.detail.get([nx * 4.3, ny * 4.3]) * 0.10;

        // Two octaves of moisture, offset so it doesn't align with elevation.
        let moisture = self.moisture.get([nx + 100.0, ny + 100.0]) * 0.65
            + self.moisture.get([nx * 2.1 + 100.0, ny * 2.1 + 100.0]) * 0.35;

        (elevation, moisture)
    }
}

/// Elevation/moisture thresholds (in noise output space, roughly [-1.0, 1.0])
/// that decide which biome a tile belongs to. Tuned by eye for the default seed.
mod biome {
    pub const DEEP_WATER_MAX: f64 = -0.65;
    pub const SHALLOW_WATER_MAX: f64 = -0.55;
    pub const SAND_MAX: f64 = -0.42;
    pub const ROCK_MIN: f64 = 0.45;
    pub const FOREST_MOISTURE_MIN: f64 = 0.10;
}

/// Classify a tile from elevation/moisture noise values.
///
/// Elevation drives the dominant biome (water -> sand -> land -> rock).
/// Moisture decides whether mid-elevation land is grass or forest.
fn classify_tile(elevation: f64, moisture: f64) -> TileType {
    if elevation < biome::DEEP_WATER_MAX {
        TileType::Water
    } else if elevation < biome::SHALLOW_WATER_MAX {
        TileType::ShallowWater
    } else if elevation < biome::SAND_MAX {
        TileType::Sand
    } else if elevation > biome::ROCK_MIN {
        TileType::Rock
    } else if moisture > biome::FOREST_MOISTURE_MIN {
        TileType::Forest
    } else {
        TileType::Grass
    }
}

/// Generate a `width x height` terrain grid using layered simplex noise.
pub fn generate_terrain(width: u32, height: u32, seed: u32) -> Vec<TileType> {
    let noise = TerrainNoise::new(seed);
    let mut tiles = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let (e, m) = noise.sample(x, y);
            tiles.push(classify_tile(e, m));
        }
    }
    tiles
}

pub fn setup_map(mut commands: Commands, mut map_resource: ResMut<WorldMap>) {
    let width = map_resource.width;
    let height = map_resource.height;

    // Initialize chunks
    for cy in 0..MAP_CHUNKS_Y {
        for cx in 0..MAP_CHUNKS_X {
            let chunk = Chunk::new(cx as i32, cy as i32);
            map_resource
                .chunks
                .insert(IVec2::new(cx as i32, cy as i32), chunk);
        }
    }

    // Generate terrain from noise and write into the chunked map.
    let terrain = generate_terrain(width, height, DEFAULT_TERRAIN_SEED);
    for y in 0..height {
        for x in 0..width {
            let tile = terrain[(y * width + x) as usize];
            map_resource.set_tile(x, y, tile);
        }
    }

    // Spawn tiles as children of a parent TileMap entity.
    commands
        .spawn((
            Name::new("TileMap"),
            TileMap,
            Transform::default(),
            Visibility::default(),
        ))
        .with_children(|parent| {
            for cy in 0..MAP_CHUNKS_Y {
                for cx in 0..MAP_CHUNKS_X {
                    let chunk = map_resource
                        .chunks
                        .get(&IVec2::new(cx as i32, cy as i32))
                        .unwrap();

                    for ly in 0..CHUNK_SIZE {
                        for lx in 0..CHUNK_SIZE {
                            let x = cx * CHUNK_SIZE + lx;
                            let y = cy * CHUNK_SIZE + ly;
                            let tile_type = chunk.get_tile(lx, ly).unwrap();

                            parent.spawn((
                                Name::new(format!("Tile ({},{}) {:?}", x, y, tile_type)),
                                Tile {
                                    x: x as i32,
                                    y: y as i32,
                                    tile_type,
                                },
                                Sprite {
                                    color: tile_type.color(),
                                    custom_size: Some(Vec2::new(TILE_SIZE, TILE_SIZE)),
                                    ..default()
                                },
                                Transform::from_translation(Vec3::new(
                                    x as f32 * TILE_SIZE,
                                    y as f32 * TILE_SIZE,
                                    0.0,
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

    #[test]
    fn grass_forest_sand_rock_shallow_water_are_walkable() {
        assert!(TileType::Grass.is_walkable());
        assert!(TileType::Forest.is_walkable());
        assert!(TileType::Sand.is_walkable());
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
        assert!(TileType::Forest.speed_multiplier() < TileType::Sand.speed_multiplier());
        assert!(TileType::Rock.speed_multiplier() < TileType::Forest.speed_multiplier());
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
        map.set_tile(3, 3, TileType::Forest);
        map.set_tile(4, 4, TileType::Water);

        assert_eq!(map.speed_at(map.tile_to_world(3, 3)), 0.6);
        assert_eq!(map.speed_at(map.tile_to_world(4, 4)), 0.0);
        assert_eq!(map.speed_at(map.tile_to_world(0, 0)), 1.0);
    }

    #[test]
    fn generated_terrain_contains_at_least_four_non_water_types() {
        let tiles = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
        let unique: HashSet<TileType> = tiles
            .iter()
            .copied()
            .filter(|t| !matches!(t, TileType::Water | TileType::ShallowWater))
            .collect();
        assert!(
            unique.len() >= 4,
            "expected at least 4 non-water terrain types, got {:?}",
            unique
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
}
