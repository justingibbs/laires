# Laires Refactor Plan

## Context

The current codebase has three related problems:

1. The baseline is not consistently green. `cargo test -q` currently fails in the GUI path.
2. Multi-file story support exists in `scan`, but chat, GUI, TUI, and skill execution still rely heavily on a single `TextBuffer` and `SceneMap`.
3. Core runtime behavior is duplicated across CLI chat, TUI, and GUI, which has already caused drift.

This plan prioritizes correctness first, then shared abstractions, then file/module decomposition.

## Goals

- Restore a green baseline before larger changes.
- Make story access consistently multi-file-aware across scan, chat, GUI, and TUI.
- Eliminate duplicated project bootstrapping and agent-loop logic.
- Separate tool registry, permission policy, custom skill loading, and skill execution.
- Preserve current user-visible behavior where possible while improving internal structure.

## Non-Goals

- Rewriting the GUI or TUI design.
- Changing provider APIs or prompt strategy unless required by extraction work.
- Large feature additions during refactor phases.

## Refactor Principles

- Prefer extracting stable seams over broad rewrites.
- Each phase should leave the project building and testable.
- Move from tuples and ad hoc state bags toward named structs with explicit responsibilities.
- Multi-file support should be modeled once and consumed everywhere.
- Shared runtime logic should live in reusable services, not in UI entrypoints.

## Target Architecture

### 1. Shared Workspace Model

Introduce a workspace/domain layer that represents the loaded project once.

Candidate shape:

```rust
pub struct ProjectWorkspace {
    pub project_root: PathBuf,
    pub config: ProjectConfig,
    pub manifest: Option<Manifest>,
    pub graph: NarrativeGraph,
    pub intent: DeclaredIntent,
    pub perspectives: CharacterPerspective,
    pub story_store: StoryStore,
}

pub enum StoryStore {
    Single {
        text_buffer: TextBuffer,
        scene_map: SceneMap,
    },
    Multi {
        file_buffers: FileBufferManager,
    },
}
```

The key point is not the exact type names. The key point is that all story-reading and scene-reading operations route through one abstraction instead of passing raw `TextBuffer` and `SceneMap` through the application.

### 2. Shared Runtime Services

Extract two services:

- `ProjectLoader`
  - Finds the project root
  - Loads config, graph, intent, perspectives, manifest, story store, provider, and skills
  - Returns a named result struct, not an 8-value tuple

- `AgentSession`
  - Builds context
  - Manages tool turns
  - Invokes skills
  - Produces structured events for CLI, TUI, and GUI frontends

### 3. Skill Modules by Concern

Split `skills.rs` into:

- `skills/mod.rs`
- `skills/registry.rs`
- `skills/permissions.rs`
- `skills/custom.rs`
- `skills/file_tools.rs`
- `skills/graph_tools.rs`
- `skills/perspective_tools.rs`
- `skills/structural_tools.rs`
- `skills/canvas_tools.rs`

Keep a small public facade so the rest of the codebase does not care how tools are organized internally.

## Phased Plan

### Phase 0: Stabilize The Baseline

Goal: get the repository building and add a minimal guardrail before extracting anything.

Scope:

- Fix the moved-value error in `src/gui/mod.rs`.
- Address any additional compile errors surfaced after that fix.
- Run `cargo test`.
- If tests are too slow for every iteration, add a lighter smoke target and document it.

Concrete changes:

- Replace large tuple destructuring + field moves with a named load result struct.
- Fix the current `model_name` ownership issue as part of that conversion.
- Remove obvious stale warnings where they obscure real regressions.

Validation:

- `cargo test`
- `cargo build`

Exit criteria:

- Main branch compiles cleanly.
- Refactor work starts from a green baseline.

### Phase 1: Introduce `ProjectLoadResult`

Goal: remove tuple-based loading and centralize bootstrapping logic without changing behavior.

Scope:

- Extract the shared project-loading flow from:
  - `src/cli/chat.rs`
  - `src/cli/tui.rs`
  - `src/gui/mod.rs`
- Create a new loader module, likely `src/runtime/project_loader.rs` or `src/app/project_loader.rs`.

Suggested types:

```rust
pub struct ProjectLoadResult {
    pub workspace: ProjectWorkspace,
    pub provider: Provider,
    pub skills: Skills,
    pub ui_summary: WorkspaceSummary,
}

pub struct WorkspaceSummary {
    pub privacy_label: String,
    pub model_name: String,
    pub scene_count: usize,
    pub char_count: usize,
    pub word_count: usize,
}
```

Tasks:

- Move repeated load logic into a single module.
- Make GUI, CLI chat, and TUI call the loader instead of duplicating startup.
- Preserve current CLI/GUI behavior while changing only construction.

Validation:

- `cargo test`
- Manual smoke:
  - `cargo run -- chat --new-session`
  - `cargo run -- open`
  - `cargo run -- gui <project>`

Exit criteria:

- No loading code path manually reconstructs the same domain objects from scratch.
- No more tuple returns for project startup.

### Phase 2: Unify Story Access Behind `StoryStore`

Goal: make story tools and runtime behavior correctly handle multi-file projects.

Scope:

- Replace direct `TextBuffer` and `SceneMap` dependence in tool execution with a story abstraction.
- Ensure scan, chat, GUI, and TUI all read scenes and stats from the same source model.

Suggested API:

```rust
pub trait StoryAccess {
    fn read_scene(&self, scene_ref: &str) -> anyhow::Result<StoryScene>;
    fn list_scenes(&self) -> Vec<StorySceneSummary>;
    fn story_stats(&self, graph: &NarrativeGraph) -> StoryStats;
    fn grep(&self, pattern: &str) -> anyhow::Result<Vec<SearchHit>>;
    fn full_text_for_canvas(&self) -> Option<String>;
}
```

Tasks:

- Add `file_buffer_manager` or equivalent to the skill context via the workspace abstraction.
- Refactor `story_grep`, `read_scene`, `list_scenes`, and `story_stats` to operate on `StoryStore`.
- Update chat and GUI agent code to source pending scenes and scene counts from the unified store.
- Decide how the TUI should behave for multi-file projects:
  - Option A: support it fully.
  - Option B: explicitly present a limited single-file view and say so in the UI.

Recommended choice:

- Support multi-file stories in runtime logic, even if the TUI initially renders only one active file at a time.

Validation:

- Add tests covering:
  - `read_scene` across multiple files
  - `list_scenes` ordering across manifest order
  - `story_grep` returning file-aware results
  - `story_stats` using aggregate counts

Exit criteria:

- The same story model powers scan, chat, GUI, and TUI.
- Tool answers are no longer silently incomplete on multi-file projects.

### Phase 3: Fix Scene Persistence Semantics

Goal: make scene persistence compatible with multi-file analysis.

Problem today:

- `scan` loops over per-file `SceneMap`s and saves each one to the same `scenes.json`.

Decisions required:

1. Store one aggregated scene-cache file keyed by file path.
2. Store one scene-cache file per story file.
3. Stop persisting scene maps until there is a clear consumer.

Recommended choice:

- Use one aggregated cache file keyed by file path, because it matches the existing manifest-based worldview and keeps project metadata centralized.

Suggested type:

```rust
pub struct SceneCache {
    pub files: Vec<FileSceneCache>,
}

pub struct FileSceneCache {
    pub file_path: String,
    pub parse_mode: ParseMode,
    pub scenes: Vec<SceneSpan>,
    pub pending_reindex: HashSet<SceneId>,
    pub character_cues: Vec<CharacterCue>,
}
```

Tasks:

- Add a new scene-cache module or extend `SceneMap` with conversion helpers.
- Update `scan` to save one aggregate file.
- Update any future scene-cache consumers to read the new format.
- If no consumer exists yet, document the cache as internal and test only write/read round-tripping.

Validation:

- Unit test aggregate cache serialization/deserialization.
- Integration test scan persistence for a two-file manifest.

Exit criteria:

- Multi-file scan output no longer overwrites itself.

### Phase 4: Extract `AgentSession`

Goal: remove drift between CLI chat, GUI agent, and TUI agent behavior.

Scope:

- Consolidate context assembly, tool-loop orchestration, usage tracking, history summarization, divergence inclusion, and tool result handling.

Suggested shape:

```rust
pub struct AgentSession {
    provider: Provider,
    skills: Skills,
    history: Vec<Message>,
}

pub struct AgentTurnInput<'a> {
    pub user_input: &'a str,
    pub workspace: &'a mut ProjectWorkspace,
    pub mode: AgentMode,
}

pub enum AgentEvent {
    Thinking,
    ToolCall { name: String, args_summary: String },
    ToolResult { name: String, result_summary: String },
    Response(String),
    Error(String),
    UsageReport { ... },
    StateChanged,
    Idle,
}
```

Tasks:

- Move the shared agent loop mechanics out of:
  - `src/cli/chat.rs`
  - `src/cli/tui.rs`
  - `src/gui/agent.rs`
- Keep frontend-specific responsibilities outside:
  - terminal rendering
  - egui state updates
  - keyboard handling

Design rule:

- Frontends should supply input and render events.
- `AgentSession` should own the policy of how an LLM turn is executed.

Validation:

- Add at least one test that exercises tool-turn loop behavior without a UI.
- Manual smoke on CLI chat, TUI, and GUI after extraction.

Exit criteria:

- There is one implementation of context assembly and multi-turn tool handling.
- Tool availability and context compression do not vary accidentally by frontend.

### Phase 5: Split `skills.rs`

Goal: reduce change surface and isolate tool families.

Scope:

- Keep external `Skills` API stable at first.
- Move implementations behind internal modules.

Order:

1. Extract `registry` and schema definitions.
2. Extract permission policy.
3. Extract custom-skill loading and audit logging.
4. Extract built-in tool families one by one.
5. Replace large `match skill_name` blocks with delegated family dispatch.

Possible end state:

```rust
match registry.lookup(skill_name) {
    SkillKind::File(tool) => file_tools::invoke(tool, args, ctx),
    SkillKind::Graph(tool) => graph_tools::invoke(tool, args, ctx),
    SkillKind::Perspective(tool) => perspective_tools::invoke(tool, args, ctx, provider).await,
    SkillKind::Structural(tool) => structural_tools::invoke(tool, args, ctx),
    SkillKind::Canvas(tool) => canvas_tools::invoke(tool, args, ctx),
    SkillKind::Custom(name) => custom::invoke(name, args, provider).await,
}
```

Validation:

- Keep or expand current unit tests while moving modules.
- Require no behavior change before any optional cleanup.

Exit criteria:

- No single file contains registry definition, custom loader, permission logic, and all tool implementations.

### Phase 6: UI-Specific Cleanup

Goal: clean up GUI/TUI code after shared services exist.

GUI opportunities:

- Extract snapshot-building helpers from `src/gui/mod.rs`.
- Replace repeated scene-boundary scans with a direct boundary-index helper.
- Move graph-view data shaping into a view-model module.

TUI opportunities:

- Separate application state, input handling, rendering, and agent integration.
- Fix file explorer counts so they are file-specific when manifest-backed projects are loaded.

Recommended order:

- GUI first, because it currently mixes app shell, domain loading, event processing, snapshot generation, and window startup.

Validation:

- `cargo test`
- Manual GUI and TUI smoke tests

Exit criteria:

- UI entrypoints are primarily composition code, not domain/runtime code.

## Testing Plan

Add or expand tests in parallel with each phase.

Priority test additions:

- Loader tests for single-file and multi-file projects.
- Story access tests covering scene lookup, stats, and grep across files.
- Scene persistence round-trip tests.
- Agent-session tests for:
  - context assembly
  - tool-loop termination
  - history compaction
- Regression test for the current moved-value GUI load bug.

## Suggested File Layout After Refactor

```text
src/
├── runtime/
│   ├── mod.rs
│   ├── project_loader.rs
│   ├── agent_session.rs
│   ├── workspace.rs
│   └── scene_cache.rs
├── concepts/
│   ├── ...
│   └── skills/
│       ├── mod.rs
│       ├── registry.rs
│       ├── permissions.rs
│       ├── custom.rs
│       ├── file_tools.rs
│       ├── graph_tools.rs
│       ├── perspective_tools.rs
│       ├── structural_tools.rs
│       └── canvas_tools.rs
├── cli/
├── gui/
└── ...
```

## Execution Order

Recommended implementation order:

1. Phase 0
2. Phase 1
3. Phase 2
4. Phase 3
5. Phase 4
6. Phase 5
7. Phase 6

This order deliberately fixes correctness before maintainability-only cleanup.

## Risks

- The biggest risk is changing multi-file behavior while some frontends still assume a single active text buffer.
- Provider and agent extraction can accidentally change prompts, context size, or tool sequencing.
- Large module moves can create noisy diffs if done before shared abstractions are stable.

## Mitigations

- Keep each phase small and shippable.
- Preserve public method names where possible until tests are in place.
- Add tests before changing persistence formats.
- Do not combine workspace unification and skills-file splitting in the same PR.

## First PR Recommendation

The first PR should do only this:

1. Fix the GUI build break.
2. Introduce `ProjectLoadResult`.
3. Centralize shared startup code in a loader module.
4. Keep behavior unchanged.

That creates a safer platform for the larger multi-file and agent-session refactors that follow.
