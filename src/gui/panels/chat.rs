use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke, Vec2};

use crate::gui::state::{AgentStatus, ChatRole, GuiState};
use crate::gui::theme::LairesTheme;

const AVATAR_SIZE: f32 = 28.0;
const BUBBLE_RADIUS: u8 = 12;
const BUBBLE_RADIUS_TAIL: u8 = 4;
const INPUT_HEIGHT: f32 = 72.0;

/// Color for the context bar based on fill percentage.
fn context_bar_color(percent: f32) -> Color32 {
    if percent >= 85.0 {
        Color32::from_rgb(0xC9, 0x2A, 0x2A) // red
    } else if percent >= 60.0 {
        Color32::from_rgb(0xE6, 0x77, 0x00) // orange
    } else if percent >= 50.0 {
        Color32::from_rgb(0xF5, 0x9F, 0x00) // yellow
    } else {
        Color32::from_rgb(0x2B, 0x8A, 0x3E) // green
    }
}

/// Renders the chat panel. Returns true if the user submitted a message.
pub fn render(ui: &mut egui::Ui, state: &mut GuiState, theme: &LairesTheme) -> bool {
    let input_reserved = INPUT_HEIGHT + 20.0;

    // === Header row: "Chat" + context bar + session controls + spinner ===
    render_header(ui, state, theme);
    ui.add_space(2.0);

    // === Context warning (only when >= 50%) ===
    if state.context_window_percent >= 50.0 {
        render_context_warning(ui, state, theme);
        ui.add_space(2.0);
    }

    // Message history
    let scroll_height = (ui.available_height() - input_reserved).max(60.0);
    egui::ScrollArea::vertical()
        .id_salt("chat_scroll")
        .max_height(scroll_height)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for (msg_idx, msg) in state.chat_history.iter().enumerate() {
                render_message(ui, msg_idx, msg, theme);
            }

            // Typing indicator when agent is active
            match &state.agent_status {
                AgentStatus::Thinking => {
                    render_typing_indicator(ui, "Thinking...", theme);
                }
                AgentStatus::ToolCall(name) => {
                    render_typing_indicator(ui, &format!("Running {name}..."), theme);
                }
                AgentStatus::Idle => {}
            }
        });

    // Input area
    ui.add_space(4.0);
    render_input(ui, state, theme)
}

/// Renders the compact header: Chat label, context bar, New Session button, spinner.
fn render_header(ui: &mut egui::Ui, state: &mut GuiState, theme: &LairesTheme) {
    ui.horizontal(|ui| {
        // "Chat" label
        ui.label(
            RichText::new("Chat")
                .color(theme.text_primary)
                .size(15.0)
                .strong(),
        );

        ui.add_space(8.0);

        // Context window progress bar (compact)
        let percent = state.context_window_percent;
        if percent > 0.0 {
            let bar_width = 100.0;
            let bar_height = 8.0;
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(bar_width, bar_height), egui::Sense::hover());

            let painter = ui.painter();

            // Background track
            painter.rect_filled(
                rect,
                CornerRadius::same(4),
                theme.bg_input,
            );

            // Filled portion
            let fill_width = (rect.width() * (percent / 100.0)).min(rect.width());
            let fill_rect = egui::Rect::from_min_size(rect.min, Vec2::new(fill_width, bar_height));
            painter.rect_filled(
                fill_rect,
                CornerRadius::same(4),
                context_bar_color(percent),
            );

            ui.add_space(4.0);

            // Percentage label
            ui.label(
                RichText::new(format!("{}%", percent as u32))
                    .color(theme.text_secondary)
                    .size(11.0),
            );
        }

        // Right-aligned controls
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Spinner when agent is active
            match &state.agent_status {
                AgentStatus::Idle => {}
                AgentStatus::Thinking | AgentStatus::ToolCall(_) => {
                    ui.spinner();
                }
            }

            ui.add_space(4.0);

            // New Session button
            let btn = egui::Button::new(
                RichText::new("+ New Session")
                    .color(theme.accent)
                    .size(11.0),
            )
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, theme.accent))
            .corner_radius(CornerRadius::same(4));

            if ui.add(btn).clicked() {
                state.new_session_requested = true;
            }
        });
    });
}

/// Renders the context warning banner with action buttons.
fn render_context_warning(ui: &mut egui::Ui, state: &mut GuiState, theme: &LairesTheme) {
    let percent = state.context_window_percent;
    let warning_bg = if percent >= 85.0 {
        Color32::from_rgb(0xFD, 0xF0, 0xF0) // red tint
    } else {
        Color32::from_rgb(0xFF, 0xF9, 0xDB) // yellow tint
    };

    egui::Frame::NONE
        .fill(warning_bg)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let icon = if percent >= 85.0 { "!" } else { "i" };
                let label = if percent >= 85.0 {
                    "Context nearly full"
                } else {
                    "Context over 50%"
                };
                ui.label(
                    RichText::new(format!("{icon} {label}"))
                        .color(theme.text_primary)
                        .size(11.0),
                );

                ui.add_space(8.0);

                // Action buttons
                if ui
                    .small_button(RichText::new("New Session").size(10.0))
                    .clicked()
                {
                    state.new_session_requested = true;
                }

                ui.label(
                    RichText::new("\u{00B7}")
                        .color(theme.text_secondary)
                        .size(10.0),
                );

                if ui
                    .small_button(RichText::new("Compact").size(10.0))
                    .clicked()
                {
                    state.compact_context_requested = true;
                }
            });
        });
}

/// Renders a single chat message with avatar and bubble.
fn render_message(
    ui: &mut egui::Ui,
    msg_idx: usize,
    msg: &crate::gui::state::ChatMessage,
    theme: &LairesTheme,
) {
    let is_user = msg.role == ChatRole::User;
    let is_error = msg.role == ChatRole::Error;
    let is_system = msg.role == ChatRole::System;

    // System messages: simple banner
    if is_system {
        ui.add_space(4.0);
        egui::Frame::NONE
            .fill(theme.bg_panel)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(12, 6))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(&msg.content)
                        .color(theme.text_secondary)
                        .size(12.0)
                        .italics(),
                );
            });
        ui.add_space(4.0);
        return;
    }

    // Error messages: red-tinted banner
    if is_error {
        ui.add_space(4.0);
        egui::Frame::NONE
            .fill(Color32::from_rgb(0xFD, 0xF0, 0xF0))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(&msg.content)
                        .color(theme.error_msg_color)
                        .size(13.0),
                );
            });
        ui.add_space(4.0);
        return;
    }

    ui.add_space(6.0);

    if is_user {
        // User message: right-aligned bubble
        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
            // Avatar
            render_avatar(ui, "Y", theme.accent, theme);

            // Bubble
            egui::Frame::NONE
                .fill(theme.accent_light)
                .corner_radius(CornerRadius {
                    nw: BUBBLE_RADIUS,
                    ne: BUBBLE_RADIUS,
                    sw: BUBBLE_RADIUS,
                    se: BUBBLE_RADIUS_TAIL,
                })
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_max_width((ui.available_width() - 8.0).max(0.0));
                    ui.label(
                        RichText::new(&msg.content)
                            .color(theme.text_primary)
                            .size(13.0),
                    );
                });
        });
    } else {
        // Assistant message: left-aligned bubble
        ui.horizontal(|ui| {
            // Avatar
            render_avatar(ui, "L", theme.accent, theme);

            ui.add_space(4.0);

            // Bubble
            egui::Frame::NONE
                .fill(Color32::WHITE)
                .stroke(Stroke::new(1.0, theme.border))
                .corner_radius(CornerRadius {
                    nw: BUBBLE_RADIUS_TAIL,
                    ne: BUBBLE_RADIUS,
                    sw: BUBBLE_RADIUS,
                    se: BUBBLE_RADIUS,
                })
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_max_width((ui.available_width() - 8.0).max(0.0));
                    ui.label(
                        RichText::new(&msg.content)
                            .color(theme.text_primary)
                            .size(13.0),
                    );
                });
        });
    }

    // Tool calls as styled pills
    for (tc_idx, tc) in msg.tool_calls.iter().enumerate() {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.add_space(AVATAR_SIZE + 8.0); // indent to align with bubble

            egui::CollapsingHeader::new(
                RichText::new(format!("\u{2699} {}", tc.name))
                    .color(theme.tool_msg_color)
                    .size(11.0)
                    .monospace(),
            )
            .id_salt(format!("tc_{msg_idx}_{tc_idx}"))
            .default_open(false)
            .show(ui, |ui| {
                egui::Frame::NONE
                    .fill(theme.bg_panel)
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(&tc.args_summary)
                                .monospace()
                                .size(11.0)
                                .color(theme.text_secondary),
                        );
                        if !tc.result_summary.is_empty() {
                            ui.add_space(4.0);
                            let sep_rect = egui::Rect::from_min_size(
                                ui.available_rect_before_wrap().min,
                                Vec2::new(ui.available_width(), 1.0),
                            );
                            ui.painter().rect_filled(sep_rect, 0.0, theme.border);
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(&tc.result_summary)
                                    .monospace()
                                    .size(11.0)
                                    .color(theme.text_secondary),
                            );
                        }
                    });
            });
        });
    }

    ui.add_space(2.0);
}

/// Renders a small circular avatar with an initial letter.
fn render_avatar(ui: &mut egui::Ui, letter: &str, color: Color32, _theme: &LairesTheme) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(AVATAR_SIZE), egui::Sense::hover());
    let painter = ui.painter();
    painter.circle_filled(rect.center(), AVATAR_SIZE / 2.0, color);
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        egui::FontId::proportional(13.0),
        Color32::WHITE,
    );
}

/// Renders a typing/thinking indicator bubble.
fn render_typing_indicator(ui: &mut egui::Ui, label: &str, theme: &LairesTheme) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        render_avatar(ui, "L", theme.accent, theme);
        ui.add_space(4.0);
        egui::Frame::NONE
            .fill(Color32::WHITE)
            .stroke(Stroke::new(1.0, theme.border))
            .corner_radius(CornerRadius::same(BUBBLE_RADIUS))
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new(label)
                            .color(theme.text_secondary)
                            .size(12.0)
                            .italics(),
                    );
                });
            });
    });
}

/// Renders the input area with styled text field and send button.
fn render_input(ui: &mut egui::Ui, state: &mut GuiState, theme: &LairesTheme) -> bool {
    let mut should_send = false;

    egui::Frame::NONE
        .fill(theme.bg_secondary)
        .stroke(Stroke::new(1.0, theme.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let response = ui.add_sized(
                    [(ui.available_width() - 52.0).max(0.0), INPUT_HEIGHT],
                    egui::TextEdit::multiline(&mut state.chat_input)
                        .id_salt("chat_input")
                        .frame(false)
                        .hint_text("Ask Laires about your story...")
                        .desired_rows(3)
                        .margin(egui::Margin::symmetric(4, 6)),
                );

                let can_send = !state.chat_input.trim().is_empty()
                    && matches!(state.agent_status, AgentStatus::Idle);

                // Send button — accent circle
                let btn_size = Vec2::splat(40.0);
                let btn_fill = if can_send {
                    theme.accent
                } else {
                    theme.bg_input
                };
                let btn_text_color = if can_send {
                    Color32::WHITE
                } else {
                    theme.text_secondary
                };
                let send_btn = egui::Button::new(
                    RichText::new("\u{2191}") // up arrow
                        .color(btn_text_color)
                        .size(18.0)
                        .strong(),
                )
                .fill(btn_fill)
                .corner_radius(CornerRadius::same(20))
                .min_size(btn_size);

                if ui.add_enabled(can_send, send_btn).clicked() {
                    should_send = true;
                }

                // Enter to send (without Shift held)
                if response.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift)
                    && can_send
                {
                    should_send = true;
                }
            });
        });

    should_send
}
