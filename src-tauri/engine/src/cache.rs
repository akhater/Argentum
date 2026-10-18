//! Step results by key, within a memory budget, least recently used first
//! out.
//!
//! Memory is counted by what the entries hold, not by entry: a step that
//! passes its input through keeps the same picture as the step before it,
//! and that picture is counted once, for as long as any entry holds it - so
//! evicting the entry that made it frees nothing while another still holds
//! it, and the count says so.

use std::collections::HashMap;
use std::sync::Arc;

use crate::Image;

pub(crate) struct Cache {
    entries: HashMap<u64, Entry>,
    /// Every picture the entries hold, by address, and how many hold it.
    held: HashMap<usize, usize>,
    budget_bytes: usize,
    used_bytes: usize,
    clock: u64,
    /// Let go last: what the next tick of the slider last moved starts from.
    keep: Option<u64>,
}

struct Entry {
    image: Arc<Image>,
    /// The photo it is a result of.
    source: u64,
    used: u64,
}

impl Cache {
    pub(crate) fn new(budget_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            held: HashMap::new(),
            budget_bytes,
            used_bytes: 0,
            clock: 0,
            keep: None,
        }
    }

    pub(crate) fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    pub(crate) fn set_budget_bytes(&mut self, bytes: usize) {
        self.budget_bytes = bytes;
        self.evict(0, None, None);
    }

    pub(crate) fn keep(&mut self, key: Option<u64>) {
        self.keep = key;
    }

    /// The result kept under `key`, marked as just used.
    pub(crate) fn get(&mut self, key: u64) -> Option<Arc<Image>> {
        self.clock += 1;
        let entry = self.entries.get_mut(&key)?;
        entry.used = self.clock;
        Some(entry.image.clone())
    }

    /// Keep `image`, a result of the photo `source`, under `key`, and let
    /// results go until what is kept fits the budget - it last.
    pub(crate) fn put(&mut self, key: u64, source: u64, image: Arc<Image>) {
        self.clock += 1;
        let holders = self.held.entry(address(&image)).or_insert(0);
        if *holders == 0 {
            self.used_bytes += image.bytes();
        }
        *holders += 1;
        let entry = Entry {
            image,
            source,
            used: self.clock,
        };
        if let Some(old) = self.entries.insert(key, entry) {
            self.release(old);
        }
        self.evict(0, Some(key), None);
    }

    /// Let results go until `bytes` more fit the budget: room for a result
    /// before it is made from the one under `input`. That one stays: letting
    /// it go would free nothing while it is being read.
    pub(crate) fn make_room(&mut self, bytes: usize, input: u64) {
        self.evict(bytes, None, Some(input));
    }

    /// Let go of every result of the photo `source`.
    pub(crate) fn forget_source(&mut self, source: u64) {
        let gone: Vec<u64> = self
            .entries
            .iter()
            .filter(|(_, e)| e.source == source)
            .map(|(&k, _)| k)
            .collect();
        for key in gone {
            if let Some(entry) = self.entries.remove(&key) {
                self.release(entry);
            }
        }
    }

    fn release(&mut self, entry: Entry) {
        let at = address(&entry.image);
        if let Some(holders) = self.held.get_mut(&at) {
            *holders -= 1;
            if *holders == 0 {
                self.held.remove(&at);
                self.used_bytes -= entry.image.bytes();
            }
        }
    }

    /// The least recently used first; `keep` and `last` only when nothing
    /// else is left, and `never` not at all.
    fn evict(&mut self, incoming: usize, last: Option<u64>, never: Option<u64>) {
        let keep = self.keep;
        while self.used_bytes + incoming > self.budget_bytes {
            let next = self
                .entries
                .iter()
                .filter(|&(&k, _)| Some(k) != never)
                .min_by_key(|&(&k, e)| (Some(k) == keep || Some(k) == last, e.used))
                .map(|(&k, _)| k);
            let Some(next) = next else {
                return;
            };
            if let Some(entry) = self.entries.remove(&next) {
                self.release(entry);
            }
        }
    }
}

fn address(image: &Arc<Image>) -> usize {
    Arc::as_ptr(image) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rgb;

    /// 12 bytes a pixel.
    fn picture(px: usize) -> Arc<Image> {
        Arc::new(Image::Rgb(Rgb::new(px, 1, vec![[0.0; 3]; px])))
    }

    #[test]
    fn a_picture_held_twice_is_counted_once_until_both_let_go() {
        let mut cache = Cache::new(1 << 30);
        let made = picture(100);
        cache.put(1, 9, made.clone());
        // A step that passed its input through: the same picture again.
        cache.put(2, 9, made);
        assert_eq!(cache.used_bytes(), 1200);
        // The entry that made it goes first; the other still holds it.
        let first = cache.entries.remove(&1).unwrap();
        cache.release(first);
        assert_eq!(cache.used_bytes(), 1200);
        let second = cache.entries.remove(&2).unwrap();
        cache.release(second);
        assert_eq!(cache.used_bytes(), 0);
    }

    #[test]
    fn eviction_goes_on_until_memory_is_really_freed() {
        let mut cache = Cache::new(2000);
        let a = picture(100);
        cache.put(1, 9, a.clone());
        cache.put(2, 9, a);
        // Over budget: letting entry 1 go frees nothing while 2 holds the
        // same picture, so 2 goes too.
        cache.put(3, 9, picture(100));
        assert_eq!(cache.used_bytes(), 1200);
        assert!(cache.get(3).is_some() && cache.get(1).is_none() && cache.get(2).is_none());
    }

    #[test]
    fn room_is_made_before_a_result_the_kept_one_going_last_and_never_its_input() {
        let mut cache = Cache::new(3700);
        for key in 1..=3 {
            cache.put(key, 9, picture(100));
        }
        cache.keep(Some(1));
        // Room for 1200 more, made from 3: 2 goes, though 1 is older.
        cache.make_room(1200, 3);
        assert_eq!(cache.used_bytes(), 2400);
        assert!(cache.get(1).is_some() && cache.get(2).is_none() && cache.get(3).is_some());
        // Then 1, when nothing else is left; 3 is being read.
        cache.make_room(5000, 3);
        assert_eq!(cache.used_bytes(), 1200);
        assert!(cache.get(3).is_some());
    }

    #[test]
    fn what_is_kept_always_fits_the_budget() {
        let mut cache = Cache::new(2000);
        cache.put(1, 9, picture(100));
        cache.keep(Some(1));
        // Both the kept one and the newest are let go last; one has to go.
        cache.put(2, 9, picture(100));
        assert_eq!(cache.used_bytes(), 1200);
    }

    #[test]
    fn a_closed_photo_is_forgotten() {
        let mut cache = Cache::new(1 << 30);
        cache.put(1, 9, picture(10));
        cache.put(2, 8, picture(10));
        cache.forget_source(9);
        assert_eq!(cache.used_bytes(), 120);
        assert!(cache.get(1).is_none() && cache.get(2).is_some());
    }
}
