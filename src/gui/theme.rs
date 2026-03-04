use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Style, Visuals};

pub struct LairesTheme {
    // Backgrounds
    pub bg_primary: Color32,
    pub bg_secondary: Color32,
    pub bg_panel: Color32,
    pub bg_input: Color32,

    // Text
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_accent: Color32,

    // Prose display
    pub prose_color: Color32,
    pub scene_boundary_color: Color32,
    pub line_number_color: Color32,

    // Graph node colors
    pub character_color: Color32,
    pub objective_color: Color32,
    pub scene_color: Color32,
    pub conflict_color: Color32,

    // Chat roles
    pub user_msg_color: Color32,
    pub assistant_msg_color: Color32,
    pub tool_msg_color: Color32,
    pub error_msg_color: Color32,

    // Accents
    pub accent: Color32,
    pub accent_secondary: Color32,
    pub border: Color32,
    pub separator: Color32,
}

impl Default for LairesTheme {
    fn default() -> Self {
        Self {
            // Backgrounds
            bg_primary: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            bg_secondary: Color32::from_rgb(0xF8, 0xF9, 0xFA),
            bg_panel: Color32::from_rgb(0xF1, 0xF3, 0xF5),
            bg_input: Color32::from_rgb(0xE9, 0xEC, 0xEF),

            // Text
            text_primary: Color32::from_rgb(0x21, 0x25, 0x29),
            text_secondary: Color32::from_rgb(0x86, 0x8E, 0x96),
            text_accent: Color32::from_rgb(0x36, 0x4F, 0xC7),

            // Prose
            prose_color: Color32::from_rgb(0x34, 0x3A, 0x40),
            scene_boundary_color: Color32::from_rgb(0xC9, 0x2A, 0x2A),
            line_number_color: Color32::from_rgb(0xCE, 0xD4, 0xDA),

            // Graph nodes
            character_color: Color32::from_rgb(0x36, 0x4F, 0xC7),
            objective_color: Color32::from_rgb(0x2B, 0x8A, 0x3E),
            scene_color: Color32::from_rgb(0xE6, 0x77, 0x00),
            conflict_color: Color32::from_rgb(0xC9, 0x2A, 0x2A),

            // Chat
            user_msg_color: Color32::from_rgb(0x2B, 0x8A, 0x3E),
            assistant_msg_color: Color32::from_rgb(0x21, 0x25, 0x29),
            tool_msg_color: Color32::from_rgb(0x86, 0x8E, 0x96),
            error_msg_color: Color32::from_rgb(0xC9, 0x2A, 0x2A),

            // Accents
            accent: Color32::from_rgb(0x36, 0x4F, 0xC7),
            accent_secondary: Color32::from_rgb(0x70, 0x48, 0xE8),
            border: Color32::from_rgb(0xDE, 0xE2, 0xE6),
            separator: Color32::from_rgb(0xDE, 0xE2, 0xE6),
        }
    }
}

impl LairesTheme {
    pub fn apply(&self, ctx: &egui::Context) {
        let mut visuals = Visuals::light();

        visuals.panel_fill = self.bg_secondary;
        visuals.window_fill = self.bg_primary;
        visuals.extreme_bg_color = self.bg_input;
        visuals.faint_bg_color = self.bg_panel;

        visuals.override_text_color = Some(self.text_primary);

        visuals.widgets.noninteractive.bg_fill = self.bg_secondary;
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.text_primary);
        visuals.widgets.noninteractive.corner_radius = CornerRadius::same(4);

        visuals.widgets.inactive.bg_fill = self.bg_input;
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, self.text_primary);

        visuals.widgets.hovered.bg_fill =
            Color32::from_rgba_premultiplied(0x36, 0x4F, 0xC7, 25);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, self.accent);

        visuals.widgets.active.bg_fill = self.accent;
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);

        visuals.selection.bg_fill = Color32::from_rgba_premultiplied(0x36, 0x4F, 0xC7, 40);
        visuals.selection.stroke = Stroke::new(1.0, self.accent);

        visuals.window_corner_radius = CornerRadius::same(8);

        let mut style = Style::default();
        style.visuals = visuals;

        // Tighter spacing for a polished feel
        style.spacing.item_spacing = egui::vec2(8.0, 4.0);
        style.spacing.window_margin = egui::Margin::same(12);

        ctx.set_style(style);
    }

    pub fn configure_fonts(ctx: &egui::Context) {
        let mut fonts = FontDefinitions::default();

        // Embed Source Serif 4 for proportional (prose) text
        fonts.font_data.insert(
            "source_serif".to_owned(),
            std::sync::Arc::new(FontData::from_static(include_bytes!(
                "../../assets/fonts/SourceSerif4-Regular.otf"
            ))),
        );

        // Embed JetBrains Mono for monospace text
        fonts.font_data.insert(
            "jetbrains_mono".to_owned(),
            std::sync::Arc::new(FontData::from_static(include_bytes!(
                "../../assets/fonts/JetBrainsMono-Regular.ttf"
            ))),
        );

        // Set Source Serif as primary proportional font (fallback to default)
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "source_serif".to_owned());

        // Set JetBrains Mono as primary monospace font (fallback to default)
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "jetbrains_mono".to_owned());

        ctx.set_fonts(fonts);
    }
}
