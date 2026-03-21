# Two-Mode Refactor Plan

## Current State

The codebase is well-structured but operates as a single implicit mode — everything flows through the same agent loop with all tools available. Canvas tools modify files directly, the GUI canvas is read-only (snapshot-driven), and there's no concept of staging changes or exporting structured output.

The refactor is additive. Existing code paths don't break — we're layering mode awareness on top.

---

## Phase 1: Mode Foundation

**Goal**: Introduce the mode concept so the system knows which mode it's operating in and can branch behavior accordingly.

### 1a. Add `SessionMode` enum

**File**: `src/gui/state.rs`

```rust
pub enum SessionMode {
    Consultant,  // Read-only analysis, produces revision briefs
    Workshop,    // Live editing of .md/.fountain via canvas tools
}
```

Add to `GuiState`:
```rust
pub session_mode: SessionMode,
```

Also add to `GuiRequest`:
```rust
SwitchMode(SessionMode),
```

### 1b. Add mode to project config

**File**: `src/config.rs`

Add optional `default_mode` to `ProjectConfig`. If unset, infer:
- Project has only `.docx`/`.txt` files → default Consultant
- Project has `.md`/`.fountain` files → default Workshop
- Mixed → let user choose (default Consultant)

### 1c. Add `editable` flag to manifest

**File**: `src/concepts/manifest.rs`

Add `editable: bool` to `StoryFile`. Infer from format:
- `.md`, `.fountain` → `true`
- `.docx`, `.txt` → `false`

Allow manual override in `manifest.toml`.

### 1d. Mode-based tool filtering

**File**: `src/concepts/skills.rs` (or `skills/mod.rs`)

Extend `SkillSetContext`:
```rust
pub enum SkillSetContext {
    Chat,
    Analysis,
    Perspective,
    Consultant,  // All read tools + brief tools, NO canvas write tools
    Workshop,    // All tools including canvas writes
}
```

When building the tool list for the LLM, filter based on mode:
- **Consultant**: Exclude `write_to_canvas`, `replace_in_canvas`, `insert_scene`. Include `add_to_brief`, `generate_brief`.
- **Workshop**: Include canvas tools. Exclude brief tools (or keep them available — a Workshop user might still want to export notes).

### 1e. Mode-aware system prompt

**File**: `src/gui/agent.rs` and `src/cli/chat.rs`

Prepend mode instructions to the agent's system prompt:

- **Consultant**: "You are analyzing the writer's manuscript. Do not modify any files. Structure your revision suggestions with specific scene references (file, scene title, line range). When the writer asks you to make changes, describe what should change and why — they will implement the changes in their own editor. Use the `add_to_brief` tool to record each specific revision suggestion."

- **Workshop**: "You are co-editing the writer's manuscript. You can use canvas tools to write, replace, and insert content directly in .md and .fountain files. Always explain what you're about to change before making edits."

---

## Phase 2: Revision Brief

**Goal**: Build the data structure and tools that let the agent produce structured, scene-anchored revision documents in Consultant mode.

### 2a. `RevisionBrief` struct

**New file**: `src/concepts/revision_brief.rs`

```rust
pub struct RevisionBrief {
    pub project_title: String,
    pub created: DateTime<Utc>,
    pub focus: String,           // What this brief addresses
    pub scope: String,           // e.g., "Scenes 8-14", "Full manuscript"
    pub overview: String,        // 2-3 sentence summary
    pub revisions: Vec<Revision>,
    pub structural_notes: Vec<StructuralNote>,
}

pub struct Revision {
    pub scene_id: Option<SceneId>,
    pub scene_title: String,
    pub file_path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub priority: Priority,
    pub current_state: String,   // Excerpt of what's there now
    pub issue: String,           // What the analysis found
    pub suggestion: String,      // Specific actionable instruction
    pub draft_passage: Option<String>,  // Optional rewrite
    pub graph_impact: Vec<String>,      // What changes in the graph
}

pub enum Priority { High, Medium, Low }

pub struct StructuralNote {
    pub category: String,  // "Pacing", "Arc completeness", "Timeline"
    pub note: String,
}
```

Methods:
- `new(project_title)` — Initialize empty brief
- `add_revision(revision)` — Append a revision
- `add_structural_note(note)` — Append a note
- `to_markdown() -> String` — Render as Markdown (using the format from two-mode-concept.md)
- `save_markdown(path)` — Write to file
- `save_docx(path)` — Write as simple .docx (Phase 2c)

### 2b. Brief-related skills

**New file**: `src/concepts/skills/brief_tools.rs`

Two new built-in skills:

**`add_to_brief`** — The agent calls this during conversation to record a revision suggestion.

```
Input:
  scene_title: String       (required)
  file: String              (required — file path)
  priority: "high"|"medium"|"low"  (default: medium)
  current_state: String     (excerpt of current text)
  issue: String             (what's wrong)
  suggestion: String        (what to change)
  draft: String             (optional rewrite)

Effect:
  Appends a Revision to the active RevisionBrief (held in SkillContext)
  Resolves scene_id and line numbers from SceneMap
  Returns confirmation with revision count
```

**`generate_brief`** — Finalizes and exports the brief.

```
Input:
  focus: String             (what this brief addresses)
  scope: String             (scene range or "full manuscript")
  overview: String          (2-3 sentence summary)
  format: "md"|"docx"      (default: md)

Effect:
  Sets brief metadata
  Renders to markdown/docx
  Saves to .laires/briefs/{date}-{slug}.{ext}
  Returns file path
```

### 2c. Simple .docx writer

**New file**: `src/concepts/docx_writer.rs` (or extend `docx.rs`)

Generates a basic .docx from the brief's Markdown output. Scope is intentionally small:
- Headings (H1-H3)
- Paragraphs
- Blockquotes (for excerpts/drafts)
- Bullet lists (for checklists, graph impact)
- Bold/italic inline

Use `zip` crate (already likely a transitive dep via docx reading) + raw XML templates. The .docx format for simple documents is just a handful of XML files in a zip. No need for a full .docx library.

### 2d. `laires brief` CLI command

**New file**: `src/cli/brief.rs`

```
laires brief                        # Export current brief as .md
laires brief --format docx          # Export as .docx
laires brief --output ./notes.md    # Custom output path
laires brief --list                 # List saved briefs
```

Reads the most recent brief from `.laires/briefs/` or the active session's accumulated brief.

### 2e. Hold RevisionBrief in shared state

Add `revision_brief: Option<RevisionBrief>` to `ProjectData` (GUI) and the equivalent CLI session state. The agent accumulates revisions during conversation; the user exports when ready.

---

## Phase 3: GUI Mode Integration

**Goal**: The GUI reflects the active mode — different tool availability, different right-pane tabs, and mode switching.

### 3a. Mode selector in GUI

**File**: `src/gui/mod.rs`

Add a mode toggle to the top bar or sidebar. Two options:
- **Project-level**: Set once at init, stored in config. Mode shown in status bar.
- **Session-level**: Toggle freely. Status bar shows current mode.

Recommend session-level — a writer might start in Consultant, review analysis, then switch to Workshop to draft a revision. The mode selector could be a simple toggle button in the status bar or top bar.

### 3b. Right pane tabs per mode

**File**: `src/gui/state.rs`, `src/gui/mod.rs`

**Consultant mode tabs**:
- Dashboard (overview + stats)
- Canvas (read-only story display)
- Brief (revision brief viewer — accumulated suggestions)
- Graph

**Workshop mode tabs**:
- Dashboard
- Canvas (editable — future: inline text input)
- Graph
- Lint
- Pacing

### 3c. Brief panel

**New file**: `src/gui/panels/brief.rs`

Renders the active `RevisionBrief` in the right pane:
- Overview header
- List of revisions grouped by priority
- Each revision shows scene reference, issue, suggestion
- "Export" button (triggers `generate_brief` skill or direct export)
- Click a revision → highlights the relevant scene in canvas

### 3d. Canvas mode awareness

**File**: `src/gui/panels/canvas.rs`

- **Consultant**: Display as-is (current behavior). Clicking a brief revision scrolls to and highlights the relevant scene.
- **Workshop**: Future — add inline editing capability. For now, edits happen through the agent chat + canvas tools. The canvas re-renders from the updated snapshot.

### 3e. Chat panel mode hints

**File**: `src/gui/panels/chat.rs`

- Show mode indicator in chat input area (e.g., "Consultant mode — analysis only" or "Workshop mode — editing enabled")
- In Consultant mode, when the agent suggests changes, the chat could show a "Add to brief" button that the user clicks to capture the suggestion.

---

## Phase 4: Agent Loop Refinement

**Goal**: The agent behaves differently per mode — different system prompts, different tool sets, different output patterns.

### 4a. Mode-aware agent session

**File**: `src/gui/agent.rs`

When processing a `Chat` request:
1. Check `session_mode` from state
2. Build tool list using mode-appropriate `SkillSetContext`
3. Set system prompt with mode-specific instructions
4. Execute turn

### 4b. Consultant mode agent behavior

The agent in Consultant mode should:
- Analyze scenes, run lint/pacing/perspective tools freely
- When the user asks for changes: describe what to change, call `add_to_brief` to record it
- At session end (or on request): call `generate_brief` to export
- Never call `write_to_canvas`, `replace_in_canvas`, or `insert_scene`

### 4c. Workshop mode agent behavior

The agent in Workshop mode should:
- Have full tool access including canvas writes
- Explain what it's about to change before executing canvas tools
- After edits: suggest running `scan_story` to update the graph
- Support undo? (Future — for now, git/jj is the undo mechanism)

### 4d. Mode switching mid-session

When the user switches from Consultant → Workshop (or vice versa):
- Update `session_mode` in state
- Rebuild tool list for next turn
- Add a system message to conversation: "Mode switched to {mode}. Tool availability updated."
- If switching to Workshop: warn if project has no editable files

---

## Phase 5: Polish & Ergonomics

### 5a. Auto-brief at end of Consultant session

When a Consultant session ends (user quits or starts new session), if there are accumulated revisions in the brief, prompt: "You have {N} revision suggestions. Export as brief? [y/N]"

### 5b. Brief history

Save all briefs to `.laires/briefs/` with timestamps. `laires brief --list` shows history. GUI brief panel can browse past briefs.

### 5c. Workshop file guards

In Workshop mode, canvas tools should refuse to modify files where `editable: false` in the manifest. Error message: "Cannot edit {file} — it's a .docx file. Use Consultant mode for revision suggestions, or convert to .md for direct editing."

### 5d. Import-to-editable workflow

Add a `laires convert` command (or skill) that converts a .docx to .md:
- Extracts text (already implemented in `docx.rs`)
- Writes as .md with scene headings preserved
- Updates manifest to mark the new .md as editable
- Keeps the original .docx (doesn't delete it)

This bridges Consultant → Workshop for writers who decide they want to go deeper.

---

## Implementation Order

```
Phase 1 (Foundation)     ~2-3 sessions
  1a. SessionMode enum
  1b. Config default_mode
  1c. Manifest editable flag
  1d. Tool filtering
  1e. System prompts

Phase 2 (Brief)          ~3-4 sessions
  2a. RevisionBrief struct
  2b. Brief skills (add_to_brief, generate_brief)
  2e. Hold brief in shared state
  2d. laires brief CLI
  2c. Simple .docx writer (can defer)

Phase 3 (GUI)            ~2-3 sessions
  3a. Mode selector
  3b. Tab filtering
  3c. Brief panel
  3d-e. Canvas + chat mode awareness

Phase 4 (Agent)          ~1-2 sessions
  4a-c. Mode-aware agent behavior
  4d. Mid-session switching

Phase 5 (Polish)         ~1-2 sessions
  5a-d. Ergonomics, guards, convert command
```

Each phase is independently useful:
- After Phase 1: Mode exists, tools are filtered, but no brief output yet (agent just describes changes in chat)
- After Phase 2: Full Consultant workflow works in CLI
- After Phase 3: GUI supports both modes visually
- After Phase 4: Agent behavior is tuned per mode
- After Phase 5: Smooth edges, conversion workflow

---

## Files Changed (Summary)

**Modified**:
- `src/config.rs` — Add `default_mode`
- `src/gui/state.rs` — Add `SessionMode`, mode to `GuiState`, `GuiRequest`
- `src/gui/mod.rs` — Mode selector, tab filtering, layout changes
- `src/gui/agent.rs` — Mode-aware tool lists, system prompts
- `src/gui/panels/canvas.rs` — Mode-aware rendering
- `src/gui/panels/chat.rs` — Mode indicator
- `src/gui/panels/sidebar.rs` — Brief integration (optional)
- `src/concepts/skills.rs` — `SkillSetContext` variants, mode filtering
- `src/concepts/skills/canvas_tools.rs` — Editable file guards
- `src/concepts/manifest.rs` — `editable` field on `StoryFile`
- `src/cli/mod.rs` — New `brief` command, mode flags
- `src/cli/chat.rs` — Mode-aware agent setup

**New**:
- `src/concepts/revision_brief.rs` — `RevisionBrief` struct + rendering
- `src/concepts/skills/brief_tools.rs` — `add_to_brief`, `generate_brief`
- `src/concepts/docx_writer.rs` — Simple .docx generation for briefs
- `src/cli/brief.rs` — `laires brief` command
- `src/gui/panels/brief.rs` — Brief viewer panel
