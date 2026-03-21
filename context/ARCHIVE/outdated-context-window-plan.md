# Context Window Management — Implementation Plan

## Problem Statement

Every LLM request in Laires currently assembles the **full narrative graph**, **all 22+ tool schemas**, **up to 20 history messages**, **divergences**, and **system prompt** — regardless of relevance. For a medium project this is ~54K tokens per chat turn at baseline. For perspective analysis (which concatenates all scene texts), it can reach 200K+. This causes:

- Empty/failed responses when context exceeds model limits (the silent-failure bug)
- Unnecessary token cost on cloud providers
- Slower responses from larger payloads
- Poor compatibility with smaller-context models (local Ollama, older APIs)

There is also **no visibility** into how much context each request consumes, making it hard to diagnose problems.

---

## Current Context Assembly (What Gets Sent)

### Per Chat Request (`gui/agent.rs:116-155`, `cli/chat.rs:144-189`)

| Component | Source | Size Range |
|-----------|--------|-----------|
| System prompt | `SYSTEM_PROMPT` constant | ~450 tokens (fixed) |
| Full narrative graph JSON | `graph.serialize_compact()` | 5–150K tokens |
| Divergences JSON | `divergence::detect_divergences()` | 0–20K tokens |
| Chat history (up to 20 msgs) | `llm_history.clone()` | 0–100K tokens |
| Current user message | user input | ~100–500 tokens |
| Tool schemas (22+ skills) | `skills.tool_schemas()` | 7–25K tokens |

### Per Perspective Request (`character_perspective.rs:192-238`)

| Component | Source | Size Range |
|-----------|--------|-----------|
| Full graph JSON | `graph.serialize_compact()` | 5–150K tokens |
| All witnessed scene texts | concatenated via `scene_texts.join()` | 10–500K tokens |

### Per Blind Spots Request (`character_perspective.rs:442-492`)

| Component | Source | Size Range |
|-----------|--------|-----------|
| All unseen scene texts | concatenated via `unseen_scenes.join()` | 10–500K+ tokens |

### Multi-Turn Tool Loop Compounding (`agent.rs:158-287`)

Each tool turn appends assistant + tool result messages to the `messages` vec. Over 10 turns, the accumulated context can double or triple the baseline.

---

## Implementation Plan

### Phase A: Visibility & Quick Wins

These changes are low-risk and immediately useful.

#### A1. Token Counting Utility

Add a `context_budget` module with approximate token counting.

**New file:** `src/concepts/context_budget.rs`

```rust
/// Approximate token count using chars/4 heuristic.
/// Good enough for budget tracking — not billing-accurate.
pub fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

/// Estimate tokens for a set of messages + tools + system prompt.
pub struct ContextReport {
    pub system_prompt_tokens: usize,
    pub graph_tokens: usize,
    pub divergence_tokens: usize,
    pub history_tokens: usize,
    pub user_message_tokens: usize,
    pub tool_schema_tokens: usize,
    pub total_tokens: usize,
}

pub fn estimate_request_context(
    system_prompt: &str,
    graph_json: &str,
    divergence_json: &str,
    history: &[Message],
    user_message: &str,
    tool_schemas: &[ToolSchema],
) -> ContextReport { ... }
```

**Integration points:**
- `gui/agent.rs`: Log `ContextReport` via `AgentEvent` before each LLM call
- `cli/chat.rs`: Print context summary before each LLM call (behind a `--verbose` or always)
- `gui/panels/status_bar.rs`: Show token count from last request

#### A2. Surface Empty Responses

**Fix the silent-failure bug.** When the LLM returns empty content with no tool calls, show an error instead of an invisible empty message.

**Files:** `gui/agent.rs:174-179`, `cli/chat.rs:207-210`

```rust
// Replace:
let text = response.content.unwrap_or_default();

// With:
let text = response.content.unwrap_or_default();
if text.is_empty() {
    let _ = events.send(AgentEvent::Error(
        "LLM returned an empty response. This may indicate the context \
         is too large for the model, a rate limit was hit, or the API \
         returned an error. Check provider logs.".to_string()
    ));
    let _ = events.send(AgentEvent::Idle);
    break;
}
```

Also log the `ResponseUsage` (prompt_tokens, completion_tokens) from the API response so the actual token count (not just the estimate) is visible when the provider returns it.

#### A3. Log API Response Usage

The `ResponseUsage` struct already exists (`provider.rs:86-90`) and is populated from both Anthropic and OpenAI responses. Surface it.

**Files:** `gui/state.rs` (add to `AgentEvent`), `gui/agent.rs`, `gui/panels/status_bar.rs`

```rust
// New event variant:
AgentEvent::UsageReport {
    prompt_tokens: u64,
    completion_tokens: u64,
    estimated_context: ContextReport,
}
```

Display in status bar: `"Last request: 42K prompt / 1.2K completion tokens"`

---

### Phase B: Graph Context Reduction

The narrative graph is the single largest context component. These changes reduce it dramatically.

#### B1. Graph Summary Mode

Add `serialize_summary()` to `NarrativeGraph` — a compact representation that gives the LLM enough to know what exists without full detail.

**File:** `src/concepts/narrative_graph.rs`

```rust
/// Compact summary: names and IDs only, no descriptions/metadata.
/// ~5-10% the size of serialize_compact().
pub fn serialize_summary(&self) -> String {
    let characters: Vec<_> = self.get_characters().iter().map(|n| {
        if let GraphNode::Character { id, name, .. } = n {
            serde_json::json!({ "id": id, "name": name })
        } else { serde_json::Value::Null }
    }).collect();

    let scenes: Vec<_> = self.get_scenes().iter().map(|n| {
        if let GraphNode::Scene { id, title, characters_present, .. } = n {
            serde_json::json!({
                "id": id,
                "title": title,
                "characters": characters_present,
            })
        } else { serde_json::Value::Null }
    }).collect();

    let objectives: Vec<_> = self.get_objectives().iter().map(|n| {
        if let GraphNode::Objective { id, label, status, .. } = n {
            serde_json::json!({ "id": id, "label": label, "status": status })
        } else { serde_json::Value::Null }
    }).collect();

    let conflicts: Vec<_> = self.get_conflicts().iter().map(|n| {
        if let GraphNode::Conflict { id, label, status, .. } = n {
            serde_json::json!({ "id": id, "label": label, "status": status })
        } else { serde_json::Value::Null }
    }).collect();

    serde_json::to_string(&serde_json::json!({
        "characters": characters,
        "scenes": scenes,
        "objectives": objectives,
        "conflicts": conflicts,
        "edge_count": self.edge_count(),
        "note": "This is a summary. Use query_graph or get_scene_analysis tools for full details."
    })).unwrap_or_default()
}
```

**Usage:** Replace `graph.serialize_compact()` with `graph.serialize_summary()` in chat context assembly (`gui/agent.rs:118`, `cli/chat.rs:145`). The LLM can still access full details via `query_graph` and `get_scene_analysis` tools.

#### B2. Relevant Subgraph Extraction

For cases where the user's query mentions specific characters or scenes, extract only the relevant subgraph.

**File:** `src/concepts/narrative_graph.rs`

```rust
/// Serialize only nodes matching the given IDs + their 1-hop neighbors.
pub fn serialize_subgraph(&self, node_ids: &[&str]) -> String { ... }
```

**File:** `src/concepts/context_budget.rs`

```rust
/// Extract character/scene references from user input by matching
/// against known node names/IDs in the graph.
pub fn extract_relevant_ids(
    user_input: &str,
    graph: &NarrativeGraph,
) -> Vec<String> { ... }
```

**Usage in chat context assembly:**
```rust
let relevant = extract_relevant_ids(&input, &d.graph);
let graph_context = if relevant.is_empty() {
    d.graph.serialize_summary()  // fallback to summary
} else {
    d.graph.serialize_subgraph(&relevant)  // focused context
};
```

---

### Phase C: History Management

#### C1. Strip Graph from History

The full graph is prepended as a user message on every turn, so historical copies in `llm_history` are pure redundancy. Strip them.

**Files:** `gui/agent.rs:145`, `cli/chat.rs:178`

This is the single biggest quick win. The graph context message is always the first message in `msgs`. History messages should never contain graph dumps.

Currently, `llm_history` stores raw user input and assistant responses (no graph), which is correct. The issue is in the **multi-turn tool loop** (`agent.rs:265-277`) where the accumulated `messages` vec includes the initial graph context message plus all tool-turn messages. This compounds within a single user turn but doesn't persist to `llm_history` (which only stores the final user input + assistant text).

**However**, in `cli/chat.rs`, `history` is serialized to disk and reloaded, so if graph context ever leaked into history entries, it would persist. Add a safety check:

```rust
// When saving to history, ensure no graph context leaks
history.push(Message {
    role: Role::User,
    content: input.to_string(),  // raw user input only, never graph context
    ..
});
```

#### C2. Conversation Summarization (Later)

For longer sessions, summarize older history turns. Two approaches:

1. **Rule-based**: Keep last 6 messages verbatim, compress older ones to `"Earlier: user asked about X, assistant found Y"` format
2. **LLM-based**: Periodically call the LLM with history + "summarize this conversation so far" (costs one extra call but produces better summaries)

Start with rule-based. The LLM-based approach can be added when the rule-based proves insufficient.

---

### Phase D: Perspective Analysis Optimization

#### D1. Scene Summary Instead of Full Text

In `generate_perspective()` and `find_blind_spots()`, replace full scene text concatenation with scene summaries from the graph.

**File:** `src/concepts/character_perspective.rs:214-237`

```rust
// Instead of reading full scene text:
// let text = text_buffer.read(scene_span.byte_range()).unwrap_or_default();

// Use the graph's scene analysis (already computed during scan):
let summary = graph.get_scene_analysis(scene_id)
    .unwrap_or_else(|| "No analysis available".to_string());
let title = scene_span.title.as_deref().unwrap_or("(untitled)");
scene_texts.push(format!("Scene \"{title}\" ({scene_id}):\n{summary}"));
```

This reduces perspective context from potentially hundreds of KB to a few KB, since scene analyses are already concise summaries stored in the graph.

#### D2. Batched Blind Spot Analysis

For `find_blind_spots()` with many unseen scenes, process in batches:

```rust
const BLIND_SPOT_BATCH_SIZE: usize = 5;

// Process unseen scenes in batches, accumulating blind spots
for batch in unseen_scenes.chunks(BLIND_SPOT_BATCH_SIZE) {
    let batch_prompt = format!("...\n{}", batch.join("\n\n---\n\n"));
    let result = provider.complete(...).await?;
    blind_spots.extend(parse_blind_spots(&result)?);
}
```

---

### Phase E: Tool Schema Optimization

#### E1. Context-Appropriate Tool Sets

Different operations need different tools. Avoid sending all 22+ schemas when only a subset is relevant.

**File:** `src/concepts/skills.rs`

```rust
pub fn tool_schemas_for_context(&self, context: SkillSetContext) -> Vec<ToolSchema> {
    let allowed_categories = match context {
        SkillSetContext::Chat => None,  // all tools available
        SkillSetContext::Analysis => Some(vec![]),  // no tools needed
        SkillSetContext::Perspective => Some(vec![
            SkillCategory::PerspectiveTools,
            SkillCategory::GraphTools,
        ]),
    };
    // filter accordingly...
}
```

#### E2. Compact Schema Descriptions

Review tool descriptions for verbosity. Many include examples and detailed formatting instructions that could be shortened without losing utility.

---

## Implementation Order

| Step | What | Files | Effort | Impact |
|------|------|-------|--------|--------|
| **A1** | Token counting utility | new `context_budget.rs`, `agent.rs`, `chat.rs` | Small | Visibility |
| **A2** | Surface empty responses | `agent.rs:174-179`, `chat.rs:207-210` | Tiny | Fixes silent bug |
| **A3** | Log API response usage | `state.rs`, `agent.rs`, `status_bar.rs` | Small | Visibility |
| **B1** | Graph summary mode | `narrative_graph.rs` | Medium | High — 90% graph reduction |
| **C1** | Verify no graph in history | `agent.rs`, `chat.rs` | Tiny | Correctness check |
| **B2** | Relevant subgraph extraction | `narrative_graph.rs`, `context_budget.rs` | Medium | High for focused queries |
| **D1** | Scene summaries in perspectives | `character_perspective.rs` | Medium | High — 90%+ reduction |
| **E1** | Context-appropriate tool sets | `skills.rs` | Small | Medium |
| **D2** | Batched blind spot analysis | `character_perspective.rs` | Medium | High for large stories |
| **C2** | History summarization | `agent.rs`, `chat.rs` | Medium | Medium |
| **E2** | Compact tool descriptions | `skills.rs` | Small | Small |

---

## Token Budget Reference

Approximate context windows for common models:

| Model | Context Window | Practical Budget (80%) |
|-------|---------------|----------------------|
| Claude Sonnet/Opus | 200K tokens | 160K |
| GPT-4o | 128K tokens | 102K |
| Gemini 2.0 Flash | 1M tokens | 800K |
| Llama 3 (8B, local) | 8K tokens | 6.4K |
| Llama 3.1 (70B) | 128K tokens | 102K |
| Mistral Large | 128K tokens | 102K |

The "practical budget" is 80% of the window, reserving 20% for the completion. The hard-coded `max_tokens: 4096` in `provider.rs:244` should eventually become configurable per model.

---

## Success Criteria

1. **Every LLM call logs its estimated token usage** — visible in GUI status bar and CLI verbose output
2. **Empty responses surface as visible errors** with context about probable cause
3. **Graph context in chat drops from 5-150K tokens to 1-5K tokens** via summary mode
4. **Perspective analysis works on 100+ scene manuscripts** without exceeding context limits
5. **Token usage per chat turn is trackable** so regressions are caught during development
