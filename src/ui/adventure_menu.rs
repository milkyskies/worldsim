//! Right-click context menu for adventure mode: surfaces the verbs the player can invoke against a clicked entity (or themselves) and writes the chosen action straight into BrainState.
//!
//! Reads: ButtonInput<MouseButton>, Camera, Transform, EntityType, ItemSlots, ActionRegistry, PlayerControlled, Ontology
//! Writes: AdventureMenuState (popup open/target), BrainState.chosen_actions (on click)
//! Upstream: ui::camera::cursor_to_world (project right-click cursor)
//! Downstream: nervous_system::execution::start_actions (consumes the chosen template)

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiContext, EguiPrimaryContextPass, PrimaryEguiContext, egui};

use crate::agent::actions::action::drink::is_adjacent_to_water;
use crate::agent::actions::{ActionRegistry, ActionType};
use crate::agent::brains::proposal::BrainState;
use crate::agent::brains::thinking::ActionTemplate;
use crate::agent::inventory::EntityType;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::{Concept, Ontology};
use crate::agent::player::PlayerControlled;
use crate::constants::actions::build::{
    CAMPFIRE_WOOD_REQUIRED, HOUSE_STONE_REQUIRED, HOUSE_WOOD_REQUIRED, LEAN_TO_WOOD_REQUIRED,
    STORAGE_CHEST_WOOD_REQUIRED,
};
use crate::ui::UiState;
use crate::ui::camera;
use crate::ui::sprite_animation::VisualOffset;
use crate::world::map::WorldMap;

/// Pick radius around the cursor for entity selection. Mirrors the
/// existing `handle_game_click` value so right-click and left-click
/// pick the same thing under the same cursor.
const PICK_RADIUS: f32 = 12.0;

/// World-space distance under which an empty right-click is treated as
/// "right-click on yourself" — gives the player an easy way to bring up
/// self-verbs without having to land the cursor exactly on their sprite.
const SELF_PICK_RADIUS: f32 = 24.0;

#[derive(Debug, Clone, Copy)]
enum MenuTarget {
    Self_,
    Entity {
        entity: Entity,
        concept: Option<Concept>,
        world_pos: Vec2,
    },
}

#[derive(Debug, Clone)]
struct MenuOpen {
    screen_pos: egui::Pos2,
    target: MenuTarget,
}

#[derive(Resource, Default)]
pub struct AdventureMenuState {
    open: Option<MenuOpen>,
}

pub struct AdventureMenuPlugin;

impl Plugin for AdventureMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AdventureMenuState>()
            .add_systems(
                Update,
                capture_right_click.run_if(crate::menu::sim_interactive),
            )
            .add_systems(
                EguiPrimaryContextPass,
                render_menu.run_if(crate::menu::sim_interactive),
            );
    }
}

/// On right-click in the game viewport, identify what was clicked and
/// open the context menu. Left-click selection (the existing handler)
/// is unchanged — right-click is a separate channel.
fn capture_right_click(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut egui_contexts: Query<&mut EguiContext, With<PrimaryEguiContext>>,
    ui_state: Option<Res<UiState>>,
    player: Query<(Entity, &Transform), With<PlayerControlled>>,
    entities: Query<(
        Entity,
        &Transform,
        Option<&Sprite>,
        Option<&VisualOffset>,
        Option<&EntityType>,
    )>,
    mut menu_state: ResMut<AdventureMenuState>,
) {
    if !buttons.just_pressed(MouseButton::Right) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };
    let Ok(mut egui_ctx) = egui_contexts.single_mut() else {
        return;
    };
    let Some((camera, camera_transform)) = cameras.iter().next() else {
        return;
    };
    let Some(world_position) = camera::cursor_to_world(
        cursor_position,
        camera,
        camera_transform,
        ui_state.as_deref(),
        egui_ctx.get_mut(),
    ) else {
        return;
    };
    let Ok((player_entity, player_transform)) = player.single() else {
        return;
    };

    // Pick the topmost (highest-z) entity the cursor is over, ignoring
    // the player's own sprite — that case is handled by SELF_PICK_RADIUS
    // below so a right-click on yourself opens the self menu instead of
    // a "do something to yourself" entity menu.
    let mut best: Option<(Entity, f32, Vec2, Concept)> = None;
    for (entity, transform, sprite, vo, entity_type) in entities.iter() {
        if entity == player_entity {
            continue;
        }
        // Only entities tagged with `EntityType` are real interactable
        // world objects (Person, AppleTree, Wolf, …). The skipped ones
        // are mostly silhouette child sprites (head, torso, eyes) that
        // share the player's transform — without this filter, right-
        // clicking on yourself picks one of your own body parts and the
        // menu opens on a concept-less "Target" instead of falling
        // through to the self-menu via SELF_PICK_RADIUS.
        let Some(et) = entity_type else {
            continue;
        };
        let visual_pos = VisualOffset::apply(vo, transform.translation.truncate());
        let dist = visual_pos.distance(world_position);
        let entity_radius = sprite
            .and_then(|s| s.custom_size)
            .map(|size| size.x.max(size.y) / 2.0)
            .unwrap_or(8.0);
        if dist >= entity_radius + PICK_RADIUS {
            continue;
        }
        let z = transform.translation.z;
        match &best {
            Some((_, best_z, _, _)) if *best_z >= z => {}
            _ => best = Some((entity, z, visual_pos, et.0)),
        }
    }

    let target = if let Some((entity, _z, world_pos, concept)) = best {
        MenuTarget::Entity {
            entity,
            concept: Some(concept),
            world_pos,
        }
    } else if player_transform
        .translation
        .truncate()
        .distance(world_position)
        < SELF_PICK_RADIUS
    {
        MenuTarget::Self_
    } else {
        // Right-click on empty space far from anything: dismiss any
        // existing menu rather than open a new one.
        menu_state.open = None;
        return;
    };

    menu_state.open = Some(MenuOpen {
        screen_pos: egui::pos2(cursor_position.x, cursor_position.y),
        target,
    });
}

/// Drop the current menu (if any) and write the selected action
/// template into the player's BrainState.
fn render_menu(
    mut contexts: Query<&mut EguiContext, With<PrimaryEguiContext>>,
    mut menu_state: ResMut<AdventureMenuState>,
    action_registry: Res<ActionRegistry>,
    ontology: Res<Ontology>,
    map: Res<WorldMap>,
    mut player: Query<(&Transform, &ItemSlots, &mut BrainState), With<PlayerControlled>>,
) {
    let Some(open) = menu_state.open.clone() else {
        return;
    };
    let Ok(mut egui_ctx) = contexts.single_mut() else {
        return;
    };
    let Ok((player_transform, inventory, mut brain_state)) = player.single_mut() else {
        // Player despawned or marker dropped — close the menu.
        menu_state.open = None;
        return;
    };

    let player_pos = player_transform.translation.truncate();
    let verbs = applicable_verbs(open.target, inventory, &ontology, player_pos, &map);
    let mut chosen_templates: Vec<ActionTemplate> = Vec::new();
    let mut close_menu = false;

    let area_response = egui::Area::new(egui::Id::new("adventure_context_menu"))
        .fixed_pos(open.screen_pos)
        .order(egui::Order::Foreground)
        .show(egui_ctx.get_mut(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                let row_width = 200.0;
                let row_size = egui::vec2(row_width, 24.0);
                ui.set_min_width(row_width);
                ui.label(menu_header(open.target));
                ui.separator();
                if verbs.is_empty() {
                    ui.weak("(no actions available)");
                } else {
                    for verb in &verbs {
                        let button = egui::Button::new(&verb.label).min_size(row_size);
                        let response = ui.add_enabled(verb.enabled, button);
                        let response = if let Some(tip) = &verb.tooltip {
                            // Tooltip shows on hover for both enabled and
                            // disabled rows — the latter is what teaches
                            // the player *why* an action is greyed.
                            response.on_hover_text(tip).on_disabled_hover_text(tip)
                        } else {
                            response
                        };
                        if response.clicked() && verb.enabled {
                            chosen_templates = build_templates(
                                verb.action_type,
                                open.target,
                                player_pos,
                                &map,
                                &action_registry,
                            );
                            close_menu = true;
                        }
                    }
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("Cancel").min_size(row_size))
                    .clicked()
                {
                    close_menu = true;
                }
            });
        });

    // Click-outside-to-dismiss: any primary or secondary click whose
    // position falls outside the popup's rect closes the menu. Lets
    // the player dismiss by clicking the world (or another spot) the
    // way every other context menu in every other app does.
    let menu_rect = area_response.response.rect;
    let ctx = egui_ctx.get_mut();
    let click_outside = ctx.input(|i| {
        if !(i.pointer.primary_clicked() || i.pointer.secondary_clicked()) {
            return false;
        }
        match i.pointer.interact_pos() {
            Some(pos) => !menu_rect.contains(pos),
            None => false,
        }
    });
    if click_outside {
        close_menu = true;
    }

    if !chosen_templates.is_empty() {
        brain_state.chosen_actions = chosen_templates;
    }
    if close_menu {
        menu_state.open = None;
    }
}

/// Header text shown at the top of the popup.
fn menu_header(target: MenuTarget) -> String {
    match target {
        MenuTarget::Self_ => "Yourself".to_string(),
        MenuTarget::Entity { concept, .. } => match concept {
            Some(c) => format!("{:?}", c),
            None => "Target".to_string(),
        },
    }
}

/// One row in the context menu. `enabled = false` rows render greyed
/// out and surface their `tooltip` on hover so the player can learn
/// *why* they can't take an action (e.g. "Need 5 wood (have 2)").
#[derive(Debug, Clone)]
struct VerbEntry {
    action_type: ActionType,
    label: String,
    enabled: bool,
    tooltip: Option<String>,
}

impl VerbEntry {
    fn enabled(action_type: ActionType, label: impl Into<String>) -> Self {
        Self {
            action_type,
            label: label.into(),
            enabled: true,
            tooltip: None,
        }
    }
    fn disabled(
        action_type: ActionType,
        label: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            action_type,
            label: label.into(),
            enabled: false,
            tooltip: Some(reason.into()),
        }
    }
}

/// Whitelist of verbs offered for a given target. Each entry is the
/// `ActionType`, the display label, an enabled flag, and an optional
/// tooltip explaining why a disabled entry is disabled.
///
/// Kept small and explicit for the MVP — the full action enumeration
/// from the rational brain has too many planner-specific assumptions
/// (auto-walk, target tile snapshotting, search filters) to drop into
/// the player path verbatim. As the player path matures, replace this
/// table with a query over `ActionRegistry` filtered by the target's
/// concept and feasibility.
fn applicable_verbs(
    target: MenuTarget,
    inventory: &ItemSlots,
    ontology: &Ontology,
    player_pos: Vec2,
    map: &WorldMap,
) -> Vec<VerbEntry> {
    match target {
        MenuTarget::Self_ => {
            let mut v = Vec::with_capacity(10);
            // One "Eat <food> (qty)" entry per distinct food in
            // inventory. Shows the player exactly what they have to
            // eat instead of a generic "Eat" that silently picks the
            // first edible. The Eat action itself still picks the
            // first food internally, so for now all of these run the
            // same action — but the visibility is the win, and a
            // future "Eat this specific concept" parameter on Eat
            // will hook up cleanly.
            for (concept, count) in collect_food_in_inventory(inventory, ontology) {
                v.push(VerbEntry::enabled(
                    ActionType::Eat,
                    format!("Eat {:?} ({count})", concept),
                ));
            }
            // Fish and Drink share the AdjacentToWater gate. Show
            // greyed when not adjacent so the player sees these
            // exist as options at all.
            if is_adjacent_to_water(player_pos, map) {
                v.push(VerbEntry::enabled(ActionType::Fish, "Fish"));
                v.push(VerbEntry::enabled(ActionType::Drink, "Drink"));
            } else {
                v.push(VerbEntry::disabled(
                    ActionType::Fish,
                    "Fish",
                    "Need to be next to water",
                ));
                v.push(VerbEntry::disabled(
                    ActionType::Drink,
                    "Drink",
                    "Need to be next to water",
                ));
            }
            // Build verbs always show with their material requirements.
            // Greyed when missing materials, with a tooltip listing what
            // you have vs. need so the player learns the resource costs.
            let wood = inventory.count(Concept::Wood);
            let stone = inventory.count(Concept::Stone);
            v.push(build_entry(
                ActionType::Build,
                "Build Campfire",
                &[(Concept::Wood, CAMPFIRE_WOOD_REQUIRED, wood)],
            ));
            v.push(build_entry(
                ActionType::BuildLeanTo,
                "Build Lean-to",
                &[(Concept::Wood, LEAN_TO_WOOD_REQUIRED, wood)],
            ));
            v.push(build_entry(
                ActionType::BuildHouse,
                "Build House",
                &[
                    (Concept::Wood, HOUSE_WOOD_REQUIRED, wood),
                    (Concept::Stone, HOUSE_STONE_REQUIRED, stone),
                ],
            ));
            v.push(build_entry(
                ActionType::BuildStorageChest,
                "Build Storage Chest",
                &[(Concept::Wood, STORAGE_CHEST_WOOD_REQUIRED, wood)],
            ));
            v.push(VerbEntry::enabled(ActionType::Sleep, "Sleep"));
            v.push(VerbEntry::enabled(ActionType::Rest, "Rest"));
            v.push(VerbEntry::enabled(ActionType::Idle, "Wait"));
            v
        }
        MenuTarget::Entity { concept, .. } => verbs_for_concept(concept),
    }
}

/// Helper: produce a build menu entry given its (concept, required, have)
/// material list. Enabled when every requirement is met, otherwise
/// greyed with a tooltip summarising the shortfall.
fn build_entry(
    action_type: ActionType,
    label: &str,
    requirements: &[(Concept, u32, u32)],
) -> VerbEntry {
    let missing: Vec<String> = requirements
        .iter()
        .filter(|(_, need, have)| have < need)
        .map(|(c, need, have)| format!("{:?}: {have}/{need}", c))
        .collect();
    if missing.is_empty() {
        VerbEntry::enabled(action_type, label)
    } else {
        let cost = requirements
            .iter()
            .map(|(c, need, _)| format!("{} {:?}", need, c))
            .collect::<Vec<_>>()
            .join(" + ");
        let reason = format!("Needs {}.\nMissing: {}", cost, missing.join(", "));
        VerbEntry::disabled(action_type, label, reason)
    }
}

/// Collect every distinct food concept in the inventory along with its
/// total count across slots. Uses both the ontology's `Edible` trait
/// (covers anything tagged via `Food HasTrait Edible`) AND a direct
/// `IsA Food` check (covers cases where the trait cache may be missing
/// the inheritance, which is what bit us in the wild — a player with
/// fresh berries saw no Eat entry because `has_edible` returned false).
/// Either signal is sufficient.
fn collect_food_in_inventory(inventory: &ItemSlots, ontology: &Ontology) -> Vec<(Concept, u32)> {
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<&'static str, (Concept, u32)> = BTreeMap::new();
    for thing in inventory.all_items() {
        let is_food = ontology.has_trait(thing.concept, Concept::Edible)
            || ontology.is_a(thing.concept, Concept::Food);
        if !is_food {
            continue;
        }
        let key = concept_key(thing.concept);
        let entry = counts.entry(key).or_insert((thing.concept, 0));
        entry.1 += 1;
    }
    counts.into_values().collect()
}

/// Stable key for grouping in `collect_food_in_inventory`. `Concept`
/// is `Copy + Hash + Eq` but `BTreeMap` needs `Ord`; we don't want to
/// derive Ord on a public enum just for this, so the Debug name is the
/// stable proxy.
fn concept_key(concept: Concept) -> &'static str {
    // Leak is fine — `Concept` is a small finite enum, the leaked
    // strings amount to one entry per variant for the lifetime of the
    // process.
    Box::leak(format!("{:?}", concept).into_boxed_str())
}

fn verbs_for_concept(concept: Option<Concept>) -> Vec<VerbEntry> {
    let Some(concept) = concept else {
        return Vec::new();
    };
    match concept {
        Concept::AppleTree | Concept::BerryBush | Concept::StoneNode | Concept::WoodLog => {
            vec![VerbEntry::enabled(ActionType::Harvest, "Harvest")]
        }
        Concept::Person => vec![
            VerbEntry::enabled(ActionType::Wave, "Wave"),
            VerbEntry::enabled(ActionType::InitiateConversation, "Talk to"),
            VerbEntry::enabled(ActionType::Attack, "Attack"),
        ],
        Concept::Wolf | Concept::Deer => vec![VerbEntry::enabled(ActionType::Attack, "Attack")],
        Concept::StorageChest => vec![
            VerbEntry::enabled(ActionType::Take, "Take from"),
            VerbEntry::enabled(ActionType::Deposit, "Deposit into"),
        ],
        Concept::Campfire => vec![VerbEntry::enabled(ActionType::WarmUp, "Warm up by")],
        Concept::LeanTo | Concept::House => {
            vec![VerbEntry::enabled(ActionType::RestInShelter, "Rest inside")]
        }
        // BuildLeanTo / BuildHouse / BuildStorageChest place a
        // ConstructionSite at the agent's tile but only handle the
        // initial setup; finishing the structure requires Construct
        // labor on the site. Without this verb the site sits there
        // forever — exactly what the user observed.
        Concept::ConstructionSite => {
            vec![VerbEntry::enabled(ActionType::Construct, "Construct")]
        }
        _ => Vec::new(),
    }
}

/// Assemble the ActionTemplate(s) the menu pushes into BrainState. Returns
/// a vec because entity-targeted actions get an auto-prepended Walk so
/// the agent walks to the target before the action engages — without
/// this, clicking Harvest on a far-away tree would just stand still
/// (the proximity precondition would fail every tick).
///
/// The Walk runs first; the targeted action stays in `chosen_actions`
/// with its proximity precondition, gets rejected each tick by
/// `start_actions`'s feasibility pass, and finally admits when the
/// agent's tile matches the target's tile.
fn build_templates(
    action_type: ActionType,
    target: MenuTarget,
    player_pos: Vec2,
    map: &WorldMap,
    registry: &ActionRegistry,
) -> Vec<ActionTemplate> {
    let Some(template) = build_single_template(action_type, target, registry) else {
        return Vec::new();
    };
    // Self-verbs need no auto-walk.
    let MenuTarget::Entity { world_pos, .. } = target else {
        return vec![template];
    };
    // Already adjacent → no Walk needed. ARRIVAL_THRESHOLD-style check
    // against tile distance: same tile = effectively adjacent for
    // Harvest/Attack/Talk.
    let player_tile = map.world_to_tile(player_pos);
    let target_tile = map.world_to_tile(world_pos);
    if player_tile == target_tile {
        return vec![template];
    }
    let (tx, ty) = target_tile;
    let walk = build_walk_to(world_pos, (tx as i32, ty as i32));
    vec![walk, template]
}

/// Build the template for a single action, no auto-walk.
fn build_single_template(
    action_type: ActionType,
    target: MenuTarget,
    registry: &ActionRegistry,
) -> Option<ActionTemplate> {
    let action = registry.get(action_type)?;
    let mut t = match target {
        MenuTarget::Self_ => action.to_template(None),
        MenuTarget::Entity { entity, .. } => action.to_template(Some(entity)),
    };
    if let MenuTarget::Entity { world_pos, .. } = target {
        t.target_position = Some(world_pos);
    }
    Some(t)
}

/// Mirrors `agent::player::build_walk_template` but with a default
/// intensity — the menu's auto-walk doesn't carry sprint state.
fn build_walk_to(world_pos: Vec2, tile: (i32, i32)) -> ActionTemplate {
    use crate::agent::actions::motor::{
        ActionPrimitive, Behavior, IntensityPolicy, Intent, TargetSelector,
    };
    use crate::agent::mind::knowledge::{Node as MindNode, Predicate, Triple, Value};
    let behavior = Behavior::new(
        ActionPrimitive::Locomote,
        TargetSelector::InPlace,
        IntensityPolicy::Normal,
        Intent::Goal,
    );
    ActionTemplate {
        name: ActionType::Walk.name().to_string(),
        action_type: ActionType::Walk,
        behavior,
        target_entity: None,
        target_position: Some(world_pos),
        preconditions: Vec::new(),
        effects: vec![Triple::new(
            MindNode::Self_,
            Predicate::LocatedAt,
            Value::Tile(tile),
        )],
        consumes: Vec::new(),
        base_cost: 0.0,
        locomotion_intensity: 0.5,
        estimated_duration_ticks: None,
        search_filter: None,
    }
}

#[cfg(test)]
mod tests {
    //! `applicable_verbs` is the only logic worth pinning here — the
    //! rest of the file is rendering glue. Verb derivation drives what
    //! the player can do, and a regression here would silently shrink
    //! the playable verb set.

    use super::*;
    use crate::agent::item_slots::ItemSlots;
    use crate::world::map::{TileType, WorldMap};

    fn empty_ontology() -> Ontology {
        crate::agent::mind::knowledge::setup_ontology()
    }

    fn empty_inventory() -> ItemSlots {
        ItemSlots::agent_carry()
    }

    fn stocked_inventory(food: Concept) -> ItemSlots {
        let mut inv = empty_inventory();
        inv.add(food, 1);
        inv
    }

    /// 5×5 map covered by a single chunk-of-grass. `WorldMap::new` is
    /// sparse — `set_tile` no-ops if the chunk doesn't exist yet — so
    /// we insert a default chunk first. Default tile is Grass, so the
    /// AdjacentToWater gate stays false until a test explicitly sets
    /// a water tile via `set_tile`.
    fn dry_map() -> WorldMap {
        use crate::world::map::Chunk;
        let mut map = WorldMap::new(5, 5);
        map.chunks
            .insert(bevy::prelude::IVec2::new(0, 0), Chunk::new(0, 0));
        map
    }

    #[test]
    fn self_menu_omits_eat_when_inventory_has_no_food() {
        let inv = empty_inventory();
        let ontology = empty_ontology();
        let map = dry_map();
        let verbs = applicable_verbs(MenuTarget::Self_, &inv, &ontology, Vec2::ZERO, &map);
        assert!(verbs.iter().all(|v| v.action_type != ActionType::Eat));
        // The other self-verbs are always offered.
        assert!(verbs.iter().any(|v| v.action_type == ActionType::Sleep));
        assert!(verbs.iter().any(|v| v.action_type == ActionType::Rest));
        assert!(verbs.iter().any(|v| v.action_type == ActionType::Idle));
    }

    #[test]
    fn self_menu_offers_eat_when_carrying_food() {
        let inv = stocked_inventory(Concept::Apple);
        let ontology = empty_ontology();
        let map = dry_map();
        let verbs = applicable_verbs(MenuTarget::Self_, &inv, &ontology, Vec2::ZERO, &map);
        assert!(verbs.iter().any(|v| v.action_type == ActionType::Eat));
    }

    #[test]
    fn self_menu_shows_fish_and_drink_disabled_when_no_water_nearby() {
        let inv = empty_inventory();
        let ontology = empty_ontology();
        let map = dry_map();
        let verbs = applicable_verbs(MenuTarget::Self_, &inv, &ontology, Vec2::ZERO, &map);
        let fish = verbs
            .iter()
            .find(|v| v.action_type == ActionType::Fish)
            .expect("Fish should appear (disabled) so the player can see it exists");
        assert!(!fish.enabled);
        assert!(fish.tooltip.is_some(), "disabled rows should explain why");
        let drink = verbs
            .iter()
            .find(|v| v.action_type == ActionType::Drink)
            .expect("Drink should appear (disabled)");
        assert!(!drink.enabled);
    }

    #[test]
    fn self_menu_offers_fish_and_drink_when_adjacent_to_water() {
        let inv = empty_inventory();
        let ontology = empty_ontology();
        // Place water one tile east of (1,1) so the agent at the center
        // of (1,1) is adjacent to it.
        let mut map = dry_map();
        map.set_tile(2, 1, TileType::Water);
        let pos = map.tile_to_world(1, 1);
        let verbs = applicable_verbs(MenuTarget::Self_, &inv, &ontology, pos, &map);
        assert!(verbs.iter().any(|v| v.action_type == ActionType::Fish));
        assert!(verbs.iter().any(|v| v.action_type == ActionType::Drink));
    }

    #[test]
    fn apple_tree_target_offers_harvest() {
        let inv = empty_inventory();
        let ontology = empty_ontology();
        let map = dry_map();
        let target = MenuTarget::Entity {
            entity: Entity::PLACEHOLDER,
            concept: Some(Concept::AppleTree),
            world_pos: Vec2::ZERO,
        };
        let verbs = applicable_verbs(target, &inv, &ontology, Vec2::ZERO, &map);
        assert!(verbs.iter().any(|v| v.action_type == ActionType::Harvest));
    }

    #[test]
    fn person_target_offers_social_and_combat_verbs() {
        let inv = empty_inventory();
        let ontology = empty_ontology();
        let map = dry_map();
        let target = MenuTarget::Entity {
            entity: Entity::PLACEHOLDER,
            concept: Some(Concept::Person),
            world_pos: Vec2::ZERO,
        };
        let verbs = applicable_verbs(target, &inv, &ontology, Vec2::ZERO, &map);
        assert!(verbs.iter().any(|v| v.action_type == ActionType::Wave));
        assert!(
            verbs
                .iter()
                .any(|v| v.action_type == ActionType::InitiateConversation)
        );
        assert!(verbs.iter().any(|v| v.action_type == ActionType::Attack));
    }

    #[test]
    fn self_menu_shows_builds_disabled_with_explanatory_tooltip_when_no_materials() {
        let inv = empty_inventory();
        let ontology = empty_ontology();
        let map = dry_map();
        let verbs = applicable_verbs(MenuTarget::Self_, &inv, &ontology, Vec2::ZERO, &map);
        let lean_to = verbs
            .iter()
            .find(|v| v.action_type == ActionType::BuildLeanTo)
            .expect("BuildLeanTo should always appear, greyed when materials missing");
        assert!(!lean_to.enabled, "no wood → lean-to disabled");
        let tip = lean_to
            .tooltip
            .as_ref()
            .expect("disabled rows need tooltips");
        assert!(
            tip.contains("Wood") && tip.contains(&format!("0/{LEAN_TO_WOOD_REQUIRED}")),
            "tooltip should name the material and show have/need: {tip}"
        );
    }

    #[test]
    fn self_menu_enables_lean_to_with_enough_wood() {
        let mut inv = empty_inventory();
        inv.add(Concept::Wood, LEAN_TO_WOOD_REQUIRED);
        let ontology = empty_ontology();
        let map = dry_map();
        let verbs = applicable_verbs(MenuTarget::Self_, &inv, &ontology, Vec2::ZERO, &map);
        let lean_to = verbs
            .iter()
            .find(|v| v.action_type == ActionType::BuildLeanTo)
            .expect("BuildLeanTo should appear");
        assert!(lean_to.enabled, "with enough wood → lean-to enabled");
        // House still needs stone.
        let house = verbs
            .iter()
            .find(|v| v.action_type == ActionType::BuildHouse)
            .expect("BuildHouse should still appear");
        assert!(!house.enabled);
    }

    #[test]
    fn self_menu_enables_house_with_enough_wood_and_stone() {
        let mut inv = empty_inventory();
        inv.add(Concept::Wood, HOUSE_WOOD_REQUIRED);
        inv.add(Concept::Stone, HOUSE_STONE_REQUIRED);
        let ontology = empty_ontology();
        let map = dry_map();
        let verbs = applicable_verbs(MenuTarget::Self_, &inv, &ontology, Vec2::ZERO, &map);
        let house = verbs
            .iter()
            .find(|v| v.action_type == ActionType::BuildHouse)
            .expect("BuildHouse should appear");
        assert!(house.enabled);
    }

    #[test]
    fn unknown_concept_offers_no_verbs() {
        let inv = empty_inventory();
        let ontology = empty_ontology();
        let map = dry_map();
        // Sapling is a valid concept but isn't in the player verb table —
        // verifies the fallback branch returns nothing rather than panic.
        let target = MenuTarget::Entity {
            entity: Entity::PLACEHOLDER,
            concept: Some(Concept::Sapling),
            world_pos: Vec2::ZERO,
        };
        let verbs = applicable_verbs(target, &inv, &ontology, Vec2::ZERO, &map);
        assert!(verbs.is_empty());
    }
}
