use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use tracing::{debug, warn};

use crate::concepts::narrative_graph::{Scope, Status};
use crate::concepts::provider::{Message, Provider, Role};
use crate::concepts::scene_map::SceneId;
use crate::error::LairesError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisTask {
    pub kind: AnalysisKind,
    pub priority: Priority,
    pub created: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnalysisKind {
    FullAnalysis,
    SceneAnalysis { scene_id: SceneId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Priority {
    Normal,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub scene_id: Option<SceneId>,
    pub characters_found: Vec<CharacterData>,
    pub objectives_found: Vec<ObjectiveData>,
    pub conflicts_found: Vec<ConflictData>,
    pub scene_metadata: Option<SceneMetadata>,
    pub content_hash: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterData {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectiveData {
    pub character_name: String,
    pub scope: Scope,
    pub description: String,
    pub evidence: Vec<String>,
    pub confidence: f64,
    pub status: Status,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictData {
    pub description: String,
    pub between: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneMetadata {
    pub title: Option<String>,
    pub summary: String,
    pub characters_present: Vec<String>,
    pub location: Option<String>,
    pub time: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug)]
pub enum AnalysisStatus {
    Idle,
    Analyzing { current: AnalysisTask },
    Error { message: String },
}

pub struct Analysis {
    queue: BTreeSet<QueueEntry>,
    cache: HashMap<(SceneId, String), AnalysisResult>,
    status: AnalysisStatus,
}

/// Wrapper for ordering in the BTreeSet (by priority then timestamp)
#[derive(Debug, Clone)]
struct QueueEntry(AnalysisTask);

impl PartialEq for QueueEntry {
    fn eq(&self, other: &Self) -> bool {
        self.0.priority == other.0.priority && self.0.created == other.0.created
    }
}
impl Eq for QueueEntry {}

impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QueueEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .0
            .priority
            .cmp(&self.0.priority)
            .then(self.0.created.cmp(&other.0.created))
    }
}

impl Analysis {
    pub fn new() -> Self {
        Self {
            queue: BTreeSet::new(),
            cache: HashMap::new(),
            status: AnalysisStatus::Idle,
        }
    }

    /// Add a task to the analysis queue
    pub fn enqueue(&mut self, task: AnalysisTask) {
        self.queue.insert(QueueEntry(task));
    }

    /// Get the number of tasks in the queue
    #[allow(dead_code)]
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// Check cache for a previous analysis
    #[allow(dead_code)]
    pub fn get_cached(&self, scene_id: &str, content_hash: &str) -> Option<&AnalysisResult> {
        self.cache
            .get(&(scene_id.to_string(), content_hash.to_string()))
    }

    /// Invalidate cached results for a scene
    #[allow(dead_code)]
    pub fn invalidate(&mut self, scene_id: &str) {
        self.cache.retain(|(sid, _), _| sid != scene_id);
    }

    /// Process the next task in the queue using the LLM
    pub async fn process_next(
        &mut self,
        provider: &mut Provider,
        scene_text: &str,
        graph_context: &str,
        content_hash: &str,
    ) -> Result<Option<AnalysisResult>, LairesError> {
        let entry = match self.queue.pop_first() {
            Some(entry) => entry,
            None => return Ok(None),
        };

        let task = entry.0;
        self.status = AnalysisStatus::Analyzing {
            current: task.clone(),
        };

        let scene_id = match &task.kind {
            AnalysisKind::SceneAnalysis { scene_id } => Some(scene_id.clone()),
            AnalysisKind::FullAnalysis => None,
        };

        let prompt = build_analysis_prompt(scene_text, graph_context, scene_id.is_some());

        let messages = vec![Message {
            role: Role::User,
            content: prompt,
            tool_calls: None,
            tool_results: None,
        }];

        let system_prompt = ANALYSIS_SYSTEM_PROMPT;

        let response = provider.complete(&messages, &[], Some(system_prompt)).await;

        match response {
            Ok(resp) => {
                let raw_content = resp.content.unwrap_or_default();

                debug!(
                    scene_id = scene_id.as_deref().unwrap_or("full"),
                    response_len = raw_content.len(),
                    "LLM response received"
                );
                debug!(
                    raw_response = %raw_content,
                    "Raw LLM response content"
                );

                if raw_content.trim().is_empty() {
                    warn!(
                        scene_id = scene_id.as_deref().unwrap_or("full"),
                        "LLM returned empty response"
                    );
                }

                let result =
                    parse_analysis_response(&raw_content, scene_id.as_deref(), content_hash);

                // Only warn if the scene has enough text to expect characters
                // (short scenes like title pages legitimately have none)
                let word_count = scene_text.split_whitespace().count();
                if result.characters_found.is_empty()
                    && result.objectives_found.is_empty()
                    && !raw_content.trim().is_empty()
                    && word_count > 50
                {
                    warn!(
                        scene_id = scene_id.as_deref().unwrap_or("full"),
                        word_count,
                        "LLM returned content but parsing extracted no characters or objectives. \
                         Run with RUST_LOG=debug to see the raw response."
                    );
                }

                // Cache the result
                if let Some(sid) = &result.scene_id {
                    self.cache
                        .insert((sid.clone(), content_hash.to_string()), result.clone());
                }

                self.status = AnalysisStatus::Idle;
                Ok(Some(result))
            }
            Err(e) => {
                self.status = AnalysisStatus::Error {
                    message: e.to_string(),
                };
                Err(e)
            }
        }
    }

    #[allow(dead_code)]
    pub fn status(&self) -> &AnalysisStatus {
        &self.status
    }
}

impl Default for Analysis {
    fn default() -> Self {
        Self::new()
    }
}

const ANALYSIS_SYSTEM_PROMPT: &str = r#"You are a narrative analysis engine for Laires.ai. Your job is to analyze fiction text and extract structured narrative data.

You MUST respond with valid JSON matching this exact schema:

{
  "characters": [
    {
      "name": "string",
      "aliases": ["string"],
      "description": "string"
    }
  ],
  "objectives": [
    {
      "character_name": "string",
      "scope": "overarching" | "act" | "scene",
      "description": "string",
      "evidence": ["string (quote from text)"],
      "confidence": 0.0-1.0,
      "status": "active" | "achieved" | "abandoned" | "blocked" | "transformed"
    }
  ],
  "conflicts": [
    {
      "description": "string",
      "between": ["character_name", "character_name"]
    }
  ],
  "scene_metadata": {
    "title": "string or null",
    "summary": "string (1-2 sentences)",
    "characters_present": ["string"],
    "location": "string or null",
    "time": "string or null"
  }
}

IMPORTANT:
- The top-level "characters" array MUST list every character with full {name, aliases, description} objects — not just names.
- "scene_metadata.characters_present" is a separate list of just the names of characters physically present in the scene.
- Both arrays are required, even though they overlap. Do not omit the top-level "characters" array.
- Be precise. Extract only what the text supports. Use confidence scores honestly. If unsure about an objective, set confidence below 0.5."#;

fn build_analysis_prompt(scene_text: &str, graph_context: &str, is_scene_analysis: bool) -> String {
    let scope = if is_scene_analysis {
        "this scene"
    } else {
        "this manuscript"
    };

    format!(
        "Analyze {scope} and extract all characters, objectives, conflicts, and scene metadata.\n\n\
         Current narrative graph context:\n```json\n{graph_context}\n```\n\n\
         Text to analyze:\n```\n{scene_text}\n```\n\n\
         Respond with JSON only."
    )
}

/// Try multiple strategies to extract JSON from an LLM response
pub(crate) fn extract_json(response: &str) -> Option<serde_json::Value> {
    let trimmed = response.trim();

    // Strategy 1: Direct parse (response is pure JSON)
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Some(v);
    }

    // Strategy 2: Strip markdown code fences (```json ... ``` or ``` ... ```)
    let fence_re = Regex::new(r"(?s)```(?:json)?\s*\n?(.*?)\n?\s*```").unwrap();
    if let Some(caps) = fence_re.captures(trimmed)
        && let Some(inner) = caps.get(1)
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(inner.as_str().trim())
    {
        return Some(v);
    }

    // Strategy 3: Find the first { ... } block in the response
    if let Some(start) = trimmed.find('{') {
        // Find the matching closing brace
        let mut depth = 0;
        let mut end = None;
        for (i, ch) in trimmed[start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(start + i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Some(end) = end
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&trimmed[start..end])
        {
            return Some(v);
        }
    }

    None
}

/// Apply analysis results to the narrative graph (implements Sync S1.3).
/// Returns the set of character IDs affected (for perspective invalidation).
pub fn apply_analysis_to_graph(
    graph: &mut crate::concepts::narrative_graph::NarrativeGraph,
    result: &AnalysisResult,
    scene_id: &str,
    file_path: &str,
) -> Vec<String> {
    use crate::concepts::narrative_graph::*;

    graph.clear_scene_analysis(scene_id, false);

    // Add characters
    for char_data in &result.characters_found {
        let existing = graph.get_characters().iter().find_map(|c| {
            if let GraphNode::Character { id, name, .. } = c {
                if name.eq_ignore_ascii_case(&char_data.name) {
                    Some(id.clone())
                } else {
                    None
                }
            } else {
                None
            }
        });

        if existing.is_none() {
            let id = new_id();
            graph.add_node(GraphNode::Character {
                id: id.clone(),
                name: char_data.name.clone(),
                aliases: char_data.aliases.clone(),
                description: Some(char_data.description.clone()),
            });
        }
    }

    // Add/update scene node
    let scene_node_exists = graph.get_node(scene_id).is_some();
    let characters_present: Vec<String> = result
        .characters_found
        .iter()
        .map(|c| {
            graph
                .get_characters()
                .iter()
                .find_map(|gc| {
                    if let GraphNode::Character { id, name, .. } = gc {
                        if name.eq_ignore_ascii_case(&c.name) {
                            Some(id.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .unwrap_or_default()
        })
        .filter(|id| !id.is_empty())
        .collect();

    let scene_node = GraphNode::Scene {
        id: scene_id.to_string(),
        title: result.scene_metadata.as_ref().and_then(|m| m.title.clone()),
        summary: result
            .scene_metadata
            .as_ref()
            .map(|m| m.summary.clone())
            .unwrap_or_default(),
        characters_present: characters_present.clone(),
        location: result
            .scene_metadata
            .as_ref()
            .and_then(|m| m.location.clone()),
        time: result.scene_metadata.as_ref().and_then(|m| m.time.clone()),
        file_path: file_path.to_string(),
    };

    if scene_node_exists {
        graph.update_node(scene_id, scene_node);
    } else {
        graph.add_node(scene_node);
    }

    // Add PresentIn edges
    for char_id in &characters_present {
        graph.add_edge(char_id, scene_id, GraphEdge::PresentIn);
    }

    // Add objectives
    for obj_data in &result.objectives_found {
        let char_id = graph.get_characters().iter().find_map(|c| {
            if let GraphNode::Character { id, name, .. } = c {
                if name.eq_ignore_ascii_case(&obj_data.character_name) {
                    Some(id.clone())
                } else {
                    None
                }
            } else {
                None
            }
        });

        if let Some(cid) = char_id {
            let obj_id = new_id();
            graph.add_node(GraphNode::Objective {
                id: obj_id.clone(),
                character_id: cid.clone(),
                scope: obj_data.scope,
                description: obj_data.description.clone(),
                evidence: obj_data.evidence.clone(),
                confidence: obj_data.confidence,
                status: obj_data.status,
            });

            graph.add_edge(
                &cid,
                &obj_id,
                GraphEdge::Pursues {
                    scene_id: Some(scene_id.to_string()),
                },
            );

            match obj_data.status {
                Status::Blocked => {
                    graph.add_edge(scene_id, &obj_id, GraphEdge::Blocks);
                }
                _ => {
                    graph.add_edge(scene_id, &obj_id, GraphEdge::Advances);
                }
            }
        }
    }

    // Add conflicts
    for conflict_data in &result.conflicts_found {
        let conflict_id = new_id();
        let objective_ids: Vec<String> = conflict_data
            .between
            .iter()
            .filter_map(|name| {
                graph.get_characters().iter().find_map(|c| {
                    if let GraphNode::Character { id, name: n, .. } = c {
                        if n.eq_ignore_ascii_case(name) {
                            Some(id.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
            })
            .collect();

        graph.add_node(GraphNode::Conflict {
            id: conflict_id,
            description: conflict_data.description.clone(),
            objectives: objective_ids,
            scene_id: Some(scene_id.to_string()),
        });
    }

    characters_present
}

fn parse_analysis_response(
    response: &str,
    scene_id: Option<&str>,
    content_hash: &str,
) -> AnalysisResult {
    let parsed = match extract_json(response) {
        Some(v) => v,
        None => {
            warn!(
                scene_id = scene_id.unwrap_or("full"),
                response_preview = %response.chars().take(200).collect::<String>(),
                "Failed to extract JSON from LLM response"
            );
            serde_json::json!({})
        }
    };

    let mut characters_found: Vec<CharacterData> = parsed["characters"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    // Try "name" first, then "character_name"
                    let name = c["name"]
                        .as_str()
                        .or_else(|| c["character_name"].as_str())?
                        .to_string();
                    Some(CharacterData {
                        name,
                        aliases: c["aliases"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        description: c["description"].as_str().unwrap_or_default().to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let objectives_found = parsed["objectives"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|o| {
                    Some(ObjectiveData {
                        character_name: o["character_name"].as_str()?.to_string(),
                        scope: match o["scope"].as_str()? {
                            "overarching" => Scope::Overarching,
                            "act" => Scope::Act,
                            _ => Scope::Scene,
                        },
                        description: o["description"].as_str()?.to_string(),
                        evidence: o["evidence"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        confidence: o["confidence"].as_f64().unwrap_or(0.5),
                        status: match o["status"].as_str().unwrap_or("active") {
                            "achieved" => Status::Achieved,
                            "abandoned" => Status::Abandoned,
                            "blocked" => Status::Blocked,
                            "transformed" => Status::Transformed,
                            _ => Status::Active,
                        },
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let conflicts_found = parsed["conflicts"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    Some(ConflictData {
                        description: c["description"].as_str()?.to_string(),
                        between: c["between"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let scene_metadata = parsed.get("scene_metadata").and_then(|sm| {
        Some(SceneMetadata {
            title: sm["title"].as_str().map(String::from),
            summary: sm["summary"].as_str()?.to_string(),
            characters_present: sm["characters_present"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            location: sm["location"].as_str().map(String::from),
            time: sm["time"].as_str().map(String::from),
        })
    });

    // Fallback: if top-level characters is empty but scene_metadata has
    // characters_present, synthesize CharacterData from those names.
    if characters_found.is_empty()
        && let Some(ref meta) = scene_metadata
    {
        let known: std::collections::HashSet<String> = characters_found
            .iter()
            .map(|c| c.name.to_lowercase())
            .collect();
        for name in &meta.characters_present {
            if !known.contains(&name.to_lowercase()) {
                characters_found.push(CharacterData {
                    name: name.clone(),
                    aliases: Vec::new(),
                    description: String::new(),
                });
            }
        }
    }

    AnalysisResult {
        scene_id: scene_id.map(String::from),
        characters_found,
        objectives_found,
        conflicts_found,
        scene_metadata,
        content_hash: content_hash.to_string(),
        timestamp: Utc::now(),
    }
}
