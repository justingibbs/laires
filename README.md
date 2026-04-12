# Laires

[![CI](https://github.com/justingibbs/laires/actions/workflows/ci.yml/badge.svg)](https://github.com/justingibbs/laires/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

An agentic writing tool that treats fiction manuscripts like codebases. Laires builds a **narrative graph** — a structured model of characters, objectives, conflicts, and scenes — and uses an LLM agent to provide structural analysis, perspective interpretation, consistency checking, and co-writing assistance.

## Features

- **Narrative graph** — Automatically extracts characters, objectives, conflicts, and relationships from your manuscript into a structured, queryable graph
- **Native desktop GUI** — A single-binary desktop app (egui/eframe) with chat, canvas, scene sidebar, and force-directed graph visualization
- **Two operating modes** — *Consultant* mode for read-only analysis with revision briefs; *Workshop* mode for live co-editing with the agent
- **Multi-turn agent chat** — An LLM agent with 24+ built-in tools for querying the graph, searching text, running lint checks, analyzing pacing, and more
- **Character perspectives** — LLM-powered subjective interpretation of scenes through any character's point of view
- **Multi-file projects** — Manage novels, screenplays, or any multi-document project with automatic file discovery and classification
- **Prose and Fountain** — Full support for Markdown prose and Fountain screenplay format, including .docx import
- **Multiple LLM providers** — Anthropic, OpenAI, Google Gemini, local models (Ollama), or any OpenAI-compatible endpoint
- **Consistency checking** — Lint rules and divergence detection to catch contradictions and plot holes
- **Custom skills** — Extend the agent with your own TOML-defined tools
- **CLI and TUI** — Full command-line interface plus a split-pane terminal UI with graph, lint, and pacing overlays
- **VCS integration** — `laires diff` and `laires log` show narrative graph changes across git/jj commits

## Prerequisites

- [Rust toolchain](https://rustup.rs/) (1.93+)
- An LLM API key (Gemini, Anthropic, OpenAI, or a local model)

## Installation

```bash
git clone https://github.com/justingibbs/laires.git && cd laires
cargo install --path .
```

This installs the `laires` binary to `~/.cargo/bin/`, which should already be on your `$PATH`.

## Quick start

### 1. Create a project directory

```bash
mkdir my-novel && cd my-novel
```

### 2. Add your API key

Create a `.env` file in the project directory:

```bash
echo 'GEMINI_API_KEY=your-key-here' > .env
```

### 3. Initialize the project

```bash
laires init --title "My Novel"
```

This creates:
- `story.md` — your manuscript
- `.laires/config.toml` — project configuration
- `.laires/graph.json` — narrative graph (populated by scan)
- `.laires/scenes.json` — scene boundary map
- `.laires/cache/` — analysis cache

### 4. Write your story

Edit `story.md` with your editor. Use Markdown headings or horizontal rules (`---`) to separate scenes:

```markdown
## The Arrival

Elena stepped off the train into the cold morning air. The town
looked nothing like the photographs.

---

## The Letter

Marcus found the letter tucked inside the old piano. The handwriting
was unmistakable.
```

### 5. Analyze your manuscript

```bash
laires scan
```

This sends each scene to the LLM, which extracts characters, objectives, conflicts, and scene metadata into the narrative graph.

### 6. Explore

```bash
laires graph                    # print full graph summary
laires graph --character elena  # show a specific character's arc
laires chat                     # interactive agent chat
laires gui                      # launch the desktop GUI
```

## Commands

| Command | Description |
|---------|-------------|
| `laires init --title "Title"` | Initialize a new project |
| `laires init --title "Title" --fountain` | Initialize as a screenplay project |
| `laires scan` | Analyze all scenes with the LLM |
| `laires scan --scene 3` | Re-analyze only scene 3 |
| `laires graph` | Print narrative graph summary |
| `laires graph --character name` | Show a character's arc |
| `laires graph --json` | Output graph as JSON |
| `laires status` | Show project stats |
| `laires chat` | Interactive agent chat |
| `laires open` | Split-pane TUI with overlays |
| `laires gui` | Native desktop GUI |
| `laires lint` | Run consistency checks |
| `laires diff` | Show graph changes since last commit |
| `laires log` | Commit history with graph diffs |
| `laires perspective <char>` | View a scene through a character's eyes |
| `laires brief` | View or list revision briefs |
| `laires convert <file>` | Convert .docx/.txt to .md |

## Configuration

The config file at `.laires/config.toml` is created by `laires init`:

```toml
[llm]
provider = "gemini"
model = "gemini-2.5-flash"
api_key_env = "GEMINI_API_KEY"
base_url = "https://generativelanguage.googleapis.com/v1beta/openai"

[project]
title = "My Novel"
format = "prose"

[analysis]
debounce_ms = 2000
auto_scan = true

[privacy]
restricted_when_cloud = []
```

### Supported providers

| Provider | `provider` value | `api_key_env` | `base_url` |
|----------|-----------------|---------------|------------|
| Google Gemini | `"gemini"` | `GEMINI_API_KEY` | `https://generativelanguage.googleapis.com/v1beta/openai` |
| Anthropic | `"anthropic"` | `ANTHROPIC_API_KEY` | `https://api.anthropic.com` |
| OpenAI | `"openai"` | `OPENAI_API_KEY` | `https://api.openai.com/v1` |
| Local (Ollama, etc.) | `"local"` | — | `http://localhost:11434/v1` |

To switch providers, edit `.laires/config.toml` and set your API key in `.env`.

## Scene detection

Laires automatically detects scene boundaries in your manuscript.

**Prose mode** (Markdown) recognizes:
- Markdown headings: `## Chapter 1`, `### The Arrival`
- Horizontal rules: `---`, `***`, `___`
- HTML comment markers: `<!-- scene: "The Confrontation" -->`

**Fountain mode** (screenplays) recognizes:
- Scene headings: `INT. COFFEE SHOP - DAY`, `EXT. PARKING LOT - NIGHT`

## Project structure

```
my-novel/
├── .env                    # API keys (git-ignored)
├── story.md                # your manuscript
├── .laires/
│   ├── config.toml         # project configuration
│   ├── graph.json          # narrative graph
│   ├── scenes.json         # scene boundary map
│   ├── skills/             # custom TOML skill definitions
│   └── cache/              # LLM analysis cache (git-ignored)
└── .gitignore
```

## Development

```bash
cargo build              # build (debug)
cargo test               # run tests
cargo run -- status      # run without installing
cargo fmt                # format code
cargo clippy             # lint
```

### Rebuilding after code changes

Laires is a compiled Rust binary, so code edits require a rebuild before they take effect.

**Quick test** — build and run directly:

```bash
cargo build && ./target/debug/laires gui
```

**Install globally** — compile an optimized build and install to `~/.cargo/bin/`:

```bash
cargo build --release && cargo install --path .
```

After this you can run `laires gui` from any directory.

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on submitting changes.

## Architecture

Laires is built on the **Concept & Synchronization** pattern (Jackson & Meng, MIT CSAIL). The system is composed of 10 independent concepts — each with its own state, actions, and invariants — coordinated through explicit synchronizations.

See `context/laires-spec.md` for the full technical specification.

## License

[MIT](LICENSE)
