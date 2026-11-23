//! How much memory the decoded-photo cache may hold (`DecodedImageCache`,
//! cache_utils.rs). Ours.
//!
//! The cache used to keep a COUNT of photos (setting `imageCacheSize`, default
//! 5). A decoded photo is 32-bit float RGB, 12 bytes a pixel: 288 MB at 24 MP
//! but 732 MB at 61 MP, so the same count held 1.4 GB or 3.7 GB. The setting is
//! now a size in megabytes (`decodedPhotoCacheMb`), and the cache counts bytes.
//!
//! Default, measured: five typical 24 MP photos (what the old default held) are
//! 1,440 MB. The default is a tenth of the installed memory, in steps of 256 MB,
//! at least 1,024 MB and at most 2,048 MB: 1,536 MB on a 16 GB machine (5.3
//! typical photos), 1,024 MB on 8 GB, 2,048 MB from 20 GB up.

use serde_json::{Map, Value};

/// A megabyte here is 1,000,000 bytes, as the number on the setting says.
pub const BYTES_PER_MB: u64 = 1_000_000;
/// One decoded photo of 24 megapixels, 12 bytes a pixel, in MB: the unit the
/// old count is converted with and the hint on the setting is worded in.
pub const TYPICAL_PHOTO_MB: u32 = 288;
pub const MIN_MB: u32 = 512;
pub const MAX_MB: u32 = 8192;
const DEFAULT_MIN_MB: u32 = 1024;
const DEFAULT_MAX_MB: u32 = 2048;
const DEFAULT_STEP_MB: u64 = 256;

/// The stored key of the count this setting replaces. Retired in
/// mods/retired_keys.rs; read once here, before it is stripped.
const OLD_COUNT_KEY: &str = "imageCacheSize";

pub fn clamp_mb(mb: u32) -> u32 {
    mb.clamp(MIN_MB, MAX_MB)
}

/// The default size for a machine with `installed_bytes` of memory.
pub fn default_mb(installed_bytes: u64) -> u32 {
    let tenth_mb = installed_bytes / 10 / BYTES_PER_MB;
    let stepped = tenth_mb / DEFAULT_STEP_MB * DEFAULT_STEP_MB;
    (stepped as u32).clamp(DEFAULT_MIN_MB, DEFAULT_MAX_MB)
}

/// What an old count of photos comes to: that many typical photos.
pub fn from_photo_count(count: u32) -> u32 {
    clamp_mb(count.saturating_mul(TYPICAL_PHOTO_MB))
}

/// Gives `current` a value when it has none: the old count converted, else the
/// default. Returns whether it changed. A value already there is never touched,
/// so the conversion happens once, whatever the file still holds.
pub fn fill(current: &mut Option<u32>, stored: &Map<String, Value>, installed_bytes: u64) -> bool {
    if current.is_some() {
        return false;
    }
    let old_count = stored
        .get(OLD_COUNT_KEY)
        .and_then(Value::as_u64)
        .map(|n| u32::try_from(n).unwrap_or(u32::MAX));
    *current = Some(match old_count {
        Some(count) => from_photo_count(count),
        None => default_mb(installed_bytes),
    });
    true
}

pub fn installed_memory_bytes() -> u64 {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    system.total_memory()
}

/// The budget in bytes for a stored size (`None`: the default).
pub fn budget_bytes(stored_mb: Option<u32>) -> u64 {
    let mb = stored_mb
        .map(clamp_mb)
        .unwrap_or_else(|| default_mb(installed_memory_bytes()));
    u64::from(mb) * BYTES_PER_MB
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    fn stored(count: Value) -> Map<String, Value> {
        let Value::Object(map) = serde_json::json!({ "imageCacheSize": count }) else {
            unreachable!()
        };
        map
    }

    #[test]
    fn a_typical_photo_is_what_a_24_megapixel_decode_holds() {
        let image = image::DynamicImage::new_rgb32f(600, 400);
        let bytes = image.as_bytes().len() as u64;
        let scale = 24_000_000 / (600 * 400) as u64;
        assert_eq!(
            bytes * scale,
            u64::from(TYPICAL_PHOTO_MB) * BYTES_PER_MB,
            "12 bytes a pixel"
        );
    }

    #[test]
    fn the_default_follows_installed_memory_between_its_floor_and_ceiling() {
        assert_eq!(default_mb(0), 1024, "unknown memory gets the floor");
        assert_eq!(default_mb(4 * GB), 1024);
        assert_eq!(default_mb(8 * GB), 1024);
        assert_eq!(default_mb(16 * GB), 1536);
        assert_eq!(default_mb(32 * GB), 2048);
        assert_eq!(default_mb(128 * GB), 2048, "and the ceiling");
        // What the old default held, five typical photos, is within reach on 16 GB.
        assert!(default_mb(16 * GB) >= 5 * TYPICAL_PHOTO_MB);
    }

    #[test]
    fn a_stored_count_becomes_that_many_typical_photos_within_the_range() {
        assert_eq!(from_photo_count(5), 1440);
        assert_eq!(from_photo_count(8), 2304);
        assert_eq!(from_photo_count(1), MIN_MB, "never under the smallest size");
        assert_eq!(from_photo_count(0), MIN_MB);
        assert_eq!(from_photo_count(1000), MAX_MB, "nor over the largest");
        assert_eq!(from_photo_count(u32::MAX), MAX_MB);
    }

    #[test]
    fn the_old_count_is_converted_once_and_a_missing_one_gets_the_default() {
        let mut value = None;
        assert!(fill(&mut value, &stored(Value::from(8)), 16 * GB));
        assert_eq!(value, Some(2304));

        // The file may still hold the old key; the size is not decided again.
        assert!(!fill(&mut value, &stored(Value::from(2)), 16 * GB));
        assert_eq!(value, Some(2304));

        let mut fresh = None;
        assert!(fill(&mut fresh, &Map::new(), 16 * GB));
        assert_eq!(fresh, Some(1536));

        for odd in [
            Value::Null,
            Value::from("many"),
            Value::from(-3),
            Value::from(2.5),
        ] {
            let mut value = None;
            fill(&mut value, &stored(odd), 16 * GB);
            assert_eq!(value, Some(1536), "a value that is no count is no count");
        }
    }

    #[test]
    fn a_stored_size_is_kept_within_the_range_and_counted_in_bytes() {
        assert_eq!(budget_bytes(Some(2000)), 2_000_000_000);
        assert_eq!(budget_bytes(Some(1)), u64::from(MIN_MB) * BYTES_PER_MB);
        assert_eq!(
            budget_bytes(Some(u32::MAX)),
            u64::from(MAX_MB) * BYTES_PER_MB
        );
        let default = budget_bytes(None);
        assert!((1024 * BYTES_PER_MB..=2048 * BYTES_PER_MB).contains(&default));
    }
}
