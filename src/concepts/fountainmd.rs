//! Deterministic FountainMD parser for Preview rendering.
//!
//! Classifies every line of a FountainMD document into block-level elements.
//! Fountain-specific elements get screenplay formatting in Preview; everything
//! else falls through to prose (Markdown) rendering.
//!
//! See `docs/fountainmd-spec.md` for the full specification.

use regex::Regex;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Element types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum FountainMdElement {
    // -- Fountain-specific (screenplay formatting) --
    SceneHeading(String),
    Character(String),
    Dialogue(String),
    Parenthetical(String),
    Transition(String),
    Synopsis(String),
    Lyric(String),
    PageBreak,
    ForcedCharacter(String),
    TitlePage(Vec<(String, String)>),

    // -- Markdown / prose (prose formatting) --
    Heading { level: u8, text: String },
    Paragraph(String),
    BlankLine,
}

// ---------------------------------------------------------------------------
// Compiled regexes (built once)
// ---------------------------------------------------------------------------

static RE_SCENE_HEADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(INT\.|EXT\.|INT\./EXT\.|I/E\.|EST\.)[\t ]+(.+)$").unwrap());

static RE_FORCED_SCENE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\.([A-Za-z].*)$").unwrap());

static RE_TRANSITION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z][A-Z0-9 ]*TO:$").unwrap());

static RE_FORCED_TRANSITION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^>\s*(.+)$").unwrap());

static RE_CHARACTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([A-Z][A-Z0-9 .'\-]{1,})(?:\s*\((?:V\.?O\.?|O\.?S\.?|CONT'?D?|O\.?C\.?)\))?$")
        .unwrap()
});

static RE_FORCED_CHARACTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^@(.+)$").unwrap());

static RE_PARENTHETICAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\(.*\)$").unwrap());

static RE_HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(#{1,3})\s+(.+)$").unwrap());

static RE_SYNOPSIS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^=\s+(.+)$").unwrap());

static RE_LYRIC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^~(.+)$").unwrap());

static RE_TITLE_KV: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Za-z][A-Za-z ]*):(.*)$").unwrap());

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parse a FountainMD document into block-level elements.
///
/// The parser is line-by-line with minimal state: it tracks whether we are
/// inside a dialogue block (entered after a character cue, exited on blank
/// line) and whether we are still in the title page (top of document).
pub fn parse(text: &str) -> Vec<FountainMdElement> {
    let lines: Vec<&str> = text.lines().collect();
    let len = lines.len();
    let mut elements = Vec::new();
    let mut i = parse_yaml_front_matter(&lines, &mut elements);

    // -- Front matter / title page detection at document start --
    // Try YAML front matter first (---fenced), then Fountain-style (bare key-value).
    if i == 0 {
        i = parse_title_page(&lines, &mut elements);
    }

    // -- Main parse loop --
    while i < len {
        let line = lines[i];
        let trimmed = line.trim();

        // Blank line
        if trimmed.is_empty() {
            elements.push(FountainMdElement::BlankLine);
            i += 1;
            continue;
        }

        // Page break: === on its own line
        if trimmed == "===" {
            elements.push(FountainMdElement::PageBreak);
            i += 1;
            continue;
        }

        // Synopsis: = text
        if let Some(caps) = RE_SYNOPSIS.captures(trimmed) {
            elements.push(FountainMdElement::Synopsis(caps[1].to_string()));
            i += 1;
            continue;
        }

        // Lyric: ~text
        if let Some(caps) = RE_LYRIC.captures(trimmed) {
            elements.push(FountainMdElement::Lyric(caps[1].to_string()));
            i += 1;
            continue;
        }

        // Heading: # / ## / ### (Override 2 — always visible)
        if let Some(caps) = RE_HEADING.captures(trimmed) {
            let level = caps[1].len() as u8;
            let text = caps[2].to_string();
            elements.push(FountainMdElement::Heading { level, text });
            i += 1;
            continue;
        }

        // Scene heading: INT./EXT. or forced with leading .
        if RE_SCENE_HEADING.is_match(trimmed) {
            elements.push(FountainMdElement::SceneHeading(trimmed.to_string()));
            i += 1;
            continue;
        }
        if let Some(caps) = RE_FORCED_SCENE.captures(trimmed) {
            elements.push(FountainMdElement::SceneHeading(caps[1].to_string()));
            i += 1;
            continue;
        }

        // Transition: uppercase ending in TO: (must be preceded by blank line)
        if RE_TRANSITION.is_match(trimmed) && preceded_by_blank(&lines, i) {
            elements.push(FountainMdElement::Transition(trimmed.to_string()));
            i += 1;
            continue;
        }

        // Forced transition: > TEXT (check for transition pattern, not blockquote)
        if let Some(caps) = RE_FORCED_TRANSITION.captures(trimmed) {
            let inner = caps[1].trim();
            // If it looks like a transition (uppercase, often ends in TO:), treat as transition
            if inner.chars().all(|c| {
                c.is_ascii_uppercase()
                    || c.is_ascii_whitespace()
                    || c == ':'
                    || c == '.'
                    || c == '-'
            }) && inner.len() > 1
            {
                elements.push(FountainMdElement::Transition(inner.to_string()));
                i += 1;
                continue;
            }
            // Otherwise fall through to paragraph (could be a blockquote)
        }

        // Forced character: @Name
        if let Some(caps) = RE_FORCED_CHARACTER.captures(trimmed) {
            let name = caps[1].trim().to_string();
            elements.push(FountainMdElement::ForcedCharacter(name));
            i += 1;
            // Consume dialogue block
            i = consume_dialogue(&lines, i, &mut elements);
            continue;
        }

        // Character cue: ALL CAPS line followed by dialogue
        // Must be preceded by a blank line and followed by a non-blank line
        if RE_CHARACTER.is_match(trimmed)
            && preceded_by_blank(&lines, i)
            && followed_by_nonblank(&lines, i, len)
        {
            elements.push(FountainMdElement::Character(trimmed.to_string()));
            i += 1;
            // Consume dialogue block (dialogue + parentheticals until blank line)
            i = consume_dialogue(&lines, i, &mut elements);
            continue;
        }

        // Default: paragraph (prose)
        elements.push(FountainMdElement::Paragraph(trimmed.to_string()));
        i += 1;
    }

    elements
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse YAML front matter (`---` fenced block at document start).
/// Returns the line index to continue parsing from (0 if no front matter found).
fn parse_yaml_front_matter(lines: &[&str], elements: &mut Vec<FountainMdElement>) -> usize {
    if lines.is_empty() {
        return 0;
    }

    // Must start with --- on line 0
    if lines[0].trim() != "---" {
        return 0;
    }

    let mut pairs = Vec::new();
    let mut i = 1; // skip opening ---

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // Closing --- ends the front matter
        if trimmed == "---" {
            i += 1; // skip closing ---
            if !pairs.is_empty() {
                elements.push(FountainMdElement::TitlePage(pairs));
            }
            return i;
        }

        // Skip blank lines within front matter
        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        // Parse key: value pairs (simple single-line YAML)
        if let Some(caps) = RE_TITLE_KV.captures(trimmed) {
            let key = caps[1].trim().to_string();
            let mut value = caps[2].trim().to_string();
            // Strip surrounding quotes if present
            if (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''))
            {
                value = value[1..value.len() - 1].to_string();
            }
            pairs.push((key, value));
        }
        // Non-KV lines within front matter are ignored (could be YAML arrays, etc.)

        i += 1;
    }

    // Reached end of document without closing --- : not valid front matter
    0
}

/// Parse title page key-value pairs at the start of the document.
/// Returns the line index to continue parsing from.
fn parse_title_page(lines: &[&str], elements: &mut Vec<FountainMdElement>) -> usize {
    if lines.is_empty() {
        return 0;
    }

    // Title page must start with a key-value pair on line 0
    if !RE_TITLE_KV.is_match(lines[0].trim()) {
        return 0;
    }

    let mut pairs = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        if trimmed.is_empty() {
            // Blank line ends the title page
            i += 1;
            break;
        }

        if let Some(caps) = RE_TITLE_KV.captures(trimmed) {
            let key = caps[1].trim().to_string();
            let value = caps[2].trim().to_string();
            pairs.push((key, value));
        } else {
            // Non-KV line before a blank — not a title page, rewind
            return 0;
        }

        i += 1;
    }

    if !pairs.is_empty() {
        elements.push(FountainMdElement::TitlePage(pairs));
    }
    i
}

/// Consume dialogue lines (and parentheticals) after a character cue.
/// Returns the next line index after the dialogue block ends.
fn consume_dialogue(lines: &[&str], mut i: usize, elements: &mut Vec<FountainMdElement>) -> usize {
    let len = lines.len();
    while i < len {
        let trimmed = lines[i].trim();

        // Blank line ends the dialogue block
        if trimmed.is_empty() {
            break;
        }

        // Parenthetical within dialogue
        if RE_PARENTHETICAL.is_match(trimmed) {
            elements.push(FountainMdElement::Parenthetical(trimmed.to_string()));
        } else {
            elements.push(FountainMdElement::Dialogue(trimmed.to_string()));
        }

        i += 1;
    }
    i
}

/// Check if the line before index `i` is blank (or `i` is 0).
fn preceded_by_blank(lines: &[&str], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    lines[i - 1].trim().is_empty()
}

/// Check if there is a non-blank line after index `i`.
fn followed_by_nonblank(lines: &[&str], i: usize, len: usize) -> bool {
    if i + 1 >= len {
        return false;
    }
    !lines[i + 1].trim().is_empty()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_markdown() {
        let text =
            "# Chapter One\n\nThis is a paragraph.\n\n## Section Two\n\nAnother paragraph here.";
        let elements = parse(text);

        assert_eq!(
            elements,
            vec![
                FountainMdElement::Heading {
                    level: 1,
                    text: "Chapter One".to_string()
                },
                FountainMdElement::BlankLine,
                FountainMdElement::Paragraph("This is a paragraph.".to_string()),
                FountainMdElement::BlankLine,
                FountainMdElement::Heading {
                    level: 2,
                    text: "Section Two".to_string()
                },
                FountainMdElement::BlankLine,
                FountainMdElement::Paragraph("Another paragraph here.".to_string()),
            ]
        );
    }

    #[test]
    fn pure_fountain() {
        let text = "\
INT. COFFEE SHOP - DAY

Alice sits alone. She checks her phone.

BOB
(entering)
You actually came.

ALICE
I almost didn't.";
        let elements = parse(text);

        assert_eq!(
            elements,
            vec![
                FountainMdElement::SceneHeading("INT. COFFEE SHOP - DAY".to_string()),
                FountainMdElement::BlankLine,
                FountainMdElement::Paragraph("Alice sits alone. She checks her phone.".to_string()),
                FountainMdElement::BlankLine,
                FountainMdElement::Character("BOB".to_string()),
                FountainMdElement::Parenthetical("(entering)".to_string()),
                FountainMdElement::Dialogue("You actually came.".to_string()),
                FountainMdElement::BlankLine,
                FountainMdElement::Character("ALICE".to_string()),
                FountainMdElement::Dialogue("I almost didn't.".to_string()),
            ]
        );
    }

    #[test]
    fn mixed_fountainmd() {
        let text = "\
# The Conversation

= This scene is the emotional core of act two.

INT. COFFEE SHOP - DAY

Alice sits alone. She checks her phone. Nothing.

BOB
(entering)
You actually came.

ALICE
I almost didn't.";
        let elements = parse(text);

        // Check key elements are in the right order
        assert!(matches!(
            elements[0],
            FountainMdElement::Heading { level: 1, .. }
        ));
        assert!(matches!(elements[2], FountainMdElement::Synopsis(_)));
        assert!(matches!(elements[4], FountainMdElement::SceneHeading(_)));
        assert!(matches!(elements[6], FountainMdElement::Paragraph(_)));
        // Find the character cues
        let chars: Vec<_> = elements
            .iter()
            .filter(|e| matches!(e, FountainMdElement::Character(_)))
            .collect();
        assert_eq!(chars.len(), 2);
    }

    #[test]
    fn transitions() {
        let text = "\nCUT TO:\n\n> SMASH CUT TO:\n";
        let elements = parse(text);

        let transitions: Vec<_> = elements
            .iter()
            .filter(|e| matches!(e, FountainMdElement::Transition(_)))
            .collect();
        assert_eq!(transitions.len(), 2);
    }

    #[test]
    fn forced_scene_heading() {
        let text = ".FLASHBACK\n\nSome action.";
        let elements = parse(text);
        assert_eq!(
            elements[0],
            FountainMdElement::SceneHeading("FLASHBACK".to_string())
        );
    }

    #[test]
    fn forced_character() {
        let text = "\n@McCOY\nDammit Jim!\n";
        let elements = parse(text);

        let has_forced = elements
            .iter()
            .any(|e| matches!(e, FountainMdElement::ForcedCharacter(_)));
        assert!(has_forced);
        let has_dialogue = elements
            .iter()
            .any(|e| matches!(e, FountainMdElement::Dialogue(d) if d == "Dammit Jim!"));
        assert!(has_dialogue);
    }

    #[test]
    fn lyrics() {
        let text = "~I'm singing in the rain";
        let elements = parse(text);
        assert_eq!(
            elements[0],
            FountainMdElement::Lyric("I'm singing in the rain".to_string())
        );
    }

    #[test]
    fn synopsis() {
        let text = "= This scene establishes the power dynamic.";
        let elements = parse(text);
        assert_eq!(
            elements[0],
            FountainMdElement::Synopsis("This scene establishes the power dynamic.".to_string())
        );
    }

    #[test]
    fn page_break() {
        let text = "Some text.\n\n===\n\nMore text.";
        let elements = parse(text);
        assert!(
            elements
                .iter()
                .any(|e| matches!(e, FountainMdElement::PageBreak))
        );
    }

    #[test]
    fn title_page() {
        let text = "Title: The Long Walk Home\nAuthor: Jane Smith\nDraft date: 2024-01-15\n\nINT. HOUSE - DAY\n";
        let elements = parse(text);
        assert!(matches!(&elements[0], FountainMdElement::TitlePage(pairs) if pairs.len() == 3));
    }

    #[test]
    fn yaml_front_matter() {
        let text =
            "---\ntitle: \"test-new-2\"\nauthor: Jane Smith\n---\n\n# Chapter One\n\nSome text.";
        let elements = parse(text);
        // First element should be TitlePage with 2 pairs
        assert!(
            matches!(&elements[0], FountainMdElement::TitlePage(pairs) if pairs.len() == 2),
            "Expected TitlePage with 2 pairs, got {:?}",
            &elements[0]
        );
        // Quotes should be stripped from values
        if let FountainMdElement::TitlePage(pairs) = &elements[0] {
            assert_eq!(pairs[0].0, "title");
            assert_eq!(pairs[0].1, "test-new-2"); // quotes stripped
            assert_eq!(pairs[1].0, "author");
            assert_eq!(pairs[1].1, "Jane Smith");
        }
        // Content after front matter should parse normally
        let has_heading = elements
            .iter()
            .any(|e| matches!(e, FountainMdElement::Heading { level: 1, .. }));
        assert!(has_heading, "Expected heading after front matter");
    }

    #[test]
    fn yaml_front_matter_no_closing_fence() {
        // No closing --- means it's not valid front matter — parse as regular content
        let text = "---\ntitle: test\nSome paragraph.";
        let elements = parse(text);
        // Should NOT produce a TitlePage
        let has_title = elements
            .iter()
            .any(|e| matches!(e, FountainMdElement::TitlePage(_)));
        assert!(
            !has_title,
            "Should not detect title page without closing ---"
        );
    }

    #[test]
    fn inline_markup_preserved() {
        let text = "This has **bold** and *italic* and __underline__ text.";
        let elements = parse(text);
        assert_eq!(
            elements[0],
            FountainMdElement::Paragraph(
                "This has **bold** and *italic* and __underline__ text.".to_string()
            )
        );
    }

    #[test]
    fn character_not_detected_without_dialogue() {
        // ALL CAPS line followed by blank should NOT be a character cue
        let text = "\nWARNING\n\nSomething else.";
        let elements = parse(text);
        let has_char = elements
            .iter()
            .any(|e| matches!(e, FountainMdElement::Character(_)));
        assert!(
            !has_char,
            "Should not detect character cue without following dialogue"
        );
    }

    #[test]
    fn parenthetical_within_dialogue() {
        let text = "\nBOB\n(whispering)\nI know the truth.\n(beat)\nOr do I?\n";
        let elements = parse(text);

        let parens: Vec<_> = elements
            .iter()
            .filter(|e| matches!(e, FountainMdElement::Parenthetical(_)))
            .collect();
        assert_eq!(parens.len(), 2);

        let dialogues: Vec<_> = elements
            .iter()
            .filter(|e| matches!(e, FountainMdElement::Dialogue(_)))
            .collect();
        assert_eq!(dialogues.len(), 2);
    }
}
