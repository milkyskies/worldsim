//! Right-click context menu for adventure mode: surfaces the verbs the player can invoke against a clicked entity (or themselves) and writes the chosen action straight into BrainState.
//!
//! Reads: ButtonInput<MouseButton>, Camera, Transform, EntityType, ItemSlots, ActionRegistry, PlayerControlled, Ontology
//! Writes: AdventureMenuState (popup open/target), BrainState.chosen_actions (on click)
//! Upstream: ui::camera::cursor_to_world (project right-click cursor)
//! Downstream: nervous_system::execution::start_actions (consumes the chosen template)

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiContext, EguiPrimaryContextPass, PrimaryEguiContext, egui};

use crate::agent::actions::{ActionRegistry, ActionType};
use crate::agent::brains::proposal::BrainState;
use crate::agent::brains::thinking::ActionTemplate;
use crate::agent::inventory::EntityType;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::{Concept, Ontology};
use crate::agent::player::PlayerControlled;
use crate::ui::UiState;
use crate::ui::camera;
use crate::ui::sprite_animation::VisualOffset;

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
    let mut best: Option<(Entity, f32, Vec2, Option<Concept>)> = None;
    for (entity, transform, sprite, vo, entity_type) in entities.iter() {
        if entity == player_entity {
            continue;
        }
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
            _ => best = Some((entity, z, visual_pos, entity_type.map(|et| et.0))),
        }
    }

    let target = if let Some((entity, _z, world_pos, concept)) = best {
        MenuTarget::Entity {
            entity,
            concept,
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
    mut player: Query<(&ItemSlots, &mut BrainState), With<PlayerControlled>>,
) {
    let Some(open) = menu_state.open.clone() else {
        return;
    };
    let Ok(mut egui_ctx) = contexts.single_mut() else {
        return;
    };
    let Ok((inventory, mut brain_state)) = player.single_mut() else {
        // Player despawned or marker dropped — close the menu.
        menu_state.open = None;
        return;
    };

    let verbs = applicable_verbs(open.target, inventory, &ontology);
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
                        if ui.button(*label).clicked() {
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
) -> Vec<(ActionType, &'static str)> {
    match target {
        MenuTarget::Self_ => {
            let mut v = Vec::with_capacity(4);
            if inventory.has_edible(ontology) {
                v.push((ActionType::Eat, "Eat"));
            }
            v.push((ActionType::Sleep, "Sleep"));
            v.push((ActionType::Rest, "Rest"));
            v.push((ActionType::Idle, "Wait"));
            v
        }
        MenuTarget::Entity { concept, .. } => verbs_for_concept(concept),
    }
}

fn verbs_for_concept(concept: Option<Concept>) -> Vec<(ActionType, &'static str)> {
    let Some(concept) = concept else {
        return Vec::new();
    };
    match concept {
        Concept::AppleTree | Concept::BerryBush | Concept::StoneNode | Concept::WoodLog => {
            vec![(ActionType::Harvest, "Harvest")]
        }
        Concept::Person => vec![
            (ActionType::Wave, "Wave"),
            (ActionType::InitiateConversation, "Talk to"),
            (ActionType::Attack, "Attack"),
        ],
        Concept::Wolf | Concept::Deer => vec![(ActionType::Attack, "Attack")],
        Concept::StorageChest => vec![
            (ActionType::Take, "Take from"),
            (ActionType::Deposit, "Deposit into"),
        ],
        Concept::Campfire => vec![(ActionType::WarmUp, "Warm up by")],
        Concept::LeanTo | Concept::House => vec![(ActionType::RestInShelter, "Rest inside")],
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

    #[test]
    fn self_menu_omits_eat_when_inventory_has_no_food() {
        let inv = empty_inventory();
        let ontology = empty_ontology();
        let verbs = applicable_verbs(MenuTarget::Self_, &inv, &ontology);
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
        let verbs = applicable_verbs(MenuTarget::Self_, &inv, &ontology);
        assert!(verbs.iter().any(|(a, _)| *a == ActionType::Eat));
    }

    #[test]
    fn apple_tree_target_offers_harvest() {
        let inv = empty_inventory();
        let ontology = empty_ontology();
        let target = MenuTarget::Entity {
            entity: Entity::PLACEHOLDER,
            concept: Some(Concept::AppleTree),
            world_pos: Vec2::ZERO,
        };
        let verbs = applicable_verbs(target, &inv, &ontology);
        assert!(verbs.iter().any(|(a, _)| *a == ActionType::Harvest));
    }

    #[test]
    fn person_target_offers_social_and_combat_verbs() {
        let inv = empty_inventory();
        let ontology = empty_ontology();
        let target = MenuTarget::Entity {
            entity: Entity::PLACEHOLDER,
            concept: Some(Concept::Person),
            world_pos: Vec2::ZERO,
        };
        let verbs = applicable_verbs(target, &inv, &ontology);
        assert!(verbs.iter().any(|(a, _)| *a == ActionType::Wave));
        assert!(
            verbs
                .iter()
                .any(|(a, _)| *a == ActionType::InitiateConversation)
        );
        assert!(verbs.iter().any(|(a, _)| *a == ActionType::Attack));
    }

    #[test]
    fn unknown_concept_offers_no_verbs() {
        let inv = empty_inventory();
        let ontology = empty_ontology();
        // Sapling is a valid concept but isn't in the player verb table —
        // verifies the fallback branch returns nothing rather than panic.
        let target = MenuTarget::Entity {
            entity: Entity::PLACEHOLDER,
            concept: Some(Concept::Sapling),
            world_pos: Vec2::ZERO,
        };
        let verbs = applicable_verbs(target, &inv, &ontology);
        assert!(verbs.is_empty());
    }
}
