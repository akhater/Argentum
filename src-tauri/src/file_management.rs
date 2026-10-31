use memmap2::{Mmap, MmapOptions};
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;

use anyhow::Result;
use chrono::{DateTime, Utc};
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, GenericImageView, ImageBuffer, Luma};
use rayon::prelude::*;
use regex::{NoExpand, regex};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sysinfo::Disks;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;
use walkdir::WalkDir;

use crate::AppState;
use crate::PendingMetadata;
use crate::app_settings::*;
use crate::as_shot::as_shot;
use crate::exif_processing;
use crate::formats::{is_raw_file, is_supported_image_file};
use crate::gpu_processing;
use crate::image_loader;
use crate::image_processing::GpuContext;
use crate::image_processing::{
    Crop, ImageFlag, ImageMetadata, apply_coarse_rotation, apply_cpu_default_raw_processing,
    apply_crop, apply_flip, apply_geometry_warp, apply_rotation,
    auto_results_to_json, default_adjustments,
    get_all_adjustments_from_json, perform_auto_analysis, resolve_render_options,
};
use crate::mask_generation::MaskDefinition;
use crate::mods::edit_status::EditStatus;
use crate::mods::file_ops::{self, Destinations};
use crate::preset_converter;
use crate::tagging::COLOR_TAG_PREFIX;

fn emit_thumbnail_cache_setup_error(app_handle: &AppHandle, path: &str, reason: &str) {
    let _ = app_handle.emit(
        "thumbnail-generation-error",
        serde_json::json!({ "path": path, "reason": reason }),
    );
}

fn compute_thumbnail_cache_hash(path_str: &str, adjustments_bytes: &[u8]) -> Option<String> {
    let (source_path, _) = parse_virtual_path(path_str);

    let img_mod_time = fs::metadata(&source_path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();

    let mut hasher = blake3::Hasher::new();
    hasher.update(path_str.as_bytes());
    hasher.update(&img_mod_time.to_le_bytes());
    hasher.update(adjustments_bytes);
    Some(hasher.finalize().to_hex().to_string())
}

struct ImageFileMetadata {
    edit: EditStatus,
    tags: Option<Vec<String>>,
    rating: u8,
    flag: Option<ImageFlag>,
    is_raw: bool,
}

fn resolve_image_metadata(
    image_path: &Path,
    sidecar_path: &Path,
    enable_xmp_sync: bool,
    settings: &AppSettings,
) -> ImageFileMetadata {
    let sidecar_existed = sidecar_path.exists();
    let mut metadata = crate::exif_processing::load_sidecar(sidecar_path);

    if enable_xmp_sync && sync_metadata_from_xmp(image_path, &mut metadata, !sidecar_existed) {
        let _ = crate::exif_processing::write_sidecar(sidecar_path, image_path, &metadata);
    }

    let is_raw = crate::formats::is_raw_file(image_path);
    let tm_override = crate::image_processing::resolve_tonemapper_override(settings, is_raw);
    ImageFileMetadata {
        edit: EditStatus::of(&metadata.adjustments, is_raw, tm_override),
        tags: metadata.tags,
        rating: metadata.rating,
        flag: metadata.flag,
        is_raw,
    }
}

fn emit_image_metadata_loaded(
    app_handle: &AppHandle,
    path: &str,
    rating: u8,
    flag: Option<ImageFlag>,
    edit: EditStatus,
    tags: &Option<Vec<String>>,
) {
    let _ = app_handle.emit(
        "image-metadata-loaded",
        serde_json::json!({
            "path": path,
            "rating": rating,
            "flag": flag,
            "is_edited": edit.is_edited,
            "has_provider_look": edit.has_provider_look,
            "tags": tags
        }),
    );
}

fn enqueue_metadata(
    app_handle: &AppHandle,
    virtual_path: String,
    image_path: PathBuf,
    sidecar_path: PathBuf,
) {
    let state = app_handle.state::<crate::AppState>();
    let manager = &state.metadata_manager;

    let mut pending = manager.pending.lock().unwrap();
    if !pending.insert(sidecar_path.clone()) {
        return;
    }
    drop(pending);

    manager.queue.lock().unwrap().push_back(PendingMetadata {
        virtual_path,
        image_path,
        sidecar_path,
    });
    manager.cvar.notify_one();
}

// Not compute-heavy — these threads mostly block waiting on iCloud to
// materialize a file, not burning CPU — so a small fixed pool is enough and
// doesn't need a user-facing setting the way thumbnail_worker_threads does.
const METADATA_WORKER_THREADS: usize = 4;

pub fn start_metadata_workers(app_handle: tauri::AppHandle) {
    let state = app_handle.state::<crate::AppState>();
    let manager = state.metadata_manager.clone();

    for _ in 0..METADATA_WORKER_THREADS {
        let app_clone = app_handle.clone();
        let manager_clone = manager.clone();

        std::thread::spawn(move || {
            loop {
                let item = {
                    let mut queue = manager_clone.queue.lock().unwrap();
                    while queue.is_empty() {
                        queue = manager_clone.cvar.wait(queue).unwrap();
                    }
                    queue.pop_front().unwrap()
                };

                let settings = load_settings(app_clone.clone()).unwrap_or_default();
                let enable_xmp_sync = settings.enable_xmp_sync.unwrap_or(false);

                let metadata = resolve_image_metadata(
                    &item.image_path,
                    &item.sidecar_path,
                    enable_xmp_sync,
                    &settings,
                );

                emit_image_metadata_loaded(
                    &app_clone,
                    &item.virtual_path,
                    metadata.rating,
                    metadata.flag,
                    metadata.edit,
                    &metadata.tags,
                );

                manager_clone
                    .pending
                    .lock()
                    .unwrap()
                    .remove(&item.sidecar_path);
            }
        });
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub adjustments: Value,
    #[serde(rename = "includeMasks", skip_serializing_if = "Option::is_none")]
    pub include_masks: Option<bool>,
    #[serde(
        rename = "includeCropTransform",
        skip_serializing_if = "Option::is_none"
    )]
    pub include_crop_transform: Option<bool>,
    #[serde(rename = "presetType", skip_serializing_if = "Option::is_none")]
    pub preset_type: Option<String>,
}

#[derive(Serialize)]
struct ExportPresetFile<'a> {
    creator: &'a str,
    presets: &'a [PresetItem],
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PresetFolder {
    pub id: String,
    pub name: String,
    pub children: Vec<Preset>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum PresetItem {
    Preset(Preset),
    Folder(PresetFolder),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PresetFile {
    pub presets: Vec<PresetItem>,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PresetImportFailure {
    pub file_name: String,
    pub error: String,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PresetImportResult {
    pub presets: Vec<PresetItem>,
    pub failures: Vec<PresetImportFailure>,
}

#[derive(Debug)]
pub enum ReadFileError {
    Io(std::io::Error),
    Locked,
    Empty,
    NotFound,
    Invalid,
}

impl fmt::Display for ReadFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadFileError::Io(err) => write!(f, "IO error: {}", err),
            ReadFileError::Locked => write!(f, "File is locked"),
            ReadFileError::Empty => write!(f, "File is empty"),
            ReadFileError::NotFound => write!(f, "File not found"),
            ReadFileError::Invalid => write!(f, "Invalid file"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImageFile {
    pub path: String,
    modified: u64,
    #[serde(flatten)]
    edit: EditStatus,
    rating: u8,
    flag: Option<ImageFlag>,
    tags: Option<Vec<String>>,
    exif: Option<HashMap<String, String>>,
    is_virtual_copy: bool,
    is_cloud_placeholder: bool,
    is_raw: bool,
    group_id: Option<String>,
}

fn make_group_key(source_path: &Path) -> String {
    let parent = source_path.parent().unwrap_or(Path::new(""));
    let stem = source_path.file_stem().unwrap_or_default();
    format!("{}/{}", parent.to_string_lossy(), stem.to_string_lossy())
}

fn assign_group_ids(files: &mut [ImageFile]) {
    #[derive(Clone)]
    struct Candidate {
        index: usize,
        key: String,
    }

    let candidates: Vec<Candidate> = files
        .iter()
        .enumerate()
        .filter(|(_, file)| !file.is_virtual_copy)
        .map(|(index, file)| {
            let (source_path, _) = parse_virtual_path(&file.path);
            let key = make_group_key(&source_path);
            Candidate { index, key }
        })
        .collect();

    let mut stem_groups: HashMap<String, Vec<Candidate>> = HashMap::new();
    for candidate in candidates {
        stem_groups
            .entry(candidate.key.clone())
            .or_default()
            .push(candidate);
    }

    stem_groups.retain(|_, candidates| candidates.len() >= 2);

    for (key, candidates) in stem_groups {
        for candidate in candidates {
            files[candidate.index].group_id = Some(key.clone());
        }
    }
}

/// The extension of a photo's edit file: `IMG.CR2.agdata`.
pub const SIDECAR_EXTENSION: &str = "agdata";

/// What follows a photo's path in a virtual copy's: `IMG.CR2?vc=a1b2c3`.
pub const VIRTUAL_COPY: &str = "?vc=";

/// The edit file of `photo`, or of its virtual copy `copy_id`:
/// `IMG.CR2.agdata`, `IMG.CR2.a1b2c3.agdata`.
pub fn sidecar_for(photo: &Path, copy_id: Option<&str>) -> PathBuf {
    let mut name = photo.file_name().unwrap_or_default().to_os_string();
    if let Some(id) = copy_id {
        name.push(".");
        name.push(id);
    }
    name.push(".");
    name.push(SIDECAR_EXTENSION);
    photo.with_file_name(name)
}

/// A virtual path's photo and copy id: `IMG.CR2?vc=a1b2c3` is
/// (`IMG.CR2`, `a1b2c3`), a photo's own path has no copy.
pub fn split_virtual_path(virtual_path: &str) -> (&str, Option<&str>) {
    match virtual_path.rsplit_once(VIRTUAL_COPY) {
        Some((photo, id)) => (photo, Some(id)),
        None => (virtual_path, None),
    }
}

pub fn parse_sidecar_name(file_name: &str) -> Option<(&str, Option<&str>)> {
    let base = file_name
        .strip_suffix(SIDECAR_EXTENSION)?
        .strip_suffix('.')?;
    if base.len() >= 7 && base.as_bytes()[base.len() - 7] == b'.' {
        let id = &base[base.len() - 6..];
        if id.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
            return Some((&base[..base.len() - 7], Some(id)));
        }
    }
    Some((base, None))
}

pub fn parse_virtual_path(virtual_path: &str) -> (PathBuf, PathBuf) {
    let (photo, copy_id) = split_virtual_path(virtual_path);
    let source_path = PathBuf::from(photo);
    let sidecar_path = sidecar_for(&source_path, copy_id);
    (source_path, sidecar_path)
}

#[tauri::command]
pub async fn read_exif_for_paths(
    paths: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<HashMap<String, HashMap<String, String>>, String> {
    let is_hdd = state
        .thumbnail_manager
        .rotational_disk
        .load(Ordering::Relaxed);

    tauri::async_runtime::spawn_blocking(move || {
        let process_path = |virtual_path: &String| {
            let (source_path, _) = parse_virtual_path(virtual_path);
            let source_path_str = source_path.to_string_lossy().to_string();

            let map = if let Some(sidecar_exif) =
                crate::exif_processing::read_rrexif_sidecar(&source_path)
            {
                sidecar_exif
            } else if is_cloud_placeholder(&source_path) {
                HashMap::new()
            } else if let Ok(mmap) = read_file_mapped(&source_path) {
                crate::exif_processing::read_exif_data(&source_path_str, &mmap)
            } else if let Ok(bytes) = fs::read(&source_path) {
                crate::exif_processing::read_exif_data(&source_path_str, &bytes)
            } else {
                HashMap::new()
            };

            if map.is_empty() {
                None
            } else {
                Some((virtual_path.clone(), map))
            }
        };

        let exif_data: HashMap<String, HashMap<String, String>> = if is_hdd {
            paths.iter().filter_map(process_path).collect()
        } else {
            paths.par_iter().filter_map(process_path).collect()
        };

        Ok(exif_data)
    })
    .await
    .unwrap_or_else(|e| Err(format!("Task failed: {}", e)))
}

#[tauri::command]
pub async fn update_exif_fields(
    paths: Vec<String>,
    updates: HashMap<String, String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        paths.par_iter().for_each(|path| {
            let original_path = Path::new(&path);
            let primary_path = crate::file_management::sidecar_for(original_path, None);
            let temp_metadata = crate::exif_processing::load_sidecar(&primary_path);

            let mut exif_data = temp_metadata.exif.unwrap_or_else(|| {
                if let Some(existing) = crate::exif_processing::read_rrexif_sidecar(original_path) {
                    existing
                } else if let Ok(mmap) = read_file_mapped(original_path) {
                    crate::exif_processing::read_exif_data_from_bytes(path, &mmap)
                } else if let Ok(bytes) = fs::read(original_path) {
                    crate::exif_processing::read_exif_data_from_bytes(path, &bytes)
                } else {
                    HashMap::new()
                }
            });

            for (k, v) in &updates {
                let trimmed = v.trim();
                if trimmed.is_empty() {
                    exif_data.remove(k);
                } else {
                    exif_data.insert(k.clone(), trimmed.to_string());
                }
            }

            let mut final_metadata = crate::exif_processing::load_sidecar(&primary_path);

            final_metadata.exif = Some(exif_data);
            let _ = crate::exif_processing::write_sidecar(
                &primary_path,
                original_path,
                &final_metadata,
            );
        });
        Ok(())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

fn match_disk_kind(disks: &Disks, canonical: &Path) -> Option<bool> {
    let mut best_match: Option<(&Path, bool)> = None;

    for disk in disks.list() {
        let mount_point = disk.mount_point();
        if canonical.starts_with(mount_point) {
            let is_longer_match = best_match
                .map(|(current, _)| mount_point.as_os_str().len() > current.as_os_str().len())
                .unwrap_or(true);
            if is_longer_match {
                best_match = Some((mount_point, disk.kind() == sysinfo::DiskKind::HDD));
            }
        }
    }

    best_match.map(|(_, is_hdd)| is_hdd)
}

fn update_rotational_disk_flag(path: &str, app_handle: &AppHandle) {
    let state = app_handle.state::<crate::AppState>();
    let Ok(canonical) = Path::new(path).canonicalize() else {
        return;
    };

    let cached_match = {
        let cache = state.disks_cache.lock().unwrap();
        cache
            .as_ref()
            .and_then(|disks| match_disk_kind(disks, &canonical))
    };

    match cached_match {
        Some(is_hdd) => {
            state
                .thumbnail_manager
                .rotational_disk
                .store(is_hdd, Ordering::Relaxed);
        }
        None => {
            if !state.disks_cache_refreshing.swap(true, Ordering::Relaxed) {
                let refresh_app_handle = app_handle.clone();
                thread::spawn(move || {
                    let disks = Disks::new_with_refreshed_list();
                    let state = refresh_app_handle.state::<crate::AppState>();
                    *state.disks_cache.lock().unwrap() = Some(disks);
                    state.disks_cache_refreshing.store(false, Ordering::Relaxed);
                });
            }
        }
    }
}

#[tauri::command]
pub fn list_images_in_dir(path: String, app_handle: AppHandle) -> Result<Vec<ImageFile>, String> {
    let settings = load_settings(app_handle.clone()).unwrap_or_default();
    let enable_xmp_sync = settings.enable_xmp_sync.unwrap_or(false);

    update_rotational_disk_flag(&path, &app_handle);

    let entries = fs::read_dir(&path).map_err(|e| e.to_string())?;
    let mut images = Vec::new();
    let mut sidecars_by_filename: HashMap<String, Vec<Option<String>>> = HashMap::new();

    for entry in entries.filter_map(Result::ok) {
        let entry_path = entry.path();
        let file_name = entry
            .file_name()
            .into_string()
            .unwrap_or_else(|os| os.to_string_lossy().into_owned());

        if let Some((source_filename, copy_id)) = parse_sidecar_name(&file_name) {
            sidecars_by_filename
                .entry(source_filename.to_string())
                .or_default()
                .push(copy_id.map(str::to_string));
        } else if is_supported_image_file(&file_name) {
            images.push((file_name, entry_path));
        }
    }

    let tasks: Vec<_> = images
        .into_iter()
        .map(|(file_name, path_buf)| {
            let sidecars = sidecars_by_filename
                .remove(&file_name)
                .unwrap_or_else(|| vec![None]);
            let path_str = path_buf.to_string_lossy().into_owned();
            (path_str, path_buf, sidecars)
        })
        .collect();

    let mut result_list: Vec<ImageFile> = tasks
        .into_par_iter()
        .flat_map(|(path_str, path_buf, sidecars)| {
            let modified = fs::metadata(&path_buf)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let is_cloud_placeholder = is_cloud_placeholder(&path_buf);

            let mut file_results = Vec::with_capacity(sidecars.len());

            for copy_id_opt in sidecars {
                let sidecar_path = sidecar_for(&path_buf, copy_id_opt.as_deref());
                let (virtual_path, is_virtual_copy) = match copy_id_opt {
                    Some(id) => (format!("{path_str}{VIRTUAL_COPY}{id}"), true),
                    None => (path_str.clone(), false),
                };

                let xmp_is_placeholder = enable_xmp_sync
                    && resolve_xmp_path(&path_buf)
                        .is_some_and(|p| crate::file_management::is_cloud_placeholder(&p));

                let metadata = if crate::file_management::is_cloud_placeholder(&sidecar_path)
                    || xmp_is_placeholder
                {
                    enqueue_metadata(
                        &app_handle,
                        virtual_path.clone(),
                        path_buf.clone(),
                        sidecar_path.clone(),
                    );
                    ImageFileMetadata {
                        edit: EditStatus::default(),
                        tags: None,
                        rating: 0,
                        flag: None,
                        is_raw: crate::formats::is_raw_file(&path_buf),
                    }
                } else {
                    resolve_image_metadata(&path_buf, &sidecar_path, enable_xmp_sync, &settings)
                };

                file_results.push(ImageFile {
                    path: virtual_path,
                    modified,
                    edit: metadata.edit,
                    tags: metadata.tags,
                    exif: None,
                    is_virtual_copy,
                    is_raw: metadata.is_raw,
                    group_id: None,
                    rating: metadata.rating,
                    flag: metadata.flag,
                    is_cloud_placeholder,
                });
            }

            file_results
        })
        .collect();

    assign_group_ids(&mut result_list);
    Ok(result_list)
}

#[tauri::command]
pub fn list_images_recursive(
    path: String,
    app_handle: AppHandle,
) -> Result<Vec<ImageFile>, String> {
    let settings = load_settings(app_handle.clone()).unwrap_or_default();
    let enable_xmp_sync = settings.enable_xmp_sync.unwrap_or(false);

    update_rotational_disk_flag(&path, &app_handle);

    let root_path = Path::new(&path);
    let mut images = Vec::new();

    let mut sidecars_by_path: HashMap<PathBuf, Vec<Option<String>>> = HashMap::new();

    for entry in WalkDir::new(root_path).into_iter().filter_map(Result::ok) {
        let entry_path = entry.path();
        if !entry_path.is_file() {
            continue;
        }

        let file_name = entry_path.file_name().unwrap_or_default().to_string_lossy();
        if let Some((source_filename, copy_id)) = parse_sidecar_name(&file_name) {
            if let Some(parent) = entry_path.parent() {
                sidecars_by_path
                    .entry(parent.join(source_filename))
                    .or_default()
                    .push(copy_id.map(str::to_string));
            }
        } else if is_supported_image_file(entry_path.to_string_lossy().as_ref()) {
            images.push(entry_path.to_path_buf());
        }
    }

    let tasks: Vec<_> = images
        .into_iter()
        .map(|path_buf| {
            let sidecars = sidecars_by_path
                .remove(&path_buf)
                .unwrap_or_else(|| vec![None]);
            let path_str = path_buf.to_string_lossy().into_owned();
            (path_str, path_buf, sidecars)
        })
        .collect();

    let mut result_list: Vec<ImageFile> = tasks
        .into_par_iter()
        .flat_map(|(path_str, path_buf, sidecars)| {
            let modified = fs::metadata(&path_buf)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let is_cloud_placeholder = is_cloud_placeholder(&path_buf);

            let mut file_results = Vec::with_capacity(sidecars.len());

            for copy_id_opt in sidecars {
                let sidecar_path = sidecar_for(&path_buf, copy_id_opt.as_deref());
                let (virtual_path, is_virtual_copy) = match copy_id_opt {
                    Some(id) => (format!("{path_str}{VIRTUAL_COPY}{id}"), true),
                    None => (path_str.clone(), false),
                };

                let xmp_is_placeholder = enable_xmp_sync
                    && resolve_xmp_path(&path_buf)
                        .is_some_and(|p| crate::file_management::is_cloud_placeholder(&p));

                let metadata = if crate::file_management::is_cloud_placeholder(&sidecar_path)
                    || xmp_is_placeholder
                {
                    enqueue_metadata(
                        &app_handle,
                        virtual_path.clone(),
                        path_buf.clone(),
                        sidecar_path.clone(),
                    );
                    ImageFileMetadata {
                        edit: EditStatus::default(),
                        tags: None,
                        rating: 0,
                        flag: None,
                        is_raw: crate::formats::is_raw_file(&path_buf),
                    }
                } else {
                    resolve_image_metadata(&path_buf, &sidecar_path, enable_xmp_sync, &settings)
                };

                file_results.push(ImageFile {
                    path: virtual_path,
                    modified,
                    edit: metadata.edit,
                    tags: metadata.tags,
                    exif: None,
                    is_virtual_copy,
                    is_raw: metadata.is_raw,
                    group_id: None,
                    rating: metadata.rating,
                    flag: metadata.flag,
                    is_cloud_placeholder,
                });
            }

            file_results
        })
        .collect();

    assign_group_ids(&mut result_list);
    Ok(result_list)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AlbumItem {
    Album {
        id: String,
        name: String,
        icon: Option<String>,
        images: Vec<String>,
    },
    Group {
        id: String,
        name: String,
        icon: Option<String>,
        children: Vec<AlbumItem>,
    },
}

fn get_albums_path(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let albums_dir = data_dir.join("albums");
    if !albums_dir.exists() {
        fs::create_dir_all(&albums_dir).map_err(|e| e.to_string())?;
    }
    Ok(albums_dir.join("albums.json"))
}

pub fn sort_album_tree(items: &mut [AlbumItem]) {
    items.sort_by(|a, b| {
        let get_sort_key = |item: &AlbumItem| match item {
            AlbumItem::Group { name, .. } => (0, name.to_lowercase()),
            AlbumItem::Album { name, .. } => (1, name.to_lowercase()),
        };

        let key_a = get_sort_key(a);
        let key_b = get_sort_key(b);

        key_a.cmp(&key_b)
    });

    for item in items.iter_mut() {
        if let AlbumItem::Group { children, .. } = item {
            sort_album_tree(children);
        }
    }
}

#[tauri::command]
pub fn get_albums(app_handle: AppHandle) -> Result<Vec<AlbumItem>, String> {
    let path = get_albums_path(&app_handle)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut items: Vec<AlbumItem> = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    sort_album_tree(&mut items);
    Ok(items)
}

#[tauri::command]
pub fn save_albums(mut tree: Vec<AlbumItem>, app_handle: AppHandle) -> Result<(), String> {
    let path = get_albums_path(&app_handle)?;
    sort_album_tree(&mut tree);
    let json_string = serde_json::to_string_pretty(&tree).map_err(|e| e.to_string())?;
    crate::mods::atomic_write::write_atomic(&path, json_string).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_to_album(
    album_id: String,
    paths: Vec<String>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let mut tree = get_albums(app_handle.clone())?;

    fn add_recursive(items: &mut [AlbumItem], target_id: &str, paths_to_add: &Vec<String>) -> bool {
        for item in items.iter_mut() {
            #[allow(clippy::collapsible_match)]
            match item {
                AlbumItem::Album { id, images, .. } if id == target_id => {
                    for p in paths_to_add {
                        if !images.contains(p) {
                            images.push(p.clone());
                        }
                    }
                    return true;
                }
                AlbumItem::Group { children, .. } => {
                    if add_recursive(children, target_id, paths_to_add) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    if add_recursive(&mut tree, &album_id, &paths) {
        save_albums(tree, app_handle)?;
    }
    Ok(())
}

fn sync_album_path_changes(
    app_handle: &AppHandle,
    renames: Option<&HashMap<String, String>>,
    deletions: Option<&HashSet<String>>,
    folder_rename: Option<(&str, &str)>,
) {
    if let Ok(mut tree) = get_albums(app_handle.clone()) {
        let mut changed = false;

        fn process_nodes(
            nodes: &mut [AlbumItem],
            renames: Option<&HashMap<String, String>>,
            deletions: Option<&HashSet<String>>,
            folder_rename: Option<(&str, &str)>,
            changed: &mut bool,
        ) {
            for node in nodes.iter_mut() {
                match node {
                    AlbumItem::Album { images, .. } => {
                        let mut new_images = Vec::new();

                        for img in images.drain(..) {
                            let mut current_img = img;

                            if let Some((old_folder, new_folder)) = folder_rename {
                                let img_path = Path::new(&current_img);
                                let old_path = Path::new(old_folder);
                                if let Ok(stripped) = img_path.strip_prefix(old_path) {
                                    let new_img_path = Path::new(new_folder).join(stripped);
                                    current_img = new_img_path.to_string_lossy().into_owned();
                                    *changed = true;
                                }
                            }

                            if let Some(r) = renames {
                                if let Some(new_path) = r.get(&current_img) {
                                    current_img = new_path.clone();
                                    *changed = true;
                                } else if let (base_path, Some(vc_id)) =
                                    split_virtual_path(&current_img)
                                    && let Some(new_base) = r.get(base_path)
                                {
                                    current_img = format!("{new_base}{VIRTUAL_COPY}{vc_id}");
                                    *changed = true;
                                }
                            }

                            let mut is_deleted = false;
                            if let Some(d) = deletions {
                                if d.contains(&current_img) {
                                    is_deleted = true;
                                } else {
                                    let img_path = Path::new(&current_img);
                                    for del_path_str in d {
                                        let del_path = Path::new(del_path_str);
                                        if img_path.starts_with(del_path) {
                                            is_deleted = true;
                                            break;
                                        }

                                        if let (base_path, Some(_)) =
                                            split_virtual_path(&current_img)
                                            && base_path == del_path_str
                                        {
                                            is_deleted = true;
                                            break;
                                        }
                                    }
                                }
                            }

                            if !is_deleted {
                                new_images.push(current_img);
                            } else {
                                *changed = true;
                            }
                        }
                        *images = new_images;
                    }
                    AlbumItem::Group { children, .. } => {
                        process_nodes(children, renames, deletions, folder_rename, changed);
                    }
                }
            }
        }

        process_nodes(&mut tree, renames, deletions, folder_rename, &mut changed);

        if changed {
            let _ = save_albums(tree, app_handle.clone());
        }
    }
}

#[tauri::command]
pub fn get_album_images(
    paths: Vec<String>,
    app_handle: AppHandle,
) -> Result<Vec<ImageFile>, String> {
    let settings = load_settings(app_handle.clone()).unwrap_or_default();
    let enable_xmp_sync = settings.enable_xmp_sync.unwrap_or(false);

    let mut result_list: Vec<ImageFile> = paths
        .into_par_iter()
        .filter_map(|virtual_path| {
            let (source_path, sidecar_path) = parse_virtual_path(&virtual_path);
            if !source_path.exists() {
                return None;
            }

            let modified = fs::metadata(&source_path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let is_virtual_copy = virtual_path.contains(VIRTUAL_COPY);
            let is_cloud_placeholder = is_cloud_placeholder(&source_path);

            let xmp_is_placeholder = enable_xmp_sync
                && resolve_xmp_path(&source_path)
                    .is_some_and(|p| crate::file_management::is_cloud_placeholder(&p));

            let metadata = if crate::file_management::is_cloud_placeholder(&sidecar_path)
                || xmp_is_placeholder
            {
                enqueue_metadata(
                    &app_handle,
                    virtual_path.clone(),
                    source_path.clone(),
                    sidecar_path.clone(),
                );
                ImageFileMetadata {
                    edit: EditStatus::default(),
                    tags: None,
                    rating: 0,
                    flag: None,
                    is_raw: crate::formats::is_raw_file(&source_path),
                }
            } else {
                resolve_image_metadata(&source_path, &sidecar_path, enable_xmp_sync, &settings)
            };

            Some(ImageFile {
                path: virtual_path.clone(),
                modified,
                edit: metadata.edit,
                tags: metadata.tags,
                exif: None,
                is_virtual_copy,
                is_raw: metadata.is_raw,
                group_id: None,
                rating: metadata.rating,
                flag: metadata.flag,
                is_cloud_placeholder,
            })
        })
        .collect();

    assign_group_ids(&mut result_list);
    Ok(result_list)
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FolderNode {
    pub name: String,
    pub path: String,
    pub children: Vec<FolderNode>,
    pub is_dir: bool,
    pub image_count: usize,
    pub has_subdirs: bool,
    pub modified: u64,
    pub created: u64,
}

fn has_subdirs(path: &Path) -> bool {
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.filter_map(Result::ok) {
            if let Ok(file_type) = entry.file_type()
                && file_type.is_dir()
            {
                let name = entry.file_name();
                if !name.to_string_lossy().starts_with('.') {
                    return true;
                }
            }
        }
    }
    false
}

fn scan_dir_lazy(
    path: &Path,
    expanded_folders: &HashSet<&str>,
    show_image_counts: bool,
    prefetch_one_level: bool,
) -> Result<(Vec<FolderNode>, usize), std::io::Error> {
    let mut children_folders = Vec::new();
    let mut current_dir_image_count = 0;

    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) => {
            log::warn!("Could not scan directory '{}': {}", path.display(), e);
            return Ok((Vec::new(), 0));
        }
    };

    for entry in entries.filter_map(Result::ok) {
        let current_path = entry.path();
        let (file_type, modified, created) = match entry.metadata() {
            Ok(meta) => {
                let ft = meta.file_type();
                let mod_time = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                let cre_time = meta.created().unwrap_or(mod_time);

                (
                    ft,
                    mod_time
                        .duration_since(std::time::SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    cre_time
                        .duration_since(std::time::SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                )
            }
            Err(_) => continue,
        };

        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        if name_str.starts_with('.') {
            continue;
        }

        if file_type.is_dir() {
            let path_str = current_path.to_string_lossy().into_owned();
            let is_expanded = expanded_folders.contains(path_str.as_str());

            let should_scan = is_expanded || prefetch_one_level;
            let next_prefetch = is_expanded;

            let (grand_children, sub_dir_own_images) = if should_scan {
                scan_dir_lazy(
                    &current_path,
                    expanded_folders,
                    show_image_counts,
                    next_prefetch,
                )?
            } else {
                let count = if show_image_counts {
                    WalkDir::new(&current_path)
                        .into_iter()
                        .filter_map(Result::ok)
                        .filter(|e| {
                            e.file_type().is_file()
                                && crate::formats::is_supported_image_file(e.path())
                        })
                        .count()
                } else {
                    0
                };
                (Vec::new(), count)
            };

            let has_any_subdirs = if should_scan {
                grand_children.iter().any(|c| c.is_dir)
            } else {
                has_subdirs(&current_path)
            };

            let grand_children_sum: usize = grand_children.iter().map(|c| c.image_count).sum();
            let total_child_count = sub_dir_own_images + grand_children_sum;

            children_folders.push(FolderNode {
                name: name_str.into_owned(),
                path: path_str,
                children: grand_children,
                is_dir: true,
                image_count: total_child_count,
                has_subdirs: has_any_subdirs,
                modified,
                created,
            });
        } else if show_image_counts
            && file_type.is_file()
            && crate::formats::is_supported_image_file(&current_path)
        {
            current_dir_image_count += 1;
        }
    }

    children_folders.sort_by_key(|a| a.name.to_lowercase());

    Ok((children_folders, current_dir_image_count))
}

fn get_folder_tree_sync(
    path: String,
    expanded_folders: Vec<String>,
    show_image_counts: bool,
) -> Result<FolderNode, String> {
    let root_path = Path::new(&path);
    if !root_path.is_dir() {
        return Err(format!("Directory does not exist: {}", path));
    }

    let (modified, created) = root_path
        .metadata()
        .map(|m| {
            let mod_time = m.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            let cre_time = m.created().unwrap_or(mod_time);
            (
                mod_time
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                cre_time
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            )
        })
        .unwrap_or((0, 0));

    let expanded_set: HashSet<&str> = expanded_folders.iter().map(|s| s.as_str()).collect();

    let (children, own_count) = scan_dir_lazy(root_path, &expanded_set, show_image_counts, true)
        .map_err(|e| e.to_string())?;

    let children_sum: usize = children.iter().map(|c| c.image_count).sum();
    let has_subdirs = children.iter().any(|c| c.is_dir);

    let name = match root_path.file_name() {
        Some(n) => n.to_string_lossy().into_owned(),
        None => {
            let trimmed = path.trim_end_matches(&['/', '\\'][..]);
            if trimmed.is_empty() {
                path.clone()
            } else {
                trimmed.to_string()
            }
        }
    };

    Ok(FolderNode {
        name,
        path: path.clone(),
        children,
        is_dir: true,
        image_count: own_count + children_sum,
        has_subdirs,
        modified,
        created,
    })
}

#[tauri::command]
pub async fn get_folder_children(
    path: String,
    show_image_counts: bool,
) -> Result<Vec<FolderNode>, String> {
    match tauri::async_runtime::spawn_blocking(move || {
        let root_path = Path::new(&path);
        if !root_path.is_dir() {
            return Err(format!("Directory does not exist: {}", path));
        }
        let empty_set = HashSet::new();
        let (children, _) = scan_dir_lazy(root_path, &empty_set, show_image_counts, false)
            .map_err(|e| e.to_string())?;

        Ok(children)
    })
    .await
    {
        Ok(Ok(children)) => Ok(children),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("Task failed: {}", e)),
    }
}

#[tauri::command]
pub async fn get_folder_tree(
    path: String,
    expanded_folders: Vec<String>,
    show_image_counts: bool,
) -> Result<FolderNode, String> {
    match tauri::async_runtime::spawn_blocking(move || {
        get_folder_tree_sync(path, expanded_folders, show_image_counts)
    })
    .await
    {
        Ok(Ok(folder_node)) => Ok(folder_node),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("Failed to execute folder tree task: {}", e)),
    }
}

#[tauri::command]
pub async fn get_pinned_folder_trees(
    paths: Vec<String>,
    expanded_folders: Vec<String>,
    show_image_counts: bool,
) -> Result<Vec<FolderNode>, String> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        let results: Vec<Result<FolderNode, String>> = paths
            .par_iter()
            .map(|path| {
                get_folder_tree_sync(path.clone(), expanded_folders.clone(), show_image_counts)
            })
            .collect();

        let mut folder_nodes = Vec::new();
        for result in results {
            match result {
                Ok(node) => folder_nodes.push(node),
                Err(e) => log::warn!("Failed to get tree for pinned folder: {}", e),
            }
        }
        folder_nodes
    })
    .await;

    match result {
        Ok(nodes) => Ok(nodes),
        Err(e) => Err(format!("Task failed: {}", e)),
    }
}

#[cfg(target_os = "macos")]
pub fn is_cloud_placeholder(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    const SF_DATALESS: u32 = 0x4000_0000;

    let c_path = match std::ffi::CString::new(path.as_os_str().as_bytes()) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let mut stat_buf: libc::stat = unsafe { std::mem::zeroed() };
    let ret = unsafe { libc::lstat(c_path.as_ptr(), &mut stat_buf) };
    ret == 0 && (stat_buf.st_flags & SF_DATALESS) != 0
}

#[cfg(not(target_os = "macos"))]
pub fn is_cloud_placeholder(_path: &Path) -> bool {
    false
}

pub fn read_file_mapped(path: &Path) -> Result<Mmap, ReadFileError> {
    if !path.is_file() {
        return Err(ReadFileError::Invalid);
    }
    if !path.exists() {
        return Err(ReadFileError::NotFound);
    }
    if path.metadata().map_err(ReadFileError::Io)?.len() == 0 {
        return Err(ReadFileError::Empty);
    }
    let file = fs::File::open(path).map_err(ReadFileError::Io)?;
    // RapidRAW f9899ca9: src-tauri/src/file_management.rs
    // Filesystems that cannot lock, such as the Flatpak document portal, are mapped unlocked.
    if let Err(fs::TryLockError::WouldBlock) = file.try_lock_shared() {
        return Err(ReadFileError::Locked);
    }
    let mmap = unsafe {
        MmapOptions::new()
            .len(file.metadata().map_err(ReadFileError::Io)?.len() as usize)
            .map(&file)
            .map_err(ReadFileError::Io)?
    };
    Ok(mmap)
}

fn find_embedded_jpeg(exif: &exif::Exif, ifd: exif::In) -> Option<&[u8]> {
    let offset = exif
        .get_field(exif::Tag::JPEGInterchangeFormat, ifd)?
        .value
        .get_uint(0)? as usize;
    let len = exif
        .get_field(exif::Tag::JPEGInterchangeFormatLength, ifd)?
        .value
        .get_uint(0)? as usize;
    exif.buf().get(offset..offset + len)
}

fn apply_exif_orientation(img: DynamicImage, orientation: u32) -> DynamicImage {
    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

fn exif_embedded_preview(exif: &exif::Exif) -> Option<DynamicImage> {
    let (jpeg_bytes, ifd) = find_embedded_jpeg(exif, exif::In::PRIMARY)
        .map(|b| (b, exif::In::PRIMARY))
        .or_else(|| {
            find_embedded_jpeg(exif, exif::In::THUMBNAIL).map(|b| (b, exif::In::THUMBNAIL))
        })?;

    let img = image::load_from_memory_with_format(jpeg_bytes, image::ImageFormat::Jpeg).ok()?;

    let orientation = exif
        .get_field(exif::Tag::Orientation, ifd)
        .and_then(|f| f.value.get_uint(0))
        .unwrap_or(1);

    Some(apply_exif_orientation(img, orientation))
}

fn try_load_embedded_raw_preview(source_path: &Path, target_res: u32) -> Option<DynamicImage> {
    let mmap = read_file_mapped(source_path).ok()?;

    let preview = match exif_processing::read_exif(&mmap) {
        Some(exif) => exif_embedded_preview(&exif)?,
        None => {
            image_loader::safe_embedded_preview_fallback(&mmap, &source_path.to_string_lossy())?
        }
    };

    (preview.width().max(preview.height()) >= (target_res as f32 * 0.95) as u32).then_some(preview)
}

pub fn generate_thumbnail_data(
    path_str: &str,
    gpu_context: Option<&GpuContext>,
    preloaded_image: Option<&DynamicImage>,
    app_handle: &AppHandle,
) -> anyhow::Result<DynamicImage> {
    let (source_path, sidecar_path) = parse_virtual_path(path_str);
    let source_path_str = source_path.to_string_lossy().to_string();
    let is_raw = is_raw_file(&source_path_str);

    let metadata: Option<ImageMetadata> = if is_cloud_placeholder(&sidecar_path) {
        enqueue_metadata(
            app_handle,
            path_str.to_string(),
            source_path.clone(),
            sidecar_path.clone(),
        );
        None
    } else {
        crate::exif_processing::peek_sidecar(&sidecar_path)
    };

    let adjustments = metadata
        .as_ref()
        .map_or(serde_json::Value::Null, |m| m.adjustments.clone());

    let settings = load_settings(app_handle.clone()).unwrap_or_default();
    let always_decode_raw = settings.always_decode_raw_thumbnails.unwrap_or(false);

    if is_raw && adjustments.is_null() && preloaded_image.is_none() && !always_decode_raw {
        let target_res = settings.medium_thumbnail_resolution.unwrap_or(1280);
        if let Some(preview) = try_load_embedded_raw_preview(&source_path, target_res) {
            return Ok(preview);
        }
    }

    if let (Some(context), Some(meta)) = (gpu_context, metadata)
        && !meta.adjustments.is_null()
    {
        let state = app_handle.state::<AppState>();
        let target_res = settings.medium_thumbnail_resolution.unwrap_or(1280);

        let base_cache_hash = crate::cache_utils::calculate_thumbnail_base_hash(&meta.adjustments);

        let crop_data: Option<Crop> = serde_json::from_value(meta.adjustments["crop"].clone()).ok();

        let cached_base: Option<(Arc<DynamicImage>, f32)> = {
            let cache = state.thumbnail_geometry_cache.lock().unwrap();
            if let Some((cached_hash, img, scale)) = cache.get(path_str) {
                let mut sufficient_resolution = true;
                if let Some(c) = &crop_data
                    && c.width > 0.0
                    && c.height > 0.0
                {
                    let final_crop_max_dim =
                        (c.width as f32 * *scale).max(c.height as f32 * *scale);
                    if final_crop_max_dim < (target_res as f32 * 0.95) {
                        sufficient_resolution = false;
                    }
                }

                if *cached_hash == base_cache_hash && sufficient_resolution {
                    Some((Arc::clone(img), *scale))
                } else {
                    None
                }
            } else {
                None
            }
        };

        let (processing_base_arc, total_scale) = if let Some((arc_img, scale)) = cached_base {
            (arc_img, scale)
        } else {
            let mut raw_scale_factor = 1.0f32;

            let composite_image = if let Some(img) = preloaded_image {
                image_loader::composite_patches_on_image(img, &adjustments)?
            } else {
                let mmap_guard;
                let vec_guard;

                let file_slice: &[u8] = match read_file_mapped(&source_path) {
                    Ok(mmap) => {
                        mmap_guard = Some(mmap);
                        mmap_guard.as_ref().unwrap()
                    }
                    Err(e) => {
                        if preloaded_image.is_none() {
                            log::warn!("Fallback read for {}: {}", source_path_str, e);
                        }
                        let bytes = fs::read(&source_path).map_err(|io_err| {
                            anyhow::anyhow!(
                                "Fallback read failed for {}: {}",
                                source_path_str,
                                io_err
                            )
                        })?;
                        vec_guard = Some(bytes);
                        vec_guard.as_ref().unwrap()
                    }
                };

                let img = image_loader::load_and_composite(
                    file_slice,
                    &source_path_str,
                    &adjustments,
                    true,
                    &settings,
                    None,
                )?;

                if is_raw {
                    raw_scale_factor = crate::raw_processing::get_fast_demosaic_scale_factor(
                        file_slice,
                        img.width(),
                        img.height(),
                    );
                }
                img
            };

            let warped_image =
                apply_geometry_warp(Cow::Borrowed(&composite_image), &meta.adjustments);

            let relit_image = crate::relight::apply_relight(warped_image, &meta.adjustments);
            let fogged_image = crate::fog::apply_fog(relit_image, &meta.adjustments);
            let blurred_image = crate::lens_blur::apply_lens_blur(fogged_image, &meta.adjustments);

            let orientation_steps =
                meta.adjustments["orientationSteps"].as_u64().unwrap_or(0) as u8;
            let coarse_rotated_image = apply_coarse_rotation(blurred_image, orientation_steps);

            let (full_w, full_h) = coarse_rotated_image.dimensions();

            let mut processing_dim = target_res;
            if let Some(c) = &crop_data
                && c.width > 0.0
                && c.height > 0.0
            {
                let crop_max_dim_loaded = c.width.max(c.height) * raw_scale_factor as f64;
                let full_max_dim = full_w.max(full_h) as f64;
                if crop_max_dim_loaded > 0.0 {
                    processing_dim = ((target_res as f64 * full_max_dim / crop_max_dim_loaded)
                        .round() as u32)
                        .min(full_w.max(full_h));
                }
            }

            let (base, gpu_scale) = if full_w > processing_dim || full_h > processing_dim {
                let base = crate::image_processing::downscale_f32_image(
                    &coarse_rotated_image,
                    processing_dim,
                    processing_dim,
                );
                let scale = if full_w > 0 {
                    base.width() as f32 / full_w as f32
                } else {
                    1.0
                };
                (base, scale)
            } else {
                (coarse_rotated_image.into_owned(), 1.0)
            };

            let total_scale = gpu_scale * raw_scale_factor;

            let mut cache = state.thumbnail_geometry_cache.lock().unwrap();
            if cache.len() >= 8 {
                let key_to_remove = cache.keys().next().cloned();
                if let Some(key) = key_to_remove {
                    cache.remove(&key);
                }
            }

            let base_arc = Arc::new(base);
            cache.insert(
                path_str.to_string(),
                (base_cache_hash, Arc::clone(&base_arc), total_scale),
            );

            (base_arc, total_scale)
        };

        let rotation_degrees = meta.adjustments["rotation"].as_f64().unwrap_or(0.0) as f32;
        let flip_horizontal = meta.adjustments["flipHorizontal"]
            .as_bool()
            .unwrap_or(false);
        let flip_vertical = meta.adjustments["flipVertical"].as_bool().unwrap_or(false);

        let flipped_image = apply_flip(
            Cow::Borrowed(&*processing_base_arc),
            flip_horizontal,
            flip_vertical,
        );
        let rotated_image = apply_rotation(flipped_image, rotation_degrees);

        let scaled_crop_json = if let Some(c) = &crop_data {
            serde_json::to_value(Crop {
                x: c.x * total_scale as f64,
                y: c.y * total_scale as f64,
                width: c.width * total_scale as f64,
                height: c.height * total_scale as f64,
            })
            .unwrap_or(serde_json::Value::Null)
        } else {
            serde_json::Value::Null
        };

        let cropped_preview = apply_crop(rotated_image, &scaled_crop_json);
        let (preview_w, preview_h) = cropped_preview.dimensions();
        let unscaled_crop_offset = crop_data.map_or((0.0, 0.0), |c| (c.x as f32, c.y as f32));

        let mask_definitions: Vec<MaskDefinition> = meta
            .adjustments
            .get("masks")
            .and_then(|m| serde_json::from_value(m.clone()).ok())
            .unwrap_or_else(Vec::new);

        let mask_bitmaps: Vec<ImageBuffer<Luma<u8>, Vec<u8>>> = mask_definitions
            .iter()
            .filter_map(|def| {
                crate::get_cached_or_generate_mask(
                    &state,
                    path_str,
                    def,
                    preview_w,
                    preview_h,
                    total_scale,
                    (
                        unscaled_crop_offset.0 * total_scale,
                        unscaled_crop_offset.1 * total_scale,
                    ),
                    &meta.adjustments,
                )
            })
            .collect();

        let gpu_adjustments = get_all_adjustments_from_json(
            &meta.adjustments,
            is_raw,
            as_shot(&source_path_str),
            resolve_render_options(&settings, is_raw),
            Some(source_path_str.as_str()),
        );
        let lut = crate::mods::provider_look::lut_of_edit(&state, &meta.adjustments, &source_path_str);

        let mut hasher = DefaultHasher::new();
        path_str.hash(&mut hasher);
        meta.adjustments.to_string().hash(&mut hasher);
        let unique_hash = hasher.finish();

        if let Ok(processed_image) = gpu_processing::process_and_get_dynamic_image(
            context,
            &state,
            cropped_preview.as_ref(),
            unique_hash,
            gpu_processing::RenderRequest {
                adjustments: gpu_adjustments,
                mask_bitmaps: &mask_bitmaps,
                lut,
                roi: None,
            },
            "generate_thumbnail_data",
        ) {
            return Ok(processed_image);
        } else {
            return Ok(cropped_preview.into_owned());
        }
    }

    let mut final_image = if let Some(img) = preloaded_image {
        image_loader::composite_patches_on_image(img, &adjustments)?
    } else {
        match read_file_mapped(&source_path) {
            Ok(mmap) => image_loader::load_and_composite(
                &mmap,
                &source_path_str,
                &adjustments,
                true,
                &settings,
                None,
            )?,
            Err(e) => {
                log::warn!("Fallback read for {}: {}", source_path_str, e);
                let bytes = fs::read(&source_path)?;
                image_loader::load_and_composite(
                    &bytes,
                    &source_path_str,
                    &adjustments,
                    true,
                    &settings,
                    None,
                )?
            }
        }
    };

    if adjustments.is_null() {
        let render = resolve_render_options(&settings, is_raw);
        let use_agx = render.tonemapper_override == Some(1);

        if use_agx {
            if !is_raw {
                final_image = crate::image_processing::apply_srgb_to_linear(final_image);
            }
            crate::image_processing::apply_cpu_agx_tonemap(&mut final_image);
        } else if is_raw {
            apply_cpu_default_raw_processing(&mut final_image);
        }
    }

    let fallback_orientation_steps = adjustments["orientationSteps"].as_u64().unwrap_or(0) as u8;
    Ok(apply_coarse_rotation(Cow::Owned(final_image), fallback_orientation_steps).into_owned())
}

fn encode_thumbnail(image: &DynamicImage, target_width: u32) -> Result<Vec<u8>> {
    let thumbnail = crate::image_processing::downscale_f32_image(image, target_width, target_width);
    let mut buf = Cursor::new(Vec::new());
    let mut encoder = JpegEncoder::new_with_quality(&mut buf, 75);
    encoder.encode_image(&thumbnail.to_rgb8())?;
    Ok(buf.into_inner())
}

fn generate_single_thumbnail_and_cache(
    path_str: &str,
    thumb_cache_dir: &Path,
    gpu_context: Option<&GpuContext>,
    preloaded_image: Option<&DynamicImage>,
    force_regenerate: bool,
    app_handle: &AppHandle,
    settings: &AppSettings,
) -> Option<(String, String, u8, EditStatus)> {
    let (source_path, sidecar_path) = parse_virtual_path(path_str);

    let (rating, edit, adjustments_bytes) = if is_cloud_placeholder(&sidecar_path) {
        enqueue_metadata(
            app_handle,
            path_str.to_string(),
            source_path.clone(),
            sidecar_path.clone(),
        );
        (0, EditStatus::default(), Vec::new())
    } else if let Some(meta) = crate::exif_processing::peek_sidecar(&sidecar_path) {
        let is_raw = crate::formats::is_raw_file(path_str);
        let tm = crate::image_processing::resolve_tonemapper_override(settings, is_raw);
        (
            meta.rating,
            EditStatus::of(&meta.adjustments, is_raw, tm),
            serde_json::to_vec(&meta.adjustments).unwrap_or_default(),
        )
    } else {
        (0, EditStatus::default(), Vec::new())
    };

    let cache_hash = compute_thumbnail_cache_hash(path_str, &adjustments_bytes)?;

    let small_path = thumb_cache_dir.join(format!("{}_small.jpg", cache_hash));
    let medium_path = thumb_cache_dir.join(format!("{}_medium.jpg", cache_hash));

    if !force_regenerate && small_path.exists() && medium_path.exists() {
        return Some((
            small_path.to_string_lossy().into_owned(),
            medium_path.to_string_lossy().into_owned(),
            rating,
            edit,
        ));
    }

    if is_cloud_placeholder(&source_path) {
        return None;
    }

    let target_width_small = settings.small_thumbnail_resolution.unwrap_or(480);
    let target_width_medium = settings.medium_thumbnail_resolution.unwrap_or(1280);

    if let Ok(thumb_image) =
        generate_thumbnail_data(path_str, gpu_context, preloaded_image, app_handle)
        && let (Ok(small_data), Ok(medium_data)) = (
            encode_thumbnail(&thumb_image, target_width_small),
            encode_thumbnail(&thumb_image, target_width_medium),
        )
    {
        let _ = fs::write(&small_path, &small_data);
        let _ = fs::write(&medium_path, &medium_data);
        return Some((
            small_path.to_string_lossy().into_owned(),
            medium_path.to_string_lossy().into_owned(),
            rating,
            edit,
        ));
    }
    None
}

fn prefetch_source_file(path_str: &str) {
    let (source_path, _) = parse_virtual_path(path_str);
    if let Ok(mut file) = std::fs::File::open(&source_path) {
        let _ = std::io::copy(&mut file, &mut std::io::sink());
    }
}

pub fn start_thumbnail_workers(app_handle: tauri::AppHandle) {
    let state = app_handle.state::<crate::AppState>();
    let manager = state.thumbnail_manager.clone();
    let settings = load_settings(app_handle.clone()).unwrap_or_default();
    let thread_count = settings.thumbnail_worker_threads.unwrap_or(4).clamp(1, 16);

    for _ in 0..thread_count {
        let app_clone = app_handle.clone();
        let manager_clone = manager.clone();

        std::thread::spawn(move || {
            loop {
                let path_to_process: String = {
                    let mut queue = manager_clone.queue.lock().unwrap();
                    while queue.is_empty() {
                        queue = manager_clone.cvar.wait(queue).unwrap();
                    }
                    let path = queue.pop_back().unwrap();

                    let mut processing = manager_clone.processing_now.lock().unwrap();
                    if processing.contains(&path) {
                        let state = app_clone.state::<crate::AppState>();
                        increment_thumbnail_progress(&state, &app_clone);
                        continue;
                    }
                    processing.insert(path.clone());
                    path
                };

                let state = app_clone.state::<crate::AppState>();
                let gpu_context =
                    crate::gpu_processing::get_or_init_gpu_context(&state, &app_clone).ok();

                let current_settings = load_settings(app_clone.clone()).unwrap_or_default();

                if let Ok(cache_dir) = get_thumb_cache_dir(&app_clone) {
                    if manager_clone.rotational_disk.load(Ordering::Relaxed) {
                        let _io_permit = manager_clone.io_gate.lock().unwrap();
                        prefetch_source_file(&path_to_process);
                    }

                    let result = generate_single_thumbnail_and_cache(
                        &path_to_process,
                        &cache_dir,
                        gpu_context.as_ref(),
                        None,
                        false,
                        &app_clone,
                        &current_settings,
                    );

                    if let Some((small_path, medium_path, rating, edit)) = result {
                        emit_thumbnail_generated(
                            &app_clone,
                            &path_to_process,
                            &small_path,
                            &medium_path,
                            rating,
                            edit,
                        );
                    }
                    increment_thumbnail_progress(&state, &app_clone);
                }
                manager_clone
                    .processing_now
                    .lock()
                    .unwrap()
                    .remove(&path_to_process);
            }
        });
    }
}

#[tauri::command]
pub fn update_thumbnail_queue(
    paths: Vec<String>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let state = app_handle.state::<crate::AppState>();

    let mut queue = state.thumbnail_manager.queue.lock().unwrap();

    if paths.is_empty() {
        queue.clear();
        let mut tracker = state.thumbnail_progress.lock().unwrap();
        tracker.total = 0;
        tracker.completed = 0;
        drop(tracker);

        let _ = app_handle.emit(
            "thumbnail-progress",
            serde_json::json!({ "current": 0, "total": 0 }),
        );
        state.thumbnail_manager.cvar.notify_all();
        return Ok(());
    }

    let mut unique_paths = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for path in paths {
        if seen.insert(path.clone()) {
            unique_paths.push(path);
        }
    }

    queue.retain(|p| !seen.contains(p));

    while queue.len() + unique_paths.len() > 500 {
        queue.pop_front();
    }

    if state
        .thumbnail_manager
        .rotational_disk
        .load(Ordering::Relaxed)
    {
        unique_paths.sort();
        for path in unique_paths.into_iter().rev() {
            queue.push_back(path);
        }
    } else {
        for path in unique_paths {
            queue.push_back(path);
        }
    }

    let queue_len = queue.len();
    drop(queue);

    let mut tracker = state.thumbnail_progress.lock().unwrap();
    tracker.total = tracker.completed + queue_len;

    let current = tracker.completed;
    let total = tracker.total;
    drop(tracker);

    let _ = app_handle.emit(
        "thumbnail-progress",
        serde_json::json!({ "current": current, "total": total }),
    );

    state.thumbnail_manager.cvar.notify_all();
    Ok(())
}

pub fn add_to_thumbnail_queue(state: &AppState, count: usize, app_handle: &AppHandle) {
    let mut tracker = state.thumbnail_progress.lock().unwrap();
    tracker.total += count;
    let current = tracker.completed;
    let total = tracker.total;
    drop(tracker);

    let _ = app_handle.emit(
        "thumbnail-progress",
        serde_json::json!({ "current": current, "total": total }),
    );
}

pub fn increment_thumbnail_progress(state: &AppState, app_handle: &AppHandle) {
    let mut tracker = state.thumbnail_progress.lock().unwrap();
    tracker.completed += 1;
    let current = tracker.completed;
    let total = tracker.total;

    if current >= total {
        tracker.total = 0;
        tracker.completed = 0;
        drop(tracker);

        let _ = app_handle.emit(
            "thumbnail-progress",
            serde_json::json!({ "current": 0, "total": 0 }),
        );
        let _ = app_handle.emit("thumbnail-generation-complete", true);
    } else {
        drop(tracker);
        let _ = app_handle.emit(
            "thumbnail-progress",
            serde_json::json!({ "current": current, "total": total }),
        );
    }
}

fn emit_thumbnail_generated(
    app_handle: &AppHandle,
    path: &str,
    small_thumbnail_path: &str,
    medium_thumbnail_path: &str,
    rating: u8,
    edit: EditStatus,
) {
    let _ = app_handle.emit(
        "thumbnail-generated",
        serde_json::json!({
            "path": path,
            "thumbnailPath": small_thumbnail_path,
            "previewPath": medium_thumbnail_path,
            "rating": rating,
            "is_edited": edit.is_edited,
            "has_provider_look": edit.has_provider_look
        }),
    );
}

pub fn resolve_lens_params_in_adjustments(
    adjustments: &mut Value,
    exif_data: &Option<HashMap<String, String>>,
    lens_db: Option<&crate::lens_correction::LensDatabase>,
) {
    if let Some(map) = adjustments.as_object_mut() {
        let mode = map
            .get("lensCorrectionMode")
            .and_then(|v| v.as_str())
            .unwrap_or("manual");

        if mode == "auto" {
            if let Some(exif) = exif_data {
                let exif_maker = exif.get("Make").map(|s| s.as_str()).unwrap_or("");
                let exif_model = exif.get("LensModel").map(|s| s.as_str()).unwrap_or("");
                let exif_camera_model = exif.get("Model").map(|s| s.as_str()).unwrap_or("");
                if let Some(db) = lens_db {
                    if let Some((detected_maker, detected_model)) =
                        crate::lens_correction::find_best_lens_match(
                            db,
                            exif_maker,
                            exif_model,
                            exif_camera_model,
                        )
                    {
                        map.insert(
                            "lensMaker".to_string(),
                            serde_json::to_value(&detected_maker).unwrap(),
                        );
                        map.insert(
                            "lensModel".to_string(),
                            serde_json::to_value(&detected_model).unwrap(),
                        );
                    } else {
                        map.remove("lensMaker");
                        map.remove("lensModel");
                    }
                }
            } else {
                map.remove("lensMaker");
                map.remove("lensModel");
            }
        }

        if let Some(db) = lens_db {
            let has_valid_lens = match (
                map.get("lensMaker").and_then(|v| v.as_str()),
                map.get("lensModel").and_then(|v| v.as_str()),
            ) {
                (Some(maker), Some(model)) if !maker.is_empty() && !model.is_empty() => {
                    let no_exif = HashMap::new();
                    let exif = exif_data.as_ref().unwrap_or(&no_exif);
                    if let Some(params) =
                        crate::lens_correction::resolve_lens_params(db, maker, model, exif)
                    {
                        map.insert(
                            "lensDistortionParams".to_string(),
                            serde_json::to_value(params).unwrap(),
                        );
                        true
                    } else {
                        false
                    }
                }
                _ => false,
            };

            if !has_valid_lens {
                map.remove("lensDistortionParams");
            }
        }
    }
}

#[tauri::command]
pub fn get_supported_file_types() -> Result<serde_json::Value, String> {
    let raw_extensions: Vec<&str> = crate::formats::RAW_EXTENSIONS
        .iter()
        .map(|(ext, _)| *ext)
        .collect();
    let non_raw_extensions: Vec<&str> = crate::formats::NON_RAW_EXTENSIONS.to_vec();

    Ok(serde_json::json!({
        "raw": raw_extensions,
        "nonRaw": non_raw_extensions
    }))
}

#[tauri::command]
pub fn create_folder(path: String) -> Result<(), String> {
    let path_obj = Path::new(&path);
    if let (Some(parent), Some(new_folder_name_os)) = (path_obj.parent(), path_obj.file_name())
        && let Some(new_folder_name) = new_folder_name_os.to_str()
        && parent.exists()
    {
        for entry in fs::read_dir(parent).map_err(|e| e.to_string())? {
            if let Ok(entry) = entry
                && entry.file_name().to_string_lossy().to_lowercase()
                    == new_folder_name.to_lowercase()
            {
                return Err("A folder with that name already exists.".to_string());
            }
        }
    }
    fs::create_dir_all(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_folder(path: String, new_name: String, app_handle: AppHandle) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.is_dir() {
        return Err("Path is not a directory.".to_string());
    }
    if let Some(parent) = p.parent() {
        for entry in fs::read_dir(parent).map_err(|e| e.to_string())? {
            if let Ok(entry) = entry
                && entry.file_name().to_string_lossy().to_lowercase() == new_name.to_lowercase()
                && entry.path() != p
            {
                return Err("A folder with that name already exists.".to_string());
            }
        }
        let new_path = parent.join(&new_name);
        fs::rename(p, &new_path).map_err(|e| e.to_string())?;

        let new_folder_str = new_path.to_string_lossy().into_owned();
        sync_album_path_changes(&app_handle, None, None, Some((&path, &new_folder_str)));

        Ok(())
    } else {
        Err("Could not determine parent directory.".to_string())
    }
}

#[tauri::command]
pub fn delete_folder(path: String, app_handle: AppHandle) -> Result<(), String> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        if let Err(trash_error) = trash::delete(&path) {
            log::warn!(
                "Failed to move folder to trash: {}. Falling back to permanent delete.",
                trash_error
            );
            fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
    }

    let mut deletions = HashSet::new();
    deletions.insert(path);
    sync_album_path_changes(&app_handle, None, Some(&deletions), None);

    Ok(())
}

#[tauri::command]
pub fn duplicate_file(
    path: String,
    target_album_id: Option<String>,
    app_handle: AppHandle,
) -> Result<String, String> {
    let dest_path_str = duplicate_file_on_disk(&path)?;

    if let Some(album_id) = target_album_id {
        let _ = add_to_album(album_id, vec![dest_path_str.clone()], app_handle);
    }

    Ok(dest_path_str)
}

/// Argentum: `duplicate_file` without the album update, so tests can run it.
pub(crate) fn duplicate_file_on_disk(path: &str) -> Result<String, String> {
    let (source_path, source_sidecar_path) = parse_virtual_path(path);
    if !source_path.is_file() {
        return Err("Source path is not a file.".to_string());
    }

    let parent = source_path
        .parent()
        .ok_or("Could not get parent directory")?;
    let stem = source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("Could not get file stem")?;
    let extension = source_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    let mut counter = 1;
    let mut dest_path;
    loop {
        let new_stem = if counter == 1 {
            format!("{}_copy", stem)
        } else {
            format!("{}_copy_{}", stem, counter - 1)
        };
        dest_path = parent.join(format!("{}.{}", new_stem, extension));
        if !dest_path.exists() {
            break;
        }
        counter += 1;
    }

    fs::copy(&source_path, &dest_path).map_err(|e| e.to_string())?;

    if source_sidecar_path.exists()
        && let Some(dest_str) = dest_path.to_str()
    {
        let (_, dest_sidecar_path) = parse_virtual_path(dest_str);
        fs::copy(&source_sidecar_path, &dest_sidecar_path).map_err(|e| e.to_string())?;
    }

    let mut source_rrexif_name = source_path.file_name().unwrap().to_os_string();
    source_rrexif_name.push(".agexif");
    let source_rrexif = source_path.with_file_name(source_rrexif_name);

    if source_rrexif.exists() {
        let mut dest_rrexif_name = dest_path.file_name().unwrap().to_os_string();
        dest_rrexif_name.push(".agexif");
        let dest_rrexif = dest_path.with_file_name(dest_rrexif_name);
        let _ = fs::copy(&source_rrexif, &dest_rrexif);
    }

    for (look_file, dest_look_file) in file_ops::look_file_copies(&source_path, &dest_path) {
        fs::copy(&look_file, &dest_look_file).map_err(|e| e.to_string())?;
    }

    Ok(dest_path.to_string_lossy().into_owned())
}

pub(crate) fn find_all_associated_files(source_image_path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut associated_files = vec![source_image_path.to_path_buf()];

    let mut rrexif_name = source_image_path
        .file_name()
        .unwrap_or_default()
        .to_os_string();
    rrexif_name.push(".agexif");
    let rrexif_path = source_image_path.with_file_name(rrexif_name);

    if rrexif_path.exists() {
        associated_files.push(rrexif_path);
    }

    if let Some(xmp_path) = resolve_xmp_path(source_image_path) {
        associated_files.push(xmp_path);
    }

    let parent_dir = source_image_path
        .parent()
        .ok_or("Could not determine parent directory")?;
    let source_filename = source_image_path
        .file_name()
        .ok_or("Could not get source filename")?
        .to_string_lossy();

    if let Ok(entries) = fs::read_dir(parent_dir) {
        for entry in entries.filter_map(Result::ok) {
            let entry_path = entry.path();
            if !entry_path.is_file() {
                continue;
            }

            let entry_os_filename = entry.file_name();
            let entry_filename = entry_os_filename.to_string_lossy();

            if file_ops::is_sidecar_of(&entry_filename, &source_filename)
                || file_ops::is_look_file_of(&entry_filename, &source_filename)
            {
                associated_files.push(entry_path);
            }
        }
    }

    Ok(associated_files)
}

#[tauri::command]
pub fn copy_files(source_paths: Vec<String>, destination_folder: String) -> Result<(), String> {
    let dest_path = Path::new(&destination_folder);
    if !dest_path.is_dir() {
        return Err(format!(
            "Destination is not a folder: {}",
            destination_folder
        ));
    }

    let unique_source_images: HashSet<PathBuf> = source_paths
        .iter()
        .map(|p| parse_virtual_path(p).0)
        .collect();

    let mut operations_to_perform = Vec::new();
    // Argentum: every destination is claimed once, so no file of this copy
    // overwrites another or one already there (mods/file_ops.rs).
    let mut destinations = Destinations::default();
    let aborted = |e: file_ops::FileOpError| format!("Copy aborted: {}", e);

    for source_image_path in &unique_source_images {
        let all_files_to_copy = find_all_associated_files(source_image_path)?;

        let source_parent = source_image_path
            .parent()
            .ok_or("Could not get parent directory")?;

        if source_parent == dest_path {
            operations_to_perform.extend(
                file_ops::same_folder_copy(
                    source_image_path,
                    &all_files_to_copy,
                    &mut destinations,
                )
                .map_err(aborted)?,
            );
        } else {
            for file_to_copy in all_files_to_copy {
                if let Some(file_name) = file_to_copy.file_name() {
                    let dest_file_path = dest_path.join(file_name);

                    if destinations
                        .claim(&file_to_copy, &dest_file_path)
                        .map_err(aborted)?
                    {
                        operations_to_perform.push((file_to_copy, dest_file_path));
                    }
                }
            }
        }
    }

    for (source, dest) in operations_to_perform {
        fs::copy(&source, &dest)
            .map_err(|e| format!("Copy failed for {}: {}", source.display(), e))?;
    }

    Ok(())
}

#[tauri::command]
pub fn move_files(
    source_paths: Vec<String>,
    destination_folder: String,
    app_handle: AppHandle,
) -> Result<(), String> {
    let renames = move_files_on_disk(&source_paths, &destination_folder)?;
    sync_album_path_changes(&app_handle, Some(&renames), None, None);
    Ok(())
}

/// Argentum: `move_files` without the album update, so tests can run it.
/// Returns each moved photo's old path and new path.
pub(crate) fn move_files_on_disk(
    source_paths: &[String],
    destination_folder: &str,
) -> Result<HashMap<String, String>, String> {
    let dest_path = Path::new(destination_folder);
    if !dest_path.is_dir() {
        return Err(format!(
            "Destination is not a folder: {}",
            destination_folder
        ));
    }

    let unique_source_images: HashSet<PathBuf> = source_paths
        .iter()
        .map(|p| parse_virtual_path(p).0)
        .collect();

    let mut operations_to_perform = Vec::new();
    let mut renames = HashMap::new();
    // Argentum: every destination is claimed once, so no file of this move
    // overwrites another or one already there (mods/file_ops.rs).
    let mut destinations = Destinations::default();

    for source_image_path in &unique_source_images {
        let source_parent = source_image_path
            .parent()
            .ok_or("Could not get parent directory")?;

        if source_parent == dest_path {
            return Err("Cannot move files into the same folder they are already in.".to_string());
        }

        let all_files_to_move = find_all_associated_files(source_image_path)?;

        for file_to_move in &all_files_to_move {
            if let Some(file_name) = file_to_move.file_name() {
                let dest_file_path = dest_path.join(file_name);

                if destinations
                    .claim(file_to_move, &dest_file_path)
                    .map_err(|e| format!("Move aborted: {}", e))?
                {
                    // Argentum: the XMP a RAW and its JPEG share is copied
                    // when the other stays behind (mods/file_ops.rs).
                    let keep_source = file_ops::is_xmp(file_to_move)
                        && file_ops::xmp_still_used(file_to_move, &unique_source_images);
                    operations_to_perform.push((file_to_move.clone(), dest_file_path, keep_source));
                }
            }
        }

        let image_file_name = source_image_path
            .file_name()
            .ok_or("Could not get file name")?;
        let dest_image_path = dest_path.join(image_file_name);
        renames.insert(
            source_image_path.to_string_lossy().into_owned(),
            dest_image_path.to_string_lossy().into_owned(),
        );
    }

    for (source, dest, keep_source) in operations_to_perform {
        if keep_source {
            fs::copy(&source, &dest)
                .map_err(|e| format!("Move failed during copy for {}: {}", source.display(), e))?;
            continue;
        }
        if fs::rename(&source, &dest).is_err() {
            fs::copy(&source, &dest)
                .map_err(|e| format!("Move failed during copy for {}: {}", source.display(), e))?;

            if let Err(e) = fs::remove_file(&source) {
                log::warn!(
                    "Moved file successfully, but failed to delete original {}: {}",
                    source.display(),
                    e
                );
            }
        }
    }

    Ok(renames)
}

#[tauri::command]
pub fn save_metadata_and_update_thumbnail(
    path: String,
    adjustments: Value,
    app_handle: AppHandle,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    let (source_path, sidecar_path) = parse_virtual_path(&path);

    let mut metadata = crate::exif_processing::load_sidecar_with_exif(&sidecar_path, &source_path);

    let mut final_adjustments = adjustments;
    {
        let lens_db_guard = state.lens_db.lock().unwrap();
        resolve_lens_params_in_adjustments(
            &mut final_adjustments,
            &metadata.exif,
            lens_db_guard.as_deref(),
        );
    }

    metadata.adjustments = final_adjustments;

    crate::exif_processing::write_sidecar(&sidecar_path, &source_path, &metadata)
        .map_err(|e| e.to_string())?;

    if let Ok(settings) = load_settings(app_handle.clone())
        && settings.enable_xmp_sync.unwrap_or(false)
    {
        let create_if_missing = settings.create_xmp_if_missing.unwrap_or(false);
        sync_metadata_to_xmp(&source_path, &metadata, create_if_missing);
    }

    let loaded_image_lock = state.original_image.lock().unwrap();
    let preloaded_image_option = if let Some(loaded_image) = loaded_image_lock.as_ref() {
        if loaded_image.path == path {
            Some(loaded_image.image.clone())
        } else {
            None
        }
    } else {
        None
    };
    drop(loaded_image_lock);

    let gpu_context = gpu_processing::get_or_init_gpu_context(&state, &app_handle).ok();
    let app_handle_clone = app_handle.clone();
    let path_clone = path.clone();

    add_to_thumbnail_queue(&state, 1, &app_handle);

    thread::spawn(move || {
        let state = app_handle_clone.state::<AppState>();
        let settings = load_settings(app_handle_clone.clone()).unwrap_or_default();

        let thumb_cache_dir = match get_thumb_cache_dir(&app_handle_clone) {
            Ok(dir) => dir,
            Err(e) => {
                log::warn!(
                    "Unable to initialize thumbnail cache directory for '{}': {}",
                    path_clone,
                    e
                );
                emit_thumbnail_cache_setup_error(&app_handle_clone, &path_clone, &e);
                increment_thumbnail_progress(&state, &app_handle_clone);
                return;
            }
        };

        let result = generate_single_thumbnail_and_cache(
            &path_clone,
            &thumb_cache_dir,
            gpu_context.as_ref(),
            preloaded_image_option.as_deref(),
            true,
            &app_handle_clone,
            &settings,
        );

        if let Some((small_path, medium_path, rating, edit)) = result {
            emit_thumbnail_generated(
                &app_handle_clone,
                &path_clone,
                &small_path,
                &medium_path,
                rating,
                edit,
            );
        }

        increment_thumbnail_progress(&state, &app_handle_clone);
    });

    Ok(())
}

#[tauri::command]
pub async fn apply_adjustments_to_paths(
    paths: Vec<String>,
    adjustments: Value,
    app_handle: AppHandle,
) -> Result<(), String> {
    // RapidRAW cabc73a4: src-tauri/src/file_management.rs. Returns once every
    // sidecar is written, so a caller that reads one back sees the paste.
    let lens_db = app_handle
        .state::<AppState>()
        .lens_db
        .lock()
        .unwrap()
        .clone();

    tauri::async_runtime::spawn_blocking(move || {
        update_sidecars_and_thumbnails(paths, app_handle, move |source_path, sidecar_path| {
            let mut metadata =
                crate::exif_processing::load_sidecar_with_exif(sidecar_path, source_path);

            if metadata.adjustments.is_null() {
                metadata.adjustments = default_adjustments();
            }

            crate::mods::provider_look::give_way_to_paste(&mut metadata.adjustments, &adjustments);

            if let (Some(new_map), Some(pasted_map)) = (
                metadata.adjustments.as_object_mut(),
                adjustments.as_object(),
            ) {
                for (k, v) in pasted_map {
                    new_map.insert(k.clone(), v.clone());
                }
            }

            resolve_lens_params_in_adjustments(
                &mut metadata.adjustments,
                &metadata.exif,
                lens_db.as_deref(),
            );
            metadata
        });
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn reset_adjustments_for_paths(
    paths: Vec<String>,
    app_handle: AppHandle,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        update_sidecars_and_thumbnails(paths, app_handle, |_, sidecar_path| {
            let mut metadata = crate::exif_processing::load_sidecar(sidecar_path);
            metadata.adjustments = default_adjustments();
            metadata
        });
    });

    Ok(())
}

// RapidRAW cabc73a4: src-tauri/src/file_management.rs. Each photo's thumbnail
// starts as soon as its sidecar is written, not after the whole batch, and one
// whose file and edit are unchanged is reused, as browsing a folder does.
fn update_sidecars_and_thumbnails(
    paths: Vec<String>,
    app_handle: AppHandle,
    update: impl Fn(&Path, &Path) -> ImageMetadata + Sync,
) {
    let state = app_handle.state::<AppState>();
    add_to_thumbnail_queue(&state, paths.len(), &app_handle);

    let settings = load_settings(app_handle.clone()).unwrap_or_default();
    let enable_xmp_sync = settings.enable_xmp_sync.unwrap_or(false);
    let create_xmp_if_missing = settings.create_xmp_if_missing.unwrap_or(false);

    let (updated_tx, updated_rx) = std::sync::mpsc::channel::<String>();
    let thumbnail_app_handle = app_handle.clone();
    let thumbnail_settings = settings.clone();
    tauri::async_runtime::spawn_blocking(move || {
        regenerate_thumbnails(
            updated_rx.into_iter(),
            &thumbnail_app_handle,
            &thumbnail_settings,
            false,
        );
    });

    paths
        .par_iter()
        .for_each_with(updated_tx, |updated_tx, path| {
            let (source_path, sidecar_path) = parse_virtual_path(path);
            let metadata = update(&source_path, &sidecar_path);

            let _ = crate::exif_processing::write_sidecar(&sidecar_path, &source_path, &metadata);
            if enable_xmp_sync {
                sync_metadata_to_xmp(&source_path, &metadata, create_xmp_if_missing);
            }

            let _ = updated_tx.send(path.clone());
        });
}

// RapidRAW cabc73a4: src-tauri/src/file_management.rs (`run_batch_workers`),
// adapted. The items are taken on the calling thread, which may wait for
// ones still being made, and each is worked on rayon's pool. Theirs gives
// every batch threads of its own; on the shared pool, batches running at once
// (auto-sync fires on every pause of a slider) still never decode more photos
// at a time than before. Never call it from rayon's pool with items that wait.
fn run_batch_workers<T: Send>(items: impl Iterator<Item = T>, work: impl Fn(T) + Sync) {
    let work = &work;
    rayon::in_place_scope(|scope| {
        for item in items {
            scope.spawn(move |_| work(item));
        }
    });
}

pub(crate) fn regenerate_thumbnails(
    paths: impl Iterator<Item = String>,
    app_handle: &AppHandle,
    settings: &AppSettings,
    force_regenerate: bool,
) {
    let state = app_handle.state::<AppState>();
    let thumb_cache_dir = match get_thumb_cache_dir(app_handle) {
        Ok(dir) => dir,
        Err(e) => {
            log::warn!("Unable to initialize thumbnail cache directory: {}", e);
            for path in paths {
                emit_thumbnail_cache_setup_error(app_handle, &path, &e);
                increment_thumbnail_progress(&state, app_handle);
            }
            return;
        }
    };

    let gpu_context = gpu_processing::get_or_init_gpu_context(&state, app_handle).ok();

    run_batch_workers(paths, |path| {
        let path_str = path.as_str();
        let result = generate_single_thumbnail_and_cache(
            path_str,
            &thumb_cache_dir,
            gpu_context.as_ref(),
            None,
            force_regenerate,
            app_handle,
            settings,
        );

        if let Some((small_path, medium_path, rating, edit)) = result {
            emit_thumbnail_generated(
                app_handle,
                path_str,
                &small_path,
                &medium_path,
                rating,
                edit,
            );
        }

        increment_thumbnail_progress(&state, app_handle);
    });
}

#[tauri::command]
pub async fn apply_auto_lens_correction_to_paths(
    paths: Vec<String>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let lens_db = app_handle
        .state::<AppState>()
        .lens_db
        .lock()
        .unwrap()
        .clone();

    tauri::async_runtime::spawn_blocking(move || {
        update_sidecars_and_thumbnails(paths, app_handle, move |source_path, sidecar_path| {
            let mut metadata =
                crate::exif_processing::load_sidecar_with_exif(sidecar_path, source_path);

            if metadata.adjustments.is_null() {
                metadata.adjustments = default_adjustments();
            }

            if let Some(obj) = metadata.adjustments.as_object_mut() {
                obj.insert("lensCorrectionMode".to_string(), serde_json::json!("auto"));
                obj.insert("lensDistortionEnabled".to_string(), serde_json::json!(true));
                obj.insert("lensTcaEnabled".to_string(), serde_json::json!(true));
                obj.insert("lensVignetteEnabled".to_string(), serde_json::json!(true));
            }

            resolve_lens_params_in_adjustments(
                &mut metadata.adjustments,
                &metadata.exif,
                lens_db.as_deref(),
            );
            metadata
        });
    })
    .await
    .map_err(|e| e.to_string())
}

/// Merge `incoming` adjustment keys into `target`, deep-merging only `sectionVisibility`
/// so that visibility flags set elsewhere are preserved.
fn merge_adjustment_values(
    target: &mut serde_json::Map<String, Value>,
    incoming: &serde_json::Map<String, Value>,
) {
    for (k, v) in incoming {
        if k == "sectionVisibility" {
            if let Some(existing_vis) = target.get_mut(k) {
                if let (Some(existing_map), Some(new_map)) =
                    (existing_vis.as_object_mut(), v.as_object())
                {
                    for (vk, vv) in new_map {
                        existing_map.insert(vk.clone(), vv.clone());
                    }
                } else {
                    target.insert(k.clone(), v.clone());
                }
            } else {
                target.insert(k.clone(), v.clone());
            }
        } else {
            target.insert(k.clone(), v.clone());
        }
    }
}

/// then the preset, then automatic lens correction when auto ran or the preset asks for it.
/// The edit Auto adjust leaves: the auto analysis of `image`, then automatic lens correction.
fn fold_auto_edits(
    adjustments: &mut Value,
    image: &DynamicImage,
    source_path: &Path,
    lens_db: Option<&crate::lens_correction::LensDatabase>,
) {
    let auto_json = auto_results_to_json(&perform_auto_analysis(image));
    if let (Some(target), Some(incoming)) = (adjustments.as_object_mut(), auto_json.as_object()) {
        merge_adjustment_values(target, incoming);
    }

    if let Some(map) = adjustments.as_object_mut() {
        map.insert("lensCorrectionMode".to_string(), serde_json::json!("auto"));
        for key in ["lensDistortionEnabled", "lensTcaEnabled", "lensVignetteEnabled"] {
            map.entry(key.to_string()).or_insert(serde_json::json!(true));
        }
    }

    if adjustments.get("lensCorrectionMode").and_then(|v| v.as_str()) == Some("auto") {
        let exif = crate::exif_processing::read_rrexif_sidecar(source_path);
        resolve_lens_params_in_adjustments(adjustments, &exif, lens_db);
    }
}

const FULL_DECODE_WORKER_THREADS: usize = 2;

async fn update_sidecars_from_images(
    paths: Vec<String>,
    app_handle: AppHandle,
    operation: &'static str,
    fast_decode: bool,
    update: impl Fn(&DynamicImage, &str, &mut Value) + Send + Sync + 'static,
) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    add_to_thumbnail_queue(&state, paths.len(), &app_handle);

    tauri::async_runtime::spawn_blocking(move || {
        let settings = load_settings(app_handle.clone()).unwrap_or_default();
        let enable_xmp_sync = settings.enable_xmp_sync.unwrap_or(false);
        let create_xmp_if_missing = settings.create_xmp_if_missing.unwrap_or(false);

        let state = app_handle.state::<AppState>();
        let thumb_cache_dir = match get_thumb_cache_dir(&app_handle) {
            Ok(dir) => dir,
            Err(e) => {
                log::warn!("Unable to initialize thumbnail cache directory: {}", e);
                for path in &paths {
                    emit_thumbnail_cache_setup_error(&app_handle, path, &e);
                }
                for _ in 0..paths.len() {
                    increment_thumbnail_progress(&state, &app_handle);
                }
                return;
            }
        };

        let gpu_context = gpu_processing::get_or_init_gpu_context(&state, &app_handle).ok();

        let process = |path: &String| {
            let loaded_image: Option<DynamicImage> = (|| -> Result<DynamicImage, String> {
                let (source_path, sidecar_path) = parse_virtual_path(path);
                let source_path_str = source_path.to_string_lossy().to_string();

                let file_bytes = read_file_mapped(&source_path).map_err(|e| e.to_string())?;
                let image = image_loader::load_base_image_from_bytes(
                    &file_bytes,
                    &source_path_str,
                    fast_decode,
                    &settings,
                    None,
                )
                .map_err(|e| e.to_string())?;

                let mut existing_metadata = crate::exif_processing::load_sidecar(&sidecar_path);

                if existing_metadata.adjustments.is_null() {
                    existing_metadata.adjustments = default_adjustments();
                }

                update(&image, &source_path_str, &mut existing_metadata.adjustments);

                let _ = crate::exif_processing::write_sidecar(
                    &sidecar_path,
                    &source_path,
                    &existing_metadata,
                );

                if enable_xmp_sync {
                    sync_metadata_to_xmp(&source_path, &existing_metadata, create_xmp_if_missing);
                }
                Ok(image)
            })()
            .map_err(|e| eprintln!("Failed to apply {} to {}: {}", operation, path, e))
            .ok();

            let result = generate_single_thumbnail_and_cache(
                path,
                &thumb_cache_dir,
                gpu_context.as_ref(),
                loaded_image.as_ref(),
                true,
                &app_handle,
                &settings,
            );

            if let Some((small_path, medium_path, rating, edit)) = result {
                emit_thumbnail_generated(
                    &app_handle,
                    path,
                    &small_path,
                    &medium_path,
                    rating,
                    edit,
                );
            }

            increment_thumbnail_progress(&state, &app_handle);
        };

        if fast_decode {
            paths.par_iter().for_each(process);
        } else if let Ok(pool) = rayon::ThreadPoolBuilder::new()
            .num_threads(FULL_DECODE_WORKER_THREADS)
            .build()
        {
            pool.install(|| paths.par_iter().for_each(process));
        } else {
            paths.iter().for_each(process);
        }
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn apply_auto_adjustments_to_paths(
    paths: Vec<String>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let lens_db = app_handle.state::<AppState>().lens_db.lock().unwrap().clone();
    update_sidecars_from_images(
        paths,
        app_handle,
        "auto adjustments",
        true,
        move |image, source_path, adjustments| {
            fold_auto_edits(adjustments, image, Path::new(source_path), lens_db.as_deref())
        },
    )
    .await
}

#[tauri::command]
pub async fn apply_auto_straighten_to_paths(
    paths: Vec<String>,
    app_handle: AppHandle,
) -> Result<(), String> {
    update_sidecars_from_images(
        paths,
        app_handle,
        "auto straighten",
        false,
        |image, source_path, adjustments| {
            crate::auto_straighten::straighten_adjustments(image, source_path, adjustments)
        },
    )
    .await
}
fn update_metadata_for_paths(
    paths: &[String],
    app_handle: &AppHandle,
    update: impl Fn(&mut ImageMetadata) + Sync,
) {
    let settings = load_settings(app_handle.clone()).unwrap_or_default();
    let enable_xmp_sync = settings.enable_xmp_sync.unwrap_or(false);
    let create_xmp_if_missing = settings.create_xmp_if_missing.unwrap_or(false);

    paths.par_iter().for_each(|path| {
        let (source_path, sidecar_path) = parse_virtual_path(path);

        let mut metadata = crate::exif_processing::load_sidecar(&sidecar_path);

        update(&mut metadata);

        let _ = crate::exif_processing::write_sidecar(&sidecar_path, &source_path, &metadata);

        if enable_xmp_sync {
            sync_metadata_to_xmp(&source_path, &metadata, create_xmp_if_missing);
        }
    });
}

#[tauri::command]
pub fn set_color_label_for_paths(
    paths: Vec<String>,
    color: Option<String>,
    app_handle: AppHandle,
) -> Result<(), String> {
    update_metadata_for_paths(&paths, &app_handle, |metadata| {
        let mut tags = metadata.tags.take().unwrap_or_default();
        tags.retain(|tag| !tag.starts_with(COLOR_TAG_PREFIX));

        if let Some(c) = &color
            && !c.is_empty()
        {
            tags.push(format!("{}{}", COLOR_TAG_PREFIX, c));
        }

        if !tags.is_empty() {
            metadata.tags = Some(tags);
        }
    });

    Ok(())
}

#[tauri::command]
pub fn set_rating_for_paths(
    paths: Vec<String>,
    rating: u8,
    app_handle: AppHandle,
) -> Result<(), String> {
    update_metadata_for_paths(&paths, &app_handle, |metadata| {
        metadata.rating = rating;
        if rating > 0 && metadata.flag == Some(ImageFlag::Reject) {
            metadata.flag = None;
        }
    });

    Ok(())
}

#[tauri::command]
pub fn set_flag_for_paths(
    paths: Vec<String>,
    flag: Option<ImageFlag>,
    app_handle: AppHandle,
) -> Result<(), String> {
    update_metadata_for_paths(&paths, &app_handle, |metadata| {
        metadata.flag = flag;
    });

    Ok(())
}

#[tauri::command]
pub fn load_metadata(path: String, app_handle: AppHandle) -> Result<ImageMetadata, String> {
    let settings = load_settings(app_handle).unwrap_or_default();
    let enable_xmp_sync = settings.enable_xmp_sync.unwrap_or(false);

    let (source_path, sidecar_path) = parse_virtual_path(&path);
    let mut metadata = crate::exif_processing::load_sidecar(&sidecar_path);

    if enable_xmp_sync && sync_metadata_from_xmp(&source_path, &mut metadata, true) {
        let _ = crate::exif_processing::write_sidecar(&sidecar_path, &source_path, &metadata);
    }

    Ok(metadata)
}

fn get_presets_path(app_handle: &AppHandle) -> Result<std::path::PathBuf, String> {
    let presets_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("presets");

    if !presets_dir.exists() {
        fs::create_dir_all(&presets_dir).map_err(|e| e.to_string())?;
    }

    Ok(presets_dir.join("presets.json"))
}

#[tauri::command]
pub fn load_presets(app_handle: AppHandle) -> Result<Vec<PresetItem>, String> {
    let path = get_presets_path(&app_handle)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_presets(presets: Vec<PresetItem>, app_handle: AppHandle) -> Result<(), String> {
    let path = get_presets_path(&app_handle)?;
    let json_string = serde_json::to_string_pretty(&presets).map_err(|e| e.to_string())?;
    crate::mods::atomic_write::write_atomic(&path, json_string).map_err(|e| e.to_string())
}

fn preset_file_display_name(file_path: &str) -> String {
    Path::new(file_path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| file_path.to_string())
}

fn collect_top_level_preset_names(items: &[PresetItem]) -> HashSet<String> {
    items
        .iter()
        .map(|item| match item {
            PresetItem::Preset(p) => p.name.clone(),
            PresetItem::Folder(f) => f.name.clone(),
        })
        .collect()
}

fn parse_preset_file(file_path: &str) -> Result<Vec<PresetItem>, String> {
    let lower_path = file_path.to_lowercase();
    let is_legacy = lower_path.ends_with(".xmp") || lower_path.ends_with(".lrtemplate");

    if !is_legacy {
        let content = fs::read_to_string(file_path)
            .map_err(|e| format!("Failed to read preset file: {}", e))?;
        let preset_file: PresetFile = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse preset file: {}", e))?;
        return Ok(preset_file.presets);
    }

    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read legacy preset file: {}", e))?;

    let xmp_content = if lower_path.ends_with(".lrtemplate") {
        if let Some(caps) = regex!(r#"(?s)s.xmp = "(.*)""#).captures(&content) {
            caps.get(1)
                .map(|m| m.as_str().replace(r#"\""#, r#"""#))
                .unwrap_or(content)
        } else {
            content
        }
    } else {
        content
    };

    let converted_preset = preset_converter::convert_xmp_to_preset(&xmp_content)?;
    Ok(vec![PresetItem::Preset(converted_preset)])
}

fn merge_imported_items(
    target: &mut Vec<PresetItem>,
    taken_names: &mut HashSet<String>,
    imported: Vec<PresetItem>,
) {
    for mut imported_item in imported {
        let original_name = match &mut imported_item {
            PresetItem::Preset(p) => {
                p.id = Uuid::new_v4().to_string();
                p.name.clone()
            }
            PresetItem::Folder(f) => {
                f.id = Uuid::new_v4().to_string();
                for child in &mut f.children {
                    child.id = Uuid::new_v4().to_string();
                }
                f.name.clone()
            }
        };

        let mut new_name = original_name.clone();
        let mut counter = 1;
        while taken_names.contains(&new_name) {
            new_name = format!("{} ({})", original_name, counter);
            counter += 1;
        }

        match &mut imported_item {
            PresetItem::Preset(p) => p.name = new_name.clone(),
            PresetItem::Folder(f) => f.name = new_name.clone(),
        }

        taken_names.insert(new_name);
        target.push(imported_item);
    }
}

fn import_preset_file_into_library(
    file_path: &str,
    app_handle: AppHandle,
) -> Result<Vec<PresetItem>, String> {
    let imported = parse_preset_file(file_path)?;

    let mut current_presets = load_presets(app_handle.clone())?;
    let mut taken_names = collect_top_level_preset_names(&current_presets);
    merge_imported_items(&mut current_presets, &mut taken_names, imported);

    save_presets(current_presets.clone(), app_handle)?;
    Ok(current_presets)
}

#[tauri::command]
pub fn handle_import_presets_from_file(
    file_path: String,
    app_handle: AppHandle,
) -> Result<Vec<PresetItem>, String> {
    import_preset_file_into_library(&file_path, app_handle)
}

#[tauri::command]
pub fn handle_import_legacy_presets_from_file(
    file_path: String,
    app_handle: AppHandle,
) -> Result<Vec<PresetItem>, String> {
    import_preset_file_into_library(&file_path, app_handle)
}

#[tauri::command]
pub fn handle_import_presets_from_files(
    file_paths: Vec<String>,
    app_handle: AppHandle,
) -> Result<PresetImportResult, String> {
    let mut current_presets = load_presets(app_handle.clone())?;
    let mut taken_names = collect_top_level_preset_names(&current_presets);

    let mut failures: Vec<PresetImportFailure> = Vec::new();
    let mut library_changed = false;

    for file_path in &file_paths {
        match parse_preset_file(file_path) {
            Ok(imported) => {
                library_changed |= !imported.is_empty();
                merge_imported_items(&mut current_presets, &mut taken_names, imported);
            }
            Err(error) => failures.push(PresetImportFailure {
                file_name: preset_file_display_name(file_path),
                error,
            }),
        }
    }

    if library_changed {
        save_presets(current_presets.clone(), app_handle)?;
    }

    Ok(PresetImportResult {
        presets: current_presets,
        failures,
    })
}

#[tauri::command]
pub fn handle_export_presets_to_file(
    presets_to_export: Vec<PresetItem>,
    file_path: String,
) -> Result<(), String> {
    let preset_file = ExportPresetFile {
        creator: "Anonymous",
        presets: &presets_to_export,
    };

    let json_string = serde_json::to_string_pretty(&preset_file)
        .map_err(|e| format!("Failed to serialize presets: {}", e))?;
    fs::write(file_path, json_string).map_err(|e| format!("Failed to write preset file: {}", e))
}

#[tauri::command]
pub fn save_community_preset(
    name: String,
    adjustments: Value,
    app_handle: AppHandle,
    include_masks: Option<bool>,
    include_crop_transform: Option<bool>,
    preset_type: Option<String>,
) -> Result<(), String> {
    let mut current_presets = load_presets(app_handle.clone())?;

    let community_folder_name = "Community";
    let community_folder_id = match current_presets.iter_mut().find(|item| {
        if let PresetItem::Folder(f) = item {
            f.name == community_folder_name
        } else {
            false
        }
    }) {
        Some(PresetItem::Folder(folder)) => folder.id.clone(),
        _ => {
            let new_folder_id = Uuid::new_v4().to_string();
            let new_folder = PresetItem::Folder(PresetFolder {
                id: new_folder_id.clone(),
                name: community_folder_name.to_string(),
                children: Vec::new(),
            });
            current_presets.insert(0, new_folder);
            new_folder_id
        }
    };

    let new_preset = Preset {
        id: Uuid::new_v4().to_string(),
        name,
        adjustments,
        include_masks,
        include_crop_transform,
        preset_type: preset_type.or(Some("style".to_string())),
    };

    if let Some(PresetItem::Folder(folder)) = current_presets.iter_mut().find(|item| {
        if let PresetItem::Folder(f) = item {
            f.id == community_folder_id
        } else {
            false
        }
    }) {
        folder.children.retain(|p| p.name != new_preset.name);
        folder.children.push(new_preset);
    }

    save_presets(current_presets, app_handle)
}

#[tauri::command]
pub fn clear_all_sidecars(root_path: String) -> Result<usize, String> {
    if !Path::new(&root_path).exists() {
        return Err(format!("Root path does not exist: {}", root_path));
    }

    let mut deleted_count = 0;
    let walker = WalkDir::new(root_path).into_iter();

    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file()
            && let Some(extension) = path.extension()
            && (extension == SIDECAR_EXTENSION || extension == "agexif")
        {
            if fs::remove_file(path).is_ok() {
                deleted_count += 1;
            } else {
                eprintln!("Failed to delete sidecar file: {:?}", path);
            }
        }
    }

    Ok(deleted_count)
}

#[tauri::command]
pub fn clear_thumbnail_cache(app_handle: AppHandle) -> Result<(), String> {
    let thumb_cache_dir = get_thumb_cache_dir(&app_handle)?;
    fs::remove_dir_all(&thumb_cache_dir)
        .map_err(|e| format!("Failed to remove thumbnail cache: {}", e))?;

    fs::create_dir_all(&thumb_cache_dir)
        .map_err(|e| format!("Failed to recreate thumbnail cache directory: {}", e))?;

    Ok(())
}

#[tauri::command]
pub fn show_in_finder(path: String) -> Result<(), String> {
    let (source_path, _) = parse_virtual_path(&path);

    #[cfg(target_os = "windows")]
    {
        let source_path_str = source_path.to_string_lossy().to_string();
        Command::new("explorer")
            .args(["/select,", &source_path_str])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "macos")]
    {
        let source_path_str = source_path.to_string_lossy().to_string();
        Command::new("open")
            .args(["-R", &source_path_str])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(parent) = source_path.parent() {
            Command::new("xdg-open")
                .arg(parent)
                .spawn()
                .map_err(|e| e.to_string())?;
        } else {
            return Err("Could not get parent directory".into());
        }
    }

    Ok(())
}

/// The files a plain delete of `paths` removes: each photo with its own files,
/// or a virtual copy's edits alone. The XMP a RAW and its JPEG share stays
/// while one of them does (mods/file_ops.rs).
pub(crate) fn files_to_delete_from_disk(paths: &[String]) -> Vec<PathBuf> {
    let mut files_to_trash = HashSet::new();
    let mut leaving = HashSet::new();

    for path_str in paths {
        let (source_path, sidecar_path) = parse_virtual_path(path_str);

        if path_str.contains(VIRTUAL_COPY) {
            if sidecar_path.exists() {
                files_to_trash.insert(sidecar_path);
            }
        } else {
            if source_path.exists() {
                match find_all_associated_files(&source_path) {
                    Ok(associated_files) => {
                        for file in associated_files {
                            files_to_trash.insert(file);
                        }
                    }
                    Err(e) => {
                        log::warn!(
                            "Could not find associated files for {}: {}",
                            source_path.display(),
                            e
                        );
                    }
                }
                leaving.insert(source_path);
            }
        }
    }

    let mut files: Vec<PathBuf> = files_to_trash
        .into_iter()
        .filter(|file| !(file_ops::is_xmp(file) && file_ops::xmp_still_used(file, &leaving)))
        .collect();
    files.sort();
    files
}

#[tauri::command]
pub fn delete_files_from_disk(paths: Vec<String>, app_handle: AppHandle) -> Result<(), String> {
    let deletions: HashSet<String> = paths.iter().cloned().collect();
    let final_paths_to_delete = files_to_delete_from_disk(&paths);

    if final_paths_to_delete.is_empty() {
        return Ok(());
    }

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    file_ops::trash_or_delete(&final_paths_to_delete);

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    for path in final_paths_to_delete {
        if path.is_file() {
            if let Err(e) = fs::remove_file(&path) {
                log::warn!("Failed to delete file {}: {}", path.display(), e);
            }
        } else if path.is_dir() {
            if let Err(e) = fs::remove_dir_all(&path) {
                log::warn!("Failed to delete directory {}: {}", path.display(), e);
            }
        }
    }

    sync_album_path_changes(&app_handle, None, Some(&deletions), None);

    Ok(())
}

#[tauri::command]
pub fn delete_files_with_associated(
    paths: Vec<String>,
    app_handle: AppHandle,
) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    let deletions: HashSet<String> = paths.iter().cloned().collect();
    // Argentum: names matched folder by folder (mods/file_ops.rs).
    let final_paths_to_delete = file_ops::files_to_delete_with_associated(&paths);

    if final_paths_to_delete.is_empty() {
        return Ok(());
    }

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    file_ops::trash_or_delete(&final_paths_to_delete);

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    for path in final_paths_to_delete {
        if path.is_file() {
            if let Err(e) = fs::remove_file(&path) {
                log::warn!("Failed to delete file {}: {}", path.display(), e);
            }
        }
    }

    sync_album_path_changes(&app_handle, None, Some(&deletions), None);

    Ok(())
}

/// The thumbnails' folder in the app's cache folder. `tauri.conf.json`'s
/// asset scope (`$APPCACHE/thumbnails/*`) names it too.
pub const THUMBNAIL_DIR: &str = "thumbnails";

/// The thumbnails' folder, made if it is missing.
pub fn get_thumb_cache_dir(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let cache_dir = app_handle
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?;
    let thumb_cache_dir = cache_dir.join(THUMBNAIL_DIR);
    if !thumb_cache_dir.exists() {
        fs::create_dir_all(&thumb_cache_dir).map_err(|e| e.to_string())?;
    }
    Ok(thumb_cache_dir)
}

pub fn get_cache_key_hash(path_str: &str) -> Option<String> {
    let (_, sidecar_path) = parse_virtual_path(path_str);

    // Read as the thumbnail itself reads it, so the two keys always agree.
    let adjustments_bytes = crate::exif_processing::peek_sidecar(&sidecar_path)
        .map(|meta| serde_json::to_vec(&meta.adjustments).unwrap_or_default())
        .unwrap_or_default();

    compute_thumbnail_cache_hash(path_str, &adjustments_bytes)
}

pub fn get_cached_or_generate_thumbnail_image(
    path_str: &str,
    app_handle: &AppHandle,
    gpu_context: Option<&GpuContext>,
) -> Result<DynamicImage> {
    let thumb_cache_dir = get_thumb_cache_dir(app_handle).map_err(|e| anyhow::anyhow!(e))?;
    let settings = load_settings(app_handle.clone()).unwrap_or_default();
    let target_width_small = settings.small_thumbnail_resolution.unwrap_or(480);
    let target_width_medium = settings.medium_thumbnail_resolution.unwrap_or(1280);

    if let Some(cache_hash) = get_cache_key_hash(path_str) {
        let cache_path = thumb_cache_dir.join(format!("{}_medium.jpg", cache_hash));

        if cache_path.exists() {
            if let Ok(image) = image::open(&cache_path) {
                return Ok(image);
            }
            eprintln!(
                "Could not open cached thumbnail, regenerating: {:?}",
                cache_path
            );
        }

        let thumb_image = generate_thumbnail_data(path_str, gpu_context, None, app_handle)?;
        if let (Ok(small_data), Ok(medium_data)) = (
            encode_thumbnail(&thumb_image, target_width_small),
            encode_thumbnail(&thumb_image, target_width_medium),
        ) {
            let _ = fs::write(
                thumb_cache_dir.join(format!("{}_small.jpg", cache_hash)),
                &small_data,
            );
            let _ = fs::write(
                thumb_cache_dir.join(format!("{}_medium.jpg", cache_hash)),
                &medium_data,
            );
        }

        Ok(thumb_image)
    } else {
        generate_thumbnail_data(path_str, gpu_context, None, app_handle)
    }
}

pub fn generate_filename_from_template(
    template: &str,
    original_path: &std::path::Path,
    sequence: usize,
    total: usize,
    file_date: &DateTime<Utc>,
) -> String {
    let stem = original_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let sequence_str = format!(
        "{:0width$}",
        sequence,
        width = total.to_string().len().max(1)
    );
    let local_date = file_date.with_timezone(&chrono::Local);
    let stem_prefix = stem.trim_end_matches(|c: char| c.is_ascii_digit());
    let filename_number = &stem[stem_prefix.len()..];

    let mut result = template.to_string();
    result = result.replace("{original_filename}", stem);
    result = result.replace("{original_filename_number}", filename_number);
    result = result.replace("{sequence}", &sequence_str);
    result = result.replace("{YYYY}", &local_date.format("%Y").to_string());
    result = result.replace("{MM}", &local_date.format("%m").to_string());
    result = result.replace("{DD}", &local_date.format("%d").to_string());
    result = result.replace("{hh}", &local_date.format("%H").to_string());
    result = result.replace("{mm}", &local_date.format("%M").to_string());

    result
}

#[tauri::command]
pub fn rename_files(
    paths: Vec<String>,
    name_template: String,
    app_handle: AppHandle,
) -> Result<Vec<String>, String> {
    let (final_new_paths, renames) = rename_files_on_disk(&paths, &name_template)?;
    sync_album_path_changes(&app_handle, Some(&renames), None, None);
    Ok(final_new_paths)
}

/// Argentum: `rename_files` without the album update, so tests can run it.
/// Returns the paths for the selection and each renamed file's old and new path.
pub(crate) fn rename_files_on_disk(
    paths: &[String],
    name_template: &str,
) -> Result<(Vec<String>, HashMap<String, String>), String> {
    if paths.is_empty() {
        return Ok((Vec::new(), HashMap::new()));
    }

    // RapidRAW 4f64511a: src-tauri/src/file_management.rs
    // An image and its virtual copies share one file, so rename each file once.
    let mut original_paths: Vec<PathBuf> = Vec::new();
    for path_str in paths {
        let (original_path, _) = parse_virtual_path(path_str);
        if !original_paths.contains(&original_path) {
            original_paths.push(original_path);
        }
    }

    let mut operations: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut renames = HashMap::new();

    for (i, original_path) in original_paths.iter().enumerate() {
        if !original_path.exists() {
            return Err(format!("File not found: {}", original_path.display()));
        }

        let parent = original_path
            .parent()
            .ok_or("Could not get parent directory")?;
        let extension = original_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        let file_date = exif_processing::get_creation_date_from_path(original_path);

        let new_stem = generate_filename_from_template(
            name_template,
            original_path,
            i + 1,
            original_paths.len(),
            &file_date,
        );
        let new_filename = format!("{}.{}", new_stem, extension);
        let new_path = parent.join(new_filename);

        if new_path.exists() && new_path != *original_path {
            return Err(format!(
                "A file with the name {} already exists.",
                new_path.display()
            ));
        }

        if operations.iter().any(|(other_original, other_new)| {
            *other_new == new_path && other_original != original_path
        }) {
            return Err(format!(
                "More than one file would be renamed to {}.",
                new_path.display()
            ));
        }

        operations.push((original_path.clone(), new_path));
    }

    // Argentum: each sidecar names the photo it belongs to, so it is renamed
    // only once that photo is.
    let mut sidecar_operations: Vec<(PathBuf, PathBuf, PathBuf)> = Vec::new();
    for (original_path, new_path) in &operations {
        let parent = original_path
            .parent()
            .ok_or("Could not get parent directory")?;
        let (Some(original_filename), Some(new_filename)) =
            (original_path.file_name(), new_path.file_name())
        else {
            continue;
        };
        let original_filename_str = original_filename.to_string_lossy();
        let new_filename_str = new_filename.to_string_lossy();

        if let Ok(entries) = fs::read_dir(parent) {
            for entry in entries.filter_map(Result::ok) {
                let entry_path = entry.path();
                let entry_os_filename = entry.file_name();
                let entry_filename = entry_os_filename.to_string_lossy();

                if file_ops::is_sidecar_of(&entry_filename, &original_filename_str)
                    || file_ops::is_look_file_of(&entry_filename, &original_filename_str)
                {
                    let new_sidecar_filename =
                        entry_filename.replacen(&*original_filename_str, &new_filename_str, 1);
                    let new_sidecar_path = parent.join(new_sidecar_filename);
                    sidecar_operations.push((original_path.clone(), entry_path, new_sidecar_path));
                }
            }
        }

        let mut old_rrexif_name = original_filename.to_os_string();
        old_rrexif_name.push(".agexif");
        let old_rrexif = original_path.with_file_name(old_rrexif_name);

        if old_rrexif.exists() {
            let mut new_rrexif_name = new_filename.to_os_string();
            new_rrexif_name.push(".agexif");
            let new_rrexif = new_path.with_file_name(new_rrexif_name);
            sidecar_operations.push((original_path.clone(), old_rrexif, new_rrexif));
        }
    }

    // Argentum: the XMP is named after the photo's stem (IMG.xmp), so it is
    // carried to each renamed photo's new stem, and kept or copied when a
    // photo that kept the old name shares it (mods/file_ops.rs).
    let mut xmp_targets: Vec<(PathBuf, PathBuf, PathBuf)> = Vec::new();
    for (original_path, new_path) in &operations {
        if let Some(xmp) = resolve_xmp_path(original_path) {
            let target = new_path.with_extension(xmp.extension().unwrap_or_default());
            if target != xmp {
                xmp_targets.push((original_path.clone(), xmp, target));
            }
        }
    }

    let mut rename = |old_path: &Path, new_path: &Path| -> bool {
        if let Err(e) = fs::rename(old_path, new_path) {
            log::warn!(
                "Failed to rename {} to {}: {}",
                old_path.display(),
                new_path.display(),
                e
            );
            return false;
        }
        renames.insert(
            old_path.to_string_lossy().into_owned(),
            new_path.to_string_lossy().into_owned(),
        );
        true
    };

    let mut renamed_photos = HashSet::new();
    for (old_path, new_path) in &operations {
        if rename(old_path, new_path) {
            renamed_photos.insert(old_path);
        }
    }
    // Argentum: a photo that kept its name (open in another program, say)
    // keeps its edits under that name.
    for (photo, old_path, new_path) in &sidecar_operations {
        if renamed_photos.contains(photo) {
            rename(old_path, new_path);
        }
    }
    let mut xmps: Vec<&PathBuf> = Vec::new();
    for (_, xmp, _) in &xmp_targets {
        if !xmps.contains(&xmp) {
            xmps.push(xmp);
        }
    }
    for xmp in xmps {
        let targets: Vec<PathBuf> = xmp_targets
            .iter()
            .filter(|(photo, from, _)| from == xmp && renamed_photos.contains(photo))
            .map(|(_, _, target)| target.clone())
            .collect();
        let still_used = file_ops::xmp_still_used(xmp, &HashSet::new());
        for (target, carry) in file_ops::xmp_after_rename(&targets, still_used) {
            if target.exists() {
                log::warn!("Not carrying {} to {}: a file is there", xmp.display(), target.display());
                continue;
            }
            let carried = match carry {
                file_ops::Carry::Copy => fs::copy(xmp, &target).map(|_| ()),
                file_ops::Carry::Move => fs::rename(xmp, &target),
            };
            if let Err(e) = carried {
                log::warn!("Failed to carry {} to {}: {}", xmp.display(), target.display(), e);
            }
        }
    }

    let final_new_paths = file_ops::paths_after_rename(paths, &renames);
    Ok((final_new_paths, renames))
}

#[tauri::command]
pub fn create_virtual_copy(
    source_virtual_path: String,
    target_album_id: Option<String>,
    app_handle: AppHandle,
) -> Result<String, String> {
    let (source_path, source_sidecar_path) = parse_virtual_path(&source_virtual_path);

    let new_copy_id = Uuid::new_v4().to_string()[..6].to_string();
    let new_virtual_path = format!(
        "{}{VIRTUAL_COPY}{}",
        source_path.to_string_lossy(),
        new_copy_id
    );
    let (_, new_sidecar_path) = parse_virtual_path(&new_virtual_path);

    let default_json =
        serde_json::to_string_pretty(&ImageMetadata::default()).map_err(|e| e.to_string())?;

    {
        let _layout = crate::exif_processing::SIDECAR_LAYOUT_LOCK.lock().unwrap();

        if source_sidecar_path.exists() {
            fs::copy(&source_sidecar_path, &new_sidecar_path)
                .map_err(|e| format!("Failed to copy sidecar file: {}", e))?;
        } else {
            crate::mods::atomic_write::write_atomic(&new_sidecar_path, &default_json)
                .map_err(|e| e.to_string())?;
        }

        // The library hides the original when only virtual copy sidecars exist.
        let primary_sidecar_path = crate::file_management::sidecar_for(&source_path, None);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&primary_sidecar_path)
        {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(default_json.as_bytes())
                    .map_err(|e| e.to_string())?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
    }

    if let Some(album_id) = target_album_id {
        let _ = add_to_album(album_id, vec![new_virtual_path.clone()], app_handle);
    }

    Ok(new_virtual_path)
}

pub fn extract_xmp_rating(content: &str) -> Option<i8> {
    if let Some(idx) = content.find("xmp:Rating=\"") {
        let start = idx + 12;
        let end = content[start..].find('"').map(|i| start + i)?;
        return content[start..end].parse().ok();
    }
    if let Some(idx) = content.find("<xmp:Rating>") {
        let start = idx + 12;
        let end = content[start..].find('<').map(|i| start + i)?;
        return content[start..end].parse().ok();
    }
    None
}

const XMP_REJECTED_RATING: i8 = -1;

pub fn xmp_rating(metadata: &ImageMetadata) -> String {
    if metadata.flag == Some(ImageFlag::Reject) {
        XMP_REJECTED_RATING.to_string()
    } else {
        metadata.rating.to_string()
    }
}

pub fn xmp_label(tags: &[String]) -> Option<String> {
    tags.iter()
        .rev()
        .find_map(|t| t.strip_prefix(COLOR_TAG_PREFIX))
        .map(|color| {
            let mut c = color.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
}

pub fn xmp_subject_bag<S: AsRef<str>>(items: &[S]) -> String {
    let mut bag = String::from("<dc:subject>\n    <rdf:Bag>\n");
    for item in items {
        bag.push_str(&format!("     <rdf:li>{}</rdf:li>\n", item.as_ref()));
    }
    bag.push_str("    </rdf:Bag>\n   </dc:subject>");
    bag
}

pub fn extract_xmp_label(content: &str) -> Option<String> {
    if let Some(idx) = content.find("xmp:Label=\"") {
        let start = idx + 11;
        let end = content[start..].find('"').map(|i| start + i)?;
        return Some(content[start..end].to_string());
    }
    if let Some(idx) = content.find("<xmp:Label>") {
        let start = idx + 11;
        let end = content[start..].find('<').map(|i| start + i)?;
        return Some(content[start..end].to_string());
    }
    None
}

pub fn extract_xmp_tags(content: &str) -> Vec<String> {
    let mut tags = Vec::new();
    if let Some(start_idx) = content.find("<dc:subject>")
        && let Some(end_idx) = content[start_idx..].find("</dc:subject>")
    {
        let subject_block = &content[start_idx..start_idx + end_idx];
        let mut current_idx = 0;
        while let Some(li_start) = subject_block[current_idx..].find("<rdf:li>") {
            let val_start = current_idx + li_start + 8;
            if let Some(li_end) = subject_block[val_start..].find("</rdf:li>") {
                tags.push(subject_block[val_start..val_start + li_end].to_string());
                current_idx = val_start + li_end + 9;
            } else {
                break;
            }
        }
    }
    tags
}

pub fn resolve_xmp_path(image_path: &Path) -> Option<PathBuf> {
    let xmp_path = image_path.with_extension("xmp");
    let xmp_path_upper = image_path.with_extension("XMP");
    if xmp_path.exists() {
        Some(xmp_path)
    } else if xmp_path_upper.exists() {
        Some(xmp_path_upper)
    } else {
        None
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Scan an image file for an embedded XMP packet (`<?xpacket begin=…?> … <?xpacket end=…?>`).
/// Cameras like Sony and phone exports store the rating/label there instead of a `.xmp` sidecar.
/// Reads sequentially in chunks and stops as soon as the packet is complete.
fn read_embedded_xmp_packet(source_path: &Path) -> Option<String> {
    const CHUNK_SIZE: usize = 64 * 1024;
    const MAX_XMP_PACKET_SIZE: usize = 16 * 1024 * 1024;
    const XMP_START: &[u8] = b"<?xpacket begin=";
    const XMP_END: &[u8] = b"<?xpacket end=";
    const XMP_CLOSE: &[u8] = b"?>";

    let mut file = fs::File::open(source_path).ok()?;
    let mut chunk = vec![0u8; CHUNK_SIZE];
    let mut pending = Vec::new();
    let mut packet: Option<Vec<u8>> = None;

    loop {
        let read = file.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }

        if let Some(packet_bytes) = packet.as_mut() {
            packet_bytes.extend_from_slice(&chunk[..read]);
            if packet_bytes.len() > MAX_XMP_PACKET_SIZE {
                return None;
            }
            if let Some(end_idx) = find_bytes(packet_bytes, XMP_END)
                && let Some(close_idx) = find_bytes(&packet_bytes[end_idx..], XMP_CLOSE)
            {
                let packet_end = end_idx + close_idx + XMP_CLOSE.len();
                packet_bytes.truncate(packet_end);
                return String::from_utf8(std::mem::take(packet_bytes)).ok();
            }
        } else {
            pending.extend_from_slice(&chunk[..read]);
            if let Some(start_idx) = find_bytes(&pending, XMP_START) {
                let mut packet_bytes = pending.split_off(start_idx);
                pending.clear();

                if let Some(end_idx) = find_bytes(&packet_bytes, XMP_END)
                    && let Some(close_idx) = find_bytes(&packet_bytes[end_idx..], XMP_CLOSE)
                {
                    let packet_end = end_idx + close_idx + XMP_CLOSE.len();
                    packet_bytes.truncate(packet_end);
                    return String::from_utf8(packet_bytes).ok();
                }

                if packet_bytes.len() > MAX_XMP_PACKET_SIZE {
                    return None;
                }
                packet = Some(packet_bytes);
            } else {
                let keep = XMP_START.len().saturating_sub(1);
                if pending.len() > keep {
                    pending.drain(..pending.len() - keep);
                }
            }
        }
    }
}

pub fn sync_metadata_from_xmp(
    source_path: &Path,
    metadata: &mut ImageMetadata,
    scan_embedded: bool,
) -> bool {
    let content_opt: Option<String> = match resolve_xmp_path(source_path) {
        Some(xmp_file) => fs::read_to_string(&xmp_file).ok(),
        None if scan_embedded => read_embedded_xmp_packet(source_path),
        None => None,
    };

    let mut changed = false;

    if let Some(content) = content_opt {
        let xmp_rating = extract_xmp_rating(&content);

        if xmp_rating == Some(XMP_REJECTED_RATING) && metadata.flag.is_none() {
            metadata.flag = Some(ImageFlag::Reject);
            changed = true;
        }

        if metadata.rating == 0
            && let Some(rating) = xmp_rating.and_then(|r| u8::try_from(r).ok())
            && rating != 0
        {
            metadata.rating = rating;
            if !metadata.adjustments.is_object() {
                metadata.adjustments = default_adjustments();
            }
            if let Some(obj) = metadata.adjustments.as_object_mut() {
                obj.insert("rating".to_string(), serde_json::json!(rating));
            }
            changed = true;
        }

        let xmp_label = extract_xmp_label(&content);
        let xmp_tags = extract_xmp_tags(&content);

        let mut current_tags = metadata.tags.clone().unwrap_or_default();
        let original_len = current_tags.len();
        let had_no_tags = metadata.tags.is_none();

        for tag in xmp_tags {
            // RapidRAW fefb6a1b: src-tauri/src/file_management.rs (unescape on read, in mods/file_ops.rs)
            let tag = file_ops::xmp_text_read(&tag);
            if !current_tags.contains(&tag) {
                current_tags.push(tag);
            }
        }

        if let Some(label) = xmp_label {
            let label = file_ops::xmp_text_read(&label);
            let label_tag = format!("{}{}", COLOR_TAG_PREFIX, label.to_lowercase());
            if !current_tags.contains(&label_tag) {
                current_tags.retain(|t| !t.starts_with(COLOR_TAG_PREFIX));
                current_tags.push(label_tag);
            }
        }

        if current_tags.len() != original_len || (had_no_tags && !current_tags.is_empty()) {
            metadata.tags = Some(current_tags);
            changed = true;
        }
    }
    changed
}

pub fn sync_metadata_to_xmp(source_path: &Path, metadata: &ImageMetadata, create_if_missing: bool) {
    let xmp_path = source_path.with_extension("xmp");
    let xmp_path_upper = source_path.with_extension("XMP");

    let mut actual_xmp = if xmp_path.exists() {
        Some(xmp_path.clone())
    } else if xmp_path_upper.exists() {
        Some(xmp_path_upper.clone())
    } else {
        None
    };

    if actual_xmp.is_none() {
        if !create_if_missing {
            return;
        }
        let skeleton = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="RapidRAW">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:dc="http://purl.org/dc/elements/1.1/">
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#;
        if let Err(e) = fs::write(&xmp_path, skeleton) {
            log::error!("Failed to create skeleton XMP: {}", e);
            return;
        }
        actual_xmp = Some(xmp_path);
    }

    if let Some(xmp_file) = actual_xmp
        && let Ok(mut content) = fs::read_to_string(&xmp_file)
    {
        let rating_str = xmp_rating(metadata);
        let re_rating_attr = regex!(r#"xmp:Rating\s*=\s*"[^"]*""#);
        let re_rating_tag = regex!(r#"<xmp:Rating\s*>[^<]*</xmp:Rating>"#);

        if re_rating_attr.is_match(&content) {
            content = re_rating_attr
                .replace(&content, format!("xmp:Rating=\"{}\"", rating_str))
                .to_string();
        } else if re_rating_tag.is_match(&content) {
            content = re_rating_tag
                .replace(&content, format!("<xmp:Rating>{}</xmp:Rating>", rating_str))
                .to_string();
        } else if let Some(last_index) = content.rfind("</rdf:Description>") {
            let (start, end) = content.split_at(last_index);
            content = format!("{} <xmp:Rating>{}</xmp:Rating>\n{}", start, rating_str, end);
        }

        let current_tags = metadata.tags.clone().unwrap_or_default();
        let label = xmp_label(&current_tags);
        let normal_tags: Vec<String> = current_tags
            .into_iter()
            .filter(|t| !t.starts_with(COLOR_TAG_PREFIX))
            .collect();

        if let Some(lbl) = label {
            // RapidRAW fefb6a1b: src-tauri/src/file_management.rs (escaped, inserted literally)
            let lbl = file_ops::xmp_text(&lbl);
            let re_label_attr = regex!(r#"xmp:Label\s*=\s*"[^"]*""#);
            let re_label_tag = regex!(r#"<xmp:Label\s*>[^<]*</xmp:Label>"#);

            if re_label_attr.is_match(&content) {
                content = re_label_attr
                    .replace(&content, NoExpand(&format!("xmp:Label=\"{}\"", lbl)))
                    .to_string();
            } else if re_label_tag.is_match(&content) {
                content = re_label_tag
                    .replace(
                        &content,
                        NoExpand(&format!("<xmp:Label>{}</xmp:Label>", lbl)),
                    )
                    .to_string();
            } else if let Some(last_index) = content.rfind("</rdf:Description>") {
                let (start, end) = content.split_at(last_index);
                content = format!("{} <xmp:Label>{}</xmp:Label>\n{}", start, lbl, end);
            }
        } else {
            let re_label_attr = regex!(r#"\s*xmp:Label\s*=\s*"[^"]*""#);
            let re_label_tag = regex!(r#"\s*<xmp:Label\s*>[^<]*</xmp:Label>"#);
            content = re_label_attr.replace_all(&content, "").to_string();
            content = re_label_tag.replace_all(&content, "").to_string();
        }

        let re_subject = regex!(r#"(?s)<dc:subject>\s*<rdf:Bag>.*?</rdf:Bag>\s*</dc:subject>"#);
        if normal_tags.is_empty() {
            content = re_subject.replace_all(&content, "").to_string();
        } else {
            // RapidRAW fefb6a1b: src-tauri/src/file_management.rs (escaped, inserted literally)
            let escaped: Vec<Cow<str>> =
                normal_tags.iter().map(|t| file_ops::xmp_text(t)).collect();
            let bag = xmp_subject_bag(&escaped);

            if re_subject.is_match(&content) {
                content = re_subject.replace(&content, NoExpand(&bag)).to_string();
            } else if let Some(last_index) = content.rfind("</rdf:Description>") {
                let (start, end) = content.split_at(last_index);
                content = format!("{} {}\n  {}", start, bag, end);
            }
        }

        if let Err(e) = crate::mods::atomic_write::write_atomic(&xmp_file, content) {
            log::warn!("Could not update {}: {}", xmp_file.display(), e);
        }
    }
}

#[cfg(test)]
mod folder_tree_count_tests {
    use super::*;

    #[test]
    fn a_look_file_is_not_counted_in_the_folder_tree() {
        let dir = tempfile::tempdir().unwrap();
        let day = dir.path().join("day");
        fs::create_dir_all(&day).unwrap();
        for folder in [dir.path(), day.as_path()] {
            fs::write(folder.join("IMG.CR2"), b"").unwrap();
            fs::write(folder.join("IMG.CR2.cam1.abc.lut.png"), b"").unwrap();
        }
        let tree = |prefetch| scan_dir_lazy(dir.path(), &HashSet::new(), true, prefetch).unwrap();

        // Its own files, then a subfolder read for the count (`prefetch`) and
        // one walked for it.
        let (children, own) = tree(true);
        assert_eq!((own, children[0].image_count), (1, 1));
        let (children, _) = tree(false);
        assert_eq!(children[0].image_count, 1);
    }
}

#[cfg(test)]
mod sidecar_name_tests {
    use super::*;

    #[test]
    fn a_photo_and_its_virtual_copies_name_their_sidecars() {
        let photo = Path::new("/p/IMG.CR2");
        assert_eq!(sidecar_for(photo, None), Path::new("/p/IMG.CR2.agdata"));
        assert_eq!(
            sidecar_for(photo, Some("a1b2c3")),
            Path::new("/p/IMG.CR2.a1b2c3.agdata")
        );
    }

    #[test]
    fn a_sidecar_name_reads_back_as_its_photo_and_copy() {
        for copy in [None, Some("a1b2c3")] {
            let sidecar = sidecar_for(Path::new("/p/IMG.CR2"), copy);
            let name = sidecar.file_name().unwrap().to_str().unwrap();
            assert_eq!(parse_sidecar_name(name), Some(("IMG.CR2", copy)));
        }
    }

    #[test]
    fn a_virtual_path_splits_into_the_photo_and_its_copy() {
        assert_eq!(
            split_virtual_path("/p/IMG.CR2?vc=a1b2c3"),
            ("/p/IMG.CR2", Some("a1b2c3"))
        );
        assert_eq!(split_virtual_path("/p/IMG.CR2"), ("/p/IMG.CR2", None));
        assert_eq!(
            parse_virtual_path(&format!("/p/IMG.CR2{VIRTUAL_COPY}a1b2c3")),
            (
                PathBuf::from("/p/IMG.CR2"),
                PathBuf::from("/p/IMG.CR2.a1b2c3.agdata")
            )
        );
    }
}

#[cfg(test)]
mod xmp_embedded_tests {
    use super::*;
    use std::io::Write;

    fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("rapidraw_xmp_test_{}_{}", std::process::id(), name));
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(bytes).unwrap();
        p
    }

    #[test]
    fn reads_rating_from_embedded_xmp_packet() {
        let mut buf = vec![0u8; 200_000];
        buf.extend_from_slice(
            b"<?xpacket begin='\xef\xbb\xbf' id='W5M0MpCehiHzreSzNTczkc9d'?>\n\
              <x:xmpmeta xmlns:x='adobe:ns:meta/'>\n\
              <rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\n\
              <rdf:Description rdf:about='' xmlns:xmp='http://ns.adobe.com/xap/1.0/'>\n\
              <xmp:Rating>5</xmp:Rating>\n</rdf:Description>\n</rdf:RDF>\n</x:xmpmeta>\n\
              <?xpacket end='w'?>",
        );
        buf.extend_from_slice(&vec![0xffu8; 50_000]);

        let path = write_temp("rating.bin", &buf);
        let packet = read_embedded_xmp_packet(&path).expect("packet found");
        assert!(packet.contains("<xmp:Rating>5</xmp:Rating>"));
        assert_eq!(extract_xmp_rating(&packet), Some(5));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn returns_none_when_no_packet() {
        let path = write_temp("nopacket.bin", &vec![0x11u8; 400_000]);
        assert!(read_embedded_xmp_packet(&path).is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn sync_skips_embedded_scan_when_disabled() {
        let mut buf = Vec::new();
        buf.extend_from_slice(
            b"<?xpacket begin='' id='x'?><xmp:Rating>3</xmp:Rating><?xpacket end='w'?>",
        );
        let path = write_temp("gated.bin", &buf);
        let mut meta = ImageMetadata::default();
        assert!(!sync_metadata_from_xmp(&path, &mut meta, false));
        assert_eq!(meta.rating, 0);
        assert!(sync_metadata_from_xmp(&path, &mut meta, true));
        assert_eq!(meta.rating, 3);
        let _ = fs::remove_file(&path);
    }
}

#[cfg(test)]
mod batch_worker_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    #[test]
    fn every_item_is_worked_once() {
        let seen: Vec<AtomicUsize> = (0..1000).map(|_| AtomicUsize::new(0)).collect();
        run_batch_workers(0..seen.len(), |i| {
            seen[i].fetch_add(1, Ordering::SeqCst);
        });
        assert!(seen.iter().all(|n| n.load(Ordering::SeqCst) == 1));
    }

    /// No more photos at once than the batch had before (rayon's pool), so the
    /// memory held by decodes in flight does not grow.
    #[test]
    fn no_more_at_once_than_rayons_pool() {
        let (now, most) = (AtomicUsize::new(0), AtomicUsize::new(0));
        run_batch_workers(0..200, |_| {
            let running = now.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(running, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(1));
            now.fetch_sub(1, Ordering::SeqCst);
        });
        assert!(most.load(Ordering::SeqCst) <= rayon::current_num_threads());
    }

    /// Auto-sync starts a batch on every pause of a slider. Batches running at
    /// once share the pool, so together they still hold no more decodes.
    #[test]
    fn batches_at_once_share_the_pool() {
        let (now, most) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let work = |_| {
            let running = now.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(running, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(1));
            now.fetch_sub(1, Ordering::SeqCst);
        };
        std::thread::scope(|scope| {
            for _ in 0..3 {
                scope.spawn(|| run_batch_workers(0..100, work));
            }
        });
        assert!(most.load(Ordering::SeqCst) <= rayon::current_num_threads());
    }

    /// The shape the batch runs in: sidecars written on rayon's pool feed the
    /// thumbnail workers through a channel, and each thumbnail uses rayon too.
    /// It must finish, with every photo done once.
    #[test]
    fn photos_written_on_rayons_pool_all_reach_the_workers() {
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let photos: Vec<usize> = (0..300).collect();
            let done: Vec<AtomicUsize> = photos.iter().map(|_| AtomicUsize::new(0)).collect();
            let (tx, rx) = std::sync::mpsc::channel::<usize>();
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    run_batch_workers(rx.into_iter(), |i| {
                        let _: u64 = (0..1000u64).into_par_iter().sum();
                        done[i].fetch_add(1, Ordering::SeqCst);
                    });
                });
                photos.par_iter().for_each_with(tx, |tx, &i| {
                    std::thread::sleep(Duration::from_micros(200));
                    tx.send(i).unwrap();
                });
            });
            let _ = finished_tx.send(done.iter().all(|n| n.load(Ordering::SeqCst) == 1));
        });
        let all_once = finished_rx
            .recv_timeout(Duration::from_secs(60))
            .expect("the batch finished");
        assert!(all_once);
    }
}
