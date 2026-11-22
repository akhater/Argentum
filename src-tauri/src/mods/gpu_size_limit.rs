//! The largest image the GPU can hold, checked in one place.
//!
//! Every render path that uploads a whole image as one texture asks here
//! first. An image wider or taller than the device's
//! `max_texture_dimension_2d` (16384 px on most GPUs; a stitched panorama
//! passes it) cannot be rendered, and the render must fail with a message
//! the user can read. Before RapidRAW `dfb0cba4` the preview path returned
//! the image unprocessed instead, so such a panorama exported without its
//! edits and said nothing.

/// `Ok` when a `width`x`height` image fits in textures of at most
/// `max_dim_px` on a side; otherwise the error the user is shown.
pub fn check_fits_gpu(width: u32, height: u32, max_dim_px: u32) -> Result<(), String> {
    if width <= max_dim_px && height <= max_dim_px {
        return Ok(());
    }
    // RapidRAW dfb0cba4: src-tauri/src/gpu_processing.rs
    Err(format!(
        "Image dimensions ({}x{}) exceed the GPU texture limit ({}). The image could not be processed; try reducing its dimensions.",
        width, height, max_dim_px
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_at_the_limit_fits() {
        assert!(check_fits_gpu(16384, 16384, 16384).is_ok());
        assert!(check_fits_gpu(6000, 4000, 16384).is_ok());
    }

    #[test]
    fn a_panorama_over_the_limit_is_an_error_naming_both_sizes() {
        let err = check_fits_gpu(20000, 5000, 16384).unwrap_err();
        assert!(err.contains("20000x5000"), "{err}");
        assert!(err.contains("16384"), "{err}");
        let err = check_fits_gpu(5000, 16385, 16384).unwrap_err();
        assert!(err.contains("5000x16385"), "{err}");
    }
}
