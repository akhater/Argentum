//! One thumbnail, made as Shoebox makes its previews (`imaging.rs` @ 4ae3a3a):
//! the camera's own JPEG, read on its own and decoded at the size needed
//! (`sb_embedded.rs`), then rawler's preview for containers that reader does
//! not know, turned as the RAW's metadata says. A RAW is never developed for a
//! thumbnail. A JPEG, TIFF or PNG is decoded by the image crate and turned as
//! its EXIF says. Made `px` on its long edge, encoded as a JPEG.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use image::DynamicImage;
use rawler::decoders::RawDecodeParams;
use rawler::rawsource::RawSource;

use super::contract::{BrowseError, BrowseErrorKind};
use super::{listing, sb_embedded};

/// A thumbnail's JPEG quality.
const QUALITY: u8 = 80;

/// The thumbnail of `path`, `px` on its long edge, as JPEG bytes.
pub fn make(path: &Path, px: u32) -> Result<Vec<u8>, BrowseError> {
    // rawler, the TIFF walk and the image decoders can panic on a broken
    // file; one bad file must not take a worker with it.
    let picture = std::panic::catch_unwind(|| {
        if listing::is_raw(path) {
            sb_raw_picture(path, px)
        } else {
            sb_image_picture(path, px)
        }
    })
    .map_err(|_| {
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

fn no_preview(path: &Path) -> BrowseError {
    BrowseError::new(
        BrowseErrorKind::NoPreview,
        format!("{}: no picture inside it to show", path.display()),
    )
}

// sb_: Shoebox imaging.rs load_raw @ 4ae3a3a, its first two steps: the
// embedded JPEG, then rawler's preview, one smaller than 3/4 of the size
// wanted only as a last resort. Its Windows codec and its full develop are
// left out: a thumbnail never develops a RAW.
fn sb_raw_picture(path: &Path, px: u32) -> Result<DynamicImage, BrowseError> {
    let good_enough = (px.min(1600) as f32 * 0.75) as u32;
    let mut image = None;
    let mut embedded = None;
    if let Some(found) = sb_embedded::preview(path, px) {
        if sb_long_of(&found.image) >= good_enough {
            image = Some(found.image);
        } else {
            embedded = Some(found.image);
        }
    }
    if image.is_none()
        && let Some(img) = sb_rawler_image(path)
    {
        if sb_long_of(&img) >= good_enough {
            image = Some(img);
        } else if embedded
            .as_ref()
            .is_none_or(|e| sb_long_of(e) < sb_long_of(&img))
        {
            embedded = Some(img);
        }
    }
    let picture = image.or(embedded).ok_or_else(|| no_preview(path))?;
    Ok(sb_orient(
        picture.thumbnail(px, px),
        sb_raw_orientation(path),
    ))
}

// sb_: Shoebox imaging.rs rawler_image @ 4ae3a3a.
/// The largest embedded image rawler can hand over (decoded at full size, so slow).
fn sb_rawler_image(path: &Path) -> Option<DynamicImage> {
    let src = RawSource::new(path).ok()?;
    let decoder = rawler::get_decoder(&src).ok()?;
    let params = RawDecodeParams::default();
    decoder
        .full_image(&src, &params)
        .ok()
        .flatten()
        .or_else(|| decoder.preview_image(&src, &params).ok().flatten())
}

// sb_: Shoebox imaging.rs rawler_meta @ 4ae3a3a, its orientation only. The
// file is mapped by path, not copied (RawSource::new_from_slice copies it).
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

// sb_: Shoebox imaging.rs load_image @ 4ae3a3a without its Windows codec: the
// image crate decodes the file, turned as its EXIF says.
fn sb_image_picture(path: &Path, px: u32) -> Result<DynamicImage, BrowseError> {
    let img = image::open(path).map_err(|e| match e {
        image::ImageError::IoError(io) => BrowseError::io(path, &io),
        other => BrowseError::new(
            BrowseErrorKind::NoPreview,
            format!("{}: {other}", path.display()),
        ),
    })?;
    Ok(sb_orient(img.thumbnail(px, px), sb_exif_orientation(path)))
}

// sb_: Shoebox imaging.rs exif_meta and meta_of @ 4ae3a3a, the orientation only.
fn sb_exif_orientation(path: &Path) -> u16 {
    let Ok(file) = File::open(path) else {
        return 0;
    };
    exif::Reader::new()
        .read_from_container(&mut BufReader::new(file))
        .ok()
        .and_then(|ex| {
            ex.get_field(exif::Tag::Orientation, exif::In::PRIMARY)?
                .value
                .get_uint(0)
        })
        .unwrap_or(0) as u16
}

// sb_: Shoebox imaging.rs long_of @ 4ae3a3a.
fn sb_long_of(img: &DynamicImage) -> u32 {
    img.width().max(img.height())
}

// sb_: Shoebox imaging.rs orient @ 4ae3a3a.
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
pub(super) mod tests {
    use super::*;

    /// A real JPEG of `w` x `h`, made by the image crate.
    pub fn jpeg(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8])
        });
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80)
            .encode_image(&img)
            .unwrap();
        out
    }

    /// A little-endian TIFF with one IFD per JPEG, each as JPEGInterchangeFormat.
    pub fn tiff_with(jpegs: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"II\x2A\x00".to_vec();
        out.extend(8u32.to_le_bytes());
        let ifd_len = 2 + 2 * 12 + 4;
        let mut data_at = 8 + ifd_len * jpegs.len();
        for (i, j) in jpegs.iter().enumerate() {
            out.extend(2u16.to_le_bytes());
            for (tag, val) in [(513u16, data_at as u32), (514u16, j.len() as u32)] {
                out.extend(tag.to_le_bytes());
                out.extend(4u16.to_le_bytes());
                out.extend(1u32.to_le_bytes());
                out.extend(val.to_le_bytes());
            }
            let next = if i + 1 < jpegs.len() {
                (8 + ifd_len * (i + 1)) as u32
            } else {
                0
            };
            out.extend(next.to_le_bytes());
            data_at += j.len();
        }
        for j in jpegs {
            out.extend(j);
        }
        out
    }

    /// A CR3's preview box: `ftyp`, then a `uuid` box holding `PRVW` and its JPEG.
    fn cr3(preview: &[u8]) -> Vec<u8> {
        let boxed = |kind: &[u8], body: &[u8]| {
            let mut b = ((8 + body.len()) as u32).to_be_bytes().to_vec();
            b.extend(kind);
            b.extend(body);
            b
        };
        let mut prvw = vec![0, 0, 0, 0, 0, 1];
        prvw.extend(1620u16.to_be_bytes());
        prvw.extend(1080u16.to_be_bytes());
        prvw.extend([0, 1]);
        prvw.extend((preview.len() as u32).to_be_bytes());
        prvw.extend(preview);
        let mut uuid = vec![
            0xEA, 0xF4, 0x2B, 0x5E, 0x1C, 0x98, 0x4B, 0x88, 0xB9, 0xFB, 0xB7, 0xDC, 0x40, 0x6E,
            0x4D, 0x16, 0, 0, 0, 0, 0, 0, 0, 1,
        ];
        uuid.extend(boxed(b"PRVW", &prvw));
        let mut file = boxed(b"ftyp", b"crx \x00\x00\x00\x01crx isom");
        file.extend(boxed(b"uuid", &uuid));
        file
    }

    fn size_of(jpeg: &[u8]) -> (u32, u32) {
        let img = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg).unwrap();
        (img.width(), img.height())
    }

    fn thumb_of(dir: &Path, name: &str, bytes: &[u8]) -> Result<(u32, u32), BrowseError> {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        make(&path, 400).map(|t| size_of(&t))
    }

    #[test]
    fn a_raw_thumbnail_comes_from_its_embedded_jpeg() {
        let dir = tempfile::tempdir().unwrap();
        let nef = tiff_with(&[jpeg(160, 120), jpeg(1200, 800)]);
        assert_eq!(thumb_of(dir.path(), "a.NEF", &nef).unwrap(), (400, 267));
        let cr3 = cr3(&jpeg(1620, 1080));
        assert_eq!(thumb_of(dir.path(), "b.CR3", &cr3).unwrap(), (400, 267));
        let mut raf = b"FUJIFILMCCD-RAW 0201FF383501".to_vec();
        let preview = jpeg(640, 480);
        raf.resize(100, 0);
        raf[84..88].copy_from_slice(&100u32.to_be_bytes());
        raf[88..92].copy_from_slice(&(preview.len() as u32).to_be_bytes());
        raf.extend(&preview);
        assert_eq!(thumb_of(dir.path(), "c.RAF", &raf).unwrap(), (400, 300));
    }

    #[test]
    fn a_jpeg_thumbnail_is_the_jpeg_made_small() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            thumb_of(dir.path(), "a.jpg", &jpeg(600, 900)).unwrap(),
            (267, 400)
        );
    }

    #[test]
    fn a_file_with_nothing_to_show_says_so_and_does_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in [
            ("a.CR3", &b"not a raw"[..]),
            ("b.jpg", b"not a jpeg"),
            ("c.NEF", b""),
            ("d.NEF", b"II\x2A\x00\xFF\xFF\xFF\xFF"),
            ("e.NEF", b"MM\x00\x2A\x00\x00\x00\x08\x00\x09"),
            ("f.CR3", b"\x00\x00\x00\x18ftypcrx \x00\x00\x00\x01"),
        ] {
            let e = thumb_of(dir.path(), name, bytes).unwrap_err();
            assert_eq!(e.kind, BrowseErrorKind::NoPreview, "{name}");
        }
    }

    /// Real files, when AG_RAWS names some: each gets a thumbnail of the
    /// right size. With AG_THUMBS_OUT, each is written there to look at.
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
}
