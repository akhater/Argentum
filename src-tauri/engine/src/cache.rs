//! Step results by key, within a memory budget, least recently used first
//! out.

use std::collections::HashMap;
use std::sync::Arc;

use crate::Image;

pub(crate) struct Cache {
    entries: HashMap<u64, Entry>,
    budget_bytes: usize,
    used_bytes: usize,
    clock: u64,
}

struct Entry {
    image: Arc<Image>,
    /// What this entry adds to memory: nothing when it is a step that passed
    /// its input through, which another entry already counts.
    bytes: usize,
    used: u64,
}

impl Cache {
    pub(crate) fn new(budget_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            budget_bytes,
            used_bytes: 0,
            clock: 0,
        }
    }

    pub(crate) fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    pub(crate) fn set_budget_bytes(&mut self, bytes: usize) {
        self.budget_bytes = bytes;
        self.evict();
    }

    /// The result kept under `key`, marked as just used.
    pub(crate) fn get(&mut self, key: u64) -> Option<Arc<Image>> {
        self.clock += 1;
        let entry = self.entries.get_mut(&key)?;
        entry.used = self.clock;
        Some(entry.image.clone())
    }

    /// Keep `image` under `key`. `fresh` when the step made it, rather than
    /// passing its input through.
    pub(crate) fn put(&mut self, key: u64, image: Arc<Image>, fresh: bool) {
        self.clock += 1;
        let bytes = if fresh { image.bytes() } else { 0 };
        if let Some(old) = self.entries.insert(
            key,
            Entry {
                image,
                bytes,
                used: self.clock,
            },
        ) {
            self.used_bytes -= old.bytes;
        }
        self.used_bytes += bytes;
        self.evict();
    }

    fn evict(&mut self) {
        while self.used_bytes > self.budget_bytes && self.entries.len() > 1 {
            let Some((&oldest, _)) = self.entries.iter().min_by_key(|(_, e)| e.used) else {
                return;
            };
            if let Some(gone) = self.entries.remove(&oldest) {
                self.used_bytes -= gone.bytes;
            }
        }
    }
}
