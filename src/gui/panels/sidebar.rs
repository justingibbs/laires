use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke, Vec2};

use crate::gui::ProjectSnapshot;
use crate::gui::state::{GuiState, RightTab, SidebarTab};
use crate::gui::theme::LairesTheme;

const ROW_HEIGHT: f32 = 34.0;
const ICON_WIDTH: f32 = 24.0;
const LEFT_PAD: f32 = 12.0;
const ACTIVE_BAR_WIDTH: f32 = 3.0;

pub fn render(
    ctx: &egui::Context,
    state: &mut GuiState,
    snapshot: &Option<ProjectSnapshot>,
    theme: &LairesTheme,
) {
    if !state.sidebar_visible {
        return;
    }

    egui::SidePanel::left("sidebar")
        .default_width(220.0)
        .min_width(180.0)
        .max_width(350.0)
        .frame(
            egui::Frame::NONE
                .fill(theme.bg_panel)
                .inner_margin(egui::Margin::same(0))
                .stroke(Stroke::new(1.0, theme.border)),
        )
        .show(ctx, |ui| {
            ui.add_space(8.0);

            // Navigation tabs
            render_nav_tab(ui, state, SidebarTab::Scenes, "\u{25B8}", theme);
            render_nav_tab(ui, state, SidebarTab::Files, "\u{1F4C1}", theme);

            ui.add_space(8.0);

            // Section header with count
            let section_label = match state.sidebar_tab {
                SidebarTab::Scenes => format!("SCENES ({})", state.scene_count),
                SidebarTab::Files => "FILES".to_string(),
            };
            ui.horizontal(|ui| {
                ui.add_space(LEFT_PAD);
                ui.label(
                    RichText::new(section_label)
                        .color(theme.text_secondary)
                        .size(10.0)
                        .strong(),
                );
            });
            ui.add_space(4.0);

            // Thin separator
            let rect = ui.available_rect_before_wrap();
            let sep_rect = egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), 1.0));
            ui.painter().rect_filled(sep_rect, 0.0, theme.border);
            ui.add_space(2.0);

            // Content
            egui::ScrollArea::vertical()
                .id_salt("sidebar_scroll")
                .show(ui, |ui| {
                    let Some(snap) = snapshot else {
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            ui.add_space(LEFT_PAD);
                            ui.label(
                                RichText::new("No project loaded.")
                                    .color(theme.text_secondary)
                                    .italics()
                                    .size(12.0),
                            );
                        });
                        return;
                    };

                    match state.sidebar_tab {
                        SidebarTab::Scenes => render_scenes(ui, state, snap, theme),
                        SidebarTab::Files => render_files(ui, state, snap, theme),
                    }
                });
        });
}

/// Renders a navigation tab row with icon, label, and active indicator.
fn render_nav_tab(
    ui: &mut egui::Ui,
    state: &mut GuiState,
    tab: SidebarTab,
    icon: &str,
    theme: &LairesTheme,
) {
    let is_active = state.sidebar_tab == tab;
    let label = match tab {
        SidebarTab::Scenes => "Scenes",
        SidebarTab::Files => "Files",
    };

    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), ROW_HEIGHT),
        egui::Sense::click(),
    );

    if response.clicked() {
        state.sidebar_tab = tab;
    }

    let painter = ui.painter();

    // Hover background
    if response.hovered() && !is_active {
        painter.rect_filled(
            rect,
            CornerRadius::same(0),
            Color32::from_rgba_premultiplied(0x36, 0x4F, 0xC7, 12),
        );
    }

    // Active background tint
    if is_active {
        painter.rect_filled(
            rect,
            CornerRadius::same(0),
            Color32::from_rgba_premultiplied(0x36, 0x4F, 0xC7, 20),
        );
    }

    // Active indicator bar (left edge)
    if is_active {
        let bar_rect =
            egui::Rect::from_min_size(rect.left_top(), Vec2::new(ACTIVE_BAR_WIDTH, rect.height()));
        painter.rect_filled(bar_rect, CornerRadius::same(0), theme.accent);
    }

    // Icon
    let icon_pos = egui::pos2(rect.min.x + LEFT_PAD, rect.center().y);
    let icon_color = if is_active {
        theme.accent
    } else {
        theme.text_secondary
    };
    painter.text(
        icon_pos,
        egui::Align2::LEFT_CENTER,
        icon,
        egui::FontId::proportional(14.0),
        icon_color,
    );

    // Label
    let text_pos = egui::pos2(rect.min.x + LEFT_PAD + ICON_WIDTH, rect.center().y);
    let text_color = if is_active {
        theme.text_primary
    } else {
        theme.text_secondary
    };
    painter.text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        text_color,
    );
}

fn render_scenes(
    ui: &mut egui::Ui,
    state: &mut GuiState,
    snap: &ProjectSnapshot,
    theme: &LairesTheme,
) {
    if snap.all_scenes.is_empty() {
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.add_space(LEFT_PAD);
            ui.label(
                RichText::new("No scenes found.")
                    .color(theme.text_secondary)
                    .size(12.0),
            );
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(LEFT_PAD);
            if ui.button("Scan Story").clicked() {
                state.scan_requested = true;
            }
        });
        return;
    }

    // Scan button when graph is empty (0 characters analyzed)
    if state.char_count == 0 {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(LEFT_PAD);
            let btn =
                egui::Button::new(RichText::new("Scan Story").color(Color32::WHITE).size(12.0))
                    .fill(theme.accent)
                    .corner_radius(CornerRadius::same(6));
            if ui.add(btn).clicked() {
                state.scan_requested = true;
            }
        });
        ui.add_space(8.0);
    }

    let multi_file = snap.all_scenes.len() > 1;
    let mut global_idx = 0usize;

    for group in &snap.all_scenes {
        if multi_file {
            ui.add_space(8.0);
            let display_name = if group.file_path.is_empty() {
                "Story".to_string()
            } else {
                group.file_path.clone()
            };
            ui.horizontal(|ui| {
                ui.add_space(LEFT_PAD);
                ui.label(
                    RichText::new(display_name.to_uppercase())
                        .color(theme.text_secondary)
                        .size(10.0)
                        .strong(),
                );
            });
            ui.add_space(2.0);
        }

        for (id, title) in &group.scenes {
            global_idx += 1;
            let fallback = format!("Scene {}", global_idx);
            let label = title.as_deref().unwrap_or(&fallback);
            let is_selected = state.selected_scene_id.as_deref() == Some(id.as_str());
            let is_stale = snap.pending_scene_ids.contains(id);
            let file_path = group.file_path.clone();

            render_scene_row(ui, label, is_selected, is_stale, theme, || {
                state.selected_scene_id = Some(id.clone());
                // Also select the file this scene belongs to
                if !file_path.is_empty() {
                    state.selected_file = Some(file_path.clone());
                }
            });
        }
    }
}

/// Renders a single scene item row with hover/selected styling.
fn render_scene_row(
    ui: &mut egui::Ui,
    label: &str,
    is_selected: bool,
    is_stale: bool,
    theme: &LairesTheme,
    on_click: impl FnOnce(),
) {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), ROW_HEIGHT),
        egui::Sense::click(),
    );

    if response.clicked() {
        on_click();
    }

    let painter = ui.painter();

    // Hover
    if response.hovered() && !is_selected {
        painter.rect_filled(
            rect,
            CornerRadius::same(0),
            Color32::from_rgba_premultiplied(0x36, 0x4F, 0xC7, 10),
        );
    }

    // Selected background
    if is_selected {
        painter.rect_filled(
            rect,
            CornerRadius::same(0),
            Color32::from_rgba_premultiplied(0x36, 0x4F, 0xC7, 18),
        );
        // Left accent bar
        let bar_rect =
            egui::Rect::from_min_size(rect.left_top(), Vec2::new(ACTIVE_BAR_WIDTH, rect.height()));
        painter.rect_filled(bar_rect, CornerRadius::same(0), theme.accent);
    }

    // Scene label
    let text_color = if is_selected {
        theme.text_primary
    } else {
        theme.text_primary
    };
    let text_pos = egui::pos2(rect.min.x + LEFT_PAD + 4.0, rect.center().y);
    painter.text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        text_color,
    );

    if is_stale {
        let badge_pos = egui::pos2(rect.max.x - LEFT_PAD, rect.center().y);
        painter.text(
            badge_pos,
            egui::Align2::RIGHT_CENTER,
            "STALE",
            egui::FontId::proportional(9.0),
            Color32::from_rgb(180, 83, 9),
        );
    }
}

fn render_files(
    ui: &mut egui::Ui,
    state: &mut GuiState,
    snap: &ProjectSnapshot,
    theme: &LairesTheme,
) {
    if snap.story_files.is_empty() && snap.context_files.is_empty() {
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.add_space(LEFT_PAD);
            ui.label(
                RichText::new("No manifest found.")
                    .color(theme.text_secondary)
                    .size(12.0),
            );
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(LEFT_PAD);
            if ui.button("Scan Story").clicked() {
                state.scan_requested = true;
            }
        });
        return;
    }

    if !snap.story_files.is_empty() {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(LEFT_PAD);
            ui.label(
                RichText::new(format!("STORY FILES ({})", snap.story_files.len()))
                    .color(theme.text_secondary)
                    .size(10.0)
                    .strong(),
            );
        });
        ui.add_space(2.0);
        for path in &snap.story_files {
            let is_selected = state.selected_file.as_deref() == Some(path.as_str());
            let is_stale = snap.pending_story_files.contains(path);
            if render_file_row(ui, path, "story", is_selected, is_stale, theme) {
                state.selected_file = Some(path.clone());
                state.selected_scene_id = None;
                state.active_right_tab = RightTab::Canvas;
            }
        }
    }

    if !snap.context_files.is_empty() {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(LEFT_PAD);
            ui.label(
                RichText::new(format!("CONTEXT FILES ({})", snap.context_files.len()))
                    .color(theme.text_secondary)
                    .size(10.0)
                    .strong(),
            );
        });
        ui.add_space(2.0);
        for path in &snap.context_files {
            render_file_row(ui, path, "context", false, false, theme);
        }
    }
}

/// Renders a single file row with icon and classification badge. Returns true if clicked.
fn render_file_row(
    ui: &mut egui::Ui,
    path: &str,
    classification: &str,
    is_selected: bool,
    is_stale: bool,
    theme: &LairesTheme,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), ROW_HEIGHT),
        egui::Sense::click(),
    );

    let painter = ui.painter();

    // Hover
    if response.hovered() && !is_selected {
        painter.rect_filled(
            rect,
            CornerRadius::same(0),
            Color32::from_rgba_premultiplied(0x36, 0x4F, 0xC7, 10),
        );
    }

    // Selected background
    if is_selected {
        painter.rect_filled(
            rect,
            CornerRadius::same(0),
            Color32::from_rgba_premultiplied(0x36, 0x4F, 0xC7, 18),
        );
        // Left accent bar
        let bar_rect =
            egui::Rect::from_min_size(rect.left_top(), Vec2::new(ACTIVE_BAR_WIDTH, rect.height()));
        painter.rect_filled(bar_rect, CornerRadius::same(0), theme.accent);
    }

    // File icon
    let icon_pos = egui::pos2(rect.min.x + LEFT_PAD, rect.center().y);
    painter.text(
        icon_pos,
        egui::Align2::LEFT_CENTER,
        "\u{1F4C4}",
        egui::FontId::proportional(12.0),
        theme.text_secondary,
    );

    // File name
    let text_pos = egui::pos2(rect.min.x + LEFT_PAD + ICON_WIDTH, rect.center().y);
    painter.text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        path,
        egui::FontId::proportional(12.0),
        theme.text_primary,
    );

    // Classification badge
    let badge_color = match classification {
        "story" => theme.accent,
        _ => theme.text_secondary,
    };
    let badge_text = if is_stale {
        format!("{} · STALE", classification.to_uppercase())
    } else {
        classification.to_uppercase()
    };
    let badge_pos = egui::pos2(rect.max.x - LEFT_PAD, rect.center().y);
    painter.text(
        badge_pos,
        egui::Align2::RIGHT_CENTER,
        &badge_text,
        egui::FontId::proportional(9.0),
        badge_color,
    );

    response.clicked()
}
