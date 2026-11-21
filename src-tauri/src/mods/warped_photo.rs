//! The picture of a photo that is not open, as the editor's own measurements
//! and masks read the open one. Ours.
//!
//! The photo is decoded as for an export, its inpainted areas composited and
//! its geometry applied through `build_full_warped_image`, which is what
//! `get_cached_full_warped_image` builds for the open photo. Restore & Enlarge
//! loads one the same way (super_resolution.rs `load_source`). Used by Auto
//! white balance on pasted photos (auto_wb_photo.rs) and by AI masks found
//! again on each photo a preset or a paste reaches (selection_again.rs).

use image::DynamicImage;

use crate::app_settings::AppSettings;

/// The photo at `path` (a virtual copy's path included) and the edit in its
/// sidecar, which the picture's geometry comes from.
pub fn load(
    path: &str,
    settings: &AppSettings,
) -> Result<(DynamicImage, serde_json::Value), String> {
    let (source_path, sidecar_path) = crate::file_management::parse_virtual_path(path);
    let adjustments = crate::exif_processing::load_sidecar(&sidecar_path).adjustments;
    let source = source_path.to_string_lossy();
    let bytes = std::fs::read(&source_path).map_err(|e| format!("Could not read {source}: {e}"))?;
    let decoded =
        crate::image_loader::load_base_image_from_bytes(&bytes, &source, false, settings, None)
            .map_err(|e| format!("Could not decode {source}: {e}"))?;
    let patched = crate::image_loader::composite_patches_on_image(&decoded, &adjustments)
        .map_err(|e| format!("Could not apply the inpainted areas: {e}"))?;
    let is_raw = crate::formats::is_raw_file(&source_path);
    let picture = crate::mask_generation::build_full_warped_image(&patched, is_raw, &adjustments)
        .into_owned();
    Ok((picture, adjustments))
}
