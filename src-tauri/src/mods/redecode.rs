//! Decoding the open photo again, without taking it off the screen.
//!
//! WHY THIS EXISTS
//!
//! Highlight recovery runs while the RAW is decoded, before demosaic, because
//! that is the only place the sensor's mosaic still exists. So flipping its
//! switch changes nothing until the photo is decoded again, and the switch used
//! to say "applies to the next photo you open". It did not even manage that:
//! the app keeps recently decoded photos and serves them from memory, so for
//! any of those the switch did nothing at all.
//!
//! WHY NOT JUST CALL `load_image`
//!
//! That is what navigation uses, and it starts by emptying the open image and
//! every cache built from it, then decodes for a second or two. Any preview
//! request in that window finds no image. That is the shape of the four failed
//! attempts at a live camera-profile switch recorded in `profile_correction.rs`
//! — a black flash, a preview worker finding nothing — and it would also throw
//! away an AI denoise or panorama result the photo is carrying.
//!
//! So this does it in the other order. The new decode is made to one side
//! while the old one stays on screen. It replaces the old one only if the
//! photo is still the one on screen and no newer flip has superseded it. Only
//! then are the caches built from the old pixels dropped. Masks, AI patches and
//! AI results are kept: they belong to the photo, not to its highlights.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::app_state::AppState;

/// Every flip of the switch, numbered. Only the newest may land.
static REQUESTS: AtomicUsize = AtomicUsize::new(0);

/// What has to be unchanged between starting a decode and using it.
#[derive(Debug, Clone, PartialEq)]
struct Moment {
    /// The app's own counter, which `load_image` bumps on every navigation.
    generation: usize,
    /// Ours: the newest flip.
    request: usize,
    path: String,
}

/// May a decode started at `then` replace what is on screen `now`?
fn still_current(then: &Moment, now: &Moment) -> bool {
    then == now
}

/// Decode the open RAW again with the current settings and put it on screen.
///
/// Returns whether it did. `false` is not an error: nothing open, not a RAW, or
/// the person moved on before the decode finished — in each case there is
/// nothing to show.
pub async fn open_photo(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<bool, String> {
    let request = REQUESTS.fetch_add(1, Ordering::SeqCst) + 1;

    let Some(open) = state
        .original_image
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|o| (o.path.clone(), o.is_raw))
    else {
        return Ok(false);
    };
    let (path, is_raw) = open;
    if !is_raw {
        return Ok(false);
    }

    let then = Moment {
        generation: state.load_image_generation.load(Ordering::SeqCst),
        request,
        path: path.clone(),
    };

    let (source_path, _) = crate::file_management::parse_virtual_path(&path);
    let source = source_path.to_string_lossy().to_string();
    let settings = crate::app_settings::load_settings(app_handle).unwrap_or_default();
    // Navigating away bumps the generation, which cancels this decode the same
    // way it cancels theirs.
    let cancel = Some((state.load_image_generation.clone(), then.generation));
    let source_for_decode = source.clone();

    let decoded = tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&source_for_decode).map_err(|e| e.to_string())?;
        crate::image_loader::load_base_image_from_bytes(
            &bytes,
            &source_for_decode,
            false,
            &settings,
            cancel,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?;

    let image = match decoded {
        Ok(image) => Arc::new(image),
        Err(e) if e.contains("cancelled") => return Ok(false),
        Err(e) => return Err(e),
    };

    {
        let mut original = state
            .original_image
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let now = Moment {
            generation: state.load_image_generation.load(Ordering::SeqCst),
            request: REQUESTS.load(Ordering::SeqCst),
            path: original
                .as_ref()
                .map(|o| o.path.clone())
                .unwrap_or_default(),
        };
        if !still_current(&then, &now) {
            return Ok(false);
        }
        if let Some(open) = original.as_mut() {
            open.image = image.clone();
        }
    }

    // Every other recently opened photo was decoded under the old setting, so
    // they go; this one goes back in under its new pixels.
    {
        let mut decoded_cache = state
            .decoded_image_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let exif = decoded_cache.get(&source).map(|(_, exif)| exif);
        decoded_cache.clear();
        if let Some(exif) = exif {
            decoded_cache.insert(source, image, exif);
        }
    }

    // What was built from the old pixels. The same caches `load_image` resets
    // for a new photo, less the ones that belong to the photo rather than to its
    // pixels: masks, AI patches, geometry, and AI results.
    *state
        .cached_preview
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
    *state
        .gpu_image_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
    *state
        .full_warped_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
    *state
        .full_transformed_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
    *state
        .patched_warped_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moment(generation: usize, request: usize, path: &str) -> Moment {
        Moment {
            generation,
            request,
            path: path.to_string(),
        }
    }

    #[test]
    fn a_decode_lands_when_nothing_has_changed() {
        let then = moment(7, 3, "a.CR3");
        assert!(still_current(&then, &then.clone()));
    }

    /// The person opened another photo while this one decoded. Putting the
    /// old photo's pixels under the new one's name is the worst outcome here.
    #[test]
    fn a_decode_is_dropped_after_navigating_away() {
        let then = moment(7, 3, "a.CR3");
        assert!(!still_current(&then, &moment(8, 3, "b.CR3")));
        // Even back on the same photo: it was loaded again in between.
        assert!(!still_current(&then, &moment(8, 3, "a.CR3")));
    }

    /// Flipped twice quickly: the first decode must not land after the second.
    #[test]
    fn a_decode_is_dropped_when_a_newer_flip_exists() {
        let then = moment(7, 3, "a.CR3");
        assert!(!still_current(&then, &moment(7, 4, "a.CR3")));
    }

    #[test]
    fn a_decode_is_dropped_when_nothing_is_open() {
        let then = moment(7, 3, "a.CR3");
        assert!(!still_current(&then, &moment(7, 3, "")));
    }
}
