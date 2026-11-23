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

use image::GrayImage;
use std::collections::HashMap;
use std::sync::Arc;

/// 256 MiB: 100 masks at preview size (2.5 MB each), 4 at the 61 MP "100 %"
/// preview (60 MB each). More than the 5 to 10 masks a photo carries at either
/// size, small beside the photo caches it sits next to.
pub const MASK_CACHE_BYTES: usize = 256 * 1024 * 1024;

struct Entry {
    bitmap: Arc<GrayImage>,
    last_used: u64,
}

/// Mask bitmaps by key, never holding more than its byte bound.
pub struct MaskCache {
    entries: HashMap<u64, Entry>,
    bytes: usize,
    bound: usize,
    clock: u64,
}

fn bytes_of(bitmap: &GrayImage) -> usize {
    bitmap.as_raw().len()
}

impl Default for MaskCache {
    fn default() -> Self {
        Self::with_bound(MASK_CACHE_BYTES)
    }
}

impl MaskCache {
    pub fn with_bound(bound: usize) -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            bound,
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
        while self.bytes + size > self.bound {
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
        let mut cache = MaskCache::with_bound(SIZE * 3 + SIZE / 2);
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
        let mut cache = MaskCache::with_bound(SIZE * 3);
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

    #[test]
    fn an_evicted_entry_comes_back_with_the_same_pixels() {
        let mut cache = MaskCache::with_bound(SIZE);
        let before = cache.insert(1, bitmap(33));
        cache.insert(2, bitmap(44));
        assert!(cache.get(1).is_none());
        let rebuilt = cache.insert(1, bitmap(33));
        assert_eq!(before.as_raw(), rebuilt.as_raw());
    }

    #[test]
    fn a_bitmap_bigger_than_the_whole_bound_is_returned_but_not_kept() {
        let mut cache = MaskCache::with_bound(SIZE - 1);
        let handed = cache.insert(1, bitmap(5));
        assert_eq!(handed.as_raw().len(), SIZE);
        assert!(cache.is_empty());
        assert_eq!(cache.bytes(), 0);
    }

    #[test]
    fn replacing_a_key_does_not_count_it_twice() {
        let mut cache = MaskCache::with_bound(SIZE * 2);
        cache.insert(1, bitmap(1));
        cache.insert(1, bitmap(2));
        assert_eq!(cache.bytes(), SIZE);
        assert_eq!(cache.len(), 1);
    }
}
