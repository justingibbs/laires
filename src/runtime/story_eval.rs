use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::concepts::declared_intent::DeclaredIntent;
#[cfg(test)]
use crate::concepts::narrative_graph::{GraphNode, NarrativeGraph, Status};
#[cfg(test)]
use crate::runtime::story_access::StoryAccess;
#[cfg(test)]
use crate::sync::divergence;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StoryEvalSnapshot {
    pub scene_count: usize,
    pub stale_scene_count: usize,
    pub dead_scene_count: usize,
    pub divergence_count: usize,
    pub orphan_count: usize,
    pub total_words: usize,
    pub average_scene_words: usize,
    pub character_count: usize,
    pub objective_count: usize,
    pub conflict_count: usize,
    pub conflict_density: f32,
    pub unresolved_arc_count: usize,
    pub average_arc_completion_percent: usize,
}

impl StoryEvalSnapshot {
    #[cfg(test)]
    pub fn capture(
        graph: &NarrativeGraph,
        intent: &DeclaredIntent,
        story: &StoryAccess<'_>,
        stale_scene_count: usize,
    ) -> Self {
        let scene_count = story.scene_count();
        let total_words = story.word_count();
        let average_scene_words = if scene_count > 0 {
            total_words / scene_count
        } else {
            0
        };

        let dead_scene_count = graph.find_dead_scenes().len();
        let divergence_count = divergence::detect_divergences(graph, intent).len();
        let orphan_count = divergence::detect_orphans(graph, intent).len();
        let character_count = graph.get_characters().len();
        let objective_count = graph.get_objectives().len();
        let conflict_count = graph.get_conflicts().len();
        let conflict_density = if scene_count > 0 {
            conflict_count as f32 / scene_count as f32
        } else {
            0.0
        };

        let mut characters_with_arcs = 0usize;
        let mut unresolved_arc_count = 0usize;
        let mut total_arc_completion = 0.0f64;

        for character in graph.get_characters() {
            let GraphNode::Character { id, .. } = character else {
                continue;
            };
            let arc = graph.get_character_arc(id);
            if arc.is_empty() {
                continue;
            }
            characters_with_arcs += 1;
            let resolved = arc
                .iter()
                .filter(|objective| {
                    matches!(
                        objective.status,
                        Status::Achieved | Status::Abandoned | Status::Transformed
                    )
                })
                .count();
            if resolved < arc.len() {
                unresolved_arc_count += 1;
            }
            total_arc_completion += resolved as f64 / arc.len() as f64;
        }

        let average_arc_completion_percent = if characters_with_arcs > 0 {
            ((total_arc_completion / characters_with_arcs as f64) * 100.0).round() as usize
        } else {
            100
        };

        Self {
            scene_count,
            stale_scene_count,
            dead_scene_count,
            divergence_count,
            orphan_count,
            total_words,
            average_scene_words,
            character_count,
            objective_count,
            conflict_count,
            conflict_density,
            unresolved_arc_count,
            average_arc_completion_percent,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::concepts::declared_intent::DeclaredIntent;
    use crate::concepts::narrative_graph::{GraphEdge, GraphNode, NarrativeGraph, Scope, new_id};
    use crate::concepts::scene_map::{ParseMode, SceneMap};
    use crate::concepts::text_buffer::TextBuffer;
    use crate::runtime::story_access::StoryAccess;

    use super::StoryEvalSnapshot;

    #[test]
    fn capture_counts_story_metrics() {
        let text = "## Scene 1\n\nEnough words to count as one real scene.\n\n## Scene 2\n\nEnough words to count as scene two as well.";
        let text_buffer = TextBuffer::from_str(text, PathBuf::from("/tmp/story.md"));
        let mut scene_map = SceneMap::new(ParseMode::Prose);
        scene_map.full_reindex(text, "story.md");

        let mut graph = NarrativeGraph::new();
        let scene_ids: Vec<String> = scene_map
            .list_scenes()
            .iter()
            .map(|scene| scene.id.clone())
            .collect();
        let character_id = new_id();
        let objective_id = new_id();

        graph.add_node(GraphNode::Character {
            id: character_id.clone(),
            name: "Elena".to_string(),
            aliases: vec![],
            description: Some("Lead".to_string()),
        });
        graph.add_node(GraphNode::Objective {
            id: objective_id.clone(),
            character_id: character_id.clone(),
            scope: Scope::Scene,
            description: "Get the documents".to_string(),
            evidence: vec![],
            confidence: 0.9,
            status: crate::concepts::narrative_graph::Status::Active,
        });
        graph.add_node(GraphNode::Scene {
            id: scene_ids[0].clone(),
            title: Some("Scene 1".to_string()),
            summary: "An infiltration".to_string(),
            characters_present: vec![character_id.clone()],
            location: None,
            time: None,
            file_path: "story.md".to_string(),
        });
        graph.add_node(GraphNode::Scene {
            id: scene_ids[1].clone(),
            title: Some("Scene 2".to_string()),
            summary: "Aftermath".to_string(),
            characters_present: vec![character_id.clone()],
            location: None,
            time: None,
            file_path: "story.md".to_string(),
        });
        graph.add_edge(&character_id, &scene_ids[0], GraphEdge::PresentIn);
        graph.add_edge(&scene_ids[0], &objective_id, GraphEdge::Advances);

        let mut intent = DeclaredIntent::new();
        intent.declare(&character_id, "description", "Different".to_string(), None);

        let story = StoryAccess::new(&text_buffer, &scene_map, None);
        let snapshot = StoryEvalSnapshot::capture(&graph, &intent, &story, 1);

        assert_eq!(snapshot.scene_count, 2);
        assert_eq!(snapshot.stale_scene_count, 1);
        assert_eq!(snapshot.dead_scene_count, 1);
        assert_eq!(snapshot.divergence_count, 1);
        assert_eq!(snapshot.orphan_count, 0);
        assert_eq!(snapshot.character_count, 1);
        assert_eq!(snapshot.objective_count, 1);
        assert_eq!(snapshot.conflict_count, 0);
        assert_eq!(snapshot.unresolved_arc_count, 1);
        assert_eq!(snapshot.average_arc_completion_percent, 0);
        assert!(snapshot.total_words > 0);
        assert!(snapshot.average_scene_words > 0);
    }
}
