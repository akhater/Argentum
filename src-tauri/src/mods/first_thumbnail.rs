//! The first thumbnail of a RAW in the grid: the JPEG its camera embedded,
//! shown at once, before the thumbnail is made.
//!
//! Opening a folder queues every photo for a thumbnail (`file_management.rs`),
//! and making one can mean developing the RAW: a CR2's only big preview is
//! the full-size JPEG in its first IFD, which their reader does not find, so
//! it develops the sensor data. Meanwhile the cell is empty. So while
//! browsing, the camera's own JPEG goes out first - read by Shoebox's reader
//! (`sb_embedded.rs`) and decoded at the small thumbnail's size - as the
//! `data` of a `thumbnail-generated` event, which the grid already takes. The
//! thumbnail made after it, with the photo's edits, replaces it as before.
//! Nothing is written.
//!
//! Two things send it, so that every photo the grid asks for has it at once,
//! not when the queue reaches it:
//!
//! - The watcher, a thread that waits on the browsing queue (`ThumbnailManager`
//!   in app_state.rs) as its workers do, and sends the first thumbnail of every
//!   photo waiting in it: newest first, a few at a time in parallel, on a pool
//!   of its own. A photo that left the queue (another folder was opened) is
//!   dropped.
//! - `emit`, which `generate_single_thumbnail_and_cache` calls as a worker
//!   starts on a photo with no cached thumbnail, for the browsing queue only:
//!   every other caller forces a new thumbnail after an edit, where the
//!   camera's picture would flash the photo back to how it was shot. It sends
//!   the first thumbnail of a photo the watcher did not reach, and its first
//!   call starts the watcher.
//!
//! The thumbnail the queue makes always arrives after the first one. The
//! watcher sends a photo's only while the photo is still in the queue, so no
//! worker has started it, and holds `SENT` while sending; a worker that takes
//! the photo meanwhile waits for `SENT` in `emit` before making anything. The
//! main thread never takes `SENT`, so sending while holding it cannot wait on a
//! main thread that waits on the watcher.

use std::collections::{BTreeSet, HashSet, VecDeque};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, MutexGuard, Once, PoisonError};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use image::DynamicImage;
use rawler::decoders::RawDecodeParams;
use rawler::rawsource::RawSource;
use rayon::prelude::*;
use tauri::{AppHandle, Emitter, Manager};

use super::sb_embedded;
use crate::app_state::ThumbnailManager;

/// As the thumbnails it stands in for (`encode_thumbnail` in file_management.rs).
const QUALITY: u8 = 75;

/// Photos whose first thumbnail the watcher sent while they waited in the
/// queue. The worker that takes one sends nothing more for it.
static SENT: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

static WATCHER: Once = Once::new();

/// Send `path`'s first thumbnail, `long_edge_px` on its long edge, if it is a
/// RAW whose camera embedded a JPEG and the watcher has not sent it already.
/// Starts the watcher on its first call.
pub fn emit(app: &AppHandle, path: &str, source: &Path, long_edge_px: u32) {
    WATCHER.call_once(|| start_watcher(app.clone()));
    if lock(&SENT).remove(path) {
        return;
    }
    if let Some(jpeg) = make(source, long_edge_px) {
        send(app, path, &data_url(&jpeg));
    }
}

fn send(app: &AppHandle, path: &str, data_url: &str) {
    let _ = app.emit(
        "thumbnail-generated",
        serde_json::json!({ "path": path, "data": data_url }),
    );
}

fn data_url(jpeg: &[u8]) -> String {
    format!("data:image/jpeg;base64,{}", STANDARD.encode(jpeg))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn start_watcher(app: AppHandle) {
    let started = std::thread::Builder::new()
        .name("first-thumbnails".into())
        .spawn(move || watch(&app));
    if let Err(e) = started {
        log::warn!("first thumbnails: the watcher did not start: {e}");
    }
}

// sb_: Shoebox src-tauri/src/cull.rs Quality::unread @ 972e878, its pool: half
// the cores, at most six, so the workers making thumbnails keep the rest.
fn sb_pool() -> Result<rayon::ThreadPool, rayon::ThreadPoolBuildError> {
    let threads = (std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get) / 2)
        .clamp(1, 6);
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|i| format!("first-thumbnail-{i}"))
        .build()
}

fn watch(app: &AppHandle) {
    let manager = app.state::<crate::AppState>().thumbnail_manager.clone();
    let pool = match sb_pool() {
        Ok(pool) => pool,
        Err(e) => {
            log::warn!("first thumbnails: no pool for the watcher: {e}");
            return;
        }
    };
    let mut tried = HashSet::new();
    loop {
        // One at a time from a spinning disk, as the workers read it.
        let at_once = if manager.rotational_disk.load(Ordering::Relaxed) {
            1
        } else {
            pool.current_num_threads()
        };
        let batch = next_batch(&manager, &SENT, &mut tried, at_once);
        let settings = crate::app_settings::load_settings(app.clone()).unwrap_or_default();
        let long_edge_px = settings.small_thumbnail_resolution.unwrap_or(480);
        let cache_dir = crate::file_management::get_thumb_cache_dir(app).ok();
        pool.install(|| {
            send_batch(
                &manager.queue,
                &SENT,
                &batch,
                |path| first_of(path, cache_dir.as_deref(), long_edge_px),
                |path, data_url| send(app, path, data_url),
            )
        });
    }
}

/// The newest `at_most` photos waiting in the queue that the watcher has not
/// tried, waiting for some if there are none. A photo that has left the queue
/// is forgotten, by `tried` and `sent`, so it is tried again if queued again.
fn next_batch(
    manager: &ThumbnailManager,
    sent: &Mutex<BTreeSet<String>>,
    tried: &mut HashSet<String>,
    at_most: usize,
) -> Vec<String> {
    loop {
        {
            // `sent` before the queue, as in send_batch.
            let mut sent = lock(sent);
            let queue = lock(&manager.queue);
            let waiting: HashSet<&str> = queue.iter().map(String::as_str).collect();
            tried.retain(|p| waiting.contains(p.as_str()));
            sent.retain(|p| waiting.contains(p.as_str()));
            // Their workers take from the back: the newest request.
            let batch: Vec<String> = queue
                .iter()
                .rev()
                .filter(|p| !tried.contains(*p))
                .take(at_most.max(1))
                .cloned()
                .collect();
            if !batch.is_empty() {
                tried.extend(batch.iter().cloned());
                return batch;
            }
        }
        // Not holding `sent` while waiting: a worker needs it in `emit`.
        let queue = lock(&manager.queue);
        if queue.iter().any(|p| !tried.contains(p)) {
            continue;
        }
        drop(
            manager
                .cvar
                .wait(queue)
                .unwrap_or_else(PoisonError::into_inner),
        );
        // Their workers wait on this condvar too. Should one wake-up have been
        // meant for one of them alone, pass it on.
        manager.cvar.notify_all();
    }
}

/// Make the first thumbnail of each photo of `batch` in parallel, on the
/// current pool, and send it if the photo is still waiting in the queue. One a
/// worker has taken meanwhile is left to the worker, which sends its own in
/// `emit` before making the thumbnail.
fn send_batch(
    queue: &Mutex<VecDeque<String>>,
    sent: &Mutex<BTreeSet<String>>,
    batch: &[String],
    first_of: impl Fn(&str) -> Option<String> + Sync,
    send: impl Fn(&str, &str) + Sync,
) {
    batch.par_iter().for_each(|path| {
        let Some(data_url) = first_of(path) else {
            return;
        };
        let mut sent = lock(sent);
        let waiting = lock(queue).iter().any(|p| p == path);
        if waiting {
            send(path, &data_url);
            sent.insert(path.clone());
        }
    });
}

/// A waiting photo's first thumbnail, as a data URL. None for a photo whose
/// thumbnail is cached, which the worker sends with its edits, and for a file
/// still in the cloud, which reading would download.
fn first_of(path: &str, cache_dir: Option<&Path>, long_edge_px: u32) -> Option<String> {
    let (source, _) = crate::file_management::parse_virtual_path(path);
    if !crate::formats::is_raw_file(&source)
        || crate::file_management::is_cloud_placeholder(&source)
    {
        return None;
    }
    // As generate_single_thumbnail_and_cache names its cache files.
    if let Some(dir) = cache_dir
        && let Some(hash) = crate::file_management::get_cache_key_hash(path)
        && dir.join(format!("{hash}_small.jpg")).exists()
        && dir.join(format!("{hash}_medium.jpg")).exists()
    {
        return None;
    }
    make(&source, long_edge_px).map(|jpeg| data_url(&jpeg))
}

/// The camera's JPEG in a RAW, `long_edge_px` on its long edge, upright, as
/// JPEG bytes. None for anything else, or a file that cannot be read.
pub fn make(source: &Path, long_edge_px: u32) -> Option<Vec<u8>> {
    if !crate::formats::is_raw_file(source) {
        return None;
    }
    // The readers can panic on a broken file; one must not take a worker with it.
    std::panic::catch_unwind(|| {
        let found = sb_embedded::preview(source, long_edge_px)?;
        let small = found.image.thumbnail(long_edge_px, long_edge_px);
        let upright = sb_orient(small, sb_raw_orientation(source));
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, QUALITY)
            .encode_image(&upright.to_rgb8())
            .ok()?;
        Some(out)
    })
    .ok()
    .flatten()
}

// sb_: Shoebox src-tauri/src/imaging.rs rawler_meta @ 4ae3a3a, its
// orientation only. The file is mapped by path, not copied.
fn sb_raw_orientation(path: &Path) -> u16 {
    let Ok(src) = RawSource::new(path) else {
        return 0;
    };
    let Ok(decoder) = rawler::get_decoder(&src) else {
        return 0;
    };
    decoder
        .raw_metadata(&src, &RawDecodeParams::default())
        .ok()
        .and_then(|md| md.exif.orientation)
        .unwrap_or(0)
}

// sb_: Shoebox src-tauri/src/imaging.rs orient @ 4ae3a3a.
/// EXIF orientation 1-8 to upright pixels.
fn sb_orient(img: DynamicImage, o: u16) -> DynamicImage {
    match o {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    fn jpeg(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8])
        });
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80)
            .encode_image(&img)
            .unwrap();
        out
    }

    /// A little-endian TIFF whose one IFD holds a JPEG, as a NEF's does.
    fn tiff_with(j: &[u8]) -> Vec<u8> {
        let mut out = b"II\x2A\x00".to_vec();
        out.extend(8u32.to_le_bytes());
        out.extend(2u16.to_le_bytes());
        for (tag, val) in [(513u16, 8 + 2 + 24 + 4), (514u16, j.len() as u32)] {
            out.extend(tag.to_le_bytes());
            out.extend(4u16.to_le_bytes());
            out.extend(1u32.to_le_bytes());
            out.extend(val.to_le_bytes());
        }
        out.extend(0u32.to_le_bytes());
        out.extend(j);
        out
    }

    fn size_of(jpeg: &[u8]) -> (u32, u32) {
        let img = image::load_from_memory(jpeg).unwrap();
        (img.width(), img.height())
    }

    /// A `w` x `h` JPEG that compresses as a photo does: smooth gradients and
    /// a little grain, not the noise `jpeg` makes.
    fn photo_jpeg(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, y| {
            let grain = ((x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)) >> 28) as u8;
            image::Rgb([
                (x * 200 / w) as u8 + grain,
                (y * 200 / h) as u8 + grain,
                ((x + y) * 100 / (w + h)) as u8 + 80 + grain,
            ])
        });
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
            .encode_image(&img)
            .unwrap();
        out
    }

    /// `count` NEF-like files in `dir`, each holding a `w` x `h` camera JPEG.
    fn nefs(dir: &Path, count: usize, w: u32, h: u32) -> Vec<String> {
        let bytes = tiff_with(&photo_jpeg(w, h));
        (0..count)
            .map(|i| {
                let path = dir.join(format!("{i:03}.NEF"));
                std::fs::write(&path, &bytes).unwrap();
                path.to_string_lossy().into_owned()
            })
            .collect()
    }

    /// A browsing queue holding `paths` as update_thumbnail_queue leaves it,
    /// the last one newest, with no worker taking from it.
    fn queued(paths: &[String]) -> std::sync::Arc<ThumbnailManager> {
        let manager = ThumbnailManager::new();
        lock(&manager.queue).extend(paths.iter().cloned());
        manager
    }

    /// The watcher's loop as `watch` runs it, on a thread of its own, saying
    /// which photo each first thumbnail it sends is for.
    fn watching(manager: std::sync::Arc<ThumbnailManager>) -> std::sync::mpsc::Receiver<String> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let pool = sb_pool().unwrap();
            let sent = Mutex::new(BTreeSet::new());
            let mut tried = HashSet::new();
            loop {
                let batch = next_batch(&manager, &sent, &mut tried, pool.current_num_threads());
                pool.install(|| {
                    send_batch(
                        &manager.queue,
                        &sent,
                        &batch,
                        |path| first_of(path, None, 480),
                        |path, _| {
                            let _ = tx.send(path.to_string());
                        },
                    )
                });
            }
        });
        rx
    }

    /// Every photo the grid asks for has its first thumbnail while all of them
    /// still wait in the queue: none waits for a worker. AG_RAWS names a folder
    /// of real RAWs to time, its first 30 by name; otherwise 30 made here, each
    /// with a 24 MP camera JPEG as a CR3 or NEF holds.
    #[test]
    fn every_waiting_photo_gets_its_first_thumbnail_without_the_workers() {
        let dir = tempfile::tempdir().unwrap();
        let photos: Vec<String> = match std::env::var_os("AG_RAWS").filter(|d| !d.is_empty()) {
            Some(raws) => screenful(Path::new(&raws))
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
            None => nefs(dir.path(), 30, 6000, 4000),
        };
        assert!(!photos.is_empty(), "no RAWs in AG_RAWS");
        let manager = queued(&photos);
        let started = Instant::now();
        let got = watching(manager.clone());
        let mut seen = HashSet::new();
        let mut first = None;
        while seen.len() < photos.len() {
            let Ok(path) = got.recv_timeout(Duration::from_secs(30)) else {
                let missing: Vec<_> = photos.iter().filter(|p| !seen.contains(*p)).collect();
                panic!("no first thumbnail for {missing:?}");
            };
            first.get_or_insert(started.elapsed());
            assert!(seen.insert(path), "a photo's first thumbnail sent twice");
        }
        let all = started.elapsed();
        assert_eq!(
            lock(&manager.queue).len(),
            photos.len(),
            "a photo left the queue"
        );
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        eprintln!(
            "first thumbnails of {} photos, all still queued: first {:.0} ms, all {:.0} ms",
            photos.len(),
            ms(first.unwrap_or_default()),
            ms(all)
        );
    }

    #[test]
    fn a_photo_a_worker_took_is_left_to_it_and_one_sent_is_sent_once() {
        let dir = tempfile::tempdir().unwrap();
        let photos = nefs(dir.path(), 2, 1200, 800);
        let manager = queued(&photos);
        let sent = Mutex::new(BTreeSet::new());
        let mut tried = HashSet::new();
        let batch = next_batch(&manager, &sent, &mut tried, 2);
        assert_eq!(
            batch,
            [photos[1].clone(), photos[0].clone()],
            "newest first"
        );
        // A worker takes the newest before its first thumbnail is ready.
        let taken = lock(&manager.queue).pop_back().unwrap();
        let out = Mutex::new(Vec::new());
        sb_pool().unwrap().install(|| {
            send_batch(
                &manager.queue,
                &sent,
                &batch,
                |path| first_of(path, None, 480),
                |path, _| lock(&out).push(path.to_string()),
            )
        });
        assert_eq!(
            *lock(&out),
            [photos[0].clone()],
            "only the one still waiting"
        );
        assert!(!lock(&sent).contains(&taken), "the worker sends its own");
        assert!(
            lock(&sent).remove(&photos[0]),
            "the worker that takes it finds it sent, and sends nothing more"
        );
    }

    #[test]
    fn newest_first_and_a_photo_queued_again_is_tried_again() {
        let photos: Vec<String> = (0..5).map(|i| format!("p{i}.NEF")).collect();
        let manager = queued(&photos);
        let sent = Mutex::new(BTreeSet::new());
        let mut tried = HashSet::new();
        assert_eq!(
            next_batch(&manager, &sent, &mut tried, 2),
            ["p4.NEF", "p3.NEF"]
        );
        assert_eq!(
            next_batch(&manager, &sent, &mut tried, 2),
            ["p2.NEF", "p1.NEF"]
        );
        lock(&sent).insert("p4.NEF".to_string());
        // Another folder: its photos replace these in the queue. Then back.
        {
            let mut queue = lock(&manager.queue);
            queue.clear();
            queue.push_back("q0.NEF".to_string());
        }
        assert_eq!(next_batch(&manager, &sent, &mut tried, 2), ["q0.NEF"]);
        assert!(
            lock(&sent).is_empty(),
            "a photo that left the queue is forgotten"
        );
        lock(&manager.queue).push_back("p4.NEF".to_string());
        assert_eq!(next_batch(&manager, &sent, &mut tried, 2), ["p4.NEF"]);
    }

    #[test]
    fn a_photo_with_a_cached_thumbnail_gets_no_first_thumbnail() {
        let dir = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let photo = nefs(dir.path(), 1, 1200, 800).remove(0);
        assert!(first_of(&photo, Some(cache.path()), 480).is_some());
        let hash = crate::file_management::get_cache_key_hash(&photo).unwrap();
        for size in ["small", "medium"] {
            std::fs::write(cache.path().join(format!("{hash}_{size}.jpg")), b"").unwrap();
        }
        assert!(
            first_of(&photo, Some(cache.path()), 480).is_none(),
            "the cached one is sent by the worker, with the photo's edits"
        );
    }

    #[test]
    fn a_raw_gets_its_cameras_jpeg_at_the_size_asked_and_other_files_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let nef = dir.path().join("a.NEF");
        std::fs::write(&nef, tiff_with(&jpeg(1200, 800))).unwrap();
        assert_eq!(size_of(&make(&nef, 480).unwrap()), (480, 320));
        let jpg = dir.path().join("b.jpg");
        std::fs::write(&jpg, jpeg(1200, 800)).unwrap();
        assert!(make(&jpg, 480).is_none(), "a JPEG file is quick already");
        for (name, bytes) in [
            ("c.CR3", &b"not a raw"[..]),
            ("d.NEF", b"II\x2A\x00\xFF\xFF\xFF\xFF"),
            ("e.NEF", b""),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            assert!(make(&path, 480).is_none(), "{name}");
        }
    }

    /// Today's work for one unedited RAW's thumbnail, as generate_thumbnail_data
    /// and generate_single_thumbnail_and_cache do it: their embedded preview
    /// when it is at least 95% of the medium size, else a develop of the RAW;
    /// then the small and medium JPEGs.
    fn today(path: &Path) -> Vec<u8> {
        let settings = crate::app_settings::AppSettings::default();
        let medium = settings.medium_thumbnail_resolution.unwrap_or(1280);
        let small = settings.small_thumbnail_resolution.unwrap_or(480);
        let path_str = path.to_string_lossy();
        let mmap = crate::file_management::read_file_mapped(path).unwrap();
        let embedded = match crate::exif_processing::read_exif(&mmap) {
            Some(exif) => [exif::In::PRIMARY, exif::In::THUMBNAIL]
                .iter()
                .find_map(|&ifd| {
                    let at = exif
                        .get_field(exif::Tag::JPEGInterchangeFormat, ifd)?
                        .value
                        .get_uint(0)? as usize;
                    let len = exif
                        .get_field(exif::Tag::JPEGInterchangeFormatLength, ifd)?
                        .value
                        .get_uint(0)? as usize;
                    image::load_from_memory(exif.buf().get(at..at + len)?).ok()
                }),
            None => crate::image_loader::safe_embedded_preview_fallback(&mmap, &path_str),
        };
        let picture = embedded
            .filter(|p| p.width().max(p.height()) >= (medium as f32 * 0.95) as u32)
            .unwrap_or_else(|| {
                let mut img = crate::image_loader::load_and_composite(
                    &mmap,
                    &path_str,
                    &serde_json::Value::Null,
                    true,
                    &settings,
                    None,
                )
                .unwrap();
                crate::image_processing::apply_cpu_default_raw_processing(&mut img);
                img
            });
        let mut last = Vec::new();
        for edge in [small, medium] {
            let thumb = crate::image_processing::downscale_f32_image(&picture, edge, edge);
            last.clear();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut last, 75)
                .encode_image(&thumb.to_rgb8())
                .unwrap();
        }
        last
    }

    /// `work` on the first `count` photos by `workers` threads taking them in
    /// turn, as the thumbnail queue does: when the first was done, and the last.
    fn timed(
        photos: &[PathBuf],
        workers: usize,
        work: fn(&Path) -> Option<Vec<u8>>,
    ) -> (Duration, Duration) {
        let next = Mutex::new(0usize);
        let first = Mutex::new(None);
        let started = Instant::now();
        std::thread::scope(|s| {
            for _ in 0..workers {
                s.spawn(|| {
                    loop {
                        let i = {
                            let mut n = next.lock().unwrap();
                            *n += 1;
                            *n - 1
                        };
                        let Some(p) = photos.get(i) else { break };
                        assert!(work(p).is_some(), "{}", p.display());
                        first.lock().unwrap().get_or_insert(started.elapsed());
                    }
                });
            }
        });
        (
            first.into_inner().unwrap().unwrap_or_default(),
            started.elapsed(),
        )
    }

    /// The first 30 RAWs of a folder, by name.
    fn screenful(dir: &Path) -> Vec<PathBuf> {
        let mut raws: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| crate::formats::is_raw_file(p))
            .collect();
        raws.sort();
        raws.truncate(30);
        raws
    }

    /// Before and after on real folders. AG_THUMBS_COLD names a folder never
    /// read, timed first, with the first thumbnails only. AG_THUMBS_DIRS
    /// (separated by `;`) are each read once, so both ways run from the
    /// system's cache and time only their own work: today's, then the first
    /// thumbnails. With the app's default number of thumbnail workers.
    #[test]
    fn first_thumbnails_before_and_after() {
        let workers = crate::app_settings::AppSettings::default()
            .thumbnail_worker_threads
            .unwrap_or(4) as usize;
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let report = |what: &str, (first, all): (Duration, Duration)| {
            eprintln!(
                "{what}, {workers} workers: first {:.0} ms, 30 {:.0} ms",
                ms(first),
                ms(all)
            );
        };
        if let Some(cold) = std::env::var_os("AG_THUMBS_COLD") {
            report(
                "first thumbnails, never read",
                timed(&screenful(Path::new(&cold)), workers, |p| make(p, 480)),
            );
        }
        let Some(dirs) = std::env::var("AG_THUMBS_DIRS")
            .ok()
            .filter(|d| !d.is_empty())
        else {
            eprintln!("first_thumbnails_before_and_after: SKIPPED - set AG_THUMBS_DIRS");
            return;
        };
        for dir in dirs.split(';').filter(|d| !d.is_empty()) {
            let photos = screenful(Path::new(dir));
            for p in &photos {
                std::fs::read(p).unwrap();
            }
            eprintln!("{dir}:");
            report("  today", timed(&photos, workers, |p| Some(today(p))));
            report(
                "  first thumbnails",
                timed(&photos, workers, |p| make(p, 480)),
            );
        }
    }
}
