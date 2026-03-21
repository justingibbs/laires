use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::concepts::file_buffer_manager::FileBufferManager;
use crate::concepts::scene_map::SceneMap;
use crate::config::SCENES_FILE;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSceneCache {
    pub file_path: String,
    pub scene_map: SceneMap,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SceneCache {
    pub files: Vec<FileSceneCache>,
}

impl SceneCache {
    pub fn empty() -> Self {
        Self { files: Vec::new() }
    }

    pub fn from_file_buffer_manager(fbm: &FileBufferManager) -> Self {
        Self {
            files: fbm
                .entries()
                .iter()
                .map(|entry| FileSceneCache {
                    file_path: entry.file_path.clone(),
                    scene_map: entry.scene_map.clone(),
                })
                .collect(),
        }
    }

    pub fn from_single_file(file_path: String, scene_map: &SceneMap) -> Self {
        Self {
            files: vec![FileSceneCache {
                file_path,
                scene_map: scene_map.clone(),
            }],
        }
    }

    pub fn save_to_project(&self, project_root: &Path) -> anyhow::Result<()> {
        let path = project_root.join(crate::config::LAIRES_DIR).join(SCENES_FILE);
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn load_from_project(project_root: &Path) -> anyhow::Result<Self> {
        let path = project_root.join(crate::config::LAIRES_DIR).join(SCENES_FILE);
        let content = std::fs::read_to_string(path)?;
        let cache = serde_json::from_str(&content)?;
        Ok(cache)
    }
}

#[cfg(test)]
mod tests {
    use crate::concepts::file_buffer_manager::FileBufferManager;
    use crate::concepts::manifest::{Manifest, ManifestMeta, StoryFile};
    use crate::concepts::scene_map::{ParseMode, SceneMap};
    use super::SceneCache;

    #[test]
    fn round_trips_single_file_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let mut scene_map = SceneMap::new(ParseMode::Prose);
        scene_map.full_reindex("## Scene 1\n\nEnough words for a valid scene here.", "story.md");

        let cache = SceneCache::from_single_file("story.md".to_string(), &scene_map);
        cache.save_to_project(tmp.path()).unwrap_err();

        std::fs::create_dir(tmp.path().join(crate::config::LAIRES_DIR)).unwrap();
        cache.save_to_project(tmp.path()).unwrap();
        let loaded = SceneCache::load_from_project(tmp.path()).unwrap();

        assert_eq!(loaded.files.len(), 1);
        assert_eq!(loaded.files[0].file_path, "story.md");
        assert_eq!(loaded.files[0].scene_map.scene_count(), 1);
    }

    #[test]
    fn builds_cache_from_file_buffer_manager() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("chapter-1.md"),
            "## One\n\nEnough words for chapter one scene here.",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("chapter-2.md"),
            "## Two\n\nEnough words for chapter two scene here.",
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
        };

        let fbm = FileBufferManager::from_manifest(&manifest, tmp.path()).unwrap();
        let cache = SceneCache::from_file_buffer_manager(&fbm);

        assert_eq!(cache.files.len(), 2);
        assert_eq!(cache.files[0].file_path, "chapter-1.md");
        assert_eq!(cache.files[1].file_path, "chapter-2.md");
    }

    #[test]
    fn round_trips_multi_file_cache_through_project_file() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(crate::config::LAIRES_DIR)).unwrap();
        std::fs::write(
            tmp.path().join("chapter-1.md"),
            "## One\n\nEnough words for chapter one scene here.",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("chapter-2.md"),
            "## Two\n\nEnough words for chapter two scene here.",
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
        };

        let fbm = FileBufferManager::from_manifest(&manifest, tmp.path()).unwrap();
        let first_scene_id = fbm.entries()[0].scene_map.list_scenes()[0].id.clone();
        let second_scene_id = fbm.entries()[1].scene_map.list_scenes()[0].id.clone();

        let cache = SceneCache::from_file_buffer_manager(&fbm);
        cache.save_to_project(tmp.path()).unwrap();
        let loaded = SceneCache::load_from_project(tmp.path()).unwrap();

        assert_eq!(loaded.files.len(), 2);
        assert_eq!(loaded.files[0].file_path, "chapter-1.md");
        assert_eq!(loaded.files[0].scene_map.list_scenes()[0].id, first_scene_id);
        assert_eq!(loaded.files[1].file_path, "chapter-2.md");
        assert_eq!(loaded.files[1].scene_map.list_scenes()[0].id, second_scene_id);
    }
}
