use crate::agent::mind::knowledge::{MindGraph, Node, Predicate, Value};
use bevy::prelude::*;
use bevy_egui::egui::{self, Color32, RichText};

/// State for the Knowledge Inspector UI
#[derive(Resource, Default)]
pub struct KnowledgeInspectorState {
    pub target_agent: Option<Entity>,
    pub filter_subject: Option<Node>,
    pub filter_predicate: Option<Predicate>,
    pub filter_object: Option<Value>,
    // Simple history stack for navigation (Back button)
    pub history: Vec<FilterState>,
    // Text search filter
    pub search_query: String,
}

#[derive(Clone)]
pub struct FilterState {
    pub subject: Option<Node>,
    pub predicate: Option<Predicate>,
    pub object: Option<Value>,
}

/// System to render the Mind Inspector UI
pub fn render_mind_inspector(
    ui: &mut egui::Ui,
    state: &mut KnowledgeInspectorState,
    world: &mut World,
) {
    // 1. Header & Navigation
    ui.horizontal(|ui| {
        if !state.history.is_empty()
            && ui.button("⬅ Back").clicked()
            && let Some(prev) = state.history.pop()
        {
            state.filter_subject = prev.subject;
            state.filter_predicate = prev.predicate;
            state.filter_object = prev.object;
        }

        if ui.button("🏠 Home").clicked() {
            push_history(state);
            state.filter_subject = None;
            state.filter_predicate = None;
            state.filter_object = None;
        }

        ui.separator();

        // Agent Selector (Debug helper)
        if let Some(target) = state.target_agent {
            if let Some(name) = world.get::<Name>(target) {
                ui.label(format!("Viewing: {}", name));
            } else {
                ui.label(format!("Viewing: {:?}", target));
            }
        } else {
            ui.label("No Agent Selected");
        }
    });

    // Text Search Bar
    ui.horizontal(|ui| {
        ui.label("🔍 Search:");
        ui.add(egui::TextEdit::singleline(&mut state.search_query).hint_text("Filter by text..."));
        if !state.search_query.is_empty() && ui.small_button("✖").clicked() {
            state.search_query.clear();
        }
    });

    ui.separator();

    // 2. Active Filter Display
    if state.filter_subject.is_some()
        || state.filter_predicate.is_some()
        || state.filter_object.is_some()
    {
        ui.horizontal(|ui| {
            ui.label("Filter:");
            if let Some(s) = &state.filter_subject {
                ui.colored_label(Color32::LIGHT_BLUE, format!("{:?}", s));
            }
            if let Some(p) = &state.filter_predicate {
                ui.colored_label(Color32::LIGHT_GRAY, format!("-{:?}->", p));
            }
            if let Some(o) = &state.filter_object {
                ui.colored_label(Color32::LIGHT_GREEN, format!("{:?}", o));
            }

            if ui.small_button("✖ Clear").clicked() {
                push_history(state);
                state.filter_subject = None;
                state.filter_predicate = None;
                state.filter_object = None;
            }
        });
        ui.separator();
    }

    // 3. Knowledge Table
    let target_entity = if let Some(e) = state.target_agent {
        e
    } else {
        return;
    };

    // We need to query the world for the agent's MindGraph
    // Since we are in an exclusive system param (World), we can get it directly
    if let Some(mind) = world.get::<MindGraph>(target_entity) {
        let filtered_triples = mind.query(
            state.filter_subject.as_ref(),
            state.filter_predicate,
            state.filter_object.as_ref(),
        );

        // Apply text search filter
        let search_lower = state.search_query.to_lowercase();
        let filtered_triples: Vec<_> = if search_lower.is_empty() {
            filtered_triples
        } else {
            filtered_triples
                .into_iter()
                .filter(|triple| {
                    let subj_text = format!("{:?}", triple.subject).to_lowercase();
                    let pred_text = format!("{:?}", triple.predicate).to_lowercase();
                    let obj_text = format!("{:?}", triple.object).to_lowercase();
                    subj_text.contains(&search_lower)
                        || pred_text.contains(&search_lower)
                        || obj_text.contains(&search_lower)
                })
                .collect()
        };

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("mind_grid")
                .striped(true)
                .min_col_width(60.0)
                .show(ui, |ui| {
                    ui.strong("Subject");
                    ui.strong("Predicate");
                    ui.strong("Object");
                    ui.strong("Conf");
                    ui.strong("Source");
                    ui.strong("Age");
                    ui.end_row();

                    for triple in filtered_triples {
                        // SUBJECT (Clickable + Context Menu)
                        let subj_text = format!("{:?}", triple.subject);
                        let subj_link =
                            ui.link(RichText::new(subj_text).color(Color32::LIGHT_BLUE));

                        // Left Click: Focus (Exclusive)
                        if subj_link.clicked() {
                            push_history(state);
                            state.filter_subject = Some(triple.subject.clone());
                            state.filter_predicate = None;
                            state.filter_object = None;
                        }

                        // Right Click: Context Menu
                        subj_link.context_menu(|ui| {
                            if ui.button("Filter by Subject (AND)").clicked() {
                                push_history(state);
                                state.filter_subject = Some(triple.subject.clone());
                                ui.close();
                            }
                            // If this node can be a value, offer to filter as object
                            if let Some(val) = node_to_value(&triple.subject)
                                && ui.button("Filter as Object").clicked()
                            {
                                push_history(state);
                                state.filter_object = Some(val);
                                // Usually we'd want to clear subject if switching to object view?
                                // But user asked for "Both", so let's keep subject if set, or just set object.
                                // For "Filter as Object", we likely mean "Show me things targeting this".
                                state.filter_subject = None;
                                ui.close();
                            }
                        });

                        // PREDICATE
                        let pred_text = format!("{:?}", triple.predicate);
                        let pred_label = ui.label(pred_text);
                        pred_label.context_menu(|ui| {
                            if ui.button("Filter by Predicate").clicked() {
                                push_history(state);
                                state.filter_predicate = Some(triple.predicate);
                                ui.close();
                            }
                        });

                        // OBJECT (Clickable + Context Menu)
                        let obj_text = format!("{:?}", triple.object);
                        let is_linkable =
                            matches!(triple.object, Value::Concept(_) | Value::Entity(_));

                        let obj_label = if is_linkable {
                            ui.link(RichText::new(obj_text).color(Color32::LIGHT_GREEN))
                        } else {
                            ui.label(obj_text)
                        };

                        // Left Click: Navigate (Pivot)
                        if is_linkable && obj_label.clicked() {
                            push_history(state);
                            // Navigate: Make Object the new SUBJECT
                            if let Some(node) = value_to_node(&triple.object) {
                                state.filter_subject = Some(node);
                                state.filter_predicate = None;
                                state.filter_object = None;
                            }
                        }

                        // Right Click: Context Menu
                        obj_label.context_menu(|ui| {
                            if ui.button("Filter by Object (AND)").clicked() {
                                push_history(state);
                                state.filter_object = Some(triple.object.clone());
                                ui.close();
                            }
                            if is_linkable && ui.button("Go to Definition (Subject)").clicked() {
                                push_history(state);
                                if let Some(node) = value_to_node(&triple.object) {
                                    state.filter_subject = Some(node);
                                    state.filter_predicate = None;
                                    state.filter_object = None;
                                }
                                ui.close();
                            }
                        });

                        // CONFIDENCE
                        ui.label(format!("{:.2}", triple.meta.confidence));

                        // SOURCE
                        let source_text = format!("{:?}", triple.meta.source);
                        let source_color = match triple.meta.source {
                            crate::agent::mind::knowledge::Source::Perception => {
                                Color32::LIGHT_BLUE
                            }
                            crate::agent::mind::knowledge::Source::Inferred => {
                                Color32::LIGHT_YELLOW
                            }
                            crate::agent::mind::knowledge::Source::Intrinsic => Color32::LIGHT_GRAY,
                            crate::agent::mind::knowledge::Source::Cultural => {
                                Color32::from_rgb(255, 200, 150)
                            }
                            crate::agent::mind::knowledge::Source::Communicated => {
                                Color32::LIGHT_GREEN
                            }
                            crate::agent::mind::knowledge::Source::Observed => {
                                Color32::from_rgb(150, 200, 255)
                            }
                            crate::agent::mind::knowledge::Source::Experienced => {
                                Color32::from_rgb(255, 150, 200)
                            }
                            crate::agent::mind::knowledge::Source::Hearsay => {
                                Color32::from_rgb(200, 150, 255) // Purple for hearsay
                            }
                        };
                        ui.colored_label(source_color, source_text);

                        // AGE (time since creation)
                        // Format as seconds for readability
                        let age_secs = triple.meta.timestamp / 1000;
                        ui.label(format!("{}s", age_secs));

                        ui.end_row();
                    }
                });

            if mind.triples.is_empty() {
                ui.label("Mind is empty.");
            }
        });
    } else {
        ui.label("Selected entity has no MindGraph.");
    }
}

fn push_history(state: &mut KnowledgeInspectorState) {
    state.history.push(FilterState {
        subject: state.filter_subject.clone(),
        predicate: state.filter_predicate,
        object: state.filter_object.clone(),
    });
    // Cap history
    if state.history.len() > 20 {
        state.history.remove(0);
    }
}

fn value_to_node(val: &Value) -> Option<Node> {
    match val {
        Value::Concept(c) => Some(Node::Concept(*c)),
        Value::Entity(e) => Some(Node::Entity(*e)),
        Value::Tile(t) => Some(Node::Tile(*t)),
        _ => None,
    }
}

fn node_to_value(node: &Node) -> Option<Value> {
    match node {
        Node::Concept(c) => Some(Value::Concept(*c)),
        Node::Entity(e) => Some(Value::Entity(*e)),
        Node::Tile(t) => Some(Value::Tile(*t)),
        _ => None,
    }
}
