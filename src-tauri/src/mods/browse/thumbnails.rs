//! Thumbnails as the grid gets them: from memory, else from disk, else made,
//! on worker threads of their own, the most wanted first.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::cache::{DiskCache, Key, MemoryCache};
use super::contract::{BrowseError, BrowseEvent};
use super::queue::Queue;
use super::thumbnail;

pub struct Thumbnails {
    px: u32,
    memory: MemoryCache,
    disk: Option<DiskCache>,
    queue: Queue,
}

impl Thumbnails {
    /// `disk` is None where there is no cache folder: thumbnails are then
    /// kept in memory only.
    pub fn new(px: u32, memory: MemoryCache, disk: Option<DiskCache>) -> Self {
        Self {
            px,
            memory,
            disk,
            queue: Queue::default(),
        }
    }

    /// Start `workers` threads making the thumbnails wanted, each telling
    /// `emit` when one is ready or cannot be made.
    pub fn start(
        self: &Arc<Self>,
        workers: usize,
        emit: Arc<dyn Fn(&BrowseEvent) + Send + Sync>,
    ) -> std::io::Result<()> {
        for n in 0..workers {
            let this = self.clone();
            let emit = emit.clone();
            std::thread::Builder::new()
                .name(format!("ag-browse-thumbs-{n}"))
                .spawn(move || {
                    loop {
                        let path = this.queue.take();
                        let event = this.event_for(&path);
                        this.queue.done(&path);
                        emit(&event);
                        // A full-size preview takes some 25 MB to decode even
                        // scaled, and the allocator keeps each thread's freed
                        // pages: before waiting, give them back.
                        if this.queue.is_empty() {
                            crate::mods::memory::collect();
                        }
                    }
                })?;
        }
        Ok(())
    }

    /// Make these, in this order, instead of whatever was waiting.
    pub fn want(&self, paths: Vec<PathBuf>) {
        self.queue.want(paths);
    }

    /// The thumbnail last made for `path`, while it is in memory.
    pub fn held(&self, path: &Path) -> Option<Arc<[u8]>> {
        self.memory.latest(path)
    }

    fn event_for(&self, path: &Path) -> BrowseEvent {
        let shown = path.to_string_lossy().into_owned();
        match self.get(path) {
            Ok(_) => BrowseEvent::Thumb { path: shown },
            Err(error) => BrowseEvent::NoThumb { path: shown, error },
        }
    }

    /// The thumbnail of `path` as it is now: kept, or made and kept.
    pub fn get(&self, path: &Path) -> Result<Arc<[u8]>, BrowseError> {
        let key = Key::of(path, self.px)?;
        if let Some(jpeg) = self.memory.get(&key) {
            return Ok(jpeg);
        }
        if let Some(jpeg) = self.disk.as_ref().and_then(|d| d.get(&key)) {
            self.memory.put(key, jpeg.clone());
            return Ok(jpeg);
        }
        let jpeg: Arc<[u8]> = thumbnail::make(path, self.px)?.into();
        if let Some(disk) = &self.disk {
            disk.put(&key, &jpeg);
        }
        self.memory.put(key, jpeg.clone());
        Ok(jpeg)
    }

    pub fn memory_bytes(&self) -> usize {
        self.memory.bytes()
    }

    pub fn disk_bytes(&self) -> u64 {
        self.disk.as_ref().map_or(0, DiskCache::bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::browse::contract::BrowseErrorKind;
    use crate::mods::browse::embedded::tests::{jpeg, tiff_with};
    use std::sync::Mutex;
    use std::sync::mpsc;

    fn folder_of_raws(n: usize) -> (tempfile::TempDir, Vec<PathBuf>) {
        let dir = tempfile::tempdir().unwrap();
        let raw = tiff_with(&[jpeg(160, 120), jpeg(800, 533)]);
        let paths = (0..n)
            .map(|i| {
                let p = dir.path().join(format!("IMG_{i:04}.NEF"));
                std::fs::write(&p, &raw).unwrap();
                p
            })
            .collect();
        (dir, paths)
    }

    #[test]
    fn a_thumbnail_is_made_once_then_kept_in_memory_and_on_disk() {
        let (dir, paths) = folder_of_raws(1);
        let cache = dir.path().join("cache");
        let t = Thumbnails::new(
            400,
            MemoryCache::new(1 << 20),
            Some(DiskCache::new(cache.clone(), 1 << 20)),
        );
        let made = t.get(&paths[0]).unwrap();
        assert!(t.memory_bytes() > 0 && t.disk_bytes() > 0);
        assert_eq!(t.get(&paths[0]).unwrap(), made);

        // A restart: memory is empty, the disk still has it. The photo's bytes
        // are made unreadable, its size and time kept: only the disk can
        // answer now.
        let again = Thumbnails::new(
            400,
            MemoryCache::new(1 << 20),
            Some(DiskCache::new(cache, 1 << 20)),
        );
        let meta = std::fs::metadata(&paths[0]).unwrap();
        std::fs::write(&paths[0], vec![0u8; meta.len() as usize]).unwrap();
        filetime::set_file_mtime(
            &paths[0],
            filetime::FileTime::from_last_modification_time(&meta),
        )
        .unwrap();
        assert_eq!(
            again.get(&paths[0]).unwrap(),
            made,
            "not read back from disk"
        );
    }

    #[test]
    fn a_photo_changed_on_disk_gets_a_new_thumbnail() {
        let (dir, paths) = folder_of_raws(1);
        let t = Thumbnails::new(
            400,
            MemoryCache::new(1 << 20),
            Some(DiskCache::new(dir.path().join("cache"), 1 << 20)),
        );
        let first = t.get(&paths[0]).unwrap();
        std::fs::write(&paths[0], tiff_with(&[jpeg(900, 900)])).unwrap();
        let second = t.get(&paths[0]).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn workers_make_what_is_wanted_and_say_so() {
        let (_dir, paths) = folder_of_raws(12);
        let broken = paths[0].with_file_name("broken.NEF");
        std::fs::write(&broken, b"nothing inside").unwrap();
        let t = Arc::new(Thumbnails::new(400, MemoryCache::new(1 << 20), None));
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        t.start(
            3,
            Arc::new(move |e: &BrowseEvent| tx.lock().unwrap().send(e.clone()).unwrap()),
        )
        .unwrap();
        let mut wanted = paths.clone();
        wanted.push(broken.clone());
        t.want(wanted);
        let mut ready = 0;
        let mut failed = Vec::new();
        for _ in 0..13 {
            match rx.recv_timeout(std::time::Duration::from_secs(20)).unwrap() {
                BrowseEvent::Thumb { path } => {
                    assert!(t.held(Path::new(&path)).is_some());
                    ready += 1;
                }
                BrowseEvent::NoThumb { path, error } => {
                    assert_eq!(error.kind, BrowseErrorKind::NoPreview);
                    failed.push(path);
                }
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(ready, 12);
        assert_eq!(failed, [broken.to_string_lossy().into_owned()]);
    }
}
