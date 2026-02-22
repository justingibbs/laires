# Laires.ai — Vision Document

## An Agentic Writing Tool Powered by Narrative Graph Intelligence

---

## The Idea

Laires.ai is a CLI/TUI tool for fiction writers that does for storytelling what agentic coding tools (Cursor, Claude Code, Aider) do for software engineering. It treats a manuscript like a codebase — indexable, navigable, and analyzable — and builds a **narrative graph** that maps characters, objectives, conflicts, and scenes into a structured, queryable model.

Writers interact with an LLM agent that has access to tools for searching, analyzing, and reasoning about their story. The agent doesn't just see raw text; it understands narrative structure through the graph, enabling it to answer questions like *"Where does Sarah's motivation contradict her actions?"* or *"Which scenes lack a clear objective conflict?"* with structural awareness, not just pattern matching over prose.

## The Analogy: Stories as Codebases

The insight driving Laires.ai is that narrative structure is deeply analogous to code structure:

| Code Concept         | Story Concept                                      |
|----------------------|----------------------------------------------------|
| Codebase             | Manuscript (collection of files)                   |
| Module / File        | Chapter / Scene                                    |
| Function             | Scene (a unit of action with inputs and outputs)   |
| Variable state       | Character objective at a point in time             |
| Call graph            | Scene sequence / narrative flow                    |
| Dependency graph     | Character relationship web                         |
| Type system          | Story rules and constraints (genre, world, tone)   |
| Race condition       | Objective conflict between characters              |
| Dead code            | Scenes that don't advance any objective            |
| Refactoring          | Revision with structural awareness                 |
| Linter               | Consistency checker (timeline, character presence)  |
| AST                  | Narrative graph                                    |
| Test suite           | Reader-perspective analysis ("Does this scene land?") |

Just as a coding agent navigates code by understanding its structure — not just grepping text — Laires.ai navigates stories by understanding narrative architecture.

## The Narrative Graph: The Core Innovation

The narrative graph is Laires.ai's equivalent of a codebase's AST and dependency graph combined. It is a structured representation of:

### Nodes

- **Characters**: Named entities with attributes, voice, and role.
- **Objectives**: What a character wants. Hierarchical — an overarching goal (e.g., "reclaim the throne") decomposes into act-level goals ("gain allies," "expose the usurper") and scene-level tactics ("persuade the general at the banquet").
- **Scenes**: The atomic unit of narrative action. Each scene declares which characters are present and which objectives are active.
- **Conflicts**: Where two or more objectives collide. These are the engine of drama.

### Edges

- **Pursues**: Character → Objective (in a given scene or across the story)
- **Decomposes into**: Objective → Sub-objectives
- **Conflicts with**: Objective ↔ Objective
- **Present in**: Character → Scene
- **Advances / Blocks**: Scene → Objective (does this scene move an objective forward, block it, or leave it static?)
- **Precedes**: Scene → Scene (narrative sequence)

### What the Graph Enables

The graph is both a tool the LLM agent can query and a visualization the writer can explore. It enables analysis such as:

- **Objective tracking**: "Show me the arc of Marcus's ambition across all chapters."
- **Conflict mapping**: "Which scenes have the strongest objective collisions?"
- **Dead scene detection**: "Are there scenes where no objective advances or is blocked?"
- **Character perspective analysis**: "Interpret Chapter 7 from Elena's perspective — what does she want, what's blocking her, and what does she decide?"
- **Structural balance**: "Act 2 has 12 scenes advancing the love plot but only 2 for the political subplot."
- **Consistency checking**: "Is there a scene where a character acts against their established objective without a motivation shift?"

## Agent Tools

Laires.ai exposes a set of tools the LLM agent can call, analogous to how coding agents use grep, file read, AST analysis, and code execution:

### File Tools
- **story_grep**: Search across manuscript files by text, regex, or semantic similarity.
- **read_scene**: Read a specific scene or chapter file.
- **list_files**: List all manuscript files with metadata (word count, last modified, characters present).

### Graph Tools
- **query_graph**: Query the narrative graph — filter by character, objective, scene, conflict, or relationship type.
- **get_character_arc**: Return the full objective trajectory for a character across all scenes.
- **get_scene_analysis**: Return which objectives are active, advanced, or blocked in a scene.
- **get_conflicts**: Return all objective conflicts, optionally filtered by character or scene range.
- **find_dead_scenes**: Identify scenes where no objective changes state.

### Perspective Tools
- **interpret_as_character**: Analyze a scene or chapter from a specific character's perspective — their knowledge, goals, emotions, and decisions as they would experience them.
- **compare_perspectives**: Show how two characters experience the same scene differently based on their respective objectives.

### Structural Tools
- **story_lint**: Run consistency checks — timeline errors, character presence conflicts, objective contradictions.
- **pacing_analysis**: Analyze scene length, objective density, and conflict frequency across the manuscript.
- **arc_completeness**: Check whether character objectives reach resolution.

## Architecture

```
┌─────────────────────────────────────────────┐
│                 CLI / TUI                    │
│        (ratatui — panels, chat, graph)       │
├─────────────────────────────────────────────┤
│              Agent Orchestrator              │
│     (tool dispatch, LLM conversation loop)   │
├──────────┬──────────┬───────────────────────┤
│  File    │  Graph   │   LLM Integration     │
│  Indexer │  Engine  │   Layer               │
│          │          │                       │
│  ripgrep │ petgraph │  OpenAI-compatible    │
│  tree-   │ serde    │  Anthropic            │
│  sitter  │ JSON     │  Pydantic AI Gateway  │
│          │ persist  │  Local (Ollama, etc.) │
├──────────┴──────────┴───────────────────────┤
│              Story Files                     │
│  Markdown (default) │ Import: docx, pdf, etc.│
│                     │ Export: epub, docx      │
└─────────────────────────────────────────────┘
```

### Tech Stack

- **Language**: Rust
- **CLI/TUI**: clap + ratatui
- **Graph engine**: petgraph, persisted as JSON
- **File indexing**: Custom parsers, ripgrep for text search, tree-sitter for structure
- **LLM integration**: Trait-based abstraction supporting OpenAI-compatible APIs, Anthropic, Pydantic AI Gateway, and local model servers (Ollama, llama.cpp, LM Studio)
- **Serialization**: serde (JSON for graph state, TOML for config)
- **Future GUI**: Tauri (Rust backend + web frontend with interactive graph visualization)

### Configuration

Users configure Laires.ai via a `.laires/config.toml` in their project directory:

```toml
[llm]
provider = "openai-compatible"  # or "anthropic", "pydantic-gateway", "local"
api_key_env = "OPENAI_API_KEY"  # environment variable name
model = "gpt-4o"
base_url = "https://api.openai.com/v1"  # or "http://localhost:11434/v1" for Ollama

[project]
manuscript_dir = "./chapters"
file_format = "markdown"        # default working format
```

## Development Roadmap

### Phase 1: Core Engine
- Markdown file reading and indexing
- Narrative graph construction (LLM-assisted analysis of scenes)
- Graph persistence (JSON)
- Basic CLI with chat loop and tool calling

### Phase 2: Agent Tools
- story_grep, read_scene, query_graph
- Character perspective analysis
- Story linting (consistency checks)
- Pacing and structural analysis

### Phase 3: Polish and Formats
- TUI with ratatui (graph panel, chat, file viewer)
- Import support (docx, PDF, plain text)
- Export support (epub, docx)
- Multiple writing format support (prose, screenplay)

### Phase 4: GUI
- Tauri desktop app
- Interactive graph visualization
- Visual scene/objective editor

---

## Open Questions for Specification

The following questions need answers before moving to a detailed technical spec:

### File Format and Structure
1. Should Markdown be the canonical working format, with import/export for other formats?
2. How should scenes be delineated within files — one file per scene, per chapter, or frontmatter markers within a single file?
3. Should we define a frontmatter schema (YAML) for scene metadata (characters present, location, time)?
4. Should the graph be stored alongside the manuscript (`.laires/` directory) or separately?

### Writing Format Support
5. How do we handle screenplays vs. novels vs. short stories? Separate parsers, or a unified model with format-specific adapters?
6. Should we support Fountain (screenplay format) natively, or treat it as an import format?
7. Do different formats need different graph models (e.g., screenplays have acts/scenes structurally; novels may not)?

### Import and Export
8. Should we support Word (.docx) and PDF as source file inputs with automatic conversion to Markdown?
9. Is EPUB export a priority? If so, should we generate it from the Markdown source or from the graph + source combined?
10. Should export preserve the narrative graph as metadata (e.g., EPUB with embedded structural data)?

### Version Control
11. Should we support or integrate with JJ (Jujutsu) for change tracking? Or Git? Or both?
12. Is there value in story-aware diffing — showing not just text changes but how the narrative graph changed between versions?
13. Should version control be built-in or opt-in (user brings their own VCS)?

### Graph Construction
14. Should the initial graph be fully LLM-generated from existing text, or should writers be able to manually declare objectives and relationships?
15. How do we handle graph drift — when the writer edits text but the graph hasn't been updated?
16. Should graph updates be automatic (re-analyze on file save) or manual (writer triggers re-analysis)?

### LLM and Privacy
17. For local model support, should we test against specific models (e.g., Llama 3, Mistral) or just support the OpenAI-compatible API generically?
18. Should there be a "privacy mode" indicator showing writers when their text is being sent to a cloud API vs. processed locally?
19. How should we handle context window limits when a novel exceeds the LLM's context?

### User Experience
20. What does the ideal TUI layout look like — split panes with graph, chat, and file viewer? Or a simpler chat-first interface?
21. Should writers be able to "talk to" a specific character (the agent adopts the character's perspective and objectives for the conversation)?
22. Should the tool support collaborative writing (multiple authors, shared graph)?
