//! How big a non-RAW image may be before we refuse to decode it.
//!
//! Decoders size their buffers from the header, before any pixel is read, so a
//! small file that declares enormous dimensions would ask for tens of gigabytes
//! and abort the process. Every place that decodes a photo the user did not
//! make themselves goes through here: `image_loader::load_image_with_orientation`.
//!
//! The budget is 400 megapixels: above the largest cameras (a 150 MP medium
//! format body, 400 MP multi-shot files) and the 150 MP HEIC cap in
//! `mods/heic.rs`, and large enough for stitched panoramas, while the
//! allocation it implies stays within what a machine that can edit such a file
//! has. A file over it fails with an ordinary error, shown as "cannot open".

use image::Limits;

/// The most pixels (width x height) we decode. Defined once, here.
pub const MAX_DECODE_PIXELS: u64 = 400_000_000;

/// Bytes per pixel of the widest integer format the decoders produce
/// (16-bit RGBA); the allocation budget is the pixel budget times this.
const MAX_BYTES_PER_PIXEL: u64 = 8;

/// The limits to give every `image` reader. Width and height are left open
/// (a 100000 x 4000 panorama is within budget); `check_pixels` bounds their
/// product, and `max_alloc` bounds what a decoder may reserve.
pub fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_DECODE_PIXELS * MAX_BYTES_PER_PIXEL);
    limits
}

/// `Ok` when a `width` x `height` image is within the pixel budget.
pub fn check_pixels(width: u32, height: u32) -> Result<(), String> {
    let pixels = u64::from(width) * u64::from(height);
    if pixels <= MAX_DECODE_PIXELS {
        return Ok(());
    }
    Err(format!(
        "Image is too large to open ({width}x{height}, {} megapixels; the limit is {}).",
        pixels / 1_000_000,
        MAX_DECODE_PIXELS / 1_000_000
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_at_the_budget_is_accepted() {
        assert!(check_pixels(20_000, 20_000).is_ok());
        assert!(check_pixels(100_000, 4_000).is_ok());
        assert!(check_pixels(65_535, 6_103).is_ok());
    }

    #[test]
    fn an_image_over_the_budget_is_refused() {
        assert!(check_pixels(20_001, 20_000).is_err());
        assert!(check_pixels(60_000, 60_000).is_err());
        assert!(check_pixels(u32::MAX, u32::MAX).is_err());
    }

    #[test]
    fn the_allocation_limit_follows_the_budget() {
        assert_eq!(limits().max_alloc, Some(3_200_000_000));
    }
}
