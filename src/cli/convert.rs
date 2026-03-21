use std::path::Path;

use crate::config::{self, LAIRES_DIR, MANIFEST_FILE};
use crate::concepts::manifest::{Manifest, StoryFile};

pub fn run(file: &str) -> anyhow::Result<()> {
    let project_dir = std::env::current_dir()?;
    let project_root = config::find_project_root(&project_dir)
        .ok_or_else(|| anyhow::anyhow!("Not in a Laires project. Run `laires init` first."))?;

    let source_path = project_root.join(file);
    if !source_path.exists() {
        anyhow::bail!("File not found: {}", source_path.display());
    }

    let ext = source_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let text = match ext {
        "docx" => {
            println!("  Extracting text from {}...", file);
            crate::concepts::docx::extract_text_from_docx(&source_path)
                .map_err(|e| anyhow::anyhow!("{e}"))?
        }
        "txt" => {
            println!("  Reading {}...", file);
            std::fs::read_to_string(&source_path)?
        }
        "md" => {
            anyhow::bail!("{} is already a Markdown file.", file);
        }
        "fountain" => {
            anyhow::bail!("{} is already a Fountain file (editable in Workshop mode).", file);
        }
        _ => {
            anyhow::bail!("Unsupported format: .{ext}. Supported: .docx, .txt");
        }
    };

    // Generate output path: same name with .md extension
    let md_filename = source_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
        + ".md";
    let md_path = source_path.parent().unwrap_or(&project_root).join(&md_filename);

    if md_path.exists() {
        anyhow::bail!(
            "Output file already exists: {}. Remove it first or rename it.",
            md_path.display()
        );
    }

    std::fs::write(&md_path, &text)?;
    println!("  Converted to: {}", md_filename);

    // Update manifest if it exists
    let manifest_path = project_root.join(LAIRES_DIR).join(MANIFEST_FILE);
    if manifest_path.exists() {
        update_manifest(&project_root, file, &md_filename, &text)?;
        println!("  Manifest updated: {} marked as editable.", md_filename);
    }

    println!(
        "\n  Original file preserved: {}\n  New editable file: {}\n",
        file, md_filename
    );

    Ok(())
}

fn update_manifest(
    project_root: &Path,
    original_path: &str,
    new_path: &str,
    text: &str,
) -> anyhow::Result<()> {
    let mut manifest = Manifest::load(project_root)?;

    let content_hash = blake3::hash(text.as_bytes()).to_hex().to_string();

    // Find the original file's order
    let order = manifest
        .story_files
        .iter()
        .find(|sf| sf.path == original_path)
        .map(|sf| sf.order)
        .unwrap_or(manifest.story_files.len());

    // Add the new .md file
    let new_file = StoryFile {
        path: new_path.to_string(),
        format: "prose".to_string(),
        order,
        content_hash,
        editable: true,
    };

    // Mark original as not a story file (move to excluded) or just add the new one
    // Strategy: add the new file, keep original. User decides which to use.
    // Check if new path already in manifest
    if !manifest.story_files.iter().any(|sf| sf.path == new_path) {
        manifest.story_files.push(new_file);
        // Re-sort by order
        manifest.story_files.sort_by_key(|sf| sf.order);
    }

    manifest.save(project_root)?;
    Ok(())
}
