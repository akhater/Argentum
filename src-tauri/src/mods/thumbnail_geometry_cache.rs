//! Geometry-corrected bases kept while thumbnails are made, within a byte
//! budget, least recently used first out.
//!
//! The design follows the engine's step cache (`engine/src/cache.rs`): bytes
//! are counted from what the entries hold, a use refreshes recency, and
//! entries go oldest-first until the total fits. The engine's type holds
//! engine images keyed by number, so this is a copy for the app's own entry
//! (`DynamicImage` keyed by path); the eviction rule is the same.
//!
//! An entry is a thumbnail-sized base (the longest side is the thumbnail
//! resolution, 1280 px by default, or more when a crop is applied), stored as
//! 32-bit float RGB: 12 bytes a pixel, so about 13 MB for a 3:2 base and
//! 20 MB for a square one at 1280 px. A cropped base can be larger.

use std::collections::HashMap;

use crate::app_state::ThumbnailGeometryEntry;

/// What the cache may hold: 128 MiB. A 1280 px base is 13 MB at 3:2 and 20 MB
/// at 1:1 (12 bytes a pixel), so this keeps about ten typical bases and at
/// least six of the largest uncropped ones. Before this the cache held eight
/// entries of any size. The newest entry is kept even when it alone is over
/// this (a large photo with a tight crop); it goes when another arrives.
pub const MAX_BYTES: usize = 128 * 1024 * 1024;

pub struct ThumbnailGeometryCache {
    entries: HashMap<String, Slot>,
    budget_bytes: usize,
    used_bytes: usize,
    clock: u64,
}

struct Slot {
    entry: ThumbnailGeometryEntry,
    bytes: usize,
    used: u64,
}

impl Default for ThumbnailGeometryCache {
    fn default() -> Self {
        Self::with_budget(MAX_BYTES)
    }
}

impl ThumbnailGeometryCache {
    pub fn with_budget(budget_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            budget_bytes,
            used_bytes: 0,
            clock: 0,
        }
    }

    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    /// The entry kept for `key`, marked as just used.
    pub fn get(&mut self, key: &str) -> Option<&ThumbnailGeometryEntry> {
        self.clock += 1;
        let slot = self.entries.get_mut(key)?;
        slot.used = self.clock;
        Some(&slot.entry)
    }

    /// Keep `entry` under `key`, then let the least recently used go until
    /// what is kept fits the budget. The newest entry is always kept, even
    /// when it alone is over the budget (a large photo with a tight crop): the
    /// photo being worked on is not made again on every refresh. It goes as
    /// soon as another entry arrives.
    pub fn insert(&mut self, key: String, entry: ThumbnailGeometryEntry) {
        self.clock += 1;
        let bytes = entry.1.as_bytes().len();
        if let Some(old) = self.entries.remove(&key) {
            self.used_bytes -= old.bytes;
        }
        self.entries.insert(
            key.clone(),
            Slot {
                entry,
                bytes,
                used: self.clock,
            },
        );
        self.used_bytes += bytes;
        while self.used_bytes > self.budget_bytes {
            let Some(oldest) = self
                .entries
                .iter()
                .filter(|(k, _)| **k != key)
                .min_by_key(|(_, s)| s.used)
                .map(|(k, _)| k.clone())
            else {
                return;
            };
            if let Some(gone) = self.entries.remove(&oldest) {
                self.used_bytes -= gone.bytes;
            }
        }
    }

    /// Every image held, for the memory report.
    pub fn images(&self) -> impl Iterator<Item = &std::sync::Arc<image::DynamicImage>> {
        self.entries.values().map(|s| &s.entry.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::DynamicImage;
    use std::sync::Arc;

    /// A 32-bit float RGB base of `px` pixels: 12 bytes a pixel.
    fn base(px: u32) -> DynamicImage {
        DynamicImage::ImageRgb32F(image::Rgb32FImage::new(px, 1))
    }

    fn entry(px: u32) -> ThumbnailGeometryEntry {
        (7, Arc::new(base(px)), 1.0)
    }

    #[test]
    fn the_least_recently_used_goes_and_the_total_never_passes_the_budget() {
        // Room for three entries of 100 px (1200 bytes each).
        let mut cache = ThumbnailGeometryCache::with_budget(3600);
        for key in ["a", "b", "c", "d", "e"] {
            cache.insert(key.to_string(), entry(100));
            assert!(cache.used_bytes() <= 3600);
        }
        assert_eq!(cache.used_bytes(), 3600);
        assert!(cache.get("a").is_none() && cache.get("b").is_none());
        assert!(cache.get("c").is_some() && cache.get("d").is_some() && cache.get("e").is_some());
    }

    #[test]
    fn a_hit_refreshes_recency() {
        let mut cache = ThumbnailGeometryCache::with_budget(2400);
        cache.insert("a".into(), entry(100));
        cache.insert("b".into(), entry(100));
        assert!(cache.get("a").is_some());
        cache.insert("c".into(), entry(100));
        // "b" was the least recently used, not "a".
        assert!(cache.get("b").is_none());
        assert!(cache.get("a").is_some() && cache.get("c").is_some());
    }

    #[test]
    fn entries_are_counted_at_their_real_size() {
        let mut cache = ThumbnailGeometryCache::with_budget(2400);
        cache.insert("small".into(), entry(50));
        cache.insert("big".into(), entry(150));
        // 600 + 1800 fits; one more small one does not.
        assert_eq!(cache.used_bytes(), 2400);
        cache.insert("small2".into(), entry(50));
        assert!(cache.used_bytes() <= 2400);
        assert!(cache.get("small").is_none());
    }

    #[test]
    fn replacing_a_key_does_not_count_it_twice() {
        let mut cache = ThumbnailGeometryCache::with_budget(10_000);
        cache.insert("a".into(), entry(100));
        cache.insert("a".into(), entry(200));
        assert_eq!(cache.used_bytes(), 2400);
    }

    #[test]
    fn the_newest_entry_is_kept_over_the_budget_until_another_arrives() {
        let mut cache = ThumbnailGeometryCache::with_budget(1000);
        cache.insert("a".into(), entry(100));
        // Served from the cache on the second request.
        assert!(cache.get("a").is_some());
        assert!(cache.get("a").is_some());
        assert_eq!(cache.used_bytes(), 1200);
        // Another entry evicts it.
        cache.insert("b".into(), entry(10));
        assert!(cache.get("a").is_none());
        assert_eq!(cache.used_bytes(), 120);
    }

    /// A base whose pixels are a function of the key, as a recomputed one is.
    fn made_from(key: &str, px: u32) -> ThumbnailGeometryEntry {
        let seed = key.bytes().map(f32::from).sum::<f32>();
        let mut img = image::Rgb32FImage::new(px, 1);
        for (i, p) in img.pixels_mut().enumerate() {
            *p = image::Rgb([seed, i as f32, seed + i as f32]);
        }
        (7, Arc::new(DynamicImage::ImageRgb32F(img)), 0.5)
    }

    #[test]
    fn a_request_after_eviction_makes_the_same_value_again() {
        let mut cache = ThumbnailGeometryCache::with_budget(2400);
        cache.insert("a".into(), made_from("a", 100));
        let before = cache.get("a").unwrap().1.as_bytes().to_vec();
        cache.insert("b".into(), made_from("b", 100));
        cache.insert("c".into(), made_from("c", 100));
        assert!(cache.get("a").is_none());
        // Missing: made again from the key, and equal to the first time.
        cache.insert("a".into(), made_from("a", 100));
        let (hash, img, scale) = cache.get("a").unwrap();
        assert_eq!((*hash, *scale), (7, 0.5));
        assert_eq!(img.as_bytes(), &before[..]);
        let other = made_from("b", 100).1;
        assert_ne!(img.as_bytes(), other.as_bytes());
    }

    #[test]
    fn a_default_base_at_1280_px_fits_the_budget_several_times() {
        let one = 1280usize * 1280 * 12;
        assert!(MAX_BYTES / one >= 6, "{} square bases fit", MAX_BYTES / one);
    }
}
