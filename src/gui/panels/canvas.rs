use eframe::egui::{self, FontFamily, FontId, RichText, Vec2};

use crate::gui::state::GuiState;
use crate::gui::theme::{LairesTheme, PROSE_FONT};
use crate::gui::ProjectSnapshot;

/// Font used for story prose text (Source Serif 4).
fn prose_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(PROSE_FONT.into()))
}

pub fn render(
    ui: &mut egui::Ui,
    state: &GuiState,
    snapshot: &Option<ProjectSnapshot>,
    theme: &LairesTheme,
) {
    let Some(snap) = snapshot else {
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new("No project loaded.")
                    .color(theme.text_secondary)
                    .italics()
                    .size(14.0),
            );
        });
        return;
    };

    if snap.story_text.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new("No story text found. Add story files and run Scan.")
                    .color(theme.text_secondary)
                    .italics()
                    .size(14.0),
            );
        });
        return;
    }

    // Wrap entire canvas in a card frame
    theme.card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());

        // === Breadcrumb header ===
        render_breadcrumb(ui, state, snap, theme);

        ui.add_space(12.0);

        // Thin divider below breadcrumb
        let rect = ui.available_rect_before_wrap();
        let sep_rect = egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), 1.0));
        ui.painter().rect_filled(sep_rect, 0.0, theme.border);
        ui.add_space(14.0);

        // === Prose content ===
        egui::ScrollArea::vertical()
            .id_salt("canvas_scroll")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                // Extra horizontal padding for a manuscript feel
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.set_max_width((ui.available_width() - 16.0).max(0.0));
                        render_prose(ui, snap, theme);
                    });
                });
            });
    });
}

/// Renders the breadcrumb with project name, selected scene, and reading stats.
fn render_breadcrumb(
    ui: &mut egui::Ui,
    state: &GuiState,
    snap: &ProjectSnapshot,
    theme: &LairesTheme,
) {
    // Top line: PROJECT > SCENE
    ui.horizontal(|ui| {
        // Project name
        let project_name = if state.project_title.is_empty() {
            "Project"
        } else {
            &state.project_title
        };
        ui.label(
            RichText::new(project_name.to_uppercase())
                .color(theme.accent)
                .size(11.0)
                .strong(),
        );

        // If a scene is selected, show it
        if let Some(scene_id) = &state.selected_scene_id {
            ui.label(
                RichText::new("\u{203A}")
                    .color(theme.text_secondary)
                    .size(11.0),
            );
            // Find the scene title
            let scene_title = snap
                .all_scenes
                .iter()
                .flat_map(|g| &g.scenes)
                .find(|(id, _)| id == scene_id)
                .and_then(|(_, title)| title.as_deref())
                .unwrap_or("Scene");
            ui.label(
                RichText::new(scene_title.to_uppercase())
                    .color(theme.text_secondary)
                    .size(11.0)
                    .strong(),
            );
        }
    });

    // Scene title (large) — show selected scene or first scene
    let display_title = selected_scene_title(state, snap)
        .unwrap_or_else(|| "Untitled".to_string());
    ui.label(
        RichText::new(&display_title)
            .font(prose_font(22.0))
            .color(theme.text_primary)
            .strong(),
    );

    // Reading metadata
    let word_count = state.word_count;
    let reading_mins = (word_count as f32 / 250.0).ceil() as usize;
    let reading_time = if reading_mins <= 1 {
        "< 1 minute".to_string()
    } else {
        format!("{} minutes", reading_mins)
    };
    ui.label(
        RichText::new(format!(
            "Estimated reading time: {} \u{00B7} {} words",
            reading_time,
            format_number(word_count),
        ))
        .color(theme.text_secondary)
        .size(12.0),
    );
}

/// Renders the prose text with scene boundary dividers instead of colored text.
fn render_prose(ui: &mut egui::Ui, snap: &ProjectSnapshot, theme: &LairesTheme) {
    // Increase line spacing for readability
    let prev_spacing = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 4.0;

    for (line_idx, line) in snap.story_text.lines().enumerate() {
        let is_boundary = snap.scene_boundary_lines.contains(&line_idx);

        if is_boundary {
            // Scene boundary: subtle horizontal rule + scene header
            ui.add_space(12.0);

            // Horizontal rule
            let rect = ui.available_rect_before_wrap();
            let rule_rect = egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), 1.0));
            ui.painter()
                .rect_filled(rule_rect, 0.0, theme.border);
            ui.add_space(6.0);

            // Scene heading in small secondary text
            ui.label(
                RichText::new(line)
                    .color(theme.text_secondary)
                    .size(12.0)
                    .strong(),
            );

            ui.add_space(8.0);
        } else if line.trim().is_empty() {
            // Empty lines get a bit of paragraph spacing
            ui.add_space(6.0);
        } else {
            // Normal prose line
            ui.label(
                RichText::new(line)
                    .font(prose_font(16.0))
                    .color(theme.prose_color),
            );
        }
    }

    // Restore spacing
    ui.spacing_mut().item_spacing.y = prev_spacing;
}

/// Find the title of the currently selected scene (or first scene if none selected).
fn selected_scene_title(state: &GuiState, snap: &ProjectSnapshot) -> Option<String> {
    let target_id = state.selected_scene_id.as_deref();
    for group in &snap.all_scenes {
        for (id, title) in &group.scenes {
            if let Some(tid) = target_id {
                if id == tid {
                    return title.clone().or_else(|| Some(format!("Scene")));
                }
            } else {
                // No selection — return first scene title
                return title.clone().or_else(|| Some(format!("Scene 1")));
            }
        }
    }
    None
}

/// Format a number with comma separators (e.g. 42850 -> "42,850").
fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}
