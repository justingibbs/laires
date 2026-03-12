Rebuild:
cargo build --release

Reinstall:
cargo install --path .

Version:
laires --version

Open terminal GUI:
laires open

Open full GUI:
cargo run -- gui

Scan the folder:
laires scan

---

need to check on LLM language part of the app.

---

# Need to address stuffing context

  Issue 2: Chat Not Working

  The chat was working earlier (the long response about "Part Five: The Names" is visible). The 4 recent user messages got no visible response. Since you
  were able to send all 4 (which requires AgentStatus::Idle each time), the agent is likely responding — but with empty content.

  The most probable cause: the Gemini API is returning empty responses or an error that produces empty content. In the agent loop
  (src/gui/agent.rs:174-198), when the LLM returns OK with no tool calls:

  let text = response.content.unwrap_or_default(); // could be ""
  let _ = events.send(AgentEvent::Response(text.clone()));
  let _ = events.send(AgentEvent::Idle);

  If content is None or empty, an invisible empty assistant message is added to chat history and the agent goes Idle, allowing the next send. This would
  create exactly the pattern you see — user messages with no visible responses.

  Possible root causes:
  - API quota/rate limit — Gemini may be rejecting requests after the initial conversation used up quota
  - Context too large — the narrative graph JSON (46KB in graph.json) is prepended to every chat request. Combined with the LLM history and the 22+ tool
  schemas, this could exceed Gemini's context limit, causing truncated/empty responses
  - Message format issue — the OpenAI-compatible endpoint may not handle the accumulated multi-turn conversation correctly

  Would you like me to investigate and fix these? I'd suggest:
  1. For Scenes: Update ProjectData to use FileBufferManager and update build_snapshot() to pull scenes from all files
  2. For Chat: Add logging/error visibility when the LLM returns empty content, and investigate the context size being sent to Gemini


---


 1. VCS Integration                                        
                                                                                                                                                           
  Implement VCS integration for Laires (Phase 3). This adds two new CLI subcommands: `laires diff` and `laires log`.

  ## What to build

  ### `laires diff`
  - Detect whether the project uses git or jj (check for .git/ or .jj/ dirs)
  - Compare `.laires/graph.json` across the current working tree vs the last commit
  - Output two sections:
    1. **Text changes**: per-scene word count deltas (e.g. `Scene 7 ("The Banquet"): +142 words, -38 words`)
    2. **Graph changes**: node/edge mutations — added/removed/changed characters, objectives, conflicts, status changes

  ### `laires log`
  - Show commit/change history with graph change summaries per commit
  - For each commit that touched `.laires/graph.json`, compute and display the graph diff

  ### Design constraints
  - VCS is opt-in — commands should error gracefully if no git/jj is found
  - Use `std::process::Command` to shell out to `git` or `jj` (no git2 crate needed)
  - Graph diffs are computed by deserializing two versions of `NarrativeGraph` and comparing nodes/edges

  ## Existing code context
  - CLI dispatch is in `src/cli/mod.rs` — add `Diff` and `Log` variants to the `Commands` enum
  - `NarrativeGraph` is in `src/concepts/narrative_graph.rs` — has `load()`, `save()`, `serialize_compact()`, serde support
  - `ProjectConfig::load()` and `find_project_root()` are in `src/config.rs`
  - Graph is stored at `.laires/graph.json`
  - Follow the pattern of existing subcommands (e.g. `src/cli/status.rs`, `src/cli/scan.rs`)
  - Create `src/cli/diff.rs` and `src/cli/log.rs`

  ## Verification
  - `cargo build` compiles
  - `cargo test` — all existing 116 tests pass plus new tests
  - Add tests for: graph diff computation (added/removed/changed nodes), graceful error when no VCS present

  ---
  2. TUI Overlays

  Implement TUI overlay panels for Laires (Phase 3). These are modal overlays triggered by keybindings in the split-pane TUI (`laires open`).

  ## What to build

  Add 5 overlay modes to the existing TUI in `src/cli/tui.rs`:

  1. **File Explorer** (`Ctrl+E`) — list files from the manifest with roles, word counts, scene counts
  2. **Story Lint** (`Ctrl+L`) — display results from the `story_lint` skill (timeline errors, presence inconsistencies, continuity issues)
  3. **Pacing Analysis** (`Ctrl+P`) — display results from `pacing_analysis` skill (scene length distribution, conflict density, rhythm)
  4. **Graph Overlay** (`Ctrl+G`) — render NarrativeGraph structure (characters, scenes, edges) as text
  5. **Status Toggle** (`Ctrl+/`) — toggle between expanded stats and minimal status bar

  Each overlay should:
  - Render as a full-pane overlay on top of the canvas pane (right side)
  - Dismiss with `Esc` to return to normal view
  - Be non-interactive (read-only display)

  ## Existing code context
  - TUI is in `src/cli/tui.rs` — uses ratatui, already has split-pane layout with chat (left) + canvas (right), keybinding handling, and overlay
  infrastructure (`TuiOverlay` enum, `Ctrl+G` already partially wired)
  - Skills are in `src/concepts/skills.rs` — `story_lint`, `pacing_analysis`, `arc_completeness` are already implemented and return structured data
  - `FileBufferManager` in `src/concepts/file_buffer_manager.rs` has `entries()`, `story_file_count()`, per-file stats
  - `NarrativeGraph` in `src/concepts/narrative_graph.rs` has `get_characters()`, `get_conflicts()`, `summary()`
  - Manifest in `src/concepts/manifest.rs` has file role info

  ## Verification
  - `cargo build` compiles
  - `cargo test` — all existing 116 tests pass plus new tests
  - Add tests for: overlay state transitions, keybinding dispatch, render output for each overlay mode

  ---
  3. Custom Skills Framework

  Implement the custom skills framework for Laires (Phase 3). This lets writers register their own skills from `.laires/skills/` without modifying core
  code.

  ## What to build

  ### Skill definition format
  - Custom skills are defined as TOML files in `.laires/skills/` (e.g. `.laires/skills/check_dialect.toml`)
  - Each file defines: name, description, category, input_schema (JSON Schema), output_schema (JSON Schema), prompt_template (the LLM prompt with
  `{{input}}` placeholders)
  - Custom skills are LLM-powered — they send the prompt template + user input to the Provider and return the result

  ### Registration and loading
  - On startup (when Skills is constructed), scan `.laires/skills/` for .toml files
  - Parse and register each as a SkillDefinition in the existing Skills registry
  - Validate schemas on load — reject malformed definitions with clear errors

  ### Permission system
  - Add `Permission` enum: `Enabled`, `Disabled`, `ConditionalOn(String)` (e.g. "provider.is_local")
  - `set_permission(skill_name, permission)` method on Skills
  - `ConditionalOn` checks: `"is_local"` gates on `Provider.is_local()`, `"is_cloud"` gates on `!Provider.is_local()`
  - Default permission for custom skills: `Enabled`

  ### Execution
  - Custom skills go through the same `Skills.invoke()` path as built-in skills
  - Permission check before dispatch
  - Log execution to an audit trail (append to `.laires/skill_log.jsonl`)

  ## Existing code context
  - Skills registry is in `src/concepts/skills.rs` — has `SkillDefinition`, `Skills` struct with `register()` and `invoke()`, `SkillContext` with
  provider/graph/scene_map access
  - Currently 19 built-in skills across 4 categories (FileTools, GraphTools, PerspectiveTools, StructuralTools)
  - `invoke()` is async, takes skill name + JSON args + `SkillContext`, returns `serde_json::Value`
  - Provider is in `src/concepts/provider.rs` — `complete()` for LLM calls, `is_local()` for privacy checks
  - Config paths in `src/config.rs` — add `SKILLS_DIR` constant

  ## Verification
  - `cargo build` compiles
  - `cargo test` — all existing 116 tests pass plus new tests
  - Add tests for: TOML parsing of skill definitions, schema validation, permission checks (enabled/disabled/conditional), registration into registry,
  execution logging