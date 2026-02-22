# Laires.ai — Technical Specification

## Concept & Synchronization Architecture

---

## Product Summary

Laires.ai is a CLI/TUI agentic writing tool that treats fiction manuscripts like codebases. It works with whatever files the author has in their project folder — any mix of `.md`, `.fountain`, `.docx`, `.txt` — and builds a narrative graph: a structured model of characters, objectives, conflicts, and scenes spanning all story files. An LLM agent uses this graph alongside targeted text retrieval to provide structural analysis, perspective interpretation, consistency checking, and co-writing assistance.

The system is architected using the **Concept & Synchronization** pattern (Jackson & Meng, MIT CSAIL). Each piece of functionality is encapsulated as an independent concept with its own state, actions, and invariants. Concepts never call each other directly. All cross-concept coordination is expressed through explicit synchronizations — declarative rules that define exactly how concepts interact. The DSL definitions are authoritative; pseudocode accompanies each for human readability.

---

## Design Decisions (Resolved)

| Decision | Resolution |
|----------|------------|
| Product name | Laires.ai |
| Architecture pattern | Concept & Synchronization (Jackson & Meng) |
| Language | Rust |
| File format support | Multi-format: .md, .fountain, .docx, .txt — files read and edited in place |
| Screenplay support | Must-have at launch via Fountain (.fountain) |
| File structure | Multi-file projects; files classified by LLM and tracked in manifest.toml |
| Graph construction | Fully LLM-generated; writer can override with declared values |
| Graph updates | Automatic on file change; incremental via rope + scene map |
| Version control | Support both Git and JJ |
| TUI layout | Chat + Canvas split pane; graph/tools as overlays |
| LLM context strategy | Send graph as structured context + retrieve relevant scenes |
| Privacy | Clear cloud vs local indicator in status bar |
| Export formats | Deferred; multi-file reading and editing is the priority |
| Export metadata | Clean output only — no graph metadata in exports |
| Collaborative writing | Single author at launch |
| Character conversation mode | Deferred to later phase |
| GUI | Tauri desktop app in a future phase |

---

## Concepts

The system is composed of ten independent concepts. Each concept is defined with its state, actions, and invariants. Concepts are fully self-contained — they have no knowledge of each other. All inter-concept coordination happens through synchronizations (defined in the next section).

---

### Concept 1: TextBuffer

**Purpose**: Owns the raw text of a single file. A rope data structure supporting efficient insertions, deletions, and range reads. Knows nothing about scenes, characters, or narrative. Single source of truth for "what does the text say right now." In a multi-file project, a `FileBufferManager` coordinates multiple TextBuffer instances — one per file tracked in the manifest.

#### DSL Definition

```
concept TextBuffer
  state
    rope: Rope                          -- the text content
    file_path: Path                     -- location on disk
    dirty: Boolean                      -- unsaved changes exist
    change_log: List<ChangeEntry>       -- byte ranges changed since last checkpoint
    
  type ChangeEntry
    range: ByteRange
    kind: Insert | Delete | Replace
    timestamp: Timestamp

  action insert(position: ByteOffset, text: String)
    -- inserts text at position in the rope
    -- appends to change_log
    -- sets dirty = true
    pre: position <= rope.length
    post: rope.length = old(rope.length) + text.length
          dirty = true
          change_log.last.range = (position, position + text.length)

  action delete(range: ByteRange)
    -- removes text in range from the rope
    pre: range.end <= rope.length
    post: rope.length = old(rope.length) - range.length
          dirty = true

  action replace(range: ByteRange, text: String)
    -- replaces text in range with new text
    pre: range.end <= rope.length
    post: dirty = true

  action read(range: ByteRange) -> String
    -- returns text in the given byte range
    pre: range.end <= rope.length

  action read_all() -> String
    -- returns the entire rope content

  action save()
    -- writes rope content to file_path
    -- clears dirty flag
    post: dirty = false

  action load()
    -- reads file_path into rope
    -- clears change_log and dirty flag
    post: dirty = false
          change_log = []

  action checkpoint() -> ChangeLog
    -- returns accumulated changes and resets the log
    post: change_log = []

  invariant rope_file_consistency
    -- when dirty = false, rope content matches file on disk
    not dirty => rope.content = read_file(file_path)
```

#### Pseudocode

TextBuffer holds a single file's text in a rope (via the `ropey` crate). When text is inserted, deleted, or replaced, the change is recorded in a change log with the affected byte range. The `dirty` flag tracks whether unsaved changes exist. `save()` writes to disk and clears the flag. `checkpoint()` returns all accumulated changes since the last checkpoint and resets the log — this is how other concepts (via synchronization) learn what changed. TextBuffer knows nothing about scenes, characters, or narrative structure.

In a multi-file project, the `FileBufferManager` wraps multiple TextBuffer instances (one per story file in the manifest). It provides aggregate operations like searching across all buffers and routing edits to the correct buffer by file path. The FileBufferManager is a coordination layer, not a concept — it delegates all text operations to the underlying TextBuffers.

---

### Concept 2: SceneMap

**Purpose**: Answers "where are the scenes in this text?" Maintains an ordered index of scene boundaries as byte-range spans across all story files. Knows about scene detection heuristics (horizontal rules, headings, Fountain scene headings) but knows nothing about characters, objectives, or meaning. Each scene span includes a file path so byte offsets are unambiguous.

#### DSL Definition

```
concept SceneMap
  state
    scenes: OrderedList<SceneSpan>      -- scene boundaries in document order
    parse_mode: Prose | Fountain        -- how to detect scene boundaries
    pending_reindex: Set<SceneId>       -- scenes needing re-analysis

  type SceneSpan
    id: SceneId
    file_path: Path                     -- which file this scene belongs to
    start: ByteOffset                   -- byte offset within that file
    end: ByteOffset                     -- byte offset within that file
    content_hash: Hash                  -- for change detection
    title: Option<String>               -- extracted scene title if detectable

  action full_reindex(file_path: Path, text: String)
    -- rebuilds all scene boundaries for this file from scratch
    -- computes content_hash for each scene
    -- each scene's file_path is set to file_path
    post: scenes_in(file_path).spans_cover(0, text.length)
          scenes_in(file_path).non_overlapping = true
          scenes_in(file_path).ids all in pending_reindex

  action reindex(file_path: Path, text: String, changed_ranges: List<ByteRange>)
    -- incrementally updates scene boundaries affected by changes in this file
    -- recomputes content_hash for affected scenes
    -- adds affected scenes to pending_reindex
    post: scenes_in(file_path).spans_cover(0, text.length)
          scenes_in(file_path).non_overlapping = true
          forall s in affected_scenes: s in pending_reindex

  action get_scene(id: SceneId) -> SceneSpan
    pre: id in scenes

  action get_scene_at(file_path: Path, offset: ByteOffset) -> SceneSpan
    -- returns the scene containing this byte offset in the given file
    pre: offset <= text.length
    post: result.start <= offset < result.end and result.file_path = file_path

  action list_scenes() -> List<SceneSpan>
    -- returns all scenes across all files in narrative order
    -- ordering: by file order (from manifest), then by byte offset within each file

  action mark_analyzed(id: SceneId)
    -- removes scene from pending_reindex queue
    post: id not in pending_reindex

  action get_pending() -> Set<SceneId>
    -- returns scenes awaiting re-analysis

  invariant complete_coverage
    -- scene spans collectively cover the entire document with no gaps or overlaps
    scenes.spans_cover(0, document_length) and scenes.non_overlapping
```

#### Pseudocode

SceneMap maintains an ordered list of SceneSpan entries, each mapping a scene ID to a file path and byte range within that file. Each SceneSpan knows which file it belongs to, making byte offsets unambiguous across a multi-file project. Scene ordering across files is determined by the file ordering in the manifest (see File Discovery & Classification section).

In Fountain mode, scene boundaries are detected deterministically from `INT.`/`EXT.` headings. In prose mode, boundaries come from horizontal rules (`---`), Markdown headings, or HTML comment markers (`<!-- scene: "title" -->`); ambiguous cases may require LLM assistance during initial analysis. The parse mode can vary per file (e.g., Act1.md in prose mode, Act2.fountain in Fountain mode).

When `reindex` is called with a file path and a list of changed byte ranges, only the scenes in that file overlapping those ranges are re-detected. Each scene's content_hash is recalculated — if it differs from the previous hash, the scene is added to `pending_reindex`, signaling that its narrative analysis is stale. The invariant guarantees complete, non-overlapping coverage within each file: every byte in a story file belongs to exactly one scene.

---

### Concept 3: NarrativeGraph

**Purpose**: The structured model of the story. Contains characters, objectives (hierarchical, with scope), scenes (as graph nodes), conflicts, and all relationships between them. Agnostic about how its data was produced — it's purely the current state of the narrative model.

#### DSL Definition

```
concept NarrativeGraph
  state
    graph: DirectedGraph<GraphNode, GraphEdge>

  type GraphNode =
    | Character { id: CharacterId, name: String, aliases: List<String>, description: Option<String> }
    | Objective { id: ObjectiveId, character_id: CharacterId, scope: Scope, 
                  description: String, evidence: List<String>, confidence: Float,
                  status: Status }
    | Scene { id: SceneId, file_path: Path, title: Option<String>, summary: String,
              characters_present: List<CharacterId>, location: Option<String>, time: Option<String> }
    | Conflict { id: ConflictId, description: String, objectives: List<ObjectiveId> }

  type Scope = Overarching | Act | Scene
  type Status = Active | Achieved | Abandoned | Blocked | Transformed

  type GraphEdge =
    | Pursues { scene_id: Option<SceneId> }     -- Character -> Objective
    | DecomposesInto                             -- Objective -> Sub-objective
    | ConflictsWith                              -- Objective <-> Objective
    | PresentIn                                  -- Character -> Scene
    | Advances                                   -- Scene -> Objective
    | Blocks                                     -- Scene -> Objective
    | Precedes                                   -- Scene -> Scene
    | Transforms { trigger_scene: SceneId }      -- Objective -> Objective

  action add_node(node: GraphNode) -> NodeId
    post: node in graph.nodes

  action remove_node(id: NodeId)
    -- removes node and all edges referencing it
    pre: id in graph.nodes
    post: id not in graph.nodes
          forall e in graph.edges: e.from != id and e.to != id

  action add_edge(from: NodeId, to: NodeId, edge: GraphEdge)
    pre: from in graph.nodes and to in graph.nodes

  action update_node(id: NodeId, changes: Partial<GraphNode>)
    pre: id in graph.nodes

  action query(filters: GraphFilters) -> Subgraph
    -- returns filtered subset of the graph

  action get_character_arc(character_id: CharacterId) -> List<ObjectiveState>
    -- returns the ordered objective trajectory for a character across all scenes
    pre: character_id in graph.nodes

  action get_scene_analysis(scene_id: SceneId) -> SceneAnalysis
    -- returns which objectives are active, advanced, or blocked in a scene
    pre: scene_id in graph.nodes

  action get_conflicts(filters: ConflictFilters) -> List<Conflict>

  action find_dead_scenes() -> List<SceneId>
    -- returns scenes where no objective changes state

  action serialize() -> JSON
  action deserialize(data: JSON)

  invariant referential_integrity
    -- every edge references valid nodes
    forall e in graph.edges: e.from in graph.nodes and e.to in graph.nodes

  invariant objective_ownership
    -- every Objective has a parent Character
    forall o in graph.nodes where o is Objective:
      exists c in graph.nodes where c is Character and c.id = o.character_id

  invariant scene_presence_consistency
    -- PresentIn edges match characters_present lists
    forall s in graph.nodes where s is Scene:
      forall c in s.characters_present:
        exists edge PresentIn from c to s
```

#### Pseudocode

NarrativeGraph is a directed graph (via `petgraph`) containing four node types and eight edge types. Characters have objectives at different scopes (overarching, act-level, scene-level). Scenes declare which characters are present. Conflicts record where objectives collide. The graph supports queries like "show me Marcus's arc" (follow Pursues edges from Marcus across scenes in Precedes order) and "find dead scenes" (scenes with no Advances or Blocks edges). Serializes to JSON for persistence. The graph enforces referential integrity (no dangling edges), objective ownership (every objective belongs to a character), and scene presence consistency (PresentIn edges match the characters_present list on Scene nodes). Critically, NarrativeGraph doesn't know *how* it was populated — that's the job of Analysis and DeclaredIntent, coordinated through synchronizations.

---

### Concept 4: DeclaredIntent

**Purpose**: Captures what the writer explicitly says about their story's structure. Writer overrides to LLM-inferred graph values. Declarations are authoritative by fiat — they don't need evidence or confidence scores. They persist even when the graph is re-inferred.

#### DSL Definition

```
concept DeclaredIntent
  state
    declarations: Map<(NodeId, Field), Declaration>

  type Declaration
    value: String
    rationale: Option<String>           -- why the writer is overriding
    timestamp: Timestamp

  action declare(node_id: NodeId, field: Field, value: String, rationale: Option<String>)
    -- sets or updates a writer override
    post: (node_id, field) in declarations

  action retract(node_id: NodeId, field: Field)
    -- removes a writer override
    pre: (node_id, field) in declarations
    post: (node_id, field) not in declarations

  action list_declarations() -> List<(NodeId, Field, Declaration)>

  action get_declaration(node_id: NodeId, field: Field) -> Option<Declaration>

  action find_orphans(valid_node_ids: Set<NodeId>) -> List<(NodeId, Field)>
    -- returns declarations referencing nodes that no longer exist
    post: forall (nid, _) in result: nid not in valid_node_ids

  invariant declaration_uniqueness
    -- at most one declaration per (node_id, field) pair
    forall (nid, f) in declarations: count((nid, f)) = 1
```

#### Pseudocode

DeclaredIntent is a key-value store mapping `(node_id, field)` pairs to writer-provided values. When a writer says "Marcus's objective is revenge, not guilt," that's a declaration on `(marcus_objective_id, "description")` with value "Revenge against the people who killed his brother" and an optional rationale. Declarations persist independently of the graph — if Analysis re-infers the graph and changes Marcus's objective, the declaration remains. `find_orphans` detects declarations pointing at nodes that no longer exist in NarrativeGraph (e.g., if a character was removed). The divergence between inferred values (in NarrativeGraph) and declared values (in DeclaredIntent) is computed by a synchronization, not by either concept alone.

---

### Concept 5: Analysis

**Purpose**: The LLM-powered inference engine. Takes text (scoped by scenes) and produces structured narrative data. Manages the analysis queue, caching, and result lifecycle. Does not write to the graph directly — it produces results that synchronizations feed into NarrativeGraph.

#### DSL Definition

```
concept Analysis
  state
    queue: OrderedSet<AnalysisTask>     -- pending analysis work
    cache: Map<(SceneId, Hash), AnalysisResult>
    status: Idle | Analyzing { current: AnalysisTask } | Error { message: String }

  type AnalysisTask
    kind: FullAnalysis | SceneAnalysis { scene_id: SceneId }
    priority: High | Normal
    created: Timestamp

  type AnalysisResult
    scene_id: Option<SceneId>           -- None for full analysis
    characters_found: List<CharacterData>
    objectives_found: List<ObjectiveData>
    conflicts_found: List<ConflictData>
    scene_metadata: Option<SceneMetadata>
    content_hash: Hash                  -- hash of text that produced this result
    timestamp: Timestamp

  type CharacterData
    name: String
    aliases: List<String>
    description: String

  type ObjectiveData
    character_name: String
    scope: Scope
    description: String
    evidence: List<String>
    confidence: Float
    status: Status

  type ConflictData
    description: String
    between: List<String>               -- character names

  type SceneMetadata
    title: Option<String>
    summary: String
    characters_present: List<String>
    location: Option<String>
    time: Option<String>

  action enqueue(task: AnalysisTask)
    post: task in queue

  action analyze_scene(scene_id: SceneId, text: String, graph_context: JSON) -> AnalysisResult
    -- sends scene text + current graph context to LLM
    -- caches result keyed by (scene_id, hash(text))
    pre: status != Analyzing
    post: status = Idle
          (scene_id, hash(text)) in cache

  action analyze_full(text: String) -> AnalysisResult
    -- sends full manuscript to LLM for initial analysis
    pre: status != Analyzing
    post: status = Idle

  action get_cached(scene_id: SceneId, content_hash: Hash) -> Option<AnalysisResult>

  action invalidate(scene_id: SceneId)
    -- marks all cached results for this scene as stale
    post: forall h: (scene_id, h) not in cache

  action process_queue() -> Option<AnalysisResult>
    -- takes the highest-priority task from the queue and executes it
    pre: queue is not empty
    post: queue.size = old(queue.size) - 1

  invariant cache_hash_validity
    -- cached results are keyed by the content hash they were derived from
    forall (sid, h) in cache: cache[(sid, h)].content_hash = h
```

#### Pseudocode

Analysis manages the lifecycle of LLM-driven story interpretation. When a scene is flagged as needing re-analysis, an AnalysisTask is enqueued. `process_queue` takes the highest-priority task, sends the scene text plus the current graph (as compact JSON context) to the LLM via Provider, and receives structured results: characters found, objectives inferred, conflicts detected, and scene metadata. Results are cached keyed by `(scene_id, content_hash)` — if the same scene text is analyzed again, the cache is hit instead of making another LLM call. `invalidate` clears cache entries for a scene when its text changes. Analysis never writes to NarrativeGraph directly; it produces AnalysisResult objects that a synchronization feeds into the graph. This separation means you could swap the LLM analysis for a rule-based system without touching graph logic.

---

### Concept 6: CharacterPerspective

**Purpose**: Constructs a subjective narrative lens for any character in the story. Filters the story through a character's knowledge, goals, and limitations to produce an interpretation of how that character experiences the narrative. Distinct from the graph (which is omniscient) — perspectives are bounded by what the character could plausibly know.

#### DSL Definition

```
concept CharacterPerspective
  state
    perspectives: Map<CharacterId, Perspective>
    scene_perspectives: Map<(CharacterId, SceneId), ScenePerspective>

  type Perspective
    character_id: CharacterId
    knowledge_boundary: Set<SceneId>    -- scenes this character has witnessed
    filtered_arc: List<ObjectiveState>  -- objectives from their POV only
    interpretation_of_others: Map<CharacterId, String>  -- how they see other characters
    generated_at: Timestamp
    graph_hash: Hash                    -- hash of graph state when generated

  type ScenePerspective
    character_id: CharacterId
    scene_id: SceneId
    wants: String                       -- what do they want in this scene
    perceives: String                   -- what do they observe/understand
    decides: String                     -- what do they choose to do
    blocked_by: Option<String>          -- what prevents their objective
    emotional_state: String             -- how do they feel
    knowledge_gained: List<String>      -- what new info do they learn
    content_hash: Hash                  -- hash of scene text when generated

  type ObjectiveState
    scene_id: SceneId
    objective: String
    status: Active | Advanced | Blocked | Achieved | Abandoned
    awareness: Full | Partial | Unaware -- does the character know their status?

  action generate(character_id: CharacterId, graph: JSON, scene_texts: Map<SceneId, String>) -> Perspective
    -- builds the full perspective for a character via LLM
    -- only includes information from scenes the character is present in
    post: character_id in perspectives
          perspectives[character_id].knowledge_boundary = 
            { s.id | s in scenes where character_id in s.characters_present }

  action get_scene_perspective(character_id: CharacterId, scene_id: SceneId, 
                                graph: JSON, scene_text: String) -> ScenePerspective
    -- how does this character experience this specific scene
    pre: character is present in scene
    post: (character_id, scene_id) in scene_perspectives

  action compare(char_a: CharacterId, char_b: CharacterId, scene_id: SceneId,
                  graph: JSON, scene_text: String) -> ComparisonResult
    -- how do two characters experience the same scene differently
    pre: both characters present in scene

  action get_knowledge_boundary(character_id: CharacterId, at_scene: SceneId) -> Set<Information>
    -- what does this character know at this point in the story
    -- accumulated from all scenes they've been present in up to at_scene

  action find_blind_spots(character_id: CharacterId) -> List<BlindSpot>
    -- where is this character missing information that the reader has (dramatic irony)
    post: forall b in result: b.scene not in perspectives[character_id].knowledge_boundary

  action invalidate(character_id: CharacterId)
    -- marks all cached perspectives for this character as stale
    post: character_id not in perspectives
          forall s: (character_id, s) not in scene_perspectives

  action invalidate_scene(character_id: CharacterId, scene_id: SceneId)
    -- marks a specific scene perspective as stale
    post: (character_id, scene_id) not in scene_perspectives

  invariant knowledge_boundary_consistency
    -- a character's perspective never includes information from scenes they weren't in
    forall p in perspectives:
      forall s in p.knowledge_boundary:
        character is present in scene s according to graph
```

#### Pseudocode

CharacterPerspective answers "what is this story like from the Jawa's point of view?" For any character — protagonist, antagonist, or bit player — it constructs a subjective interpretation bounded by what that character could plausibly know. The Jawa doesn't know about the Rebellion; they see droids, a sale, and then stormtroopers. The perspective is built by filtering the NarrativeGraph through the character's knowledge boundary (the set of scenes they appear in), then using the LLM to interpret each scene from their subjective position: what do they want, what do they perceive, what blocks them, how do they feel. `compare` shows how two characters experience the same scene differently — Luke sees a life-changing purchase, the Jawa sees a completed transaction. `find_blind_spots` identifies dramatic irony: moments where the reader knows something the character doesn't. Perspectives are cached and invalidated when the underlying graph or scene text changes.

---

### Concept 7: Skills

**Purpose**: The registry of capabilities available to the agent. Defines what the agent can do, with schemas for inputs and outputs. Handles dispatch, execution logging, and permission management. The boundary between the agent and the rest of the system — Chat never calls other concepts directly, it invokes skills.

#### DSL Definition

```
concept Skills
  state
    registry: Map<SkillName, SkillDefinition>
    execution_log: List<Execution>
    permissions: Map<SkillName, Permission>

  type SkillDefinition
    name: SkillName
    description: String
    input_schema: Schema
    output_schema: Schema
    category: FileTools | GraphTools | PerspectiveTools | StructuralTools | CanvasTools

  type Execution
    skill_name: SkillName
    args: JSON
    result: JSON | Error
    timestamp: Timestamp
    duration_ms: Int

  type Permission = Enabled | Disabled | ConditionalOn { condition: String }

  action register(skill: SkillDefinition)
    post: skill.name in registry

  action unregister(skill_name: SkillName)
    pre: skill_name in registry
    post: skill_name not in registry

  action invoke(skill_name: SkillName, args: JSON) -> JSON
    -- validates args against input schema
    -- checks permissions
    -- executes the skill
    -- logs the execution
    pre: skill_name in registry
         permissions[skill_name] != Disabled
         args matches registry[skill_name].input_schema
    post: execution_log.last.skill_name = skill_name

  action list_available() -> List<SkillDefinition>
    -- returns all skills where permission != Disabled
    post: forall s in result: permissions[s.name] != Disabled

  action get_schema(skill_name: SkillName) -> SkillDefinition
    pre: skill_name in registry

  action get_history(filter: Option<SkillName>) -> List<Execution>

  action set_permission(skill_name: SkillName, permission: Permission)
    pre: skill_name in registry
    post: permissions[skill_name] = permission

  invariant schema_compliance
    -- every invocation is validated against the skill's input schema
    forall e in execution_log where e.result is not Error:
      e.args matches registry[e.skill_name].input_schema
```

#### Pseudocode

Skills is the tool registry. Every capability the agent can invoke — `story_grep`, `read_scene`, `query_graph`, `interpret_as_character`, `write_to_canvas`, `story_lint`, etc. — is registered here with a name, description, input/output schema, and category. When Chat wants the agent to do something, it calls `Skills.invoke`, which validates arguments, checks permissions, dispatches to the appropriate implementation, and logs the execution. Permissions can be Enabled, Disabled, or ConditionalOn (e.g., "write_to_canvas is disabled in read-only mode," or "story_grep is disabled when Provider.is_local is false and the skill would send text to a cloud API"). The execution log provides an audit trail. Skills can be extended — a writer could register a custom skill like "check_dialect_consistency" without modifying Chat or any other concept.

#### Default Skill Registry

**File Tools**

| Skill | Description | Reads From | Writes To |
|-------|-------------|------------|-----------|
| `story_grep` | Search story text by regex or keyword across all story files | TextBuffer (all) | — |
| `read_scene` | Read full text of a specific scene (from any file) | TextBuffer, SceneMap | — |
| `read_range` | Read a line/byte range from a specific file | TextBuffer | — |
| `list_scenes` | List all scenes across all story files with metadata | SceneMap | — |
| `story_stats` | Overall manuscript statistics (aggregated across all files) | TextBuffer (all), SceneMap | — |
| `read_context_file` | Read a supporting file (outline, characters, notes) | Manifest, disk | — |
| `list_files` | List all files in the manifest with roles and metadata | Manifest | — |

**Graph Tools**

| Skill | Description | Reads From | Writes To |
|-------|-------------|------------|-----------|
| `query_graph` | Query graph with filters | NarrativeGraph | — |
| `get_character_arc` | Full objective trajectory for a character | NarrativeGraph | — |
| `get_scene_analysis` | Objectives active/advanced/blocked in a scene | NarrativeGraph | — |
| `get_conflicts` | All objective conflicts | NarrativeGraph | — |
| `find_dead_scenes` | Scenes where no objective changes state | NarrativeGraph | — |
| `get_divergences` | All inferred vs declared mismatches | NarrativeGraph, DeclaredIntent | — |

**Perspective Tools**

| Skill | Description | Reads From | Writes To |
|-------|-------------|------------|-----------|
| `interpret_as_character` | Analyze from a character's perspective | CharacterPerspective, NarrativeGraph | CharacterPerspective |
| `compare_perspectives` | Compare how two characters see a scene | CharacterPerspective, NarrativeGraph | CharacterPerspective |
| `find_blind_spots` | Find dramatic irony moments | CharacterPerspective, NarrativeGraph | — |
| `get_knowledge_at` | What a character knows at a point in the story | CharacterPerspective, NarrativeGraph | — |

**Structural Tools**

| Skill | Description | Reads From | Writes To |
|-------|-------------|------------|-----------|
| `story_lint` | Consistency checks (timeline, presence, continuity) | NarrativeGraph, SceneMap | — |
| `pacing_analysis` | Scene length, conflict density, rhythm | NarrativeGraph, SceneMap, TextBuffer | — |
| `arc_completeness` | Check if character objectives resolve | NarrativeGraph | — |

**Canvas Tools**

| Skill | Description | Reads From | Writes To |
|-------|-------------|------------|-----------|
| `write_to_canvas` | Stream text into the canvas at a position | — | TextBuffer (via Canvas) |
| `replace_in_canvas` | Replace a range in the canvas | SceneMap | TextBuffer (via Canvas) |
| `insert_scene` | Insert a new scene at a position | SceneMap | TextBuffer (via Canvas) |

---

### Concept 8: Canvas

**Purpose**: The right-pane editing surface in the TUI. Renders TextBuffer content as an editable document. Handles cursor, selection, scrolling, and text input. Translates keystrokes into TextBuffer operations. Receives AI-streamed text. A view over TextBuffer, not a separate copy.

#### DSL Definition

```
concept Canvas
  state
    cursor: ByteOffset
    selection: Option<ByteRange>
    scroll_offset: LineNumber
    viewport: Dimensions
    render_cache: List<StyledLine>
    streaming: Option<StreamState>

  type StreamState
    position: ByteOffset                -- where the AI is writing
    active: Boolean

  action render() -> List<StyledLine>
    -- renders the current viewport from TextBuffer content

  action handle_keypress(key: KeyEvent)
    -- translates keypress into cursor movement, selection, or text operation
    -- text operations become TextBuffer.insert / TextBuffer.delete / TextBuffer.replace

  action set_cursor(position: ByteOffset)
    post: cursor = position

  action begin_stream(position: ByteOffset)
    -- AI starts writing at this position
    post: streaming = Some({ position, active: true })

  action stream_chunk(text: String)
    -- AI sends a chunk of text to be inserted at the stream position
    pre: streaming is Some and streaming.active = true
    post: streaming.position = old(streaming.position) + text.length

  action end_stream()
    post: streaming = None

  action scroll_to_scene(scene_span: SceneSpan)
    -- scrolls viewport to show the given scene

  invariant view_consistency
    -- Canvas always reflects TextBuffer content; it holds no independent text state
    render() is derived entirely from TextBuffer.read_all() + styling
```

#### Pseudocode

Canvas is a TUI view over TextBuffer, built with `ratatui`. It renders styled lines from the rope content, handles cursor movement and text selection, and translates keystrokes into TextBuffer operations (insert, delete, replace). When the AI generates prose, `begin_stream` sets a write position, `stream_chunk` inserts text incrementally (the writer sees it appear character by character or chunk by chunk), and `end_stream` finalizes. The writer can edit freely while not in a streaming state. Canvas can highlight scene boundaries (by consulting SceneMap span ranges for styling) but doesn't interpret them. Canvas is purely a view — it holds no independent copy of the text. Its render output is always derived from TextBuffer's current state.

---

### Concept 9: Chat

**Purpose**: The left-pane conversational interface. Manages conversation history, context assembly, and the agent loop. Assembles the right context for each LLM turn: graph + relevant scenes + conversation history. Dispatches tool calls through Skills. The "brain" of the agent interaction.

#### DSL Definition

```
concept Chat
  state
    history: List<Message>
    context: ContextPayload
    agent_status: Idle | Thinking | ToolCall { skill: SkillName } | Streaming

  type Message
    role: User | Assistant | System
    content: String
    tool_calls: Option<List<ToolCall>>
    tool_results: Option<List<ToolResult>>
    timestamp: Timestamp

  type ContextPayload
    graph_json: JSON                    -- compact narrative graph
    divergences: List<Divergence>       -- inferred vs declared mismatches
    relevant_scenes: Map<SceneId, String>  -- scene text pulled in for this turn
    system_prompt: String

  type ToolCall
    skill_name: SkillName
    args: JSON

  type ToolResult
    skill_name: SkillName
    result: JSON

  action send_message(text: String)
    -- adds user message to history
    -- triggers context assembly and LLM call
    post: history.last.role = User
          history.last.content = text

  action assemble_context(graph: JSON, divergences: List<Divergence>) -> ContextPayload
    -- builds the context payload for the next LLM call
    -- always includes: current graph JSON, divergences, system prompt
    -- relevant_scenes starts empty; agent retrieves scenes via tool calls
    post: context.graph_json = graph
          context.divergences = divergences

  action dispatch_tool(tool_call: ToolCall) -> ToolResult
    -- routes a tool call through Skills.invoke
    post: history contains tool_call and its result

  action receive_response(response: String)
    -- handles LLM response (may include tool calls, text, or both)
    post: history.last.role = Assistant

  action receive_stream_chunk(chunk: String)
    -- handles streaming LLM response incrementally
    pre: agent_status = Streaming

  action clear_history()
    post: history = []

  invariant context_includes_graph
    -- every LLM call includes the current narrative graph
    forall call in llm_calls: call.context.graph_json is not empty
```

#### Pseudocode

Chat manages the conversation between the writer and the LLM agent. On each turn, `assemble_context` builds a payload that always includes the full NarrativeGraph as compact JSON (typically 2-5K tokens — the agent's "map" of the story) and any inferred/declared divergences. The agent then decides which scenes to read by making tool calls (`read_scene`, `story_grep`) through Skills — Chat dispatches these via `dispatch_tool`, which routes through `Skills.invoke`. The agent's response may include prose (streamed to Canvas), analysis, or further tool calls. Chat maintains the conversation history for multi-turn context. The key invariant: the agent always has structural awareness of the entire story via the graph, even when it only reads a few scenes' worth of text.

---

### Concept 10: Provider

**Purpose**: Manages the LLM connection — which model, which API, authentication, and the privacy state (cloud vs. local). Answers "where is my text going?"

#### DSL Definition

```
concept Provider
  state
    config: ProviderConfig
    connection_status: Connected | Disconnected | Error { message: String }
    is_local: Boolean                   -- trust-critical: is text leaving the machine?
    metrics: UsageMetrics

  type ProviderConfig
    provider_type: OpenAiCompatible | Anthropic | PydanticGateway | Local
    model: String
    base_url: URL
    api_key_env: Option<String>         -- environment variable name, not the key itself

  type UsageMetrics
    total_tokens: Int
    total_requests: Int
    total_latency_ms: Int

  action configure(config: ProviderConfig)
    -- sets the active provider configuration
    -- determines is_local based on provider_type and base_url
    post: is_local = (config.provider_type = Local) or (config.base_url.host = "localhost")

  action complete(messages: List<Message>, tools: List<ToolSchema>) -> LlmResponse
    -- makes a completion request to the configured provider
    pre: connection_status = Connected
    post: metrics.total_requests = old(metrics.total_requests) + 1

  action stream(messages: List<Message>, tools: List<ToolSchema>) -> Stream<LlmChunk>
    pre: connection_status = Connected

  action test_connection() -> Boolean
    post: connection_status = Connected | Error

  action switch_provider(config: ProviderConfig)
    -- changes providers mid-session
    post: self.config = config
          is_local recalculated

  invariant local_accuracy
    -- is_local accurately reflects whether text leaves the machine
    is_local = true => all requests go to localhost or loopback
    is_local = false => requests go to remote endpoints
```

#### Pseudocode

Provider is a thin abstraction over LLM API access. It supports four provider types: OpenAI-compatible (any API matching the chat completions spec), Anthropic (native API with tool use), Pydantic AI Gateway (unified gateway), and Local (Ollama, llama.cpp, LM Studio, vLLM — anything OpenAI-compatible on localhost). The critical property is `is_local`: this boolean drives the privacy indicator in the TUI status bar. It's determined by provider type and base URL — if the URL points to localhost or loopback, text stays on-machine. Provider tracks usage metrics (tokens, requests, latency) for cost awareness. It knows nothing about stories, graphs, or narratives — it just sends messages and returns responses.

---

## Synchronizations

Synchronizations define how concepts interact. Each synchronization is a declarative rule: when an action occurs in one concept, it triggers an action in another. Concepts never call each other directly. All coordination flows through these rules.

The synchronizations are grouped by the primary data flow they govern.

---

### Sync Group 1: Text Change Propagation

*When the writer edits text, changes ripple through the system: TextBuffer → SceneMap → Analysis → NarrativeGraph → CharacterPerspective.*

#### S1.1: TextBuffer.edit → SceneMap.reindex

**DSL**
```
sync text_to_scenes
  on TextBuffer.insert(position, text)
  or TextBuffer.delete(range)
  or TextBuffer.replace(range, text):
    let changes = TextBuffer.checkpoint()
    let full_text = TextBuffer.read_all()
    SceneMap.reindex(TextBuffer.file_path, full_text, changes.ranges)
```

**Pseudocode**: When any edit occurs in a TextBuffer (insert, delete, replace), checkpoint the accumulated changes, read the full text, and trigger SceneMap to reindex the affected byte ranges for that file. The file_path is passed so SceneMap knows which file's scenes to update. This identifies which scenes were modified.

#### S1.2: SceneMap.reindex → Analysis.enqueue

**DSL**
```
sync scenes_to_analysis
  on SceneMap.reindex(text, changed_ranges)
  when SceneMap.get_pending() is not empty:
    for scene_id in SceneMap.get_pending():
      Analysis.enqueue({ kind: SceneAnalysis(scene_id), priority: Normal })
```

**Pseudocode**: After SceneMap reindexes, any scenes whose content hash changed are in the pending set. For each pending scene, enqueue an analysis task. This doesn't happen immediately — the analysis queue is processed on a debounce timer (see S1.3).

#### S1.3: Debounced Analysis.process_queue → NarrativeGraph.update

**DSL**
```
sync analysis_to_graph
  on Analysis.process_queue() -> result
  when result is not None:
    for character in result.characters_found:
      if character not in NarrativeGraph:
        NarrativeGraph.add_node(Character(character))
    for objective in result.objectives_found:
      NarrativeGraph.update_node(objective.id, objective)
    for conflict in result.conflicts_found:
      NarrativeGraph.update_node(conflict.id, conflict)
    if result.scene_metadata is Some:
      NarrativeGraph.update_node(result.scene_id, result.scene_metadata)
    SceneMap.mark_analyzed(result.scene_id)
```

**Pseudocode**: When Analysis finishes processing a task from its queue, the result (characters, objectives, conflicts, scene metadata) is fed into NarrativeGraph. New characters are added as nodes. Existing objectives and conflicts are updated. The scene is marked as analyzed in SceneMap, removing it from the pending set. This runs on a debounce timer: 2 seconds after the last TextBuffer edit, the queue is processed. This prevents re-analysis mid-sentence.

#### S1.4: NarrativeGraph.update → CharacterPerspective.invalidate

**DSL**
```
sync graph_to_perspectives
  on NarrativeGraph.update_node(id, changes)
  or NarrativeGraph.add_edge(from, to, edge)
  or NarrativeGraph.remove_node(id):
    let affected_characters = characters_affected_by(id, changes)
    for character_id in affected_characters:
      CharacterPerspective.invalidate(character_id)
```

**Pseudocode**: When the narrative graph changes (a node is updated, an edge is added, a node is removed), determine which characters are affected by the change and invalidate their cached perspectives. For example, if Marcus's objective changes in Scene 7, his perspective is invalidated. If a new conflict is added between Marcus and Elena, both perspectives are invalidated. Perspectives are regenerated lazily — only when a skill like `interpret_as_character` requests them.

---

### Sync Group 2: Divergence Detection

*When LLM-inferred values and writer-declared values disagree, the system detects and surfaces the divergence.*

#### S2.1: Analysis.result + DeclaredIntent → Divergence Detection

**DSL**
```
sync detect_divergences
  on Analysis.process_queue() -> result
  for objective in result.objectives_found:
    let declared = DeclaredIntent.get_declaration(objective.id, "description")
    if declared is Some and declared.value != objective.description:
      emit Divergence {
        node_id: objective.id,
        field: "description",
        inferred: objective.description,
        inferred_confidence: objective.confidence,
        declared: declared.value,
        declared_rationale: declared.rationale
      }
```

**Pseudocode**: After Analysis produces results and they're fed into the graph (via S1.3), check each updated node against DeclaredIntent. If the writer has declared a value for a field that the LLM inferred differently, emit a Divergence. Divergences are included in Chat's context assembly (S4.1) so the agent is always aware of them and can surface them to the writer.

#### S2.2: DeclaredIntent.declare → Divergence Re-check

**DSL**
```
sync declaration_divergence_check
  on DeclaredIntent.declare(node_id, field, value, rationale):
    let inferred = NarrativeGraph.get_node(node_id).get_field(field)
    if inferred != value:
      emit Divergence {
        node_id: node_id,
        field: field,
        inferred: inferred,
        declared: value,
        declared_rationale: rationale
      }
```

**Pseudocode**: When the writer makes a new declaration, immediately check it against the current inferred value in NarrativeGraph. If they differ, emit a Divergence. This ensures divergences are detected both when the LLM re-infers (S2.1) and when the writer declares (S2.2).

#### S2.3: NarrativeGraph.remove_node → DeclaredIntent orphan detection

**DSL**
```
sync graph_removal_to_declarations
  on NarrativeGraph.remove_node(id):
    let orphans = DeclaredIntent.find_orphans(NarrativeGraph.all_node_ids())
    for (node_id, field) in orphans:
      emit OrphanedDeclaration { node_id, field }
```

**Pseudocode**: When a node is removed from NarrativeGraph, check DeclaredIntent for declarations that now reference nonexistent nodes. Emit warnings so the writer can retract or re-target them.

---

### Sync Group 3: Canvas ↔ TextBuffer Bidirectional Sync

*Canvas edits flow into TextBuffer. TextBuffer changes (from any source) update Canvas.*

#### S3.1: Canvas.handle_keypress → TextBuffer.edit

**DSL**
```
sync canvas_to_textbuffer
  on Canvas.handle_keypress(key)
  when key produces text_operation:
    match text_operation:
      Insert(pos, text) => TextBuffer.insert(pos, text)
      Delete(range)     => TextBuffer.delete(range)
      Replace(range, t) => TextBuffer.replace(range, t)
```

**Pseudocode**: When the writer types in Canvas, keystrokes that produce text operations (character insertion, backspace deletion, paste replacement) are translated into TextBuffer operations. This is how human edits enter the system.

#### S3.2: Canvas.stream_chunk → TextBuffer.insert

**DSL**
```
sync canvas_stream_to_textbuffer
  on Canvas.stream_chunk(text)
  when Canvas.streaming is Some:
    TextBuffer.insert(Canvas.streaming.position, text)
```

**Pseudocode**: When the AI streams text into Canvas, each chunk is inserted into TextBuffer at the current streaming position. This ensures TextBuffer is always the source of truth, even during AI generation.

#### S3.3: TextBuffer.* → Canvas.render

**DSL**
```
sync textbuffer_to_canvas
  on TextBuffer.insert(_, _)
  or TextBuffer.delete(_)
  or TextBuffer.replace(_, _)
  or TextBuffer.load():
    Canvas.render()
```

**Pseudocode**: After any TextBuffer mutation (from any source — human editing, AI streaming, or file loading), Canvas re-renders. This ensures the writer always sees the current state.

---

### Sync Group 4: Agent Context Assembly

*When the writer sends a chat message, context is assembled from multiple concepts.*

#### S4.1: Chat.send_message → Context Assembly

**DSL**
```
sync assemble_agent_context
  on Chat.send_message(text):
    let graph_json = NarrativeGraph.serialize()
    let divergences = collect_active_divergences()
    Chat.assemble_context(graph_json, divergences)
    let response = Provider.stream(
      Chat.history ++ [{ role: User, content: text }],
      Skills.list_available().schemas
    )
    Chat.receive_response(response)
```

**Pseudocode**: When the writer sends a message, the system assembles context by serializing the current NarrativeGraph to compact JSON and collecting all active divergences. This context, plus the conversation history and the list of available skills (with schemas), is sent to the Provider. The agent's response is handled by Chat — it may include text (displayed in the chat pane), tool calls (dispatched through Skills), or prose to stream into Canvas.

#### S4.2: Chat.dispatch_tool → Skills.invoke

**DSL**
```
sync chat_to_skills
  on Chat.dispatch_tool(tool_call):
    let result = Skills.invoke(tool_call.skill_name, tool_call.args)
    Chat.history.append(ToolResult(tool_call.skill_name, result))
```

**Pseudocode**: When the agent makes a tool call, Chat dispatches it through Skills.invoke. The result is appended to the conversation history so the agent can reason about it on the next turn. Chat never calls NarrativeGraph, TextBuffer, or any other concept directly — everything goes through Skills.

---

### Sync Group 5: Provider State → UI

*Provider state changes are reflected in the TUI status bar.*

#### S5.1: Provider.configure / Provider.switch_provider → Status Bar Update

**DSL**
```
sync provider_to_status_bar
  on Provider.configure(config)
  or Provider.switch_provider(config):
    emit StatusBarUpdate {
      indicator: if Provider.is_local then "⌂ Local" else "☁ Cloud",
      model_name: Provider.config.model,
      connection: Provider.connection_status
    }
```

**Pseudocode**: When the provider configuration changes, the TUI status bar updates to reflect whether text is being sent to a cloud API or processed locally. The indicator shows the model name and connection status. Green for local, amber for cloud.

#### S5.2: Provider.is_local → Skills.permissions

**DSL**
```
sync provider_privacy_to_skills
  on Provider.configure(config)
  or Provider.switch_provider(config):
    if not Provider.is_local:
      -- no additional restrictions by default; privacy indicator handles awareness
      -- writers can optionally configure restricted skills in config.toml
      for skill_name in config.restricted_when_cloud:
        Skills.set_permission(skill_name, Disabled)
    else:
      for skill_name in Skills.registry:
        Skills.set_permission(skill_name, Enabled)
```

**Pseudocode**: When the provider changes, skill permissions can be updated based on privacy configuration. By default all skills are enabled regardless of provider (the privacy indicator gives the writer awareness). But writers can optionally configure skills that should be disabled when using cloud providers (e.g., if they don't want `story_grep` sending text fragments to a remote API). When switching to a local provider, all skills are re-enabled.

---

### Sync Group 6: File Persistence

*Graph and scene map state is persisted alongside the manuscript.*

#### S6.1: NarrativeGraph.update → Persist Graph

**DSL**
```
sync persist_graph
  on NarrativeGraph.add_node(_)
  or NarrativeGraph.update_node(_, _)
  or NarrativeGraph.remove_node(_)
  or NarrativeGraph.add_edge(_, _, _)
  debounce 1000ms:
    write NarrativeGraph.serialize() to ".laires/graph.json"
```

**Pseudocode**: After any graph mutation, debounce for 1 second, then serialize the graph to `.laires/graph.json`. The debounce prevents excessive disk writes during rapid updates (e.g., during initial full analysis).

#### S6.2: SceneMap.reindex → Persist Scene Map

**DSL**
```
sync persist_scene_map
  on SceneMap.reindex(_, _)
  or SceneMap.full_reindex(_):
    write SceneMap.scenes to ".laires/scenes.json"
```

**Pseudocode**: After scene boundaries are reindexed, persist the scene map to disk. This allows fast startup — on next launch, the scene map can be loaded from disk rather than rebuilt from scratch.

#### S6.3: DeclaredIntent.declare / retract → Persist Overrides

**DSL**
```
sync persist_declarations
  on DeclaredIntent.declare(_, _, _, _)
  or DeclaredIntent.retract(_, _):
    write DeclaredIntent.declarations to ".laires/overrides.json"
```

**Pseudocode**: Writer declarations are persisted immediately to `.laires/overrides.json`. These are the writer's explicit statements about their story — they should never be lost.

---

### Sync Group 7: Stale Data Guards

*When a skill reads from a concept that has pending updates, the system handles staleness.*

#### S7.1: Skills.invoke (graph read) → Staleness Check

**DSL**
```
sync staleness_guard
  on Skills.invoke(skill_name, args)
  when skill_name in GraphTools and SceneMap.get_pending() is not empty:
    emit StalenessWarning {
      skill: skill_name,
      pending_scenes: SceneMap.get_pending(),
      message: "Graph may be stale — N scenes have unanalyzed changes"
    }
    -- proceed with invocation using current (possibly stale) data
    -- do not block
```

**Pseudocode**: When a graph-reading skill is invoked and there are scenes pending re-analysis, emit a warning to the agent. The skill still executes with current data — it doesn't block on analysis completion. The agent can decide whether to wait for re-analysis or proceed with stale data and caveat its response.

---

## Synchronization Map (Visual Summary)

```
                        ┌──────────────────┐
                        │    Provider       │
                        │  (LLM connection) │
                        └──────┬───────────┘
                               │ S5.1: status bar
                               │ S5.2: skill permissions
                    ┌──────────┼──────────────────────────┐
                    │          │                          │
              ┌─────▼─────┐   │    ┌──────────┐    ┌─────▼─────┐
              │   Chat     │◄──────│  Skills   │    │  Status   │
              │  (agent    │ S4.2  │ (registry)│    │  Bar (UI) │
              │   loop)    ├──────►│           │    └───────────┘
              └─────┬──────┘      └─────┬─────┘
                    │ S4.1              │ S7.1: staleness guard
                    │ context           │ invoke dispatches to:
                    │ assembly          │
        ┌───────────┼──────────────────┼───────────────────────┐
        │           │                  │                       │
  ┌─────▼──────┐  ┌▼──────────────┐  ┌▼───────────────┐ ┌─────▼──────────────┐
  │ Narrative  │  │ Declared      │  │ Character      │ │ Canvas             │
  │ Graph      │  │ Intent        │  │ Perspective    │ │ (editing surface)  │
  └──┬────┬────┘  └───────┬───────┘  └────────────────┘ └──────┬─────────────┘
     │    │               │                                    │
     │    │ S2.1,S2.2:    │ S2.3:                    S3.1,S3.2:│ edits
     │    │ divergence    │ orphan                             │
     │    │ detection     │ detection                          │
     │    │               │                              ┌─────▼──────┐
     │  S1.4: invalidate  │                              │ TextBuffer │
     │  perspectives      │                              │ (rope)     │
     │                    │                              └─────┬──────┘
     │              S6.3: persist                              │
     │                                                   S1.1: │ checkpoint
     │                                                         │
   S1.3:                                                 ┌─────▼──────┐
   analysis ◄──── S1.2: enqueue ◄────────────────────────│ SceneMap   │
   to graph                                              │ (spans)    │
     │                                                   └────────────┘
     │
   S6.1: persist graph          ┌──────────┐
   S6.2: persist scene map ────►│  .laires/ │
   S6.3: persist declarations ──►│  (disk)   │
                                └──────────┘
                    
                    ┌──────────┐
                    │ Analysis │
                    │ (LLM     │
                    │ inference)│
                    └──────────┘
                    ▲ S1.2: enqueue tasks
                    │ S1.3: results → graph
                    │ S2.1: divergence check
```

---

## Project Directory Structure

```
my-novel/
├── Act1-TheDescent.md          # story file (prose)
├── Act2-TheReckoning.md        # story file (prose)
├── Act3-TheReturn.fountain     # story file (Fountain screenplay)
├── outline.md                  # supporting file (outline)
├── characters.md               # supporting file (character sheets)
├── world-notes.md              # supporting file (notes)
├── outline-v1.md               # excluded (older version, detected by LLM)
├── .laires/
│   ├── config.toml             # project configuration
│   ├── manifest.toml           # file classification, ordering, and roles (see below)
│   ├── graph.json              # narrative graph (derived, S6.1)
│   ├── scenes.json             # scene map with file paths + byte offsets (derived, S6.2)
│   ├── overrides.json          # writer-declared overrides (S6.3)
│   ├── cache/
│   │   ├── scene_hashes.json   # content hashes for change detection
│   │   └── analysis_cache/     # cached LLM analysis per scene
│   └── skills/                 # custom user-defined skills
├── .git/ or .jj/               # version control (optional)
└── .gitignore                  # includes .laires/cache/
```

### Manifest Structure

The manifest (`.laires/manifest.toml`) is generated by `laires scan` during file discovery and classification. It records which files are part of the project, their roles, formats, ordering, and content hashes for change detection.

```toml
[meta]
last_scan = "2026-02-21T14:30:00Z"
classification_model = "claude-haiku-4-5-20251001"  # cheap LLM used for classification

[[story_files]]
path = "Act1-TheDescent.md"
format = "prose"
order = 1
content_hash = "abc123def456"

[[story_files]]
path = "Act2-TheReckoning.md"
format = "prose"
order = 2
content_hash = "789ghi012jkl"

[[story_files]]
path = "Act3-TheReturn.fountain"
format = "fountain"
order = 3
content_hash = "mno345pqr678"

[[context_files]]
path = "outline.md"
role = "outline"
content_hash = "stu901vwx234"

[[context_files]]
path = "characters.md"
role = "characters"
content_hash = "yza567bcd890"

[[context_files]]
path = "world-notes.md"
role = "notes"
content_hash = "efg123hij456"

[[excluded]]
path = "outline-v1.md"
reason = "Older version of outline.md"
```

**File roles:**
- `story` — Primary narrative files. Included in scene map and narrative graph analysis. Ordered by `order` field for cross-file scene numbering.
- `outline` — Story outlines. Not scene-mapped, but queryable by the agent via `read_context_file` skill.
- `characters` — Character sheets or profiles. Queryable by the agent.
- `notes` — Author's notes, world-building, research. Queryable by the agent.
- `excluded` — Files detected but excluded from analysis (older versions, unrelated files). Listed with a reason for transparency.

---

## Configuration

```toml
# .laires/config.toml

[llm]
provider = "anthropic"
model = "claude-sonnet-4-20250514"
api_key_env = "ANTHROPIC_API_KEY"
# base_url = "https://api.anthropic.com"   # default for anthropic provider

# Cheap model used for file classification during scan
[llm.classification]
model = "claude-haiku-4-5-20251001"        # fast, inexpensive model for file discovery

# Alternative: local model
# [llm]
# provider = "local"
# model = "llama3:70b"
# base_url = "http://localhost:11434/v1"

[project]
title = "The Long Way Home"

[analysis]
debounce_ms = 2000            # wait 2s after last edit before re-analyzing
auto_scan = true              # automatic graph updates on file change

[privacy]
restricted_when_cloud = []    # skill names to disable when using cloud provider

[tui]
canvas_width = 60             # percentage of terminal width for canvas pane
```

Note: The `format` field has been removed from `[project]`. File formats are now detected per-file and recorded in the manifest. The manifest (`.laires/manifest.toml`) is generated and maintained by `laires scan` — see File Discovery & Classification.

---

## CLI Commands

```
laires init                     # initialize a new project in current directory (creates .laires/)
laires open                     # open the TUI (chat + canvas)
laires scan                     # discover/classify new files, re-analyze changed story files
laires scan --full              # ignore manifest, re-classify and re-analyze everything from scratch
laires scan --scene 7           # re-analyze a specific scene
laires graph                    # print narrative graph summary to stdout
laires graph --character marcus # print a specific character's arc
laires lint                     # run consistency checks
laires diff                     # story-aware diff (requires VCS)
laires config                   # open config in editor
laires status                   # show project status (files, scenes, characters, last scan)
laires skills                   # list registered skills
laires perspective marcus       # print Marcus's perspective summary
```

---

## TUI Keybindings

| Key | Action |
|-----|--------|
| `Ctrl+G` | Toggle graph overlay |
| `Ctrl+E` | Toggle file explorer overlay |
| `Ctrl+L` | Toggle story lint overlay |
| `Ctrl+P` | Toggle pacing analysis overlay |
| `Tab` | Switch focus between chat and canvas |
| `Ctrl+S` | Save canvas (triggers S1.1 → S1.2 → S1.3 chain) |
| `Ctrl+/` | Toggle status bar detail (expanded stats vs minimal) |
| `Ctrl+Q` | Quit |
| `Esc` | Dismiss active overlay |

---

## Context Assembly Strategy

When the agent processes a user query, context is assembled via synchronization S4.1:

1. **Always included**: Full NarrativeGraph as compact JSON (typically 2-5K tokens). This gives the agent structural awareness of the entire story without reading all text.

2. **Always included**: Active divergences between inferred and declared values. The agent is always aware of where the writer disagrees with the LLM's interpretation.

3. **Selectively included**: The agent decides which scenes to read by invoking skills (`read_scene`, `story_grep`). For "How does Marcus's motivation change in Act 2?" it pulls only Act 2 scenes where Marcus is present.

4. **Conversation history**: Recent chat messages for continuity.

Assembly budget (approximate, varies by model):

```
Graph JSON:          2,000 - 5,000 tokens
Divergences:           100 - 500 tokens
Retrieved scenes:    2,000 - 10,000 tokens (agent controls via skills)
Conversation:        1,000 - 3,000 tokens
System prompt:       1,000 - 2,000 tokens
Response budget:     2,000 - 4,000 tokens
─────────────────────────────────────────
Total:               8,000 - 24,000 tokens per turn
```

This fits within even modest context windows (32K) and scales with story complexity (characters, objectives, conflicts) rather than word count.

---

## Version Control Integration

Laires.ai detects Git or JJ in the project directory and integrates accordingly.

**Standard VCS Operations:**
- `laires diff` — shows text diff plus narrative graph diff
- `laires log` — shows commit/change history with graph change summaries

**Story-Aware Diffing:**

```
Commit abc123 → def456:

Text changes:
  Scene 7 ("The Banquet"): +142 words, -38 words

Graph changes:
  Marcus.objective [Scene 7]:
    - "Persuade the general to ally"
    + "Threaten the general into compliance"
  Conflict added:
    Marcus.objective ↔ Elena.objective in Scene 7
  Scene 7 status:
    Elena.objective: Active → Blocked
```

Graph diffs are computed by comparing `.laires/graph.json` across commits. VCS is opt-in — Laires.ai works fine without any VCS present.

---

## File Discovery & Classification

When `laires scan` runs, it performs a two-phase process:

### Phase 1: File Discovery & Classification

1. **Walk the project directory** — find all text-like files (`.md`, `.fountain`, `.docx`, `.txt`), excluding `.laires/`, `.git/`, `.jj/`, and other dot-directories.

2. **Check manifest** — if `.laires/manifest.toml` exists, compare discovered files against it:
   - **New files**: files on disk not in the manifest → classify them.
   - **Changed files**: files whose content hash differs from manifest → flag for re-analysis.
   - **Removed files**: files in manifest but no longer on disk → remove from manifest, clean up graph nodes.
   - **Unchanged files**: skip classification and analysis.

3. **Classify new files via LLM** — send file names and content snippets (first ~500 words of each) to a cheap LLM (e.g., claude-haiku) with a classification prompt. The LLM returns:
   - **Role**: story, outline, characters, notes, or excluded
   - **Format**: prose, fountain, or other
   - **Suggested ordering** for story files (which act/chapter comes first)
   - **Version grouping**: detects numbered suffixes, draft/final variants, and suggests which files are current vs. older versions

4. **Confirm with user** — present the classification in a human-readable summary:
   ```
   Found 7 files in project:

   Story files (in order):
     1. Act1-TheDescent.md        (prose, 12,340 words)
     2. Act2-TheReckoning.md      (prose, 15,210 words)
     3. Act3-TheReturn.fountain   (fountain, 9,870 words)

   Supporting files:
     - outline.md                 (outline)
     - characters.md              (character sheets)
     - world-notes.md             (notes)

   Excluded (older versions):
     - outline-v1.md              (older version of outline.md)

   Does this look right? [Y/n/edit]
   ```
   The user can accept, reclassify individual files, or exclude files.

5. **Save manifest** — write the confirmed classification to `.laires/manifest.toml` with content hashes.

### Phase 2: Scene Analysis

Proceeds with the existing scan logic, but now iterates over all story files in manifest order:

1. For each story file, load into a TextBuffer and run SceneMap with the appropriate parse mode (prose or fountain).
2. For scenes with changed content hashes, enqueue analysis tasks.
3. Analysis results update the NarrativeGraph with `file_path` metadata on Scene nodes.
4. Cross-file narrative edges (e.g., Precedes between the last scene of Act1 and the first scene of Act2) are inferred during analysis.

### `laires scan --full`

Ignores the existing manifest entirely. Re-discovers all files, re-classifies via LLM, re-confirms with user, and re-analyzes all scenes. Useful when the project structure has changed significantly or the manifest is corrupted.

### Supporting Files in Agent Context

Files classified as `outline`, `characters`, or `notes` are not scene-mapped or included in the narrative graph. However, they are listed in the manifest and accessible to the agent via the `read_context_file` skill. This allows the agent to answer questions like "Am I following my outline?" by reading the outline file on demand, without polluting the scene map with non-narrative content.

---

## Parsers

### Prose Mode (.md)

Scene detection heuristics (applied during SceneMap.full_reindex, with LLM assistance from Analysis for ambiguous cases):

- Horizontal rules (`---`, `***`) as explicit scene breaks
- Heading-level markers (`## Chapter 3`, `### Scene 2`) as structural boundaries
- HTML comment markers: `<!-- scene: "The Confrontation" -->` (respected by SceneMap, invisible in rendered Markdown)
- Contextual cues (time jumps, location changes, POV shifts) — identified by Analysis during initial scan

Optional frontmatter:

```yaml
---
title: "The Long Way Home"
author: "Jane Doe"
format: prose
---
```

### Fountain Mode (.fountain)

Scene headings (`INT.`, `EXT.`) → Scene nodes (deterministic, no LLM needed for boundaries). Character cues (uppercase names before dialogue) → Character presence. Act breaks → ObjectiveScope::Act boundaries. Section headings (`#`, `##`) → structural grouping. The Fountain parser extracts structure deterministically, then Analysis interprets content for objectives, conflicts, and relationships.

---

## Development Phases

### Phase 1: Core Engine (MVP)

Concepts implemented: TextBuffer, SceneMap, NarrativeGraph, Analysis, Provider, Skills (partial).

Synchronizations implemented: S1.1, S1.2, S1.3, S6.1, S6.2.

Deliverables:
- Rust project scaffold with clap CLI
- Rope-based TextBuffer (ropey) with change tracking
- SceneMap with prose mode parser
- NarrativeGraph data structures (petgraph) with JSON persistence
- Analysis with OpenAI-compatible provider
- Basic Skills registry with file and graph tools
- CLI chat loop with graph + scene context assembly
- `laires init`, `laires scan`, `laires graph` commands

### Phase 2: TUI + Fountain + Perspectives

Concepts implemented: Canvas, Chat, DeclaredIntent, CharacterPerspective.

Synchronizations implemented: S1.4, S2.1, S2.2, S2.3, S3.1, S3.2, S3.3, S4.1, S4.2, S5.1, S5.2, S6.3, S7.1.

Deliverables:
- TUI with ratatui: Chat + Canvas split pane
- Canvas with AI streaming and bi-directional TextBuffer sync
- Fountain parser for screenplay support
- Incremental re-analysis (rope change detection → scene-level re-scan)
- DeclaredIntent with divergence detection
- CharacterPerspective with perspective tools
- Graph overlay (Ctrl+G), privacy indicator
- All synchronizations operational
- `laires lint`, `laires status`, `laires perspective` commands

### Phase 3: Multi-File Projects + Providers

Concepts extended: TextBuffer (FileBufferManager), SceneMap (file-aware), NarrativeGraph (file_path on Scene nodes), Skills (cross-file + context file skills).

New components: Manifest system, LLM-powered file classifier, .docx parser.

Deliverables:
- Multi-file project support — any mix of .md, .fountain, .docx, .txt files in the project folder
- LLM-powered file discovery and classification (story vs. outline vs. notes vs. older versions)
- Manifest system (`.laires/manifest.toml`) for file roles, ordering, and change detection
- FileBufferManager coordinating multiple TextBuffer instances
- Cross-file scene mapping with file-aware SceneMap
- NarrativeGraph with file_path metadata on Scene nodes
- `read_context_file` and `list_files` skills for supporting file access
- .docx file reading (via `docx` crate)
- `laires scan` two-phase workflow (classify → confirm → analyze)
- `laires scan --full` for complete re-classification
- Anthropic and Pydantic AI Gateway provider support
- Local model support (Ollama, llama.cpp, LM Studio)
- Git and JJ integration with story-aware diffing
- File explorer overlay, pacing/arc overlays
- Custom skill registration (`.laires/skills/`)
- `laires diff`, `laires skills` commands

### Phase 4: Tauri GUI (Future)

Deliverables:
- Tauri desktop app wrapping the Rust core
- Interactive graph visualization (D3 / React Flow / Cytoscape.js)
- Visual scene/objective editor
- Drag-and-drop scene reordering with graph-aware impact preview
- Character conversation mode (agent adopts character perspective)

---

## Rust Crate Dependencies (Planned)

| Crate | Purpose | Used By |
|-------|---------|---------|
| `clap` | CLI argument parsing | CLI layer |
| `ratatui` + `crossterm` | TUI framework | Canvas, Chat, overlays |
| `ropey` | Rope data structure | TextBuffer |
| `petgraph` | Graph data structure | NarrativeGraph |
| `serde` + `serde_json` | Serialization | All concepts (persistence) |
| `toml` | Config file parsing | Provider, project config |
| `tokio` | Async runtime | Analysis, Provider, Chat |
| `reqwest` | HTTP client for LLM APIs | Provider |
| `regex` | Text search patterns | Skills (story_grep) |
| `grep-regex` + `grep-searcher` | Ripgrep internals | Skills (story_grep) |
| `similar` | Text diffing | VCS integration |
| `docx-rs` or `docx` | Read/write .docx files | FileBufferManager (.docx support) |
| `notify` | File system watcher | TextBuffer auto-reload |
| `tracing` | Structured logging | All concepts |

---

## Success Criteria

Laires.ai succeeds if a fiction writer can:

1. Point it at a folder of story files and get a structural map of their story within minutes.
2. Ask "where does my protagonist's motivation break down?" and get a scene-specific, structurally-aware answer.
3. Ask "what does the Jawa see?" and get a perspective-bounded interpretation that respects the character's knowledge limits.
4. Write in the canvas while chatting with the agent, with both staying in sync — no stale versions, no copy-pasting.
5. Override the LLM's interpretation ("Marcus's objective is revenge, not guilt") and have the system track the divergence constructively.
6. Trust the privacy indicator to know where their unpublished work is going.
7. Read the spec and understand how the system works — because the Concept & Synchronization architecture makes the connections visible, not buried in code.