use std::path::Path;

use crate::concepts::file_buffer_manager::FileEntry;
use crate::concepts::scene_map::{ParseMode, SceneMap};
use crate::concepts::text_buffer::ByteRange;
use crate::runtime::story_access::StoryAccess;

use super::{SkillContext, Skills};

impl Skills {
    pub(super) fn exec_write_to_canvas(
        &self,
        args: &serde_json::Value,
        ctx: &mut SkillContext<'_>,
    ) -> serde_json::Value {
        let position = match args["position"].as_u64() {
            Some(p) => p as usize,
            None => return serde_json::json!({ "error": "Missing required field: position" }),
        };
        let text = match args["text"].as_str() {
            Some(t) => t,
            None => return serde_json::json!({ "error": "Missing required field: text" }),
        };

        let requested_file = match optional_file_arg(args) {
            Ok(file) => file,
            Err(err) => return err,
        };

        match resolve_canvas_target(ctx, requested_file) {
            Ok(CanvasTarget::Primary) => match ctx.text_buffer.insert(position, text) {
                Ok(()) => {
                    let full_text = ctx.text_buffer.read_all();
                    ctx.scene_map.full_reindex(&full_text, "");
                    serde_json::json!({
                        "status": "written",
                        "position": position,
                        "bytes_written": text.len(),
                        "file_path": ctx.text_buffer.file_path().to_string_lossy(),
                    })
                }
                Err(e) => serde_json::json!({ "error": e.to_string() }),
            },
            Ok(CanvasTarget::Manifest(file_path)) => {
                apply_manifest_edit(ctx, &file_path, |entry| {
                    entry.text_buffer.insert(position, text)
                })
                .map(|(_, scene_count)| {
                    serde_json::json!({
                        "status": "written",
                        "position": position,
                        "bytes_written": text.len(),
                        "file_path": file_path,
                        "scene_count": scene_count,
                    })
                })
                .unwrap_or_else(|err| serde_json::json!({ "error": err }))
            }
            Err(err) => err,
        }
    }

    pub(super) fn exec_replace_in_canvas(
        &self,
        args: &serde_json::Value,
        ctx: &mut SkillContext<'_>,
    ) -> serde_json::Value {
        let start = match args["start"].as_u64() {
            Some(s) => s as usize,
            None => return serde_json::json!({ "error": "Missing required field: start" }),
        };
        let end = match args["end"].as_u64() {
            Some(e) => e as usize,
            None => return serde_json::json!({ "error": "Missing required field: end" }),
        };
        let text = match args["text"].as_str() {
            Some(t) => t,
            None => return serde_json::json!({ "error": "Missing required field: text" }),
        };

        let requested_file = match optional_file_arg(args) {
            Ok(file) => file,
            Err(err) => return err,
        };
        let range = ByteRange::new(start, end);

        match resolve_canvas_target(ctx, requested_file) {
            Ok(CanvasTarget::Primary) => match ctx.text_buffer.replace(range, text) {
                Ok(()) => {
                    let full_text = ctx.text_buffer.read_all();
                    ctx.scene_map.full_reindex(&full_text, "");
                    serde_json::json!({
                        "status": "replaced",
                        "start": start,
                        "end": end,
                        "new_length": text.len(),
                        "file_path": ctx.text_buffer.file_path().to_string_lossy(),
                    })
                }
                Err(e) => serde_json::json!({ "error": e.to_string() }),
            },
            Ok(CanvasTarget::Manifest(file_path)) => {
                apply_manifest_edit(ctx, &file_path, |entry| {
                    entry.text_buffer.replace(range, text)
                })
                .map(|(_, scene_count)| {
                    serde_json::json!({
                        "status": "replaced",
                        "start": start,
                        "end": end,
                        "new_length": text.len(),
                        "file_path": file_path,
                        "scene_count": scene_count,
                    })
                })
                .unwrap_or_else(|err| serde_json::json!({ "error": err }))
            }
            Err(err) => err,
        }
    }

    pub(super) fn exec_insert_scene(
        &self,
        args: &serde_json::Value,
        ctx: &mut SkillContext<'_>,
    ) -> serde_json::Value {
        let title = match args["title"].as_str() {
            Some(t) => t,
            None => return serde_json::json!({ "error": "Missing required field: title" }),
        };
        let content = args["content"].as_str().unwrap_or("");
        let scene_text = format!("\n\n---\n\n## {title}\n\n{content}");

        let requested_file = match optional_file_arg(args) {
            Ok(file) => file,
            Err(err) => return err,
        };
        let (target, position) = match resolve_insert_target(args, ctx, requested_file) {
            Ok(result) => result,
            Err(err) => return err,
        };

        match target {
            CanvasTarget::Primary => match ctx.text_buffer.insert(position, &scene_text) {
                Ok(()) => {
                    let full_text = ctx.text_buffer.read_all();
                    ctx.scene_map.full_reindex(&full_text, "");
                    serde_json::json!({
                        "status": "inserted",
                        "position": position,
                        "title": title,
                        "bytes_written": scene_text.len(),
                        "new_scene_count": ctx.scene_map.scene_count(),
                        "file_path": ctx.text_buffer.file_path().to_string_lossy(),
                    })
                }
                Err(e) => serde_json::json!({ "error": e.to_string() }),
            },
            CanvasTarget::Manifest(file_path) => apply_manifest_edit(ctx, &file_path, |entry| {
                entry.text_buffer.insert(position, &scene_text)
            })
            .map(|(_, scene_count)| {
                serde_json::json!({
                    "status": "inserted",
                    "position": position,
                    "title": title,
                    "bytes_written": scene_text.len(),
                    "new_scene_count": scene_count,
                    "file_path": file_path,
                })
            })
            .unwrap_or_else(|err| serde_json::json!({ "error": err })),
        }
    }
}

enum CanvasTarget {
    Primary,
    Manifest(String),
}

fn optional_file_arg(args: &serde_json::Value) -> Result<Option<&str>, serde_json::Value> {
    match args.get("file") {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(path)) => Ok(Some(path.as_str())),
        Some(_) => {
            Err(serde_json::json!({ "error": "Field 'file' must be a string when provided" }))
        }
    }
}

fn resolve_canvas_target(
    ctx: &SkillContext<'_>,
    requested_file: Option<&str>,
) -> Result<CanvasTarget, serde_json::Value> {
    if let Some(fbm) = ctx.file_buffer_manager.as_deref() {
        if let Some(file) = requested_file {
            if fbm.get_entry(file).is_some() {
                // Check editability via manifest
                if let Some(manifest) = ctx.manifest {
                    if let Some(sf) = manifest.story_files.iter().find(|sf| sf.path == file) {
                        if !sf.editable {
                            return Err(serde_json::json!({
                                "error": format!(
                                    "Cannot edit '{}' — it is a read-only file ({}). \
                                     Use Consultant mode for revision suggestions, \
                                     or convert to .md with `laires convert`.",
                                    file, sf.format
                                )
                            }));
                        }
                    }
                }
                return Ok(CanvasTarget::Manifest(file.to_string()));
            }
            return Err(unknown_manifest_file_error(fbm, file));
        }

        if fbm.story_file_count() == 1 {
            let file_path = &fbm.entries()[0].file_path;
            // Check editability for single-file projects too
            if let Some(manifest) = ctx.manifest {
                if let Some(sf) = manifest.story_files.iter().find(|sf| &sf.path == file_path) {
                    if !sf.editable {
                        return Err(serde_json::json!({
                            "error": format!(
                                "Cannot edit '{}' — it is a read-only file ({}). \
                                 Use Consultant mode for revision suggestions, \
                                 or convert to .md with `laires convert`.",
                                file_path, sf.format
                            )
                        }));
                    }
                }
            }
            return Ok(CanvasTarget::Manifest(file_path.clone()));
        }

        return Err(serde_json::json!({
            "error": "Canvas write tools require a 'file' field when a manifest has multiple story files",
            "available_files": fbm.entries().iter().map(|entry| entry.file_path.clone()).collect::<Vec<_>>(),
        }));
    }

    if let Some(file) = requested_file {
        let current = ctx.text_buffer.file_path();
        let current_name = current.file_name().and_then(|name| name.to_str());
        let requested_name = Path::new(file).file_name().and_then(|name| name.to_str());
        if Some(file) != current.to_str() && requested_name != current_name {
            return Err(serde_json::json!({
                "error": format!(
                    "Requested file '{}' does not match the loaded story file '{}'",
                    file,
                    current.display()
                )
            }));
        }
    }

    Ok(CanvasTarget::Primary)
}

fn resolve_insert_target(
    args: &serde_json::Value,
    ctx: &SkillContext<'_>,
    requested_file: Option<&str>,
) -> Result<(CanvasTarget, usize), serde_json::Value> {
    if let Some(scene_ref) = args.get("after_scene").and_then(|value| value.as_str()) {
        let story = StoryAccess::new(
            &*ctx.text_buffer,
            &*ctx.scene_map,
            ctx.file_buffer_manager.as_deref(),
        );
        let scene = story.read_scene(scene_ref).ok_or_else(|| {
            serde_json::json!({
                "error": format!("Scene '{}' not found", scene_ref)
            })
        })?;

        if let Some(file) = requested_file {
            if !scene.file_path.is_empty() && file != scene.file_path {
                return Err(serde_json::json!({
                    "error": format!(
                        "Scene '{}' belongs to '{}', not '{}'",
                        scene_ref,
                        scene.file_path,
                        file
                    )
                }));
            }
        }

        if let Some(fbm) = ctx.file_buffer_manager.as_deref() {
            let entry = fbm.get_entry(&scene.file_path).ok_or_else(|| {
                serde_json::json!({
                    "error": format!("Scene '{}' resolved to unknown file '{}'", scene_ref, scene.file_path)
                })
            })?;
            let scene_span = entry.scene_map.get_scene(&scene.id).ok_or_else(|| {
                serde_json::json!({
                    "error": format!("Scene '{}' no longer exists in '{}'", scene_ref, scene.file_path)
                })
            })?;
            return Ok((CanvasTarget::Manifest(scene.file_path), scene_span.end));
        }

        let scene_span = ctx.scene_map.get_scene(&scene.id).ok_or_else(|| {
            serde_json::json!({
                "error": format!("Scene '{}' no longer exists in the current story", scene_ref)
            })
        })?;
        return Ok((CanvasTarget::Primary, scene_span.end));
    }

    let position = match args["position"].as_u64() {
        Some(p) => p as usize,
        None => {
            return Err(serde_json::json!({
                "error": "Missing required field: position (or provide after_scene)"
            }));
        }
    };

    resolve_canvas_target(ctx, requested_file).map(|target| (target, position))
}

fn apply_manifest_edit(
    ctx: &mut SkillContext<'_>,
    file_path: &str,
    edit: impl FnOnce(&mut FileEntry) -> crate::error::Result<()>,
) -> Result<(String, usize), String> {
    let sync_primary = {
        let fbm = ctx.file_buffer_manager.as_deref().ok_or_else(|| {
            "Manifest-backed edit requested without a file buffer manager".to_string()
        })?;
        let entry = fbm
            .get_entry(file_path)
            .ok_or_else(|| format!("Story file '{}' not found in manifest", file_path))?;
        entry.text_buffer.file_path() == ctx.text_buffer.file_path()
    };

    let (updated_text, updated_scene_map, scene_count) = {
        let fbm = ctx.file_buffer_manager.as_deref_mut().ok_or_else(|| {
            "Manifest-backed edit requested without a file buffer manager".to_string()
        })?;
        let entry = fbm
            .get_entry_mut(file_path)
            .ok_or_else(|| format!("Story file '{}' not found in manifest", file_path))?;
        edit(entry).map_err(|e| e.to_string())?;
        reindex_entry(entry);
        (
            entry.text_buffer.read_all(),
            entry.scene_map.clone(),
            entry.scene_map.scene_count(),
        )
    };

    if sync_primary {
        let current_len = ctx.text_buffer.read_all().len();
        ctx.text_buffer
            .replace(ByteRange::new(0, current_len), &updated_text)
            .map_err(|e| e.to_string())?;
        *ctx.scene_map = updated_scene_map;
    }

    Ok((file_path.to_string(), scene_count))
}

fn reindex_entry(entry: &mut FileEntry) {
    let entry_text = entry.text_buffer.read_all();
    let parse_mode = if entry.format == "fountain" {
        ParseMode::Fountain
    } else {
        ParseMode::Prose
    };
    let mut scene_map = SceneMap::new(parse_mode);
    scene_map.full_reindex(&entry_text, &entry.file_path);
    entry.scene_map = scene_map;
}

fn unknown_manifest_file_error(
    fbm: &crate::concepts::file_buffer_manager::FileBufferManager,
    file: &str,
) -> serde_json::Value {
    serde_json::json!({
        "error": format!("Story file '{}' not found in manifest", file),
        "available_files": fbm.entries().iter().map(|entry| entry.file_path.clone()).collect::<Vec<_>>(),
    })
}
