//! Tauri commands for Argentum's own features.

use crate::app_state::AppState;
use crate::get_cached_full_warped_image;
use crate::mods::auto_wb::{self, AutoWhiteBalance, DetectMode};

/// Solve white balance from the point the user clicked, in normalised image
/// coordinates (0..1, origin top-left).
///
/// Takes coordinates rather than a sampled colour on purpose, and this is the
/// fix for the picker never settling. The frontend used to read the pixel off
/// the *processed preview* and send that — an image with the current white
/// balance already applied, plus exposure, curves and everything else. Each
/// click therefore solved from its own previous output, moved the sliders, and
/// changed what the next click would see. Clicking one spot repeatedly wandered
/// forever instead of converging.
///
/// Sampling here uses the geometry-only cache, keyed on crop and rotation, which
/// no colour slider can move. Same point, same answer. It is also the exact
/// image auto-WB analyses, so the wand and the picker finally agree.
pub async fn solve_white_balance_at_point(
    x: f32,
    y: f32,
    js_adjustments: serde_json::Value,
    state: tauri::State<'_, AppState>,
) -> Result<AutoWhiteBalance, String> {
    let image = get_cached_full_warped_image(&state, &js_adjustments)?;

    let result = auto_wb::white_balance_at(image.as_ref(), x, y)
        .ok_or_else(|| "That colour is too dark or too saturated to balance from".to_string())?;

    log::info!(
        "[wb_picker] ({:.3}, {:.3}) -> illuminant xy ({:.4}, {:.4}) ~{:.0}K -> temp {:.1} tint {:.1}",
        x,
        y,
        result.x,
        result.y,
        result.temperature_k,
        result.temperature,
        result.tint
    );

    Ok(result)
}

/// Detect the scene illuminant and return the white balance that neutralises it.
///
/// Runs on the geometry-corrected image already cached for the editor, so it
/// sees the same pixels you do — crop and rotation included.
pub async fn detect_auto_white_balance(
    js_adjustments: serde_json::Value,
    mode: DetectMode,
    state: tauri::State<'_, AppState>,
) -> Result<AutoWhiteBalance, String> {
    let image = get_cached_full_warped_image(&state, &js_adjustments)?;

    // That image has already been through `apply_cpu_default_raw_processing`,
    // which gamma-encodes and boosts contrast. The detection needs scene-linear
    // data, so ask it to undo that first — see `to_scene_linear`.
    let started = std::time::Instant::now();
    let result = auto_wb::auto_white_balance(image.as_ref(), mode, true)
        .ok_or_else(|| "Could not detect an illuminant in this image".to_string())?;

    log::info!(
        "[auto_wb] {:?}: illuminant xy ({:.4}, {:.4}) ~{:.0}K -> temp {:.1} tint {:.1} in {:.1?}",
        mode,
        result.x,
        result.y,
        result.temperature_k,
        result.temperature,
        result.tint,
        started.elapsed()
    );

    Ok(result)
}

/// Sample the RGB of the finished picture at a point, in normalised image
/// coordinates (0..1, origin top-left).
///
/// This exists because the displayed photo cannot be read from the frontend at
/// all. RapidRAW renders the editor view to a **native WGPU surface composited
/// behind the webview** — there is no canvas and no `<img>` carrying the live
/// result. An earlier readout sampled the only large image in the DOM, which is
/// the cached `_medium.jpg` thumbnail, regenerated on save and never while a
/// slider moves. It read 207/177/179 at correct white balance and 214/189/192
/// at temperature -100, on a photo that had gone completely blue.
///
/// So the pixel has to be produced here, by actually rendering. A one-pixel ROI
/// keeps that cheap: the GPU does the same work it would for the preview, over
/// a single texel.
///
/// **Masks are not applied.** They need bitmaps assembled per render, and this
/// is a measuring instrument for global colour — white balance, and later DCP,
/// highlight recovery and filmic. If a masked reading is ever wanted, that is
/// where to start.
pub async fn sample_processed_pixel(
    x: f32,
    y: f32,
    js_adjustments: serde_json::Value,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<[u8; 3], String> {
    let context = crate::gpu_processing::get_or_init_gpu_context(&state, &app_handle)?;

    let mut adjustments = js_adjustments.clone();
    crate::adjustment_utils::hydrate_adjustments(&state, &mut adjustments);

    let base = crate::get_cached_full_warped_image(&state, &adjustments)?;
    let (width, height) = (base.width(), base.height());
    if width == 0 || height == 0 {
        return Err("No image to sample".to_string());
    }

    let px = (x.clamp(0.0, 1.0) * (width - 1) as f32).round() as u32;
    let py = (y.clamp(0.0, 1.0) * (height - 1) as f32).round() as u32;

    let loaded = state
        .original_image
        .lock()
        .unwrap()
        .clone()
        .ok_or("No original image loaded")?;

    let tonemapper =
        crate::image_processing::resolve_tonemapper_override_from_handle(&app_handle, loaded.is_raw);
    let mut all = crate::image_processing::get_all_adjustments_from_json(
        &adjustments,
        loaded.is_raw,
        tonemapper,
    );
    // Clipping indicators paint pure red or blue over blown pixels, which would
    // be read back as if it were the colour of the picture.
    all.global.show_clipping = 0;

    let lut = adjustments["lutPath"]
        .as_str()
        .and_then(|p| crate::lut_processing::get_or_load_lut(&state, p).ok());

    // A distinct hash, so this never collides with the preview's own cache entry.
    let hash = crate::cache_utils::calculate_full_job_hash(&loaded.path, &adjustments)
        .wrapping_add(0x5247_4250);

    let processed = crate::gpu_processing::process_and_get_dynamic_image(
        &context,
        &state,
        base.as_ref(),
        hash,
        crate::gpu_processing::RenderRequest {
            adjustments: all,
            mask_bitmaps: &[],
            lut,
            roi: Some(crate::gpu_processing::Roi {
                x: px,
                y: py,
                width: 1,
                height: 1,
            }),
        },
        "sample_processed_pixel",
    )?;

    let rgb = processed.to_rgb8();
    let p = rgb.get_pixel(0, 0);
    Ok([p[0], p[1], p[2]])
}

/// Throw away cached metadata for one photo so it is read from the file again.
///
/// WHY THIS IS NEEDED
///
/// EXIF is cached in two places, and both outlive a fix. A photo with an
/// `.agdata` sidecar has its EXIF written *into* that sidecar; everything else
/// goes into a per-folder JSON keyed on the file's mtime and size. Neither is
/// invalidated by the app changing, only by the photo changing — which it never
/// does.
///
/// So when lens reading was fixed, photos already edited kept a frozen snapshot
/// with an empty `LensModel` and went on failing to auto-detect while their
/// neighbours worked. It looked like an intermittent bug and cost a long time to
/// pin down. Clearing it by hand is not something to ask of anyone.
///
/// Only the cache is removed. Edits, ratings and tags in the sidecar are left
/// exactly as they are — EXIF is derived data and comes straight back on the
/// next read.
pub fn refresh_image_metadata(
    path: String,
    app_handle: tauri::AppHandle,
) -> Result<std::collections::HashMap<String, String>, String> {
    use tauri::Manager;

    let image = std::path::Path::new(&path);

    // 1. The sidecar's embedded copy, if there is one.
    let mut sidecar_name = image.file_name().unwrap_or_default().to_os_string();
    sidecar_name.push(".agdata");
    let sidecar = image.with_file_name(sidecar_name);

    if sidecar.exists()
        && let Ok(text) = std::fs::read_to_string(&sidecar)
        && let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(obj) = json.as_object_mut()
        && obj.remove("exif").is_some()
        && let Ok(out) = serde_json::to_string_pretty(&json)
    {
        std::fs::write(&sidecar, out).map_err(|e| e.to_string())?;
        log::info!("[refresh] cleared cached exif from {}", sidecar.display());
    }

    // 2. The per-folder cache, named by a hash of the folder path.
    if let Some(folder) = image.parent()
        && let Ok(cache_dir) = app_handle.path().app_cache_dir()
    {
        let hash = blake3::hash(folder.to_string_lossy().as_bytes())
            .to_hex()
            .to_string();
        let cache_file = cache_dir.join("exif").join(format!("{hash}.json"));
        if cache_file.exists() {
            let _ = std::fs::remove_file(&cache_file);
            log::info!("[refresh] cleared folder exif cache for {}", folder.display());
        }
    }

    // Read it straight back and hand it to the caller. Reloading the window
    // would also work and was the first attempt, but it drops the session and
    // returns to the welcome screen - a heavy price for refreshing one field.
    let bytes = std::fs::read(image).map_err(|e| e.to_string())?;
    Ok(crate::exif_processing::read_exif_data(&path, &bytes))
}

// ---------------------------------------------------------------------------
// Camera profiles
//
// Argentum reads `.dcp` profiles and does not ship any — see mods/profiles.rs
// for why. These four are what the UI needs: what this photo has, add one,
// list them, remove one.
// ---------------------------------------------------------------------------

/// The library path, or an error the user can act on.
fn profile_library() -> Result<&'static std::path::Path, String> {
    crate::mods::profiles::library()
        .ok_or_else(|| "the profile library is not ready yet".to_string())
}

/// Which camera this photo is from, and whether a profile matches it.
pub fn camera_profile_status(path: String) -> Result<crate::mods::profiles::Status, String> {
    let library = profile_library()?;
    Ok(crate::mods::profiles::status_for(library, std::path::Path::new(&path)))
}

/// Copy a `.dcp` the user picked into the library.
pub fn import_camera_profile(
    path: String,
) -> Result<crate::mods::profiles::Installed, String> {
    let library = profile_library()?;
    let installed = crate::mods::profiles::import(library, std::path::Path::new(&path))?;
    // The profile names the whole camera, so it also supplies the maker for a
    // gear entry that has none.
    if let Some(camera) = installed.camera.as_deref() {
        let model: String = camera.split_whitespace().skip(1).collect::<Vec<_>>().join(" ");
        crate::mods::profiles::learn_make_from(library, &model, camera);
    }
    log::info!(
        "[profile] imported {} for {}",
        installed.file,
        installed.camera.as_deref().unwrap_or("an unnamed camera")
    );
    Ok(installed)
}

/// Everything in the library.
pub fn list_camera_profiles() -> Result<Vec<crate::mods::profiles::Installed>, String> {
    Ok(crate::mods::profiles::installed(profile_library()?))
}

/// Delete a profile file from the library.
pub fn remove_camera_profile(file: String) -> Result<(), String> {
    crate::mods::profiles::remove(profile_library()?, &file)
}

/// Every camera a photo has been opened from, with the profile it would use.
pub fn list_cameras() -> Result<Vec<crate::mods::profiles::Camera>, String> {
    Ok(crate::mods::profiles::cameras(profile_library()?))
}

/// Drop a camera from the gear list.
pub fn forget_camera(model: String) -> Result<(), String> {
    crate::mods::profiles::forget_camera(profile_library()?, &model);
    Ok(())
}

/// Look for a published profile for this camera and install it.
///
/// One step on purpose: "is there one" and "get it" are not a decision the user
/// needs to make twice. Returns whether anything was found.
pub async fn get_profile_online(make: String, model: String) -> Result<Option<String>, String> {
    let library = profile_library()?;
    let Some(found) = crate::mods::profiles_online::search(&make, &model).await? else {
        return Ok(None);
    };
    crate::mods::profiles_online::fetch_into(library, &found).await?;
    // The file is named for the whole camera, so it also tells us the maker
    // when the gear list does not have one.
    if let Some(stem) = found.file.strip_suffix(".dcp") {
        crate::mods::profiles::learn_make_from(library, &model, stem);
    }
    log::info!("[profile] downloaded {} for {model}", found.file);
    Ok(Some(found.file))
}


/// What My Gear needs to draw one camera's row.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraProfiles {
    pub profiles: Vec<crate::mods::profiles::Installed>,
    /// Whether RawTherapee's own file for this body is already here, so the
    /// row knows there is nothing left for "Find one" to fetch.
    pub published_installed: bool,
}

/// Every profile in the library that fits this camera.
pub fn profiles_for_camera(make: String, model: String) -> Result<CameraProfiles, String> {
    let library = profile_library()?;
    Ok(CameraProfiles {
        profiles: crate::mods::profiles::matching(library, &make, &model),
        published_installed: crate::mods::profiles::published_is_installed(library, &make, &model),
    })
}

