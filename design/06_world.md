# Layer 10: The Physical World

The stage on which agents live, move, and die.

---

## 10A. Hyper-Local Geography (Materiality)

### The Tile Grid (1m scale)
The simulation runs on a grid where `1 Tile = 1 Meter`.
Every tile is a complex data structure, not just a texture.

```rust
struct Tile {
    biome: BiomeType,
    elevation: u8,       // 0-255 (1 unit = 10cm height)
    moisture: u8,        // 0-100 (Flammability, Mud)
    temperature: i8,     // -50 to +50 C
    occupant: Option<EntityId>,
    objects: Vec<ObjectId>,
}
```

### Elevation & Slope Mechanics
- **Movement Cost**: `Cost = Base_Cost * (1.0 + Slope_Angle)`.
  - Walking up a steep hill costs 3x Fatigue.
  - Cliffs (>45° slope) are impassable without "Climbing" skill.
- **Water Flow**: Rain flows from High → Low elevation, forming dynamic Rivers.
- **Visibility**: Being high up grants 2x Perception Range. Being low (in a valley) blocks line-of-sight to adjacent valleys.

### Occlusion Logic (Line of Sight)
```rust
fn can_see(observer_pos, target_pos) {
    let ray = cast_ray(observer_pos, target_pos);
    for tile in ray {
        if tile.elevation > observer_pos.elevation { return false; } // Hill blocks view
        if tile.contains_opaque_object() { return false; } // Forest/Wall blocks view
    }
    return true;
}
```

---

## 10B. Deep Resources (Harvesting)

Resources are not infinite. They are nodes with state.

### Flora System (Vegetation)
- **Growth Cycle**: `Seed → Sprout → Sapling → Mature → Old → Dead`.
- **Seasonality**: Fruit trees only produce `Food` in Late Summer / Autumn.
- **Regeneration**: Forests expand slowly (year-by-year) into adjacent Empty tiles if `Moisture > 50`.

### Resource Node Types
| Node Type | Biome | Tool Required | Skill | Yield |
|-----------|-------|---------------|-------|-------|
| **Apple Tree** | Forest/Grass | None (Pick) | Foraging | 5 Apples / Year |
| **Oak Tree** | Forest | Axe | Woodcutting | 10 Logs (Destroys Tree) |
| **Iron Vein** | Mountain | Pickaxe | Mining | 50 Ore (Finite) |
| **River Fish** | Water | Fishing Rod | Fishing | Infinite (Rate limited) |
| **Clay Deposit** | Swamp/Riverbank | Shovel | None | 20 Clay (Regens heavily with Rain) |

### Material Properties
Materials determine *Affordances*.
- **Wood**: Flammable, Floats, Rot-susceptible.
- **Stone**: Heavy (high weight), Fireproof, Durable.
- **Iron**: High durability, Requires Smelting (Forge).

---

## 10C. Environmental Physics

### 1. Water System
- **Rivers**: Not static sprites. Rivers have `FlowDirection` and `Volume`.
- **Flood**: Heavy Rain → `RiverVolume` spikes → Overflows banks → Turns Grass to `Mud` (Movement Cost 2x).
- **Thirst**: Agents must stand adjacent to Fresh Water to `Drink`.

### 2. Temperature System
- **Gradient**: Temperature isn't global.
  - Near Fire: +40°C.
  - In Shadow: -5°C vs Sun.
  - Night: -10°C drop.
- **Insulation**: Clothes (Inventory) reduce the delta between Ambient Temp and Body Temp.

### 3. Light System
- **Global Light**: Sun angle determines base light (0.0 to 1.0).
- **Local Light**: Torch/Campfire emits light (Radius 5 tiles).
- **Vision Cap**: `Max_Vision = Perception * Light_Level`.
  - Pitch black cave = Agents are effectively blind (0 vision).

