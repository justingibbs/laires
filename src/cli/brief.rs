use std::path::Path;

use crate::config::{self, BRIEFS_DIR, LAIRES_DIR};

pub fn run(list: bool) -> anyhow::Result<()> {
    let project_dir = std::env::current_dir()?;
    let project_root = config::find_project_root(&project_dir)
        .ok_or_else(|| anyhow::anyhow!("Not in a Laires project. Run `laires init` first."))?;

    let briefs_dir = project_root.join(LAIRES_DIR).join(BRIEFS_DIR);

    if list {
        return list_briefs(&briefs_dir);
    }

    // Show the most recent brief
    show_latest_brief(&briefs_dir)
}

fn list_briefs(briefs_dir: &Path) -> anyhow::Result<()> {
    if !briefs_dir.exists() {
        println!("  No briefs yet. Use Consultant mode to generate revision briefs.");
        return Ok(());
    }

    let mut entries: Vec<_> = std::fs::read_dir(briefs_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
        .collect();

    if entries.is_empty() {
        println!("  No briefs yet. Use Consultant mode to generate revision briefs.");
        return Ok(());
    }

    // Sort by modification time (most recent first)
    entries.sort_by(|a, b| {
        let a_time = a.metadata().and_then(|m| m.modified()).ok();
        let b_time = b.metadata().and_then(|m| m.modified()).ok();
        b_time.cmp(&a_time)
    });

    println!("\n  Revision Briefs:\n");
    for entry in &entries {
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let size_label = if size > 1024 {
            format!("{:.1}KB", size as f64 / 1024.0)
        } else {
            format!("{}B", size)
        };
        println!("    {} ({})", name, size_label);
    }
    println!("\n  {} brief(s) in .laires/briefs/\n", entries.len());

    Ok(())
}

fn show_latest_brief(briefs_dir: &Path) -> anyhow::Result<()> {
    if !briefs_dir.exists() {
        println!("  No briefs yet. Use Consultant mode to generate revision briefs.");
        return Ok(());
    }

    let mut entries: Vec<_> = std::fs::read_dir(briefs_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
        .collect();

    if entries.is_empty() {
        println!("  No briefs yet. Use Consultant mode to generate revision briefs.");
        return Ok(());
    }

    // Most recent first
    entries.sort_by(|a, b| {
        let a_time = a.metadata().and_then(|m| m.modified()).ok();
        let b_time = b.metadata().and_then(|m| m.modified()).ok();
        b_time.cmp(&a_time)
    });

    let latest = &entries[0];
    let content = std::fs::read_to_string(latest.path())?;
    println!("{}", content);

    Ok(())
}
