use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use uuid::Uuid;

use crate::concepts::text_buffer::ByteRange;

pub type SceneId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterCue {
    pub character_name: String,
    pub scene_id: SceneId,
    pub byte_offset: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneSpan {
    pub id: SceneId,
    pub start: usize,
    pub end: usize,
    pub content_hash: String,
    pub title: Option<String>,
    #[serde(default)]
    pub file_path: String,
}

impl SceneSpan {
    pub fn byte_range(&self) -> ByteRange {
        ByteRange::new(self.start, self.end)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParseMode {
    Prose,
    Fountain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneMap {
    scenes: Vec<SceneSpan>,
    parse_mode: ParseMode,
    pending_reindex: HashSet<SceneId>,
    #[serde(default)]
    character_cues: Vec<CharacterCue>,
}

impl SceneMap {
    pub fn new(parse_mode: ParseMode) -> Self {
        Self {
            scenes: Vec::new(),
            parse_mode,
            pending_reindex: HashSet::new(),
            character_cues: Vec::new(),
        }
    }

    /// Rebuild all scene boundaries from scratch
    pub fn full_reindex(&mut self, text: &str, file_path: &str) {
        let boundaries = match self.parse_mode {
            ParseMode::Prose => detect_prose_boundaries(text),
            ParseMode::Fountain => detect_fountain_boundaries(text),
        };

        self.scenes.clear();
        self.pending_reindex.clear();

        if boundaries.is_empty() {
            // Entire text is one scene
            let id = new_scene_id();
            let hash = content_hash(text);
            self.scenes.push(SceneSpan {
                id: id.clone(),
                start: 0,
                end: text.len(),
                content_hash: hash,
                title: None,
                file_path: file_path.to_string(),
            });
            self.pending_reindex.insert(id);
            return;
        }

        // Build raw scene spans from boundaries
        let mut raw_scenes: Vec<(usize, usize, Option<String>)> = Vec::new();

        // Text before the first boundary
        if boundaries[0].offset > 0 {
            raw_scenes.push((0, boundaries[0].offset, None));
        }

        for (i, boundary) in boundaries.iter().enumerate() {
            let scene_end = if i + 1 < boundaries.len() {
                boundaries[i + 1].offset
            } else {
                text.len()
            };
            raw_scenes.push((boundary.offset, scene_end, boundary.title.clone()));
        }

        // Filter out scenes with negligible content and fold them into neighbors
        for (start, end, title) in &raw_scenes {
            let scene_text = &text[*start..*end];
            let word_count = scene_text.split_whitespace().count();

            if word_count < MIN_SCENE_WORDS {
                // Too small — fold into the previous scene if possible
                if let Some(last) = self.scenes.last_mut() {
                    last.end = *end;
                    last.content_hash = content_hash(&text[last.start..last.end]);
                    // Prefer a real title over none
                    if title.is_some() && last.title.is_none() {
                        last.title.clone_from(title);
                    }
                }
                continue;
            }

            let id = new_scene_id();
            let hash = content_hash(scene_text);
            self.scenes.push(SceneSpan {
                id: id.clone(),
                start: *start,
                end: *end,
                content_hash: hash,
                title: title.clone(),
                file_path: file_path.to_string(),
            });
            self.pending_reindex.insert(id);
        }

        // If no scenes survived filtering, add the whole text as one scene
        if self.scenes.is_empty() && !text.is_empty() {
            let id = new_scene_id();
            let hash = content_hash(text);
            self.scenes.push(SceneSpan {
                id: id.clone(),
                start: 0,
                end: text.len(),
                content_hash: hash,
                title: None,
                file_path: file_path.to_string(),
            });
            self.pending_reindex.insert(id);
        }

        // Detect character cues in Fountain mode
        if self.parse_mode == ParseMode::Fountain {
            self.detect_character_cues(text);
        }
    }

    /// Detect character cues in Fountain text and assign them to scenes
    fn detect_character_cues(&mut self, text: &str) {
        self.character_cues.clear();

        let cue_re = Regex::new(
            r"(?m)^\s*\n([A-Z][A-Z0-9 .'()\-]+?)(?:\s*\((?:V\.?O\.?|O\.?S\.?|CONT'?D?|O\.?C\.?)\))?\s*\n"
        ).unwrap();

        for caps in cue_re.captures_iter(text) {
            let full_match = caps.get(0).unwrap();
            let name_match = caps.get(1).unwrap();
            let byte_offset = full_match.start();
            let character_name = name_match.as_str().trim().to_string();

            // Find which scene this cue belongs to
            if let Some(scene) = self.scenes.iter().find(|s| s.start <= byte_offset && byte_offset < s.end) {
                self.character_cues.push(CharacterCue {
                    character_name,
                    scene_id: scene.id.clone(),
                    byte_offset,
                });
            }
        }
    }

    /// Get detected character cues (Fountain mode only)
    pub fn character_cues(&self) -> &[CharacterCue] {
        &self.character_cues
    }

    /// Incrementally update scene boundaries affected by changes
    #[allow(dead_code)]
    pub fn reindex(&mut self, text: &str, file_path: &str, changed_ranges: &[ByteRange]) {
        // For simplicity in Phase 1, do a full reindex but preserve scene IDs
        // where content hasn't changed
        let old_scenes: Vec<SceneSpan> = self.scenes.clone();
        let old_hashes: Vec<(String, String)> = old_scenes
            .iter()
            .map(|s| (s.id.clone(), s.content_hash.clone()))
            .collect();

        self.full_reindex(text, file_path);

        // Try to preserve scene IDs for scenes whose content hasn't changed
        // (matching by position and hash)
        for scene in &mut self.scenes {
            for (old_id, old_hash) in &old_hashes {
                if scene.content_hash == *old_hash {
                    scene.id.clone_from(old_id);
                    // If hash matches, no re-analysis needed
                    self.pending_reindex.remove(&scene.id);
                    break;
                }
            }
        }

        // Mark scenes that overlap with changed ranges as pending
        for scene in &self.scenes {
            let scene_range = scene.byte_range();
            for changed in changed_ranges {
                if scene_range.overlaps(changed) {
                    self.pending_reindex.insert(scene.id.clone());
                    break;
                }
            }
        }
    }

    /// Get a scene by ID
    pub fn get_scene(&self, id: &str) -> Option<&SceneSpan> {
        self.scenes.iter().find(|s| s.id == id)
    }

    /// Get the scene containing a byte offset
    #[allow(dead_code)]
    pub fn get_scene_at(&self, offset: usize) -> Option<&SceneSpan> {
        self.scenes
            .iter()
            .find(|s| s.start <= offset && offset < s.end)
    }

    /// List all scenes in document order
    pub fn list_scenes(&self) -> &[SceneSpan] {
        &self.scenes
    }

    /// Mark a scene as analyzed
    pub fn mark_analyzed(&mut self, id: &str) {
        self.pending_reindex.remove(id);
    }

    /// Get scenes awaiting re-analysis
    pub fn get_pending(&self) -> &HashSet<SceneId> {
        &self.pending_reindex
    }

    /// Get the number of scenes
    pub fn scene_count(&self) -> usize {
        self.scenes.len()
    }

    /// Save scene map to disk
    #[allow(dead_code)]
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load scene map from disk
    #[allow(dead_code)]
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let map: SceneMap = serde_json::from_str(&content)?;
        Ok(map)
    }
}

/// Minimum word count for a scene to be considered real content.
/// Scenes below this threshold are merged into the next scene.
const MIN_SCENE_WORDS: usize = 5;

/// Maximum byte gap between two boundaries to consider them part of the
/// same section break (e.g., `---\n\n## Part One` is one break, not two).
const MERGE_GAP_BYTES: usize = 50;

// -- Scene boundary detection --

struct SceneBoundary {
    offset: usize,
    title: Option<String>,
}

/// Detect scene boundaries in prose (Markdown) text
fn detect_prose_boundaries(text: &str) -> Vec<SceneBoundary> {
    let mut boundaries = Vec::new();

    // Pattern 1: Horizontal rules (---, ***, ___)
    let hr_re =
        Regex::new(r"(?m)^[ \t]*(-{3,}|\*{3,}|_{3,})[ \t]*$").unwrap();

    // Pattern 2: Markdown headings (## Chapter 3, ### Scene 2)
    let heading_re = Regex::new(r"(?m)^(#{1,6})\s+(.+)$").unwrap();

    // Pattern 3: HTML comment markers <!-- scene: "title" -->
    let comment_re =
        Regex::new(r#"(?m)<!--\s*scene:\s*"([^"]+)"\s*-->"#).unwrap();

    // Collect all boundaries with their byte offsets
    for m in hr_re.find_iter(text) {
        let line_start = text[..m.start()].rfind('\n').map_or(0, |p| p + 1);
        boundaries.push(SceneBoundary {
            offset: line_start,
            title: None,
        });
    }

    for caps in heading_re.captures_iter(text) {
        let m = caps.get(0).unwrap();
        let line_start = text[..m.start()].rfind('\n').map_or(0, |p| p + 1);
        let title = caps.get(2).map(|t| t.as_str().trim().to_string());
        boundaries.push(SceneBoundary {
            offset: line_start,
            title,
        });
    }

    for caps in comment_re.captures_iter(text) {
        let m = caps.get(0).unwrap();
        let line_start = text[..m.start()].rfind('\n').map_or(0, |p| p + 1);
        let title = caps.get(1).map(|t| t.as_str().to_string());
        boundaries.push(SceneBoundary {
            offset: line_start,
            title,
        });
    }

    // Sort by offset
    boundaries.sort_by_key(|b| b.offset);

    // Merge boundaries that are close together (e.g., `---` followed by `## Heading`).
    // When two boundaries are within MERGE_GAP_BYTES of each other, keep the one
    // with a title (the heading), or the later one if neither has a title.
    let mut merged: Vec<SceneBoundary> = Vec::new();
    for b in boundaries {
        if let Some(last) = merged.last_mut() {
            if b.offset - last.offset < MERGE_GAP_BYTES {
                // Merge: prefer the one with a title
                if b.title.is_some() {
                    last.title = b.title;
                    last.offset = b.offset;
                }
                continue;
            }
        }
        merged.push(b);
    }

    merged
}

/// Detect scene boundaries in Fountain (screenplay) text
fn detect_fountain_boundaries(text: &str) -> Vec<SceneBoundary> {
    let mut boundaries = Vec::new();

    // Fountain scene headings: INT. or EXT. (or INT./EXT., I/E, etc.)
    let scene_re =
        Regex::new(r"(?mi)^(INT\.|EXT\.|INT\./EXT\.|I/E\.|EST\.)[\t ]+(.+)$")
            .unwrap();

    for caps in scene_re.captures_iter(text) {
        let m = caps.get(0).unwrap();
        let line_start = text[..m.start()].rfind('\n').map_or(0, |p| p + 1);
        let title = caps.get(0).map(|t| t.as_str().trim().to_string());
        boundaries.push(SceneBoundary {
            offset: line_start,
            title,
        });
    }

    boundaries.sort_by_key(|b| b.offset);
    boundaries
}

fn content_hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

fn new_scene_id() -> SceneId {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prose_single_scene() {
        let mut map = SceneMap::new(ParseMode::Prose);
        map.full_reindex("Just a simple story with no breaks.", "");
        assert_eq!(map.scene_count(), 1);
        assert_eq!(map.scenes[0].start, 0);
    }

    #[test]
    fn test_prose_heading_boundaries() {
        let text = "## Chapter 1\n\nSome text here, enough words to count as real content for the scene.\n\n## Chapter 2\n\nMore text here, also enough words to count as real content.";
        let mut map = SceneMap::new(ParseMode::Prose);
        map.full_reindex(text, "");
        assert_eq!(map.scene_count(), 2);
        assert_eq!(
            map.scenes[0].title.as_deref(),
            Some("Chapter 1")
        );
        assert_eq!(
            map.scenes[1].title.as_deref(),
            Some("Chapter 2")
        );
    }

    #[test]
    fn test_prose_hr_with_heading_merges() {
        // HR followed by heading should produce ONE scene, not two
        let text = "First scene with enough words to be a real scene here.\n\n---\n\n## Part Two\n\nSecond scene also with enough words to count as real.";
        let mut map = SceneMap::new(ParseMode::Prose);
        map.full_reindex(text, "");
        assert_eq!(map.scene_count(), 2);
        assert_eq!(map.scenes[1].title.as_deref(), Some("Part Two"));
    }

    #[test]
    fn test_prose_comment_markers() {
        let text = "Intro text with enough words to be a real scene definitely.\n\n<!-- scene: \"The Arrival\" -->\n\nScene text with enough words to also be real content here.";
        let mut map = SceneMap::new(ParseMode::Prose);
        map.full_reindex(text, "");
        assert!(map.scene_count() >= 2);
    }

    #[test]
    fn test_fountain_scene_headings() {
        let text = "INT. COFFEE SHOP - DAY\n\nDialogue here with enough words to fill a scene properly.\n\nEXT. PARKING LOT - NIGHT\n\nMore text here also enough words to be a scene.";
        let mut map = SceneMap::new(ParseMode::Fountain);
        map.full_reindex(text, "");
        assert_eq!(map.scene_count(), 2);
    }

    #[test]
    fn test_ghost_train_structure() {
        // Simulates the ghost train story structure: title, subtitle, then 5 parts with HR+heading
        let text = "\
# The Ghost Train\n\n\
### A Mystery in Five Parts\n\n\
---\n\n\
## Part One: Smoke and Mirrors\n\n\
Maguire stood with his hands in his coat pockets and watched the structure fold in on itself in a shower of sparks. Somewhere behind the police tape, a woman was screaming. Not the theatrical kind you hear in films. Her name was Jenny Godson.\n\n\
---\n\n\
## Part Two: The Official Story\n\n\
The investigation such as it was lasted a few weeks. Maguire was not lead that honour went to a senior detective named Grainger, a heavy man with a red face and an instinct for the path of least resistance.\n\n\
---\n\n\
## Part Three: The King of the Cross\n\n\
Every city has a shadow. Sydney shadow had a name: Victor Kovac. Maguire had never met the man but every copper in New South Wales knew the legend.\n\n\
---\n\n\
*Author's Note: This story is fiction.*";
        let mut map = SceneMap::new(ParseMode::Prose);
        map.full_reindex(text, "");
        // Should be ~4-5 scenes (the 3 parts + maybe title preamble + author's note),
        // NOT 10+ scenes from separate HR and heading boundaries
        assert!(
            map.scene_count() <= 6,
            "Expected <= 6 scenes, got {}",
            map.scene_count()
        );
        // The parts should have their titles
        let titles: Vec<Option<&str>> =
            map.scenes.iter().map(|s| s.title.as_deref()).collect();
        assert!(
            titles.iter().any(|t| t == &Some("Part One: Smoke and Mirrors")),
            "Missing Part One title in {titles:?}"
        );
    }

    #[test]
    fn test_get_scene_at_offset() {
        let text = "## Scene 1\n\nFirst text with enough words to be a real scene content.\n\n## Scene 2\n\nSecond text with enough words to be a real scene content.";
        let mut map = SceneMap::new(ParseMode::Prose);
        map.full_reindex(text, "");

        let scene = map.get_scene_at(0).unwrap();
        assert_eq!(scene.title.as_deref(), Some("Scene 1"));

        let scene = map.get_scene_at(text.len() - 1).unwrap();
        assert_eq!(scene.title.as_deref(), Some("Scene 2"));
    }

    #[test]
    fn test_all_scenes_pending_after_full_reindex() {
        let text = "## Scene 1\n\nText enough words to be real scene content here.\n\n## Scene 2\n\nText enough words to be real scene content here.";
        let mut map = SceneMap::new(ParseMode::Prose);
        map.full_reindex(text, "");
        assert_eq!(map.get_pending().len(), map.scene_count());
    }

    #[test]
    fn test_mark_analyzed() {
        let text = "## Scene 1\n\nText enough words to be real scene content here.";
        let mut map = SceneMap::new(ParseMode::Prose);
        map.full_reindex(text, "");

        let id = map.scenes[0].id.clone();
        map.mark_analyzed(&id);
        assert!(!map.get_pending().contains(&id));
    }

    #[test]
    fn test_fountain_character_cue_detection() {
        let text = "INT. COFFEE SHOP - DAY\n\nSome establishing action with enough words to count as real.\n\nMARCUS\nHello there, how are you doing today?\n\nELENA\nI'm doing just fine, thank you very much.\n";
        let mut map = SceneMap::new(ParseMode::Fountain);
        map.full_reindex(text, "");

        let cues = map.character_cues();
        let names: Vec<&str> = cues.iter().map(|c| c.character_name.as_str()).collect();
        assert!(names.contains(&"MARCUS"), "Expected MARCUS in {names:?}");
        assert!(names.contains(&"ELENA"), "Expected ELENA in {names:?}");
    }

    #[test]
    fn test_fountain_cue_with_parenthetical() {
        let text = "INT. OFFICE - NIGHT\n\nSome establishing action with enough words to count as real.\n\nMARCUS (V.O.)\nI remember that day clearly, it was something.\n\nELENA (CONT'D)\nAnd then what happened after all that?\n";
        let mut map = SceneMap::new(ParseMode::Fountain);
        map.full_reindex(text, "");

        let cues = map.character_cues();
        let names: Vec<&str> = cues.iter().map(|c| c.character_name.as_str()).collect();
        assert!(names.contains(&"MARCUS"), "Expected MARCUS in {names:?}");
        assert!(names.contains(&"ELENA"), "Expected ELENA in {names:?}");
    }

    #[test]
    fn test_fountain_cue_to_scene_assignment() {
        let text = "INT. COFFEE SHOP - DAY\n\nSome establishing action with enough words for scene one content.\n\nMARCUS\nHello there, a line of dialogue for the scene.\n\nEXT. PARKING LOT - NIGHT\n\nMore establishing text with enough words for scene two content.\n\nELENA\nGoodbye, another line of dialogue for scene two.\n";
        let mut map = SceneMap::new(ParseMode::Fountain);
        map.full_reindex(text, "");

        assert_eq!(map.scene_count(), 2);
        let cues = map.character_cues();

        // MARCUS should be in scene 1, ELENA in scene 2
        let marcus_cue = cues.iter().find(|c| c.character_name == "MARCUS");
        let elena_cue = cues.iter().find(|c| c.character_name == "ELENA");

        assert!(marcus_cue.is_some(), "Expected MARCUS cue");
        assert!(elena_cue.is_some(), "Expected ELENA cue");

        let scene1_id = &map.list_scenes()[0].id;
        let scene2_id = &map.list_scenes()[1].id;
        assert_eq!(&marcus_cue.unwrap().scene_id, scene1_id);
        assert_eq!(&elena_cue.unwrap().scene_id, scene2_id);
    }

    #[test]
    fn test_file_path_set_after_reindex() {
        let text = "## Scene 1\n\nText enough words to be real scene content here.";
        let mut map = SceneMap::new(ParseMode::Prose);
        map.full_reindex(text, "chapter-1.md");
        assert_eq!(map.scenes[0].file_path, "chapter-1.md");
    }

    #[test]
    fn test_file_path_serde_default_compat() {
        // Simulate loading old JSON without file_path field
        let json = r#"{
            "scenes": [{
                "id": "test-id",
                "start": 0,
                "end": 10,
                "content_hash": "abc",
                "title": null
            }],
            "parse_mode": "Prose",
            "pending_reindex": [],
            "character_cues": []
        }"#;
        let map: SceneMap = serde_json::from_str(json).unwrap();
        assert_eq!(map.scenes[0].file_path, "");
    }
}
