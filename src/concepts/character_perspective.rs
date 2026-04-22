use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use std::path::Path;

use crate::concepts::narrative_graph::{CharacterId, GraphNode, NarrativeGraph};
use crate::concepts::provider::{Message, Provider, Role};
use crate::concepts::scene_map::SceneId;

const BLIND_SPOT_BATCH_SIZE: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Perspective {
    pub character_id: CharacterId,
    pub knowledge_boundary: HashSet<SceneId>,
    pub filtered_arc: Vec<PerspectiveObjectiveState>,
    pub interpretation_of_others: HashMap<CharacterId, String>,
    pub generated_at: DateTime<Utc>,
    pub graph_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenePerspective {
    pub character_id: CharacterId,
    pub scene_id: SceneId,
    pub wants: String,
    pub perceives: String,
    pub decides: String,
    pub blocked_by: Option<String>,
    pub emotional_state: String,
    pub knowledge_gained: Vec<String>,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerspectiveObjectiveState {
    pub scene_id: SceneId,
    pub objective: String,
    pub status: String,
    pub awareness: Awareness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Awareness {
    Full,
    Partial,
    Unaware,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonResult {
    pub scene_id: SceneId,
    pub character_a: CharacterId,
    pub character_b: CharacterId,
    pub perspective_a: ScenePerspective,
    pub perspective_b: ScenePerspective,
    pub divergences: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlindSpot {
    pub character_id: CharacterId,
    pub scene_id: SceneId,
    pub information: String,
    pub known_by: Vec<CharacterId>,
}

pub struct CharacterPerspective {
    perspectives: HashMap<CharacterId, Perspective>,
    scene_perspectives: HashMap<(CharacterId, SceneId), ScenePerspective>,
}

impl CharacterPerspective {
    pub fn new() -> Self {
        Self {
            perspectives: HashMap::new(),
            scene_perspectives: HashMap::new(),
        }
    }

    /// Get a cached perspective for a character
    #[allow(dead_code)]
    pub fn get_perspective(&self, character_id: &str) -> Option<&Perspective> {
        self.perspectives.get(character_id)
    }

    /// Store a generated perspective
    pub fn store_perspective(&mut self, perspective: Perspective) {
        self.perspectives
            .insert(perspective.character_id.clone(), perspective);
    }

    /// Get a cached scene perspective
    pub fn get_scene_perspective(
        &self,
        character_id: &str,
        scene_id: &str,
    ) -> Option<&ScenePerspective> {
        self.scene_perspectives
            .get(&(character_id.to_string(), scene_id.to_string()))
    }

    /// Store a generated scene perspective
    pub fn store_scene_perspective(&mut self, sp: ScenePerspective) {
        self.scene_perspectives
            .insert((sp.character_id.clone(), sp.scene_id.clone()), sp);
    }

    /// Invalidate all cached perspectives for a character
    #[allow(dead_code)]
    pub fn invalidate(&mut self, character_id: &str) {
        self.perspectives.remove(character_id);
        self.scene_perspectives
            .retain(|(cid, _), _| cid != character_id);
    }

    /// Invalidate a specific scene perspective
    #[allow(dead_code)]
    pub fn invalidate_scene(&mut self, character_id: &str, scene_id: &str) {
        self.scene_perspectives
            .remove(&(character_id.to_string(), scene_id.to_string()));
    }

    /// Save all cached perspectives to disk
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let wrapper = PerspectiveCache {
            perspectives: self.perspectives.clone(),
            scene_perspectives: self
                .scene_perspectives
                .iter()
                .map(|((cid, sid), sp)| (format!("{cid}::{sid}"), sp.clone()))
                .collect(),
        };
        let json = serde_json::to_string_pretty(&wrapper)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load cached perspectives from disk
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let wrapper: PerspectiveCache = serde_json::from_str(&content)?;
        let scene_perspectives = wrapper
            .scene_perspectives
            .into_iter()
            .filter_map(|(key, sp)| {
                let parts: Vec<&str> = key.splitn(2, "::").collect();
                if parts.len() == 2 {
                    Some(((parts[0].to_string(), parts[1].to_string()), sp))
                } else {
                    None
                }
            })
            .collect();
        Ok(Self {
            perspectives: wrapper.perspectives,
            scene_perspectives,
        })
    }

    /// Remove perspectives whose graph_hash doesn't match the current hash
    pub fn invalidate_by_graph_hash(&mut self, current_hash: &str) {
        self.perspectives
            .retain(|_, p| p.graph_hash == current_hash);
    }

    /// Compute knowledge boundary: set of scenes this character has witnessed
    /// Pure graph traversal, no LLM needed
    pub fn compute_knowledge_boundary(
        graph: &NarrativeGraph,
        character_id: &str,
    ) -> HashSet<SceneId> {
        let mut boundary = HashSet::new();
        for scene_node in graph.get_scenes() {
            if let GraphNode::Scene {
                id,
                characters_present,
                ..
            } = scene_node
                && characters_present.contains(&character_id.to_string())
            {
                boundary.insert(id.clone());
            }
        }
        boundary
    }

    /// Generate a full perspective for a character via LLM
    pub async fn generate_perspective(
        &mut self,
        character_id: &str,
        graph: &NarrativeGraph,
        provider: &mut Provider,
    ) -> Result<Perspective, crate::error::LairesError> {
        let knowledge_boundary = Self::compute_knowledge_boundary(graph, character_id);

        // Get character name
        let char_name = graph
            .get_node(character_id)
            .and_then(|n| {
                if let GraphNode::Character { name, .. } = n {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| character_id.to_string());

        // Use scene summaries from the graph instead of full text
        let mut scene_texts = Vec::new();
        for scene_id in &knowledge_boundary {
            if let Some(GraphNode::Scene { title, summary, .. }) = graph.get_node(scene_id) {
                let title = title.as_deref().unwrap_or("(untitled)");
                scene_texts.push(format!("Scene \"{title}\" ({scene_id}):\n{summary}"));
            }
        }

        let graph_json = graph.serialize_compact();
        let prompt = format!(
            "Analyze the story from the perspective of \"{char_name}\" (ID: {character_id}).\n\n\
             This character is present in {} scene(s). They can ONLY know about events they witnessed.\n\n\
             Narrative graph:\n```json\n{graph_json}\n```\n\n\
             Scenes this character witnesses:\n{}\n\n\
             Respond with JSON:\n```json\n{{\n\
               \"filtered_arc\": [{{ \"scene_id\": \"...\", \"objective\": \"...\", \"status\": \"active|advanced|blocked|achieved|abandoned\", \"awareness\": \"full|partial|unaware\" }}],\n\
               \"interpretation_of_others\": {{ \"character_id\": \"how they see this character\" }},\n\
             }}\n```",
            knowledge_boundary.len(),
            scene_texts.join("\n\n---\n\n"),
        );

        let messages = vec![Message {
            role: Role::User,
            content: prompt,
            tool_calls: None,
            tool_results: None,
        }];

        let system = "You are a narrative analysis engine. Analyze the story strictly from the given character's subjective perspective. Only include information from scenes they were present in.";

        let response = provider.complete(&messages, &[], Some(system)).await?;
        let raw = response.content.unwrap_or_default();
        let parsed =
            crate::concepts::analysis::extract_json(&raw).unwrap_or_else(|| serde_json::json!({}));

        let filtered_arc = parsed["filtered_arc"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|o| {
                        Some(PerspectiveObjectiveState {
                            scene_id: o["scene_id"].as_str()?.to_string(),
                            objective: o["objective"].as_str()?.to_string(),
                            status: o["status"].as_str().unwrap_or("active").to_string(),
                            awareness: match o["awareness"].as_str() {
                                Some("partial") => Awareness::Partial,
                                Some("unaware") => Awareness::Unaware,
                                _ => Awareness::Full,
                            },
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let interpretation_of_others = parsed["interpretation_of_others"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        let perspective = Perspective {
            character_id: character_id.to_string(),
            knowledge_boundary,
            filtered_arc,
            interpretation_of_others,
            generated_at: Utc::now(),
            graph_hash: blake3::hash(graph_json.as_bytes()).to_hex().to_string(),
        };

        self.store_perspective(perspective.clone());
        Ok(perspective)
    }

    /// Generate a scene perspective for a character via LLM
    pub async fn generate_scene_perspective(
        &mut self,
        character_id: &str,
        scene_id: &str,
        graph: &NarrativeGraph,
        scene_text: &str,
        provider: &mut Provider,
    ) -> Result<ScenePerspective, crate::error::LairesError> {
        let char_name = graph
            .get_node(character_id)
            .and_then(|n| {
                if let GraphNode::Character { name, .. } = n {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| character_id.to_string());

        let graph_json = graph.serialize_compact();
        let prompt = format!(
            "Analyze this scene from {char_name}'s perspective.\n\n\
             Graph context:\n```json\n{graph_json}\n```\n\n\
             Scene text:\n```\n{scene_text}\n```\n\n\
             Respond with JSON:\n```json\n{{\n\
               \"wants\": \"what they want in this scene\",\n\
               \"perceives\": \"what they observe/understand\",\n\
               \"decides\": \"what they choose to do\",\n\
               \"blocked_by\": \"what prevents their objective or null\",\n\
               \"emotional_state\": \"how they feel\",\n\
               \"knowledge_gained\": [\"new info they learn\"]\n\
             }}\n```"
        );

        let messages = vec![Message {
            role: Role::User,
            content: prompt,
            tool_calls: None,
            tool_results: None,
        }];

        let system = "You are a narrative analysis engine. Analyze the scene strictly from the named character's subjective perspective.";

        let response = provider.complete(&messages, &[], Some(system)).await?;
        let raw = response.content.unwrap_or_default();
        let parsed =
            crate::concepts::analysis::extract_json(&raw).unwrap_or_else(|| serde_json::json!({}));

        let sp = ScenePerspective {
            character_id: character_id.to_string(),
            scene_id: scene_id.to_string(),
            wants: parsed["wants"].as_str().unwrap_or("").to_string(),
            perceives: parsed["perceives"].as_str().unwrap_or("").to_string(),
            decides: parsed["decides"].as_str().unwrap_or("").to_string(),
            blocked_by: parsed["blocked_by"].as_str().map(String::from),
            emotional_state: parsed["emotional_state"].as_str().unwrap_or("").to_string(),
            knowledge_gained: parsed["knowledge_gained"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            content_hash: blake3::hash(scene_text.as_bytes()).to_hex().to_string(),
        };

        self.store_scene_perspective(sp.clone());
        Ok(sp)
    }

    /// Compare how two characters experience the same scene via LLM
    pub async fn compare_perspectives(
        &mut self,
        char_a: &str,
        char_b: &str,
        scene_id: &str,
        graph: &NarrativeGraph,
        scene_text: &str,
        provider: &mut Provider,
    ) -> Result<ComparisonResult, crate::error::LairesError> {
        // Generate scene perspectives for both characters if not cached
        let sp_a = if let Some(sp) = self.get_scene_perspective(char_a, scene_id) {
            sp.clone()
        } else {
            self.generate_scene_perspective(char_a, scene_id, graph, scene_text, provider)
                .await?
        };

        let sp_b = if let Some(sp) = self.get_scene_perspective(char_b, scene_id) {
            sp.clone()
        } else {
            self.generate_scene_perspective(char_b, scene_id, graph, scene_text, provider)
                .await?
        };

        // Ask LLM for divergences
        let prompt = format!(
            "Compare these two perspectives on the same scene and list the key divergences.\n\n\
             Perspective A ({char_a}):\n{}\n\n\
             Perspective B ({char_b}):\n{}\n\n\
             Respond with JSON: {{ \"divergences\": [\"string describing each divergence\"] }}",
            serde_json::to_string_pretty(&sp_a).unwrap_or_default(),
            serde_json::to_string_pretty(&sp_b).unwrap_or_default(),
        );

        let messages = vec![Message {
            role: Role::User,
            content: prompt,
            tool_calls: None,
            tool_results: None,
        }];

        let response = provider
            .complete(&messages, &[], Some("You are a narrative analysis engine."))
            .await?;
        let raw = response.content.unwrap_or_default();
        let parsed =
            crate::concepts::analysis::extract_json(&raw).unwrap_or_else(|| serde_json::json!({}));

        let divergences = parsed["divergences"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Ok(ComparisonResult {
            scene_id: scene_id.to_string(),
            character_a: char_a.to_string(),
            character_b: char_b.to_string(),
            perspective_a: sp_a,
            perspective_b: sp_b,
            divergences,
        })
    }

    /// Find blind spots: where a character is missing information the reader has
    pub async fn find_blind_spots(
        &mut self,
        character_id: &str,
        graph: &NarrativeGraph,
        provider: &mut Provider,
    ) -> Result<Vec<BlindSpot>, crate::error::LairesError> {
        let knowledge_boundary = Self::compute_knowledge_boundary(graph, character_id);

        let char_name = graph
            .get_node(character_id)
            .and_then(|n| {
                if let GraphNode::Character { name, .. } = n {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| character_id.to_string());

        // Collect scene summaries for scenes the character is NOT present in
        let mut unseen_scenes = Vec::new();
        for scene_node in graph.get_scenes() {
            if let GraphNode::Scene {
                id,
                characters_present,
                title,
                summary,
                ..
            } = scene_node
                && !characters_present.contains(&character_id.to_string())
            {
                let title_str = title.as_deref().unwrap_or("(untitled)");
                unseen_scenes.push(format!(
                    "Scene \"{title_str}\" ({id}) [present: {}]:\n{summary}",
                    characters_present.join(", ")
                ));
            }
        }

        if unseen_scenes.is_empty() {
            return Ok(vec![]);
        }

        // Process unseen scenes in batches to avoid exceeding context limits
        let mut blind_spots = Vec::new();
        for batch in unseen_scenes.chunks(BLIND_SPOT_BATCH_SIZE) {
            let prompt = format!(
                "Identify dramatic irony / blind spots for \"{char_name}\" (ID: {character_id}).\n\n\
                 This character is present in scenes: {:?}\n\n\
                 These are scenes they did NOT witness:\n{}\n\n\
                 For each piece of important information in the unseen scenes that {char_name} \
                 does not know about, respond with JSON:\n\
                 ```json\n{{ \"blind_spots\": [\n\
                   {{ \"scene_id\": \"...\", \"information\": \"what they don't know\", \
                      \"known_by\": [\"character_ids who do know\"] }}\n\
                 ] }}\n```",
                knowledge_boundary,
                batch.join("\n\n---\n\n"),
            );

            let messages = vec![Message {
                role: Role::User,
                content: prompt,
                tool_calls: None,
                tool_results: None,
            }];

            let response = provider
                .complete(
                    &messages,
                    &[],
                    Some("You are a narrative analysis engine. Identify dramatic irony."),
                )
                .await?;
            let raw = response.content.unwrap_or_default();
            let parsed = crate::concepts::analysis::extract_json(&raw)
                .unwrap_or_else(|| serde_json::json!({}));

            let batch_spots: Vec<BlindSpot> = parsed["blind_spots"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|b| {
                            Some(BlindSpot {
                                character_id: character_id.to_string(),
                                scene_id: b["scene_id"].as_str()?.to_string(),
                                information: b["information"].as_str()?.to_string(),
                                known_by: b["known_by"]
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

            blind_spots.extend(batch_spots);
        }

        Ok(blind_spots)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct PerspectiveCache {
    perspectives: HashMap<CharacterId, Perspective>,
    scene_perspectives: HashMap<String, ScenePerspective>,
}

impl Default for CharacterPerspective {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concepts::narrative_graph::GraphEdge;

    fn make_test_graph() -> NarrativeGraph {
        let mut g = NarrativeGraph::new();

        let char_a = "char_a".to_string();
        let char_b = "char_b".to_string();
        let scene_1 = "scene_1".to_string();
        let scene_2 = "scene_2".to_string();

        g.add_node(GraphNode::Character {
            id: char_a.clone(),
            name: "Marcus".to_string(),
            aliases: vec![],
            description: None,
        });

        g.add_node(GraphNode::Character {
            id: char_b.clone(),
            name: "Elena".to_string(),
            aliases: vec![],
            description: None,
        });

        g.add_node(GraphNode::Scene {
            id: scene_1.clone(),
            title: Some("Scene 1".to_string()),
            summary: "Both present.".to_string(),
            characters_present: vec![char_a.clone(), char_b.clone()],
            location: None,
            time: None,
            file_path: String::new(),
        });

        g.add_node(GraphNode::Scene {
            id: scene_2.clone(),
            title: Some("Scene 2".to_string()),
            summary: "Only Elena.".to_string(),
            characters_present: vec![char_b.clone()],
            location: None,
            time: None,
            file_path: String::new(),
        });

        g.add_edge(&char_a, &scene_1, GraphEdge::PresentIn);
        g.add_edge(&char_b, &scene_1, GraphEdge::PresentIn);
        g.add_edge(&char_b, &scene_2, GraphEdge::PresentIn);

        g
    }

    #[test]
    fn test_store_get_perspective() {
        let mut cp = CharacterPerspective::new();
        let p = Perspective {
            character_id: "char_a".to_string(),
            knowledge_boundary: ["scene_1".to_string()].into_iter().collect(),
            filtered_arc: vec![],
            interpretation_of_others: HashMap::new(),
            generated_at: Utc::now(),
            graph_hash: "abc".to_string(),
        };
        cp.store_perspective(p);
        assert!(cp.get_perspective("char_a").is_some());
        assert!(cp.get_perspective("char_b").is_none());
    }

    #[test]
    fn test_store_get_scene_perspective() {
        let mut cp = CharacterPerspective::new();
        let sp = ScenePerspective {
            character_id: "char_a".to_string(),
            scene_id: "scene_1".to_string(),
            wants: "Survive".to_string(),
            perceives: "Danger".to_string(),
            decides: "Fight".to_string(),
            blocked_by: None,
            emotional_state: "Tense".to_string(),
            knowledge_gained: vec![],
            content_hash: "hash".to_string(),
        };
        cp.store_scene_perspective(sp);
        assert!(cp.get_scene_perspective("char_a", "scene_1").is_some());
        assert!(cp.get_scene_perspective("char_a", "scene_2").is_none());
    }

    #[test]
    fn test_invalidate() {
        let mut cp = CharacterPerspective::new();
        let p = Perspective {
            character_id: "char_a".to_string(),
            knowledge_boundary: HashSet::new(),
            filtered_arc: vec![],
            interpretation_of_others: HashMap::new(),
            generated_at: Utc::now(),
            graph_hash: "abc".to_string(),
        };
        cp.store_perspective(p);
        let sp = ScenePerspective {
            character_id: "char_a".to_string(),
            scene_id: "scene_1".to_string(),
            wants: "".to_string(),
            perceives: "".to_string(),
            decides: "".to_string(),
            blocked_by: None,
            emotional_state: "".to_string(),
            knowledge_gained: vec![],
            content_hash: "".to_string(),
        };
        cp.store_scene_perspective(sp);

        cp.invalidate("char_a");
        assert!(cp.get_perspective("char_a").is_none());
        assert!(cp.get_scene_perspective("char_a", "scene_1").is_none());
    }

    #[test]
    fn test_invalidate_scene() {
        let mut cp = CharacterPerspective::new();
        let sp1 = ScenePerspective {
            character_id: "char_a".to_string(),
            scene_id: "scene_1".to_string(),
            wants: "".to_string(),
            perceives: "".to_string(),
            decides: "".to_string(),
            blocked_by: None,
            emotional_state: "".to_string(),
            knowledge_gained: vec![],
            content_hash: "".to_string(),
        };
        let sp2 = ScenePerspective {
            character_id: "char_a".to_string(),
            scene_id: "scene_2".to_string(),
            wants: "".to_string(),
            perceives: "".to_string(),
            decides: "".to_string(),
            blocked_by: None,
            emotional_state: "".to_string(),
            knowledge_gained: vec![],
            content_hash: "".to_string(),
        };
        cp.store_scene_perspective(sp1);
        cp.store_scene_perspective(sp2);

        cp.invalidate_scene("char_a", "scene_1");
        assert!(cp.get_scene_perspective("char_a", "scene_1").is_none());
        assert!(cp.get_scene_perspective("char_a", "scene_2").is_some());
    }

    #[test]
    fn test_compute_knowledge_boundary() {
        let g = make_test_graph();

        let kb_a = CharacterPerspective::compute_knowledge_boundary(&g, "char_a");
        assert_eq!(kb_a.len(), 1);
        assert!(kb_a.contains("scene_1"));

        let kb_b = CharacterPerspective::compute_knowledge_boundary(&g, "char_b");
        assert_eq!(kb_b.len(), 2);
        assert!(kb_b.contains("scene_1"));
        assert!(kb_b.contains("scene_2"));
    }

    #[test]
    fn test_perspective_save_load_roundtrip() {
        let mut cp = CharacterPerspective::new();
        let p = Perspective {
            character_id: "char_a".to_string(),
            knowledge_boundary: ["scene_1".to_string()].into_iter().collect(),
            filtered_arc: vec![],
            interpretation_of_others: HashMap::new(),
            generated_at: Utc::now(),
            graph_hash: "hash123".to_string(),
        };
        cp.store_perspective(p);
        let sp = ScenePerspective {
            character_id: "char_a".to_string(),
            scene_id: "scene_1".to_string(),
            wants: "Survive".to_string(),
            perceives: "Danger".to_string(),
            decides: "Fight".to_string(),
            blocked_by: None,
            emotional_state: "Tense".to_string(),
            knowledge_gained: vec!["secret".to_string()],
            content_hash: "hash456".to_string(),
        };
        cp.store_scene_perspective(sp);

        let dir = std::env::temp_dir().join("laires_test_perspectives");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("perspectives.json");

        cp.save(&path).unwrap();
        let loaded = CharacterPerspective::load(&path).unwrap();

        let lp = loaded.get_perspective("char_a").unwrap();
        assert_eq!(lp.graph_hash, "hash123");
        assert!(lp.knowledge_boundary.contains("scene_1"));

        let lsp = loaded.get_scene_perspective("char_a", "scene_1").unwrap();
        assert_eq!(lsp.wants, "Survive");
        assert_eq!(lsp.knowledge_gained, vec!["secret".to_string()]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_perspective_invalidation_by_hash() {
        let mut cp = CharacterPerspective::new();
        cp.store_perspective(Perspective {
            character_id: "char_a".to_string(),
            knowledge_boundary: HashSet::new(),
            filtered_arc: vec![],
            interpretation_of_others: HashMap::new(),
            generated_at: Utc::now(),
            graph_hash: "old_hash".to_string(),
        });
        cp.store_perspective(Perspective {
            character_id: "char_b".to_string(),
            knowledge_boundary: HashSet::new(),
            filtered_arc: vec![],
            interpretation_of_others: HashMap::new(),
            generated_at: Utc::now(),
            graph_hash: "current_hash".to_string(),
        });

        cp.invalidate_by_graph_hash("current_hash");
        assert!(cp.get_perspective("char_a").is_none());
        assert!(cp.get_perspective("char_b").is_some());
    }

    #[test]
    fn test_blind_spot_batch_size_constant() {
        assert_eq!(BLIND_SPOT_BATCH_SIZE, 5);
    }

    #[test]
    fn test_blind_spot_batching_logic() {
        // Verify chunks() produces expected batch counts
        let scenes: Vec<String> = (0..12).map(|i| format!("scene_{i}")).collect();
        let batches: Vec<&[String]> = scenes.chunks(BLIND_SPOT_BATCH_SIZE).collect();
        assert_eq!(batches.len(), 3); // 5 + 5 + 2
        assert_eq!(batches[0].len(), 5);
        assert_eq!(batches[1].len(), 5);
        assert_eq!(batches[2].len(), 2);

        // Exact multiple
        let scenes: Vec<String> = (0..10).map(|i| format!("scene_{i}")).collect();
        let batches: Vec<&[String]> = scenes.chunks(BLIND_SPOT_BATCH_SIZE).collect();
        assert_eq!(batches.len(), 2);

        // Fewer than batch size
        let scenes: Vec<String> = (0..3).map(|i| format!("scene_{i}")).collect();
        let batches: Vec<&[String]> = scenes.chunks(BLIND_SPOT_BATCH_SIZE).collect();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 3);
    }

    #[test]
    fn test_scene_summary_used_for_perspective() {
        // Verify the graph's scene summary is accessible for perspective generation
        let g = make_test_graph();
        let kb = CharacterPerspective::compute_knowledge_boundary(&g, "char_a");

        let mut scene_texts = Vec::new();
        for scene_id in &kb {
            if let Some(GraphNode::Scene { title, summary, .. }) = g.get_node(scene_id) {
                let title = title.as_deref().unwrap_or("(untitled)");
                scene_texts.push(format!("Scene \"{title}\" ({scene_id}):\n{summary}"));
            }
        }

        assert_eq!(scene_texts.len(), 1);
        assert!(scene_texts[0].contains("Scene \"Scene 1\""));
        assert!(scene_texts[0].contains("Both present."));
    }

    #[test]
    fn test_unseen_scenes_use_graph_summaries() {
        // Verify unseen scenes are collected from graph nodes with summaries
        let g = make_test_graph();
        let character_id = "char_a";
        let knowledge_boundary = CharacterPerspective::compute_knowledge_boundary(&g, character_id);

        let mut unseen_scenes = Vec::new();
        for scene_node in g.get_scenes() {
            if let GraphNode::Scene {
                id,
                characters_present,
                title,
                summary,
                ..
            } = scene_node
                && !characters_present.contains(&character_id.to_string())
            {
                let title_str = title.as_deref().unwrap_or("(untitled)");
                unseen_scenes.push(format!(
                    "Scene \"{title_str}\" ({id}) [present: {}]:\n{summary}",
                    characters_present.join(", ")
                ));
            }
        }

        // char_a is only in scene_1, so scene_2 should be unseen
        assert_eq!(unseen_scenes.len(), 1);
        assert!(unseen_scenes[0].contains("Scene \"Scene 2\""));
        assert!(unseen_scenes[0].contains("Only Elena."));
        assert!(unseen_scenes[0].contains("[present: char_b]"));
        assert!(!knowledge_boundary.contains("scene_2"));
    }
}
