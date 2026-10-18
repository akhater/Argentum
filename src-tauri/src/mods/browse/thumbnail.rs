//! One thumbnail: the picture inside a RAW, or a JPEG, PNG or TIFF itself,
//! made `px` on its long edge, the right way up, as a JPEG.
//!
//! A RAW is never developed here. Its embedded JPEGs come first
//! (`embedded.rs`), then whatever preview rawler can hand over; a RAW with
//! neither has no thumbnail. A JPEG is decoded at the smallest of 1/2, 1/4
//! or 1/8 its size that still covers the thumbnail: a camera's full-size
//! preview costs an eighth of the work and a sixty-fourth of the memory.

use std::fs::File;
use std::io::Cursor;
use std::path::Path;

use image::metadata::Orientation;
use image::{DynamicImage, GrayImage, ImageDecoder, ImageReader, RgbImage};
use rawler::decoders::RawDecodeParams;
use rawler::rawsource::RawSource;

use super::contract::{BrowseError, BrowseErrorKind};
use super::{embedded, listing};

/// A thumbnail's JPEG quality.
const QUALITY: u8 = 80;

/// The thumbnail of `path`, `px` on its long edge, as JPEG bytes.
pub fn make(path: &Path, px: u32) -> Result<Vec<u8>, BrowseError> {
    // rawler and the image decoders can panic on a broken file; one bad file
    // must not take a worker with it.
    let picture = std::panic::catch_unwind(|| picture(path, px)).map_err(|_| {
        BrowseError::new(
            BrowseErrorKind::NoPreview,
            format!("{}: the file could not be read", path.display()),
        )
    })??;
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, QUALITY)
        .encode_image(&picture.to_rgb8())
        .map_err(|e| BrowseError::new(BrowseErrorKind::Internal, e.to_string()))?;
    Ok(out)
}

fn picture(path: &Path, px: u32) -> Result<DynamicImage, BrowseError> {
    if listing::is_raw(path) {
        embedded_picture(path, px)
    } else if is_jpeg(path) {
        jpeg_picture(path, px)
    } else {
        image_picture(path, px)
    }
}

fn is_jpeg(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"))
}

fn no_preview(path: &Path) -> BrowseError {
    BrowseError::new(
        BrowseErrorKind::NoPreview,
        format!("{}: no picture inside it to show", path.display()),
    )
}

/// The file's bytes, mapped rather than read: only the pages a preview is in
/// are read from disk, not the whole RAW.
fn map(path: &Path) -> Result<memmap2::Mmap, BrowseError> {
    let file = File::open(path).map_err(|e| BrowseError::io(path, &e))?;
    let len = file
        .metadata()
        .map_err(|e| BrowseError::io(path, &e))?
        .len();
    if len == 0 {
        return Err(no_preview(path));
    }
    // As RapidRAW's read_file_mapped (file_management.rs): a file someone
    // holds locked for writing is not mapped.
    if file.try_lock_shared().is_err() {
        return Err(BrowseError::new(
            BrowseErrorKind::Io,
            format!("{}: in use by another program", path.display()),
        ));
    }
    // SAFETY: mapped read-only and dropped before this call returns its
    // thumbnail. A file truncated by another program meanwhile is the one
    // risk, the same one the app's RAW loading takes.
    unsafe { memmap2::Mmap::map(&file) }.map_err(|e| BrowseError::io(path, &e))
}

fn embedded_picture(path: &Path, px: u32) -> Result<DynamicImage, BrowseError> {
    let file = map(path)?;
    let (picture, jpeg) = embedded::by_preference(embedded::jpegs(&file), px)
        .into_iter()
        .find_map(|e| Some((ag_decode_scaled(e.jpeg, px)?, Some(e.jpeg))))
        .or_else(|| Some((rr_full_image(&file)?, None)))
        .ok_or_else(|| no_preview(path))?;
    let mut small = picture.thumbnail(px, px);
    // The RAW says which way up; failing that, the preview's own EXIF.
    let turn = embedded::orientation(&file).or_else(|| jpeg.and_then(exif_orientation));
    small.apply_orientation(as_turn(turn));
    Ok(small)
}

// rr_: RapidRAW src-tauri/src/raw_processing.rs extract_embedded_preview,
// v1.6.5 (79c2a46b), without the turn: every picture here is turned once,
// above. Only for RAWs none of embedded.rs's layouts finds a JPEG in, as
// rawler copies the whole file into memory first.
fn rr_full_image(file: &[u8]) -> Option<DynamicImage> {
    let source = RawSource::new_from_slice(file);
    rawler::get_decoder(&source)
        .ok()?
        .full_image(&source, &RawDecodeParams::default())
        .ok()?
}

/// The Orientation a JPEG's EXIF records, read from its headers only.
fn exif_orientation(jpeg: &[u8]) -> Option<u16> {
    let exif = exif::Reader::new()
        .read_from_container(&mut Cursor::new(jpeg))
        .ok()?;
    let turn = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)?;
    u16::try_from(turn.value.get_uint(0)?).ok()
}

fn as_turn(exif: Option<u16>) -> Orientation {
    exif.and_then(|o| u8::try_from(o).ok())
        .and_then(Orientation::from_exif)
        .unwrap_or(Orientation::NoTransforms)
}

/// A JPEG decoded at the smallest scale whose long edge is still at least
/// `px`: 1, 1/2, 1/4 or 1/8. None if it cannot be.
fn ag_decode_scaled(jpeg: &[u8], px: u32) -> Option<DynamicImage> {
    let mut decoder = jpeg_decoder::Decoder::new(jpeg);
    decoder.read_info().ok()?;
    let info = decoder.info()?;
    let (w, h) = (info.width as u32, info.height as u32);
    let shrink = (px as f64 / w.max(h).max(1) as f64).min(1.0);
    let wanted = |side: u32| ((side as f64 * shrink).ceil() as u16).max(1);
    let (w, h) = decoder.scale(wanted(w), wanted(h)).ok()?;
    let pixels = decoder.decode().ok()?;
    let (w, h) = (w as u32, h as u32);
    match decoder.info()?.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => {
            RgbImage::from_raw(w, h, pixels).map(DynamicImage::ImageRgb8)
        }
        jpeg_decoder::PixelFormat::L8 => {
            GrayImage::from_raw(w, h, pixels).map(DynamicImage::ImageLuma8)
        }
        _ => None,
    }
}

/// A JPEG file: decoded scaled, turned as its EXIF says.
fn jpeg_picture(path: &Path, px: u32) -> Result<DynamicImage, BrowseError> {
    let file = map(path)?;
    let mut small = ag_decode_scaled(&file, px)
        .ok_or_else(|| no_preview(path))?
        .thumbnail(px, px);
    small.apply_orientation(as_turn(exif_orientation(&file)));
    Ok(small)
}

/// A PNG or TIFF: the file is the picture.
fn image_picture(path: &Path, px: u32) -> Result<DynamicImage, BrowseError> {
    let unreadable = |e: image::ImageError| {
        BrowseError::new(
            BrowseErrorKind::NoPreview,
            format!("{}: {e}", path.display()),
        )
    };
    let mut decoder = ImageReader::open(path)
        .map_err(|e| BrowseError::io(path, &e))?
        .with_guessed_format()
        .map_err(|e| BrowseError::io(path, &e))?
        .into_decoder()
        .map_err(unreadable)?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut small = DynamicImage::from_decoder(decoder)
        .map_err(unreadable)?
        .thumbnail(px, px);
    small.apply_orientation(orientation);
    Ok(small)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::browse::embedded::tests::{cr3, jpeg, tiff, tiff_with};

    fn size_of(jpeg: &[u8]) -> (u32, u32) {
        let img = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg).unwrap();
        (img.width(), img.height())
    }

    #[test]
    fn a_raw_thumbnail_comes_from_its_embedded_jpeg() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("a.NEF");
        std::fs::write(&raw, tiff_with(&[jpeg(160, 120), jpeg(1200, 800)])).unwrap();
        assert_eq!(size_of(&make(&raw, 400).unwrap()), (400, 267));
    }

    #[test]
    fn a_raw_held_upright_gives_an_upright_thumbnail() {
        let dir = tempfile::tempdir().unwrap();
        let nef = dir.path().join("a.NEF");
        std::fs::write(&nef, tiff(Some(6), &[jpeg(1200, 800)])).unwrap();
        assert_eq!(size_of(&make(&nef, 400).unwrap()), (267, 400));
        let cr3_path = dir.path().join("b.CR3");
        std::fs::write(&cr3_path, cr3(&jpeg(1620, 1080), 8)).unwrap();
        assert_eq!(size_of(&make(&cr3_path, 400).unwrap()), (267, 400));
    }

    #[test]
    fn a_big_jpeg_is_decoded_at_the_smallest_scale_that_covers_the_thumbnail() {
        let big = jpeg(1600, 1200);
        let at = |px| {
            let img = ag_decode_scaled(&big, px).unwrap();
            (img.width(), img.height())
        };
        assert_eq!(at(400), (400, 300));
        assert_eq!(at(401), (800, 600));
        assert_eq!(at(150), (200, 150));
        // Already smaller: whole.
        assert_eq!(ag_decode_scaled(&jpeg(300, 200), 400).unwrap().width(), 300);
        assert!(ag_decode_scaled(b"not a jpeg", 400).is_none());
    }

    #[test]
    fn a_jpeg_thumbnail_is_the_jpeg_made_small() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("a.jpg");
        std::fs::write(&photo, jpeg(600, 900)).unwrap();
        assert_eq!(size_of(&make(&photo, 400).unwrap()), (267, 400));
    }

    #[test]
    fn a_file_with_nothing_to_show_says_so() {
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in [
            ("a.CR3", &b"not a raw"[..]),
            ("b.jpg", b"not a jpeg"),
            ("c.NEF", b""),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(
                make(&path, 400).unwrap_err().kind,
                BrowseErrorKind::NoPreview,
                "{name}"
            );
        }
        let gone = make(&dir.path().join("gone.CR3"), 400).unwrap_err();
        assert_eq!(gone.kind, BrowseErrorKind::NotFound);
    }

    /// Real RAWs, when AG_RAWS names some: each gets a thumbnail of the right
    /// size, from its embedded picture.
    #[test]
    fn real_raws_have_thumbnails() {
        let Some(raws) = std::env::var("AG_RAWS")
            .ok()
            .filter(|s| !s.trim().is_empty())
        else {
            eprintln!("real_raws_have_thumbnails: SKIPPED - set AG_RAWS");
            return;
        };
        for raw in raws.split(';').filter(|s| !s.is_empty()) {
            let started = std::time::Instant::now();
            let thumb = make(Path::new(raw), 400).unwrap_or_else(|e| panic!("{raw}: {e}"));
            let (w, h) = size_of(&thumb);
            eprintln!(
                "{raw}: {w} x {h}, {} KB, {:?}",
                thumb.len() / 1024,
                started.elapsed()
            );
            assert_eq!(w.max(h), 400);
            if let Some(out) = std::env::var_os("AG_THUMBS_OUT") {
                let name = Path::new(raw)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                std::fs::write(Path::new(&out).join(name + ".jpg"), &thumb).unwrap();
            }
        }
    }

    /// Where a thumbnail's time goes, step by step, for AG_RAWS: each step
    /// the best of three, the file already in memory. The preview's decode is
    /// timed whole (the image crate) and scaled (jpeg-decoder), which is why
    /// the second is used.
    #[test]
    fn where_the_time_goes() {
        let Some(raws) = std::env::var("AG_RAWS")
            .ok()
            .filter(|s| !s.trim().is_empty())
        else {
            eprintln!("where_the_time_goes: SKIPPED - set AG_RAWS");
            return;
        };
        let best = |f: &mut dyn FnMut()| {
            (0..3)
                .map(|_| {
                    let t = std::time::Instant::now();
                    f();
                    t.elapsed().as_secs_f64() * 1000.0
                })
                .fold(f64::MAX, f64::min)
        };
        for raw in raws.split(';').filter(|s| listing::is_raw(Path::new(s))) {
            let file = std::fs::read(raw).unwrap();
            let found = embedded::by_preference(embedded::jpegs(&file), 400);
            let jpeg = found[0].jpeg;
            let find_ms = best(&mut || drop(embedded::by_preference(embedded::jpegs(&file), 400)));
            let orientation_ms = best(&mut || {
                std::hint::black_box(embedded::orientation(&file));
            });
            let whole_ms = best(&mut || {
                let p =
                    image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg).unwrap();
                drop(p.thumbnail(400, 400));
            });
            let scaled = ag_decode_scaled(jpeg, 400).unwrap();
            let scaled_ms =
                best(&mut || drop(ag_decode_scaled(jpeg, 400).unwrap().thumbnail(400, 400)));
            let small = scaled.thumbnail(400, 400);
            let encode_ms = best(&mut || {
                let mut out = Vec::new();
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, QUALITY)
                    .encode_image(&small.to_rgb8())
                    .unwrap();
            });
            eprintln!(
                "{}: find {find_ms:.2} + orientation {orientation_ms:.2} ({:?}); preview {}x{}: whole {whole_ms:.1}, scaled to {}x{} {scaled_ms:.1} (decode and resize); encode {encode_ms:.1} ms",
                Path::new(raw).file_name().unwrap().to_string_lossy(),
                embedded::orientation(&file),
                found[0].width,
                found[0].height,
                scaled.width(),
                scaled.height(),
            );
        }
    }
}
