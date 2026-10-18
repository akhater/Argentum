//! Browsing's budgets, measured on a real folder: `AG_BROWSE_DIR` names one,
//! the bigger the better (1,000 photos or more). Unset, they say they skipped.
//!
//! Each budget was set from its first measurements (2026-10-18: folders of
//! 1,069 to 1,162 CR2s and CR3s never read before, 8 workers, nothing
//! cached; a CR2's only preview big enough is its full-size JPEG, the slowest
//! kind) and is never raised.
//!
//!     AG_BROWSE_DIR=C:\photos\big cargo test --lib mods::browse::budgets -- --nocapture

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::cache::{DISK_BUDGET_BYTES, DiskCache, MEMORY_BUDGET_BYTES, MemoryCache};
use super::contract::{BrowseEvent, THUMB_PX};
use super::listing::Listings;
use super::thumbnails::Thumbnails;
use super::workers;

/// Listing the whole folder, every photo stat'ed and its sidecar looked for.
const LIST_BUDGET: Duration = Duration::from_millis(250);
/// From asking for the visible thumbnails to the first one ready, with
/// nothing cached.
const FIRST_THUMB_BUDGET: Duration = Duration::from_millis(400);
/// From asking to all of a screenful ready, nothing cached.
const SCREENFUL_BUDGET: Duration = Duration::from_millis(1000);
/// The same screenful from the disk cache, after a restart.
const SCREENFUL_CACHED_BUDGET: Duration = Duration::from_millis(100);
/// What browsing a whole folder may add to the process's memory: the
/// thumbnails kept in memory, and a fixed part for the workers and the disk
/// cache's index. Measured: 82 MB for a folder of 1,069, its cache 28 MB.
const MEMORY_GROWTH_BUDGET_MB: f64 = (MEMORY_BUDGET_BYTES >> 20) as f64 + 64.0;
/// A screenful: six columns by five rows of a 1600 x 900 window.
const SCREENFUL: usize = 30;

fn folder() -> Option<PathBuf> {
    std::env::var_os("AG_BROWSE_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

fn listed(dir: &Path) -> (Vec<PathBuf>, Duration, Duration) {
    let started = Instant::now();
    let mut first = None;
    let mut paths = Vec::new();
    Listings::default().start(dir).unwrap().stream(|part, _| {
        first.get_or_insert_with(|| started.elapsed());
        paths.extend(part.into_iter().map(|p| PathBuf::from(p.path)));
    });
    paths.sort();
    (paths, first.unwrap_or_default(), started.elapsed())
}

/// The process's committed memory, Task Manager's "Memory", once the workers
/// have gone idle and handed back what they freed. (Its working set would
/// also count the pages of files mapped while they are read.)
fn process_mb() -> f64 {
    std::thread::sleep(Duration::from_millis(200));
    crate::mods::memory::committed() as f64 / 1e6
}

/// Thumbnails for `wanted`, timed: to the first, and to the last.
fn timed(
    thumbnails: &Arc<Thumbnails>,
    rx: &mpsc::Receiver<BrowseEvent>,
    wanted: &[PathBuf],
) -> (Duration, Duration) {
    let started = Instant::now();
    thumbnails.want(wanted.to_vec());
    let mut first = None;
    for _ in wanted {
        match rx.recv_timeout(Duration::from_secs(60)).unwrap() {
            BrowseEvent::Thumb { .. } => {
                first.get_or_insert_with(|| started.elapsed());
            }
            BrowseEvent::NoThumb { path, error } => panic!("{path}: {error}"),
            other => panic!("{other:?}"),
        }
    }
    (first.unwrap_or_default(), started.elapsed())
}

fn started(cache: &Path) -> (Arc<Thumbnails>, mpsc::Receiver<BrowseEvent>) {
    let thumbnails = Arc::new(Thumbnails::new(
        THUMB_PX,
        MemoryCache::new(MEMORY_BUDGET_BYTES),
        Some(DiskCache::new(cache.to_path_buf(), DISK_BUDGET_BYTES)),
    ));
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    thumbnails
        .start(
            workers(),
            Arc::new(move |e: &BrowseEvent| {
                let _ = tx.lock().unwrap().send(e.clone());
            }),
        )
        .unwrap();
    (thumbnails, rx)
}

#[test]
fn a_big_folder_lists_and_shows_its_first_screenful_within_budget() {
    let Some(dir) = folder() else {
        eprintln!("browse budgets: SKIPPED - set AG_BROWSE_DIR to a folder of photos");
        return;
    };
    let (paths, first_part, all) = listed(&dir);
    eprintln!(
        "listing: {} photos, first part {:.1} ms, all {:.1} ms",
        paths.len(),
        first_part.as_secs_f64() * 1000.0,
        all.as_secs_f64() * 1000.0
    );
    assert!(paths.len() > SCREENFUL);
    assert!(all <= LIST_BUDGET, "listing took {all:?}");

    let cache = tempfile::tempdir().unwrap();
    let mb_before = process_mb();
    let (thumbnails, rx) = started(cache.path());
    let screen = &paths[..SCREENFUL];
    let (first, screenful) = timed(&thumbnails, &rx, screen);
    eprintln!(
        "{} workers, nothing cached: first thumbnail {:.0} ms, a screenful of {SCREENFUL} {:.0} ms",
        workers(),
        first.as_secs_f64() * 1000.0,
        screenful.as_secs_f64() * 1000.0
    );

    // Then the rest of the folder, as a scroll to the end would ask for it,
    // to see memory and the caches hold their bounds: the process grows by
    // what the caches hold, not by the number of photos.
    let mb_screenful = process_mb();
    let half = paths.len() / 2;
    let (_, first_half) = timed(&thumbnails, &rx, &paths[SCREENFUL..half]);
    let mb_half = process_mb();
    let (_, second_half) = timed(&thumbnails, &rx, &paths[half..]);
    let rest = first_half + second_half;
    let mb_after = process_mb();
    eprintln!(
        "the whole folder: {:.1} s, {:.1} thumbnails a second; memory cache {:.1} MB (budget {} MB), \
         disk cache {:.1} MB (budget {} MB); process {mb_before:.0} MB at the start, \
         {mb_screenful:.0} after a screenful, {mb_half:.0} at half the folder, {mb_after:.0} at all of it",
        rest.as_secs_f64(),
        (paths.len() - SCREENFUL) as f64 / rest.as_secs_f64(),
        thumbnails.memory_bytes() as f64 / 1e6,
        MEMORY_BUDGET_BYTES >> 20,
        thumbnails.disk_bytes() as f64 / 1e6,
        DISK_BUDGET_BYTES >> 20,
    );
    assert!(thumbnails.memory_bytes() <= MEMORY_BUDGET_BYTES);
    assert!(thumbnails.disk_bytes() <= DISK_BUDGET_BYTES);
    assert!(
        mb_after - mb_before <= MEMORY_GROWTH_BUDGET_MB,
        "browsing added {:.0} MB",
        mb_after - mb_before
    );

    // A restart: memory empty, the disk cache full.
    let (again, rx) = started(cache.path());
    let (first_cached, cached) = timed(&again, &rx, screen);
    eprintln!(
        "after a restart, from the disk cache: first {:.1} ms, a screenful {:.1} ms",
        first_cached.as_secs_f64() * 1000.0,
        cached.as_secs_f64() * 1000.0
    );

    assert!(
        first <= FIRST_THUMB_BUDGET,
        "first thumbnail took {first:?}"
    );
    assert!(
        screenful <= SCREENFUL_BUDGET,
        "a screenful took {screenful:?}"
    );
    assert!(
        cached <= SCREENFUL_CACHED_BUDGET,
        "a cached screenful took {cached:?}"
    );
}
