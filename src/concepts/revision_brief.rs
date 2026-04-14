use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Write;
use std::path::Path;

use crate::concepts::scene_map::SceneId;

// ── Types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    High,
    Medium,
    Low,
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Priority::High => write!(f, "High"),
            Priority::Medium => write!(f, "Medium"),
            Priority::Low => write!(f, "Low"),
        }
    }
}

impl Priority {
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "high" => Priority::High,
            "low" => Priority::Low,
            _ => Priority::Medium,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Revision {
    pub scene_id: Option<SceneId>,
    pub scene_title: String,
    pub file_path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub priority: Priority,
    pub current_state: String,
    pub issue: String,
    pub suggestion: String,
    pub draft_passage: Option<String>,
    pub graph_impact: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralNote {
    pub category: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevisionBrief {
    pub project_title: String,
    pub created: DateTime<Utc>,
    pub focus: String,
    pub scope: String,
    pub overview: String,
    pub revisions: Vec<Revision>,
    pub structural_notes: Vec<StructuralNote>,
}

// ── Construction ───────────────────────────────────────────────────

impl RevisionBrief {
    pub fn new(project_title: &str) -> Self {
        Self {
            project_title: project_title.to_string(),
            created: Utc::now(),
            focus: String::new(),
            scope: String::new(),
            overview: String::new(),
            revisions: Vec::new(),
            structural_notes: Vec::new(),
        }
    }

    pub fn add_revision(&mut self, revision: Revision) {
        self.revisions.push(revision);
    }

    pub fn add_structural_note(&mut self, category: &str, note: &str) {
        self.structural_notes.push(StructuralNote {
            category: category.to_string(),
            note: note.to_string(),
        });
    }

    pub fn revision_count(&self) -> usize {
        self.revisions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.revisions.is_empty() && self.structural_notes.is_empty()
    }
}

// ── Markdown Rendering ─────────────────────────────────────────────

impl RevisionBrief {
    pub fn to_markdown(&self) -> String {
        let mut md = String::with_capacity(4096);

        // Header
        writeln!(md, "# Revision Brief — {}", self.project_title).unwrap();
        writeln!(
            md,
            "Generated: {}",
            self.created.format("%Y-%m-%d %H:%M UTC")
        )
        .unwrap();
        writeln!(md).unwrap();
        writeln!(md, "---").unwrap();
        writeln!(md).unwrap();

        // Overview
        writeln!(md, "## Overview").unwrap();
        writeln!(md).unwrap();
        if !self.overview.is_empty() {
            writeln!(md, "{}", self.overview).unwrap();
            writeln!(md).unwrap();
        }
        if !self.focus.is_empty() {
            writeln!(md, "**Focus**: {}", self.focus).unwrap();
        }
        if !self.scope.is_empty() {
            writeln!(md, "**Scope**: {}", self.scope).unwrap();
        }
        if !self.focus.is_empty() || !self.scope.is_empty() {
            writeln!(md).unwrap();
        }

        writeln!(md, "---").unwrap();
        writeln!(md).unwrap();

        // Revisions
        if !self.revisions.is_empty() {
            writeln!(md, "## Revisions").unwrap();
            writeln!(md).unwrap();

            for (i, rev) in self.revisions.iter().enumerate() {
                let scene_label = if rev.scene_title.is_empty() {
                    format!("Revision {}", i + 1)
                } else {
                    rev.scene_title.clone()
                };
                writeln!(md, "### {}", scene_label).unwrap();

                if !rev.file_path.is_empty() {
                    write!(md, "**File**: {}", rev.file_path).unwrap();
                    if rev.line_start > 0 || rev.line_end > 0 {
                        write!(md, ", lines {}-{}", rev.line_start, rev.line_end).unwrap();
                    }
                    writeln!(md).unwrap();
                }
                writeln!(md, "**Priority**: {}", rev.priority).unwrap();
                writeln!(md).unwrap();

                if !rev.current_state.is_empty() {
                    writeln!(md, "**Current state**:").unwrap();
                    for line in rev.current_state.lines() {
                        writeln!(md, "> {}", line).unwrap();
                    }
                    writeln!(md).unwrap();
                }

                writeln!(md, "**Issue**:").unwrap();
                writeln!(md, "{}", rev.issue).unwrap();
                writeln!(md).unwrap();

                writeln!(md, "**Suggested revision**:").unwrap();
                writeln!(md, "{}", rev.suggestion).unwrap();
                writeln!(md).unwrap();

                if let Some(ref draft) = rev.draft_passage {
                    writeln!(md, "**Draft passage**:").unwrap();
                    for line in draft.lines() {
                        writeln!(md, "> {}", line).unwrap();
                    }
                    writeln!(md).unwrap();
                }

                if !rev.graph_impact.is_empty() {
                    writeln!(md, "**Graph impact**:").unwrap();
                    for impact in &rev.graph_impact {
                        writeln!(md, "- {}", impact).unwrap();
                    }
                    writeln!(md).unwrap();
                }

                writeln!(md, "---").unwrap();
                writeln!(md).unwrap();
            }
        }

        // Structural Notes
        if !self.structural_notes.is_empty() {
            writeln!(md, "## Structural Notes").unwrap();
            writeln!(md).unwrap();
            for note in &self.structural_notes {
                writeln!(md, "- **{}**: {}", note.category, note.note).unwrap();
            }
            writeln!(md).unwrap();
        }

        // Checklist
        if !self.revisions.is_empty() {
            writeln!(md, "## Checklist").unwrap();
            writeln!(md).unwrap();
            for rev in &self.revisions {
                let label = if rev.scene_title.is_empty() {
                    rev.issue.lines().next().unwrap_or("(revision)").to_string()
                } else {
                    format!("{}: {}", rev.scene_title, first_sentence(&rev.issue))
                };
                writeln!(md, "- [ ] {}", label).unwrap();
            }
            for note in &self.structural_notes {
                writeln!(
                    md,
                    "- [ ] {}: {}",
                    note.category,
                    first_sentence(&note.note)
                )
                .unwrap();
            }
            writeln!(md).unwrap();
        }

        md
    }
}

// ── Persistence ────────────────────────────────────────────────────

impl RevisionBrief {
    /// Save the brief as a Markdown file.
    pub fn save_markdown(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_markdown())?;
        Ok(())
    }

    /// Generate a default filename based on the creation date and focus.
    pub fn default_filename(&self) -> String {
        let date = self.created.format("%Y-%m-%d");
        let slug = if self.focus.is_empty() {
            "revision".to_string()
        } else {
            slugify(&self.focus)
        };
        format!("{date}-{slug}.md")
    }

    /// Save to the project's `.laires/briefs/` directory with an auto-generated name.
    pub fn save_to_project(&self, project_root: &Path) -> anyhow::Result<std::path::PathBuf> {
        let briefs_dir = project_root
            .join(crate::config::LAIRES_DIR)
            .join(crate::config::BRIEFS_DIR);
        let filename = self.default_filename();
        let path = briefs_dir.join(&filename);
        self.save_markdown(&path)?;
        Ok(path)
    }
}

// ── Helpers ────────────────────────────────────────────────────────

fn first_sentence(text: &str) -> &str {
    let end = text
        .find(". ")
        .or_else(|| text.find(".\n"))
        .map(|i| i + 1)
        .unwrap_or(text.len().min(120));
    &text[..end]
}

fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(40)
        .collect()
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_brief_renders() {
        let brief = RevisionBrief::new("Test Project");
        let md = brief.to_markdown();
        assert!(md.contains("# Revision Brief — Test Project"));
        assert!(md.contains("Generated:"));
        assert!(brief.is_empty());
    }

    #[test]
    fn brief_with_revisions_renders() {
        let mut brief = RevisionBrief::new("The Long Way Home");
        brief.focus = "Character arc for Sarah".to_string();
        brief.scope = "Scenes 8-14".to_string();
        brief.overview = "Sarah's motivation shift needs a visible trigger.".to_string();

        brief.add_revision(Revision {
            scene_id: Some("scene-12".to_string()),
            scene_title: "Scene 12: The Letter".to_string(),
            file_path: "chapter-3.md".to_string(),
            line_start: 203,
            line_end: 218,
            priority: Priority::High,
            current_state: "Sarah stared at the envelope.".to_string(),
            issue: "Sarah's reaction contradicts her established objective.".to_string(),
            suggestion: "Add 2-3 sentences showing her processing the letter.".to_string(),
            draft_passage: Some("She turned the envelope over twice.".to_string()),
            graph_impact: vec![
                "Sarah objective shifts: protect_marcus -> self_preservation".to_string(),
            ],
        });

        brief.add_structural_note("Pacing", "Scenes 8-11 are all dialogue-heavy.");

        let md = brief.to_markdown();

        assert!(md.contains("## Overview"));
        assert!(md.contains("**Focus**: Character arc for Sarah"));
        assert!(md.contains("**Scope**: Scenes 8-14"));
        assert!(md.contains("### Scene 12: The Letter"));
        assert!(md.contains("**File**: chapter-3.md, lines 203-218"));
        assert!(md.contains("**Priority**: High"));
        assert!(md.contains("> Sarah stared at the envelope."));
        assert!(md.contains("**Issue**:"));
        assert!(md.contains("**Suggested revision**:"));
        assert!(md.contains("> She turned the envelope over twice."));
        assert!(md.contains("**Graph impact**:"));
        assert!(md.contains("## Structural Notes"));
        assert!(md.contains("- **Pacing**: Scenes 8-11"));
        assert!(md.contains("## Checklist"));
        assert!(md.contains("- [ ] Scene 12: The Letter:"));
    }

    #[test]
    fn default_filename_uses_slug() {
        let mut brief = RevisionBrief::new("Test");
        brief.focus = "Character Arc for Sarah".to_string();
        let filename = brief.default_filename();
        assert!(filename.ends_with(".md"));
        assert!(filename.contains("character-arc-for-sarah"));
    }

    #[test]
    fn priority_from_str_lossy() {
        assert_eq!(Priority::from_str_lossy("high"), Priority::High);
        assert_eq!(Priority::from_str_lossy("LOW"), Priority::Low);
        assert_eq!(Priority::from_str_lossy("unknown"), Priority::Medium);
    }

    #[test]
    fn save_to_project_creates_briefs_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".laires")).unwrap();

        let brief = RevisionBrief::new("Test");
        let path = brief.save_to_project(root).unwrap();

        assert!(path.exists());
        assert!(path.to_string_lossy().contains(".laires/briefs/"));
    }
}
