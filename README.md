# Laires.ai

An agentic writing tool that treats fiction manuscripts like codebases. Laires builds a **narrative graph** — a structured model of characters, objectives, conflicts, and scenes — and uses an LLM agent to provide structural analysis, perspective interpretation, consistency checking, and co-writing assistance.

## Prerequisites

- [Rust toolchain](https://rustup.rs/) (1.93+)
- A Gemini API key (default) or other LLM provider key

## Installation

```bash
git clone <repo-url> && cd laires
cargo install --path .
```

This installs the `laires` binary to `~/.cargo/bin/`, which should already be on your `$PATH`.

## Quick Start

### 1. Create a project directory

```bash
mkdir my-novel && cd my-novel
```

Or test story example
```bash
cargo run -- gui ~/Projects/laires-test-story
```

### 2. Add your API key

Create a `.env` file in the project root:

```bash
echo 'GEMINI_API_KEY=your-key-here' > .env
```

### 3. Initialize the project

```bash
laires init --title "My Novel"
```

This creates:
- `story.md` — your manuscript (single file)
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

### 6. Explore the graph

```bash
laires graph                    # print full graph summary
laires graph --character elena  # show a specific character's arc
laires graph --json             # output raw JSON
```

### 7. Chat with the agent

```bash
laires chat
```

An interactive session where the agent has access to your narrative graph and can search/read your story. Ask questions like:
- "Where does Marcus's motivation break down?"
- "Which scenes lack a clear conflict?"
- "Summarize Elena's arc across all scenes"

Type `quit` to exit.

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

### Supported Providers

| Provider | `provider` value | `api_key_env` | `base_url` |
|----------|-----------------|---------------|------------|
| Google Gemini | `"gemini"` | `GEMINI_API_KEY` | `https://generativelanguage.googleapis.com/v1beta/openai` |
| Anthropic | `"anthropic"` | `ANTHROPIC_API_KEY` | `https://api.anthropic.com` |
| OpenAI | `"openai"` | `OPENAI_API_KEY` | `https://api.openai.com/v1` |
| Local (Ollama, etc.) | `"local"` | — | `http://localhost:11434/v1` |

To switch providers, edit `.laires/config.toml` and set your API key in `.env`.

## Scene Detection

Laires automatically detects scene boundaries in your manuscript.

**Prose mode** (Markdown) recognizes:
- Markdown headings: `## Chapter 1`, `### The Arrival`
- Horizontal rules: `---`, `***`, `___`
- HTML comment markers: `<!-- scene: "The Confrontation" -->`

**Fountain mode** (screenplays) recognizes:
- Scene headings: `INT. COFFEE SHOP - DAY`, `EXT. PARKING LOT - NIGHT`

## Project Structure

```
my-novel/
├── .env                    # API keys (git-ignored)
├── story.md                # your manuscript
├── .laires/
│   ├── config.toml         # project configuration
│   ├── graph.json          # narrative graph
│   ├── scenes.json         # scene boundary map
│   └── cache/              # LLM analysis cache (git-ignored)
└── .gitignore
```

## Development

```bash
cargo build              # build
cargo test               # run tests (19 tests)
cargo run -- status      # run without installing
```

## Architecture

Laires is built on the **Concept & Synchronization** pattern (Jackson & Meng, MIT CSAIL). The system is composed of 10 independent concepts — each with its own state, actions, and invariants — coordinated through explicit synchronizations.

See `context/laires-spec.md` for the full technical specification.

## License

MIT
