# Scale

Single-source-of-truth for time, space, movement, and density tuning. Read this before adding any system that has a duration, distance, or rate.

---

## TL;DR for adding a new system

| What you're adding | Use |
|---|---|
| Anything within a single day (hunger, sleep, thirst, fatigue) | Real-time game-seconds, real-life numbers |
| Plant, animal, food, gestation, season, weather | **Biology clock — real days ÷ 6** |
| Aging, life-stage transitions | **Aging clock — real years × 18 = game-days** |
| Anything visible (movement speed, action durations, animations) | **Visual-feel scale** — tune for screen readability, NOT real m/s or seconds |
| Bodies, buildings, walking distances, beds, walls | Real-life dimensions in meters (1 tile = 1m) |
| Vision, hearing, foraging radius, territory | **Compressed ~10-20×** from real life so the playable area stays interesting |
| Population density per biome | **Compressed beyond reality** — see density section below |

---

## Time

### Anchor

- **1 tick = 1 game-second** (`GameTime::TICKS_PER_SECOND = 1` in `src/core/time.rs`)
- **60 wallclock ticks/second** at default speed → 1 real-sec = 1 game-minute
- **Calendar:** 60-day year, 4 seasons of 15 days each
  - 1 game-day ≈ 24 real-min at default speed
  - 1 game-year ≈ 24 real-hours

### Two compressed in-game clocks

Real-life ratios are preserved within each clock; the two clocks run at different speeds because a single creature has biological processes happening at different effective rates.

#### Biology clock — 6× compression

1 real year ≈ 60 game-days. Used for plants, animals, gestation, food spoilage.

| System | Real life | Game |
|---|---|---|
| Leafy greens / berries | 30-60 days | 5-10 days |
| Grain (wheat, rice) | ~120 days | ~20 days |
| Fruit tree maturation | 3-5 years | ~90-180 days |
| Annual fruit cycle | 1 year | 60 days |
| Chicken egg → hatch | 21 days | 3-4 days |
| Pig gestation | ~115 days | ~19 days |
| Cow gestation | ~280 days | ~47 days |
| Human pregnancy | 270 days | ~45 days |
| Raw meat spoilage | 3-7 days | 12-28 game-hours |

Ratios preserved — wheat still matures before a baby is born.

#### Aging clock — 20× compression

1 real year ≈ 18 game-days for aging only. Decoupled from biology so generational play stays feasible.

| Stage | Real life | Game |
|---|---|---|
| Infant | 0-2 yrs | ~36 days |
| Child | 2-13 yrs | ~200 days |
| Teen | 13-18 yrs | ~90 days |
| Adult prime | 18-50 yrs | ~2 game-years |
| Elder | 50-80 yrs | ~1.5 game-years |
| Full lifespan | ~80 yrs | ~4 game-years (~240 days) |

At default speed, a person born in-game lives ~96 real-hours of play.

**Why two clocks:** pregnancy at biology rate (45 days) feels like a season-spanning event. Childhood at the same rate would mean 13 game-years (~13 real days of nonstop play) per person reaching adulthood — unplayable. Aging needs its own faster clock; the cost is a known inconsistency (a baby develops faster in the womb than it grows after birth) accepted in exchange for generational play.

### Visual-feel scale (the third, implicit "clock")

The 60× wallclock compression means literal real-life durations look ridiculous on screen:
- Eating a meal at real pace = 0.3 real-sec (a flicker)
- Walking at 1.4 m/s = 84 visible tiles per real-sec (sprinting)
- A combat exchange at real speed = a single frame

So **everything visible is tuned for screen readability**, not realism:

| Type | Examples | Tune for |
|---|---|---|
| Action durations | Eat (20 ticks), Drink (15), Harvest (30), Build campfire (120), Attack (30) | Visible 0.3-3 real-sec play |
| Movement speed | `BASE_SPEED_PER_TICK = 1.5` | Brisk walk on screen — see Movement section |
| Animation timings | Hop bounces, attack swings | Readable per-action |
| Visual effects | Smoke wisps, fire flicker | Per-real-time frequency |

This is a **calibration philosophy**, not a numeric clock. When adding a visible action, ask "does it read on screen?" not "does this match real-life duration?"

### Logged rates

`--why` and field-logger `:why` output rates in **per rate-unit** (per-game-minute). A `-0.3` aerobic rate means "drains 0.3 points per game-minute" = "5.5 game-hours to empty a 100 pool." When tuning physiological constants, benchmark against real-world time-to-exhaustion and convert to per-game-minute. Do not think in ticks.

When adding biology-clock or aging-clock rates, label the unit in `--why` output (`per biology-day`, `per aging-year`) so tuning is unambiguous.

### Tuning levers

| Constant | Default | Effect of changing |
|---|---|---|
| `DAYS_PER_YEAR` | 60 | Shorter = more arcadey, longer = sluggish |
| `BIOLOGY_COMPRESSION` | 6× | Higher = crops/pregnancy faster, ratios preserved |
| `AGING_COMPRESSION` | 20× | Higher = generations fly by, lower = epic single-life campaigns |

---

## Space

### Anchor

- **1 tile = 1 meter** (RimWorld convention; one human occupies one tile)
- Continuous Minecraft-style world (no per-map size cap)
- `TILE_SIZE` in `src/world/map.rs` is the **render size in pixels per tile** — unrelated to the meters-per-tile semantic

### Two layers, one grid

Time has two clocks. Space has **one grid (1 tile = 1m), two tuning layers**:

| Layer | What | How tuned |
|---|---|---|
| **Physical** | Bodies, beds, walls, buildings, combat reach | Real-life accurate. 1m = 1m. |
| **Functional ranges** | Vision, hearing, smell, foraging radius, territory size, inter-tribe distance | Compressed ~10-20× from real life so the playable area stays interesting |

A second true spatial scale (world map vs local map, Dwarf Fortress style) only becomes necessary if multiple distant regions with their own tribes exist and travel between them is meaningful — that's a Sovereign-mode concern for later, not the playtest.

### Buildings (RimWorld-style tile construction)

Walls are 1 tile thick, doors are 1 tile.

| Structure | Outer | Interior | Fits |
|---|---|---|---|
| Lean-to / windbreak | 2x3 (no walls) | — | 1 sleeper, no door |
| Sleeping hut | 5x5 | 3x3 | 1 bed (1x2) + floor space |
| Family house | 8x10 | 6x8 | 2 beds, hearth, storage |
| Longhouse | 10x16 | 8x14 | 4-6 beds + shared hearth |
| Storage shed | 5x6 | 3x4 | crates / baskets |
| Workshop | 7x8 | 5x6 | workbench + materials |
| Communal hall | 12x14 | 10x12 | hearth, gathering, food prep |

Anything smaller than 5x5 outer for a sleeping building has no usable interior.

### Settlements

| Settlement | Footprint | Population |
|---|---|---|
| Lean-to | 2x3 | 1-2 |
| Single hut | 5x5 | 1 family (3-5) |
| Hamlet (3-5 huts) | 30x30 | 10-25 |
| Village | 80x80 | 50-150 |
| Large village | 150x150 | 200-400 |
| Town | 300x300+ | 500+ |

### Vegetation, biomes, geography

| Thing | Tiles |
|---|---|
| Sapling | 1 |
| Mature tree | 1 (or 2x2 for ancients) |
| Bush, berry patch | 1-2 |
| Wild grain cluster | 5-20 scattered |
| Dense forest tree spacing | every 2-3 tiles |
| Light woodland tree spacing | every 5-8 tiles |
| Grove | 15x15 |
| Small forest | 60x60 |
| Forest (gettable lost) | 200x200 |
| Plains / grassland | 300x300+ |
| Stream / river / big river | 1-2 / 4-8 / 15-30 wide |
| Pond | 8x8 |
| Lake | 50x50 to 200x200 |
| Hill | 20x20 |
| Mountain | 100x100+ |

### Wildlife (territories compressed; group sizes real)

| Group | Territory | Group size |
|---|---|---|
| Wolf pack | 250x250 | 4-8 |
| Bear (solitary) | 150x150 | 1-2 |
| Boar sounder | 80x80 | 5-12 |
| Deer herd | 200x200 | 6-15 |
| Rabbit warren area | 30x30 | 8-20 |

### Tribal range (compression matters most here)

| Activity | Radius from camp |
|---|---|
| Daily forage | 100-200 tiles |
| Hunt trip | 200-400 tiles |
| Long expedition | 500-800 tiles |
| Seasonal migration | 1000+ tiles |

### Inter-tribe distance

| Distance apart | Relationship |
|---|---|
| < 150 tiles | Constant contact |
| 150-500 | Seasonal contact, trade, occasional conflict |
| 500-1500 | Rare expeditions, legendary "other people" |
| 1500+ | Effectively isolated |

### Sensory ranges (tiered, per agent)

Vision, hearing, and smell use **three tiers** — closer = more information. Don't model perception as a single boolean radius.

| Tier | What the agent knows |
|---|---|
| Detect | "Something is there" — silhouette, motion, heat blob |
| Recognize | "It's a wolf" / "it's a person" — kind/species |
| Identify | "It's Bjorn, he's bleeding, holding a spear" — identity + state |

#### Vision (open ground, daylight, clear weather)

| Tier | Range |
|---|---|
| Detect | 60 tiles |
| Recognize | 25 tiles |
| Identify | 10 tiles |

#### Hearing

| Source | Detect | Identify |
|---|---|---|
| Normal voice | 12 | 8 |
| Shout / alarm | 60 | 40 |
| Combat / scream | 100 | 60 |
| Wolf howl, drum | 150 | 80 |

#### Smell

| Source | Range |
|---|---|
| Predator → prey downwind | 25 |
| Smoke from a fire | 100+ (drifts far) |
| Cooking food | 30 |
| Blood / corpse | 20 |

#### Modifiers (multiply base range)

| Condition | Vision | Hearing | Smell |
|---|---|---|---|
| In forest | x0.25 | x0.7 | x1.0 |
| In dense undergrowth | x0.15 | x0.6 | x1.0 |
| Behind a wall | x0 (LOS blocked) | x0.5 | x0.3 |
| Night, no moon | x0.15 | x1.0 | x1.0 |
| Night, near fire | x0.4 within 8 of fire | x1.0 | x1.0 |
| Heavy rain / fog | x0.3 | x0.5 | x0.2 |
| Snowstorm | x0.2 | x0.4 | x0.1 |
| Uphill viewer | x1.5 | x1.0 | x1.0 |
| Crouched target | x0.5 | x0.7 | x1.0 |
| Wind toward viewer | x1.0 | x1.3 | x2.0 |
| Wind away from viewer | x1.0 | x0.7 | x0.3 |

---

## Movement (visually-tuned, NOT realistic m/s)

The **most important place where visual feel beats realism**. Real-life walking at 1.4 m/s under the 60× wallclock compression looks like sprinting on screen. So movement is tuned for screen readability.

| Gait | Real m/s (reference) | Game tiles/tick | In-game m/s | Visual on screen |
|---|---|---|---|---|
| Stroll | 1.0 | 0.06 (1 px/tick) | 0.06 | Slow shuffle |
| **Walk (default)** | 1.4 | **0.094 (1.5 px/tick)** | **0.094** | **RimWorld-equivalent walk — clickable** |
| Brisk walk | 2.0 | 0.13 (2 px/tick) | 0.13 | Purposeful walk |
| Jog | 3.0 | 0.25 (4 px/tick) | 0.25 | Brisk movement |
| Run | 5.0 | 0.5 (8 px/tick) | 0.5 | Run |
| Sprint | 7-8 | 1.0+ (16+ px/tick) | 1.0+ | Sprint |

`BASE_SPEED_PER_TICK = 1.5` (in `src/constants.rs`) sets the human baseline. Species multipliers (Human 1.0, Deer 1.2, Wolf 1.4, Rabbit 1.5) scale from there.

**Why visually-tuned:** the 60× time compression makes any literal real-life speed look ridiculous. RimWorld and DF make the same compromise. Calibrate against "is the movement readable to the player?", not "does it match m/s."

### Anything that scales WITH movement speed

When `BASE_SPEED_PER_TICK` changes, the following must rescale together:

- `STAMINA_PER_TILE_NORMAL` and `STAMINA_PER_TILE_TIRED` — per-tile stamina cost. Inversely proportional to speed (faster speed → lower per-tile cost) so per-real-time fatigue stays constant.

Time-based drains (hunger, thirst, wakefulness, body temperature) do **not** scale with movement — they tick on game-time and are independent.

---

## Density (the secret sauce of fun)

Real-life is *too sparse* for a sim game to feel alive. Real hunter-gatherer density is ~0.0003 humans/ha — on a 4 ha playtest island that gives 0.001 humans. We compress aggressively here because **density is what creates emergent encounters**.

| Setting | Density | Effect |
|---|---|---|
| Real hunter-gatherer | 0.0003 humans/ha | Nothing happens |
| RimWorld | 1.5 pawns/ha | Active feel |
| **Worldsim playtest target** | **5 humans/ha** | RimWorld+ for chaos |
| Dwarf Fortress fort | 5-50 dwarves/ha | Constant chaos |
| Worldbox | 10-100/ha | Mass spectacle |

Object scale stays realistic (huts still 5×5, beds still 1×2 — looks correct). Only the *encounter density* is compressed. That's the magic — looks real, plays dense.

### Per-biome density (eventual full-game)

For Sovereign-mode continent-scale maps, density varies per biome to drive different story types:

| Biome | Humans/ha | Story type |
|---|---|---|
| Plains / farmland | 5-10 | Kingdoms, court intrigue, organized warfare |
| Coast | 3-5 | Fishing villages, trade |
| Forest | 1-3 | Tribal alliances and feuds |
| Mountain | 0.5-1 | Fortified holdfasts, contested passes |
| Swamp | 0.5-1 | Sparse, dangerous, creepy |
| Tundra | 0.1-0.5 | Lone families, harsh survival sagas |
| Desert | 0.1-0.3 | Nomadic, oasis-clustered |

Same per-tile feel everywhere; biome dictates density, density dictates which kinds of social/political stories emerge.

---

## Current playtest config (208×208 RimWorld-scale)

The active values in `src/constants.rs` and `src/world/map.rs`. These are the tuning targets while building out the simulation core. Eventually they'll become per-mode `ScenarioConfig` (Adventure mode, Sovereign mode, test scenarios).

| Constant | Value | Notes |
|---|---|---|
| Map dimensions | 208×208 (4.3 ha) | RimWorld-equivalent intimate scale |
| `BASE_SPEED_PER_TICK` | 1.5 | RimWorld visual walk pace |
| `STAMINA_PER_TILE_NORMAL` / `_TIRED` | 0.054 / 0.108 | Scaled to speed |
| `DRIFT_RANGE_MIN` / `_MAX` | 15 / 40 | Wander segment length |
| Human vision | 240 px (15 tiles) | Sees most of map but not all |
| Deer vision | 200 px (12.5 tiles) | |
| Wolf vision | 280 px (17.5 tiles) | Predator advantage |
| Rabbit vision | 120 px (7.5 tiles) | Near-sighted prey |
| Humans (group A / B) | 12 / 8 | ~5 humans/ha density |
| Apple trees / berry bushes | 22 / 30 | Natural food density |
| Deer / wolves | 12 / 8 | Active food chain |
| Stones / wood logs | 14 / 18 | Resource scatter |
| Wolf min distance from settlement | 30 tiles | Predator separation |
| Deer min distance from settlement | 18 tiles | |
| Island mask | OFF (`ISLAND_MASK_STRENGTH = 0`) | Plain noise terrain + river |

---

## How the playtest scales to the full game

Future Adventure-mode and Sovereign-mode scenarios share the same simulation but use different **per-mode** scenario configs:

| Lever | Playtest | Sovereign-mode (eventual) | Adventure-mode (eventual) |
|---|---|---|---|
| Map dimensions | 208×208 | 4000×4000+ (5 biomes) | Same as Sovereign — shared world |
| Density per biome | uniform 5/ha | varies per biome (table above) | varies per biome |
| Per-tile feel | unchanged | unchanged | unchanged |
| Movement speed | 1.5 px/tick | 1.5 px/tick | 1.5 px/tick |
| Vision range | 15 tiles | 15 tiles | 15 tiles |
| Camera default | mid zoom | far zoom (continent view) | close zoom (player POV) |
| LOD / chunk sim | not needed | required | required |

The simulation core (per-tile rules, agent brains, biology rates, vision) is **shared across all modes**. Only camera, controls, HUD, and total map size change per mode.

---

## Tuning lever quick reference

| Lever | Lives in | Effect |
|---|---|---|
| `MAP_CHUNKS_X/Y` | `src/world/map.rs` | Map size (chunks of 16 tiles each) |
| `BASE_SPEED_PER_TICK` | `src/constants.rs` (movement) | All agent visual speed |
| `STAMINA_PER_TILE_*` | `src/constants.rs` (actions::walk) | Walking fatigue (must scale inversely with speed) |
| `DRIFT_RANGE_*` | `src/constants.rs` (actions::graze) | Wander segment length |
| `*_SPAWN_COUNT` | `src/constants.rs` (world) | Population density |
| `vision_range` | `src/agent/body/species.rs` | Per-species perception |
| `ISLAND_MASK_STRENGTH` | `src/world/map.rs` | 0 = plain terrain, 1 = full island shape |
| `ISLAND_FALLOFF_EXPONENT` | `src/world/map.rs` | Island shape (when mask on) |
| `COAST_NOISE_STRENGTH` | `src/world/map.rs` | Coastline irregularity (when mask on) |
| `MASK_THRESHOLD` (mountain) | `src/world/map.rs` (sample fn) | Mountain rarity |
