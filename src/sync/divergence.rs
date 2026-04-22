use serde::{Deserialize, Serialize};

use crate::concepts::declared_intent::{DeclKey, DeclaredIntent};
use crate::concepts::narrative_graph::NarrativeGraph;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Divergence {
    pub node_id: String,
    pub field: String,
    pub inferred: String,
    pub declared: String,
    pub rationale: Option<String>,
}

/// Compare all declarations against current graph node fields.
/// Returns divergences where inferred and declared values differ.
pub fn detect_divergences(graph: &NarrativeGraph, intent: &DeclaredIntent) -> Vec<Divergence> {
    let mut divergences = Vec::new();

    for (key, decl) in intent.list_declarations() {
        if let Some(inferred) = graph.get_node_field(&key.node_id, &key.field)
            && inferred != decl.value
        {
            divergences.push(Divergence {
                node_id: key.node_id.clone(),
                field: key.field.clone(),
                inferred,
                declared: decl.value.clone(),
                rationale: decl.rationale.clone(),
            });
        }
    }

    divergences
}

/// Find declarations that reference nodes no longer in the graph.
pub fn detect_orphans(graph: &NarrativeGraph, intent: &DeclaredIntent) -> Vec<DeclKey> {
    let valid_ids: std::collections::HashSet<String> = graph.all_node_ids().into_iter().collect();
    intent.find_orphans(&valid_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concepts::narrative_graph::{GraphNode, new_id};

    #[test]
    fn test_no_divergence_when_matching() {
        let mut graph = NarrativeGraph::new();
        let id = new_id();
        graph.add_node(GraphNode::Character {
            id: id.clone(),
            name: "Marcus".to_string(),
            aliases: vec![],
            description: Some("A soldier".to_string()),
        });

        let mut intent = DeclaredIntent::new();
        intent.declare(&id, "description", "A soldier".to_string(), None);

        let divs = detect_divergences(&graph, &intent);
        assert!(divs.is_empty());
    }

    #[test]
    fn test_divergence_when_different() {
        let mut graph = NarrativeGraph::new();
        let id = new_id();
        graph.add_node(GraphNode::Character {
            id: id.clone(),
            name: "Marcus".to_string(),
            aliases: vec![],
            description: Some("A soldier".to_string()),
        });

        let mut intent = DeclaredIntent::new();
        intent.declare(
            &id,
            "description",
            "A vengeful warrior".to_string(),
            Some("Writer override".to_string()),
        );

        let divs = detect_divergences(&graph, &intent);
        assert_eq!(divs.len(), 1);
        assert_eq!(divs[0].inferred, "A soldier");
        assert_eq!(divs[0].declared, "A vengeful warrior");
        assert_eq!(divs[0].rationale.as_deref(), Some("Writer override"));
    }

    #[test]
    fn test_orphan_detection() {
        let mut graph = NarrativeGraph::new();
        let alive_id = new_id();
        graph.add_node(GraphNode::Character {
            id: alive_id.clone(),
            name: "Elena".to_string(),
            aliases: vec![],
            description: None,
        });

        let mut intent = DeclaredIntent::new();
        intent.declare(&alive_id, "name", "Elena".to_string(), None);
        intent.declare("deleted_node", "description", "Gone".to_string(), None);

        let orphans = detect_orphans(&graph, &intent);
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].node_id, "deleted_node");
    }

    #[test]
    fn test_no_divergence_for_nonexistent_field() {
        let mut graph = NarrativeGraph::new();
        let id = new_id();
        graph.add_node(GraphNode::Character {
            id: id.clone(),
            name: "Marcus".to_string(),
            aliases: vec![],
            description: None,
        });

        let mut intent = DeclaredIntent::new();
        // Declare a field that doesn't exist on the node (description is None)
        intent.declare(&id, "description", "Override".to_string(), None);

        // get_node_field returns None for description when it's None,
        // so no divergence is detected (no inferred value to compare against)
        let divs = detect_divergences(&graph, &intent);
        assert!(divs.is_empty());
    }
}
