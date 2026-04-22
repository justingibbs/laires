//! Preview renderer for the canvas panel.
//!
//! Takes parsed `FountainMdElement`s and renders them with:
//! - Screenplay formatting for Fountain-specific elements
//! - Prose formatting (serif font, full-width) for everything else

use std::sync::LazyLock;

use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Vec2};
use regex::Regex;

use crate::concepts::fountainmd::FountainMdElement;
use crate::gui::theme::{LairesTheme, PROSE_FONT};

/// Font for story prose text.
fn prose_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(PROSE_FONT.into()))
}

/// Font for screenplay elements that benefit from monospace feel.
fn mono_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Render a list of FountainMD elements in Preview mode.
pub fn render(ui: &mut egui::Ui, elements: &[FountainMdElement], theme: &LairesTheme) {
    let prev_spacing = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 2.0;

    for element in elements {
        match element {
            FountainMdElement::SceneHeading(text) => {
                render_scene_heading(ui, text, theme);
            }
            FountainMdElement::Character(text) | FountainMdElement::ForcedCharacter(text) => {
                render_character(ui, text, theme);
            }
            FountainMdElement::Dialogue(text) => {
                render_dialogue(ui, text, theme);
            }
            FountainMdElement::Parenthetical(text) => {
                render_parenthetical(ui, text, theme);
            }
            FountainMdElement::Transition(text) => {
                render_transition(ui, text, theme);
            }
            FountainMdElement::Synopsis(text) => {
                render_synopsis(ui, text, theme);
            }
            FountainMdElement::Lyric(text) => {
                render_lyric(ui, text, theme);
            }
            FountainMdElement::PageBreak => {
                render_page_break(ui, theme);
            }
            FountainMdElement::TitlePage(pairs) => {
                render_title_page(ui, pairs, theme);
            }
            FountainMdElement::Heading { level, text } => {
                render_heading(ui, *level, text, theme);
            }
            FountainMdElement::Paragraph(text) => {
                render_paragraph(ui, text, theme);
            }
            FountainMdElement::BlankLine => {
                ui.add_space(6.0);
            }
        }
    }

    ui.spacing_mut().item_spacing.y = prev_spacing;
}

// ---------------------------------------------------------------------------
// Screenplay element renderers
// ---------------------------------------------------------------------------

fn render_scene_heading(ui: &mut egui::Ui, text: &str, theme: &LairesTheme) {
    ui.add_space(16.0);

    // Horizontal rule above
    let rect = ui.available_rect_before_wrap();
    let rule_rect = egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), 1.0));
    ui.painter().rect_filled(rule_rect, 0.0, theme.border);
    ui.add_space(8.0);

    ui.label(
        RichText::new(text.to_uppercase())
            .font(mono_font(14.0))
            .color(theme.text_primary)
            .strong(),
    );

    ui.add_space(8.0);
}

fn render_character(ui: &mut egui::Ui, text: &str, theme: &LairesTheme) {
    ui.add_space(12.0);

    // Center the character name
    let avail = ui.available_width();
    ui.horizontal(|ui| {
        let text_width_est = text.len() as f32 * 8.0; // rough estimate
        let pad = ((avail - text_width_est) / 2.0).max(0.0);
        ui.add_space(pad);
        ui.label(
            RichText::new(text)
                .font(mono_font(13.0))
                .color(theme.text_primary)
                .strong(),
        );
    });
}

fn render_dialogue(ui: &mut egui::Ui, text: &str, theme: &LairesTheme) {
    // Dialogue: centered, narrower width (~60%)
    let avail = ui.available_width();
    let dialogue_width = (avail * 0.6).max(200.0);
    let pad = ((avail - dialogue_width) / 2.0).max(0.0);

    ui.horizontal(|ui| {
        ui.add_space(pad);
        ui.vertical(|ui| {
            ui.set_max_width(dialogue_width);
            render_inline_text(ui, text, prose_font(14.0), theme.prose_color);
        });
    });
}

fn render_parenthetical(ui: &mut egui::Ui, text: &str, theme: &LairesTheme) {
    // Centered, italic, muted
    let avail = ui.available_width();
    let text_width_est = text.len() as f32 * 7.0;
    let pad = ((avail - text_width_est) / 2.0).max(0.0);

    ui.horizontal(|ui| {
        ui.add_space(pad);
        ui.label(
            RichText::new(text)
                .font(prose_font(13.0))
                .color(theme.text_secondary)
                .italics(),
        );
    });
}

fn render_transition(ui: &mut egui::Ui, text: &str, theme: &LairesTheme) {
    ui.add_space(8.0);

    // Right-aligned
    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
        ui.label(
            RichText::new(text.to_uppercase())
                .font(mono_font(13.0))
                .color(theme.text_primary),
        );
    });

    ui.add_space(4.0);
}

fn render_synopsis(ui: &mut egui::Ui, text: &str, theme: &LairesTheme) {
    ui.add_space(4.0);

    // Left accent border + italic + muted
    let rect = ui.available_rect_before_wrap();
    let border_rect = egui::Rect::from_min_size(rect.min, Vec2::new(3.0, 18.0));
    ui.painter()
        .rect_filled(border_rect, 0.0, theme.accent_secondary);

    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(
            RichText::new(text)
                .font(prose_font(13.0))
                .color(theme.text_secondary)
                .italics(),
        );
    });

    ui.add_space(4.0);
}

fn render_lyric(ui: &mut egui::Ui, text: &str, theme: &LairesTheme) {
    ui.horizontal(|ui| {
        ui.add_space(32.0);
        ui.label(
            RichText::new(text)
                .font(prose_font(14.0))
                .color(theme.prose_color)
                .italics(),
        );
    });
}

fn render_page_break(ui: &mut egui::Ui, theme: &LairesTheme) {
    ui.add_space(16.0);
    let rect = ui.available_rect_before_wrap();
    let rule_rect = egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), 3.0));
    ui.painter()
        .rect_filled(rule_rect, 0.0, theme.text_secondary);
    ui.add_space(16.0);
    // Allocate the space so layout advances past the rule
    ui.allocate_space(Vec2::new(rect.width(), 3.0));
}

fn render_title_page(ui: &mut egui::Ui, pairs: &[(String, String)], theme: &LairesTheme) {
    ui.add_space(24.0);

    for (key, value) in pairs {
        if key.to_lowercase() == "title" {
            // Title gets large heading treatment
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(value)
                        .font(prose_font(28.0))
                        .color(theme.text_primary)
                        .strong(),
                );
            });
        } else {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{}:", key))
                        .font(prose_font(13.0))
                        .color(theme.text_secondary),
                );
                ui.label(
                    RichText::new(value)
                        .font(prose_font(13.0))
                        .color(theme.text_primary),
                );
            });
        }
    }

    ui.add_space(24.0);

    // Divider after title page
    let rect = ui.available_rect_before_wrap();
    let rule_rect = egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), 1.0));
    ui.painter().rect_filled(rule_rect, 0.0, theme.border);
    ui.add_space(16.0);
}

// ---------------------------------------------------------------------------
// Prose / Markdown element renderers
// ---------------------------------------------------------------------------

fn render_heading(ui: &mut egui::Ui, level: u8, text: &str, theme: &LairesTheme) {
    let size = match level {
        1 => 22.0,
        2 => 18.0,
        _ => 15.0,
    };

    ui.add_space(if level == 1 { 16.0 } else { 12.0 });
    ui.label(
        RichText::new(text)
            .font(FontId::new(size, FontFamily::Name(PROSE_FONT.into())))
            .color(theme.text_primary)
            .strong(),
    );
    ui.add_space(4.0);
}

fn render_paragraph(ui: &mut egui::Ui, text: &str, theme: &LairesTheme) {
    render_inline_text(ui, text, prose_font(16.0), theme.prose_color);
}

// ---------------------------------------------------------------------------
// Inline formatting
// ---------------------------------------------------------------------------

/// Render text with inline **bold**, *italic*, _italic_, __underline__ markup.
///
/// Splits the text into styled `RichText` segments. This is intentionally
/// simple — it handles the common FountainMD inline cases without pulling
/// in a full CommonMark parser.
fn render_inline_text(ui: &mut egui::Ui, text: &str, base_font: FontId, base_color: Color32) {
    // Fast path: no markup characters at all
    if !text.contains('*') && !text.contains('_') {
        ui.label(RichText::new(text).font(base_font).color(base_color));
        return;
    }

    // Parse inline spans
    let spans = parse_inline_spans(text);

    if spans.len() == 1 && spans[0].style == InlineStyle::Plain {
        ui.label(
            RichText::new(&spans[0].text)
                .font(base_font)
                .color(base_color),
        );
        return;
    }

    // Use horizontal_wrapped for multi-span layout
    ui.horizontal_wrapped(|ui| {
        for span in &spans {
            let mut rt = RichText::new(&span.text)
                .font(base_font.clone())
                .color(base_color);

            match span.style {
                InlineStyle::Bold => {
                    rt = rt.strong();
                }
                InlineStyle::Italic => {
                    rt = rt.italics();
                }
                InlineStyle::BoldItalic => {
                    rt = rt.strong().italics();
                }
                InlineStyle::Underline => {
                    rt = rt.underline();
                }
                InlineStyle::Plain => {}
            }

            ui.label(rt);
        }
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InlineStyle {
    Plain,
    Bold,
    Italic,
    BoldItalic,
    Underline,
}

#[derive(Debug, Clone)]
struct InlineSpan {
    text: String,
    style: InlineStyle,
}

/// Simple inline markup parser.
///
/// Handles: `***bold italic***`, `**bold**`, `*italic*`, `_italic_`, `__underline__`.
/// Does not handle nested/overlapping markup — keeps it simple for v1.
fn parse_inline_spans(text: &str) -> Vec<InlineSpan> {
    // Explicit alternations — Rust regex crate has no backreferences.
    // Order matters: longest delimiters first.
    static RE_INLINE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\*{3}(.+?)\*{3}|__(.+?)__|(\*{2})(.+?)\*{2}|(\*)(.+?)\*|(_)(.+?)_").unwrap()
    });

    let mut spans = Vec::new();
    let mut last_end = 0;

    for caps in RE_INLINE.captures_iter(text) {
        let m = caps.get(0).unwrap();

        // Plain text before this match
        if m.start() > last_end {
            spans.push(InlineSpan {
                text: text[last_end..m.start()].to_string(),
                style: InlineStyle::Plain,
            });
        }

        // Determine which group matched
        if let Some(content) = caps.get(1) {
            // ***bold italic***
            spans.push(InlineSpan {
                text: content.as_str().to_string(),
                style: InlineStyle::BoldItalic,
            });
        } else if let Some(content) = caps.get(2) {
            // __underline__
            spans.push(InlineSpan {
                text: content.as_str().to_string(),
                style: InlineStyle::Underline,
            });
        } else if caps.get(3).is_some() {
            // **bold**
            spans.push(InlineSpan {
                text: caps[4].to_string(),
                style: InlineStyle::Bold,
            });
        } else if caps.get(5).is_some() {
            // *italic*
            spans.push(InlineSpan {
                text: caps[6].to_string(),
                style: InlineStyle::Italic,
            });
        } else if caps.get(7).is_some() {
            // _italic_
            spans.push(InlineSpan {
                text: caps[8].to_string(),
                style: InlineStyle::Italic,
            });
        }

        last_end = m.end();
    }

    // Trailing plain text
    if last_end < text.len() {
        spans.push(InlineSpan {
            text: text[last_end..].to_string(),
            style: InlineStyle::Plain,
        });
    }

    if spans.is_empty() {
        spans.push(InlineSpan {
            text: text.to_string(),
            style: InlineStyle::Plain,
        });
    }

    spans
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_bold() {
        let spans = parse_inline_spans("Hello **world** today");
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].style, InlineStyle::Plain);
        assert_eq!(spans[1].style, InlineStyle::Bold);
        assert_eq!(spans[1].text, "world");
        assert_eq!(spans[2].style, InlineStyle::Plain);
    }

    #[test]
    fn inline_italic_star() {
        let spans = parse_inline_spans("Hello *world* today");
        assert_eq!(spans[1].style, InlineStyle::Italic);
        assert_eq!(spans[1].text, "world");
    }

    #[test]
    fn inline_italic_underscore() {
        let spans = parse_inline_spans("Hello _world_ today");
        assert_eq!(spans[1].style, InlineStyle::Italic);
    }

    #[test]
    fn inline_underline() {
        let spans = parse_inline_spans("This is __important__ text");
        assert_eq!(spans[1].style, InlineStyle::Underline);
        assert_eq!(spans[1].text, "important");
    }

    #[test]
    fn inline_bold_italic() {
        let spans = parse_inline_spans("Hello ***world*** today");
        assert_eq!(spans[1].style, InlineStyle::BoldItalic);
        assert_eq!(spans[1].text, "world");
    }

    #[test]
    fn inline_no_markup() {
        let spans = parse_inline_spans("Plain text here");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style, InlineStyle::Plain);
    }

    #[test]
    fn inline_multiple() {
        let spans = parse_inline_spans("**bold** and *italic* and __underline__");
        let styles: Vec<_> = spans.iter().map(|s| &s.style).collect();
        assert!(styles.contains(&&InlineStyle::Bold));
        assert!(styles.contains(&&InlineStyle::Italic));
        assert!(styles.contains(&&InlineStyle::Underline));
    }
}
