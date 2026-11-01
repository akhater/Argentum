use crate::AppState;
use crate::app_settings::AppSettings;
use image::DynamicImage;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

pub const GEOMETRY_KEYS: &[&str] = &[
    "transformDistortion",
    "transformVertical",
    "transformHorizontal",
    "transformRotate",
    "transformAspect",
    "transformScale",
    "transformXOffset",
    "transformYOffset",
    "lensDistortionAmount",
    "lensVignetteAmount",
    "lensTcaAmount",
    "lensDistortionParams",
    "lensMaker",
    "lensModel",
    "lensDistortionEnabled",
    "lensTcaEnabled",
    "lensVignetteEnabled",
    "guidedPerspective",
];

/// Hashes what moves pixels around: `GEOMETRY_KEYS`, then the camera's own
/// lens correction (`mods/lens_builtin.rs`). A new warp's keys go in a list
/// of their own, chained here: `calculate_visual_hash` skips `GEOMETRY_KEYS`.
pub fn hash_geometry(adjustments: &serde_json::Value, hasher: &mut impl Hasher) {
    let keys = GEOMETRY_KEYS
        .iter()
        .chain(crate::mods::lens_builtin::KEYS.iter());
    for key in keys {
        if let Some(val) = adjustments.get(key) {
            key.hash(hasher);
            val.to_string().hash(hasher);
        }
    }
    // Argentum: the Lens Correction eye closed undoes the warp; open, the hash is as before.
    if !crate::image_processing::is_lens_correction_shown(adjustments) {
        "lens eye closed".hash(hasher);
    }
}

fn hash_ai_patches(adjustments: &serde_json::Value, hasher: &mut DefaultHasher) {
    if let Some(patches_val) = adjustments.get("aiPatches")
        && let Some(patches_arr) = patches_val.as_array()
    {
        patches_arr.len().hash(hasher);

        for patch in patches_arr {
            if let Some(id) = patch.get("id").and_then(|v| v.as_str()) {
                id.hash(hasher);
            }

            let is_visible = patch
                .get("visible")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            is_visible.hash(hasher);

            // upstream #1307
            // Hash the contents, not the length: two patches of the same base64
            // length collided and the cache served whichever arrived first.
            if let Some(patch_data) = patch.get("patchData") {
                patch_data
                    .get("color")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .hash(hasher);

                patch_data
                    .get("mask")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .hash(hasher);
            } else {
                patch
                    .get("patchDataBase64")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .hash(hasher);
            }
            // end upstream #1307

            if let Some(sub_masks_val) = patch.get("subMasks") {
                sub_masks_val.to_string().hash(hasher);
            }

            let invert = patch
                .get("invert")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            invert.hash(hasher);
        }
    }
}

fn hash_orientation(adjustments: &serde_json::Value, hasher: &mut DefaultHasher) {
    adjustments["orientationSteps"]
        .as_u64()
        .unwrap_or(0)
        .hash(hasher);
    adjustments["rotation"]
        .as_f64()
        .unwrap_or(0.0)
        .to_bits()
        .hash(hasher);
    adjustments["flipHorizontal"]
        .as_bool()
        .unwrap_or(false)
        .hash(hasher);
    adjustments["flipVertical"]
        .as_bool()
        .unwrap_or(false)
        .hash(hasher);
}

fn hash_effect_keys(adjustments: &serde_json::Value, keys: &[&str], hasher: &mut DefaultHasher) {
    for key in keys {
        if let Some(val) = adjustments.get(*key) {
            key.hash(hasher);
            match val.as_str() {
                Some(s) => s.hash(hasher),
                None => val.to_string().hash(hasher),
            }
        }
    }
}

pub fn calculate_patch_hash(adjustments: &serde_json::Value) -> u64 {
    let mut hasher = DefaultHasher::new();

    hash_ai_patches(adjustments, &mut hasher);

    let uses_generated_masks = adjustments
        .get("aiPatches")
        .and_then(|v| v.as_array())
        .is_some_and(|patches| {
            patches.iter().any(|patch| {
                patch
                    .get("patchData")
                    .and_then(|data| data.get("mask"))
                    .and_then(|mask| mask.as_str())
                    .is_none_or(|mask| mask.is_empty())
            })
        });
    uses_generated_masks.hash(&mut hasher);

    if uses_generated_masks {
        hash_orientation(adjustments, &mut hasher);

        hash_geometry(adjustments, &mut hasher);
    }

    hasher.finish()
}

pub fn calculate_geometry_hash(adjustments: &serde_json::Value) -> u64 {
    let mut hasher = DefaultHasher::new();

    calculate_patch_hash(adjustments).hash(&mut hasher);

    hash_geometry(adjustments, &mut hasher);

    hasher.finish()
}

pub fn calculate_patched_warped_hash(adjustments: &serde_json::Value) -> u64 {
    let mut hasher = DefaultHasher::new();

    calculate_geometry_hash(adjustments).hash(&mut hasher);
    calculate_effects_hash(adjustments).hash(&mut hasher);

    hasher.finish()
}

pub fn calculate_effects_hash(adjustments: &serde_json::Value) -> u64 {
    let mut hasher = DefaultHasher::new();

    let blur_enabled =
        crate::image_processing::is_effect_enabled(adjustments, "lensBlur", "lensBlurEnabled");
    blur_enabled.hash(&mut hasher);

    if blur_enabled {
        hash_effect_keys(
            adjustments,
            &[
                "lensBlurAmount",
                "lensBlurDiffusion",
                "lensBlurShape",
                "lensBlurMinDepth",
                "lensBlurMaxDepth",
                "lensBlurMinFade",
                "lensBlurMaxFade",
                "lensBlurDepthMap",
            ],
            &mut hasher,
        );
    }

    let relight_enabled =
        crate::image_processing::is_effect_enabled(adjustments, "relight", "relightEnabled");
    relight_enabled.hash(&mut hasher);

    if relight_enabled {
        hash_effect_keys(
            adjustments,
            &[
                "relightLights",
                "relightAmbient",
                "relightSoftness",
                "relightShine",
                "relightShadows",
                "relightShadowSoftness",
                "relightNormalMap",
            ],
            &mut hasher,
        );
    }

    let fog_enabled = crate::image_processing::is_effect_enabled(adjustments, "fog", "fogEnabled");
    fog_enabled.hash(&mut hasher);

    if fog_enabled {
        hash_effect_keys(
            adjustments,
            &[
                "fogAmount",
                "fogStart",
                "fogDensity",
                "fogHeight",
                "fogVariation",
                "fogGlow",
                "fogTemperature",
                "fogTint",
                "fogDepthMap",
            ],
            &mut hasher,
        );
        hash_orientation(adjustments, &mut hasher);
    }

    hasher.finish()
}

pub fn calculate_thumbnail_base_hash(adjustments: &serde_json::Value) -> u64 {
    let mut hasher = DefaultHasher::new();

    calculate_patched_warped_hash(adjustments).hash(&mut hasher);

    adjustments["orientationSteps"]
        .as_u64()
        .unwrap_or(0)
        .hash(&mut hasher);

    hasher.finish()
}

pub fn calculate_visual_hash(path: &str, adjustments: &serde_json::Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);

    if let Some(obj) = adjustments.as_object() {
        for (key, value) in obj {
            if GEOMETRY_KEYS.contains(&key.as_str()) {
                continue;
            }

            match key.as_str() {
                "crop" | "rotation" | "orientationSteps" | "flipHorizontal" | "flipVertical" => (),
                _ => {
                    key.hash(&mut hasher);
                    value.to_string().hash(&mut hasher);
                }
            }
        }
    }

    hasher.finish()
}

pub fn calculate_transform_hash(adjustments: &serde_json::Value) -> u64 {
    let mut hasher = DefaultHasher::new();

    let orientation_steps = adjustments["orientationSteps"].as_u64().unwrap_or(0);
    orientation_steps.hash(&mut hasher);

    let rotation = adjustments["rotation"].as_f64().unwrap_or(0.0);
    (rotation.to_bits()).hash(&mut hasher);

    let flip_h = adjustments["flipHorizontal"].as_bool().unwrap_or(false);
    flip_h.hash(&mut hasher);

    let flip_v = adjustments["flipVertical"].as_bool().unwrap_or(false);
    flip_v.hash(&mut hasher);

    let blur_enabled =
        crate::image_processing::is_effect_enabled(adjustments, "lensBlur", "lensBlurEnabled");
    blur_enabled.hash(&mut hasher);
    if blur_enabled {
        if let Some(val) = adjustments.get("lensBlurAmount") {
            val.to_string().hash(&mut hasher);
        }
        if let Some(val) = adjustments.get("lensBlurDiffusion") {
            val.to_string().hash(&mut hasher);
        }
        if let Some(val) = adjustments.get("lensBlurShape") {
            val.as_str().unwrap_or("").hash(&mut hasher);
        }
        if let Some(val) = adjustments.get("lensBlurMinDepth") {
            val.to_string().hash(&mut hasher);
        }
        if let Some(val) = adjustments.get("lensBlurMaxDepth") {
            val.to_string().hash(&mut hasher);
        }
        if let Some(val) = adjustments.get("lensBlurMinFade") {
            val.to_string().hash(&mut hasher);
        }
        if let Some(val) = adjustments.get("lensBlurMaxFade") {
            val.to_string().hash(&mut hasher);
        }
        if let Some(val) = adjustments.get("lensBlurDepthMap") {
            val.as_str().unwrap_or("").hash(&mut hasher);
        }
    }

    let relight_enabled =
        crate::image_processing::is_effect_enabled(adjustments, "relight", "relightEnabled");
    relight_enabled.hash(&mut hasher);
    if relight_enabled {
        for key in [
            "relightLights",
            "relightAmbient",
            "relightSoftness",
            "relightShine",
            "relightShadows",
            "relightShadowSoftness",
        ] {
            if let Some(val) = adjustments.get(key) {
                val.to_string().hash(&mut hasher);
            }
        }
        if let Some(val) = adjustments.get("relightNormalMap") {
            val.as_str().unwrap_or("").len().hash(&mut hasher);
        }
    }

    let fog_enabled = crate::image_processing::is_effect_enabled(adjustments, "fog", "fogEnabled");
    fog_enabled.hash(&mut hasher);
    if fog_enabled {
        for key in [
            "fogAmount",
            "fogStart",
            "fogDensity",
            "fogHeight",
            "fogVariation",
            "fogGlow",
            "fogTemperature",
            "fogTint",
        ] {
            if let Some(val) = adjustments.get(key) {
                val.to_string().hash(&mut hasher);
            }
        }
        if let Some(val) = adjustments.get("fogDepthMap") {
            val.as_str().unwrap_or("").len().hash(&mut hasher);
        }
    }

    if let Some(crop_val) = adjustments.get("crop")
        && !crop_val.is_null()
    {
        crop_val.to_string().hash(&mut hasher);
    }

    hash_geometry(adjustments, &mut hasher);

    hash_ai_patches(adjustments, &mut hasher);

    hasher.finish()
}

pub fn calculate_full_job_hash(path: &str, adjustments: &serde_json::Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    adjustments.to_string().hash(&mut hasher);
    hasher.finish()
}

// RapidRAW e11750e7: src-tauri/src/cache_utils.rs.
/// A path alone goes stale: tethering reuses file names and raw settings change the pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeFingerprint {
    len: u64,
    modified: Option<SystemTime>,
    settings_hash: u64,
}

impl DecodeFingerprint {
    pub fn for_file(path: &Path, settings: &AppSettings) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self::new(
            &metadata,
            settings,
            crate::mods::highlights::enabled(),
        ))
    }

    /// Every setting `image_loader::load_base_image_from_bytes` reads.
    fn new(metadata: &std::fs::Metadata, settings: &AppSettings, highlight_recovery: bool) -> Self {
        let mut hasher = DefaultHasher::new();
        settings
            .raw_highlight_compression
            .map(f32::to_bits)
            .hash(&mut hasher);
        settings.linear_raw_mode.hash(&mut hasher);
        settings
            .raw_preprocessing_color_nr
            .map(f32::to_bits)
            .hash(&mut hasher);
        settings
            .raw_preprocessing_sharpening
            .map(f32::to_bits)
            .hash(&mut hasher);
        settings.apply_preprocessing_to_non_raws.hash(&mut hasher);
        settings.use_apple_raw9.hash(&mut hasher);
        // Argentum: highlight recovery runs inside the decode (mods/highlights.rs).
        highlight_recovery.hash(&mut hasher);
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            settings_hash: hasher.finish(),
        }
    }
}

struct DecodedEntry {
    path: String,
    fingerprint: DecodeFingerprint,
    image: Arc<DynamicImage>,
    exif: HashMap<String, String>,
}

pub struct DecodedImageCache {
    capacity: usize,
    items: Vec<DecodedEntry>,
}

impl DecodedImageCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            items: Vec::with_capacity(capacity),
        }
    }

    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
        while self.items.len() > self.capacity {
            self.items.remove(0);
        }
    }

    /// The photo decoded from `path`, if it was decoded from the file as it is
    /// now and with the same settings. An entry that no longer matches is a
    /// miss, and is dropped (RapidRAW e11750e7).
    pub fn get(
        &mut self,
        path: &str,
        fingerprint: &DecodeFingerprint,
    ) -> Option<(Arc<DynamicImage>, HashMap<String, String>)> {
        let pos = self.items.iter().position(|e| e.path == path)?;
        let entry = self.items.remove(pos);
        if entry.fingerprint != *fingerprint {
            return None;
        }
        let result = (Arc::clone(&entry.image), entry.exif.clone());
        self.items.push(entry);
        Some(result)
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn insert(
        &mut self,
        path: String,
        fingerprint: DecodeFingerprint,
        image: Arc<DynamicImage>,
        exif: HashMap<String, String>,
    ) {
        if let Some(pos) = self.items.iter().position(|e| e.path == path) {
            self.items.remove(pos);
        } else if self.items.len() >= self.capacity {
            self.items.remove(0);
        }
        self.items.push(DecodedEntry {
            path,
            fingerprint,
            image,
            exif,
        });
    }
}

pub fn clear_preview_stage_caches(state: &AppState) {
    *state
        .patched_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
    *state
        .patched_warped_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
    *state
        .working_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
    *state
        .effects_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
    *state
        .full_transformed_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
}

// Argentum: off the UI thread, a render holds these locks - see mods/ui_thread.rs
#[tauri::command]
pub async fn clear_image_caches(app_handle: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;

    tokio::task::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        if let Ok(mut decoded_cache) = state.decoded_image_cache.lock() {
            decoded_cache.clear();
        }
        if let Ok(mut gpu_cache) = state.gpu_image_cache.lock() {
            *gpu_cache = None;
        }
        if let Ok(mut preview_cache) = state.cached_preview.lock() {
            *preview_cache = None;
        }
        if let Ok(mut warped_cache) = state.full_warped_cache.lock() {
            *warped_cache = None;
        }
        clear_preview_stage_caches(&state);
    })
    .await
    .map_err(|e| format!("Task panicked: {}", e))
}

#[tauri::command]
pub fn clear_session_caches(state: tauri::State<AppState>) {
    if let Ok(mut patch_cache) = state.patch_cache.lock() {
        patch_cache.clear();
    }
    if let Ok(mut mask_cache) = state.mask_cache.lock() {
        mask_cache.clear();
    }
    if let Ok(mut geometry_cache) = state.geometry_cache.lock() {
        geometry_cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn scratch_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("argentum-decode-fingerprint");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn fingerprint(path: &Path, settings: &AppSettings, recovery: bool) -> DecodeFingerprint {
        DecodeFingerprint::new(&std::fs::metadata(path).unwrap(), settings, recovery)
    }

    fn image() -> Arc<DynamicImage> {
        Arc::new(DynamicImage::new_rgb32f(2, 2))
    }

    #[test]
    fn the_same_file_and_settings_match() {
        let path = scratch_file("same", b"raw bytes");
        let settings = AppSettings::default();
        assert_eq!(
            fingerprint(&path, &settings, true),
            fingerprint(&path, &settings, true)
        );
        assert_eq!(
            DecodeFingerprint::for_file(&path, &settings),
            DecodeFingerprint::for_file(&path, &settings)
        );
    }

    /// Tethering writes capt0000.nef again for the next shot.
    #[test]
    fn a_file_rewritten_under_the_same_name_does_not_match() {
        let path = scratch_file("rewritten", b"first shot");
        let settings = AppSettings::default();
        let before = fingerprint(&path, &settings, true);

        std::fs::write(&path, b"second shot, longer").unwrap();
        assert_ne!(before, fingerprint(&path, &settings, true), "new length");

        let same_length = fingerprint(&path, &settings, true);
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_modified(SystemTime::now() + Duration::from_secs(60))
            .unwrap();
        drop(file);
        assert_ne!(
            same_length,
            fingerprint(&path, &settings, true),
            "same length, new time"
        );
    }

    #[test]
    fn every_setting_the_decode_reads_moves_it() {
        let path = scratch_file("settings", b"raw bytes");
        let base = AppSettings::default();
        let before = fingerprint(&path, &base, true);
        let changed: [(&str, AppSettings); 6] = [
            (
                "highlight compression",
                AppSettings {
                    raw_highlight_compression: Some(4.0),
                    ..base.clone()
                },
            ),
            (
                "linear mode",
                AppSettings {
                    linear_raw_mode: "something else".into(),
                    ..base.clone()
                },
            ),
            (
                "colour noise reduction",
                AppSettings {
                    raw_preprocessing_color_nr: Some(0.05),
                    ..base.clone()
                },
            ),
            (
                "sharpening",
                AppSettings {
                    raw_preprocessing_sharpening: Some(0.9),
                    ..base.clone()
                },
            ),
            (
                "non-RAWs too",
                AppSettings {
                    apply_preprocessing_to_non_raws: Some(true),
                    ..base.clone()
                },
            ),
            (
                "Apple RAW 9",
                AppSettings {
                    use_apple_raw9: Some(true),
                    ..base.clone()
                },
            ),
        ];
        for (what, settings) in changed {
            assert_ne!(before, fingerprint(&path, &settings, true), "{what}");
        }
        assert_ne!(
            before,
            fingerprint(&path, &base, false),
            "highlight recovery"
        );
    }

    /// Saving any other setting must not empty the cache: reopening stays instant.
    #[test]
    fn a_setting_the_decode_does_not_read_leaves_it() {
        let path = scratch_file("unrelated", b"raw bytes");
        let base = AppSettings::default();
        let other = AppSettings {
            theme: Some("something else".into()),
            image_cache_size: Some(9),
            ..base.clone()
        };
        assert_eq!(
            fingerprint(&path, &base, true),
            fingerprint(&path, &other, true)
        );
    }

    #[test]
    fn a_match_is_served_and_a_stale_entry_is_dropped() {
        let path = scratch_file("cache", b"raw bytes");
        let settings = AppSettings::default();
        let now = fingerprint(&path, &settings, true);
        let other = fingerprint(&path, &settings, false);
        let mut cache = DecodedImageCache::new(5);
        let decoded = image();
        cache.insert(
            "a.cr3".into(),
            now.clone(),
            Arc::clone(&decoded),
            HashMap::new(),
        );

        let (hit, _) = cache.get("a.cr3", &now).expect("same file, same settings");
        assert!(Arc::ptr_eq(&hit, &decoded));
        assert!(cache.get("b.cr3", &now).is_none());

        assert!(cache.get("a.cr3", &other).is_none(), "settings changed");
        assert!(
            cache.get("a.cr3", &now).is_none(),
            "the stale entry is gone, not kept for later"
        );
    }

    #[test]
    fn the_oldest_photo_still_leaves_first() {
        let path = scratch_file("capacity", b"raw bytes");
        let fp = fingerprint(&path, &AppSettings::default(), true);
        let mut cache = DecodedImageCache::new(2);
        for name in ["a", "b", "c"] {
            cache.insert(name.into(), fp.clone(), image(), HashMap::new());
        }
        assert!(cache.get("a", &fp).is_none());
        assert!(cache.get("b", &fp).is_some());
        assert!(cache.get("c", &fp).is_some());

        // Using a photo makes it the newest.
        cache.get("b", &fp);
        cache.insert("d".into(), fp.clone(), image(), HashMap::new());
        assert!(cache.get("c", &fp).is_none());
        assert!(cache.get("b", &fp).is_some());
    }

    fn geometry_of(adjustments: &serde_json::Value) -> u64 {
        let mut hasher = DefaultHasher::new();
        hash_geometry(adjustments, &mut hasher);
        hasher.finish()
    }

    /// Every key that moves pixels around, ours for the camera's own lens
    /// correction included, moves the geometry.
    #[test]
    fn every_geometry_key_moves_the_geometry() {
        let none = serde_json::json!({});
        let keys = GEOMETRY_KEYS
            .iter()
            .chain(crate::mods::lens_builtin::KEYS.iter());
        for key in keys {
            let one = serde_json::json!({ *key: 1 });
            assert_ne!(geometry_of(&none), geometry_of(&one), "{key}");
        }
    }

    /// The built-in lens correction warps the picture, so the transform hash
    /// tells the two apart for every cache keyed on it: the preview, the
    /// export's reuse of it, the LUT swatches.
    #[test]
    fn the_built_in_lens_correction_moves_the_transform_hash() {
        let off = serde_json::json!({ "lensBuiltin": false });
        let on = serde_json::json!({ "lensBuiltin": true });
        assert_ne!(
            calculate_transform_hash(&off),
            calculate_transform_hash(&on)
        );
        let params =
            |k1: f64| serde_json::json!({ "lensBuiltin": true, "lensBuiltinParams": { "k1": k1 } });
        assert_ne!(
            calculate_transform_hash(&params(0.1)),
            calculate_transform_hash(&params(0.2))
        );
    }

    /// The Lens Correction eye closed undoes the warp, so the transform hash
    /// moves with it; an open eye, said or not, leaves the hash as it was.
    #[test]
    fn the_lens_correction_eye_moves_the_transform_hash() {
        let plain = serde_json::json!({ "lensBuiltin": true });
        let open =
            serde_json::json!({ "lensBuiltin": true, "sectionVisibility": { "lens": true } });
        let closed =
            serde_json::json!({ "lensBuiltin": true, "sectionVisibility": { "lens": false } });
        assert_eq!(
            calculate_transform_hash(&plain),
            calculate_transform_hash(&open)
        );
        assert_ne!(
            calculate_transform_hash(&plain),
            calculate_transform_hash(&closed)
        );
    }

    /// The visual hash keys the picture before the warp. It skips
    /// `GEOMETRY_KEYS` and reads every other key, the built-in lens's
    /// included; folding ours into the geometry leaves it as it was.
    #[test]
    fn the_visual_hash_skips_the_geometry_keys_and_reads_the_rest() {
        let visual = |adjustments| calculate_visual_hash("photo.cr3", &adjustments);
        assert_eq!(
            visual(serde_json::json!({ "transformRotate": 0 })),
            visual(serde_json::json!({ "transformRotate": 5 }))
        );
        assert_ne!(
            visual(serde_json::json!({ "lensBuiltin": false })),
            visual(serde_json::json!({ "lensBuiltin": true }))
        );
    }
}
