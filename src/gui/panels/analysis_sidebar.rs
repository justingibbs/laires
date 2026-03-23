use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke, Vec2};

use crate::gui::ProjectSnapshot;
use crate::gui::state::GuiState;
use crate::gui::theme::LairesTheme;

/// Width of the analysis sidebar when visible.
pub const SIDEBAR_WIDTH: f32 = 230.0;

/// Renders the analysis sidebar (right of canvas) with entity legend,
/// detection status, and an explainer card.
pub fn render(
    ui: &mut egui::Ui,
    state: &mut GuiState,
    snapshot: &Option<ProjectSnapshot>,
    theme: &LairesTheme,
) {
    let Some(snap) = snapshot else { return };

    ui.set_min_width(SIDEBAR_WIDTH - 16.0);

    // === Token Legend ===
    ui.label(
        RichText::new("TOKEN LEGEND")
            .color(theme.text_secondary)
            .size(10.0)
            .strong(),
    );
    ui.add_space(8.0);

    // Character count
    let char_count = snap
        .graph_nodes
        .iter()
        .filter(|n| n.node_type == "character")
        .count();
    // Location/object counts from graph (objectives can proxy for locations/objects)
    let objective_count = snap
        .graph_nodes
        .iter()
        .filter(|n| n.node_type == "objective")
        .count();
    let scene_count = snap
        .graph_nodes
        .iter()
        .filter(|n| n.node_type == "scene")
        .count();

    render_toggle_row(
        ui,
        &mut state.highlight_characters,
        "Characters",
        char_count,
        theme.character_color,
        theme,
    );
    ui.add_space(4.0);
    render_toggle_row(
        ui,
        &mut state.highlight_locations,
        "Locations",
        scene_count, // scenes as location proxy
        theme.objective_color,
        theme,
    );
    ui.add_space(4.0);
    render_toggle_row(
        ui,
        &mut state.highlight_objects,
        "Objects",
        objective_count,
        theme.scene_color,
        theme,
    );

    ui.add_space(16.0);

    // === Detection Status ===
    ui.label(
        RichText::new("DETECTION STATUS")
            .color(theme.text_secondary)
            .size(10.0)
            .strong(),
    );
    ui.add_space(8.0);

    let total_nodes = snap.graph_nodes.len();
    let total_edges = snap.graph_edges.len();

    // Approximate detection levels from graph completeness
    let (resolved_pct, partial_pct, unknown_pct) = if total_nodes == 0 {
        (0.0, 0.0, 100.0)
    } else {
        // Heuristic: nodes with edges = resolved, nodes without = partial, remainder unknown
        let nodes_with_edges: std::collections::HashSet<&str> = snap
            .graph_edges
            .iter()
            .flat_map(|e| [e.source.as_str(), e.target.as_str()])
            .collect();
        let resolved = nodes_with_edges.len().min(total_nodes);
        let partial = total_nodes.saturating_sub(resolved);
        let r = resolved as f32 / total_nodes as f32 * 100.0;
        let p = partial as f32 / total_nodes as f32 * 100.0;
        let u = (100.0 - r - p).max(0.0);
        (r, p, u)
    };

    render_status_bar(ui, "Resolved", resolved_pct, theme.objective_color, theme);
    ui.add_space(6.0);
    render_status_bar(ui, "Partially Known", partial_pct, theme.scene_color, theme);
    ui.add_space(6.0);
    render_status_bar(ui, "Unknown", unknown_pct, theme.text_secondary, theme);

    ui.add_space(16.0);

    // === How it works card ===
    egui::Frame::NONE
        .fill(Color32::from_rgb(0xED, 0xF2, 0xFF)) // light accent tint
        .stroke(Stroke::new(1.0, Color32::from_rgb(0xC5, 0xD4, 0xF7)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.label(
                RichText::new("\u{2139} How it works")
                    .color(theme.accent)
                    .size(11.0)
                    .strong(),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Laires scans your text for Named Entities (NER) and \
                     cross-references them with your project\u{2019}s Story Graph.",
                )
                .color(theme.text_secondary)
                .size(11.0),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "{} nodes \u{00B7} {} connections tracked",
                    total_nodes, total_edges
                ))
                .color(theme.text_primary)
                .size(11.0)
                .strong(),
            );
        });

    ui.add_space(12.0);

    // View full analysis link
    if ui
        .link(
            RichText::new("View Full Analysis Data \u{2192}")
                .color(theme.accent)
                .size(12.0),
        )
        .on_hover_text("Switch to Graph tab")
        .clicked()
    {
        state.active_right_tab = crate::gui::state::RightTab::Graph;
    }
}

/// A toggle row: colored dot + label + count, clickable to toggle.
fn render_toggle_row(
    ui: &mut egui::Ui,
    enabled: &mut bool,
    label: &str,
    count: usize,
    color: Color32,
    theme: &LairesTheme,
) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), egui::Sense::click());

    if response.clicked() {
        *enabled = !*enabled;
    }

    let painter = ui.painter();

    // Hover
    if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(4), theme.accent_hover);
    }

    // Colored dot (filled when enabled, outline when disabled)
    let dot_center = egui::pos2(rect.min.x + 10.0, rect.center().y);
    if *enabled {
        painter.circle_filled(dot_center, 5.0, color);
    } else {
        painter.circle_stroke(dot_center, 5.0, Stroke::new(1.5, color));
    }

    // Label
    let text_color = if *enabled {
        theme.text_primary
    } else {
        theme.text_secondary
    };
    painter.text(
        egui::pos2(rect.min.x + 24.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        text_color,
    );

    // Count badge
    painter.text(
        egui::pos2(rect.max.x - 8.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        count.to_string(),
        egui::FontId::proportional(11.0),
        theme.text_secondary,
    );
}

/// A labeled horizontal status bar with percentage.
fn render_status_bar(
    ui: &mut egui::Ui,
    label: &str,
    percentage: f32,
    color: Color32,
    theme: &LairesTheme,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(theme.text_primary).size(11.0));
        ui.label(
            RichText::new(format!("{:.0}%", percentage))
                .color(color)
                .size(11.0)
                .strong(),
        );
    });

    // Bar
    let bar_w = ui.available_width();
    let bar_h = 6.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(bar_w, bar_h), egui::Sense::hover());

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), theme.bg_input);

    let frac = (percentage / 100.0).clamp(0.0, 1.0);
    if frac > 0.0 {
        let filled =
            egui::Rect::from_min_size(rect.min, Vec2::new(rect.width() * frac, rect.height()));
        painter.rect_filled(filled, CornerRadius::same(3), color);
    }
}
