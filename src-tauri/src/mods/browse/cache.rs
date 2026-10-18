//! Where made thumbnails are kept: in memory and on disk, each within a
//! budget in bytes, the least recently used going first.
//!
//! A thumbnail is kept under its photo's path, size and time of writing and
//! the thumbnail's size, so a photo that changes on disk gets a new one, and
//! an old one is simply never asked for again until it ages out.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use super::contract::BrowseError;

/// Thumbnails kept in memory, in bytes of JPEG: some 2,500 at `THUMB_PX`.
pub const MEMORY_BUDGET_BYTES: usize = 64 << 20;
/// Thumbnails kept on disk, in bytes: some 20,000.
pub const DISK_BUDGET_BYTES: u64 = 512 << 20;
/// Once over its budget, the disk cache is trimmed to this share of it, so
/// it is not trimmed again at every new thumbnail.
const DISK_TRIM_TO: f64 = 0.9;

/// One version of one photo's thumbnail.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key {
    pub path: PathBuf,
    pub bytes: u64,
    pub modified_ms: u64,
    pub px: u32,
}

impl Key {
    /// The key of `path` as it is on disk now: one stat.
    pub fn of(path: &Path, px: u32) -> Result<Key, BrowseError> {
        let meta = std::fs::metadata(path).map_err(|e| BrowseError::io(path, &e))?;
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as u64);
        Ok(Key {
            path: path.to_path_buf(),
            bytes: meta.len(),
            modified_ms,
            px,
        })
    }

    /// Its file's name in the disk cache.
    fn file_name(&self) -> String {
        let mut h = blake3::Hasher::new();
        h.update(self.path.to_string_lossy().as_bytes());
        h.update(&self.bytes.to_le_bytes());
        h.update(&self.modified_ms.to_le_bytes());
        h.update(&self.px.to_le_bytes());
        format!("{}.jpg", &h.finalize().to_hex()[..32])
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Thumbnails in memory, one per photo.
pub struct MemoryCache {
    budget_bytes: usize,
    held: Mutex<Held>,
}

#[derive(Default)]
struct Held {
    by_path: HashMap<PathBuf, (Key, Arc<[u8]>, u64)>,
    /// Paths by when they were last used.
    order: BTreeMap<u64, PathBuf>,
    tick: u64,
    bytes: usize,
}

impl Held {
    fn touch(&mut self, path: &Path) {
        self.tick += 1;
        let tick = self.tick;
        if let Some(entry) = self.by_path.get_mut(path) {
            self.order.remove(&entry.2);
            entry.2 = tick;
            self.order.insert(tick, path.to_path_buf());
        }
    }

    fn remove(&mut self, path: &Path) {
        if let Some((_, jpeg, used)) = self.by_path.remove(path) {
            self.order.remove(&used);
            self.bytes -= jpeg.len();
        }
    }
}

impl MemoryCache {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            budget_bytes,
            held: Mutex::default(),
        }
    }

    /// The thumbnail for exactly this version of the photo.
    pub fn get(&self, key: &Key) -> Option<Arc<[u8]>> {
        let mut held = lock(&self.held);
        let jpeg = held
            .by_path
            .get(&key.path)
            .filter(|(k, _, _)| k == key)
            .map(|(_, j, _)| j.clone())?;
        held.touch(&key.path);
        Some(jpeg)
    }

    /// Whichever thumbnail is held for `path`: the one just made, for the
    /// interface to fetch.
    pub fn latest(&self, path: &Path) -> Option<Arc<[u8]>> {
        let mut held = lock(&self.held);
        let jpeg = held.by_path.get(path).map(|(_, j, _)| j.clone())?;
        held.touch(path);
        Some(jpeg)
    }

    /// Keep it, letting the least recently used go until it fits.
    pub fn put(&self, key: Key, jpeg: Arc<[u8]>) {
        if jpeg.len() > self.budget_bytes {
            return;
        }
        let mut held = lock(&self.held);
        held.remove(&key.path);
        while held.bytes + jpeg.len() > self.budget_bytes {
            let Some((_, oldest)) = held.order.pop_first() else {
                break;
            };
            if let Some((_, j, _)) = held.by_path.remove(&oldest) {
                held.bytes -= j.len();
            }
        }
        held.tick += 1;
        let tick = held.tick;
        held.bytes += jpeg.len();
        held.order.insert(tick, key.path.clone());
        held.by_path.insert(key.path.clone(), (key, jpeg, tick));
    }

    #[cfg(test)]
    pub fn bytes(&self) -> usize {
        lock(&self.held).bytes
    }
}

/// Thumbnails on disk, one file each, in a folder of their own.
///
/// Its index is read from the folder the first time it is used, and a file's
/// time of last use is its modified time, touched at each use, so the order
/// survives a restart.
pub struct DiskCache {
    dir: PathBuf,
    budget_bytes: u64,
    index: Mutex<Option<Index>>,
}

#[derive(Default)]
struct Index {
    /// File name to its size and last use, in milliseconds.
    files: HashMap<String, (u64, u64)>,
    bytes: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

impl DiskCache {
    pub fn new(dir: PathBuf, budget_bytes: u64) -> Self {
        Self {
            dir,
            budget_bytes,
            index: Mutex::new(None),
        }
    }

    fn index(&self) -> MutexGuard<'_, Option<Index>> {
        let mut index = lock(&self.index);
        if index.is_none() {
            *index = Some(self.read_index());
        }
        index
    }

    fn read_index(&self) -> Index {
        let mut index = Index::default();
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return index;
        };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            // A write cut short by a crash.
            if name.ends_with(".tmp") {
                let _ = std::fs::remove_file(entry.path());
                continue;
            }
            let used = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64);
            index.bytes += meta.len();
            index.files.insert(name, (meta.len(), used));
        }
        index
    }

    pub fn get(&self, key: &Key) -> Option<Arc<[u8]>> {
        let name = key.file_name();
        if !self.index().as_ref()?.files.contains_key(&name) {
            return None;
        }
        let path = self.dir.join(&name);
        let read = std::fs::read(&path);
        if read.is_ok() {
            let _ = filetime::set_file_mtime(&path, filetime::FileTime::now());
        }
        let mut index = self.index();
        let index = index.as_mut()?;
        match read {
            Ok(jpeg) => {
                if let Some(entry) = index.files.get_mut(&name) {
                    entry.1 = now_ms();
                }
                Some(jpeg.into())
            }
            Err(_) => {
                if let Some((bytes, _)) = index.files.remove(&name) {
                    index.bytes -= bytes;
                }
                None
            }
        }
    }

    /// Keep it, then trim to budget if over it. A thumbnail that cannot be
    /// written is only not kept.
    pub fn put(&self, key: &Key, jpeg: &[u8]) {
        let name = key.file_name();
        if let Err(e) = self.write(&name, jpeg) {
            log::warn!("[browse] thumbnail not cached on disk: {e}");
            return;
        }
        let mut index = self.index();
        let Some(index) = index.as_mut() else { return };
        if let Some((old, _)) = index.files.insert(name, (jpeg.len() as u64, now_ms())) {
            index.bytes -= old;
        }
        index.bytes += jpeg.len() as u64;
        if index.bytes > self.budget_bytes {
            self.trim(index);
        }
    }

    fn write(&self, name: &str, jpeg: &[u8]) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!("{name}.tmp"));
        std::fs::write(&tmp, jpeg)?;
        std::fs::rename(&tmp, self.dir.join(name))
    }

    fn trim(&self, index: &mut Index) {
        let target = (self.budget_bytes as f64 * DISK_TRIM_TO) as u64;
        let mut by_use: Vec<(u64, String, u64)> = index
            .files
            .iter()
            .map(|(name, (bytes, used))| (*used, name.clone(), *bytes))
            .collect();
        by_use.sort_unstable();
        for (_, name, bytes) in by_use {
            if index.bytes <= target {
                break;
            }
            let _ = std::fs::remove_file(self.dir.join(&name));
            index.files.remove(&name);
            index.bytes -= bytes;
        }
    }

    #[cfg(test)]
    pub fn bytes(&self) -> u64 {
        self.index().as_ref().map_or(0, |i| i.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: usize) -> Key {
        Key {
            path: PathBuf::from(format!("C:/photos/IMG_{n:04}.CR3")),
            bytes: 30_000_000,
            modified_ms: 1_760_000_000_000,
            px: 400,
        }
    }

    fn jpeg(len: usize) -> Arc<[u8]> {
        vec![7u8; len].into()
    }

    #[test]
    fn memory_stays_within_its_budget_dropping_the_least_recently_used() {
        let cache = MemoryCache::new(10_000);
        for n in 0..5000 {
            cache.put(key(n), jpeg(1000 + n % 7));
            assert!(cache.bytes() <= 10_000);
        }
        // Use the oldest still held, then add one more: the one after it goes.
        let oldest = (0..5000).find(|&n| cache.get(&key(n)).is_some()).unwrap();
        cache.put(key(9999), jpeg(1000));
        assert!(cache.get(&key(oldest)).is_some());
        assert!(cache.get(&key(oldest + 1)).is_none());
        assert!(cache.bytes() <= 10_000);
    }

    #[test]
    fn memory_tells_versions_apart_and_hands_over_the_latest() {
        let cache = MemoryCache::new(10_000);
        cache.put(key(1), jpeg(10));
        let changed = Key {
            modified_ms: key(1).modified_ms + 1,
            ..key(1)
        };
        assert!(cache.get(&changed).is_none());
        cache.put(changed.clone(), jpeg(20));
        assert_eq!(cache.get(&changed).unwrap().len(), 20);
        assert_eq!(cache.latest(&key(1).path).unwrap().len(), 20);
        // One per photo: the old version went.
        assert_eq!(cache.bytes(), 20);
    }

    #[test]
    fn disk_stays_within_its_budget_and_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let budget = 50_000;
        let cache = DiskCache::new(dir.path().to_path_buf(), budget);
        for n in 0..200 {
            cache.put(&key(n), &jpeg(1000));
            assert!(cache.bytes() <= budget);
        }
        let on_disk: u64 = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().metadata().unwrap().len())
            .sum();
        assert!(on_disk <= budget, "{on_disk} bytes on disk");
        // The newest are kept; the oldest went.
        assert!(cache.get(&key(199)).is_some());
        assert!(cache.get(&key(0)).is_none());

        let again = DiskCache::new(dir.path().to_path_buf(), budget);
        assert_eq!(again.bytes(), on_disk);
        assert_eq!(again.get(&key(199)).unwrap().len(), 1000);
    }

    #[test]
    fn a_changed_photo_misses_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path().to_path_buf(), 50_000);
        cache.put(&key(1), &jpeg(10));
        let bigger = Key {
            bytes: key(1).bytes + 1,
            ..key(1)
        };
        assert!(cache.get(&bigger).is_none());
        assert!(cache.get(&key(1)).is_some());
    }

    #[test]
    fn a_file_deleted_behind_its_back_is_a_miss_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path().to_path_buf(), 50_000);
        cache.put(&key(1), &jpeg(10));
        for e in std::fs::read_dir(dir.path()).unwrap() {
            std::fs::remove_file(e.unwrap().path()).unwrap();
        }
        assert!(cache.get(&key(1)).is_none());
        assert_eq!(cache.bytes(), 0);
    }

    #[test]
    fn a_key_is_one_stat_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.CR3");
        std::fs::write(&p, b"12345").unwrap();
        let k = Key::of(&p, 400).unwrap();
        assert_eq!(k.bytes, 5);
        assert_eq!(Key::of(&p, 400).unwrap().file_name(), k.file_name());
        assert_ne!(Key::of(&p, 200).unwrap().file_name(), k.file_name());
    }
}
