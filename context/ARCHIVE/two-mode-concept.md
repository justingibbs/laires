# Laires.ai — Two-Mode Writing Concept

## The Problem

Writers work in Word, Google Docs, Scrivener, Final Draft — they have established workflows and won't abandon them. But Laires's most powerful features (canvas tools, agent-driven revision, scene insertion) only work when Laires can write back to files. Round-tripping .docx is fragile and complex: styles, formatting, track changes, embedded images, and publisher templates make faithful write-back a losing battle.

**Insight**: Rather than trying to do one thing poorly (edit .docx in place), offer two modes that each do their job well.

---

## Mode 1: Consultant

**Tagline**: "Bring your files. Get a revision brief."

The writer's files stay untouched. Laires reads them, builds the narrative graph, and the agent produces a structured **revision brief** — a document the writer takes back to their own editor.

### Workflow

```
Writer's files (.docx, .txt, .md, .fountain, any mix)
        │
        ▼
   laires init / scan
        │
        ▼
   Narrative graph built
        │
        ▼
   Agent conversation (chat / GUI)
   ├── Analysis: graph queries, lint, pacing, perspectives
   ├── Exploration: "What if Sarah betrays Marcus in Scene 12?"
   └── Revision planning: agent proposes specific changes
        │
        ▼
   laires brief [--format md|docx]
        │
        ▼
   Revision Brief (exported file)
```

### The Revision Brief

The revision brief is Laires's primary output in Consultant mode. It is a structured, scene-anchored document that gives the writer specific, actionable instructions they can execute in their own editor.

#### Format

```markdown
# Revision Brief — {project_title}
Generated: {date}
Based on: {conversation_summary or agent session ID}

---

## Overview

{2-3 sentence summary of what this revision addresses}

**Focus**: {e.g., "Character arc consistency for Sarah", "Pacing in Act 2",
            "Conflict escalation between Marcus and Elena"}
**Scope**: {e.g., "Scenes 8-14", "Full manuscript", "Chapter 3"}
**Priority**: {High / Medium / Low}

---

## Revisions

### Scene {N}: {scene_title}
**File**: {filename}, lines {start}-{end}
**Priority**: {High / Medium / Low}

**Current state**:
> {Brief excerpt or summary of what's there now — enough for the writer
> to locate the passage in their editor}

**Issue**:
{What the analysis found — e.g., "Sarah's reaction contradicts her
established objective of protecting Marcus. She has no motivation shift
to justify this reversal."}

**Suggested revision**:
{Specific, actionable instruction — not vague advice. e.g., "Add 2-3
sentences before Sarah's dialogue where she processes the letter from
Scene 6. Her shift from protection to self-preservation needs a visible
trigger. Consider her discovering that Marcus knew about the betrayal
all along."}

**Draft passage** (optional):
> {If the agent wrote a draft during conversation, include it here as
> a starting point the writer can adapt to their voice}

**Graph impact**:
- Sarah's objective shifts: `protect_marcus` → `self_preservation` (Scene {N})
- New conflict edge: Sarah ↔ Marcus (active from Scene {N})
- Resolves dead scene flag on Scene {N-1}

---

### Scene {M}: {scene_title}
...

---

## Structural Notes

{Broader observations that don't map to a single scene}

- **Pacing**: {e.g., "Scenes 8-11 are all dialogue-heavy interior scenes.
  Consider breaking this run with an action beat or location change."}
- **Arc completeness**: {e.g., "Marcus's political subplot has no scene
  advancing it between Scene 5 and Scene 19 — 14-scene gap."}
- **Timeline**: {e.g., "Scene 12 references 'last Tuesday' but Scene 10
  establishes the current day as Monday."}

---

## Checklist

- [ ] Scene {N}: {one-line summary of the revision}
- [ ] Scene {M}: {one-line summary}
- [ ] Structural: {one-line summary}
- ...
```

#### Design Principles for the Brief

1. **Scene-anchored**: Every revision references a specific scene, file, and line range. The writer should be able to find the passage immediately in their editor.
2. **Specific, not vague**: "Add a motivation shift before Sarah's dialogue" not "Consider developing Sarah's character more."
3. **Respects the writer's voice**: Draft passages are offered as starting points, clearly marked as optional. The brief tells you *what* to change and *why*, but the writer decides *how*.
4. **Graph-aware**: Each revision notes its impact on the narrative graph — what edges change, what flags resolve. This helps the writer understand structural consequences.
5. **Prioritized**: Not everything is equally urgent. High-priority items are structural problems (contradictions, dead scenes). Low-priority items are polish suggestions.
6. **Checklistable**: The writer can print it out or open it alongside their editor and work through it.

#### Export

- **Markdown** (default): Clean .md file the writer can open anywhere
- **Word (.docx)**: Generated as a *new* simple document (not modifying the writer's files). Basic formatting only — headings, blockquotes, bullet lists. This is a fresh .docx, so no round-trip complexity.

---

## Mode 2: Workshop

**Tagline**: "Write in Laires. The agent is your co-editor."

The writer works directly in Laires using **Markdown** or **Fountain** files. Canvas tools make real edits to real files. The agent can draft scenes, revise passages, and restructure — all live, all version-controlled.

### Supported Formats

- **Markdown (.md)**: For prose fiction — novels, short stories, creative nonfiction
- **Fountain (.fountain)**: For screenplays and teleplays (Fountain is already plain text / Markdown-adjacent)

Both are plain text, git-diffable, and have existing SceneMap parsing support.

### Workflow

```
Writer creates/imports .md or .fountain files
        │
        ▼
   laires init / scan
        │
        ▼
   Narrative graph built
        │
        ▼
   Agent conversation (chat / GUI)
   ├── Analysis (same as Consultant mode)
   ├── Live editing via canvas tools:
   │   ├── write_to_canvas — append/write new content
   │   ├── replace_in_canvas — revise existing passages
   │   └── insert_scene — add a new scene at a position
   └── Writer reviews changes in canvas, accepts/rejects
        │
        ▼
   Files are modified in place
   Git/JJ tracks all changes (laires diff / laires log)
```

### Key Differences from Consultant Mode

| Aspect | Consultant | Workshop |
|--------|-----------|----------|
| File formats | Any (.docx, .txt, .md, .fountain) | .md and .fountain only |
| Files modified | Never | Yes, via canvas tools |
| Agent output | Revision brief (exported document) | Direct file edits |
| Version control | Optional (writer's own VCS) | Recommended (git/jj for change tracking) |
| Primary use case | Analysis & revision planning | Drafting & co-editing |
| Canvas tools | Read-only display | Full read/write |

---

## How the Two Modes Coexist

A single Laires project can contain both read-only imported files and editable Markdown/Fountain files. The manifest already tracks per-file metadata — adding an `editable` flag or inferring it from format is straightforward.

```
project/
├── novel-draft.docx          # Consultant mode — read-only, brief output
├── chapter-revision.md       # Workshop mode — agent can edit directly
├── screenplay.fountain       # Workshop mode — agent can edit directly
└── .laires/
    ├── manifest.toml          # tracks format, role, editability per file
    └── briefs/                # generated revision briefs
        └── 2026-03-15-arc-revision.md
```

A writer might import their .docx, run analysis in Consultant mode, then start a new .md file in Workshop mode to draft a revised chapter — using the brief as their guide. The narrative graph spans all files regardless of mode.

---

## Implementation Notes

### New Components

1. **`RevisionBrief` struct**: Collects scene-anchored revisions during an agent conversation. The agent populates this through a new `add_to_brief` skill or by structuring its final output.

2. **`laires brief` CLI command**: Exports the current brief to .md or .docx.
   - `laires brief` — export as Markdown (default)
   - `laires brief --format docx` — export as Word
   - `laires brief --output path/to/file` — specify output path
   - Without explicit export, briefs are saved to `.laires/briefs/`

3. **Brief generation skill**: A new built-in skill that the agent calls to structure its revision suggestions into brief format. This can be invoked explicitly ("generate a revision brief") or automatically at the end of a consultation session.

4. **Simple .docx writer**: For brief export only. Generates a basic Word document with headings, blockquotes, and lists. No need to preserve existing formatting — this is a new document. Could use a lightweight crate or raw XML generation (the .docx format for simple documents is just zipped XML).

### Existing Components That Support This

- **SceneMap**: Already provides scene boundaries, line ranges, and file references — exactly what the brief needs for anchoring.
- **NarrativeGraph**: `get_scene_analysis()`, `diff_graphs()` supply the "graph impact" section.
- **Canvas tools**: Already implemented for Workshop mode writes.
- **Manifest**: Already tracks per-file metadata; adding format-based editability is minimal.
- **`laires diff` / `laires log`**: Already provide VCS integration for Workshop mode change tracking.
- **.docx reader** (`docx.rs`): Already handles import. The .docx *writer* for briefs is a separate, simpler concern.

### What This Does NOT Require

- .docx round-trip editing (the hard problem we're avoiding)
- A new text editor component (egui canvas already exists)
- Changes to the narrative graph model
- Changes to the provider/LLM layer
