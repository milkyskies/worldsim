use bevy::prelude::*;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum TileType {
    Grass,
    Water,
}

impl TileType {
    pub fn is_walkable(&self) -> bool {
        matches!(self, TileType::Grass | TileType::Water)
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

    /// Check if a world position is walkable (in bounds and not water)
    pub fn is_walkable(&self, pos: Vec2) -> bool {
        if !self.in_bounds(pos) {
            return false;
        }
        let (tx, ty) = self.world_to_tile(pos);
        self.get_tile(tx, ty)
            .map(|t| t.is_walkable())
            .unwrap_or(false)
    }

    /// Get pixel bounds of the map
    pub fn pixel_bounds(&self) -> (f32, f32) {
        (
            self.width as f32 * TILE_SIZE,
            self.height as f32 * TILE_SIZE,
        )
    }
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

    // Create a river that meanders across the map
    // We update across chunk boundaries seamlessly using the new `set_tile` helper
    for y in 0..height {
        // Meander the river
        let river_x = 40 + ((y as f32 * 0.15).sin() * 8.0) as u32;
        if river_x < width {
            map_resource.set_tile(river_x, y, TileType::Water);
            // Make river wider in places
            if y % 5 < 3 && river_x + 1 < width {
                map_resource.set_tile(river_x + 1, y, TileType::Water);
            }
            if y % 7 < 4 && river_x + 2 < width {
                map_resource.set_tile(river_x + 2, y, TileType::Water);
            }
        }
    }

    // Add a lake
    for dy in 0..15 {
        for dx in 0..15 {
            let x = 60 + dx;
            let y = 60 + dy;
            if x < width && y < height {
                map_resource.set_tile(x, y, TileType::Water);
            }
        }
    }

    // Spawn tiles as children of a parent TileMap entity
    // We iterate chunks to spawn them roughly in order
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

                    // Spawn tiles for this chunk
                    for ly in 0..CHUNK_SIZE {
                        for lx in 0..CHUNK_SIZE {
                            let x = cx * CHUNK_SIZE + lx;
                            let y = cy * CHUNK_SIZE + ly;
                            let tile_type = chunk.get_tile(lx, ly).unwrap();

                            let color = match tile_type {
                                TileType::Grass => Color::srgb(0.2, 0.8, 0.2), // Green
                                TileType::Water => Color::srgb(0.2, 0.2, 0.8), // Blue
                            };

                            parent.spawn((
                                Name::new(format!("Tile ({},{}) {:?}", x, y, tile_type)),
                                Tile {
                                    x: x as i32,
                                    y: y as i32,
                                    tile_type,
                                },
                                Sprite {
                                    color,
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
