# Laires — Technical Overview

## What It Is

Laires is an agentic writing tool that treats fiction manuscripts like codebases. It reads a project's story files (`.md`, `.fountain`, `.docx`, `.txt`), builds a **narrative graph** — a structured model of characters, objectives, conflicts, and scenes — and exposes that graph to an LLM agent equipped with 28 tools for structural analysis, consistency checking, perspective interpretation, and co-writing.

## The Core Analogy: Stories as Codebases

| Code Concept      | Story Concept                                     |
|-------------------|----------------------------------------------------|
| Codebase          | Project folder (collection of story files)         |
| Module / File     | Story file / Chapter                               |
| Function          | Scene (a unit of action with inputs and outputs)   |
| Variable state    | Character objective at a point in time             |
| Call graph         | Scene sequence / narrative flow                    |
| Dependency graph  | Character relationship web                         |
| Type system       | Story rules and constraints (genre, world, tone)   |
| Race condition    | Objective conflict between characters              |
| Dead code         | Scenes that don't advance any objective            |
| Linter            | Consistency checker (timeline, character presence)  |
| AST               | Narrative graph                                    |

## The Narrative Graph

A directed graph (petgraph `DiGraph`) with four node types and eight edge types.

**Nodes:**

- **Character** — name, aliases, description
- **Objective** — what a character wants; scoped to Overarching / Act / Scene; tracked as Active / Achieved / Abandoned / Blocked / Transformed
- **Scene** — title, summary, characters present, location, time, source file
- **Conflict** — where two or more objectives collide

**Edges:**

- `Pursues` — Character &rarr; Objective (optionally scoped to a scene)
- `DecomposesInto` — Objective &rarr; Sub-objective
- `ConflictsWith` — Objective &harr; Objective
- `PresentIn` — Character &rarr; Scene
- `Advances` — Scene &rarr; Objective
- `Blocks` — Scene &rarr; Objective
- `Precedes` — Scene &rarr; Scene (narrative sequence)
- `Transforms` — Objective mutation triggered by a scene

The graph is fully LLM-generated from scene text during `laires scan`, then queryable by the agent and the writer. Writers can override any inferred value with a **declared intent** — an explicit assertion that takes precedence over what the LLM extracted.

## Agent Tools (28 built-in + custom)

The LLM agent has access to tools organized into seven categories:

| Category | Tools | Purpose |
|----------|-------|---------|
| **FileTools** (6) | `story_grep`, `read_scene`, `list_scenes`, `story_stats`, `read_context_file`, `list_files` | Search and read project files |
| **GraphTools** (7) | `query_graph`, `get_character_arc`, `get_conflicts`, `find_dead_scenes`, `get_scene_analysis`, `get_divergences`, `scan_story` | Query and rebuild the narrative graph |
| **PerspectiveTools** (4) | `interpret_as_character`, `compare_perspectives`, `find_blind_spots`, `get_knowledge_at` | Subjective character viewpoints via LLM |
| **StructuralTools** (5) | `story_lint`, `pacing_analysis`, `arc_completeness`, `declare_intent`, `retract_intent` | Consistency checks and writer overrides |
| **CanvasTools** (3) | `write_to_canvas`, `replace_in_canvas`, `insert_scene` | Edit story files (Workshop mode only) |
| **BriefTools** (2) | `add_to_brief`, `generate_brief` | Build revision briefs (Consultant mode only) |
| **CustomTools** | User-defined TOML files in `.laires/skills/` | Extend the agent with project-specific tools |

## Two Operating Modes

- **Consultant** — Read-only analysis. The agent examines the manuscript and produces a structured revision brief. CanvasTools are disabled; BriefTools are enabled. Best for `.docx`/`.txt` projects or when the writer wants suggestions without direct edits.
- **Workshop** — Live co-editing. The agent can read and write `.md`/`.fountain` files through CanvasTools. All tool categories are available. File guards prevent edits to non-editable formats.

---

# Architecture

## Data Flow

```
 Story Files (.md, .fountain, .docx, .txt)
       |
       v
 +--------------+     LLM classifies files      +----------------+
 | File Discovery| --------------------------->  | Manifest       |
 | (recursive    |     as story / outline /      | (.laires/      |
 |  walk + hash) |     characters / notes        |  manifest.toml)|
 +--------------+                                +----------------+
       |                                                |
       v                                                v
 +--------------+                                +------------------+
 | TextBuffer   |  one per story file            | FileBufferManager|
 | (in-memory   |  <--------------------------- | (coordinates     |
 |  file content)|                               |  all buffers)    |
 +--------------+                                +------------------+
       |
       v
 +--------------+     regex boundary detection
 | SceneMap     |     (Prose: headings/rules/comments)
 | (scene spans,|     (Fountain: INT./EXT. headings)
 |  byte ranges,|
 |  hashes)     |
 +--------------+
       |
       v                                         +------------------+
 +--------------+     LLM extracts characters,   | NarrativeGraph   |
 | Analysis     |     objectives, conflicts  --> | (petgraph DiGraph|
 | (per-scene   |     from each scene's text     |  .laires/        |
 |  LLM calls)  |                                |  graph.json)     |
 +--------------+                                +------------------+
                                                        |
                     +----------------------------------+
                     |                |                  |
                     v                v                  v
              +----------+    +-------------+    +-----------+
              | CLI      |    | TUI         |    | GUI       |
              | (clap    |    | (ratatui    |    | (egui/    |
              |  commands)|    |  split-pane)|    |  eframe)  |
              +----------+    +-------------+    +-----------+
                     |                |                  |
                     +-------+--------+------------------+
                             |
                             v
                     +-----------------+
                     | Agent Loop      |
                     | (LLM + Skills   |
                     |  multi-turn     |
                     |  tool calling)  |
                     +-----------------+
                             |
                     +-------+--------+
                     |                |
                     v                v
              +------------+  +----------------+
              | Provider   |  | DeclaredIntent |
              | (Anthropic,|  | (writer        |
              |  OpenAI,   |  |  overrides)    |
              |  Gemini,   |  +----------------+
              |  Local)    |         |
              +------------+         v
                              +-------------+
                              | Divergence  |
                              | Detection   |
                              | (inferred   |
                              |  vs declared)|
                              +-------------+
```

## Concept Modules

The system is structured around independent concept modules, each owning its own state and operations. Concepts do not call each other directly.

| Module | Purpose |
|--------|---------|
| `text_buffer` | In-memory file content; byte-range reads and writes |
| `scene_map` | Detects scene boundaries via regex; dual-mode (Prose / Fountain) |
| `narrative_graph` | Directed graph of Characters, Objectives, Scenes, Conflicts; serialization, diffing, queries |
| `provider` | LLM abstraction over Anthropic, OpenAI-compatible, Local (Ollama), PydanticGateway endpoints |
| `analysis` | Queues and caches per-scene LLM analysis tasks; parses extraction results |
| `skills` | Tool registry; 7 categories, mode-filtered schemas, async invocation with `SkillContext` |
| `manifest` | File discovery, LLM classification, TOML persistence; tracks content hashes and editability |
| `file_buffer_manager` | Coordinates multiple TextBuffer + SceneMap instances across a multi-file project |
| `declared_intent` | Stores explicit writer overrides on graph node fields; persisted to TOML |
| `character_perspective` | LLM-powered subjective scene interpretation; awareness tracking, blind spot detection |
| `revision_brief` | Accumulates scene-anchored revision suggestions in Consultant mode |
| `context_budget` | Token estimation and history summarization for LLM context windows |
| `canvas` | Viewport and scene highlight state for the TUI/GUI |
| `docx` | Reads `.docx` files (zip + XML extraction) |

## Synchronization

Cross-concept coordination is handled by explicit sync functions, not background jobs or event systems. The primary sync mechanism is **divergence detection**: comparing the LLM-inferred narrative graph against writer-declared intents.

Two functions in `src/sync/divergence.rs`:

- **`detect_divergences(graph, intent)`** — Returns a list of mismatches where an inferred graph value differs from what the writer declared. Each `Divergence` carries the node ID, field, inferred value, declared value, and the writer's rationale.
- **`detect_orphans(graph, intent)`** — Returns declared intents that reference nodes no longer in the graph.

These are called on-demand by `laires lint`, the agent's `get_divergences` tool, and during agent context preparation — surfacing conflicts between what the LLM thinks and what the writer asserts.

## Interfaces

All three interfaces share the same domain modules and agent loop:

- **CLI** (`src/cli/`) — 13 clap subcommands: `init`, `scan`, `graph`, `status`, `lint`, `chat`, `open`, `gui`, `diff`, `log`, `perspective`, `brief`, `convert`
- **TUI** (`src/cli/tui.rs`) — ratatui split-pane: chat left, canvas right, with graph/lint/pacing/file-explorer overlays (Ctrl+G/L/P/E)
- **GUI** (`src/gui/`) — egui/eframe desktop app: sidebar (scenes/files), chat panel, canvas, force-directed graph view, status bar with mode toggle

## Persistence

All project state lives under `.laires/`:

| File | Format | Contents |
|------|--------|----------|
| `config.toml` | TOML | Provider, model, project title, format, analysis settings |
| `manifest.toml` | TOML | Classified files with roles, order, hashes, editability |
| `graph.json` | JSON | Full narrative graph (nodes + edges) |
| `scenes.json` | JSON | Scene boundary cache |
| `overrides.toml` | TOML | Writer-declared intent overrides |
| `perspectives/` | JSON | Cached character perspective analyses |
| `cache/` | — | LLM analysis cache (git-ignored) |
| `skills/*.toml` | TOML | Custom tool definitions |
