use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::concepts::analysis::extract_json;
use crate::config::{LAIRES_DIR, MANIFEST_FILE};

// ── Types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FileRole {
    Story,
    Outline,
    Characters,
    Notes,
}

impl std::fmt::Display for FileRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileRole::Story => write!(f, "story"),
            FileRole::Outline => write!(f, "outline"),
            FileRole::Characters => write!(f, "characters"),
            FileRole::Notes => write!(f, "notes"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryFile {
    pub path: String,
    pub format: String, // "prose" or "fountain"
    pub order: usize,
    pub content_hash: String,
    /// Whether the agent can write to this file (Workshop mode).
    /// Defaults to true for `.md` and `.fountain`, false for `.docx` and `.txt`.
    #[serde(default = "default_editable")]
    pub editable: bool,
}

fn default_editable() -> bool {
    true
}

impl StoryFile {
    /// Infer editability from file extension.
    pub fn infer_editable(path: &str) -> bool {
        let lower = path.to_lowercase();
        lower.ends_with(".md") || lower.ends_with(".fountain")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextFile {
    pub path: String,
    pub role: FileRole,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExcludedFile {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestMeta {
    pub last_scan: String,
    pub classification_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub meta: ManifestMeta,
    pub story_files: Vec<StoryFile>,
    pub context_files: Vec<ContextFile>,
    pub excluded: Vec<ExcludedFile>,
}

// ── Persistence ────────────────────────────────────────────────────

impl Manifest {
    pub fn load(project_dir: &Path) -> anyhow::Result<Self> {
        let path = project_dir.join(LAIRES_DIR).join(MANIFEST_FILE);
        let content = std::fs::read_to_string(&path)?;
        let manifest: Manifest = toml::from_str(&content)?;
        Ok(manifest)
    }

    pub fn save(&self, project_dir: &Path) -> anyhow::Result<()> {
        let path = project_dir.join(LAIRES_DIR).join(MANIFEST_FILE);
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn exists(project_dir: &Path) -> bool {
        project_dir.join(LAIRES_DIR).join(MANIFEST_FILE).exists()
    }
}

// ── File Discovery ─────────────────────────────────────────────────

const SUPPORTED_EXTENSIONS: &[&str] = &["md", "fountain", "txt", "docx"];
const SKIP_DIRS: &[&str] = &[".laires", ".git", ".jj"];
const SNIPPET_WORD_LIMIT: usize = 500;

#[derive(Debug, Clone)]
pub struct DiscoveredFile {
    pub path: String, // relative to project root
    pub extension: String,
    pub content_hash: String,
    pub word_count: usize,
    pub snippet: String, // first ~500 words
}

pub fn discover_files(project_dir: &Path) -> anyhow::Result<Vec<DiscoveredFile>> {
    let mut files = Vec::new();
    walk_dir(project_dir, project_dir, &mut files)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn walk_dir(root: &Path, dir: &Path, out: &mut Vec<DiscoveredFile>) -> anyhow::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        // Skip hidden dirs and known non-content dirs
        if path.is_dir() {
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            walk_dir(root, &path, out)?;
            continue;
        }

        // Check extension
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !SUPPORTED_EXTENSIONS.contains(&ext) {
            continue;
        }

        let (content_hash, word_count, snippet) = if ext == "docx" {
            match crate::concepts::docx::extract_text_from_docx(&path) {
                Ok(text) => {
                    let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
                    let words: Vec<&str> = text.split_whitespace().collect();
                    let wc = words.len();
                    let snippet_words = &words[..wc.min(SNIPPET_WORD_LIMIT)];
                    let snippet = snippet_words.join(" ");
                    (hash, wc, snippet)
                }
                Err(e) => {
                    eprintln!("Warning: could not read {}: {e}", path.display());
                    let hash = blake3::hash(b"").to_hex().to_string();
                    (hash, 0, "(binary .docx -- extraction failed)".to_string())
                }
            }
        } else {
            let content = std::fs::read_to_string(&path)?;
            let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
            let words: Vec<&str> = content.split_whitespace().collect();
            let word_count = words.len();
            let snippet_words = &words[..word_count.min(SNIPPET_WORD_LIMIT)];
            let snippet = snippet_words.join(" ");
            (hash, word_count, snippet)
        };

        let rel_path = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        out.push(DiscoveredFile {
            path: rel_path,
            extension: ext.to_string(),
            content_hash,
            word_count,
            snippet,
        });
    }

    Ok(())
}

// ── Diffing ────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct ManifestDiff {
    pub new_files: Vec<DiscoveredFile>,
    pub changed_files: Vec<DiscoveredFile>,
    pub removed_files: Vec<String>,
    #[allow(dead_code)]
    pub unchanged_files: Vec<String>,
}

pub fn diff_against_manifest(discovered: &[DiscoveredFile], existing: &Manifest) -> ManifestDiff {
    use std::collections::HashMap;

    // Build hash map from existing manifest (path -> content_hash)
    let mut existing_hashes: HashMap<&str, &str> = HashMap::new();
    for sf in &existing.story_files {
        existing_hashes.insert(&sf.path, &sf.content_hash);
    }
    for cf in &existing.context_files {
        existing_hashes.insert(&cf.path, &cf.content_hash);
    }
    for ef in &existing.excluded {
        // Excluded files don't have hashes, use empty string as sentinel
        existing_hashes.insert(&ef.path, "");
    }

    let mut new_files = Vec::new();
    let mut changed_files = Vec::new();
    let mut unchanged_files = Vec::new();

    let mut seen_paths: std::collections::HashSet<String> = std::collections::HashSet::new();

    for df in discovered {
        seen_paths.insert(df.path.clone());
        match existing_hashes.get(df.path.as_str()) {
            None => new_files.push(df.clone()),
            Some(&hash) => {
                if !hash.is_empty() && hash != df.content_hash {
                    changed_files.push(df.clone());
                } else {
                    unchanged_files.push(df.path.clone());
                }
            }
        }
    }

    // Files in manifest but not on disk
    let removed_files: Vec<String> = existing_hashes
        .keys()
        .filter(|p| !seen_paths.contains(**p))
        .map(|p| p.to_string())
        .collect();

    ManifestDiff {
        new_files,
        changed_files,
        removed_files,
        unchanged_files,
    }
}

// ── LLM Classification ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ClassificationResult {
    pub story_files: Vec<ClassifiedStoryFile>,
    pub context_files: Vec<ClassifiedContextFile>,
    pub excluded: Vec<ClassifiedExcluded>,
}

#[derive(Debug, Clone)]
pub struct ClassifiedStoryFile {
    pub path: String,
    pub format: String,
    pub order: usize,
}

#[derive(Debug, Clone)]
pub struct ClassifiedContextFile {
    pub path: String,
    pub role: FileRole,
}

#[derive(Debug, Clone)]
pub struct ClassifiedExcluded {
    pub path: String,
    pub reason: String,
}

pub fn build_classification_prompt(files: &[DiscoveredFile]) -> String {
    let mut prompt = String::from(
        "You are classifying files in a fiction writing project. \
         For each file, determine its role and respond with JSON.\n\n\
         Files found:\n\n",
    );

    for (i, f) in files.iter().enumerate() {
        prompt.push_str(&format!(
            "{}. **{}** (ext: .{}, {} words)\n   Snippet: {}\n\n",
            i + 1,
            f.path,
            f.extension,
            f.word_count,
            truncate_snippet(&f.snippet, 200),
        ));
    }

    prompt.push_str(
        "Classify each file into one of these categories:\n\
         - **story**: Main narrative content (chapters, scenes). Specify format: \"prose\" or \"fountain\".\n\
         - **outline**: Plot outlines, beat sheets, structure notes.\n\
         - **characters**: Character sheets, bios, descriptions.\n\
         - **notes**: Research, worldbuilding, reference material.\n\
         - **excluded**: Not relevant (e.g., old versions, duplicates, non-project files). Give a reason.\n\n\
         For story files, also determine the reading order (1, 2, 3...).\n\n\
         Respond with JSON only:\n\
         ```json\n\
         {\n\
           \"story_files\": [\n\
             { \"path\": \"chapter-1.md\", \"format\": \"prose\", \"order\": 1 }\n\
           ],\n\
           \"context_files\": [\n\
             { \"path\": \"characters.md\", \"role\": \"characters\" }\n\
           ],\n\
           \"excluded\": [\n\
             { \"path\": \"old-draft.md\", \"reason\": \"Older version of chapter 1\" }\n\
           ]\n\
         }\n\
         ```",
    );

    prompt
}

fn truncate_snippet(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}...")
    }
}

pub fn parse_classification_response(response: &str) -> anyhow::Result<ClassificationResult> {
    let parsed = extract_json(response)
        .ok_or_else(|| anyhow::anyhow!("Failed to extract JSON from classification response"))?;

    let story_files = parsed["story_files"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|sf| {
                    Some(ClassifiedStoryFile {
                        path: sf["path"].as_str()?.to_string(),
                        format: sf["format"].as_str().unwrap_or("prose").to_string(),
                        order: sf["order"].as_u64().unwrap_or(0) as usize,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let context_files = parsed["context_files"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|cf| {
                    let role = match cf["role"].as_str()? {
                        "outline" => FileRole::Outline,
                        "characters" => FileRole::Characters,
                        "notes" => FileRole::Notes,
                        _ => FileRole::Notes,
                    };
                    Some(ClassifiedContextFile {
                        path: cf["path"].as_str()?.to_string(),
                        role,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let excluded = parsed["excluded"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|ef| {
                    Some(ClassifiedExcluded {
                        path: ef["path"].as_str()?.to_string(),
                        reason: ef["reason"]
                            .as_str()
                            .unwrap_or("No reason given")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(ClassificationResult {
        story_files,
        context_files,
        excluded,
    })
}

// ── User Confirmation ──────────────────────────────────────────────

pub fn print_classification(result: &ClassificationResult) {
    println!("\n  File Classification:\n");

    if !result.story_files.is_empty() {
        println!("  Story files (narrative content):");
        for sf in &result.story_files {
            println!("    {}. {} ({})", sf.order, sf.path, sf.format);
        }
    }

    if !result.context_files.is_empty() {
        println!("\n  Context files:");
        for cf in &result.context_files {
            println!("    {} ({})", cf.path, cf.role);
        }
    }

    if !result.excluded.is_empty() {
        println!("\n  Excluded:");
        for ef in &result.excluded {
            println!("    {} — {}", ef.path, ef.reason);
        }
    }

    println!();
}

// ── Build Manifest ─────────────────────────────────────────────────

pub fn build_manifest_from_classification(
    result: ClassificationResult,
    discovered: &[DiscoveredFile],
    model: &str,
) -> Manifest {
    use std::collections::HashMap;

    let hash_map: HashMap<&str, &str> = discovered
        .iter()
        .map(|d| (d.path.as_str(), d.content_hash.as_str()))
        .collect();

    let story_files = result
        .story_files
        .iter()
        .map(|sf| StoryFile {
            path: sf.path.clone(),
            format: sf.format.clone(),
            order: sf.order,
            content_hash: hash_map.get(sf.path.as_str()).unwrap_or(&"").to_string(),
            editable: StoryFile::infer_editable(&sf.path),
        })
        .collect();

    let context_files = result
        .context_files
        .iter()
        .map(|cf| ContextFile {
            path: cf.path.clone(),
            role: cf.role.clone(),
            content_hash: hash_map.get(cf.path.as_str()).unwrap_or(&"").to_string(),
        })
        .collect();

    let excluded = result
        .excluded
        .iter()
        .map(|ef| ExcludedFile {
            path: ef.path.clone(),
            reason: ef.reason.clone(),
        })
        .collect();

    Manifest {
        meta: ManifestMeta {
            last_scan: chrono::Utc::now().to_rfc3339(),
            classification_model: model.to_string(),
        },
        story_files,
        context_files,
        excluded,
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_discover_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        fs::write(root.join("chapter-1.md"), "Once upon a time").unwrap();
        fs::write(root.join("chapter-2.md"), "The hero arrived").unwrap();
        fs::write(root.join("notes.txt"), "Research notes").unwrap();
        fs::write(root.join("script.fountain"), "INT. OFFICE").unwrap();
        // Non-matching extension should be skipped
        fs::write(root.join("image.png"), "binary").unwrap();

        let files = discover_files(root).unwrap();
        assert_eq!(files.len(), 4);

        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"chapter-1.md"));
        assert!(paths.contains(&"chapter-2.md"));
        assert!(paths.contains(&"notes.txt"));
        assert!(paths.contains(&"script.fountain"));
        assert!(!paths.contains(&"image.png"));
    }

    #[test]
    fn test_discover_skips_hidden_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git/config.md"), "git config").unwrap();

        fs::create_dir_all(root.join(".laires")).unwrap();
        fs::write(root.join(".laires/config.md"), "laires config").unwrap();

        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::write(root.join(".hidden/secret.md"), "hidden").unwrap();

        fs::write(root.join("story.md"), "The story").unwrap();

        let files = discover_files(root).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "story.md");
    }

    #[test]
    fn test_manifest_save_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(LAIRES_DIR)).unwrap();

        let manifest = Manifest {
            meta: ManifestMeta {
                last_scan: "2026-01-01T00:00:00Z".to_string(),
                classification_model: "test-model".to_string(),
            },
            story_files: vec![StoryFile {
                path: "chapter-1.md".to_string(),
                format: "prose".to_string(),
                order: 1,
                content_hash: "abc123".to_string(),
                editable: true,
            }],
            context_files: vec![ContextFile {
                path: "characters.md".to_string(),
                role: FileRole::Characters,
                content_hash: "def456".to_string(),
            }],
            excluded: vec![ExcludedFile {
                path: "old-draft.md".to_string(),
                reason: "Older version".to_string(),
            }],
        };

        manifest.save(root).unwrap();
        let loaded = Manifest::load(root).unwrap();

        assert_eq!(loaded.meta.last_scan, "2026-01-01T00:00:00Z");
        assert_eq!(loaded.meta.classification_model, "test-model");
        assert_eq!(loaded.story_files.len(), 1);
        assert_eq!(loaded.story_files[0].path, "chapter-1.md");
        assert_eq!(loaded.story_files[0].format, "prose");
        assert_eq!(loaded.story_files[0].order, 1);
        assert_eq!(loaded.context_files.len(), 1);
        assert_eq!(loaded.context_files[0].role, FileRole::Characters);
        assert_eq!(loaded.excluded.len(), 1);
    }

    #[test]
    fn test_diff_detects_new_files() {
        let existing = Manifest {
            meta: ManifestMeta {
                last_scan: String::new(),
                classification_model: String::new(),
            },
            story_files: vec![StoryFile {
                path: "chapter-1.md".to_string(),
                format: "prose".to_string(),
                order: 1,
                content_hash: "hash1".to_string(),
                editable: true,
            }],
            context_files: vec![],
            excluded: vec![],
        };

        let discovered = vec![
            DiscoveredFile {
                path: "chapter-1.md".to_string(),
                extension: "md".to_string(),
                content_hash: "hash1".to_string(),
                word_count: 100,
                snippet: String::new(),
            },
            DiscoveredFile {
                path: "chapter-2.md".to_string(),
                extension: "md".to_string(),
                content_hash: "hash2".to_string(),
                word_count: 200,
                snippet: String::new(),
            },
        ];

        let diff = diff_against_manifest(&discovered, &existing);
        assert_eq!(diff.new_files.len(), 1);
        assert_eq!(diff.new_files[0].path, "chapter-2.md");
        assert_eq!(diff.unchanged_files.len(), 1);
        assert!(diff.changed_files.is_empty());
        assert!(diff.removed_files.is_empty());
    }

    #[test]
    fn test_diff_detects_changed_files() {
        let existing = Manifest {
            meta: ManifestMeta {
                last_scan: String::new(),
                classification_model: String::new(),
            },
            story_files: vec![StoryFile {
                path: "chapter-1.md".to_string(),
                format: "prose".to_string(),
                order: 1,
                content_hash: "old_hash".to_string(),
                editable: true,
            }],
            context_files: vec![],
            excluded: vec![],
        };

        let discovered = vec![DiscoveredFile {
            path: "chapter-1.md".to_string(),
            extension: "md".to_string(),
            content_hash: "new_hash".to_string(),
            word_count: 150,
            snippet: String::new(),
        }];

        let diff = diff_against_manifest(&discovered, &existing);
        assert_eq!(diff.changed_files.len(), 1);
        assert_eq!(diff.changed_files[0].path, "chapter-1.md");
        assert!(diff.new_files.is_empty());
        assert!(diff.unchanged_files.is_empty());
    }

    #[test]
    fn test_diff_detects_removed_files() {
        let existing = Manifest {
            meta: ManifestMeta {
                last_scan: String::new(),
                classification_model: String::new(),
            },
            story_files: vec![
                StoryFile {
                    path: "chapter-1.md".to_string(),
                    format: "prose".to_string(),
                    order: 1,
                    content_hash: "hash1".to_string(),
                    editable: true,
                },
                StoryFile {
                    path: "chapter-2.md".to_string(),
                    format: "prose".to_string(),
                    order: 2,
                    content_hash: "hash2".to_string(),
                    editable: true,
                },
            ],
            context_files: vec![],
            excluded: vec![],
        };

        let discovered = vec![DiscoveredFile {
            path: "chapter-1.md".to_string(),
            extension: "md".to_string(),
            content_hash: "hash1".to_string(),
            word_count: 100,
            snippet: String::new(),
        }];

        let diff = diff_against_manifest(&discovered, &existing);
        assert_eq!(diff.removed_files.len(), 1);
        assert_eq!(diff.removed_files[0], "chapter-2.md");
    }

    #[test]
    fn test_parse_classification_response() {
        let response = r#"```json
{
  "story_files": [
    { "path": "chapter-1.md", "format": "prose", "order": 1 },
    { "path": "chapter-2.md", "format": "prose", "order": 2 }
  ],
  "context_files": [
    { "path": "characters.md", "role": "characters" },
    { "path": "outline.md", "role": "outline" }
  ],
  "excluded": [
    { "path": "old-draft.md", "reason": "Older version of chapter 1" }
  ]
}
```"#;

        let result = parse_classification_response(response).unwrap();
        assert_eq!(result.story_files.len(), 2);
        assert_eq!(result.story_files[0].path, "chapter-1.md");
        assert_eq!(result.story_files[0].order, 1);
        assert_eq!(result.story_files[1].path, "chapter-2.md");
        assert_eq!(result.story_files[1].format, "prose");

        assert_eq!(result.context_files.len(), 2);
        assert_eq!(result.context_files[0].role, FileRole::Characters);
        assert_eq!(result.context_files[1].role, FileRole::Outline);

        assert_eq!(result.excluded.len(), 1);
        assert_eq!(result.excluded[0].path, "old-draft.md");
    }

    #[test]
    fn test_build_classification_prompt() {
        let files = vec![
            DiscoveredFile {
                path: "chapter-1.md".to_string(),
                extension: "md".to_string(),
                content_hash: "hash1".to_string(),
                word_count: 5000,
                snippet: "Once upon a time in a kingdom far away".to_string(),
            },
            DiscoveredFile {
                path: "characters.md".to_string(),
                extension: "md".to_string(),
                content_hash: "hash2".to_string(),
                word_count: 200,
                snippet: "Character Sheet: Princess Aurora".to_string(),
            },
        ];

        let prompt = build_classification_prompt(&files);
        assert!(prompt.contains("chapter-1.md"));
        assert!(prompt.contains("characters.md"));
        assert!(prompt.contains("5000 words"));
        assert!(prompt.contains("Once upon a time"));
        assert!(prompt.contains("Character Sheet"));
        assert!(prompt.contains("story_files"));
        assert!(prompt.contains("context_files"));
    }

    #[test]
    fn test_content_hash_consistency() {
        let content = "The same content should produce the same hash";
        let hash1 = blake3::hash(content.as_bytes()).to_hex().to_string();
        let hash2 = blake3::hash(content.as_bytes()).to_hex().to_string();
        assert_eq!(hash1, hash2);

        let different = "Different content";
        let hash3 = blake3::hash(different.as_bytes()).to_hex().to_string();
        assert_ne!(hash1, hash3);
    }
}
