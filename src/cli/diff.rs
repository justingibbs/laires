use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::concepts::narrative_graph::{diff_graphs, GraphDiff, GraphNode, NarrativeGraph};
use crate::config::{self, GRAPH_FILE, LAIRES_DIR};

#[derive(Debug, Clone, Copy)]
pub enum Vcs {
    Git,
    Jj,
}

impl std::fmt::Display for Vcs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Vcs::Git => write!(f, "git"),
            Vcs::Jj => write!(f, "jj"),
        }
    }
}

/// Detect which VCS is in use by checking for .git/ or .jj/ directories
pub fn detect_vcs(project_root: &Path) -> Option<Vcs> {
    // Walk up from project_root to find VCS root
    let mut dir = project_root.to_path_buf();
    loop {
        if dir.join(".git").exists() {
            return Some(Vcs::Git);
        }
        if dir.join(".jj").exists() {
            return Some(Vcs::Jj);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// Get the VCS repository root
pub fn vcs_root(vcs: Vcs, project_root: &Path) -> anyhow::Result<PathBuf> {
    match vcs {
        Vcs::Git => {
            let output = Command::new("git")
                .args(["rev-parse", "--show-toplevel"])
                .current_dir(project_root)
                .output()?;
            if !output.status.success() {
                anyhow::bail!("Failed to find git root");
            }
            let root = String::from_utf8(output.stdout)?.trim().to_string();
            Ok(PathBuf::from(root))
        }
        Vcs::Jj => {
            let output = Command::new("jj")
                .args(["workspace", "root"])
                .current_dir(project_root)
                .output()?;
            if !output.status.success() {
                anyhow::bail!("Failed to find jj workspace root");
            }
            let root = String::from_utf8(output.stdout)?.trim().to_string();
            Ok(PathBuf::from(root))
        }
    }
}

/// Get a file's contents from a specific VCS revision
pub fn vcs_show(vcs: Vcs, project_root: &Path, file_path: &str, revision: &str) -> anyhow::Result<String> {
    match vcs {
        Vcs::Git => {
            let spec = format!("{revision}:{file_path}");
            let output = Command::new("git")
                .args(["show", &spec])
                .current_dir(project_root)
                .output()?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("git show failed: {stderr}");
            }
            Ok(String::from_utf8(output.stdout)?)
        }
        Vcs::Jj => {
            let output = Command::new("jj")
                .args(["file", "show", file_path, "-r", revision])
                .current_dir(project_root)
                .output()?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("jj file show failed: {stderr}");
            }
            Ok(String::from_utf8(output.stdout)?)
        }
    }
}

/// Get list of commits that touched a specific file
pub fn vcs_log_for_file(vcs: Vcs, project_root: &Path, file_path: &str) -> anyhow::Result<Vec<CommitInfo>> {
    match vcs {
        Vcs::Git => {
            // Format: hash<TAB>subject<TAB>date
            let output = Command::new("git")
                .args([
                    "log",
                    "--pretty=format:%H\t%s\t%ai",
                    "--follow",
                    "--",
                    file_path,
                ])
                .current_dir(project_root)
                .output()?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("git log failed: {stderr}");
            }
            let stdout = String::from_utf8(output.stdout)?;
            Ok(stdout
                .lines()
                .filter(|l| !l.is_empty())
                .filter_map(|line| {
                    let parts: Vec<&str> = line.splitn(3, '\t').collect();
                    if parts.len() >= 3 {
                        Some(CommitInfo {
                            id: parts[0].to_string(),
                            summary: parts[1].to_string(),
                            date: parts[2].to_string(),
                        })
                    } else {
                        None
                    }
                })
                .collect())
        }
        Vcs::Jj => {
            // jj log with template
            let output = Command::new("jj")
                .args([
                    "log",
                    "--no-graph",
                    "-T",
                    r#"change_id ++ "\t" ++ description.first_line() ++ "\t" ++ committer.timestamp() ++ "\n""#,
                    "-r",
                    "ancestors(@)",
                    "-s",
                ])
                .current_dir(project_root)
                .output()?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("jj log failed: {stderr}");
            }
            let stdout = String::from_utf8(output.stdout)?;
            // Filter to only commits that touched the file
            let mut commits = Vec::new();
            let mut current_commit: Option<CommitInfo> = None;
            for line in stdout.lines() {
                if line.contains('\t') && !line.starts_with(' ') {
                    // Commit line
                    if let Some(c) = current_commit.take() {
                        commits.push(c);
                    }
                    let parts: Vec<&str> = line.splitn(3, '\t').collect();
                    if parts.len() >= 3 {
                        current_commit = Some(CommitInfo {
                            id: parts[0].to_string(),
                            summary: parts[1].to_string(),
                            date: parts[2].to_string(),
                        });
                    }
                } else if line.contains(file_path) {
                    // This commit touched our file, keep it
                    // (current_commit stays set)
                } else if !line.starts_with(' ') {
                    // New commit line that doesn't match, clear current
                    current_commit = None;
                }
            }
            if let Some(c) = current_commit {
                commits.push(c);
            }
            Ok(commits)
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub id: String,
    pub summary: String,
    pub date: String,
}

/// Compute per-scene word count deltas between old and new graphs
fn scene_word_count_deltas(old: &NarrativeGraph, new: &NarrativeGraph) -> Vec<String> {
    let old_scenes: HashMap<&str, &GraphNode> = old
        .get_scenes()
        .into_iter()
        .map(|n| (n.node_id(), n))
        .collect();
    let new_scenes: HashMap<&str, &GraphNode> = new
        .get_scenes()
        .into_iter()
        .map(|n| (n.node_id(), n))
        .collect();

    let mut lines = Vec::new();

    // Check scenes in new graph
    for (&id, &new_node) in &new_scenes {
        if let GraphNode::Scene {
            title: new_title,
            summary: new_summary,
            ..
        } = new_node
        {
            let new_words = new_summary.split_whitespace().count();
            let label = new_title.as_deref().unwrap_or("(untitled)");

            match old_scenes.get(id) {
                Some(&GraphNode::Scene {
                    summary: old_summary,
                    ..
                }) => {
                    let old_words = old_summary.split_whitespace().count();
                    if old_words != new_words {
                        let delta = new_words as isize - old_words as isize;
                        let sign = if delta > 0 { "+" } else { "" };
                        lines.push(format!(
                            "  Scene \"{label}\" (id: {id:.8}): {sign}{delta} words ({old_words} → {new_words})"
                        ));
                    }
                }
                _ => {
                    lines.push(format!(
                        "  Scene \"{label}\" (id: {id:.8}): +{new_words} words (new)"
                    ));
                }
            }
        }
    }

    // Check removed scenes
    for (&id, &old_node) in &old_scenes {
        if !new_scenes.contains_key(id) {
            if let GraphNode::Scene {
                title, summary, ..
            } = old_node
            {
                let old_words = summary.split_whitespace().count();
                let label = title.as_deref().unwrap_or("(untitled)");
                lines.push(format!(
                    "  Scene \"{label}\" (id: {id:.8}): -{old_words} words (removed)"
                ));
            }
        }
    }

    lines.sort();
    lines
}

/// Format a GraphDiff for display
pub fn format_graph_diff(diff: &GraphDiff) -> String {
    let mut out = String::new();

    if !diff.added_nodes.is_empty() {
        out.push_str("  Added:\n");
        for node in &diff.added_nodes {
            out.push_str(&format!("    + {} {}: {}\n", node.node_type_name(), node.node_id(), node_label(node)));
        }
    }

    if !diff.removed_nodes.is_empty() {
        out.push_str("  Removed:\n");
        for node in &diff.removed_nodes {
            out.push_str(&format!("    - {} {}: {}\n", node.node_type_name(), node.node_id(), node_label(node)));
        }
    }

    if !diff.changed_nodes.is_empty() {
        out.push_str("  Changed:\n");
        for change in &diff.changed_nodes {
            out.push_str(&format!(
                "    ~ {} {} ({}): {}\n",
                change.new.node_type_name(),
                &change.id[..change.id.len().min(8)],
                node_label(&change.new),
                change.fields.join(", ")
            ));
        }
    }

    if !diff.added_edges.is_empty() {
        out.push_str(&format!("  Edges added: {}\n", diff.added_edges.len()));
        for edge in &diff.added_edges {
            out.push_str(&format!(
                "    + {} → {} [{}]\n",
                &edge.from[..edge.from.len().min(8)],
                &edge.to[..edge.to.len().min(8)],
                edge.edge.edge_type_name()
            ));
        }
    }

    if !diff.removed_edges.is_empty() {
        out.push_str(&format!("  Edges removed: {}\n", diff.removed_edges.len()));
        for edge in &diff.removed_edges {
            out.push_str(&format!(
                "    - {} → {} [{}]\n",
                &edge.from[..edge.from.len().min(8)],
                &edge.to[..edge.to.len().min(8)],
                edge.edge.edge_type_name()
            ));
        }
    }

    out
}

fn node_label(node: &GraphNode) -> String {
    match node {
        GraphNode::Character { name, .. } => name.clone(),
        GraphNode::Objective {
            description, status, ..
        } => format!("{description} [{status:?}]"),
        GraphNode::Scene { title, .. } => {
            title.as_deref().unwrap_or("(untitled)").to_string()
        }
        GraphNode::Conflict { description, .. } => description.clone(),
    }
}

/// Format a compact one-line summary of a GraphDiff
pub fn format_diff_summary(diff: &GraphDiff) -> String {
    let mut parts = Vec::new();
    if !diff.added_nodes.is_empty() {
        parts.push(format!("+{} nodes", diff.added_nodes.len()));
    }
    if !diff.removed_nodes.is_empty() {
        parts.push(format!("-{} nodes", diff.removed_nodes.len()));
    }
    if !diff.changed_nodes.is_empty() {
        parts.push(format!("~{} nodes", diff.changed_nodes.len()));
    }
    if !diff.added_edges.is_empty() {
        parts.push(format!("+{} edges", diff.added_edges.len()));
    }
    if !diff.removed_edges.is_empty() {
        parts.push(format!("-{} edges", diff.removed_edges.len()));
    }
    if parts.is_empty() {
        "no changes".to_string()
    } else {
        parts.join(", ")
    }
}

pub fn run() -> anyhow::Result<()> {
    let project_dir = std::env::current_dir()?;
    let project_root = config::find_project_root(&project_dir)
        .ok_or_else(|| anyhow::anyhow!("Not in a Laires project. Run `laires init` first."))?;

    let vcs = detect_vcs(&project_root).ok_or_else(|| {
        anyhow::anyhow!("No version control detected. Initialize git or jj in your project first.")
    })?;

    let vcs_root = vcs_root(vcs, &project_root)?;

    // Compute the relative path from VCS root to graph.json
    let graph_abs = project_root.join(LAIRES_DIR).join(GRAPH_FILE);
    let graph_rel = graph_abs
        .strip_prefix(&vcs_root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| format!("{LAIRES_DIR}/{GRAPH_FILE}"));

    // Load current graph
    let current_graph = if graph_abs.exists() {
        NarrativeGraph::load(&graph_abs)?
    } else {
        println!("No graph.json found. Run `laires scan` first.");
        return Ok(());
    };

    // Load committed graph
    let revision = match vcs {
        Vcs::Git => "HEAD",
        Vcs::Jj => "@-",
    };

    let old_graph = match vcs_show(vcs, &vcs_root, &graph_rel, revision) {
        Ok(content) => {
            let data: serde_json::Value = serde_json::from_str(&content)?;
            NarrativeGraph::deserialize(&data)?
        }
        Err(_) => {
            println!("No committed graph.json found (this may be a new project).");
            println!("All current graph nodes will appear as additions.\n");
            NarrativeGraph::new()
        }
    };

    // Compute diff
    let diff = diff_graphs(&old_graph, &current_graph);

    if diff.is_empty() {
        println!("No changes to narrative graph since last commit.");
        return Ok(());
    }

    // Text changes (per-scene word count deltas)
    let text_deltas = scene_word_count_deltas(&old_graph, &current_graph);
    if !text_deltas.is_empty() {
        println!("Text changes:");
        for line in &text_deltas {
            println!("{line}");
        }
        println!();
    }

    // Graph changes
    println!("Graph changes:");
    print!("{}", format_graph_diff(&diff));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_detect_vcs_none() {
        // A temp directory with no VCS
        let tmp = std::env::temp_dir().join("laires-test-no-vcs");
        std::fs::create_dir_all(&tmp).ok();
        assert!(detect_vcs(&tmp).is_none());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_detect_vcs_git() {
        let tmp = std::env::temp_dir().join("laires-test-git-detect");
        std::fs::create_dir_all(tmp.join(".git")).ok();
        assert!(matches!(detect_vcs(&tmp), Some(Vcs::Git)));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_scene_word_count_deltas() {
        let mut old = NarrativeGraph::new();
        old.add_node(GraphNode::Scene {
            id: "s1".to_string(),
            title: Some("Opening".to_string()),
            summary: "The hero arrives at the village.".to_string(),
            characters_present: vec![],
            location: None,
            time: None,
            file_path: String::new(),
        });

        let mut new = NarrativeGraph::new();
        new.add_node(GraphNode::Scene {
            id: "s1".to_string(),
            title: Some("Opening".to_string()),
            summary: "The hero arrives at the village and meets the elder who tells a story.".to_string(),
            characters_present: vec![],
            location: None,
            time: None,
            file_path: String::new(),
        });

        let deltas = scene_word_count_deltas(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert!(deltas[0].contains("Opening"));
        assert!(deltas[0].contains("+")); // words were added
    }

    #[test]
    fn test_format_diff_summary_empty() {
        let diff = GraphDiff {
            added_nodes: vec![],
            removed_nodes: vec![],
            changed_nodes: vec![],
            added_edges: vec![],
            removed_edges: vec![],
        };
        assert_eq!(format_diff_summary(&diff), "no changes");
    }

    #[test]
    fn test_format_diff_summary_with_changes() {
        let diff = GraphDiff {
            added_nodes: vec![GraphNode::Character {
                id: "c1".to_string(),
                name: "A".to_string(),
                aliases: vec![],
                description: None,
            }],
            removed_nodes: vec![],
            changed_nodes: vec![],
            added_edges: vec![],
            removed_edges: vec![],
        };
        assert_eq!(format_diff_summary(&diff), "+1 nodes");
    }
}
