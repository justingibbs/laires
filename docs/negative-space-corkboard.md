# Negative Space Corkboard

## The Core Idea

A story is a compression. Out of the thousands of scenes that *could* exist in a narrative — every offscreen conversation, every unwitnessed event, every moment between cuts — the writer selects a few dozen and calls that the story. The unchosen scenes don't stop mattering. They're the substrate the story rests on. Readers feel them even when they can't see them.

This is the filmmaker's maxim: **the story moves most between scenes**. The cut between a couple arguing at dinner and the same couple silently packing boxes does more narrative work than either scene alone. What happened in the gap — the sleepless night, the phone call to a lawyer, the moment of doubt — is implied, felt, but never shown.

Every writing tool models the scenes. None model the space between them. And every writing tool assumes the prose comes first — that planning and structure are something you do *before* you open the tool, in your head or on index cards. The tool only shows up once you've started writing.

The Negative Space Corkboard is both — a **freeform planning surface** where the writer thinks through structure, and an **analytical layer** that reveals the space between scenes. Cards can exist before any prose does. The story grows on the board, and the board grows with the story.

It makes the negative space visible, navigable, and generative — without overwhelming the writer with the full possibility space.

## The Business Analogy

This problem isn't unique to fiction. A business is a million moving parts — the hand movements of a seamstress at Levi's, the quality bar set by twenty-year-old customer expectations, the supply chain that connects cotton fields to retail floors. No executive can hold all of it. So they compress it into a narrative: brand story, operational strategy, quarterly goals. The narrative selects the important parts and omits the rest.

But the omitted parts still exert pressure. When quality complaints spike, the seamstress's technique — previously invisible in the executive narrative — becomes the most important thing in the room. A good executive has instinct about when something in the negative space is becoming load-bearing. A great tool makes that instinct systematic.

The writer faces the same problem. When a beta reader says "I didn't buy Maria's change of heart," it's usually because something in the negative space needed to become a scene and didn't. The writer's instinct often knows this already, but it's hard to be systematic about a space you can't see.

Laires' job: **make the negative space navigable without making it overwhelming.**

## Three Sources of Cards

The corkboard is not a read-only view of existing prose. It's a dynamic surface where cards come from three sources, and the writer controls which are visible.

### 1. Writer-Created Cards

The writer puts cards on the board directly. A card can be as sparse as a title — "Maria confronts her father" — or as rich as a full scene outline with characters, objectives, and notes. These cards exist *before* any prose does. They are the writer's thinking made tangible: beats, ideas, questions, structural intentions.

Writer-created cards are first-class objects. They can be reordered, linked, annotated, fleshed out through agent conversation, and eventually promoted into full scenes with prose. Or they can stay as cards forever — structural scaffolding the reader never sees but the writer needs.

### 2. Note-Derived Cards

Many writers maintain planning documents alongside their prose: outlines, character backstories, beat sheets, chapter plans, research notes. These files already live in the project and are already classified by the manifest system.

When the writer enables note-derived cards, the system reads planning documents and extracts beats, events, and structural intentions from them — surfacing them as cards on the board. The extraction is LLM-assisted: a beat sheet that says "Act 2 turning point: Elena discovers the letters" becomes a card titled "Elena discovers the letters" tagged with the relevant character and source document.

Note-derived cards are linked back to their source. If the writer updates the outline, the cards update. If the writer promotes a note-derived card to a scene, the link persists as provenance.

### 3. Story-Derived Cards

These are the cards from the original spec — scenes parsed from `.md` and `.fountain` files by the `SceneMap`, enriched by the `NarrativeGraph` with character presence, objectives, conflicts, and analysis. They represent the story as it currently exists in prose.

Story-derived cards are **togglable**. The writer can show the board with just their planning cards, or toggle on story cards to see how the written scenes map onto their structural intentions. When a story card corresponds to a writer-created card (same beat, same characters, same narrative function), the system links them — the planning card shows a "written" indicator, and the story card shows its planning origin.

### Matching: Plans to Prose

When story-derived cards are toggled on, the system needs to match them to existing planning cards. This matching is LLM-assisted, using:

- Character overlap (does the story scene feature the same characters as the planned beat?)
- Objective alignment (does the scene advance/block the same objectives?)
- Summary similarity (does the prose accomplish what the card described?)
- Sequence position (is the scene roughly where the card sits in the board order?)

Matches are suggested, not forced. The writer confirms or corrects them. Unmatched planning cards represent beats not yet written. Unmatched story cards represent scenes the writer wrote without planning — organic discoveries. Both are valuable signals.

## Concepts

### 1. The Delta

Between every pair of adjacent scenes, there is a computable delta — the set of things that changed off-screen:

- **Time elapsed** — minutes, days, weeks
- **Character movement** — who disappeared, who appeared, who changed location
- **Objective state changes** — an objective that was Active is now Blocked, but we never saw why
- **Knowledge shifts** — a character who didn't know something in Scene A knows it in Scene B
- **Relationship changes** — enemies became allies, trust was broken, alliances formed
- **Emotional state** — a character who was confident is now afraid

The narrative graph already tracks most of this. Objectives carry status (Active / Blocked / Achieved / Abandoned / Transformed) and are linked to scenes via Advances and Blocks edges. Character presence is tracked per scene. The character perspective system can compute knowledge state at any point.

The Delta is what the cut *is* — the narrative work happening in the white space.

### 2. Pressure

Not all negative space is equal. Some gaps are stable — the writer skipped them because nothing important happened there, and the reader agrees. Other gaps are under **pressure** — something significant changed off-screen, and the story is relying on the reader to infer it without evidence.

Pressure is computable from the Delta:

| Signal | Meaning |
|--------|---------|
| Objective jumps from Active to Achieved with no intermediate scene | Resolution happened off-screen — was that intentional? |
| Character relationship reverses between adjacent scenes | Major shift with no witnessed catalyst |
| Character gains knowledge with no source scene | Information appeared from nowhere |
| Long time gap with many active objectives | A lot happened invisibly |
| Character disappears for many scenes then returns changed | Transformation without witness |

High-pressure gaps aren't errors. A writer may intentionally skip the obvious scene to create mystery or pacing. But they should be **visible choices**, not blind spots.

### 3. Possibility Fan

At any high-pressure transition, the LLM can generate a **fan of possible scenes** — scenes that could logically exist in that gap given the character states, objectives, conflicts, and narrative momentum at that point.

These aren't recommendations. They're the possibility space made tangible:

- A scene the writer hadn't considered but immediately recognizes as right
- A scene that reveals a motivation gap they need to address elsewhere
- A scene that exists in their head but they'd chosen to skip — seeing it materialized confirms the skip was correct
- A scene from a POV they hadn't considered
- A scene that accomplishes the same narrative work through a different dramatic strategy (confrontation instead of conversation, action instead of dialogue)

The fan appears as **ghost cards** on the corkboard — translucent, uncommitted, explorable. The writer can dismiss them, promote one to a real scene card, or use them as inspiration without acting on any directly.

### 4. Compression Ratio

Every scene occupies a position on a compression spectrum:

- **Low compression** — this scene follows the previous one closely in time, with the same characters, in the same location. The camera barely moved. The story is lingering here.
- **High compression** — weeks passed, characters scattered, objectives shifted. The story jumped over a lot of ground to land here.

Neither extreme is wrong. A story that lingers everywhere drags. A story that compresses everything feels thin. The craft is in the variation — knowing when to slow down and when to cut.

The compression ratio is a **density map of narrative attention**. Where did the writer choose to point the camera? Where did they look away? The corkboard visualizes this as the weight/thickness of the transition connectors between scene cards.

### 5. Alternative Scenes

Beyond the gaps: for any *existing* scene, the system can generate alternatives — other scenes that could accomplish similar narrative work at that position in the story.

"You wrote a quiet dialogue scene at the cafe. Given the active objectives and conflicts at this point, here are three other scenes that could serve a similar structural role":

- The same information revealed through action instead of dialogue
- The same beat from another character's POV
- A scene that advances the same objective but through a different conflict

This isn't about replacing the writer's scene. It's about showing the **design space around their choice**. The writer chose one path through a vast graph of possible stories. Seeing adjacent paths confirms their instincts or sparks new ideas.

## The Corkboard

### Visual Design

The corkboard is a 2D canvas with four kinds of objects:

**Plan Cards** (solid, warm color — amber/cork) — the writer's own structural intentions. Created directly on the board or extracted from notes. Each card shows:

- Title (required — even if it's just "the confrontation")
- Characters involved (optional, as small avatars or initials)
- Notes / summary (optional, one or two lines)
- Status: Unwritten / In Progress / Written (linked to a story scene)
- Source indicator: hand-created vs. extracted from a note file (with source link)

**Story Cards** (solid, cool color — slate/blue) — scenes parsed from prose files. Toggled on/off by the writer. Each card shows:

- Scene title / number
- Characters present (as small avatars or initials)
- One-line summary
- Compression indicator (how much negative space precedes this scene)
- Objective activity (which objectives this scene advances, blocks, or transforms)
- Match indicator: linked plan card (if matched), or "unplanned" badge

When story cards are toggled on and matched to plan cards, the plan card and story card can be shown as a **stacked pair** — the plan card behind, the story card in front — or the writer can choose to collapse matched pairs into a single card showing both plan and prose metadata.

**Transition Edges** (between cards) — the cuts, rendered as connectors whose visual weight reflects the Delta magnitude:

- Thin, faint line: small delta (next morning, same characters, nothing changed off-screen)
- Thick, saturated line: large delta (weeks passed, objectives changed, characters moved)
- Annotated on hover: what specifically changed in the gap
- Pressure indicator: pulsing or highlighted when the gap has high narrative pressure

Transition edges compute between whatever cards are visible — plan cards, story cards, or both. When only plan cards are visible, pressure is estimated from the structural information available (character overlap, objective intent, sequence distance). When story cards are toggled on, pressure uses the full Delta computation from the narrative graph.

**Ghost Cards** (translucent) — AI-generated possible scenes, appearing at high-pressure transitions when the writer clicks/hovers a transition edge:

- Faded rendering, clearly distinguished from plan and story cards
- Dismissable (swipe away / click X)
- Promotable to plan card (adds to the board as a structural intention)
- Promotable to scene (creates draft prose in the canvas — Workshop mode only)
- Generated on-demand, not pre-computed for every gap

### Layout Modes

**Linear** — Cards in a row, left to right, narrative order. Transition edges between each pair. Simplest view. Good for sequential pacing review.

**Clustered** — Force-directed layout where scenes cluster by shared characters, thematic similarity, and causal links. Subplots form visible groups. Bridge scenes (connecting subplots) sit between clusters. Isolated scenes float to the edges. This is the view that reveals emergent structure.

**Character Swim Lanes** — One horizontal lane per major character. Scene cards sit in the lane of their POV character (or span multiple lanes if ensemble). Gaps in a character's lane are immediately visible — where does this character disappear from the narrative? What happens to them off-screen?

### Interaction Model

**Create** — Double-click empty space on the board to create a new plan card. A minimal card appears with a title field focused. The writer types a beat ("Maria confronts her father"), optionally tags characters and objectives, and the card is live on the board. Cards can also be created by the agent during chat.

**Browse** — Pan and zoom the corkboard. Hover cards for detail. Hover transitions to see the Delta annotation.

**Toggle** — A toolbar control shows/hides story-derived cards. When toggled on, the system runs the matching heuristic and links story cards to plan cards. The writer can also toggle note-derived cards independently. Three toggles, three layers — the writer composes the view they need.

**Explore** — Click a transition edge to generate the Possibility Fan. Ghost cards appear. Browse them, dismiss them, or promote one.

**Reorder** — Drag any card (plan or story) to a new position. The system recomputes:
  - Downstream character states (who knows what, when)
  - Objective status cascades (does the new order break any arc?)
  - Compression ratios (did a low-compression region become high-compression?)
  - Transition Deltas (what changed about the cuts?)
  - Flags inconsistencies without blocking the move

  When a plan card is reordered, the writer is effectively saying "I want this beat to happen here in the story." If the card is linked to a story scene, the system flags what would need to change in the prose for the reorder to work.

**Flesh Out** — Select a plan card and invoke the agent (via chat or right-click menu) to develop it. The agent can:
  - Expand a one-line beat into a full scene outline (characters, setting, conflict, resolution)
  - Suggest which characters should be present based on active objectives
  - Identify what the scene needs to accomplish structurally (what pressure it relieves, what arcs it advances)
  - Generate draft prose (Workshop mode) or a revision brief entry (Consultant mode)
  - Split a dense card into multiple beats

**Inspect** — Click a story card to open it in the canvas panel (existing behavior). Click a plan card to open its detail view (notes, linked characters/objectives, source document if note-derived). Click a ghost card to see its generated content in a preview pane.

**Alternative** — Right-click (or long-press) any card to generate alternative scenes for that position. Alternatives appear as a fanned set of ghost cards behind the real one.

**Link** — Drag from one card to another to create a causal or thematic link. These links are separate from transition edges (which are sequential). A link says "these two beats are related" — a setup and payoff, a parallel, a callback. Links inform the clustering layout and the agent's structural reasoning.

### The Corkboard Agent

The corkboard has its own agent context — a scoped version of the existing chat agent with skills tuned for structural work. The writer can chat with the agent while looking at the board, and the agent can read and modify the board state.

**What the agent sees:** the full board — all plan cards, their order, their links, their status (written/unwritten). When story cards are toggled on, the agent also sees the narrative graph data (objectives, character arcs, pressure, Deltas).

**What the agent can do:**

- Create, reorder, merge, split, and delete plan cards
- Flesh out a card with structural detail or draft prose
- Suggest where a new card should go based on narrative pressure and arc analysis
- Answer structural questions: "What happens to Elena between chapters 3 and 7?" "Which objectives have no resolution scene?"
- Propose reorderings and explain the consequences: "If you move the confrontation before the discovery, here's what breaks and what improves"
- Generate possibility fans on demand for any transition
- Compare the plan (cards) against the prose (story scenes) and identify gaps in both directions

**How the writer invokes it:** The agent lives in the existing chat panel. When the corkboard is the active view, the agent's system prompt includes corkboard context. The writer can also invoke agent actions directly from the board — right-click a card, select "Flesh out" or "Why is this here?" and the response appears in chat.

The agent is conversational. The writer can have a back-and-forth about structural decisions:

> **Writer:** "I have these three scenes in Act 2 but the pacing feels slow. What if I cut the café scene?"
>
> **Agent:** "The café scene is the only place where David learns about the inheritance. If you cut it, you need that information to arrive somewhere else — the phone call in scene 12 could carry it, but that changes the dynamic from intimate to transactional. Here's what the board looks like with and without it." *(reorders cards, shows pressure changes)*
>
> **Writer:** "What if David already knows — what if he overheard it in the opening?"
>
> **Agent:** "That works structurally. It means the café scene can become about something else — or be cut entirely. The pressure between scenes 4 and 8 drops from high to low. Want me to update the card?"

## Data Model

### PlanCard (new)

```
PlanCard {
    id: PlanCardId,
    title: String,
    summary: Option<String>,
    notes: Option<String>,              // freeform writer notes
    characters: Vec<CharacterId>,       // tagged characters
    objectives: Vec<ObjectiveId>,       // tagged objectives
    status: PlanCardStatus,             // Unwritten | InProgress | Written
    source: PlanCardSource,             // Manual | NoteFile(path, span) | Promoted(GhostId)
    linked_scene: Option<SceneId>,      // matched story scene, if any
    links: Vec<PlanCardId>,             // causal/thematic links to other cards
    position: BoardPosition,            // x, y on the corkboard (persisted)
    created_at: DateTime,
    updated_at: DateTime,
}

PlanCardStatus = Unwritten | InProgress | Written

PlanCardSource =
    | Manual                            // writer created directly on the board
    | NoteFile { path: PathBuf, span: Option<(usize, usize)> }  // extracted from a planning doc
    | Promoted { ghost_id: GhostId }    // promoted from a ghost card
```

### TransitionEdge (new)

```
TransitionEdge {
    from_card: CardRef,                 // PlanCardId or SceneId
    to_card: CardRef,
    delta: Option<Delta>,               // full Delta when story cards are involved, estimated otherwise
    pressure: f64,                      // computed from delta magnitude (or estimated from plan metadata)
    ghost_scenes: Vec<GhostScene>,      // generated on-demand, cached
}

CardRef = PlanCard(PlanCardId) | StoryScene(SceneId)
```

### Delta (new)

```
Delta {
    time_elapsed: Option<String>,       // "3 days", "weeks", etc.
    characters_appeared: Vec<CharacterId>,
    characters_disappeared: Vec<CharacterId>,
    location_change: Option<(String, String)>,  // from, to
    objective_changes: Vec<ObjectiveChange>,     // id, old_status, new_status
    knowledge_gains: Vec<KnowledgeGain>,        // character, what they learned
    relationship_shifts: Vec<RelationshipShift>, // between, nature of change
}
```

### GhostScene (new)

```
GhostScene {
    id: GhostId,
    title: String,
    summary: String,
    characters_present: Vec<CharacterId>,
    rationale: String,              // why this scene could exist here
    narrative_function: String,     // what structural work it would do
    generated_content: Option<String>,  // draft text, generated on promotion
    source_transition: (CardRef, CardRef),  // the gap it fills
}
```

### NoteExtraction (new)

```
NoteExtraction {
    source_file: PathBuf,
    cards: Vec<PlanCard>,           // extracted plan cards
    extraction_model: String,       // which LLM model did the extraction
    extracted_at: DateTime,
    file_hash: String,              // for cache invalidation — re-extract when file changes
}
```

### PlanMatch (new)

```
PlanMatch {
    plan_card: PlanCardId,
    story_scene: SceneId,
    confidence: f64,                // 0.0–1.0, how sure the system is about the match
    match_signals: Vec<MatchSignal>, // character overlap, objective alignment, summary similarity, position
    confirmed: Option<bool>,        // None = suggested, Some(true) = confirmed, Some(false) = rejected
}
```

### Pressure Computation

```
// Full pressure (when story cards are present)
pressure(delta) =
    w_time * time_magnitude(delta.time_elapsed)
  + w_obj  * count(delta.objective_changes where status_jump > 1)
  + w_know * count(delta.knowledge_gains with no source)
  + w_rel  * count(delta.relationship_shifts)
  + w_char * count(delta.characters_disappeared intersect delta.characters_appeared)

// Estimated pressure (plan cards only, no prose to analyze)
estimated_pressure(card_a, card_b) =
    w_char_diff * character_set_distance(card_a.characters, card_b.characters)
  + w_obj_diff  * objective_set_distance(card_a.objectives, card_b.objectives)
  + w_seq       * sequence_gap(card_a.position, card_b.position)
  + w_semantic  * semantic_distance(card_a.summary, card_b.summary)  // embedding similarity
```

Weights are tunable. High pressure doesn't mean "add a scene here" — it means "a lot of narrative work is happening invisibly here, and the writer should be aware of that."

## Integration with Existing Infrastructure

| Feature | Builds On |
|---------|-----------|
| Plan card persistence | `.laires/corkboard.toml` — new file, same pattern as declared intents |
| Note extraction | `manifest.rs` file classification + `analysis.rs` LLM pipeline |
| Plan-to-scene matching | `analysis.rs` LLM pipeline + `narrative_graph` character/objective queries |
| Delta computation | `narrative_graph` objective status tracking, `Advances`/`Blocks` edges, `PresentIn` edges |
| Knowledge tracking | `character_perspective` system, `get_knowledge_at` skill |
| Compression visualization | `pacing_analysis` skill (word counts + conflict density per scene) |
| Possibility fan generation | `analysis.rs` LLM pipeline + graph context for prompt construction |
| Ghost card rendering | `graph_view.rs` force-directed layout + new translucent node type |
| Plan card rendering | `graph_view.rs` — new node type alongside existing Scene/Character/Objective nodes |
| Scene reordering effects | `divergence.rs` detection + `story_lint` validation |
| Alternative scene generation | `skills.rs` registry + provider system for LLM calls |
| Character swim lanes | `get_character_arc` skill + `PresentIn` edge queries |
| Corkboard agent context | `gui/agent.rs` system prompt + `skills.rs` — new CorkboardTools skill set |
| Flesh-out interaction | `skills.rs` + provider LLM call, same pipeline as existing analysis |
| Mode integration | `SessionMode` — Consultant corkboard is read-only + brief; Workshop is freeform + prose |

### New Skills (CorkboardTools)

| Skill | Description |
|-------|-------------|
| `create_card` | Create a new plan card with title, optional characters/objectives/notes |
| `update_card` | Modify an existing plan card's fields |
| `delete_card` | Remove a plan card from the board |
| `reorder_cards` | Move a card to a new position, report consequences |
| `merge_cards` | Combine two plan cards into one |
| `split_card` | Break a dense plan card into multiple cards |
| `flesh_out` | Expand a sparse card into a detailed scene outline via LLM |
| `link_cards` | Create a causal/thematic link between two cards |
| `match_to_scene` | Manually link a plan card to a story scene |
| `show_gaps` | List unwritten plan cards and unplanned story scenes |
| `board_summary` | Return a structural summary of the current board state |

## What This Is Not

- **Not an outlining tool that replaces the writer's process.** The corkboard doesn't impose a planning methodology. Writers who plan extensively can put fifty cards on the board before writing a word. Writers who discover their story by writing can start with prose and add plan cards retroactively as structure emerges. The board adapts to the writer, not the other way around.
- **Not a scene generator.** The system helps the writer think through structure, but it doesn't decide what happens in the story. Ghost cards are explorations of possibility space, not recommendations. Fleshing out a card gives the writer material to work with, not a finished scene to accept.
- **Not prescriptive.** High pressure is not an error. Ghost cards are not recommendations. Unwritten plan cards are not obligations. The writer sculpts by omission as much as by inclusion, and the system respects that.
- **Not exhaustive.** The full possibility space is infinite. The system surfaces a tractable, curated sample — enough to spark insight, not enough to paralyze.

## Mode Integration

The key principle: **planning is always unrestricted.** Creating, reordering, linking, and fleshing out cards is structural thinking, not prose editing. Both modes get the full corkboard planning surface. The mode split only matters at the **promotion boundary** — the moment a card becomes prose.

**Both Modes (unrestricted planning):**
- Create, reorder, delete, merge, split plan cards
- Link cards (causal/thematic connections)
- Toggle story cards and note-derived cards on/off
- Browse, explore transitions, generate possibility fans
- Chat with the corkboard agent about structural questions
- Flesh out cards into detailed outlines (characters, setting, conflict, structural function)

**Consultant Mode (planning → recommendations):**
- Fleshing out a card produces an outline and a revision brief entry — not draft prose
- Promoting a ghost card creates a plan card (structural intention), not a scene in the manuscript
- The agent frames structural suggestions as recommendations: "this card could become a scene where..." rather than writing the scene
- Story cards are read-only — they show the state of the prose but cannot be edited from the board

**Workshop Mode (planning → prose):**
- Everything in Consultant mode, plus:
- Fleshing out a card can produce draft prose, not just an outline
- Promoting a ghost card can generate a scene draft directly in the canvas
- The agent can create and modify prose through canvas tools
- Full promotion flow: card → outline → draft → scene in manuscript, as a single agent interaction or step by step

## Persistence

Plan cards persist in `.laires/corkboard.toml` — the same pattern used by declared intents. The file stores:

- All plan cards with their metadata, positions, links, and status
- Note extraction cache (file hashes + extracted card IDs, for cache invalidation)
- Plan-to-scene match confirmations
- Board layout preferences (which toggles are on, which layout mode is active)

The corkboard state is version-controlled alongside the rest of `.laires/`. This means structural planning history is preserved in git/jj — the writer can see how their structural thinking evolved alongside the prose.

## Resolved Decisions

- **Delta computation** — user-initiated via a "Compute Deltas" button in the corkboard toolbar, same pattern as the existing "Scan" button in the app. Deltas are not computed automatically on every change — the writer decides when to refresh. This keeps the corkboard responsive and avoids surprise LLM calls.

- **Ghost scene caching** — ghost scenes are cached after generation. When the narrative graph changes (scenes added, reordered, prose edited), cached ghosts are marked stale but not automatically regenerated. The writer clicks a "Regenerate" button (per-transition or board-wide) to refresh stale ghosts. This keeps LLM costs predictable and under the writer's control.

- **Pressure thresholds** — ship with sensible defaults calibrated to general fiction. No per-genre tuning in v1. The default weights treat objective status jumps and unsourced knowledge gains as the strongest pressure signals, with time gaps and relationship shifts as secondary. If the defaults prove wrong for specific genres, we can expose weight tuning later — but we're not building that UI now.

- **Layout persistence** — yes, always. Any manual card arrangement persists in `corkboard.toml` across sessions. When the writer drags a card, that position is theirs until they move it again or reset the layout. Layout mode (Linear / Clustered / Swim Lanes) also persists.

## Open Questions

- ~~**Interaction with modes**~~ — resolved, see Mode Integration section below.

- **Note extraction granularity** — how fine-grained should extraction be? A beat sheet with 50 entries produces 50 cards, which may clutter the board. Should the writer control extraction granularity (chapter-level vs. scene-level vs. beat-level)?

- **Card lifecycle** — when a plan card is promoted to a full scene and the prose diverges from the original plan, should the plan card update to reflect the prose, or preserve the original intention as a historical record?

- **Collaboration** — if multiple writers share a project, do they share one corkboard or maintain separate boards? Can plan cards be attributed to specific writers?

- **Board size** — a novel might have 60+ scenes and twice as many plan cards. What are the UX limits? Does the board need collapsing/grouping for large projects (e.g., collapse an act into a single super-card)?
