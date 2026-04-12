# FountainMD Preview Plan

**Status:** Draft
**Depends on:** `docs/fountainmd-spec.md` (v0.1.0)

---

## Goal

Add a **Markdown / Preview** toggle to the canvas panel. In Markdown mode the user sees raw text (editable in Workshop, read-only in Consultant). In Preview mode the user sees a rendered view that formats Fountain-specific elements with screenplay styling and everything else as prose via CommonMark — always read-only.

The rendering model is simple:

> **Fountain elements get screenplay formatting. Everything else is prose.**

No region detection. No heat model. No ambiguity. The parser pattern-matches Fountain elements; anything that doesn't match falls through to prose rendered as Markdown.

---

## Design Decisions (from discussion)

1. **Plain text defaults to prose.** Action blocks and regular paragraphs look the same — serif font, full-width. Only explicitly Fountain elements (scene headings, character cues, dialogue, etc.) get screenplay formatting.

2. **FountainMD spec adopted.** Three overrides from standard Fountain:
   - `_text_` = italic (not underline); `__text__` = underline
   - `#` headers render visibly (not invisible section markers)
   - `= text` is a writer annotation (synopsis)

3. **SceneMap untouched.** The new FountainMD parser lives alongside SceneMap. SceneMap continues doing scene-boundary detection for the graph/analysis pipeline. The FountainMD parser only serves the Preview renderer.

4. **File extension determines behavior:**
   | Extension | Parser | Markdown mode | Preview mode |
   |-----------|--------|---------------|--------------|
   | `.fountain` | FountainMD | Raw text | Screenplay + Markdown |
   | `.md` | FountainMD | Raw text | Rendered Markdown |
   | `.txt` | None | Raw text | No toggle shown |
   | `.docx` | None | Raw text (extracted) | No toggle shown |

   For `.md` files, the FountainMD parser simply produces no screenplay elements (unless the writer actually uses `INT.`/`EXT.`), so it "just works" for pure prose.

---

## Architecture

### New module: `concepts/fountainmd.rs`

A deterministic, line-by-line parser that takes raw text and produces `Vec<FountainMdElement>`.

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum FountainMdElement {
    // Fountain-specific (get screenplay formatting)
    SceneHeading(String),
    Character(String),               // includes (V.O.), (CONT'D) etc.
    Dialogue(String),
    Parenthetical(String),
    Transition(String),
    Synopsis(String),                // = text
    Lyric(String),                   // ~text
    PageBreak,                       // ===
    DualDialogue {                   // ^
        left: Vec<FountainMdElement>,
        right: Vec<FountainMdElement>,
    },
    TitlePage(Vec<(String, String)>),

    // Markdown (get prose formatting — delegate to CommonMark)
    Heading { level: u8, text: String },   // # / ## / ###
    Paragraph(String),                      // plain text block
    BlankLine,
}
```

The parser does NOT attempt full CommonMark parsing of prose blocks. It only classifies block-level elements. Inline formatting (`**bold**`, `*italic*`, `__underline__`) is left as raw markup in the `String` content — the renderer handles inline formatting.

#### Detection rules (applied per-line, in priority order):

1. **Title page** — key-value pairs at document start (before any blank line with non-kv content)
2. **Page break** — `===` on its own line
3. **Synopsis** — line starting with `= ` (space required)
4. **Lyric** — line starting with `~`
5. **Scene heading** — line matching `^(INT\.|EXT\.|INT\./EXT\.|I/E\.|EST\.)` or forced with leading `.`
6. **Transition** — all-caps line ending in `TO:`, or `>` forced transition in screenplay context
7. **Character cue** — all-caps line (2+ chars, not a transition) where the next non-blank line exists and isn't another element. Includes parenthetical extensions like `(V.O.)`.
8. **Dialogue** — lines immediately following a character cue, until blank line
9. **Parenthetical** — `(text)` on its own line within a dialogue block
10. **Heading** — line starting with `# `, `## `, or `### ` (Override 2)
11. **Blank line** — empty or whitespace-only
12. **Paragraph** — everything else (the default fallback)

The parser is stateful only to the extent of tracking "am I inside a dialogue block?" (entered after a character cue, exited on blank line). No other state needed.

### Preview renderer: `gui/panels/canvas_preview.rs`

A new module (or a section within `canvas.rs`) that takes `&[FountainMdElement]` and renders them in an egui `ScrollArea`.

#### Screenplay element styling:

| Element | Font | Alignment | Extra |
|---------|------|-----------|-------|
| Scene heading | Prose font, 14pt, bold, uppercase | Left | Top/bottom spacing, horizontal rule above |
| Character cue | Mono or prose, 13pt, bold | Center | Top spacing |
| Dialogue | Prose, 14pt | Center, narrower max-width (~60%) | — |
| Parenthetical | Prose, 13pt, italic | Center | Muted color |
| Transition | Prose, 13pt, uppercase | Right | — |
| Synopsis | Prose, 13pt, italic | Left | Muted color, left accent border |
| Lyric | Prose, 14pt, italic | Left, indented | — |
| Page break | — | — | Thick horizontal rule |

#### Prose element styling:

| Element | Rendering approach |
|---------|-------------------|
| Heading | Prose font, sized by level (H1=22pt, H2=18pt, H3=15pt), bold |
| Paragraph | Prose font, 16pt (same as current `render_prose`) |
| Blank line | `ui.add_space(6.0)` |

#### Inline formatting:

Within any element's text content, the renderer processes inline markup:
- `**text**` → bold (via `RichText::strong()`)
- `*text*` or `_text_` → italic (via `RichText::italics()`)
- `__text__` → underline (via `RichText::underline()`)
- `***text***` → bold + italic

This is a simple regex pass that splits text into styled `RichText` segments laid out on a single `ui.horizontal_wrapped()`. We do NOT use `egui_commonmark` here — it's overkill for inline-only formatting and would fight with our custom block layout.

### State changes: `gui/state.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasViewMode {
    Markdown,  // Raw text (editable in Workshop)
    Preview,   // Rendered (always read-only)
}
```

Add to `GuiState`:
```rust
pub canvas_view_mode: CanvasViewMode,
```

Default: `CanvasViewMode::Markdown`.

### Canvas integration: `gui/panels/canvas.rs`

The `render()` function gets a new code path:

```
if canvas_view_mode == Preview {
    // Parse display_text → Vec<FountainMdElement>
    // Call canvas_preview::render(ui, &elements, theme)
} else {
    // Existing behavior (render_editable or render_prose)
}
```

The toggle control sits in the breadcrumb bar, right-aligned:

```
PROJECT > FILE > SCENE          [Markdown | Preview]
Scene Title
Estimated reading time...
```

It's a pair of small text buttons styled as a segmented control. When Preview is active in Workshop mode, a subtle `(read-only)` label appears.

The toggle is hidden for `.txt` and `.docx` files (no preview available).

### Snapshot pipeline: `gui/mod.rs`

`ProjectSnapshot` gains:
```rust
/// Whether the selected file supports Preview mode.
pub preview_available: bool,
```

Set based on file extension during `build_snapshot()`. The canvas reads this to decide whether to show the toggle.

The parsed `Vec<FountainMdElement>` is NOT cached in the snapshot — it's computed on each frame from `display_text`. The parser is fast enough for this (line-by-line, no allocations beyond the output vec). If profiling shows otherwise, we add a cache keyed on text hash.

---

## Implementation Phases

### Phase 1: Parser (`concepts/fountainmd.rs`)

- Define `FountainMdElement` enum
- Implement `parse(text: &str) -> Vec<FountainMdElement>`
- Tests covering:
  - Pure Markdown document (headings, paragraphs, blank lines)
  - Pure Fountain document (scene headings, character/dialogue blocks, transitions)
  - Mixed document (the example from the FountainMD spec §6.3)
  - Edge cases: forced scene headings (`.`), forced character (`@`), parentheticals, synopsis, lyrics
  - Inline markup preserved in content strings (not parsed by this layer)

### Phase 2: Preview Renderer (`gui/panels/canvas_preview.rs`)

- Implement `render(ui, elements, theme)` for all element types
- Screenplay layout: centered dialogue, right-aligned transitions, scene heading rules
- Prose layout: sized headings, paragraph spacing
- Inline formatting: bold/italic/underline regex splitter for `RichText` segments

### Phase 3: Toggle + Integration

- Add `CanvasViewMode` to `GuiState`
- Add `preview_available` to `ProjectSnapshot`
- Wire toggle into breadcrumb bar
- Connect parse → render pipeline in `canvas.rs`
- Handle Workshop mode: Preview switches to read-only, Markdown switches back to editable
- Hide toggle for unsupported file types

### Phase 4: Polish

- Verify spacing/typography matches the "manuscript feel" of the current canvas
- Test with real `.fountain` files and mixed FountainMD documents
- Ensure canvas scroll position is preserved when toggling
- Performance check: if parsing per-frame is too slow for large docs, add hash-keyed cache

---

## What This Plan Does NOT Include

- **Editing in Preview mode.** Preview is always read-only. Edit in Markdown mode.
- **Replacing SceneMap.** The FountainMD parser is for rendering only. SceneMap stays as-is.
- **Full CommonMark block parsing.** We don't parse nested lists, code blocks, tables, etc. in Preview. Paragraphs render as prose text. If a writer uses complex Markdown, they see it formatted in Markdown mode (plain text) and slightly degraded in Preview (treated as a paragraph). This is acceptable for v1.
- **LLM-based parsing.** The spec's §5 inference rules are for the agent during analysis. The Preview renderer uses deterministic rules only.
- **`>` blockquote/transition ambiguity.** For v1, `>` in Preview always renders as a blockquote unless it matches the transition pattern (uppercase ending in `TO:`). Good enough.

---

## Open Items

- **Keyboard shortcut for toggle?** e.g., `Cmd+Shift+P` for Preview. Not required for v1.
- **Synopsis visibility toggle?** The spec says synopses are "hidden in final output." In Preview, should they be visible (as muted annotations) or hidden? Suggest: visible in Preview with a subtle style, since this is a drafting tool, not a final-output renderer.
- **Dual dialogue rendering.** Side-by-side layout in egui is doable but fiddly. Could defer to v2 and render as sequential dialogue blocks with a `(dual)` label.
