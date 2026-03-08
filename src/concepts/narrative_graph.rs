use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use uuid::Uuid;

use crate::concepts::scene_map::SceneId;

pub type CharacterId = String;
pub type ObjectiveId = String;
pub type ConflictId = String;
pub type NodeId = String;

// -- Node Types --

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
        #[serde(default)]
        file_path: String,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Access the underlying petgraph DiGraph (read-only).
    pub fn inner_graph(&self) -> &DiGraph<GraphNode, GraphEdge> {
        &self.graph
    }

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
                file_path,
                ..
            } => match field {
                "title" => title.clone(),
                "summary" => Some(summary.clone()),
                "location" => location.clone(),
                "time" => time.clone(),
                "file_path" => Some(file_path.clone()),
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

    /// Compact summary: names and IDs only, no descriptions/evidence/summaries.
    /// ~5-10% the size of serialize_compact(). The LLM can use query_graph or
    /// get_scene_analysis tools to access full details.
    pub fn serialize_summary(&self) -> String {
        let characters: Vec<_> = self
            .get_characters()
            .iter()
            .filter_map(|n| {
                if let GraphNode::Character { id, name, .. } = n {
                    Some(serde_json::json!({ "id": id, "name": name }))
                } else {
                    None
                }
            })
            .collect();

        let scenes: Vec<_> = self
            .get_scenes()
            .iter()
            .filter_map(|n| {
                if let GraphNode::Scene {
                    id,
                    title,
                    characters_present,
                    ..
                } = n
                {
                    Some(serde_json::json!({
                        "id": id,
                        "title": title,
                        "characters": characters_present,
                    }))
                } else {
                    None
                }
            })
            .collect();

        let objectives: Vec<_> = self
            .get_objectives()
            .iter()
            .filter_map(|n| {
                if let GraphNode::Objective {
                    id,
                    description,
                    status,
                    ..
                } = n
                {
                    Some(serde_json::json!({ "id": id, "label": description, "status": status }))
                } else {
                    None
                }
            })
            .collect();

        let conflicts: Vec<_> = self
            .get_conflicts()
            .iter()
            .filter_map(|n| {
                if let GraphNode::Conflict {
                    id, description, ..
                } = n
                {
                    Some(serde_json::json!({ "id": id, "label": description }))
                } else {
                    None
                }
            })
            .collect();

        serde_json::to_string(&serde_json::json!({
            "characters": characters,
            "scenes": scenes,
            "objectives": objectives,
            "conflicts": conflicts,
            "edge_count": self.edge_count(),
            "note": "This is a summary. Use query_graph or get_scene_analysis tools for full details."
        }))
        .unwrap_or_default()
    }

    /// Serialize only nodes matching the given IDs + their 1-hop neighbors,
    /// plus all edges between included nodes.
    pub fn serialize_subgraph(&self, node_ids: &[&str]) -> String {
        let mut included: HashSet<NodeIndex> = HashSet::new();

        // Add seed nodes
        for id in node_ids {
            if let Some(&idx) = self.index_map.get(*id) {
                included.insert(idx);
            }
        }

        // Add 1-hop neighbors
        let seeds: Vec<NodeIndex> = included.iter().copied().collect();
        for idx in seeds {
            for edge in self.graph.edges_directed(idx, Direction::Outgoing) {
                included.insert(edge.target());
            }
            for edge in self.graph.edges_directed(idx, Direction::Incoming) {
                included.insert(edge.source());
            }
        }

        // Serialize included nodes
        let nodes: Vec<&GraphNode> = included
            .iter()
            .filter_map(|&idx| self.graph.node_weight(idx))
            .collect();

        // Serialize edges between included nodes
        let edges: Vec<SerializedEdge> = self
            .graph
            .edge_indices()
            .filter_map(|ei| {
                let (from, to) = self.graph.edge_endpoints(ei)?;
                if included.contains(&from) && included.contains(&to) {
                    let from_node = &self.graph[from];
                    let to_node = &self.graph[to];
                    let edge = self.graph.edge_weight(ei)?;
                    Some(SerializedEdge {
                        from: from_node.node_id().to_string(),
                        to: to_node.node_id().to_string(),
                        edge: edge.clone(),
                    })
                } else {
                    None
                }
            })
            .collect();

        serde_json::to_string(&serde_json::json!({
            "nodes": nodes,
            "edges": edges,
            "note": "Subgraph for relevant nodes. Use query_graph or get_scene_analysis tools for other nodes."
        }))
        .unwrap_or_default()
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
                    title,
                    summary,
                    file_path,
                    ..
                } = s
                {
                    let label = title.as_deref().unwrap_or("(untitled)");
                    let prefix = if file_path.is_empty() {
                        String::new()
                    } else {
                        format!("[{file_path}] ")
                    };
                    out.push_str(&format!("  - {prefix}{label}: {summary}\n"));
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

// -- Graph Diff --

#[derive(Debug)]
pub struct GraphDiff {
    pub added_nodes: Vec<GraphNode>,
    pub removed_nodes: Vec<GraphNode>,
    pub changed_nodes: Vec<NodeChange>,
    pub added_edges: Vec<SerializedEdge>,
    pub removed_edges: Vec<SerializedEdge>,
}

#[derive(Debug)]
pub struct NodeChange {
    pub id: NodeId,
    pub old: GraphNode,
    pub new: GraphNode,
    pub fields: Vec<String>,
}

impl GraphDiff {
    pub fn is_empty(&self) -> bool {
        self.added_nodes.is_empty()
            && self.removed_nodes.is_empty()
            && self.changed_nodes.is_empty()
            && self.added_edges.is_empty()
            && self.removed_edges.is_empty()
    }
}

/// Compare two graphs and return their differences
pub fn diff_graphs(old: &NarrativeGraph, new: &NarrativeGraph) -> GraphDiff {
    let old_ids: HashMap<&str, &GraphNode> = old
        .graph
        .node_weights()
        .map(|n| (n.node_id(), n))
        .collect();
    let new_ids: HashMap<&str, &GraphNode> = new
        .graph
        .node_weights()
        .map(|n| (n.node_id(), n))
        .collect();

    let mut added_nodes = Vec::new();
    let mut removed_nodes = Vec::new();
    let mut changed_nodes = Vec::new();

    // Find added and changed nodes
    for (&id, &new_node) in &new_ids {
        match old_ids.get(id) {
            None => added_nodes.push(new_node.clone()),
            Some(&old_node) => {
                if old_node != new_node {
                    let fields = describe_node_changes(old_node, new_node);
                    changed_nodes.push(NodeChange {
                        id: id.to_string(),
                        old: old_node.clone(),
                        new: new_node.clone(),
                        fields,
                    });
                }
            }
        }
    }

    // Find removed nodes
    for (&id, &old_node) in &old_ids {
        if !new_ids.contains_key(id) {
            removed_nodes.push(old_node.clone());
        }
    }

    // Compare edges
    let old_edges = collect_edges(old);
    let new_edges = collect_edges(new);

    let added_edges: Vec<SerializedEdge> = new_edges
        .iter()
        .filter(|e| !old_edges.contains(e))
        .cloned()
        .collect();

    let removed_edges: Vec<SerializedEdge> = old_edges
        .iter()
        .filter(|e| !new_edges.contains(e))
        .cloned()
        .collect();

    GraphDiff {
        added_nodes,
        removed_nodes,
        changed_nodes,
        added_edges,
        removed_edges,
    }
}

fn collect_edges(graph: &NarrativeGraph) -> Vec<SerializedEdge> {
    graph
        .graph
        .edge_indices()
        .filter_map(|ei| {
            let (from, to) = graph.graph.edge_endpoints(ei)?;
            let from_node = &graph.graph[from];
            let to_node = &graph.graph[to];
            let edge = graph.graph.edge_weight(ei)?;
            Some(SerializedEdge {
                from: from_node.node_id().to_string(),
                to: to_node.node_id().to_string(),
                edge: edge.clone(),
            })
        })
        .collect()
}

fn describe_node_changes(old: &GraphNode, new: &GraphNode) -> Vec<String> {
    let mut changes = Vec::new();
    match (old, new) {
        (
            GraphNode::Character {
                name: on,
                description: od,
                aliases: oa,
                ..
            },
            GraphNode::Character {
                name: nn,
                description: nd,
                aliases: na,
                ..
            },
        ) => {
            if on != nn {
                changes.push(format!("name: {on} → {nn}"));
            }
            if od != nd {
                changes.push("description changed".to_string());
            }
            if oa != na {
                changes.push("aliases changed".to_string());
            }
        }
        (
            GraphNode::Objective {
                description: od,
                status: os,
                confidence: oc,
                scope: osc,
                ..
            },
            GraphNode::Objective {
                description: nd,
                status: ns,
                confidence: nc,
                scope: nsc,
                ..
            },
        ) => {
            if od != nd {
                changes.push("description changed".to_string());
            }
            if os != ns {
                changes.push(format!("status: {os:?} → {ns:?}"));
            }
            if (oc - nc).abs() > f64::EPSILON {
                changes.push(format!("confidence: {oc:.2} → {nc:.2}"));
            }
            if osc != nsc {
                changes.push(format!("scope: {osc:?} → {nsc:?}"));
            }
        }
        (
            GraphNode::Scene {
                title: ot,
                summary: os,
                characters_present: ocp,
                location: ol,
                time: otm,
                ..
            },
            GraphNode::Scene {
                title: nt,
                summary: ns,
                characters_present: ncp,
                location: nl,
                time: ntm,
                ..
            },
        ) => {
            if ot != nt {
                changes.push(format!(
                    "title: {} → {}",
                    ot.as_deref().unwrap_or("(none)"),
                    nt.as_deref().unwrap_or("(none)")
                ));
            }
            if os != ns {
                changes.push("summary changed".to_string());
            }
            if ocp != ncp {
                changes.push("characters_present changed".to_string());
            }
            if ol != nl {
                changes.push("location changed".to_string());
            }
            if otm != ntm {
                changes.push("time changed".to_string());
            }
        }
        (
            GraphNode::Conflict {
                description: od,
                objectives: oo,
                ..
            },
            GraphNode::Conflict {
                description: nd,
                objectives: no,
                ..
            },
        ) => {
            if od != nd {
                changes.push("description changed".to_string());
            }
            if oo != no {
                changes.push("objectives changed".to_string());
            }
        }
        _ => {
            changes.push(format!(
                "type changed: {} → {}",
                old.node_type_name(),
                new.node_type_name()
            ));
        }
    }
    changes
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
            file_path: String::new(),
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
            file_path: String::new(),
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
            file_path: String::new(),
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

    #[test]
    fn test_file_path_on_scene_node() {
        let mut g = NarrativeGraph::new();
        let scene_id = new_id();
        g.add_node(GraphNode::Scene {
            id: scene_id.clone(),
            title: Some("Opening".to_string()),
            summary: "The story begins.".to_string(),
            characters_present: vec![],
            location: None,
            time: None,
            file_path: "chapter-1.md".to_string(),
        });

        assert_eq!(
            g.get_node_field(&scene_id, "file_path").unwrap(),
            "chapter-1.md"
        );
    }

    #[test]
    fn test_diff_graphs_added_nodes() {
        let old = NarrativeGraph::new();
        let mut new = NarrativeGraph::new();
        new.add_node(GraphNode::Character {
            id: "char-1".to_string(),
            name: "Alice".to_string(),
            aliases: vec![],
            description: None,
        });

        let diff = diff_graphs(&old, &new);
        assert_eq!(diff.added_nodes.len(), 1);
        assert!(diff.removed_nodes.is_empty());
        assert!(diff.changed_nodes.is_empty());
    }

    #[test]
    fn test_diff_graphs_removed_nodes() {
        let mut old = NarrativeGraph::new();
        old.add_node(GraphNode::Character {
            id: "char-1".to_string(),
            name: "Alice".to_string(),
            aliases: vec![],
            description: None,
        });
        let new = NarrativeGraph::new();

        let diff = diff_graphs(&old, &new);
        assert!(diff.added_nodes.is_empty());
        assert_eq!(diff.removed_nodes.len(), 1);
        assert!(diff.changed_nodes.is_empty());
    }

    #[test]
    fn test_diff_graphs_changed_nodes() {
        let mut old = NarrativeGraph::new();
        old.add_node(GraphNode::Objective {
            id: "obj-1".to_string(),
            character_id: "char-1".to_string(),
            scope: Scope::Overarching,
            description: "Survive".to_string(),
            evidence: vec![],
            confidence: 0.9,
            status: Status::Active,
        });

        let mut new = NarrativeGraph::new();
        new.add_node(GraphNode::Objective {
            id: "obj-1".to_string(),
            character_id: "char-1".to_string(),
            scope: Scope::Overarching,
            description: "Survive".to_string(),
            evidence: vec![],
            confidence: 0.9,
            status: Status::Achieved,
        });

        let diff = diff_graphs(&old, &new);
        assert!(diff.added_nodes.is_empty());
        assert!(diff.removed_nodes.is_empty());
        assert_eq!(diff.changed_nodes.len(), 1);
        assert!(diff.changed_nodes[0]
            .fields
            .iter()
            .any(|f| f.contains("status")));
    }

    #[test]
    fn test_diff_graphs_edge_changes() {
        let mut old = NarrativeGraph::new();
        old.add_node(GraphNode::Character {
            id: "c1".to_string(),
            name: "A".to_string(),
            aliases: vec![],
            description: None,
        });
        old.add_node(GraphNode::Scene {
            id: "s1".to_string(),
            title: None,
            summary: "test".to_string(),
            characters_present: vec![],
            location: None,
            time: None,
            file_path: String::new(),
        });
        old.add_edge("c1", "s1", GraphEdge::PresentIn);

        let mut new = NarrativeGraph::new();
        new.add_node(GraphNode::Character {
            id: "c1".to_string(),
            name: "A".to_string(),
            aliases: vec![],
            description: None,
        });
        new.add_node(GraphNode::Scene {
            id: "s1".to_string(),
            title: None,
            summary: "test".to_string(),
            characters_present: vec![],
            location: None,
            time: None,
            file_path: String::new(),
        });
        // Edge removed, new edge added
        new.add_edge("s1", "c1", GraphEdge::Advances);

        let diff = diff_graphs(&old, &new);
        assert_eq!(diff.added_edges.len(), 1);
        assert_eq!(diff.removed_edges.len(), 1);
    }

    #[test]
    fn test_diff_graphs_empty() {
        let g1 = NarrativeGraph::new();
        let g2 = NarrativeGraph::new();
        let diff = diff_graphs(&g1, &g2);
        assert!(diff.is_empty());
    }

    #[test]
    fn test_file_path_serde_default() {
        // Simulate deserializing a graph.json that predates the file_path field
        let json = serde_json::json!({
            "nodes": [
                {
                    "Scene": {
                        "id": "scene-old",
                        "title": "Legacy Scene",
                        "summary": "From before file_path existed.",
                        "characters_present": [],
                        "location": null,
                        "time": null
                    }
                }
            ],
            "edges": []
        });

        let g = NarrativeGraph::deserialize(&json).unwrap();
        assert_eq!(
            g.get_node_field("scene-old", "file_path").unwrap(),
            ""
        );
    }

    #[test]
    fn test_serialize_summary_compact() {
        let mut g = NarrativeGraph::new();

        g.add_node(GraphNode::Character {
            id: "char-1".to_string(),
            name: "Alice".to_string(),
            aliases: vec!["Ali".to_string()],
            description: Some("A very long description that should be excluded from the summary to save tokens.".to_string()),
        });
        g.add_node(GraphNode::Scene {
            id: "scene-1".to_string(),
            title: Some("The Opening".to_string()),
            summary: "A long scene summary that should not appear in the compact output.".to_string(),
            characters_present: vec!["char-1".to_string()],
            location: Some("New York".to_string()),
            time: Some("Morning".to_string()),
            file_path: "chapter1.md".to_string(),
        });
        g.add_node(GraphNode::Objective {
            id: "obj-1".to_string(),
            character_id: "char-1".to_string(),
            scope: Scope::Overarching,
            description: "Survive".to_string(),
            evidence: vec!["lots of evidence text".to_string()],
            confidence: 0.9,
            status: Status::Active,
        });
        g.add_node(GraphNode::Conflict {
            id: "conf-1".to_string(),
            description: "Internal struggle".to_string(),
            objectives: vec!["obj-1".to_string()],
        });
        g.add_edge("char-1", "obj-1", GraphEdge::Pursues { scene_id: None });

        let summary = g.serialize_summary();
        let compact = g.serialize_compact();

        // Summary should be significantly smaller
        assert!(
            summary.len() < compact.len(),
            "summary ({}) should be smaller than compact ({})",
            summary.len(),
            compact.len()
        );

        // Summary should contain names/IDs but not descriptions/evidence
        let parsed: serde_json::Value = serde_json::from_str(&summary).unwrap();
        assert_eq!(parsed["characters"][0]["name"], "Alice");
        assert_eq!(parsed["characters"][0]["id"], "char-1");
        assert!(parsed["characters"][0].get("description").is_none());
        assert!(parsed["characters"][0].get("aliases").is_none());

        assert_eq!(parsed["scenes"][0]["title"], "The Opening");
        assert!(parsed["scenes"][0].get("summary").is_none());
        assert!(parsed["scenes"][0].get("location").is_none());

        assert_eq!(parsed["objectives"][0]["label"], "Survive");
        assert_eq!(parsed["objectives"][0]["status"], "Active");
        assert!(parsed["objectives"][0].get("evidence").is_none());
        assert!(parsed["objectives"][0].get("confidence").is_none());

        assert_eq!(parsed["conflicts"][0]["label"], "Internal struggle");
        assert_eq!(parsed["edge_count"], 1);
        assert!(parsed["note"].as_str().unwrap().contains("summary"));
    }

    #[test]
    fn test_serialize_summary_empty_graph() {
        let g = NarrativeGraph::new();
        let summary = g.serialize_summary();
        let parsed: serde_json::Value = serde_json::from_str(&summary).unwrap();
        assert_eq!(parsed["characters"].as_array().unwrap().len(), 0);
        assert_eq!(parsed["scenes"].as_array().unwrap().len(), 0);
        assert_eq!(parsed["edge_count"], 0);
    }

    #[test]
    fn test_serialize_subgraph_includes_neighbors() {
        let mut g = NarrativeGraph::new();

        g.add_node(GraphNode::Character {
            id: "alice".to_string(),
            name: "Alice".to_string(),
            aliases: vec![],
            description: Some("Protagonist".to_string()),
        });
        g.add_node(GraphNode::Character {
            id: "bob".to_string(),
            name: "Bob".to_string(),
            aliases: vec![],
            description: None,
        });
        g.add_node(GraphNode::Character {
            id: "charlie".to_string(),
            name: "Charlie".to_string(),
            aliases: vec![],
            description: None,
        });
        g.add_node(GraphNode::Scene {
            id: "scene-1".to_string(),
            title: Some("Meeting".to_string()),
            summary: "Alice meets Bob".to_string(),
            characters_present: vec!["alice".to_string(), "bob".to_string()],
            location: None,
            time: None,
            file_path: String::new(),
        });
        g.add_node(GraphNode::Scene {
            id: "scene-2".to_string(),
            title: Some("Solo".to_string()),
            summary: "Charlie alone".to_string(),
            characters_present: vec!["charlie".to_string()],
            location: None,
            time: None,
            file_path: String::new(),
        });

        // Alice -> scene-1, Bob -> scene-1, Charlie -> scene-2
        g.add_edge("alice", "scene-1", GraphEdge::PresentIn);
        g.add_edge("bob", "scene-1", GraphEdge::PresentIn);
        g.add_edge("charlie", "scene-2", GraphEdge::PresentIn);

        // Query just Alice — should include Alice + scene-1 (neighbor) + Bob (neighbor of scene-1? no, only 1-hop from alice)
        let subgraph = g.serialize_subgraph(&["alice"]);
        let parsed: serde_json::Value = serde_json::from_str(&subgraph).unwrap();
        let nodes = parsed["nodes"].as_array().unwrap();

        // Alice (seed) + scene-1 (1-hop neighbor via PresentIn)
        let node_ids: Vec<&str> = nodes
            .iter()
            .map(|n| {
                n.get("Character")
                    .and_then(|c| c["id"].as_str())
                    .or_else(|| n.get("Scene").and_then(|s| s["id"].as_str()))
                    .unwrap()
            })
            .collect();
        assert!(node_ids.contains(&"alice"));
        assert!(node_ids.contains(&"scene-1"));
        // Charlie and scene-2 should NOT be included
        assert!(!node_ids.contains(&"charlie"));
        assert!(!node_ids.contains(&"scene-2"));

        // Edges between included nodes should be present
        let edges = parsed["edges"].as_array().unwrap();
        assert!(!edges.is_empty());
    }

    #[test]
    fn test_serialize_subgraph_empty_ids() {
        let mut g = NarrativeGraph::new();
        g.add_node(GraphNode::Character {
            id: "c1".to_string(),
            name: "Test".to_string(),
            aliases: vec![],
            description: None,
        });

        let subgraph = g.serialize_subgraph(&[]);
        let parsed: serde_json::Value = serde_json::from_str(&subgraph).unwrap();
        assert_eq!(parsed["nodes"].as_array().unwrap().len(), 0);
        assert_eq!(parsed["edges"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_serialize_subgraph_nonexistent_id() {
        let g = NarrativeGraph::new();
        let subgraph = g.serialize_subgraph(&["nonexistent"]);
        let parsed: serde_json::Value = serde_json::from_str(&subgraph).unwrap();
        assert_eq!(parsed["nodes"].as_array().unwrap().len(), 0);
    }
}
