use crate::android_integration::is_android_content_uri;
#[cfg(target_os = "android")]
use crate::android_integration::{
    get_android_cached_lut_path, read_android_content_uri, resolve_android_content_uri_name,
};
use anyhow::anyhow;
use image::{DynamicImage, GenericImageView, Rgb, Rgb32FImage};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, copy, create_dir_all, read_dir};
use std::io::{BufRead, BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::UNIX_EPOCH;
use uuid::Uuid;

use base64::{Engine as _, engine::general_purpose};
use mozjpeg_rs::{Encoder, Preset};
use tauri::{AppHandle, Manager, State};

use crate::AppState;
use crate::cache_utils::calculate_transform_hash;
use crate::image_processing::{
    RenderRequest, get_all_adjustments_from_json, process_and_get_dynamic_image,
    resolve_tonemapper_override_from_handle,
};

#[derive(Debug, Clone)]
pub struct Lut {
    pub size: u32,
    pub data: Vec<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LutEntry {
    pub name: String,
    pub path: String,
    pub is_built_in: bool,
    pub library_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LutLibrary {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LutLibraryManifest {
    version: u32,
    libraries: Vec<LutLibrary>,
    assignments: HashMap<String, String>,
}

const LUT_LIBRARY_MANIFEST_VERSION: u32 = 1;
const UNCATEGORIZED_LIBRARY_ID: &str = "uncategorized";
static LUT_LIBRARY_MANIFEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn library_manifest_lock() -> std::sync::MutexGuard<'static, ()> {
    LUT_LIBRARY_MANIFEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LutPreviewRequest {
    pub path: String,
    pub is_built_in: bool,
}

#[derive(Serialize)]
pub struct LutParseResult {
    pub size: u32,
}

#[derive(Serialize)]
pub struct LutPreview {
    pub path: String,
    pub thumb: Option<String>,
}

fn strip_verbatim(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string())
}

fn film_luts_dir(app_handle: &AppHandle) -> Option<PathBuf> {
    app_handle
        .path()
        .resolve("resources/film_luts", tauri::path::BaseDirectory::Resource)
        .ok()
        .map(|p| strip_verbatim(&p))
}

pub fn get_luts_dir(app_data_dir: &Path) -> anyhow::Result<PathBuf> {
    let luts_dir = app_data_dir.join("luts");
    if !luts_dir.exists() {
        create_dir_all(&luts_dir)?;
    }
    Ok(luts_dir)
}

fn library_manifest_path(luts_dir: &Path) -> PathBuf {
    luts_dir.join("library.json")
}

fn default_library_manifest() -> LutLibraryManifest {
    LutLibraryManifest {
        version: LUT_LIBRARY_MANIFEST_VERSION,
        libraries: vec![LutLibrary {
            id: UNCATEGORIZED_LIBRARY_ID.to_string(),
            name: "Uncategorized".to_string(),
        }],
        assignments: HashMap::new(),
    }
}

fn parse_library_manifest(content: &str) -> anyhow::Result<LutLibraryManifest> {
    let mut manifest: LutLibraryManifest = serde_json::from_str(content)?;
    if manifest.version > LUT_LIBRARY_MANIFEST_VERSION {
        return Err(anyhow!(
            "Unsupported LUT library manifest version: {}",
            manifest.version
        ));
    }
    if manifest.version == 0 {
        manifest.version = LUT_LIBRARY_MANIFEST_VERSION;
    }
    if !manifest
        .libraries
        .iter()
        .any(|library| library.id == UNCATEGORIZED_LIBRARY_ID)
    {
        manifest.libraries.insert(
            0,
            LutLibrary {
                id: UNCATEGORIZED_LIBRARY_ID.to_string(),
                name: "Uncategorized".to_string(),
            },
        );
    }
    Ok(manifest)
}

fn load_latest_manifest_backup(luts_dir: &Path) -> Option<LutLibraryManifest> {
    let mut candidates: Vec<(std::time::SystemTime, PathBuf)> = read_dir(luts_dir)
        .ok()?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?;
            if !name.starts_with("library.json.bak.") {
                return None;
            }
            let modified = path
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .unwrap_or(UNIX_EPOCH);
            Some((modified, path))
        })
        .collect();
    candidates.sort_by_key(|(modified, _)| *modified);
    candidates.reverse();

    candidates.into_iter().find_map(|(_, path)| {
        let content = std::fs::read_to_string(path).ok()?;
        parse_library_manifest(&content).ok()
    })
}

fn declares_unsupported_manifest_version(content: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .and_then(|value| value.get("version").and_then(|version| version.as_u64()))
        .is_some_and(|version| version > LUT_LIBRARY_MANIFEST_VERSION as u64)
}

fn load_library_manifest(luts_dir: &Path) -> anyhow::Result<LutLibraryManifest> {
    let path = library_manifest_path(luts_dir);
    match std::fs::read_to_string(&path) {
        Ok(content) => match parse_library_manifest(&content) {
            Ok(manifest) => Ok(manifest),
            Err(primary_error) if declares_unsupported_manifest_version(&content) => {
                Err(primary_error)
            }
            Err(primary_error) => load_latest_manifest_backup(luts_dir)
                .ok_or(primary_error),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(load_latest_manifest_backup(luts_dir).unwrap_or_else(default_library_manifest))
        }
        Err(error) => Err(error.into()),
    }
}

fn save_library_manifest(luts_dir: &Path, manifest: &LutLibraryManifest) -> anyhow::Result<()> {
    let path = library_manifest_path(luts_dir);
    let suffix = Uuid::new_v4();
    let temp_path = luts_dir.join(format!("library.json.tmp.{}", suffix));
    let backup_path = luts_dir.join(format!("library.json.bak.{}", suffix));
    let json = serde_json::to_string_pretty(manifest)?;
    std::fs::write(&temp_path, json)?;

    if path.exists() {
        std::fs::rename(&path, &backup_path)?;
        if let Err(rename_error) = std::fs::rename(&temp_path, &path) {
            let _ = std::fs::rename(&backup_path, &path);
            let _ = std::fs::remove_file(&temp_path);
            return Err(rename_error.into());
        }
        let _ = std::fs::remove_file(&backup_path);
    } else if let Err(rename_error) = std::fs::rename(&temp_path, &path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(rename_error.into());
    }
    Ok(())
}

fn library_id_exists(manifest: &LutLibraryManifest, library_id: &str) -> bool {
    manifest
        .libraries
        .iter()
        .any(|library| library.id == library_id)
}

fn ensure_lut_assignments(
    entries: &mut [LutEntry],
    manifest: &mut LutLibraryManifest,
    luts_dir: &Path,
) -> bool {
    let mut changed = false;
    for entry in entries.iter_mut().filter(|entry| !entry.is_built_in) {
        let key = manifest_key(luts_dir, &entry.path);
        if !manifest.assignments.contains_key(&key)
            && let Some(legacy_assignment) = manifest.assignments.remove(&entry.path)
        {
            manifest.assignments.insert(key.clone(), legacy_assignment);
            changed = true;
        }
        let assignment = manifest
            .assignments
            .entry(key.clone())
            .or_insert_with(|| {
                changed = true;
                UNCATEGORIZED_LIBRARY_ID.to_string()
            })
            .clone();

        let library_id = if library_id_exists(manifest, &assignment) {
            assignment
        } else {
            changed = true;
            manifest
                .assignments
                .insert(key, UNCATEGORIZED_LIBRARY_ID.to_string());
            UNCATEGORIZED_LIBRARY_ID.to_string()
        };
        entry.library_id = Some(library_id);
    }
    changed
}

pub fn list_luts_in_dir(dir: &Path, is_built_in: bool) -> anyhow::Result<Vec<LutEntry>> {
    let mut entries: Vec<LutEntry> = Vec::new();
    if !dir.exists() {
        return Ok(entries);
    }
    for entry in read_dir(dir)? {
        let path = entry?.path();
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        let is_supported = matches!(
            extension.as_str(),
            "cube" | "3dl" | "png" | "jpg" | "jpeg" | "tiff"
        );

        if is_supported {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("LUT")
                .to_string();

            entries.push(LutEntry {
                name,
                path: strip_verbatim(&path).to_string_lossy().into_owned(),
                is_built_in,
                library_id: None,
            });
        }
    }
    entries.sort_by_key(|a| a.name.to_lowercase());
    Ok(entries)
}

fn unique_lut_destination(dir: &Path, stem: &str, extension: &str) -> PathBuf {
    let mut candidate = dir.join(format!("{}.{}", stem, extension));
    let mut suffix = 1;
    while candidate.exists() && suffix < 1000 {
        candidate = dir.join(format!("{} ({}).{}", stem, suffix, extension));
        suffix += 1;
    }
    candidate
}

pub fn import_luts_to_dir(dir: &Path, source_paths: &[String]) -> anyhow::Result<Vec<String>> {
    let mut imported_paths = Vec::new();
    for source in source_paths {
        if let Err(error) = parse_lut_file(source) {
            log::warn!("Skipping invalid LUT '{}': {}", source, error);
            continue;
        }

        #[cfg(target_os = "android")]
        if is_android_content_uri(source) {
            match import_android_lut(source) {
                Ok(path) => {
                    imported_paths.push(strip_verbatim(&path).to_string_lossy().into_owned())
                }
                Err(error) => {
                    log::error!("Failed to import LUT from '{}': {}", source, error);
                }
            }
            continue;
        }

        let source_path = Path::new(source);
        let stem = source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("LUT");
        let extension = source_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("cube")
            .to_lowercase();
        let destination = unique_lut_destination(dir, stem, &extension);
        match copy(source_path, &destination) {
            Ok(_) => {
                imported_paths.push(strip_verbatim(&destination).to_string_lossy().into_owned())
            }
            Err(error) => {
                log::error!("Failed to copy LUT '{}': {}", source, error);
            }
        }
    }
    Ok(imported_paths)
}

#[cfg(target_os = "android")]
fn import_android_lut(source: &str) -> anyhow::Result<PathBuf> {
    let resolved_name = resolve_android_content_uri_name(source)
        .map_err(|e| anyhow!("Failed to resolve content URI: {}", e))?;
    let stem = Path::new(&resolved_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("LUT")
        .to_string();
    let extension = Path::new(&resolved_name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("cube")
        .to_lowercase();
    let bytes = read_android_content_uri(source)
        .map_err(|e| anyhow!("Failed to read content URI: {}", e))?;

    let cache_path = get_android_cached_lut_path(source, &extension)?;
    let cache_dir = cache_path
        .parent()
        .ok_or_else(|| anyhow!("Invalid cache path"))?
        .to_path_buf();
    let destination = unique_lut_destination(&cache_dir, &stem, &extension);
    std::fs::write(&destination, &bytes)?;
    Ok(destination)
}

fn parse_cube(reader: impl BufRead) -> anyhow::Result<Lut> {
    let mut size: Option<u32> = None;
    let mut data: Vec<f32> = Vec::new();
    let mut line_num = 0;

    for line in reader.lines() {
        line_num += 1;
        let line = line?;
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0].to_uppercase().as_str() {
            "TITLE" | "DOMAIN_MIN" | "DOMAIN_MAX" => continue,

            "LUT_3D_SIZE" => {
                if parts.len() < 2 {
                    return Err(anyhow!(
                        "Malformed LUT_3D_SIZE on line {}: '{}'",
                        line_num,
                        line
                    ));
                }
                size = Some(parts[1].parse().map_err(|e| {
                    anyhow!(
                        "Failed to parse LUT_3D_SIZE on line {}: '{}'. Error: {}",
                        line_num,
                        line,
                        e
                    )
                })?);
            }
            _ => {
                if size.is_some() {
                    if parts.len() < 3 {
                        return Err(anyhow!(
                            "Invalid data line on line {}: '{}'. Expected 3 float values, found {}",
                            line_num,
                            line,
                            parts.len()
                        ));
                    }
                    let r: f32 = parts[0].parse().map_err(|e| {
                        anyhow!(
                            "Failed to parse R value on line {}: '{}'. Error: {}",
                            line_num,
                            line,
                            e
                        )
                    })?;
                    let g: f32 = parts[1].parse().map_err(|e| {
                        anyhow!(
                            "Failed to parse G value on line {}: '{}'. Error: {}",
                            line_num,
                            line,
                            e
                        )
                    })?;
                    let b: f32 = parts[2].parse().map_err(|e| {
                        anyhow!(
                            "Failed to parse B value on line {}: '{}'. Error: {}",
                            line_num,
                            line,
                            e
                        )
                    })?;
                    data.push(r);
                    data.push(g);
                    data.push(b);
                }
            }
        }
    }

    let lut_size = size.ok_or(anyhow!("LUT_3D_SIZE not found in .cube file"))?;
    let expected_len = (lut_size * lut_size * lut_size * 3) as usize;
    if data.len() != expected_len {
        return Err(anyhow!(
            "LUT data size mismatch. Expected {} float values (for size {}), but found {}. The file may be corrupt or incomplete.",
            expected_len,
            lut_size,
            data.len()
        ));
    }

    Ok(Lut {
        size: lut_size,
        data,
    })
}

fn parse_3dl(reader: impl BufRead) -> anyhow::Result<Lut> {
    let mut data: Vec<f32> = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() == 3 {
            let r: f32 = parts[0].parse()?;
            let g: f32 = parts[1].parse()?;
            let b: f32 = parts[2].parse()?;
            data.push(r);
            data.push(g);
            data.push(b);
        }
    }

    let total_values = data.len();
    if total_values == 0 {
        return Err(anyhow!("No data found in 3DL file"));
    }
    let num_entries = total_values / 3;
    let size = (num_entries as f64).cbrt().round() as u32;

    if size * size * size != num_entries as u32 {
        return Err(anyhow!(
            "Invalid 3DL LUT data size: the number of entries ({}) is not a perfect cube.",
            num_entries
        ));
    }

    Ok(Lut { size, data })
}

fn parse_hald(image: DynamicImage) -> anyhow::Result<Lut> {
    let (width, height) = image.dimensions();
    if width != height {
        return Err(anyhow!(
            "HALD image must be square, but dimensions are {}x{}",
            width,
            height
        ));
    }

    let total_pixels = width * height;
    let size = (total_pixels as f64).cbrt().round() as u32;

    if size * size * size != total_pixels {
        return Err(anyhow!(
            "Invalid HALD image dimensions: total pixels ({}) is not a perfect cube.",
            total_pixels
        ));
    }

    let mut data = Vec::with_capacity((total_pixels * 3) as usize);
    let rgb_image = image.to_rgb8();

    for pixel in rgb_image.pixels() {
        data.push(pixel[0] as f32 / 255.0);
        data.push(pixel[1] as f32 / 255.0);
        data.push(pixel[2] as f32 / 255.0);
    }

    Ok(Lut { size, data })
}

pub fn parse_lut_file(path_str: &str) -> anyhow::Result<Lut> {
    let normalized_path_str = path_str.strip_prefix(r"\\?\").unwrap_or(path_str);

    if normalized_path_str.starts_with(r"\\") || normalized_path_str.starts_with("//") {
        return Err(anyhow!("Network paths (UNC) are not allowed for LUTs"));
    }

    if path_str.contains("..") {
        return Err(anyhow!("Directory traversal (..) is not allowed"));
    }

    let path = std::path::Path::new(path_str);
    if let Some(std::path::Component::Prefix(prefix)) = path.components().next() {
        match prefix.kind() {
            std::path::Prefix::UNC(_, _)
            | std::path::Prefix::VerbatimUNC(_, _)
            | std::path::Prefix::DeviceNS(_) => {
                return Err(anyhow!("Device/UNC prefix paths are not allowed"));
            }
            _ => {}
        }
    }

    let (extension, bytes): (String, Option<Vec<u8>>) =
        if cfg!(target_os = "android") && is_android_content_uri(path_str) {
            #[cfg(target_os = "android")]
            {
                let resolved_name = resolve_android_content_uri_name(path_str)
                    .unwrap_or_else(|_| path_str.to_string());
                let ext = Path::new(&resolved_name)
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("cube")
                    .to_lowercase();
                let uri_bytes = read_android_content_uri(path_str).map_err(|e| anyhow!("{}", e))?;
                (ext, Some(uri_bytes))
            }
            #[cfg(not(target_os = "android"))]
            {
                (String::new(), None)
            }
        } else {
            let ext = Path::new(path_str)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            (ext, None)
        };

    match extension.as_str() {
        "cube" => {
            if let Some(b) = bytes {
                parse_cube(BufReader::new(Cursor::new(b)))
            } else {
                let file = File::open(path_str)?;
                parse_cube(BufReader::new(file))
            }
        }
        "3dl" => {
            if let Some(b) = bytes {
                parse_3dl(BufReader::new(Cursor::new(b)))
            } else {
                let file = File::open(path_str)?;
                parse_3dl(BufReader::new(file))
            }
        }
        "png" | "jpg" | "jpeg" | "tiff" => {
            let img = if let Some(b) = bytes {
                image::load_from_memory(&b)?
            } else {
                image::open(path_str)?
            };
            parse_hald(img)
        }
        _ => Err(anyhow!("Unsupported LUT file format: {}", extension)),
    }
}

pub fn generate_identity_lut_image(size: u32) -> DynamicImage {
    let width = size;
    let height = size * size;
    let mut img = Rgb32FImage::new(width, height);

    for z in 0..size {
        for y in 0..size {
            for x in 0..size {
                let r = x as f32 / (size - 1) as f32;
                let g = y as f32 / (size - 1) as f32;
                let b = z as f32 / (size - 1) as f32;

                img.put_pixel(x, z * size + y, Rgb([r, g, b]));
            }
        }
    }

    DynamicImage::ImageRgb32F(img)
}

pub fn convert_image_to_cube_lut(image: &DynamicImage, size: u32) -> Result<Vec<u8>, String> {
    let f32_image = image.to_rgb32f();
    let mut out = String::new();

    out.push_str(&format!("LUT_3D_SIZE {}\n", size));
    out.push_str("DOMAIN_MIN 0.0 0.0 0.0\n");
    out.push_str("DOMAIN_MAX 1.0 1.0 1.0\n");

    for z in 0..size {
        for y in 0..size {
            for x in 0..size {
                let pixel = f32_image.get_pixel(x, z * size + y);
                out.push_str(&format!(
                    "{:.6} {:.6} {:.6}\n",
                    pixel[0].clamp(0.0, 1.0),
                    pixel[1].clamp(0.0, 1.0),
                    pixel[2].clamp(0.0, 1.0)
                ));
            }
        }
    }

    Ok(out.into_bytes())
}

pub fn get_or_load_lut(state: &State<AppState>, path: &str) -> Result<Arc<Lut>, String> {
    let mut cache = state.lut_cache.lock().unwrap();
    if let Some(lut) = cache.get(path) {
        return Ok(lut.clone());
    }

    let lut = parse_lut_file(path).map_err(|e| e.to_string())?;
    let arc_lut = Arc::new(lut);
    cache.insert(path.to_string(), arc_lut.clone());
    Ok(arc_lut)
}

#[tauri::command]
pub fn list_luts(app_handle: AppHandle) -> Result<Vec<LutEntry>, String> {
    let _manifest_guard = library_manifest_lock();
    list_luts_unlocked(&app_handle)
}

fn list_luts_unlocked(app_handle: &AppHandle) -> Result<Vec<LutEntry>, String> {
    let mut all_luts = Vec::new();

    if let Some(resource_path) = film_luts_dir(app_handle)
        && let Ok(built_in) = list_luts_in_dir(&resource_path, true)
    {
        all_luts.extend(built_in);
    }

    let data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let luts_dir = get_luts_dir(&data_dir).map_err(|e| e.to_string())?;

    #[cfg(target_os = "android")]
    {
        if let Ok(user_luts) = list_luts_in_dir(&luts_dir, false) {
            all_luts.extend(user_luts);
        }
        if let Ok(cached) = list_luts_in_cache() {
            all_luts.extend(cached);
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        if let Ok(user_luts) = list_luts_in_dir(&luts_dir, false) {
            all_luts.extend(user_luts);
        }
    }

    let mut manifest = load_library_manifest(&luts_dir).map_err(|e| e.to_string())?;
    if ensure_lut_assignments(&mut all_luts, &mut manifest, &luts_dir) {
        save_library_manifest(&luts_dir, &manifest).map_err(|e| e.to_string())?;
    }

    Ok(all_luts)
}

fn manifest_key(luts_dir: &Path, path: &str) -> String {
    let normalized_path = strip_verbatim(Path::new(path));
    let normalized_luts_dir = strip_verbatim(luts_dir);
    if let Ok(relative_path) = normalized_path.strip_prefix(&normalized_luts_dir) {
        return relative_path.to_string_lossy().replace('\\', "/");
    }
    format!(
        "external:{}",
        normalized_path.to_string_lossy().replace('\\', "/")
    )
}

#[tauri::command]
pub fn list_lut_libraries(app_handle: AppHandle) -> Result<Vec<LutLibrary>, String> {
    let _manifest_guard = library_manifest_lock();
    let _ = list_luts_unlocked(&app_handle)?;
    let data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let luts_dir = get_luts_dir(&data_dir).map_err(|e| e.to_string())?;
    let manifest = load_library_manifest(&luts_dir).map_err(|e| e.to_string())?;
    Ok(manifest.libraries)
}

#[tauri::command]
pub fn create_lut_library(app_handle: AppHandle, name: String) -> Result<LutLibrary, String> {
    let _manifest_guard = library_manifest_lock();
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return Err("Library name cannot be empty".to_string());
    }

    let data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let luts_dir = get_luts_dir(&data_dir).map_err(|e| e.to_string())?;
    let mut manifest = load_library_manifest(&luts_dir).map_err(|e| e.to_string())?;

    if manifest
        .libraries
        .iter()
        .any(|library| library.name.eq_ignore_ascii_case(trimmed_name))
    {
        return Err("A LUT library with that name already exists".to_string());
    }

    let library = LutLibrary {
        id: Uuid::new_v4().to_string(),
        name: trimmed_name.to_string(),
    };
    manifest.libraries.push(library.clone());
    save_library_manifest(&luts_dir, &manifest).map_err(|e| e.to_string())?;
    Ok(library)
}

#[tauri::command]
pub fn rename_lut_library(
    app_handle: AppHandle,
    library_id: String,
    name: String,
) -> Result<Vec<LutLibrary>, String> {
    let _manifest_guard = library_manifest_lock();
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return Err("Library name cannot be empty".to_string());
    }
    if library_id == UNCATEGORIZED_LIBRARY_ID {
        return Err("The Uncategorized library cannot be renamed".to_string());
    }

    let data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let luts_dir = get_luts_dir(&data_dir).map_err(|e| e.to_string())?;
    let mut manifest = load_library_manifest(&luts_dir).map_err(|e| e.to_string())?;
    if manifest
        .libraries
        .iter()
        .any(|library| library.id != library_id && library.name.eq_ignore_ascii_case(trimmed_name))
    {
        return Err("A LUT library with that name already exists".to_string());
    }

    let library = manifest
        .libraries
        .iter_mut()
        .find(|library| library.id == library_id)
        .ok_or_else(|| "LUT library not found".to_string())?;
    library.name = trimmed_name.to_string();
    save_library_manifest(&luts_dir, &manifest).map_err(|e| e.to_string())?;
    Ok(manifest.libraries)
}

#[tauri::command]
pub fn delete_lut_library(
    app_handle: AppHandle,
    library_id: String,
) -> Result<Vec<LutLibrary>, String> {
    let _manifest_guard = library_manifest_lock();
    if library_id == UNCATEGORIZED_LIBRARY_ID {
        return Err("The Uncategorized library cannot be deleted".to_string());
    }

    let data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let luts_dir = get_luts_dir(&data_dir).map_err(|e| e.to_string())?;
    let mut manifest = load_library_manifest(&luts_dir).map_err(|e| e.to_string())?;
    let original_len = manifest.libraries.len();
    manifest
        .libraries
        .retain(|library| library.id != library_id);
    if manifest.libraries.len() == original_len {
        return Err("LUT library not found".to_string());
    }
    for assignment in manifest.assignments.values_mut() {
        if assignment == &library_id {
            *assignment = UNCATEGORIZED_LIBRARY_ID.to_string();
        }
    }
    save_library_manifest(&luts_dir, &manifest).map_err(|e| e.to_string())?;
    Ok(manifest.libraries)
}

#[tauri::command]
pub fn set_lut_library(
    app_handle: AppHandle,
    path: String,
    library_id: String,
) -> Result<Vec<LutEntry>, String> {
    let manifest_guard = library_manifest_lock();
    let data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let luts_dir = get_luts_dir(&data_dir).map_err(|e| e.to_string())?;
    let mut manifest = load_library_manifest(&luts_dir).map_err(|e| e.to_string())?;
    if !library_id_exists(&manifest, &library_id) {
        return Err("LUT library not found".to_string());
    }
    manifest
        .assignments
        .insert(manifest_key(&luts_dir, &path), library_id);
    save_library_manifest(&luts_dir, &manifest).map_err(|e| e.to_string())?;
    drop(manifest_guard);
    list_luts(app_handle)
}

#[cfg(target_os = "android")]
fn get_lut_cache_dir() -> anyhow::Result<PathBuf> {
    let cache_path = get_android_cached_lut_path("_", "tmp")?;
    cache_path
        .parent()
        .ok_or_else(|| anyhow!("Invalid cache path"))
        .map(|p| p.to_path_buf())
}

#[cfg(target_os = "android")]
fn list_luts_in_cache() -> anyhow::Result<Vec<LutEntry>> {
    let cache_dir = get_lut_cache_dir()?;

    if !cache_dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries: Vec<LutEntry> = Vec::new();
    for entry in read_dir(&cache_dir)? {
        let path = entry?.path();
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        let is_supported = matches!(
            extension.as_str(),
            "cube" | "3dl" | "png" | "jpg" | "jpeg" | "tiff"
        );

        if is_supported {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("LUT")
                .to_string();
            entries.push(LutEntry {
                name,
                path: strip_verbatim(&path).to_string_lossy().into_owned(),
                is_built_in: false,
                library_id: None,
            });
        }
    }
    entries.sort_by_key(|a| a.name.to_lowercase());
    Ok(entries)
}

#[tauri::command]
pub fn import_luts(
    app_handle: AppHandle,
    source_paths: Vec<String>,
    library_id: Option<String>,
) -> Result<Vec<LutEntry>, String> {
    let manifest_guard = library_manifest_lock();
    let data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let luts_dir = get_luts_dir(&data_dir).map_err(|e| e.to_string())?;
    let mut manifest = load_library_manifest(&luts_dir).map_err(|e| e.to_string())?;
    let target_library_id = library_id.unwrap_or_else(|| UNCATEGORIZED_LIBRARY_ID.to_string());
    if !library_id_exists(&manifest, &target_library_id) {
        return Err("LUT library not found".to_string());
    }
    let imported_paths = import_luts_to_dir(&luts_dir, &source_paths).map_err(|e| e.to_string())?;
    for imported_path in imported_paths {
        manifest.assignments.insert(
            manifest_key(&luts_dir, &imported_path),
            target_library_id.clone(),
        );
    }
    save_library_manifest(&luts_dir, &manifest).map_err(|e| e.to_string())?;

    drop(manifest_guard);
    list_luts(app_handle)
}

#[tauri::command]
pub fn remove_lut(app_handle: AppHandle, path: String) -> Result<Vec<LutEntry>, String> {
    let manifest_guard = library_manifest_lock();
    let data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let luts_dir = get_luts_dir(&data_dir).map_err(|e| e.to_string())?;
    let normalized_luts_dir = strip_verbatim(&luts_dir);
    let target_path = strip_verbatim(Path::new(&path));

    if let Some(resource_path) = film_luts_dir(&app_handle)
        && target_path.starts_with(&resource_path)
    {
        return Err("Cannot delete built-in film emulations".to_string());
    }

    #[cfg(target_os = "android")]
    {
        let cache_dir = strip_verbatim(&get_lut_cache_dir().map_err(|e| e.to_string())?);
        if !target_path.starts_with(&normalized_luts_dir) && !target_path.starts_with(&cache_dir) {
            return Err(
                "Access denied: Cannot remove files outside the user LUT directory".to_string(),
            );
        }
    }
    #[cfg(not(target_os = "android"))]
    if !target_path.starts_with(&normalized_luts_dir) {
        return Err(
            "Access denied: Cannot remove files outside the user LUT directory".to_string(),
        );
    }

    if !target_path.exists() {
        return Err("LUT file not found".to_string());
    }

    let mut manifest = load_library_manifest(&luts_dir).map_err(|e| e.to_string())?;
    let assignment_key = manifest_key(&luts_dir, &path);
    let previous_assignment = manifest.assignments.remove(&assignment_key);
    save_library_manifest(&luts_dir, &manifest).map_err(|e| e.to_string())?;

    if let Err(delete_error) = trash::delete(&target_path) {
        if let Some(previous_assignment) = previous_assignment {
            manifest
                .assignments
                .insert(assignment_key, previous_assignment);
        }
        let _ = save_library_manifest(&luts_dir, &manifest);
        return Err(format!(
            "Failed to move LUT to the recycle bin: {}",
            delete_error
        ));
    }

    drop(manifest_guard);
    list_luts(app_handle)
}

fn render_lut_swatch(
    context: &crate::image_processing::GpuContext,
    state: &State<AppState>,
    base_image: &DynamicImage,
    transform_hash: u64,
    adjustments: crate::image_processing::AllAdjustments,
    lut_path: &str,
) -> Option<String> {
    let lut = get_or_load_lut(state, lut_path).ok()?;
    let processed = process_and_get_dynamic_image(
        context,
        state,
        base_image,
        transform_hash,
        RenderRequest {
            adjustments,
            mask_bitmaps: &[],
            lut: Some(lut),
            roi: None,
        },
        "generate_lut_previews",
    )
    .ok()?;

    let rgb = processed.to_rgb8();
    let (width, height) = rgb.dimensions();
    let bytes = Encoder::new(Preset::BaselineFastest)
        .quality(80)
        .encode_rgb(&rgb.into_vec(), width, height)
        .ok()?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        general_purpose::STANDARD.encode(&bytes)
    ))
}

#[tauri::command]
pub fn generate_lut_previews(
    luts: Vec<LutPreviewRequest>,
    size: u32,
    state: State<AppState>,
    app_handle: AppHandle,
) -> Result<Vec<LutPreview>, String> {
    let context = crate::image_processing::get_or_init_gpu_context(&state, &app_handle)?;
    let loaded_image = state
        .original_image
        .lock()
        .unwrap()
        .clone()
        .ok_or("No original image loaded for LUT previews")?;
    let is_raw = loaded_image.is_raw;

    let base_json = serde_json::json!({});
    let (base_image, _scale, _offset) =
        crate::generate_transformed_preview(&state, &loaded_image, &base_json, size)?;

    let tm_override = resolve_tonemapper_override_from_handle(&app_handle, is_raw);
    let transform_hash = calculate_transform_hash(&base_json);

    let previews = luts
        .into_iter()
        .map(|request| {
            let swatch_lut_json = serde_json::json!({
                "lutPath": "preview",
                "lutIntensity": 100,
                "lutIsSceneReferred": request.is_built_in,
                "sectionVisibility": { "effects": true }
            });
            let swatch_adjustments = get_all_adjustments_from_json(
                &swatch_lut_json,
                is_raw,
                tm_override,
                Some(loaded_image.path.as_str()),
            );

            let thumb = render_lut_swatch(
                &context,
                &state,
                &base_image,
                transform_hash,
                swatch_adjustments,
                &request.path,
            );
            LutPreview {
                path: request.path,
                thumb,
            }
        })
        .collect();

    Ok(previews)
}

#[tauri::command]
pub fn load_and_parse_lut(path: String, state: State<AppState>) -> Result<LutParseResult, String> {
    let lut = parse_lut_file(&path).map_err(|e| e.to_string())?;
    let lut_size = lut.size;

    let mut cache = state.lut_cache.lock().unwrap();
    cache.insert(path, Arc::new(lut));

    Ok(LutParseResult { size: lut_size })
}
