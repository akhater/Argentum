//! How big an exported photo may be. Ours.
//!
//! The export dialog lets the user type a target size, and an extra zero would
//! ask for an image of billions of pixels: the allocation aborts the whole
//! process, mid-batch. The size an export would write is checked here, before
//! anything is allocated, and the interface asks for the same limit
//! (`export_pixel_limit`) instead of keeping a copy.
//!
//! The limit is the one `decode_limits` sets for opening a photo: what the
//! app can open it can also write, and the allocation it implies is one the
//! machine already needs to edit such a file.

use std::fmt;

use crate::mods::decode_limits::MAX_DECODE_PIXELS;

/// The most pixels (width x height) an export writes.
pub const MAX_EXPORT_PIXELS: u64 = MAX_DECODE_PIXELS;

/// Why an export size was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportSizeError {
    /// A side came out as zero (a target of 0).
    Empty { width: u32, height: u32 },
    /// The photo would be over the limit.
    TooLarge { width: u32, height: u32, limit: u64 },
}

impl fmt::Display for ExportSizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { width, height } => write!(
                f,
                "The export size {width}x{height} is empty: the target size must be at least 1 pixel."
            ),
            Self::TooLarge {
                width,
                height,
                limit,
            } => write!(
                f,
                "The export size {width}x{height} ({} megapixels) is over the limit of {} megapixels.",
                u64::from(*width) * u64::from(*height) / 1_000_000,
                limit / 1_000_000
            ),
        }
    }
}

impl From<ExportSizeError> for String {
    fn from(error: ExportSizeError) -> Self {
        error.to_string()
    }
}

/// `Ok` when a `width` x `height` export is non-empty and within the limit.
pub fn check_export_size(width: u32, height: u32) -> Result<(), ExportSizeError> {
    if width == 0 || height == 0 {
        return Err(ExportSizeError::Empty { width, height });
    }
    if u64::from(width) * u64::from(height) > MAX_EXPORT_PIXELS {
        return Err(ExportSizeError::TooLarge {
            width,
            height,
            limit: MAX_EXPORT_PIXELS,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_normal_size_is_accepted() {
        assert!(check_export_size(6000, 4000).is_ok());
        assert!(check_export_size(1, 1).is_ok());
    }

    #[test]
    fn the_limit_is_the_decode_limit() {
        assert_eq!(MAX_EXPORT_PIXELS, MAX_DECODE_PIXELS);
    }

    #[test]
    fn just_under_and_at_the_limit_are_accepted() {
        assert!(check_export_size(20_000, 19_999).is_ok());
        assert!(check_export_size(20_000, 20_000).is_ok());
    }

    #[test]
    fn just_over_the_limit_is_refused_naming_both_numbers() {
        let error = check_export_size(20_001, 20_000).unwrap_err();
        assert!(matches!(error, ExportSizeError::TooLarge { .. }));
        let text = error.to_string();
        assert!(text.contains("20001x20000"), "{text}");
        assert!(text.contains("400 megapixels"), "{text}");
    }

    #[test]
    fn a_runaway_size_is_refused_without_overflow() {
        assert!(check_export_size(u32::MAX, u32::MAX).is_err());
        assert!(check_export_size(1_000_000, 666_667).is_err());
    }

    #[test]
    fn a_zero_side_is_refused() {
        assert!(matches!(
            check_export_size(0, 100),
            Err(ExportSizeError::Empty { .. })
        ));
        assert!(matches!(
            check_export_size(100, 0),
            Err(ExportSizeError::Empty { .. })
        ));
    }
}
