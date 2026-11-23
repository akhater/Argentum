//! The generated mask bitmaps of the open photo, shared and bounded by bytes.
//!
//! A mask's bitmap is one grey byte per preview pixel (about 2.5 MB at preview
//! size, 60 MB when the preview is the whole 61 MP photo). Every render reads
//! each mask on the photo, so a hit hands out an `Arc` (a pointer bump) and the
//! renderer borrows the pixels; before, each hit cloned them.
//!
//! North star 5: every cache bounded, the budget a number checked by a test.
//! The bound is in bytes, not entries: [`MASK_CACHE_BYTES`]. When an insert would
//! pass it the least recently used entries go first; one evicted is simply
//! generated again on its next request, exactly as a never-seen one is. An
//! entry bigger than the whole bound is handed back uncached.
//!
//! The masks of one photo must never evict each other. A render asks for every
//! mask of the photo in the same order, so with least-recently-used eviction
//! alone, masks that together pass the byte bound (5 masks at the 61 MP preview
//! shown at 100 % zoom: 300 MB) would all miss on every slider tick. So the
//! [`MASK_CACHE_KEEP`] most recently used entries are never evicted, and the
//! byte bound applies to what is older. The cache holds at most the larger of
//! the byte bound and `MASK_CACHE_KEEP` bitmaps, and a photo change empties it
//! (`clear_session_caches`). Only the interactive preview asks for masks that
//! large; thumbnails, the estimate and preset previews ask at small sizes.

use image::GrayImage;
use std::collections::HashMap;
use std::sync::Arc;

/// 256 MiB: 100 masks at preview size (2.5 MB each), 4 at the 61 MP "100 %"
/// preview (60 MB each). Entries past the kept ones below go once the total
/// passes it.
pub const MASK_CACHE_BYTES: usize = 256 * 1024 * 1024;

/// Entries never evicted for bytes: the 8 most recently used, which covers the
/// masks a photo carries. Worst case 8 x 60 MB = 480 MB, at 100 % zoom on a
/// 61 MP photo with 8 masks; 20 MB at preview size.
pub const MASK_CACHE_KEEP: usize = 8;

struct Entry {
    bitmap: Arc<GrayImage>,
    last_used: u64,
}

/// Mask bitmaps by key, never holding more than its byte bound.
pub struct MaskCache {
    entries: HashMap<u64, Entry>,
    bytes: usize,
    bound: usize,
    keep: usize,
    clock: u64,
}

fn bytes_of(bitmap: &GrayImage) -> usize {
    bitmap.as_raw().len()
}

impl Default for MaskCache {
    fn default() -> Self {
        Self::with_limits(MASK_CACHE_BYTES, MASK_CACHE_KEEP)
    }
}

impl MaskCache {
    pub fn with_limits(bound: usize, keep: usize) -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            bound,
            keep,
            clock: 0,
        }
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    /// The bitmap under `key`, marked as just used. No pixel is copied.
    pub fn get(&mut self, key: u64) -> Option<Arc<GrayImage>> {
        let now = self.tick();
        let entry = self.entries.get_mut(&key)?;
        entry.last_used = now;
        Some(entry.bitmap.clone())
    }

    /// Keeps `bitmap` under `key`, evicting the least recently used entries
    /// until it fits, and returns the shared handle (stored or not).
    pub fn insert(&mut self, key: u64, bitmap: GrayImage) -> Arc<GrayImage> {
        let size = bytes_of(&bitmap);
        let bitmap = Arc::new(bitmap);
        if let Some(old) = self.entries.remove(&key) {
            self.bytes -= bytes_of(&old.bitmap);
        }
        if size > self.bound {
            return bitmap;
        }
        while self.bytes + size > self.bound && self.entries.len() >= self.keep {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| *k)
            else {
                break;
            };
            if let Some(gone) = self.entries.remove(&oldest) {
                self.bytes -= bytes_of(&gone.bitmap);
            }
        }
        let last_used = self.tick();
        self.bytes += size;
        self.entries.insert(
            key,
            Entry {
                bitmap: bitmap.clone(),
                last_used,
            },
        );
        bitmap
    }

    /// The bitmap under `key`, or the one `generate` makes, kept for next time.
    /// The lock is not held while `generate` runs.
    pub fn get_or_generate(
        cache: &std::sync::Mutex<Self>,
        key: u64,
        generate: impl FnOnce() -> Option<GrayImage>,
    ) -> Option<Arc<GrayImage>> {
        {
            let mut held = cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(hit) = held.get(key) {
                return Some(hit);
            }
        }
        let bitmap = generate()?;
        let mut held = cache.lock().unwrap_or_else(|e| e.into_inner());
        Some(held.insert(key, bitmap))
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Bytes of pixels held; never above the bound.
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::mem_probe;
    use image::Luma;

    const SIDE: u32 = 1000;
    const SIZE: usize = (SIDE * SIDE) as usize;

    fn bitmap(fill: u8) -> GrayImage {
        GrayImage::from_pixel(SIDE, SIDE, Luma([fill]))
    }

    #[test]
    fn a_second_request_shares_the_bitmap_and_allocates_none() {
        let mut cache = MaskCache::default();
        let first = cache.insert(7, bitmap(9));
        // Anything made once is made before counting.
        let (second, peak) = mem_probe::peak_bytes(|| cache.get(7));
        let second = second.expect("kept");
        assert!(Arc::ptr_eq(&first, &second));
        assert!(
            peak < SIZE / 100,
            "a hit held {peak} bytes; a bitmap is {SIZE}"
        );
    }

    #[test]
    fn it_never_passes_its_byte_bound() {
        let mut cache = MaskCache::with_limits(SIZE * 3 + SIZE / 2, 0);
        for key in 0..40 {
            cache.insert(key, bitmap(key as u8));
            assert!(cache.bytes() <= SIZE * 3 + SIZE / 2);
            assert!(cache.len() <= 3);
        }
        assert_eq!(cache.len(), 3);
        assert_eq!(cache.bytes(), SIZE * 3);
    }

    #[test]
    fn the_least_recently_used_goes_first() {
        let mut cache = MaskCache::with_limits(SIZE * 3, 0);
        for key in 1..=3 {
            cache.insert(key, bitmap(key as u8));
        }
        // 1 is the oldest insert, but a read makes it the newest.
        assert!(cache.get(1).is_some());
        cache.insert(4, bitmap(4));
        assert!(cache.get(2).is_none(), "2 was the least recently used");
        assert!(cache.get(1).is_some());
        assert!(cache.get(3).is_some());
        assert!(cache.get(4).is_some());
    }

    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A stand-in generator: the same pixels for the same key, and a count.
    fn generated(key: u64, made: &AtomicUsize) -> Option<GrayImage> {
        made.fetch_add(1, Ordering::SeqCst);
        Some(GrayImage::from_fn(SIDE, SIDE, |x, y| {
            Luma([(x as u64 * 7 + y as u64 * 3 + key) as u8])
        }))
    }

    #[test]
    fn an_evicted_mask_is_regenerated_with_the_same_pixels() {
        let cache = Mutex::new(MaskCache::with_limits(SIZE, 0));
        let made = AtomicUsize::new(0);
        let first = MaskCache::get_or_generate(&cache, 1, || generated(1, &made)).unwrap();
        MaskCache::get_or_generate(&cache, 2, || generated(2, &made)).unwrap();
        assert_eq!(made.load(Ordering::SeqCst), 2);
        let again = MaskCache::get_or_generate(&cache, 1, || generated(1, &made)).unwrap();
        assert_eq!(
            made.load(Ordering::SeqCst),
            3,
            "1 was evicted, so made again"
        );
        assert!(!Arc::ptr_eq(&first, &again));
        assert_eq!(first.as_raw(), again.as_raw());
    }

    #[test]
    fn one_photos_masks_larger_than_the_budget_are_each_made_once() {
        // Six masks, a budget of two: without the kept entries all six would
        // be made again on every pass.
        let cache = Mutex::new(MaskCache::with_limits(SIZE * 2, 6));
        let made = AtomicUsize::new(0);
        for _pass in 0..3 {
            for key in 0..6 {
                MaskCache::get_or_generate(&cache, key, || generated(key, &made)).unwrap();
            }
        }
        assert_eq!(made.load(Ordering::SeqCst), 6);
        assert_eq!(cache.lock().unwrap().len(), 6);
    }

    #[test]
    fn masks_beyond_the_kept_ones_still_go_when_over_the_budget() {
        let mut cache = MaskCache::with_limits(SIZE * 2, 2);
        for key in 0..10 {
            cache.insert(key, bitmap(1));
        }
        assert_eq!(cache.len(), 2);
        assert!(cache.get(9).is_some() && cache.get(8).is_some());
    }

    #[test]
    fn a_bitmap_bigger_than_the_whole_bound_is_returned_but_not_kept() {
        let mut cache = MaskCache::with_limits(SIZE - 1, 0);
        let handed = cache.insert(1, bitmap(5));
        assert_eq!(handed.as_raw().len(), SIZE);
        assert!(cache.is_empty());
        assert_eq!(cache.bytes(), 0);
    }

    #[test]
    fn replacing_a_key_does_not_count_it_twice() {
        let mut cache = MaskCache::with_limits(SIZE * 2, 0);
        cache.insert(1, bitmap(1));
        cache.insert(1, bitmap(2));
        assert_eq!(cache.bytes(), SIZE);
        assert_eq!(cache.len(), 1);
    }
}
