use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke};

use crate::gui::ProjectSnapshot;
use crate::gui::state::{AgentStatus, AppMode, GuiState, RightTab, SearchResult, SessionMode};
use crate::gui::theme::LairesTheme;

/// Stable ID for the search text input — used to request focus from Cmd+F.
pub const SEARCH_INPUT_ID: &str = "search_bar_input";

/// Renders the top navigation bar (branded header with search + actions).
pub fn render(
    ctx: &egui::Context,
    state: &mut GuiState,
    snapshot: &Option<ProjectSnapshot>,
    theme: &LairesTheme,
) {
    if state.app_mode != AppMode::Project {
        return;
    }

    egui::TopBottomPanel::top("top_bar")
        .exact_height(52.0)
        .frame(
            egui::Frame::NONE
                .fill(Color32::WHITE)
                .inner_margin(egui::Margin::symmetric(16, 0))
                .stroke(Stroke::new(1.0, theme.border)),
        )
        .show(ctx, |ui| {
            let bar_width = ui.available_width();
            let is_narrow = bar_width < 900.0;
            let is_very_narrow = bar_width < 600.0;

            ui.set_max_width(bar_width);
            ui.horizontal_centered(|ui| {
                ui.set_max_width(bar_width);
                // === Left section: Branding ===
                ui.label(RichText::new("\u{25C8}").color(theme.accent).size(22.0));
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Laires")
                            .color(theme.text_primary)
                            .size(16.0)
                            .strong(),
                    );
                    if !is_very_narrow {
                        ui.label(
                            RichText::new("Writing Analysis")
                                .color(theme.text_secondary)
                                .size(10.0),
                        );
                    }
                });

                ui.add_space(8.0);

                // === Center-left: Project title + provider info ===
                if !state.project_title.is_empty() {
                    ui.label(RichText::new("\u{2502}").color(theme.border).size(20.0));
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.add_space(8.0);
                        let title = RichText::new(&state.project_title)
                            .color(theme.text_primary)
                            .size(14.0)
                            .strong();
                        if ui.link(title).on_hover_text("Switch project").clicked() {
                            ctx.memory_mut(|mem| {
                                mem.data
                                    .insert_temp(egui::Id::new("switch_to_welcome"), true);
                            });
                        }
                        if !is_very_narrow && !state.model_name.is_empty() {
                            ui.label(
                                RichText::new(format!(
                                    "{} \u{00B7} {}",
                                    state.privacy_label, state.model_name
                                ))
                                .color(theme.text_secondary)
                                .size(11.0),
                            );
                        }
                    });
                }

                // === Session mode toggle badge ===
                ui.add_space(8.0);
                let (mode_label, mode_color) = match state.session_mode {
                    SessionMode::Consultant => ("Consultant", Color32::from_rgb(59, 130, 246)),
                    SessionMode::Workshop => ("Workshop", Color32::from_rgb(34, 197, 94)),
                };
                let btn = egui::Button::new(
                    RichText::new(mode_label)
                        .color(mode_color)
                        .size(10.0)
                        .strong(),
                )
                .fill(mode_color.gamma_multiply(0.15))
                .stroke(Stroke::NONE)
                .corner_radius(CornerRadius::same(4));
                let tooltip = match state.session_mode {
                    SessionMode::Consultant => "Switch to Workshop mode (live editing)",
                    SessionMode::Workshop => "Switch to Consultant mode (read-only analysis)",
                };
                if ui.add(btn).on_hover_text(tooltip).clicked() {
                    ctx.memory_mut(|mem| {
                        mem.data.insert_temp(egui::Id::new("switch_mode"), true);
                    });
                }

                // === Center: Search bar (only if there's room) ===
                if !is_narrow {
                    ui.add_space(16.0);
                    let search_width = (ui.available_width() - 280.0).max(80.0);
                    let search_bar_resp = ui.allocate_ui(egui::vec2(search_width, 32.0), |ui| {
                        ui.add_space(4.0);
                        egui::Frame::NONE
                            .fill(theme.bg_secondary)
                            .stroke(Stroke::new(1.0, theme.border))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.horizontal_centered(|ui| {
                                    ui.label(
                                        RichText::new("\u{1F50D}")
                                            .color(theme.text_secondary)
                                            .size(12.0),
                                    );
                                    let te = egui::TextEdit::singleline(&mut state.search_query)
                                        .hint_text("Search story elements...")
                                        .text_color(theme.text_primary)
                                        .font(egui::FontId::proportional(12.0))
                                        .frame(false)
                                        .desired_width(ui.available_width())
                                        .id(egui::Id::new(SEARCH_INPUT_ID));
                                    let response = ui.add(te);

                                    // Run search when query changes
                                    if response.changed() {
                                        state.search_results =
                                            run_search(&state.search_query, snapshot);
                                        state.search_selected_index = 0;
                                        state.search_active = !state.search_query.is_empty();
                                    }

                                    // Open dropdown when the input gains focus and has a query
                                    if response.gained_focus() && !state.search_query.is_empty() {
                                        state.search_active = true;
                                    }
                                });
                            });
                    });

                    // Show dropdown whenever active and there are results.
                    // Dismissal is handled explicitly: Escape, navigation, or
                    // clearing the query — NOT by TextEdit focus loss, which
                    // would race with the click on the dropdown button.
                    if state.search_active && !state.search_results.is_empty() {
                        let bar_rect = search_bar_resp.response.rect;
                        render_search_dropdown(ctx, state, theme, bar_rect);
                    }
                }

                // === Right section: Stats + Actions ===
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Settings (rightmost)
                    let gear = RichText::new("\u{2699}")
                        .size(18.0)
                        .color(theme.text_secondary);
                    if ui.link(gear).on_hover_text("Settings (Cmd+,)").clicked() {
                        ctx.memory_mut(|mem| {
                            mem.data.insert_temp(egui::Id::new("open_settings"), true);
                        });
                    }

                    ui.add_space(8.0);

                    // Agent status
                    match &state.agent_status {
                        AgentStatus::Idle => {
                            ui.label(
                                RichText::new("\u{25CF}")
                                    .color(theme.objective_color)
                                    .size(11.0),
                            );
                        }
                        AgentStatus::Thinking => {
                            ui.spinner();
                        }
                        AgentStatus::ToolCall(name) => {
                            ui.spinner();
                            if !is_very_narrow {
                                ui.label(
                                    RichText::new(format!("{name}..."))
                                        .color(theme.accent)
                                        .size(11.0),
                                );
                            }
                        }
                    }

                    ui.add_space(8.0);

                    // Scan button — always available so the user controls
                    // when LLM analysis runs (like hitting Save in Word).
                    {
                        let (label, fill, text_color) = if state.review_pending {
                            let pending_label = snapshot
                                .as_ref()
                                .map(|snap| format!("Scan ({} stale)", snap.pending_scene_count,))
                                .unwrap_or_else(|| "Scan".to_string());
                            (pending_label, Color32::from_rgb(180, 83, 9), Color32::WHITE)
                        } else if state.char_count == 0 {
                            ("Scan Story".to_string(), theme.accent, Color32::WHITE)
                        } else {
                            ("Scan".to_string(), theme.accent, Color32::WHITE)
                        };
                        let scan_btn =
                            egui::Button::new(RichText::new(label).color(text_color).size(12.0))
                                .fill(fill)
                                .corner_radius(CornerRadius::same(8));
                        let response = ui.add(scan_btn);
                        if response.clicked() {
                            ctx.memory_mut(|mem| {
                                mem.data.insert_temp(egui::Id::new("scan_requested"), true);
                            });
                        }
                        if state.review_pending
                            && let Some(text) = &state.review_status_text
                        {
                            response.on_hover_text(text);
                        }
                    }

                    // Compact stats (hide on very narrow)
                    if !is_very_narrow {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(format!(
                                "{} scenes \u{00B7} {} chars \u{00B7} {} words",
                                state.scene_count, state.char_count, state.word_count
                            ))
                            .color(theme.text_secondary)
                            .size(11.0),
                        );
                    }

                    if !is_narrow && let Some(usage) = &state.last_usage {
                        ui.add_space(8.0);
                        ui.label(RichText::new(usage).color(theme.text_secondary).size(10.0));
                    }
                });
            });
        });
}

/// Run a case-insensitive substring search across all file texts in the snapshot.
fn run_search(query: &str, snapshot: &Option<ProjectSnapshot>) -> Vec<SearchResult> {
    let Some(snap) = snapshot else {
        return Vec::new();
    };
    let query_lower = query.to_lowercase();
    if query_lower.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();

    // Search per-file texts (ordered by story_files for stable ordering)
    if !snap.file_texts.is_empty() {
        for file_path in &snap.story_files {
            if let Some(ftd) = snap.file_texts.get(file_path) {
                for (line_idx, line) in ftd.text.lines().enumerate() {
                    if line.to_lowercase().contains(&query_lower) {
                        results.push(SearchResult {
                            file_path: file_path.clone(),
                            line_number: line_idx + 1,
                            context: line.trim().to_string(),
                        });
                        if results.len() >= 50 {
                            return results;
                        }
                    }
                }
            }
        }
    } else {
        // Single-file fallback: search combined story_text
        for (line_idx, line) in snap.story_text.lines().enumerate() {
            if line.to_lowercase().contains(&query_lower) {
                results.push(SearchResult {
                    file_path: String::new(),
                    line_number: line_idx + 1,
                    context: line.trim().to_string(),
                });
                if results.len() >= 50 {
                    return results;
                }
            }
        }
    }

    results
}

/// Renders a floating dropdown of search results below the search bar.
fn render_search_dropdown(
    ctx: &egui::Context,
    state: &mut GuiState,
    theme: &LairesTheme,
    bar_rect: egui::Rect,
) {
    // Handle keyboard navigation while search is active
    ctx.input(|i| {
        if i.key_pressed(egui::Key::ArrowDown)
            || (i.key_pressed(egui::Key::Enter) && i.modifiers.shift)
        {
            // intentionally empty — handled below
        }
    });
    let move_down = ctx.input(|i| i.key_pressed(egui::Key::ArrowDown));
    let move_up = ctx.input(|i| i.key_pressed(egui::Key::ArrowUp));
    let confirm = ctx.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));

    if escape {
        state.search_active = false;
        state.search_query.clear();
        state.search_results.clear();
        return;
    }

    let result_count = state.search_results.len();
    if move_down && result_count > 0 {
        state.search_selected_index = (state.search_selected_index + 1).min(result_count - 1);
    }
    if move_up && state.search_selected_index > 0 {
        state.search_selected_index -= 1;
    }
    if confirm && state.search_selected_index < result_count {
        navigate_to_result(state, state.search_selected_index);
        return;
    }

    let dropdown_pos = egui::pos2(bar_rect.left(), bar_rect.bottom() + 4.0);
    let dropdown_width = bar_rect.width().max(300.0);
    let max_visible = 10;
    let row_height = 28.0;
    let dropdown_height = (result_count.min(max_visible) as f32) * row_height + 8.0;

    egui::Area::new(egui::Id::new("search_results_dropdown"))
        .fixed_pos(dropdown_pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(Color32::WHITE)
                .stroke(Stroke::new(1.0, theme.border))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(4, 4))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 2],
                    blur: 8,
                    spread: 0,
                    color: Color32::from_black_alpha(20),
                })
                .show(ui, |ui| {
                    ui.set_width(dropdown_width);
                    egui::ScrollArea::vertical()
                        .max_height(dropdown_height)
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            let mut clicked_idx: Option<usize> = None;

                            for (idx, result) in state.search_results.clone().iter().enumerate() {
                                let is_selected = idx == state.search_selected_index;

                                // Build the row label text
                                let file_label = if result.file_path.is_empty() {
                                    format!("L{}", result.line_number)
                                } else {
                                    let fname = std::path::Path::new(&result.file_path)
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or(&result.file_path);
                                    format!("{}:{}", fname, result.line_number)
                                };
                                let context = if result.context.len() > 80 {
                                    format!("{}...", &result.context[..77])
                                } else {
                                    result.context.clone()
                                };

                                // Use a LayoutJob for colored file label + context
                                let mut job = egui::text::LayoutJob::default();
                                job.append(
                                    &file_label,
                                    0.0,
                                    egui::TextFormat {
                                        font_id: egui::FontId::proportional(11.0),
                                        color: theme.accent,
                                        ..Default::default()
                                    },
                                );
                                job.append(
                                    &format!("  {context}"),
                                    0.0,
                                    egui::TextFormat {
                                        font_id: egui::FontId::proportional(11.0),
                                        color: theme.text_primary,
                                        ..Default::default()
                                    },
                                );
                                let btn_text = egui::WidgetText::from(job);

                                let fill = if is_selected {
                                    theme.accent.gamma_multiply(0.12)
                                } else {
                                    Color32::TRANSPARENT
                                };
                                let btn = egui::Button::new(btn_text)
                                    .fill(fill)
                                    .stroke(Stroke::NONE)
                                    .corner_radius(CornerRadius::same(4))
                                    .min_size(egui::vec2(dropdown_width - 8.0, 0.0));

                                let response = ui.add(btn);
                                if response.clicked() {
                                    clicked_idx = Some(idx);
                                }
                                if response.hovered() {
                                    state.search_selected_index = idx;
                                }
                            }

                            if let Some(idx) = clicked_idx {
                                state.search_selected_index = idx;
                                navigate_to_result(state, idx);
                            }
                        });

                    // Result count footer
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        let count_text = if state.search_results.len() >= 50 {
                            "50+ matches".to_string()
                        } else {
                            format!(
                                "{} {}",
                                state.search_results.len(),
                                if state.search_results.len() == 1 {
                                    "match"
                                } else {
                                    "matches"
                                }
                            )
                        };
                        ui.label(
                            RichText::new(count_text)
                                .color(theme.text_secondary)
                                .size(10.0),
                        );
                    });
                });
        });
}

/// Navigate to a search result: switch file, set scroll target, switch to Canvas tab.
fn navigate_to_result(state: &mut GuiState, index: usize) {
    if let Some(result) = state.search_results.get(index) {
        if !result.file_path.is_empty() {
            state.search_navigate_file = Some(result.file_path.clone());
        }
        state.search_scroll_to_line = Some(result.line_number.saturating_sub(1)); // 0-based
        state.active_right_tab = RightTab::Canvas;
        state.selected_scene_id = None;
        state.search_active = false;
    }
}
