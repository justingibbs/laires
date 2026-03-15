use regex::Regex;

use crate::concepts::file_buffer_manager::FileBufferManager;
use crate::concepts::scene_map::{SceneId, SceneMap};
use crate::concepts::text_buffer::TextBuffer;

#[derive(Debug, Clone)]
pub struct StorySceneSummary {
    pub number: usize,
    pub id: SceneId,
    pub title: Option<String>,
    pub file_path: String,
    pub word_count: usize,
}

#[derive(Debug, Clone)]
pub struct StorySceneContent {
    pub number: usize,
    pub id: SceneId,
    pub title: Option<String>,
    pub file_path: String,
    pub text: String,
    pub word_count: usize,
}

#[derive(Debug, Clone)]
pub struct StorySearchHit {
    pub file_path: String,
    pub byte_offset: usize,
    pub line_number: usize,
    pub context: String,
}

pub struct StoryAccess<'a> {
    primary_text_buffer: &'a TextBuffer,
    primary_scene_map: &'a SceneMap,
    file_buffer_manager: Option<&'a FileBufferManager>,
}

impl<'a> StoryAccess<'a> {
    pub fn new(
        primary_text_buffer: &'a TextBuffer,
        primary_scene_map: &'a SceneMap,
        file_buffer_manager: Option<&'a FileBufferManager>,
    ) -> Self {
        Self {
            primary_text_buffer,
            primary_scene_map,
            file_buffer_manager,
        }
    }

    pub fn scene_count(&self) -> usize {
        self.file_buffer_manager
            .map(|fbm| fbm.total_scene_count())
            .unwrap_or_else(|| self.primary_scene_map.scene_count())
    }

    pub fn word_count(&self) -> usize {
        self.file_buffer_manager
            .map(|fbm| fbm.total_word_count())
            .unwrap_or_else(|| self.primary_text_buffer.word_count())
    }

    pub fn line_count(&self) -> usize {
        self.file_buffer_manager
            .map(|fbm| {
                fbm.entries()
                    .iter()
                    .map(|entry| entry.text_buffer.line_count())
                    .sum()
            })
            .unwrap_or_else(|| self.primary_text_buffer.line_count())
    }

    pub fn pending_scene_count(&self) -> usize {
        self.file_buffer_manager
            .map(|fbm| fbm.get_pending_scenes().len())
            .unwrap_or_else(|| self.primary_scene_map.get_pending().len())
    }

    pub fn resolve_scene_id(&self, scene_ref: &str) -> Option<String> {
        if let Ok(num) = scene_ref.parse::<usize>() {
            return self
                .list_scenes()
                .get(num.saturating_sub(1))
                .map(|scene| scene.id.clone());
        }

        if let Some(fbm) = self.file_buffer_manager {
            if fbm.get_scene(scene_ref).is_some() {
                return Some(scene_ref.to_string());
            }
        }

        self.primary_scene_map
            .get_scene(scene_ref)
            .map(|scene| scene.id.clone())
    }

    pub fn read_scene(&self, scene_ref: &str) -> Option<StorySceneContent> {
        self.resolve_scene_id(scene_ref)
            .and_then(|scene_id| self.read_scene_by_id(&scene_id))
    }

    pub fn read_scene_by_id(&self, scene_id: &str) -> Option<StorySceneContent> {
        if let Some(fbm) = self.file_buffer_manager {
            let mut number = 1;
            for entry in fbm.entries() {
                for scene in entry.scene_map.list_scenes() {
                    if scene.id == scene_id {
                        let text = entry
                            .text_buffer
                            .read(scene.byte_range())
                            .unwrap_or_default();
                        return Some(StorySceneContent {
                            number,
                            id: scene.id.clone(),
                            title: scene.title.clone(),
                            file_path: scene.file_path.clone(),
                            word_count: text.split_whitespace().count(),
                            text,
                        });
                    }
                    number += 1;
                }
            }
            return None;
        }

        self.primary_scene_map
            .list_scenes()
            .iter()
            .enumerate()
            .find(|(_, scene)| scene.id == scene_id)
            .map(|(idx, scene)| {
                let text = self
                    .primary_text_buffer
                    .read(scene.byte_range())
                    .unwrap_or_default();
                StorySceneContent {
                    number: idx + 1,
                    id: scene.id.clone(),
                    title: scene.title.clone(),
                    file_path: scene.file_path.clone(),
                    word_count: text.split_whitespace().count(),
                    text,
                }
            })
    }

    pub fn list_scenes(&self) -> Vec<StorySceneSummary> {
        if let Some(fbm) = self.file_buffer_manager {
            let mut scenes = Vec::new();
            let mut number = 1;
            for entry in fbm.entries() {
                for scene in entry.scene_map.list_scenes() {
                    let text = entry
                        .text_buffer
                        .read(scene.byte_range())
                        .unwrap_or_default();
                    scenes.push(StorySceneSummary {
                        number,
                        id: scene.id.clone(),
                        title: scene.title.clone(),
                        file_path: scene.file_path.clone(),
                        word_count: text.split_whitespace().count(),
                    });
                    number += 1;
                }
            }
            return scenes;
        }

        self.primary_scene_map
            .list_scenes()
            .iter()
            .enumerate()
            .map(|(idx, scene)| {
                let text = self
                    .primary_text_buffer
                    .read(scene.byte_range())
                    .unwrap_or_default();
                StorySceneSummary {
                    number: idx + 1,
                    id: scene.id.clone(),
                    title: scene.title.clone(),
                    file_path: scene.file_path.clone(),
                    word_count: text.split_whitespace().count(),
                }
            })
            .collect()
    }

    pub fn search(&self, pattern: &str) -> Result<Vec<StorySearchHit>, regex::Error> {
        if let Some(fbm) = self.file_buffer_manager {
            return Ok(fbm
                .search(pattern)
                .into_iter()
                .map(|hit| StorySearchHit {
                    file_path: hit.file_path,
                    byte_offset: hit.byte_offset,
                    line_number: hit.line_number,
                    context: hit.context,
                })
                .collect());
        }

        let re = Regex::new(pattern)?;
        let text = self.primary_text_buffer.read_all();
        let hits = text
            .lines()
            .enumerate()
            .filter_map(|(line_num, line)| {
                re.is_match(line).then(|| StorySearchHit {
                    file_path: String::new(),
                    byte_offset: 0,
                    line_number: line_num + 1,
                    context: line.trim().to_string(),
                })
            })
            .collect();
        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::concepts::file_buffer_manager::FileBufferManager;
    use crate::concepts::manifest::{Manifest, ManifestMeta, StoryFile};
    use crate::concepts::scene_map::{ParseMode, SceneMap};
    use crate::concepts::text_buffer::TextBuffer;

    use super::StoryAccess;

    fn single_story() -> (TextBuffer, SceneMap) {
        let text = "## Scene 1\n\nAlpha beta gamma delta epsilon.\n\n## Scene 2\n\nZeta eta theta iota kappa.";
        let text_buffer = TextBuffer::from_str(text, PathBuf::from("/tmp/story.md"));
        let mut scene_map = SceneMap::new(ParseMode::Prose);
        scene_map.full_reindex(text, "");
        (text_buffer, scene_map)
    }

    fn multi_story() -> (tempfile::TempDir, TextBuffer, SceneMap, FileBufferManager) {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            temp_dir.path().join("chapter-1.md"),
            "## One\n\nalpha beta gamma delta epsilon\n\n## Two\n\nzeta eta theta iota kappa",
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("chapter-2.md"),
            "## Three\n\nafter midnight the station is empty and cold",
        )
        .unwrap();

        let manifest = Manifest {
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
                },
                StoryFile {
                    path: "chapter-2.md".to_string(),
                    format: "prose".to_string(),
                    order: 2,
                    content_hash: "h2".to_string(),
                },
            ],
            context_files: vec![],
            excluded: vec![],
        };

        let fbm = FileBufferManager::from_manifest(&manifest, temp_dir.path()).unwrap();

        let (text_buffer, scene_map) = single_story();
        (temp_dir, text_buffer, scene_map, fbm)
    }

    #[test]
    fn aggregates_multi_file_scene_counts() {
        let (_tmp, text_buffer, scene_map, fbm) = multi_story();
        let story = StoryAccess::new(&text_buffer, &scene_map, Some(&fbm));

        assert_eq!(story.scene_count(), 3);
        assert_eq!(story.list_scenes().len(), 3);
    }

    #[test]
    fn resolves_scene_numbers_across_files() {
        let (_tmp, text_buffer, scene_map, fbm) = multi_story();
        let story = StoryAccess::new(&text_buffer, &scene_map, Some(&fbm));

        let scene = story.read_scene("3").unwrap();
        assert_eq!(scene.number, 3);
        assert_eq!(scene.file_path, "chapter-2.md");
    }

    #[test]
    fn searches_across_manifest_files() {
        let (_tmp, text_buffer, scene_map, fbm) = multi_story();
        let story = StoryAccess::new(&text_buffer, &scene_map, Some(&fbm));

        let hits = story.search("midnight").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file_path, "chapter-2.md");
    }
}
