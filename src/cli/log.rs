use crate::concepts::narrative_graph::{diff_graphs, NarrativeGraph};
use crate::config::{self, GRAPH_FILE, LAIRES_DIR};

use super::diff::{detect_vcs, format_diff_summary, format_graph_diff, vcs_log_for_file, vcs_root, vcs_show};

pub fn run(count: usize, verbose: bool) -> anyhow::Result<()> {
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

    // Get commits that touched graph.json
    let commits = vcs_log_for_file(vcs, &vcs_root, &graph_rel)?;

    if commits.is_empty() {
        println!("No commits found that modified {graph_rel}.");
        return Ok(());
    }

    let display_count = commits.len().min(count);
    println!(
        "Narrative graph history ({display_count} of {} commits):\n",
        commits.len()
    );

    // For each adjacent pair of commits, compute graph diff
    for i in 0..display_count {
        let commit = &commits[i];
        let short_id = &commit.id[..commit.id.len().min(12)];

        // Load graph at this commit
        let current_graph = match vcs_show(vcs, &vcs_root, &graph_rel, &commit.id) {
            Ok(content) => {
                let data: serde_json::Value = serde_json::from_str(&content)?;
                NarrativeGraph::deserialize(&data)?
            }
            Err(_) => continue,
        };

        // Load graph at the parent commit (next in list, since log is newest-first)
        let parent_graph = if i + 1 < commits.len() {
            match vcs_show(vcs, &vcs_root, &graph_rel, &commits[i + 1].id) {
                Ok(content) => {
                    let data: serde_json::Value = serde_json::from_str(&content)?;
                    NarrativeGraph::deserialize(&data)?
                }
                Err(_) => NarrativeGraph::new(),
            }
        } else {
            // First commit that introduced graph.json
            NarrativeGraph::new()
        };

        let diff = diff_graphs(&parent_graph, &current_graph);
        let summary = format_diff_summary(&diff);

        println!("{short_id}  {}", commit.date);
        println!("  {}", commit.summary);
        println!("  Graph: {summary}");

        if verbose && !diff.is_empty() {
            print!("{}", format_graph_diff(&diff));
        }

        println!();
    }

    Ok(())
}
