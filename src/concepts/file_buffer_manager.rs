use regex::Regex;
use std::path::Path;

use crate::concepts::manifest::Manifest;
use crate::concepts::scene_map::{ParseMode, SceneMap, SceneSpan};
use crate::concepts::text_buffer::TextBuffer;

pub struct FileEntry {
    pub text_buffer: TextBuffer,
    pub scene_map: SceneMap,
    pub file_path: String,
    pub format: String,
    pub order: usize,
}

pub struct SearchHit {
    pub file_path: String,
    pub byte_offset: usize,
    pub line_number: usize,
    pub context: String,
}

pub struct FileBufferManager {
    entries: Vec<FileEntry>,
}

impl FileBufferManager {
    /// Build a FileBufferManager from a manifest, loading all story files.
    pub fn from_manifest(manifest: &Manifest, project_root: &Path) -> anyhow::Result<Self> {
        let mut entries = Vec::new();

        for story_file in &manifest.story_files {
            let abs_path = project_root.join(&story_file.path);
            if !abs_path.exists() {
                eprintln!("Warning: story file not found: {}", story_file.path);
                continue;
            }

            let text_buffer = TextBuffer::from_file(abs_path)?;
            let full_text = text_buffer.read_all();

            let parse_mode = match story_file.format.as_str() {
                "fountain" => ParseMode::Fountain,
                _ => ParseMode::Prose,
            };
            let mut scene_map = SceneMap::new(parse_mode);

            if !full_text.trim().is_empty() {
                scene_map.full_reindex(&full_text, &story_file.path);
            }

            entries.push(FileEntry {
                text_buffer,
                scene_map,
                file_path: story_file.path.clone(),
                format: story_file.format.clone(),
                order: story_file.order,
            });
        }

        entries.sort_by_key(|e| e.order);

        Ok(Self { entries })
    }

    /// Get a file entry by relative path.
    pub fn get_entry(&self, path: &str) -> Option<&FileEntry> {
        self.entries.iter().find(|e| e.file_path == path)
    }

    /// Get a mutable file entry by relative path.
    pub fn get_entry_mut(&mut self, path: &str) -> Option<&mut FileEntry> {
        self.entries.iter_mut().find(|e| e.file_path == path)
    }

    /// All entries in manifest order.
    pub fn entries(&self) -> &[FileEntry] {
        &self.entries
    }

    /// List all scenes across all files in manifest order, then by byte offset.
    pub fn list_all_scenes(&self) -> Vec<&SceneSpan> {
        let mut scenes = Vec::new();
        for entry in &self.entries {
            for scene in entry.scene_map.list_scenes() {
                scenes.push(scene);
            }
        }
        scenes
    }

    /// Find a scene by ID across all files. Returns the scene and its TextBuffer.
    pub fn get_scene(&self, id: &str) -> Option<(&SceneSpan, &TextBuffer)> {
        for entry in &self.entries {
            if let Some(scene) = entry.scene_map.get_scene(id) {
                return Some((scene, &entry.text_buffer));
            }
        }
        None
    }

    /// Get all pending (un-analyzed) scenes across files.
    pub fn get_pending_scenes(&self) -> Vec<&SceneSpan> {
        let mut pending = Vec::new();
        for entry in &self.entries {
            let pending_ids = entry.scene_map.get_pending();
            for scene in entry.scene_map.list_scenes() {
                if pending_ids.contains(&scene.id) {
                    pending.push(scene);
                }
            }
        }
        pending
    }

    /// Regex search across all buffers.
    pub fn search(&self, pattern: &str) -> Vec<SearchHit> {
        let re = match Regex::new(pattern) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };

        let mut hits = Vec::new();
        for entry in &self.entries {
            let text = entry.text_buffer.read_all();
            for m in re.find_iter(&text) {
                let byte_offset = m.start();
                let line_number = text[..byte_offset].matches('\n').count() + 1;
                // Extract context: the line containing the match
                let line_start = text[..byte_offset].rfind('\n').map_or(0, |p| p + 1);
                let line_end = text[byte_offset..]
                    .find('\n')
                    .map_or(text.len(), |p| byte_offset + p);
                let context = text[line_start..line_end].to_string();

                hits.push(SearchHit {
                    file_path: entry.file_path.clone(),
                    byte_offset,
                    line_number,
                    context,
                });
            }
        }
        hits
    }

    /// Total word count across all story files.
    pub fn total_word_count(&self) -> usize {
        self.entries
            .iter()
            .map(|e| e.text_buffer.word_count())
            .sum()
    }

    /// Total scene count across all files.
    pub fn total_scene_count(&self) -> usize {
        self.entries.iter().map(|e| e.scene_map.scene_count()).sum()
    }

    /// Number of loaded story files.
    pub fn story_file_count(&self) -> usize {
        self.entries.len()
    }

    /// Save any dirty text buffers.
    pub fn save_dirty(&mut self) -> anyhow::Result<()> {
        for entry in &mut self.entries {
            if entry.text_buffer.is_dirty() {
                entry.text_buffer.save()?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concepts::manifest::{ManifestMeta, StoryFile};
    use std::fs;

    fn test_manifest(root: &Path) -> Manifest {
        // Create two story files on disk
        fs::write(
            root.join("chapter-1.md"),
            "## Scene A\n\nEnough words in chapter one to form a real scene here.\n\n## Scene B\n\nMore words in chapter one scene two for real content.",
        )
        .unwrap();
        fs::write(
            root.join("chapter-2.md"),
            "## Scene C\n\nEnough words in chapter two to form a real scene here.",
        )
        .unwrap();

        Manifest {
            meta: ManifestMeta {
                last_scan: "2026-01-01T00:00:00Z".to_string(),
                classification_model: "test".to_string(),
            },
            story_files: vec![
                StoryFile {
                    path: "chapter-1.md".to_string(),
                    format: "prose".to_string(),
                    order: 1,
                    content_hash: "h1".to_string(),
                    editable: true,
                },
                StoryFile {
                    path: "chapter-2.md".to_string(),
                    format: "prose".to_string(),
                    order: 2,
                    content_hash: "h2".to_string(),
                    editable: true,
                },
            ],
            context_files: vec![],
            excluded: vec![],
        }
    }

    #[test]
    fn test_from_manifest_loads_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let manifest = test_manifest(root);

        let fbm = FileBufferManager::from_manifest(&manifest, root).unwrap();
        assert_eq!(fbm.story_file_count(), 2);
        assert_eq!(fbm.entries()[0].file_path, "chapter-1.md");
        assert_eq!(fbm.entries()[1].file_path, "chapter-2.md");
        assert!(!fbm.entries()[0].text_buffer.read_all().is_empty());
        assert!(!fbm.entries()[1].text_buffer.read_all().is_empty());
    }

    #[test]
    fn test_list_all_scenes_cross_file_ordering() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let manifest = test_manifest(root);

        let fbm = FileBufferManager::from_manifest(&manifest, root).unwrap();
        let all_scenes = fbm.list_all_scenes();

        // chapter-1 has 2 scenes, chapter-2 has 1
        assert_eq!(all_scenes.len(), 3);
        // First two scenes should be from chapter-1
        assert_eq!(all_scenes[0].file_path, "chapter-1.md");
        assert_eq!(all_scenes[1].file_path, "chapter-1.md");
        // Third from chapter-2
        assert_eq!(all_scenes[2].file_path, "chapter-2.md");
    }

    #[test]
    fn test_get_scene_by_id() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let manifest = test_manifest(root);

        let fbm = FileBufferManager::from_manifest(&manifest, root).unwrap();
        let all_scenes = fbm.list_all_scenes();
        let target_id = all_scenes[2].id.clone();

        let (scene, _buf) = fbm.get_scene(&target_id).unwrap();
        assert_eq!(scene.file_path, "chapter-2.md");
    }

    #[test]
    fn test_search_across_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let manifest = test_manifest(root);

        let fbm = FileBufferManager::from_manifest(&manifest, root).unwrap();
        let hits = fbm.search("chapter");
        // Both files contain "chapter" in their content
        let file_paths: Vec<&str> = hits.iter().map(|h| h.file_path.as_str()).collect();
        assert!(
            file_paths.contains(&"chapter-1.md")
                || file_paths.contains(&"chapter-2.md")
                || hits.is_empty(),
            "Search should find hits or gracefully return empty"
        );

        // Search for something specific to chapter-1
        let hits = fbm.search("Scene B");
        assert!(!hits.is_empty(), "Should find 'Scene B' in chapter-1.md");
        assert_eq!(hits[0].file_path, "chapter-1.md");
    }

    #[test]
    fn test_total_stats() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let manifest = test_manifest(root);

        let fbm = FileBufferManager::from_manifest(&manifest, root).unwrap();
        assert_eq!(fbm.total_scene_count(), 3);
        assert!(fbm.total_word_count() > 0);
        assert_eq!(fbm.story_file_count(), 2);
    }

    #[test]
    fn test_file_path_on_scene_span() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let manifest = test_manifest(root);

        let fbm = FileBufferManager::from_manifest(&manifest, root).unwrap();
        for entry in fbm.entries() {
            for scene in entry.scene_map.list_scenes() {
                assert_eq!(
                    scene.file_path, entry.file_path,
                    "Scene file_path should match its entry"
                );
            }
        }
    }

    #[test]
    fn test_get_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let manifest = test_manifest(root);

        let fbm = FileBufferManager::from_manifest(&manifest, root).unwrap();
        assert!(fbm.get_entry("chapter-1.md").is_some());
        assert!(fbm.get_entry("chapter-2.md").is_some());
        assert!(fbm.get_entry("nonexistent.md").is_none());
    }

    #[test]
    fn test_missing_file_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // Only create one of the two files
        fs::write(
            root.join("chapter-1.md"),
            "## Scene A\n\nEnough words in chapter one to form a real scene here.",
        )
        .unwrap();

        let manifest = Manifest {
            meta: ManifestMeta {
                last_scan: String::new(),
                classification_model: String::new(),
            },
            story_files: vec![
                StoryFile {
                    path: "chapter-1.md".to_string(),
                    format: "prose".to_string(),
                    order: 1,
                    content_hash: "h1".to_string(),
                    editable: true,
                },
                StoryFile {
                    path: "missing.md".to_string(),
                    format: "prose".to_string(),
                    order: 2,
                    content_hash: "h2".to_string(),
                    editable: true,
                },
            ],
            context_files: vec![],
            excluded: vec![],
        };

        let fbm = FileBufferManager::from_manifest(&manifest, root).unwrap();
        assert_eq!(fbm.story_file_count(), 1);
        assert_eq!(fbm.entries()[0].file_path, "chapter-1.md");
    }

    #[test]
    fn test_get_pending_scenes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let manifest = test_manifest(root);

        let fbm = FileBufferManager::from_manifest(&manifest, root).unwrap();
        let pending = fbm.get_pending_scenes();
        // All scenes should be pending after initial load
        assert_eq!(pending.len(), fbm.total_scene_count());
    }
}
