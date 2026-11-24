//! The Lensfun database, parsed the first time something needs it.
//!
//! `set_dir` only remembers where the 59 XML files are; `get` parses them once,
//! on the thread that asks first, and every other caller waits for that one
//! parse, so start-up does not wait for the parse.
//! A missing folder gives an empty database, as before (CLAUDE.md, rule 4).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use crate::lens_correction::{self, LensDatabase};

#[derive(Default)]
pub struct LazyLensDb {
    dir: Mutex<Option<PathBuf>>,
    db: OnceLock<Arc<LensDatabase>>,
}

impl LazyLensDb {
    /// Where the Lensfun XML files are. Called once, from `setup`.
    pub fn set_dir(&self, dir: PathBuf) {
        *self.dir.lock().unwrap() = Some(dir);
    }

    /// The database, parsed now if nobody has yet. Blocks while another thread
    /// parses, so call it from a worker, never from the UI thread.
    pub fn get(&self) -> Arc<LensDatabase> {
        self.get_or_load(|dir| match dir {
            Some(dir) => lens_correction::load_lensfun_dir(&dir),
            None => lens_correction::empty_database(),
        })
    }

    fn get_or_load(&self, load: impl FnOnce(Option<PathBuf>) -> LensDatabase) -> Arc<LensDatabase> {
        self.db
            .get_or_init(|| {
                let dir = self.dir.lock().unwrap().clone();
                Arc::new(load(dir))
            })
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn shipped_dir() -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("lensfun_db")
    }

    #[test]
    fn parses_once_when_callers_race() {
        let cell = LazyLensDb::default();
        cell.set_dir(shipped_dir());
        let parses = AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    cell.get_or_load(|dir| {
                        parses.fetch_add(1, Ordering::SeqCst);
                        lens_correction::load_lensfun_dir(&dir.unwrap())
                    });
                });
            }
        });
        assert_eq!(parses.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn first_reader_gets_the_full_database() {
        let cell = LazyLensDb::default();
        cell.set_dir(shipped_dir());
        let db = cell.get();
        assert!(!db.lenses.is_empty());
        assert!(!db.cameras.is_empty());
        assert!(
            Arc::ptr_eq(&db, &cell.get()),
            "second call reuses the first parse"
        );
    }

    #[test]
    fn missing_folder_gives_an_empty_database() {
        let cell = LazyLensDb::default();
        cell.set_dir(shipped_dir().join("does-not-exist"));
        let db = cell.get();
        assert!(db.lenses.is_empty() && db.cameras.is_empty() && db.mounts.is_empty());
    }
}
