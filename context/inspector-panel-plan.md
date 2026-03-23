# Inspector Panel — Implementation Plan

## Context

The Graph tab currently shows an interactive force-directed graph with a small 260x90px info panel in the bottom-right corner when a node is clicked. This panel shows only the node label, type badge, and connection count. The inspector replaces this with a rich, scrollable detail panel that shows all available data for the selected node — fields, relationships, and connected edges.

## Design

### Layout: Split Graph Tab

When a node is selected, the Graph tab splits:

```
┌──────────────────────────────────────────────────────────────┐
│  [Dashboard]  [Canvas]  [Graph]  [Brief/Lint/Pacing]         │
├──────────────────────────────────┬───────────────────────────┤
│                                  │  Inspector                │
│     Force-directed graph         │                           │
│     (pan/zoom/click)             │  ● CHARACTER              │
│                                  │  Marcus Aurelius           │
│                                  │  ─────────────────────    │
│                                  │  Aliases: Marc, The Gen.  │
│                                  │  A Roman general seeking  │
│                                  │  to reclaim his honor...  │
│                                  │  ─────────────────────    │
│                                  │  CONNECTIONS (8)          │
│                                  │  Pursues ─────────────    │
│                                  │    → Reclaim the throne   │
│                                  │    → Protect Elena        │
│  Legend (bottom-left)            │  Present in ──────────    │
│  ● Char ● Obj ● Scene ● Conf    │    → Scene 3: The Feast   │
│                                  │    → Scene 7: Betrayal    │
└──────────────────────────────────┴───────────────────────────┘
```

- Graph area: ~65% width (shrinks from 100%)
- Inspector panel: ~35% width (~280px), right side, scrollable
- When no node is selected: graph fills full width (current behavior)
- Clicking a connected node in the inspector selects that node (navigation)

### Data Flow: Enrich ProjectSnapshot

Currently `ProjectSnapshot.graph_nodes` stores only `GraphNodeInfo { id, label, node_type }`. The inspector needs full node data.

**Change**: Replace `Vec<GraphNodeInfo>` with `Vec<SnapshotNode>` that carries all fields:

```rust
pub struct SnapshotNode {
    pub id: String,
    pub label: String,        // Short display label (existing truncation logic)
    pub node_type: String,    // "character", "objective", "scene", "conflict"
    pub detail: NodeDetail,   // Full data for inspector
}

pub enum NodeDetail {
    Character {
        name: String,
        aliases: Vec<String>,
        description: Option<String>,
    },
    Objective {
        character_id: String,
        scope: String,          // "Overarching" / "Act" / "Scene"
        description: String,
        evidence: Vec<String>,
        confidence: f64,
        status: String,         // "Active" / "Achieved" / etc.
    },
    Scene {
        title: Option<String>,
        summary: String,
        characters_present: Vec<String>,
        location: Option<String>,
        time: Option<String>,
        file_path: String,
    },
    Conflict {
        description: String,
        objectives: Vec<String>,
    },
}
```

This stays lock-free — `build_snapshot()` already reads the full `GraphNode` to extract labels, so cloning the extra fields is trivial. The `SnapshotNode` keeps the existing `id`/`label`/`node_type` fields so graph_view.rs needs minimal changes.

### Inspector Panel Content Per Node Type

**Character**:
- Type badge (indigo dot + "CHARACTER")
- Name (large)
- Aliases (if any)
- Description
- Connections section

**Objective**:
- Type badge (green dot + "OBJECTIVE")
- Description (large)
- Owner character (clickable)
- Scope badge (Overarching / Act / Scene)
- Status badge (Active / Achieved / Abandoned / Blocked / Transformed)
- Confidence bar (0-100%)
- Evidence quotes (collapsible)
- Connections section

**Scene**:
- Type badge (amber dot + "SCENE")
- Title (large)
- Summary
- Location + Time (if present)
- File path
- Characters present (each clickable)
- Connections section

**Conflict**:
- Type badge (red dot + "CONFLICT")
- Description (large)
- Participating objectives (each clickable)
- Connections section

### Connections Section (all types)

Groups edges by type, showing the connected node's label as a clickable link:

```
CONNECTIONS (8)
─────────────
Pursues →
  Reclaim the throne          [click to select]
  Protect Elena               [click to select]
Present in →
  Scene 3: The Feast          [click to select]
  Scene 7: Betrayal           [click to select]
Advances ←
  Scene 12: The Duel          [click to select]
```

Arrow direction: `→` for outgoing edges, `←` for incoming.

---

## Files Changed

### Modified

| File | Changes |
|------|---------|
| `src/gui/panels/graph_view.rs` | Move `GraphNodeInfo` → `SnapshotNode` with `NodeDetail`. Remove `render_info_panel()`. Split render area when inspector is active. |
| `src/gui/mod.rs` | Update `build_snapshot()` to populate `SnapshotNode.detail` from `GraphNode` fields. Update `ProjectSnapshot` field type. |
| `src/gui/panels/mod.rs` | Add `pub mod inspector;` |

### New

| File | Purpose |
|------|---------|
| `src/gui/panels/inspector.rs` | Inspector panel: `render()` function, per-type detail rendering, connections section with clickable navigation |

---

## Implementation Phases

### Phase 1: Enrich Snapshot Data

1. In `graph_view.rs`: rename `GraphNodeInfo` to `SnapshotNode`, add `detail: NodeDetail` field and the `NodeDetail` enum
2. In `mod.rs` `build_snapshot()`: populate `NodeDetail` from `GraphNode` variant fields
3. Update all `GraphNodeInfo` references (graph_view.rs rendering, dashboard.rs if any)
4. Verify: `cargo build` — no behavior change yet

### Phase 2: Create Inspector Panel

1. Create `src/gui/panels/inspector.rs` with:
   - `pub fn render(ui, state, snapshot, theme)` — main entry point
   - `render_character()`, `render_objective()`, `render_scene()`, `render_conflict()` — per-type detail sections
   - `render_connections()` — shared edge listing with clickable navigation
2. Register in `panels/mod.rs`
3. Verify: compiles but not wired in yet

### Phase 3: Wire Into Graph Tab

1. In `graph_view.rs`: remove `render_info_panel()` call
2. In `mod.rs` tab dispatch for `RightTab::Graph`: split the area horizontally when `selected_node_id.is_some()` — graph on left, inspector on right (wrapped in `card_frame()`)
3. Inspector click on a connected node sets `state.selected_node_id` → graph highlights new node, inspector updates
4. Verify: full interactive flow works

### Phase 4: Polish

1. Scroll long content (evidence lists, many connections)
2. Deselect on clicking empty graph space (already works via existing click logic)
3. Confidence bar rendering (horizontal progress bar)
4. Status/scope badges (colored pills)
5. Verify: `cargo test` — 196 tests still pass

---

## Design Decisions

| Decision | Choice | Why |
|----------|--------|-----|
| Inspector location | Split within Graph tab | No tab bar clutter; inspector is contextual to graph interaction |
| Data source | Enriched snapshot (lock-free) | Consistent with existing pattern; build_snapshot already reads full nodes |
| New tab vs split | Split pane | Inspector is a detail view of graph selection, not an independent view |
| Clickable connections | Set selected_node_id | Enables graph navigation without leaving the view |
| SnapshotNode vs separate struct | Combined struct | Keeps id/label/node_type for graph rendering + detail for inspector in one place |
