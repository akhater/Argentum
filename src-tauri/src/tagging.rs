use anyhow::Result;
use futures::stream::{self, StreamExt};
use image::{DynamicImage, imageops::FilterType};
use ndarray::{Array, Axis};
use ort::session::Session;
use ort::value::Tensor;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tokenizers::Tokenizer;
use tokio::task::JoinHandle;
use walkdir::WalkDir;

use crate::file_management::{self, parse_virtual_path};
use crate::formats::is_supported_image_file;
use crate::hierarchy::TAG_HIERARCHY;
use crate::image_processing::ImageMetadata;
use crate::{AppState, candidates::TAG_CANDIDATES};

pub const COLOR_TAG_PREFIX: &str = "color:";
pub const USER_TAG_PREFIX: &str = "user:";

fn preprocess_clip_image(image: &DynamicImage) -> Array<f32, ndarray::Dim<[usize; 4]>> {
    let input_size = 224;
    let resized = image.resize_to_fill(input_size, input_size, FilterType::Triangle);
    let rgb_image = resized.to_rgb8();

    let mean = [0.48145466, 0.4578275, 0.40821073];
    let std = [0.26862954, 0.261_302_6, 0.275_777_1];

    let mut array = Array::zeros((1, 3, input_size as usize, input_size as usize));
    for (x, y, pixel) in rgb_image.enumerate_pixels() {
        array[[0, 0, y as usize, x as usize]] = (pixel[0] as f32 / 255.0 - mean[0]) / std[0];
        array[[0, 1, y as usize, x as usize]] = (pixel[1] as f32 / 255.0 - mean[1]) / std[1];
        array[[0, 2, y as usize, x as usize]] = (pixel[2] as f32 / 255.0 - mean[2]) / std[2];
    }
    array
}

fn softmax(array: &Array<f32, ndarray::Dim<[usize; 2]>>) -> Array<f32, ndarray::Dim<[usize; 2]>> {
    let mut new_array = array.clone();
    for mut row in new_array.axis_iter_mut(Axis(0)) {
        let max_val = row.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));
        row.mapv_inplace(|x| (x - max_val).exp());
        let sum = row.sum();
        if sum > 0.0 {
            row.mapv_inplace(|x| x / sum);
        }
    }
    new_array
}

fn rgb_to_hsv((r, g, b): (u8, u8, u8)) -> (f32, f32, f32) {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let h = if delta.abs() < f32::EPSILON {
        0.0
    } else if (max - r).abs() < f32::EPSILON {
        60.0 * (((g - b) / delta) % 6.0)
    } else if (max - g).abs() < f32::EPSILON {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };
    let h = if h < 0.0 { h + 360.0 } else { h };

    let s = if max.abs() < f32::EPSILON {
        0.0
    } else {
        delta / max
    };
    let v = max;

    (h, s, v)
}

pub fn extract_color_tags(image: &DynamicImage) -> Vec<String> {
    let resized = image.resize(100, 100, FilterType::Triangle);
    let rgb_image = resized.to_rgb8();
    let mut color_counts: HashMap<String, u32> = HashMap::new();

    for pixel in rgb_image.pixels() {
        let rgb = (pixel[0], pixel[1], pixel[2]);
        let (h, s, v) = rgb_to_hsv(rgb);

        let color_name = if v < 0.2 {
            "black".to_string()
        } else if s < 0.1 {
            if v > 0.8 {
                "white".to_string()
            } else {
                "gray".to_string()
            }
        } else {
            match h {
                _ if !(20.0..340.0).contains(&h) => "red".to_string(),
                _ if (20.0..45.0).contains(&h) => "orange".to_string(),
                _ if (45.0..70.0).contains(&h) => "yellow".to_string(),
                _ if (70.0..160.0).contains(&h) => "green".to_string(),
                _ if (160.0..260.0).contains(&h) => "blue".to_string(),
                _ if (260.0..340.0).contains(&h) => "purple".to_string(),
                _ => "unknown".to_string(),
            }
        };

        if (color_name == "orange" || color_name == "red") && v < 0.6 && s < 0.7 {
            *color_counts.entry("brown".to_string()).or_insert(0) += 1;
        } else {
            *color_counts.entry(color_name).or_insert(0) += 1;
        }
    }

    let mut colorful_tags: Vec<(String, u32)> = color_counts
        .iter()
        .filter(|(name, _)| !matches!(name.as_str(), "black" | "white" | "gray"))
        .map(|(name, &count)| (name.clone(), count))
        .collect();

    colorful_tags.sort_by_key(|b| std::cmp::Reverse(b.1));

    if !colorful_tags.is_empty() {
        colorful_tags
            .into_iter()
            .take(2)
            .map(|(name, _)| name)
            .collect()
    } else {
        color_counts
            .into_iter()
            .max_by_key(|&(_, count)| count)
            .map(|(name, _)| vec![name])
            .unwrap_or_default()
    }
}

pub fn generate_tags_with_clip(
    image: &DynamicImage,
    clip_session_mutex: &Mutex<Session>,
    tokenizer: &Tokenizer,
    custom_tags: Option<Vec<String>>,
    max_tags: usize,
) -> Result<Vec<String>> {
    let image_input = preprocess_clip_image(image);

    let is_custom = custom_tags.as_ref().map(|t| !t.is_empty()).unwrap_or(false);
    let text_inputs: Vec<String> = if is_custom {
        custom_tags.as_ref().unwrap().clone()
    } else {
        TAG_CANDIDATES.iter().map(|&s| s.to_string()).collect()
    };

    let encodings = tokenizer
        .encode_batch(text_inputs.clone(), true)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let max_len = encodings
        .iter()
        .map(|e| e.get_ids().len())
        .max()
        .unwrap_or(0);

    let mut ids_data = Vec::new();
    let mut mask_data = Vec::new();
    for encoding in encodings {
        let mut ids = encoding
            .get_ids()
            .iter()
            .map(|&i| i as i64)
            .collect::<Vec<_>>();
        let mut mask = encoding
            .get_attention_mask()
            .iter()
            .map(|&m| m as i64)
            .collect::<Vec<_>>();
        ids.resize(max_len, 0);
        mask.resize(max_len, 0);
        ids_data.extend_from_slice(&ids);
        mask_data.extend_from_slice(&mask);
    }

    let ids_array = Array::from_shape_vec((text_inputs.len(), max_len), ids_data)?;
    let mask_array = Array::from_shape_vec((text_inputs.len(), max_len), mask_data)?;

    let image_input_dyn = image_input.into_dyn();
    let ids_array_dyn = ids_array.into_dyn();
    let mask_array_dyn = mask_array.into_dyn();

    let image_layout = image_input_dyn.as_standard_layout();
    let ids_layout = ids_array_dyn.as_standard_layout();
    let mask_layout = mask_array_dyn.as_standard_layout();

    let image_val = Tensor::from_array(image_layout.into_owned())?;
    let ids_val = Tensor::from_array(ids_layout.into_owned())?;
    let mask_val = Tensor::from_array(mask_layout.into_owned())?;

    let mut clip_session = clip_session_mutex.lock().unwrap();
    let outputs = clip_session.run(ort::inputs![ids_val, image_val, mask_val])?;

    let logits_dyn = outputs[0].try_extract_array::<f32>()?.to_owned();
    let logits = logits_dyn.into_dimensionality::<ndarray::Dim<[usize; 2]>>()?;
    let probs = softmax(&logits);

    let confidence_threshold = 0.005;
    let mut scored_tags: Vec<(String, f32)> = Vec::new();

    let prob_row = probs.row(0);
    for (i, &prob) in prob_row.iter().enumerate() {
        if prob > confidence_threshold {
            scored_tags.push((text_inputs[i].clone(), prob));
        }
    }

    scored_tags.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let ranked_tags: Vec<String> = scored_tags.into_iter().map(|(tag, _)| tag).collect();

    let color_tags = if is_custom {
        Vec::new()
    } else {
        extract_color_tags(image)
    };

    Ok(limit_tags(&ranked_tags, !is_custom, color_tags, max_tags))
}

fn limit_tags(
    ranked_tags: &[String],
    add_parents: bool,
    color_tags: Vec<String>,
    max_tags: usize,
) -> Vec<String> {
    let mut final_tags: Vec<String> = Vec::new();
    let mut add_tag = |tag: &str| {
        if final_tags.len() < max_tags && !final_tags.iter().any(|t| t == tag) {
            final_tags.push(tag.to_string());
        }
    };

    for tag in ranked_tags {
        add_tag(tag);
        if add_parents && let Some(parents) = TAG_HIERARCHY.get(tag.as_str()) {
            for parent in parents {
                add_tag(parent);
            }
        }
    }
    for color_tag in &color_tags {
        add_tag(color_tag);
    }

    final_tags
}

#[tauri::command]
pub async fn start_background_indexing(
    folder_path: String,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if let Some(handle) = state.indexing_task_handle.lock().unwrap().take() {
        println!("Cancelling previous indexing task.");
        handle.abort();
    }

    let settings = crate::load_settings(app_handle.clone())?;
    if !settings.enable_ai_tagging.unwrap_or(false) {
        return Ok(());
    }

    let max_concurrent_tasks = settings.tagging_thread_count.unwrap_or(3).max(1) as usize;
    let custom_ai_tags = settings.custom_ai_tags.clone();
    let ai_tag_count = settings.ai_tag_count.unwrap_or(10) as usize;

    let clip_models = crate::ai_processing::get_or_init_clip_models(
        &app_handle,
        &state.ai_state,
        &state.ai_init_lock,
    )
    .await
    .map_err(|e| e.to_string())?;

    let app_handle_clone = app_handle.clone();

    let task: JoinHandle<()> = tokio::spawn(async move {
        let _ = app_handle_clone.emit("indexing-started", ());
        println!("Starting background indexing for: {}", folder_path);
        println!(
            "Using {} concurrent threads for AI tagging.",
            max_concurrent_tasks
        );

        let state_clone = app_handle_clone.state::<AppState>();
        let gpu_context =
            crate::gpu_processing::get_or_init_gpu_context(&state_clone, &app_handle).ok();

        let image_paths: Vec<PathBuf> = match fs::read_dir(&folder_path) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.is_file() && is_supported_image_file(path.to_string_lossy().as_ref())
                })
                .collect(),
            Err(e) => {
                eprintln!("Failed to read directory '{}': {}", folder_path, e);
                let _ = app_handle_clone
                    .emit("indexing-error", format!("Failed to read directory: {}", e));
                *app_handle_clone
                    .state::<AppState>()
                    .indexing_task_handle
                    .lock()
                    .unwrap() = None;
                return;
            }
        };

        println!(
            "Found {} images to process in {}",
            image_paths.len(),
            folder_path
        );
        let total_images = image_paths.len();
        let processed_count = Arc::new(Mutex::new(0));
        let custom_ai_tags_shared = Arc::new(custom_ai_tags);

        stream::iter(image_paths)
            .for_each_concurrent(max_concurrent_tasks, |path| {
                let app_handle_inner = app_handle_clone.clone();
                let clip_models_inner = clip_models.clone();
                let gpu_context_inner = gpu_context.clone();
                let processed_count_inner = Arc::clone(&processed_count);
                let tags_inner = Arc::clone(&custom_ai_tags_shared);

                async move {
                    let path_str = path.to_string_lossy().to_string();
                    let (source_path, sidecar_path) = parse_virtual_path(&path_str);

                    let metadata = crate::exif_processing::load_sidecar(&sidecar_path);

                    let should_generate_tags = match &metadata.tags {
                        None => true,
                        Some(tags) => !tags.iter().any(|tag| {
                            !tag.starts_with(COLOR_TAG_PREFIX) && !tag.starts_with(USER_TAG_PREFIX)
                        }),
                    };

                    if should_generate_tags {
                        match file_management::get_cached_or_generate_thumbnail_image(
                            &path_str,
                            &app_handle_inner,
                            gpu_context_inner.as_ref(),
                        ) {
                            Ok(image) => {
                                if let Ok(ai_tags) = generate_tags_with_clip(
                                    &image,
                                    &clip_models_inner.model,
                                    &clip_models_inner.tokenizer,
                                    (*tags_inner).clone(),
                                    ai_tag_count,
                                ) {
                                    println!("Found AI tags for {}: {:?}", path_str, ai_tags);

                                    // Argentum: added to the sidecar as it is now,
                                    // which may have changed while the tags were
                                    // found (mods/write_locks.rs).
                                    let _ = crate::mods::write_locks::update_sidecar(
                                        &sidecar_path,
                                        &source_path,
                                        |metadata| {
                                            let mut existing_tags: HashSet<String> = metadata
                                                .tags
                                                .take()
                                                .unwrap_or_default()
                                                .into_iter()
                                                .collect();
                                            existing_tags.extend(ai_tags);
                                            let mut final_tags: Vec<String> =
                                                existing_tags.into_iter().collect();
                                            final_tags.sort_unstable();
                                            metadata.tags = Some(final_tags);
                                        },
                                    );
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "Could not get or generate image for tagging {}: {}",
                                    path_str, e
                                );
                            }
                        }
                    }

                    let mut count = processed_count_inner.lock().unwrap();
                    *count += 1;
                    let _ = app_handle_inner.emit(
                        "indexing-progress",
                        serde_json::json!({
                            "current": *count,
                            "total": total_images
                        }),
                    );
                }
            })
            .await;

        println!("Background indexing finished for: {}", folder_path);
        let _ = app_handle_clone.emit("indexing-finished", ());

        *app_handle_clone
            .state::<AppState>()
            .indexing_task_handle
            .lock()
            .unwrap() = None;
    });

    *state.indexing_task_handle.lock().unwrap() = Some(task);

    Ok(())
}

fn modify_tags_for_path(
    path_str: &str,
    app_handle: &AppHandle,
    modify_fn: impl Fn(&mut Vec<String>),
) -> Result<(), String> {
    let (source_path, sidecar_path) = parse_virtual_path(path_str);

    let sidecar_lock = crate::mods::write_locks::lock_sidecar(&sidecar_path);
    let mut metadata =
        crate::exif_processing::try_load_sidecar(&sidecar_path).map_err(|e| e.to_string())?;

    let mut tags = metadata.tags.unwrap_or_default();
    modify_fn(&mut tags);

    tags.sort_unstable();
    tags.dedup();

    if tags.is_empty() {
        metadata.tags = None;
    } else {
        metadata.tags = Some(tags);
    }

    crate::exif_processing::write_sidecar(&sidecar_path, &source_path, &metadata)
        .map_err(|e| e.to_string())?;
    drop(sidecar_lock);

    if let Ok(settings) = crate::load_settings(app_handle.clone())
        && settings.enable_xmp_sync.unwrap_or(false)
    {
        let create_if_missing = settings.create_xmp_if_missing.unwrap_or(false);
        file_management::sync_metadata_to_xmp(
            &source_path,
            &sidecar_path,
            &metadata,
            create_if_missing,
        );
    }

    Ok(())
}

#[tauri::command]
pub fn add_tag_for_paths(
    paths: Vec<String>,
    tag: String,
    app_handle: AppHandle,
) -> Result<(), String> {
    paths.par_iter().for_each(|path| {
        let tag_clone = tag.clone();
        if let Err(e) = modify_tags_for_path(path, &app_handle, |tags| {
            if !tags.contains(&tag_clone) {
                tags.push(tag_clone.clone());
            }
        }) {
            eprintln!("Failed to add tag to {}: {}", path, e);
        }
    });
    Ok(())
}

#[tauri::command]
pub fn remove_tag_for_paths(
    paths: Vec<String>,
    tag: String,
    app_handle: AppHandle,
) -> Result<(), String> {
    paths.par_iter().for_each(|path| {
        let tag_clone = tag.clone();
        if let Err(e) = modify_tags_for_path(path, &app_handle, |tags| {
            tags.retain(|t| t != &tag_clone);
        }) {
            eprintln!("Failed to remove tag from {}: {}", path, e);
        }
    });
    Ok(())
}

/// Removes, from every sidecar under `folder`, the tags `keep` does not keep:
/// a photo's own sidecar and each virtual copy's. Each change goes through the
/// sidecar's lock and an atomic write (`mods/write_locks.rs`); a sidecar that
/// cannot be read is left as it is and logged; a photo with no sidecar gets
/// none. `sync_xmp` runs after each saved change, as the tag commands do.
/// Gives the number of sidecars changed.
fn clear_tags_under(
    folder: &Path,
    keep: impl Fn(&str) -> bool,
    sync_xmp: impl Fn(&Path, &Path, &ImageMetadata),
) -> usize {
    let mut changed = 0;
    for entry in WalkDir::new(folder).into_iter().filter_map(|e| e.ok()) {
        let sidecar = entry.path();
        if !sidecar.is_file() {
            continue;
        }
        let Some(name) = sidecar.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some((source_name, copy_id)) = file_management::parse_sidecar_name(name) else {
            continue;
        };
        let source = sidecar.with_file_name(source_name);
        if file_management::sidecar_for(&source, copy_id) != sidecar {
            continue;
        }

        // Peek first: a sidecar with nothing to clear is never rewritten, and
        // peeking never writes (no .bak copies). One that cannot be read is
        // skipped here and left as it is.
        let has_tags_to_clear = crate::exif_processing::peek_sidecar(sidecar)
            .and_then(|metadata| metadata.tags)
            .is_some_and(|tags| tags.iter().any(|tag| !keep(tag.as_str())));
        if !has_tags_to_clear {
            continue;
        }

        let saved = crate::mods::write_locks::update_sidecar(sidecar, &source, |metadata| {
            let kept: Vec<String> = metadata
                .tags
                .take()
                .unwrap_or_default()
                .into_iter()
                .filter(|tag| keep(tag.as_str()))
                .collect();
            metadata.tags = (!kept.is_empty()).then_some(kept);
        });
        match saved {
            Ok(metadata) => {
                sync_xmp(&source, sidecar, &metadata);
                changed += 1;
            }
            Err(e) => {
                log::warn!("Could not clear the tags of {}: {e}", sidecar.display());
            }
        }
    }
    changed
}

/// Clears the tags `keep` does not keep under the folder `root_path`, on a
/// blocking thread: the walk reads every sidecar, and a cleared one that becomes
/// empty is removed, which reads the photo's EXIF.
async fn clear_tags_in_folder(
    root_path: String,
    app_handle: AppHandle,
    keep: fn(&str) -> bool,
) -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<usize, String> {
        let folder = crate::mods::clear_scope::folder_to_clear(&root_path)?;

        let settings = crate::load_settings(app_handle.clone()).unwrap_or_default();
        let enable_xmp_sync = settings.enable_xmp_sync.unwrap_or(false);
        let create_xmp_if_missing = settings.create_xmp_if_missing.unwrap_or(false);

        Ok(clear_tags_under(folder, keep, |source, sidecar, metadata| {
            if enable_xmp_sync {
                file_management::sync_metadata_to_xmp(
                    source,
                    sidecar,
                    metadata,
                    create_xmp_if_missing,
                );
            }
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Keeps color tags and user tags, removes the others (AI tags).
#[tauri::command]
pub async fn clear_ai_tags(root_path: String, app_handle: AppHandle) -> Result<usize, String> {
    clear_tags_in_folder(root_path, app_handle, |tag| {
        tag.starts_with(COLOR_TAG_PREFIX) || tag.starts_with(USER_TAG_PREFIX)
    })
    .await
}

/// Keeps only color tags, removes AI and user tags.
#[tauri::command]
pub async fn clear_all_tags(root_path: String, app_handle: AppHandle) -> Result<usize, String> {
    clear_tags_in_folder(root_path, app_handle, |tag| {
        tag.starts_with(COLOR_TAG_PREFIX)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exif_processing::{load_sidecar, write_sidecar};
    use crate::file_management::sidecar_for;
    use crate::mods::write_locks::update_sidecar;
    use serde_json::json;
    use std::cell::Cell;

    fn keep_color_and_user(tag: &str) -> bool {
        tag.starts_with(COLOR_TAG_PREFIX) || tag.starts_with(USER_TAG_PREFIX)
    }

    fn no_xmp(_: &Path, _: &Path, _: &ImageMetadata) {}

    fn edited_with_tags(tags: &[&str]) -> ImageMetadata {
        ImageMetadata {
            rating: 4,
            adjustments: json!({ "exposure": 0.5, "contrast": 12 }),
            tags: Some(tags.iter().map(|tag| tag.to_string()).collect()),
            ..ImageMetadata::default()
        }
    }

    fn photo(dir: &Path, name: &str) -> PathBuf {
        let photo = dir.join(name);
        std::fs::write(&photo, "raw").unwrap();
        photo
    }

    #[test]
    fn clearing_ai_tags_of_a_current_sidecar_keeps_the_edit_and_everything_else() {
        let dir = tempfile::tempdir().unwrap();
        let photo = photo(dir.path(), "IMG_0001.CR2");
        let sidecar = sidecar_for(&photo, None);
        write_sidecar(
            &sidecar,
            &photo,
            &edited_with_tags(&["color:red", "user:cat", "ai:dog"]),
        )
        .unwrap();
        let mut expected = serde_json::to_value(load_sidecar(&sidecar)).unwrap();

        assert_eq!(clear_tags_under(dir.path(), keep_color_and_user, no_xmp), 1);

        expected["tags"] = json!(["color:red", "user:cat"]);
        let after = serde_json::to_value(load_sidecar(&sidecar)).unwrap();
        assert_eq!(after, expected);
    }

    #[test]
    fn clearing_tags_removes_a_virtual_copys_ai_tags_too() {
        let dir = tempfile::tempdir().unwrap();
        let photo = photo(dir.path(), "IMG_0002.CR2");
        let copy = sidecar_for(&photo, Some("a1b2c3"));
        write_sidecar(&copy, &photo, &edited_with_tags(&["ai:dog"])).unwrap();

        assert_eq!(clear_tags_under(dir.path(), keep_color_and_user, no_xmp), 1);

        let after = load_sidecar(&copy);
        assert_eq!(after.tags, None);
        assert_eq!(after.rating, 4);
    }

    #[test]
    fn a_photo_with_no_sidecar_gets_none() {
        let dir = tempfile::tempdir().unwrap();
        let photo = photo(dir.path(), "IMG_0003.CR2");

        assert_eq!(clear_tags_under(dir.path(), keep_color_and_user, no_xmp), 0);

        assert!(!sidecar_for(&photo, None).exists());
    }

    #[test]
    fn a_sidecar_that_cannot_be_read_is_left_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let photo = photo(dir.path(), "IMG_0004.CR2");
        let sidecar = sidecar_for(&photo, None);
        write_sidecar(&sidecar, &photo, &edited_with_tags(&["ai:dog"])).unwrap();
        let bytes = std::fs::read(&sidecar).unwrap();
        let _failing = crate::mods::sidecar_read::injected::fail_reads_of(&sidecar);

        assert_eq!(clear_tags_under(dir.path(), keep_color_and_user, no_xmp), 0);

        assert_eq!(std::fs::read(&sidecar).unwrap(), bytes);
    }

    #[test]
    fn the_xmp_is_synced_for_each_sidecar_whose_tags_were_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let tagged = photo(dir.path(), "IMG_0005.CR2");
        let plain = photo(dir.path(), "IMG_0006.CR2");
        write_sidecar(
            &sidecar_for(&tagged, None),
            &tagged,
            &edited_with_tags(&["ai:dog"]),
        )
        .unwrap();
        write_sidecar(
            &sidecar_for(&plain, None),
            &plain,
            &edited_with_tags(&["user:cat"]),
        )
        .unwrap();
        let synced = Cell::new(0);

        let cleared = clear_tags_under(dir.path(), keep_color_and_user, |source, sidecar, _| {
            assert_eq!(source, tagged.as_path());
            assert_eq!(sidecar, sidecar_for(&tagged, None).as_path());
            synced.set(synced.get() + 1);
        });

        assert_eq!(cleared, 1);
        assert_eq!(synced.get(), 1);
    }

    #[test]
    fn clearing_tags_while_an_edit_is_saved_loses_neither_change() {
        let dir = tempfile::tempdir().unwrap();
        let photo = photo(dir.path(), "IMG_0007.CR2");
        let sidecar = sidecar_for(&photo, None);
        write_sidecar(&sidecar, &photo, &edited_with_tags(&[])).unwrap();
        const LAST: u32 = 300;

        std::thread::scope(|scope| {
            scope.spawn(|| {
                for i in 0..=LAST {
                    update_sidecar(&sidecar, &photo, |m| {
                        m.adjustments = json!({ "exposure": i });
                    })
                    .unwrap();
                }
            });
            scope.spawn(|| {
                for _ in 0..=LAST {
                    update_sidecar(&sidecar, &photo, |m| {
                        m.tags
                            .get_or_insert_with(Vec::new)
                            .push("ai:dog".to_string());
                    })
                    .unwrap();
                    clear_tags_under(dir.path(), keep_color_and_user, no_xmp);
                }
            });
        });

        let kept = load_sidecar(&sidecar);
        assert_eq!(kept.adjustments["exposure"], json!(LAST));
        assert_eq!(kept.rating, 4);
        assert!(
            kept.tags
                .unwrap_or_default()
                .iter()
                .all(|tag| !tag.starts_with("ai:"))
        );
    }
}
