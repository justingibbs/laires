use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, RichText, Vec2};

use crate::gui::state::{GuiState, SessionMode};
use crate::gui::theme::{LairesTheme, PROSE_FONT};
use crate::gui::ProjectSnapshot;

/// Font used for story prose text (Source Serif 4).
fn prose_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(PROSE_FONT.into()))
}

pub fn render(
    ui: &mut egui::Ui,
    state: &mut GuiState,
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

    // Auto-select first file if none selected and we have per-file data
    if state.selected_file.is_none() && !snap.file_texts.is_empty() {
        // Use the first story file from the ordered list
        if let Some(first) = snap.story_files.first() {
            state.selected_file = Some(first.clone());
        }
    }

    // Resolve which text/boundaries to display
    let (display_text, display_boundaries, display_word_count) =
        if let Some(ref file_key) = state.selected_file {
            if let Some(ftd) = snap.file_texts.get(file_key) {
                (&ftd.text, &ftd.boundary_lines, ftd.word_count)
            } else {
                // Selected file not found in snapshot — fall back to combined
                (&snap.story_text, &snap.scene_boundary_lines, state.word_count)
            }
        } else {
            // No file selection (single-file project) — use combined text
            (&snap.story_text, &snap.scene_boundary_lines, state.word_count)
        };

    // Sync canvas edit buffer when file selection changes or buffer is empty
    let edit_file_matches = state.canvas_edit_file == state.selected_file;
    if !edit_file_matches || (state.canvas_edit_text.is_empty() && !display_text.is_empty()) {
        if !state.canvas_dirty {
            state.canvas_edit_text = display_text.clone();
            state.canvas_edit_file = state.selected_file.clone();
        }
    }

    // Wrap entire canvas in a card frame — fill all available space.
    // Frame overhead: outer_margin(4*2) + inner_margin(16*2) + stroke(1*2) + shadow(~4)
    let frame_overhead = 46.0;
    let target_inner_h = (ui.available_height() - frame_overhead).max(0.0);
    theme.card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.set_min_height(target_inner_h);

        // === File tabs (only when multiple files) ===
        if snap.file_texts.len() > 1 {
            render_file_tabs(ui, state, snap, theme);
            ui.add_space(4.0);
        }

        // === Breadcrumb header ===
        render_breadcrumb(ui, state, snap, theme, display_word_count);

        ui.add_space(12.0);

        // Thin divider below breadcrumb
        let rect = ui.available_rect_before_wrap();
        let sep_rect = egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), 1.0));
        ui.painter().rect_filled(sep_rect, 0.0, theme.border);
        ui.add_space(14.0);

        // === Prose content ===
        if state.session_mode == SessionMode::Workshop {
            render_editable(ui, state, theme);
        } else {
            egui::ScrollArea::vertical()
                .id_salt("canvas_scroll")
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    // Extra horizontal padding for a manuscript feel
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        ui.vertical(|ui| {
                            ui.set_max_width((ui.available_width() - 16.0).max(0.0));
                            render_prose(ui, display_text, display_boundaries, theme);
                        });
                    });
                });
        }
    });
}

/// Renders the canvas as an editable TextEdit in Workshop mode.
fn render_editable(ui: &mut egui::Ui, state: &mut GuiState, theme: &LairesTheme) {
    egui::ScrollArea::vertical()
        .id_salt("canvas_edit_scroll")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    let max_w = (ui.available_width() - 16.0).max(200.0);
                    ui.set_max_width(max_w);

                    let response = egui::TextEdit::multiline(&mut state.canvas_edit_text)
                        .font(prose_font(16.0))
                        .text_color(theme.prose_color)
                        .desired_width(max_w)
                        .frame(false)
                        .margin(egui::Margin::ZERO)
                        .show(ui);

                    if response.response.changed() {
                        state.canvas_dirty = true;
                    }
                });
            });
        });
}

/// Renders clickable file tabs when multiple story files exist.
fn render_file_tabs(
    ui: &mut egui::Ui,
    state: &mut GuiState,
    snap: &ProjectSnapshot,
    theme: &LairesTheme,
) {
    ui.horizontal(|ui| {
        for file_path in &snap.story_files {
            if !snap.file_texts.contains_key(file_path) {
                continue;
            }
            let is_active = state.selected_file.as_deref() == Some(file_path);

            // Extract just the filename for display
            let display_name = std::path::Path::new(file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(file_path);

            let text = RichText::new(display_name).size(12.0).color(if is_active {
                theme.accent
            } else {
                theme.text_secondary
            });

            let btn = egui::Button::new(text)
                .fill(if is_active {
                    Color32::from_rgba_premultiplied(0x36, 0x4F, 0xC7, 20)
                } else {
                    Color32::TRANSPARENT
                })
                .stroke(if is_active {
                    egui::Stroke::new(1.0, theme.accent)
                } else {
                    egui::Stroke::NONE
                })
                .corner_radius(CornerRadius::same(4));

            if ui.add(btn).clicked() {
                state.selected_file = Some(file_path.clone());
                state.selected_scene_id = None;
            }
        }
    });
}

/// Renders the breadcrumb with project name, file name, selected scene, and reading stats.
fn render_breadcrumb(
    ui: &mut egui::Ui,
    state: &GuiState,
    snap: &ProjectSnapshot,
    theme: &LairesTheme,
    word_count: usize,
) {
    // Top line: PROJECT > FILE > SCENE
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

        // Show file name if selected
        if let Some(ref file_path) = state.selected_file {
            let display_name = std::path::Path::new(file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(file_path);
            ui.label(
                RichText::new("\u{203A}")
                    .color(theme.text_secondary)
                    .size(11.0),
            );
            ui.label(
                RichText::new(display_name.to_uppercase())
                    .color(theme.text_secondary)
                    .size(11.0)
                    .strong(),
            );
        }

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

    // Scene title (large) — show selected scene or first scene in active file
    let display_title = selected_scene_title(state, snap)
        .unwrap_or_else(|| "Untitled".to_string());
    ui.label(
        RichText::new(&display_title)
            .font(prose_font(22.0))
            .color(theme.text_primary)
            .strong(),
    );

    // Reading metadata — scoped to active file
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
fn render_prose(
    ui: &mut egui::Ui,
    text: &str,
    boundary_lines: &std::collections::HashSet<usize>,
    theme: &LairesTheme,
) {
    // Increase line spacing for readability
    let prev_spacing = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 4.0;

    for (line_idx, line) in text.lines().enumerate() {
        let is_boundary = boundary_lines.contains(&line_idx);

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

/// Find the title of the currently selected scene (or first scene in active file if none selected).
fn selected_scene_title(state: &GuiState, snap: &ProjectSnapshot) -> Option<String> {
    let target_id = state.selected_scene_id.as_deref();

    // When a file is selected, only look at scenes from that file
    let groups: Vec<&crate::gui::FileSceneGroup> = if let Some(ref file_path) = state.selected_file
    {
        snap.all_scenes
            .iter()
            .filter(|g| g.file_path == *file_path)
            .collect()
    } else {
        snap.all_scenes.iter().collect()
    };

    for group in groups {
        for (id, title) in &group.scenes {
            if let Some(tid) = target_id {
                if id == tid {
                    return title.clone().or_else(|| Some("Scene".to_string()));
                }
            } else {
                // No selection — return first scene title
                return title.clone().or_else(|| Some("Scene 1".to_string()));
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
