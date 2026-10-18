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
//! `generate_single_thumbnail_and_cache` calls it once a cached thumbnail has
//! not answered, and only for the browsing queue: every other caller forces a
//! new thumbnail after an edit, where the camera's picture would flash the
//! photo back to how it was shot.

use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use image::DynamicImage;
use rawler::decoders::RawDecodeParams;
use rawler::rawsource::RawSource;
use tauri::{AppHandle, Emitter};

use super::sb_embedded;

/// As the thumbnails it stands in for (`encode_thumbnail` in file_management.rs).
const QUALITY: u8 = 75;

/// Send `path`'s first thumbnail, `long_edge_px` on its long edge, if it is a
/// RAW whose camera embedded a JPEG.
pub fn emit(app: &AppHandle, path: &str, source: &Path, long_edge_px: u32) {
    if let Some(jpeg) = make(source, long_edge_px) {
        let data = format!("data:image/jpeg;base64,{}", STANDARD.encode(jpeg));
        let _ = app.emit(
            "thumbnail-generated",
            serde_json::json!({ "path": path, "data": data }),
        );
    }
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
