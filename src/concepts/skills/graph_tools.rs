use crate::concepts::declared_intent::DeclaredIntent;
use crate::concepts::narrative_graph::{GraphNode, NarrativeGraph};
use crate::runtime::story_access::StoryAccess;
use crate::sync::divergence;

use super::{find_character_id, Skills};

impl Skills {
    pub(super) fn exec_query_graph(
        &self,
        args: &serde_json::Value,
        graph: &NarrativeGraph,
    ) -> serde_json::Value {
        let node_type = args["node_type"].as_str();

        let nodes: Vec<&GraphNode> = match node_type {
            Some("character") => graph.get_characters(),
            Some("objective") => graph.get_objectives(),
            Some("scene") => graph.get_scenes(),
            Some("conflict") => graph.get_conflicts(),
            _ => {
                let mut all = Vec::new();
                all.extend(graph.get_characters());
                all.extend(graph.get_objectives());
                all.extend(graph.get_scenes());
                all.extend(graph.get_conflicts());
                all
            }
        };

        serde_json::json!({
            "node_count": nodes.len(),
            "nodes": nodes,
        })
    }

    pub(super) fn exec_get_character_arc(
        &self,
        args: &serde_json::Value,
        graph: &NarrativeGraph,
    ) -> serde_json::Value {
        let character_name = args["character"].as_str().unwrap_or("");
        match find_character_id(graph, character_name) {
            Some(id) => {
                let arc = graph.get_character_arc(&id);
                serde_json::json!({
                    "character": character_name,
                    "arc": arc,
                })
            }
            None => serde_json::json!({
                "error": format!("Character not found: {character_name}")
            }),
        }
    }

    pub(super) fn exec_get_conflicts(
        &self,
        _args: &serde_json::Value,
        graph: &NarrativeGraph,
    ) -> serde_json::Value {
        let conflicts = graph.get_conflicts();
        serde_json::json!({
            "conflict_count": conflicts.len(),
            "conflicts": conflicts,
        })
    }

    pub(super) fn exec_find_dead_scenes(
        &self,
        graph: &NarrativeGraph,
    ) -> serde_json::Value {
        let dead = graph.find_dead_scenes();
        serde_json::json!({
            "dead_scene_count": dead.len(),
            "dead_scenes": dead,
        })
    }

    pub(super) fn exec_get_scene_analysis(
        &self,
        args: &serde_json::Value,
        graph: &NarrativeGraph,
        story: &StoryAccess<'_>,
    ) -> serde_json::Value {
        let scene_ref = args["scene"].as_str().unwrap_or("1");

        match story
            .resolve_scene_id(scene_ref)
            .and_then(|id| graph.get_scene_analysis(&id))
        {
            Some(analysis) => serde_json::json!(analysis),
            None => serde_json::json!({
                "error": format!("Scene not found or not in graph: {scene_ref}")
            }),
        }
    }

    pub(super) fn exec_get_divergences(
        &self,
        graph: &NarrativeGraph,
        intent: Option<&DeclaredIntent>,
    ) -> serde_json::Value {
        match intent {
            Some(intent) => {
                let divs = divergence::detect_divergences(graph, intent);
                let orphans = divergence::detect_orphans(graph, intent);
                serde_json::json!({
                    "divergence_count": divs.len(),
                    "divergences": divs,
                    "orphan_count": orphans.len(),
                    "orphans": orphans,
                })
            }
            None => serde_json::json!({
                "divergence_count": 0,
                "divergences": [],
                "orphan_count": 0,
                "orphans": [],
                "note": "No writer overrides loaded"
            }),
        }
    }
}
