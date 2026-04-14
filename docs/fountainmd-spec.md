# FountainMD Specification
**Version:** 0.1.0
**Status:** Draft
**Based on:** Fountain (https://fountain.io) + CommonMark Markdown

---

## 1. Overview

FountainMD is a plain-text markup format for documents that mix narrative prose and screenplay content. It is a superset of Fountain with three deliberate modifications to resolve conflicts with CommonMark Markdown. The goal is to let writers type naturally — using Markdown conventions for prose sections and Fountain conventions for screenplay sections — without mode-switching syntax or fences.

**LLM agents parsing FountainMD must apply this spec in full.** Where FountainMD rules differ from standard Fountain or Markdown, this spec takes precedence. The sections below that describe overrides explicitly call out the default behavior being replaced.

---

## 2. Design Philosophy

- **Infer, don't require.** Writers should not need to declare what "mode" they are in. Context determines element type.
- **Minimal syntax delta.** FountainMD changes as little as possible from Fountain and Markdown. Only three rules differ from standard Fountain.
- **LLM-first parsing.** This format is designed to be parsed by a language model, not a regex engine. Ambiguity is resolved by context, not by strict pattern enforcement.
- **Single source of truth.** This document is both the human-readable spec and the LLM system prompt context. Keep it concise.

---

## 3. FountainMD Rule Overrides

These three rules differ from standard Fountain. All other Fountain rules apply unchanged.

### Override 1: Underline Syntax

| Format | Standard Fountain | **FountainMD** |
|--------|------------------|----------------|
| Underline | `_text_` | `__text__` (double underscore) |
| Italic | `*text*` | `*text*` (unchanged) |
| Single `_text_` | Underline | **Italic** (Markdown-compatible) |

**Rationale:** Aligns emphasis syntax with Markdown conventions. Single delimiter = lighter emphasis, double delimiter = stronger. Writers coming from Markdown will find `_text_` producing italics as expected.

**Note for LLM:** If you encounter `_text_` in screenplay content, treat it as italic, not underline. If you encounter `__text__`, treat it as underline regardless of context.

### Override 2: Headers are Markdown Headers

| Format | Standard Fountain | **FountainMD** |
|--------|------------------|----------------|
| `# Text` | Section marker (invisible in output) | **Rendered H1 header** |
| `## Text` | Sub-section marker (invisible) | **Rendered H2 header** |
| `### Text` | Sub-sub-section (invisible) | **Rendered H3 header** |

**Rationale:** Writers using this format expect `#` headers to render visibly, as in any Markdown document. Fountain's invisible section behavior is replaced entirely.

**Note for LLM:** Headers render as styled document headers in both prose and screenplay regions. They are visible in output. There are no invisible structural section markers in FountainMD.

### Override 3: Synopsis as Writer's Annotation

The `=` prefix retains its Fountain meaning — a synopsis or writer's note — but is clarified as follows:

- `= text` on its own line is a **writer annotation**: visible in draft/edit views, hidden in final formatted output.
- Writers may use it anywhere: before a scene heading, after a header, mid-screenplay.
- It is the replacement for Fountain's section-based structural notes, now that `#` headers are visual.

**Example:**
```
= This scene establishes the power dynamic between Alice and Bob.

INT. CONFERENCE ROOM - DAY
```

---

## 4. Element Reference

### 4.1 Markdown Elements (prose regions)

These elements behave exactly as CommonMark Markdown.

| Element | Syntax | Notes |
|---------|--------|-------|
| H1 Header | `# Header Text` | Rendered visually |
| H2 Header | `## Header Text` | Rendered visually |
| H3 Header | `### Header Text` | Rendered visually |
| Bold | `**text**` | |
| Italic | `*text*` or `_text_` | Both produce italic |
| Bold Italic | `***text***` | |
| Underline | `__text__` | FountainMD extension |
| Blockquote | `> text` | Prose only — see note below |
| Unordered list | `- item` or `* item` | |
| Ordered list | `1. item` | |
| Inline code | `` `code` `` | |
| Code block | ```` ```lang ```` | |
| Horizontal rule | `---` | |
| Link | `[text](url)` | |
| Image | `![alt](url)` | |

**Note on `>`:** In prose context, `>` is a Markdown blockquote. In screenplay context, `> TEXT` is a Fountain forced transition (e.g., `> SMASH CUT TO:`). Context determines which applies. If the line following `>` is uppercase and ends in `TO:` or similar transition language, treat as Fountain transition. Otherwise treat as blockquote.

### 4.2 Fountain Elements (screenplay regions)

These elements behave as standard Fountain unless overridden above.

| Element | Syntax / Detection | Notes |
|---------|-------------------|-------|
| Scene Heading | Line starting with `INT.`, `EXT.`, `INT./EXT.`, `EST.` | Auto-detected |
| Forced Scene Heading | `.Text` (leading period) | Forces any line to scene heading |
| Action | Any line not matching another element | Default fallback |
| Character Cue | ALL CAPS line, alone, followed by dialogue | See inference rules §5 |
| Forced Character | `@Name` | Forces mixed-case name as character cue |
| Dialogue | Line immediately following a character cue | |
| Parenthetical | `(text)` on its own line within dialogue block | |
| Transition | Uppercase line ending in `TO:` | e.g., `CUT TO:`, `FADE TO:` |
| Forced Transition | `> TEXT` in screenplay context | See blockquote note above |
| Lyrics | `~text` | Line of sung lyric |
| Synopsis / Note | `= text` | Writer annotation, hidden in final output |
| Dual Dialogue | `^` after second character cue | Side-by-side dialogue |
| Page Break | `===` | Forced page break |
| Boneyard | `/* text */` | Ignored completely in all output |
| Title Page | Key-value pairs at very top of document | e.g., `Title:`, `Author:` |
| Underline | `__text__` | FountainMD syntax (not `_text_`) |
| Italic | `*text*` or `_text_` | Both work in screenplay context |
| Bold | `**text**` | |
| Bold Italic | `***text***` | |
| Escape | `\*`, `\_`, etc. | Suppress formatting character |

---

## 5. Inference Rules for LLM Agents

Because FountainMD is parsed by a language model, ambiguous constructs are resolved by context. Apply these rules in order.

### 5.1 Detecting Region Type (Prose vs. Screenplay)

There is no explicit mode declaration. Determine the current region type by looking at surrounding content:

- **Screenplay indicators:** Presence of scene headings (`INT.`/`EXT.`), ALL CAPS character cues, parentheticals, or transitions nearby.
- **Prose indicators:** Continuous paragraphs, Markdown lists, headers followed by flowing text, no screenplay-specific elements nearby.
- **Mixed documents** are normal. A document may open with Markdown prose (a title, an intro, notes), then transition into screenplay content, then return to prose for appendices or commentary. Each region is parsed according to its local context.
- **When in doubt,** treat the content as prose/action. Fountain's own rule: "When in doubt, return text as Action."

### 5.2 ALL CAPS Resolution

ALL CAPS text is a common point of ambiguity.

| Context | Treatment |
|---------|-----------|
| Alone on a line, followed by a line of dialogue or parenthetical | **Character cue** |
| Alone on a line, followed by a blank line | **Transition** if it ends in `TO:`, otherwise **Action** |
| Mid-sentence or mid-action line | **Emphasis only** — not a character or transition |
| At top of document with no screenplay context | **Prose** — likely an acronym or styled heading |

### 5.3 `#` Header vs. Former Fountain Section

Standard Fountain uses `#` as an invisible section marker. **FountainMD does not.** All `#` lines render as visible headers. If a writer intends a structural note, they use `= text` instead.

### 5.4 `>` Blockquote vs. Transition

- In a **prose region:** `> text` = Markdown blockquote.
- In a **screenplay region:** `> TEXT` = forced Fountain transition.
- If `>` is followed by uppercase text that reads as a transition (`SMASH CUT TO:`, `FADE OUT.`), treat as transition regardless of region.

### 5.5 `_text_` vs. `__text__`

- Single underscore `_text_` = **italic** in all contexts (Markdown-compatible).
- Double underscore `__text__` = **underline** in all contexts (FountainMD extension).
- Never treat single underscore as underline. This is a hard override of standard Fountain.

### 5.6 Lyric Lines

`~` prefix marks a lyric. This is unambiguous and requires no inference. Render as italicized, indented, or otherwise visually distinct from action and dialogue per the output renderer's stylesheet.

---

## 6. Output Contract

When an LLM agent parses FountainMD, it must return a structured JSON array of elements. Each element has the following shape:

```json
{
  "type": "<element_type>",
  "content": "<text content, with inline markup preserved>",
  "meta": {}
}
```

### 6.1 Element Types

| `type` value | Description |
|---|---|
| `heading` | Markdown header. Include `"level": 1\|2\|3` in `meta`. |
| `action` | Screenplay action / scene description |
| `scene_heading` | INT./EXT. scene heading |
| `character` | Character cue |
| `dialogue` | Dialogue line |
| `parenthetical` | Parenthetical within dialogue |
| `transition` | Transition line |
| `lyric` | `~` prefixed lyric line |
| `synopsis` | `=` prefixed writer annotation |
| `dual_dialogue` | Pair of simultaneous dialogue blocks. Include `"left"` and `"right"` arrays in `meta`. |
| `paragraph` | Prose paragraph (Markdown body text) |
| `list` | Markdown list. Include `"ordered": true\|false` and `"items": []` in `meta`. |
| `blockquote` | Markdown blockquote |
| `code_block` | Fenced code block. Include `"language"` in `meta` if declared. |
| `rule` | Horizontal rule (`---`) |
| `page_break` | Forced page break (`===`) |
| `boneyard` | Ignored content (never include in output array) |
| `title_page` | Key-value pairs from title page. Represent as `"meta": { "key": "value", ... }` |

### 6.2 Inline Markup

Inline emphasis within `content` strings is preserved using the following markers for the renderer to process:

- Bold: `**text**`
- Italic: `*text*`
- Underline: `__text__`
- Bold Italic: `***text***`

Do not convert inline markup to HTML in the JSON output. Leave it as FountainMD syntax for the renderer.

### 6.3 Example Output

**Input:**
```
# The Conversation

= This scene is the emotional core of act two.

INT. COFFEE SHOP - DAY

Alice sits alone. She checks her phone. Nothing.

BOB
(entering)
You actually came.

ALICE
I almost didn't.
```

**Expected JSON output:**
```json
[
  {
    "type": "heading",
    "content": "The Conversation",
    "meta": { "level": 1 }
  },
  {
    "type": "synopsis",
    "content": "This scene is the emotional core of act two.",
    "meta": {}
  },
  {
    "type": "scene_heading",
    "content": "INT. COFFEE SHOP - DAY",
    "meta": {}
  },
  {
    "type": "action",
    "content": "Alice sits alone. She checks her phone. Nothing.",
    "meta": {}
  },
  {
    "type": "character",
    "content": "BOB",
    "meta": {}
  },
  {
    "type": "parenthetical",
    "content": "(entering)",
    "meta": {}
  },
  {
    "type": "dialogue",
    "content": "You actually came.",
    "meta": {}
  },
  {
    "type": "character",
    "content": "ALICE",
    "meta": {}
  },
  {
    "type": "dialogue",
    "content": "I almost didn't.",
    "meta": {}
  }
]
```

---

## 7. Title Page

If the document begins with key-value pairs before any other content, treat them as title page metadata. Standard Fountain title page keys are supported:

```
Title: The Long Walk Home
Credit: Written by
Author: Jane Smith
Draft date: 2024-01-15
Contact: jane@example.com
```

Any key is valid. Output as a single `title_page` element with all pairs in `meta`.

---

## 8. Versioning and Changelog

### v0.1.0 (initial draft)
- Defined three Fountain overrides: `__text__` for underline, `#` headers rendered visually, `=` retains synopsis role.
- Established inference rules for region detection, ALL CAPS, `>`, and `_`/`__`.
- Defined JSON output contract with element type taxonomy.
- Aligned prose elements with CommonMark Markdown.

---

## 9. What Remains Standard Fountain

Everything not mentioned above follows the Fountain spec at https://fountain.io/syntax:

- Scene heading auto-detection and forced scene headings
- Character detection and forced character cues (`@`)
- Dialogue and parenthetical structure
- Transition detection and forced transitions
- Dual dialogue (`^`)
- Lyrics (`~`)
- Boneyard (`/* */`)
- Page breaks (`===`)
- Scene numbers (`#1#` on scene headings)
- Emphasis escaping with backslash
- Notes (`[[text]]`)
- Title page key-value format

---

*FountainMD is an open extension of Fountain. Fountain is © John August and Stu Maschwitz.*