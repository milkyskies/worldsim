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
    HOUSE_STONE_REQUIRED, HOUSE_WOOD_REQUIRED, LEAN_TO_WOOD_REQUIRED, STORAGE_CHEST_WOOD_REQUIRED,
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
    let mut chosen_template: Option<ActionTemplate> = None;
    let mut close_menu = false;

    egui::Area::new(egui::Id::new("adventure_context_menu"))
        .fixed_pos(open.screen_pos)
        .order(egui::Order::Foreground)
        .show(egui_ctx.get_mut(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(140.0);
                ui.label(menu_header(open.target));
                ui.separator();
                if verbs.is_empty() {
                    ui.weak("(no actions available)");
                } else {
                    for (action_type, label) in &verbs {
                        if ui.button(label).clicked() {
                            chosen_template =
                                build_template(*action_type, open.target, &action_registry);
                            close_menu = true;
                        }
                    }
                }
                ui.separator();
                if ui.small_button("Cancel").clicked() {
                    close_menu = true;
                }
            });
        });

    if let Some(template) = chosen_template {
        brain_state.chosen_actions = vec![template];
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

/// Whitelist of verbs offered for a given target. Each entry is the
/// `ActionType` plus its display label.
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
) -> Vec<(ActionType, String)> {
    match target {
        MenuTarget::Self_ => {
            let mut v = Vec::with_capacity(8);
            // One "Eat <food> (qty)" entry per distinct food in
            // inventory. Shows the player exactly what they have to
            // eat instead of a generic "Eat" that silently picks the
            // first edible. The Eat action itself still picks the
            // first food internally, so for now all of these run the
            // same action — but the visibility is the win, and a
            // future "Eat this specific concept" parameter on Eat
            // will hook up cleanly.
            for (concept, count) in collect_food_in_inventory(inventory, ontology) {
                v.push((ActionType::Eat, format!("Eat {:?} ({count})", concept)));
            }
            // Fish and Drink share the AdjacentToWater gate. Offering
            // them only when the gate would pass keeps the menu honest
            // — clicking a verb that the engine would immediately
            // reject is worse than not seeing it.
            if is_adjacent_to_water(player_pos, map) {
                v.push((ActionType::Fish, "Fish".into()));
                v.push((ActionType::Drink, "Drink".into()));
            }
            // Build verbs gate on inventory materials. Only show when
            // the player can actually start the build — the placement
            // step has a precondition that would otherwise reject the
            // click silently.
            let wood = inventory.count(Concept::Wood);
            let stone = inventory.count(Concept::Stone);
            if wood >= LEAN_TO_WOOD_REQUIRED {
                v.push((ActionType::BuildLeanTo, "Build Lean-to".into()));
            }
            if wood >= HOUSE_WOOD_REQUIRED && stone >= HOUSE_STONE_REQUIRED {
                v.push((ActionType::BuildHouse, "Build House".into()));
            }
            if wood >= STORAGE_CHEST_WOOD_REQUIRED {
                v.push((ActionType::BuildStorageChest, "Build Storage Chest".into()));
            }
            v.push((ActionType::Sleep, "Sleep".into()));
            v.push((ActionType::Rest, "Rest".into()));
            v.push((ActionType::Idle, "Wait".into()));
            v
        }
        MenuTarget::Entity { concept, .. } => verbs_for_concept(concept),
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

fn verbs_for_concept(concept: Option<Concept>) -> Vec<(ActionType, String)> {
    let Some(concept) = concept else {
        return Vec::new();
    };
    match concept {
        Concept::AppleTree | Concept::BerryBush | Concept::StoneNode | Concept::WoodLog => {
            vec![(ActionType::Harvest, "Harvest".into())]
        }
        Concept::Person => vec![
            (ActionType::Wave, "Wave".into()),
            (ActionType::InitiateConversation, "Talk to".into()),
            (ActionType::Attack, "Attack".into()),
        ],
        Concept::Wolf | Concept::Deer => vec![(ActionType::Attack, "Attack".into())],
        Concept::StorageChest => vec![
            (ActionType::Take, "Take from".into()),
            (ActionType::Deposit, "Deposit into".into()),
        ],
        Concept::Campfire => vec![(ActionType::WarmUp, "Warm up by".into())],
        Concept::LeanTo | Concept::House => {
            vec![(ActionType::RestInShelter, "Rest inside".into())]
        }
        _ => Vec::new(),
    }
}

/// Assemble the ActionTemplate for the chosen verb. Self-verbs use the
/// registry's no-target template; entity verbs target the clicked entity
/// and snapshot its world position so the existing `target_position`
/// drive feeds the movement pipeline.
fn build_template(
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
        assert!(verbs.iter().all(|(a, _)| *a != ActionType::Eat));
        // The other self-verbs are always offered.
        assert!(verbs.iter().any(|(a, _)| *a == ActionType::Sleep));
        assert!(verbs.iter().any(|(a, _)| *a == ActionType::Rest));
        assert!(verbs.iter().any(|(a, _)| *a == ActionType::Idle));
    }

    #[test]
    fn self_menu_offers_eat_when_carrying_food() {
        let inv = stocked_inventory(Concept::Apple);
        let ontology = empty_ontology();
        let map = dry_map();
        let verbs = applicable_verbs(MenuTarget::Self_, &inv, &ontology, Vec2::ZERO, &map);
        assert!(verbs.iter().any(|(a, _)| *a == ActionType::Eat));
    }

    #[test]
    fn self_menu_omits_fish_and_drink_when_no_water_nearby() {
        let inv = empty_inventory();
        let ontology = empty_ontology();
        let map = dry_map();
        let verbs = applicable_verbs(MenuTarget::Self_, &inv, &ontology, Vec2::ZERO, &map);
        assert!(verbs.iter().all(|(a, _)| *a != ActionType::Fish));
        assert!(verbs.iter().all(|(a, _)| *a != ActionType::Drink));
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
        assert!(verbs.iter().any(|(a, _)| *a == ActionType::Fish));
        assert!(verbs.iter().any(|(a, _)| *a == ActionType::Drink));
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
        assert!(verbs.iter().any(|(a, _)| *a == ActionType::Harvest));
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
        assert!(verbs.iter().any(|(a, _)| *a == ActionType::Wave));
        assert!(
            verbs
                .iter()
                .any(|(a, _)| *a == ActionType::InitiateConversation)
        );
        assert!(verbs.iter().any(|(a, _)| *a == ActionType::Attack));
    }

    #[test]
    fn self_menu_omits_builds_when_no_materials() {
        let inv = empty_inventory();
        let ontology = empty_ontology();
        let map = dry_map();
        let verbs = applicable_verbs(MenuTarget::Self_, &inv, &ontology, Vec2::ZERO, &map);
        for v in &verbs {
            assert!(v.0 != ActionType::BuildLeanTo);
            assert!(v.0 != ActionType::BuildHouse);
            assert!(v.0 != ActionType::BuildStorageChest);
        }
    }

    #[test]
    fn self_menu_offers_lean_to_with_enough_wood() {
        let mut inv = empty_inventory();
        inv.add(Concept::Wood, LEAN_TO_WOOD_REQUIRED);
        let ontology = empty_ontology();
        let map = dry_map();
        let verbs = applicable_verbs(MenuTarget::Self_, &inv, &ontology, Vec2::ZERO, &map);
        assert!(verbs.iter().any(|(a, _)| *a == ActionType::BuildLeanTo));
        // House needs more wood + stone — not yet satisfied.
        assert!(verbs.iter().all(|(a, _)| *a != ActionType::BuildHouse));
    }

    #[test]
    fn self_menu_offers_house_with_enough_wood_and_stone() {
        let mut inv = empty_inventory();
        inv.add(Concept::Wood, HOUSE_WOOD_REQUIRED);
        inv.add(Concept::Stone, HOUSE_STONE_REQUIRED);
        let ontology = empty_ontology();
        let map = dry_map();
        let verbs = applicable_verbs(MenuTarget::Self_, &inv, &ontology, Vec2::ZERO, &map);
        assert!(verbs.iter().any(|(a, _)| *a == ActionType::BuildHouse));
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
