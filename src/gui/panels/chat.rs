use eframe::egui::{self, RichText};

use crate::gui::state::{AgentStatus, ChatRole, GuiState};
use crate::gui::theme::LairesTheme;

/// Renders the chat panel. Returns true if the user submitted a message.
pub fn render(ui: &mut egui::Ui, state: &mut GuiState, theme: &LairesTheme) -> bool {
    let mut should_send = false;
    let input_height = 80.0;
    // Reserve space for separator + input area at the bottom
    let input_reserved = input_height + 16.0;
    let scroll_height = (ui.available_height() - input_reserved).max(100.0);

    // Section header
    ui.label(RichText::new("Chat").color(theme.text_secondary).small());
    ui.add_space(4.0);

    // Message history — fills available height minus input area
    egui::ScrollArea::vertical()
        .id_salt("chat_scroll")
        .max_height(scroll_height)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for (msg_idx, msg) in state.chat_history.iter().enumerate() {
                let (prefix, color) = match msg.role {
                    ChatRole::User => ("> ", theme.user_msg_color),
                    ChatRole::Assistant => ("", theme.assistant_msg_color),
                    ChatRole::System => ("", theme.text_secondary),
                    ChatRole::Error => ("Error: ", theme.error_msg_color),
                };

                ui.horizontal_wrapped(|ui| {
                    if !prefix.is_empty() {
                        ui.label(RichText::new(prefix).color(color).strong());
                    }
                    ui.label(RichText::new(&msg.content).color(color));
                });

                // Tool calls (collapsible)
                for (tc_idx, tc) in msg.tool_calls.iter().enumerate() {
                    egui::CollapsingHeader::new(
                        RichText::new(format!("[{}]", tc.name))
                            .color(theme.tool_msg_color)
                            .small(),
                    )
                    .id_salt(format!("tc_{msg_idx}_{tc_idx}"))
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(&tc.args_summary)
                                .monospace()
                                .small()
                                .color(theme.text_secondary),
                        );
                        if !tc.result_summary.is_empty() {
                            ui.separator();
                            ui.label(
                                RichText::new(&tc.result_summary)
                                    .monospace()
                                    .small()
                                    .color(theme.text_secondary),
                            );
                        }
                    });
                }

                ui.add_space(4.0);
            }

            // Agent status indicator
            match &state.agent_status {
                AgentStatus::Thinking => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new("Thinking...")
                                .color(theme.text_secondary)
                                .italics(),
                        );
                    });
                }
                AgentStatus::ToolCall(name) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new(format!("Calling {name}..."))
                                .color(theme.text_secondary)
                                .italics(),
                        );
                    });
                }
                AgentStatus::Streaming => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new("Streaming...")
                                .color(theme.text_secondary)
                                .italics(),
                        );
                    });
                }
                AgentStatus::Idle => {}
            }
        });

    ui.separator();

    // Input area pinned at the bottom
    ui.horizontal(|ui| {
        let response = ui.add_sized(
            [ui.available_width() - 60.0, input_height],
            egui::TextEdit::multiline(&mut state.chat_input)
                .id_salt("chat_input")
                .hint_text("Ask Laires about your story...")
                .desired_rows(3),
        );

        let can_send = !state.chat_input.trim().is_empty()
            && matches!(state.agent_status, AgentStatus::Idle);

        if ui
            .add_enabled(
                can_send,
                egui::Button::new("Send").min_size(egui::vec2(50.0, input_height)),
            )
            .clicked()
        {
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

    should_send
}
