use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

use crate::concepts::scene_map::SceneId;

pub type CharacterId = String;
pub type ObjectiveId = String;
pub type ConflictId = String;
pub type NodeId = String;

// -- Node Types --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphNode {
    Character {
        id: CharacterId,
        name: String,
        aliases: Vec<String>,
        description: Option<String>,
    },
    Objective {
        id: ObjectiveId,
        character_id: CharacterId,
        scope: Scope,
        description: String,
        evidence: Vec<String>,
        confidence: f64,
        status: Status,
    },
    Scene {
        id: SceneId,
        title: Option<String>,
        summary: String,
        characters_present: Vec<CharacterId>,
        location: Option<String>,
        time: Option<String>,
    },
    Conflict {
        id: ConflictId,
        description: String,
        objectives: Vec<ObjectiveId>,
    },
}

impl GraphNode {
    pub fn node_id(&self) -> &str {
        match self {
            GraphNode::Character { id, .. } => id,
            GraphNode::Objective { id, .. } => id,
            GraphNode::Scene { id, .. } => id,
            GraphNode::Conflict { id, .. } => id,
        }
    }

    pub fn node_type_name(&self) -> &'static str {
        match self {
            GraphNode::Character { .. } => "character",
            GraphNode::Objective { .. } => "objective",
            GraphNode::Scene { .. } => "scene",
            GraphNode::Conflict { .. } => "conflict",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    Overarching,
    Act,
    Scene,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Active,
    Achieved,
    Abandoned,
    Blocked,
    Transformed,
}

// -- Edge Types --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphEdge {
    Pursues {
        scene_id: Option<SceneId>,
    },
    DecomposesInto,
    ConflictsWith,
    PresentIn,
    Advances,
    Blocks,
    Precedes,
    Transforms {
        trigger_scene: SceneId,
    },
}

impl GraphEdge {
    pub fn edge_type_name(&self) -> &'static str {
        match self {
            GraphEdge::Pursues { .. } => "pursues",
            GraphEdge::DecomposesInto => "decomposes_into",
            GraphEdge::ConflictsWith => "conflicts_with",
            GraphEdge::PresentIn => "present_in",
            GraphEdge::Advances => "advances",
            GraphEdge::Blocks => "blocks",
            GraphEdge::Precedes => "precedes",
            GraphEdge::Transforms { .. } => "transforms",
        }
    }
}

// -- Serializable graph format --

#[derive(Debug, Serialize, Deserialize)]
pub struct SerializedGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<SerializedEdge>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SerializedEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub edge: GraphEdge,
}

// -- Query types --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectiveState {
    pub scene_id: SceneId,
    pub objective_id: ObjectiveId,
    pub description: String,
    pub status: Status,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneAnalysis {
    pub scene_id: SceneId,
    pub characters_present: Vec<CharacterId>,
    pub objectives_active: Vec<ObjectiveId>,
    pub objectives_advanced: Vec<ObjectiveId>,
    pub objectives_blocked: Vec<ObjectiveId>,
}

// -- Main Graph --

pub struct NarrativeGraph {
    graph: DiGraph<GraphNode, GraphEdge>,
    /// Map from node ID string to petgraph NodeIndex
    index_map: HashMap<NodeId, NodeIndex>,
}

impl NarrativeGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            index_map: HashMap::new(),
        }
    }

    /// Add a node to the graph, returns its ID
    pub fn add_node(&mut self, node: GraphNode) -> NodeId {
        let id = node.node_id().to_string();
        let idx = self.graph.add_node(node);
        self.index_map.insert(id.clone(), idx);
        id
    }

    /// Remove a node and all its edges
    pub fn remove_node(&mut self, id: &str) -> Option<GraphNode> {
        if let Some(&idx) = self.index_map.get(id) {
            let node = self.graph.remove_node(idx);
            self.index_map.remove(id);
            // Rebuild index map since petgraph reuses indices after removal
            self.rebuild_index_map();
            node
        } else {
            None
        }
    }

    /// Add an edge between two nodes
    pub fn add_edge(
        &mut self,
        from: &str,
        to: &str,
        edge: GraphEdge,
    ) -> Option<()> {
        let from_idx = *self.index_map.get(from)?;
        let to_idx = *self.index_map.get(to)?;
        self.graph.add_edge(from_idx, to_idx, edge);
        Some(())
    }

    /// Update a node by ID
    pub fn update_node(&mut self, id: &str, new_node: GraphNode) -> Option<()> {
        let idx = *self.index_map.get(id)?;
        self.graph[idx] = new_node;
        Some(())
    }

    /// Get a node by ID
    pub fn get_node(&self, id: &str) -> Option<&GraphNode> {
        let idx = *self.index_map.get(id)?;
        self.graph.node_weight(idx)
    }

    /// Get all nodes of a given type
    pub fn get_characters(&self) -> Vec<&GraphNode> {
        self.graph
            .node_weights()
            .filter(|n| matches!(n, GraphNode::Character { .. }))
            .collect()
    }

    pub fn get_objectives(&self) -> Vec<&GraphNode> {
        self.graph
            .node_weights()
            .filter(|n| matches!(n, GraphNode::Objective { .. }))
            .collect()
    }

    pub fn get_scenes(&self) -> Vec<&GraphNode> {
        self.graph
            .node_weights()
            .filter(|n| matches!(n, GraphNode::Scene { .. }))
            .collect()
    }

    pub fn get_conflicts(&self) -> Vec<&GraphNode> {
        self.graph
            .node_weights()
            .filter(|n| matches!(n, GraphNode::Conflict { .. }))
            .collect()
    }

    /// Get the character arc: ordered objective trajectory across scenes
    pub fn get_character_arc(
        &self,
        character_id: &str,
    ) -> Vec<ObjectiveState> {
        let mut arc = Vec::new();

        // Find all objectives for this character
        for node in self.graph.node_weights() {
            if let GraphNode::Objective {
                id,
                character_id: cid,
                description,
                status,
                ..
            } = node
            {
                if cid == character_id {
                    // Find scenes this objective is active in via edges
                    let obj_idx = self.index_map.get(id.as_str());
                    if let Some(&idx) = obj_idx {
                        // Check for Advances/Blocks edges pointing to this objective
                        for edge_ref in
                            self.graph.edges_directed(idx, Direction::Incoming)
                        {
                            let source = &self.graph[edge_ref.source()];
                            if let GraphNode::Scene {
                                id: scene_id, ..
                            } = source
                            {
                                arc.push(ObjectiveState {
                                    scene_id: scene_id.clone(),
                                    objective_id: id.clone(),
                                    description: description.clone(),
                                    status: *status,
                                });
                            }
                        }

                        // If no scene edges, still record the objective
                        if arc.iter().all(|a| a.objective_id != *id) {
                            arc.push(ObjectiveState {
                                scene_id: String::new(),
                                objective_id: id.clone(),
                                description: description.clone(),
                                status: *status,
                            });
                        }
                    }
                }
            }
        }

        arc
    }

    /// Find scenes where no objective changes state ("dead scenes")
    pub fn find_dead_scenes(&self) -> Vec<SceneId> {
        let mut dead = Vec::new();

        for node in self.graph.node_weights() {
            if let GraphNode::Scene { id, .. } = node {
                let idx = self.index_map.get(id.as_str());
                if let Some(&idx) = idx {
                    let has_advances_or_blocks = self
                        .graph
                        .edges_directed(idx, Direction::Outgoing)
                        .any(|e| {
                            matches!(
                                e.weight(),
                                GraphEdge::Advances | GraphEdge::Blocks
                            )
                        });

                    if !has_advances_or_blocks {
                        dead.push(id.clone());
                    }
                }
            }
        }

        dead
    }

    /// Get scene analysis: which objectives are active/advanced/blocked in a scene
    pub fn get_scene_analysis(&self, scene_id: &str) -> Option<SceneAnalysis> {
        let scene_idx = *self.index_map.get(scene_id)?;
        let scene_node = self.graph.node_weight(scene_idx)?;

        let characters_present = if let GraphNode::Scene {
            characters_present, ..
        } = scene_node
        {
            characters_present.clone()
        } else {
            return None;
        };

        let mut objectives_active = Vec::new();
        let mut objectives_advanced = Vec::new();
        let mut objectives_blocked = Vec::new();

        // Walk outgoing edges from the scene to find Advances/Blocks
        for edge_ref in self.graph.edges_directed(scene_idx, Direction::Outgoing) {
            let target_node = &self.graph[edge_ref.target()];
            if let GraphNode::Objective { id, .. } = target_node {
                match edge_ref.weight() {
                    GraphEdge::Advances => {
                        objectives_advanced.push(id.clone());
                    }
                    GraphEdge::Blocks => {
                        objectives_blocked.push(id.clone());
                    }
                    _ => {}
                }
            }
        }

        // Find objectives that are active for characters present in this scene
        // but not explicitly advanced or blocked
        for char_id in &characters_present {
            if let Some(&char_idx) = self.index_map.get(char_id.as_str()) {
                for edge_ref in self.graph.edges_directed(char_idx, Direction::Outgoing) {
                    if matches!(edge_ref.weight(), GraphEdge::Pursues { .. }) {
                        if let GraphNode::Objective { id, status, .. } =
                            &self.graph[edge_ref.target()]
                        {
                            if matches!(status, Status::Active)
                                && !objectives_advanced.contains(id)
                                && !objectives_blocked.contains(id)
                            {
                                if !objectives_active.contains(id) {
                                    objectives_active.push(id.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        Some(SceneAnalysis {
            scene_id: scene_id.to_string(),
            characters_present,
            objectives_active,
            objectives_advanced,
            objectives_blocked,
        })
    }

    /// Get a specific field value from a node by ID
    pub fn get_node_field(&self, node_id: &str, field: &str) -> Option<String> {
        let node = self.get_node(node_id)?;
        match node {
            GraphNode::Character {
                name,
                description,
                aliases,
                ..
            } => match field {
                "name" => Some(name.clone()),
                "description" => description.clone(),
                "aliases" => Some(
                    aliases
                        .iter()
                        .map(|a| a.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
                _ => None,
            },
            GraphNode::Objective {
                description,
                status,
                confidence,
                scope,
                ..
            } => match field {
                "description" => Some(description.clone()),
                "status" => Some(format!("{status:?}")),
                "confidence" => Some(format!("{confidence}")),
                "scope" => Some(format!("{scope:?}")),
                _ => None,
            },
            GraphNode::Scene {
                title,
                summary,
                location,
                time,
                ..
            } => match field {
                "title" => title.clone(),
                "summary" => Some(summary.clone()),
                "location" => location.clone(),
                "time" => time.clone(),
                _ => None,
            },
            GraphNode::Conflict { description, .. } => match field {
                "description" => Some(description.clone()),
                _ => None,
            },
        }
    }

    /// Get all node IDs
    pub fn all_node_ids(&self) -> Vec<NodeId> {
        self.index_map.keys().cloned().collect()
    }

    /// Get node count
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Get edge count
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Serialize to JSON
    pub fn serialize(&self) -> serde_json::Value {
        let nodes: Vec<&GraphNode> = self.graph.node_weights().collect();
        let edges: Vec<SerializedEdge> = self
            .graph
            .edge_indices()
            .filter_map(|ei| {
                let (from, to) = self.graph.edge_endpoints(ei)?;
                let from_node = &self.graph[from];
                let to_node = &self.graph[to];
                let edge = self.graph.edge_weight(ei)?;
                Some(SerializedEdge {
                    from: from_node.node_id().to_string(),
                    to: to_node.node_id().to_string(),
                    edge: edge.clone(),
                })
            })
            .collect();

        serde_json::json!({
            "nodes": nodes,
            "edges": edges,
        })
    }

    /// Serialize to a compact JSON string
    pub fn serialize_compact(&self) -> String {
        serde_json::to_string(&self.serialize()).unwrap_or_default()
    }

    /// Deserialize from JSON
    pub fn deserialize(data: &serde_json::Value) -> anyhow::Result<Self> {
        let serialized: SerializedGraph = serde_json::from_value(data.clone())?;
        let mut graph = Self::new();

        for node in serialized.nodes {
            graph.add_node(node);
        }

        for edge in serialized.edges {
            graph.add_edge(&edge.from, &edge.to, edge.edge);
        }

        Ok(graph)
    }

    /// Save to disk
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(&self.serialize())?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load from disk
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let data: serde_json::Value = serde_json::from_str(&content)?;
        Self::deserialize(&data)
    }

    /// Print a human-readable summary
    pub fn summary(&self) -> String {
        let characters = self.get_characters();
        let objectives = self.get_objectives();
        let scenes = self.get_scenes();
        let conflicts = self.get_conflicts();

        let mut out = String::new();
        out.push_str(&format!(
            "Narrative Graph: {} characters, {} objectives, {} scenes, {} conflicts\n",
            characters.len(),
            objectives.len(),
            scenes.len(),
            conflicts.len()
        ));

        if !characters.is_empty() {
            out.push_str("\nCharacters:\n");
            for c in &characters {
                if let GraphNode::Character {
                    name, description, ..
                } = c
                {
                    out.push_str(&format!(
                        "  - {}{}\n",
                        name,
                        description
                            .as_ref()
                            .map(|d| format!(": {d}"))
                            .unwrap_or_default()
                    ));
                }
            }
        }

        if !scenes.is_empty() {
            out.push_str("\nScenes:\n");
            for s in &scenes {
                if let GraphNode::Scene {
                    title, summary, ..
                } = s
                {
                    let label = title.as_deref().unwrap_or("(untitled)");
                    out.push_str(&format!("  - {label}: {summary}\n"));
                }
            }
        }

        if !conflicts.is_empty() {
            out.push_str("\nConflicts:\n");
            for c in &conflicts {
                if let GraphNode::Conflict { description, .. } = c {
                    out.push_str(&format!("  - {description}\n"));
                }
            }
        }

        out
    }

    fn rebuild_index_map(&mut self) {
        self.index_map.clear();
        for idx in self.graph.node_indices() {
            let node = &self.graph[idx];
            self.index_map.insert(node.node_id().to_string(), idx);
        }
    }
}

impl Default for NarrativeGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a new unique ID for graph nodes
pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_get_nodes() {
        let mut g = NarrativeGraph::new();

        let char_id = new_id();
        g.add_node(GraphNode::Character {
            id: char_id.clone(),
            name: "Marcus".to_string(),
            aliases: vec!["Marc".to_string()],
            description: Some("A conflicted soldier".to_string()),
        });

        let node = g.get_node(&char_id).unwrap();
        if let GraphNode::Character { name, .. } = node {
            assert_eq!(name, "Marcus");
        } else {
            panic!("Expected Character node");
        }
    }

    #[test]
    fn test_add_edges() {
        let mut g = NarrativeGraph::new();

        let char_id = new_id();
        let scene_id = new_id();

        g.add_node(GraphNode::Character {
            id: char_id.clone(),
            name: "Marcus".to_string(),
            aliases: vec![],
            description: None,
        });

        g.add_node(GraphNode::Scene {
            id: scene_id.clone(),
            title: Some("The Banquet".to_string()),
            summary: "Marcus confronts the general.".to_string(),
            characters_present: vec![char_id.clone()],
            location: Some("Great Hall".to_string()),
            time: None,
        });

        g.add_edge(&char_id, &scene_id, GraphEdge::PresentIn);
        assert_eq!(g.edge_count(), 1);
    }

    #[test]
    fn test_serialize_deserialize() {
        let mut g = NarrativeGraph::new();

        let char_id = new_id();
        g.add_node(GraphNode::Character {
            id: char_id.clone(),
            name: "Elena".to_string(),
            aliases: vec![],
            description: None,
        });

        let json = g.serialize();
        let g2 = NarrativeGraph::deserialize(&json).unwrap();
        assert_eq!(g2.node_count(), 1);
    }

    #[test]
    fn test_find_dead_scenes() {
        let mut g = NarrativeGraph::new();

        let scene_id = new_id();
        g.add_node(GraphNode::Scene {
            id: scene_id.clone(),
            title: Some("Empty Scene".to_string()),
            summary: "Nothing happens.".to_string(),
            characters_present: vec![],
            location: None,
            time: None,
        });

        let dead = g.find_dead_scenes();
        assert_eq!(dead.len(), 1);
        assert_eq!(dead[0], scene_id);
    }

    #[test]
    fn test_get_scene_analysis() {
        let mut g = NarrativeGraph::new();

        let char_id = new_id();
        g.add_node(GraphNode::Character {
            id: char_id.clone(),
            name: "Marcus".to_string(),
            aliases: vec![],
            description: None,
        });

        let scene_id = new_id();
        g.add_node(GraphNode::Scene {
            id: scene_id.clone(),
            title: Some("Battle".to_string()),
            summary: "A battle occurs.".to_string(),
            characters_present: vec![char_id.clone()],
            location: None,
            time: None,
        });

        let obj_active = new_id();
        g.add_node(GraphNode::Objective {
            id: obj_active.clone(),
            character_id: char_id.clone(),
            scope: Scope::Overarching,
            description: "Survive".to_string(),
            evidence: vec![],
            confidence: 0.9,
            status: Status::Active,
        });
        g.add_edge(&char_id, &obj_active, GraphEdge::Pursues { scene_id: None });

        let obj_advanced = new_id();
        g.add_node(GraphNode::Objective {
            id: obj_advanced.clone(),
            character_id: char_id.clone(),
            scope: Scope::Scene,
            description: "Win fight".to_string(),
            evidence: vec![],
            confidence: 0.8,
            status: Status::Active,
        });
        g.add_edge(&char_id, &obj_advanced, GraphEdge::Pursues { scene_id: None });
        g.add_edge(&scene_id, &obj_advanced, GraphEdge::Advances);

        let obj_blocked = new_id();
        g.add_node(GraphNode::Objective {
            id: obj_blocked.clone(),
            character_id: char_id.clone(),
            scope: Scope::Act,
            description: "Find ally".to_string(),
            evidence: vec![],
            confidence: 0.7,
            status: Status::Blocked,
        });
        g.add_edge(&char_id, &obj_blocked, GraphEdge::Pursues { scene_id: None });
        g.add_edge(&scene_id, &obj_blocked, GraphEdge::Blocks);

        let analysis = g.get_scene_analysis(&scene_id).unwrap();
        assert_eq!(analysis.scene_id, scene_id);
        assert_eq!(analysis.characters_present, vec![char_id]);
        assert!(analysis.objectives_advanced.contains(&obj_advanced));
        assert!(analysis.objectives_blocked.contains(&obj_blocked));
        assert!(analysis.objectives_active.contains(&obj_active));
    }

    #[test]
    fn test_get_node_field() {
        let mut g = NarrativeGraph::new();

        let char_id = new_id();
        g.add_node(GraphNode::Character {
            id: char_id.clone(),
            name: "Elena".to_string(),
            aliases: vec!["Lena".to_string()],
            description: Some("A spy".to_string()),
        });

        assert_eq!(g.get_node_field(&char_id, "name").unwrap(), "Elena");
        assert_eq!(g.get_node_field(&char_id, "description").unwrap(), "A spy");
        assert_eq!(g.get_node_field(&char_id, "aliases").unwrap(), "Lena");
        assert!(g.get_node_field(&char_id, "nonexistent").is_none());
        assert!(g.get_node_field("bogus", "name").is_none());
    }

    #[test]
    fn test_remove_node() {
        let mut g = NarrativeGraph::new();
        let id = new_id();
        g.add_node(GraphNode::Character {
            id: id.clone(),
            name: "Test".to_string(),
            aliases: vec![],
            description: None,
        });
        assert_eq!(g.node_count(), 1);
        g.remove_node(&id);
        assert_eq!(g.node_count(), 0);
    }
}
