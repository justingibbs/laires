use eframe::egui::{self, Color32, CornerRadius, RichText, Vec2};

use crate::gui::ProjectSnapshot;
use crate::gui::panels::graph_view::NodeDetail;
use crate::gui::state::GuiState;
use crate::gui::theme::LairesTheme;

const SECTION_SPACING: f32 = 12.0;
const BADGE_RADIUS: u8 = 4;

/// Renders the inspector panel for the currently selected graph node.
/// Returns true if the selection changed (a connected node was clicked).
pub fn render(
    ui: &mut egui::Ui,
    state: &mut GuiState,
    snapshot: &ProjectSnapshot,
    theme: &LairesTheme,
) {
    let selected_id = match &state.selected_node_id {
        Some(id) => id.clone(),
        None => return,
    };

    let Some(node) = snapshot.graph_nodes.iter().find(|n| n.id == selected_id) else {
        return;
    };

    egui::ScrollArea::vertical()
        .id_salt("inspector_scroll")
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            // Close button
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                if ui
                    .small_button(
                        RichText::new("\u{2715}")
                            .size(12.0)
                            .color(theme.text_secondary),
                    )
                    .clicked()
                {
                    state.selected_node_id = None;
                }
            });

            // Type badge
            render_type_badge(ui, &node.node_type, theme);
            ui.add_space(4.0);

            // Node title
            ui.label(
                RichText::new(&node.label)
                    .size(18.0)
                    .strong()
                    .color(theme.text_primary),
            );
            ui.add_space(SECTION_SPACING);

            // Type-specific fields
            match &node.detail {
                NodeDetail::Character {
                    aliases,
                    description,
                    ..
                } => render_character(ui, aliases, description, theme),
                NodeDetail::Objective {
                    character_id,
                    scope,
                    description,
                    evidence,
                    confidence,
                    status,
                } => render_objective(
                    ui,
                    character_id,
                    scope,
                    description,
                    evidence,
                    *confidence,
                    status,
                    snapshot,
                    state,
                    theme,
                ),
                NodeDetail::Scene {
                    summary,
                    characters_present,
                    location,
                    time,
                    file_path,
                    ..
                } => render_scene(
                    ui,
                    summary,
                    characters_present,
                    location,
                    time,
                    file_path,
                    snapshot,
                    state,
                    theme,
                ),
                NodeDetail::Conflict {
                    description,
                    objectives,
                } => render_conflict(ui, description, objectives, snapshot, state, theme),
            }

            // Connections section
            ui.add_space(SECTION_SPACING);
            render_connections(ui, &selected_id, snapshot, state, theme);
        });
}

fn render_type_badge(ui: &mut egui::Ui, node_type: &str, theme: &LairesTheme) {
    let color = node_type_color(node_type, theme);
    let label = node_type.to_uppercase();

    ui.horizontal(|ui| {
        // Colored dot
        let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
        ui.painter().circle_filled(dot_rect.center(), 4.0, color);

        // Type label
        ui.label(RichText::new(label).size(10.0).strong().color(color));
    });
}

fn render_character(
    ui: &mut egui::Ui,
    aliases: &[String],
    description: &Option<String>,
    theme: &LairesTheme,
) {
    if !aliases.is_empty() {
        render_field_label(ui, "Aliases", theme);
        ui.label(
            RichText::new(aliases.join(", "))
                .size(13.0)
                .color(theme.text_primary),
        );
        ui.add_space(8.0);
    }

    if let Some(desc) = description {
        render_field_label(ui, "Description", theme);
        ui.label(RichText::new(desc).size(13.0).color(theme.text_primary));
    }
}

fn render_objective(
    ui: &mut egui::Ui,
    character_id: &str,
    scope: &str,
    description: &str,
    evidence: &[String],
    confidence: f64,
    status: &str,
    snapshot: &ProjectSnapshot,
    state: &mut GuiState,
    theme: &LairesTheme,
) {
    // Description (full, since label may be truncated)
    render_field_label(ui, "Description", theme);
    ui.label(
        RichText::new(description)
            .size(13.0)
            .color(theme.text_primary),
    );
    ui.add_space(8.0);

    // Owner character (clickable)
    render_field_label(ui, "Character", theme);
    if let Some(char_node) = snapshot.graph_nodes.iter().find(|n| n.id == character_id) {
        if render_clickable_node(ui, &char_node.label, &char_node.node_type, theme) {
            state.selected_node_id = Some(character_id.to_string());
        }
    } else {
        ui.label(
            RichText::new(character_id)
                .size(13.0)
                .color(theme.text_primary),
        );
    }
    ui.add_space(8.0);

    // Scope + Status badges
    ui.horizontal(|ui| {
        render_pill(ui, scope, theme.accent, theme);
        ui.add_space(4.0);
        let status_color = status_color(status, theme);
        render_pill(ui, status, status_color, theme);
    });
    ui.add_space(8.0);

    // Confidence bar
    render_field_label(
        ui,
        &format!("Confidence: {:.0}%", confidence * 100.0),
        theme,
    );
    let bar_width = ui.available_width().min(200.0);
    let bar_height = 6.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(bar_width, bar_height), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), theme.bg_input);
    let fill_w = (rect.width() * confidence as f32).min(rect.width());
    let fill_rect = egui::Rect::from_min_size(rect.min, Vec2::new(fill_w, bar_height));
    let bar_color = if confidence >= 0.7 {
        theme.objective_color
    } else if confidence >= 0.4 {
        Color32::from_rgb(0xE6, 0x77, 0x00)
    } else {
        theme.conflict_color
    };
    painter.rect_filled(fill_rect, CornerRadius::same(3), bar_color);
    ui.add_space(8.0);

    // Evidence (collapsible)
    if !evidence.is_empty() {
        egui::CollapsingHeader::new(
            RichText::new(format!("Evidence ({})", evidence.len()))
                .size(11.0)
                .color(theme.text_secondary),
        )
        .default_open(false)
        .show(ui, |ui| {
            for ev in evidence {
                ui.add_space(2.0);
                egui::Frame::NONE
                    .fill(theme.bg_panel)
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(ev)
                                .size(11.0)
                                .italics()
                                .color(theme.text_secondary),
                        );
                    });
            }
        });
    }
}

fn render_scene(
    ui: &mut egui::Ui,
    summary: &str,
    characters_present: &[String],
    location: &Option<String>,
    time: &Option<String>,
    file_path: &str,
    snapshot: &ProjectSnapshot,
    state: &mut GuiState,
    theme: &LairesTheme,
) {
    // Summary
    if !summary.is_empty() {
        render_field_label(ui, "Summary", theme);
        ui.label(RichText::new(summary).size(13.0).color(theme.text_primary));
        ui.add_space(8.0);
    }

    // Location + Time
    if location.is_some() || time.is_some() {
        ui.horizontal(|ui| {
            if let Some(loc) = location {
                ui.label(
                    RichText::new(format!("\u{1F4CD} {loc}"))
                        .size(12.0)
                        .color(theme.text_secondary),
                );
            }
            if let Some(t) = time {
                ui.label(
                    RichText::new(format!("\u{1F551} {t}"))
                        .size(12.0)
                        .color(theme.text_secondary),
                );
            }
        });
        ui.add_space(8.0);
    }

    // File path
    if !file_path.is_empty() {
        render_field_label(ui, "File", theme);
        ui.label(
            RichText::new(file_path)
                .size(12.0)
                .monospace()
                .color(theme.text_secondary),
        );
        ui.add_space(8.0);
    }

    // Characters present (clickable)
    if !characters_present.is_empty() {
        render_field_label(ui, "Characters Present", theme);
        for char_id in characters_present {
            if let Some(char_node) = snapshot.graph_nodes.iter().find(|n| n.id == *char_id) {
                if render_clickable_node(ui, &char_node.label, &char_node.node_type, theme) {
                    state.selected_node_id = Some(char_id.clone());
                }
            } else {
                ui.label(RichText::new(char_id).size(12.0).color(theme.text_primary));
            }
        }
    }
}

fn render_conflict(
    ui: &mut egui::Ui,
    description: &str,
    objectives: &[String],
    snapshot: &ProjectSnapshot,
    state: &mut GuiState,
    theme: &LairesTheme,
) {
    // Full description
    render_field_label(ui, "Description", theme);
    ui.label(
        RichText::new(description)
            .size(13.0)
            .color(theme.text_primary),
    );
    ui.add_space(8.0);

    // Participating objectives (clickable)
    if !objectives.is_empty() {
        render_field_label(ui, "Objectives", theme);
        for obj_id in objectives {
            if let Some(obj_node) = snapshot.graph_nodes.iter().find(|n| n.id == *obj_id) {
                if render_clickable_node(ui, &obj_node.label, &obj_node.node_type, theme) {
                    state.selected_node_id = Some(obj_id.clone());
                }
            } else {
                ui.label(RichText::new(obj_id).size(12.0).color(theme.text_primary));
            }
        }
    }
}

/// Renders the connections section: all edges grouped by type.
fn render_connections(
    ui: &mut egui::Ui,
    node_id: &str,
    snapshot: &ProjectSnapshot,
    state: &mut GuiState,
    theme: &LairesTheme,
) {
    // Gather edges involving this node
    let mut outgoing: Vec<(&str, &str)> = Vec::new(); // (edge_label, target_id)
    let mut incoming: Vec<(&str, &str)> = Vec::new(); // (edge_label, source_id)

    for edge in &snapshot.graph_edges {
        if edge.source == node_id {
            outgoing.push((&edge.label, &edge.target));
        } else if edge.target == node_id {
            incoming.push((&edge.label, &edge.source));
        }
    }

    let total = outgoing.len() + incoming.len();
    if total == 0 {
        return;
    }

    // Section header
    render_separator(ui, theme);
    ui.add_space(4.0);
    ui.label(
        RichText::new(format!("CONNECTIONS ({})", total))
            .size(10.0)
            .strong()
            .color(theme.text_secondary),
    );
    ui.add_space(6.0);

    // Group outgoing by edge type
    let mut out_groups: std::collections::BTreeMap<&str, Vec<&str>> =
        std::collections::BTreeMap::new();
    for (label, target) in &outgoing {
        out_groups.entry(label).or_default().push(target);
    }

    for (edge_type, targets) in &out_groups {
        ui.label(
            RichText::new(format!("{} \u{2192}", format_edge_type(edge_type)))
                .size(11.0)
                .strong()
                .color(theme.text_secondary),
        );
        for target_id in targets {
            if let Some(target_node) = snapshot.graph_nodes.iter().find(|n| n.id == *target_id) {
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    if render_clickable_node(ui, &target_node.label, &target_node.node_type, theme)
                    {
                        state.selected_node_id = Some(target_id.to_string());
                    }
                });
            }
        }
        ui.add_space(4.0);
    }

    // Group incoming by edge type
    let mut in_groups: std::collections::BTreeMap<&str, Vec<&str>> =
        std::collections::BTreeMap::new();
    for (label, source) in &incoming {
        in_groups.entry(label).or_default().push(source);
    }

    for (edge_type, sources) in &in_groups {
        ui.label(
            RichText::new(format!("{} \u{2190}", format_edge_type(edge_type)))
                .size(11.0)
                .strong()
                .color(theme.text_secondary),
        );
        for source_id in sources {
            if let Some(source_node) = snapshot.graph_nodes.iter().find(|n| n.id == *source_id) {
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    if render_clickable_node(ui, &source_node.label, &source_node.node_type, theme)
                    {
                        state.selected_node_id = Some(source_id.to_string());
                    }
                });
            }
        }
        ui.add_space(4.0);
    }
}

// -- Helpers --

fn render_field_label(ui: &mut egui::Ui, label: &str, theme: &LairesTheme) {
    ui.label(
        RichText::new(label)
            .size(10.0)
            .strong()
            .color(theme.text_secondary),
    );
}

/// Renders a clickable node reference. Returns true if clicked.
fn render_clickable_node(
    ui: &mut egui::Ui,
    label: &str,
    node_type: &str,
    theme: &LairesTheme,
) -> bool {
    let color = node_type_color(node_type, theme);
    let response = ui.horizontal(|ui| {
        // Small colored dot
        let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
        ui.painter().circle_filled(dot_rect.center(), 3.0, color);

        // Clickable label
        let resp = ui.add(
            egui::Label::new(RichText::new(label).size(12.0).color(theme.text_accent))
                .sense(egui::Sense::click()),
        );
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
    });
    response.inner
}

fn render_pill(ui: &mut egui::Ui, text: &str, color: Color32, _theme: &LairesTheme) {
    let bg = Color32::from_rgba_premultiplied(color.r(), color.g(), color.b(), 20);
    egui::Frame::NONE
        .fill(bg)
        .corner_radius(CornerRadius::same(BADGE_RADIUS))
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(10.0).strong().color(color));
        });
}

fn render_separator(ui: &mut egui::Ui, theme: &LairesTheme) {
    let rect = ui.available_rect_before_wrap();
    let sep = egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), 1.0));
    ui.painter().rect_filled(sep, 0.0, theme.border);
    ui.add_space(1.0);
}

fn node_type_color(node_type: &str, theme: &LairesTheme) -> Color32 {
    match node_type {
        "character" => theme.character_color,
        "objective" => theme.objective_color,
        "scene" => theme.scene_color,
        "conflict" => theme.conflict_color,
        _ => theme.text_secondary,
    }
}

fn status_color(status: &str, theme: &LairesTheme) -> Color32 {
    match status {
        "Active" => theme.objective_color,
        "Achieved" => Color32::from_rgb(0x2B, 0x8A, 0x3E),
        "Abandoned" => theme.text_secondary,
        "Blocked" => theme.conflict_color,
        "Transformed" => theme.accent_secondary,
        _ => theme.text_secondary,
    }
}

fn format_edge_type(edge_type: &str) -> &str {
    match edge_type {
        "pursues" => "Pursues",
        "decomposes" => "Decomposes into",
        "conflicts" => "Conflicts with",
        "present_in" => "Present in",
        "advances" => "Advances",
        "blocks" => "Blocks",
        "precedes" => "Precedes",
        "transforms" => "Transforms",
        other => other,
    }
}
