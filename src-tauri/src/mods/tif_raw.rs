//! Telling a camera raw that calls itself `.TIF` from an ordinary TIFF.
//!
//! THE PROBLEM
//!
//! The original EOS-1D and EOS-1Ds are older than the CR2 container. Canon
//! wrote their raws as TIFF files with a `.TIF` extension — a real Bayer mosaic
//! reached through a MakerNote offset, with two small RGB previews sitting in
//! the IFD chain in front of it.
//!
//! `formats.rs` decides what is a raw by extension, and `tif` is in
//! `NON_RAW_EXTENSIONS`. So these files never reach the raw path. They go to the
//! ordinary image loader, which reads the first image in the chain — on
//! `91BX8040.TIF` from RapidRAW issue #1678 that is a 288x192 thumbnail — and
//! the app opens a 288x192 picture of an 11 megapixel photograph.
//!
//! WHY THE EXTENSION CANNOT DECIDE IT
//!
//! Moving `tif` into `RAW_EXTENSIONS` would send every scan, every export and
//! every Photoshop file down the raw decoder. The extension is genuinely
//! ambiguous, and has been since 2002. Only the contents settle it.
//!
//! WHY RAWLER IS THE TEST
//!
//! Not "is the Make Canon" — an ordinary TIFF exported from a Canon photo
//! inherits that, and can inherit a whole MakerNote with it. The question is
//! whether a raw decoder will actually open the file, so the honest way to ask
//! is to ask the decoder. `get_decoder` only identifies a decoder; for a Canon
//! TIFF it can succeed from Make/Model alone, even when the file has no raw
//! payload. The probe therefore also asks that decoder for a dummy raw image.
//! This validates the raw-specific payload without doing the full pixel decode,
//! and keeps this decision aligned with the code path that opens the image.
//!
//! The test and the consequence are then the same function, and cannot drift
//! apart.
//!
//! WHY IT IS CACHED
//!
//! `is_raw_file` is called from thirty-odd places and is a pure string
//! comparison everywhere else. This adds a file open, so the answer is
//! remembered against the file's size and modification time. A `.tif` is
//! sniffed once; every other extension never touches the disk at all.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

/// The extensions that are ambiguous. Nothing else is sniffed.
const AMBIGUOUS: &[&str] = &["tif", "tiff"];

/// What a file looked like when we last answered for it.
type Stamp = (u64, Option<std::time::SystemTime>);

fn cache() -> &'static Mutex<HashMap<String, (Stamp, bool)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Stamp, bool)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Is this `.tif` actually a camera raw?
///
/// False for every other extension without touching the disk, so this is safe
/// to put in front of a path that does not exist yet — an export target, say.
pub fn is_camera_raw<P: AsRef<Path>>(path: P) -> bool {
    let path = path.as_ref();

    let ambiguous = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| AMBIGUOUS.iter().any(|a| a.eq_ignore_ascii_case(ext)));
    if !ambiguous {
        return false;
    }

    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let stamp: Stamp = (meta.len(), meta.modified().ok());
    let key = path.to_string_lossy().into_owned();

    if let Ok(cache) = cache().lock()
        && let Some((seen, answer)) = cache.get(&key)
        && *seen == stamp
    {
        return *answer;
    }

    let answer = a_raw_decoder_opens_it(path);

    if let Ok(mut cache) = cache().lock() {
        // Not a hot path — one entry per .tif ever opened — but a library of
        // scans should not grow this without bound either.
        if cache.len() > 4096 {
            cache.clear();
        }
        cache.insert(key, (stamp, answer));
    }
    answer
}

/// The whole test: can rawler initialize a raw image from this file?
fn a_raw_decoder_opens_it(path: &Path) -> bool {
    let Ok(source) = rawler::rawsource::RawSource::new(path) else {
        return false;
    };
    match rawler::get_decoder(&source) {
        Ok(decoder) => {
            match decoder.raw_image(&source, &rawler::decoders::RawDecodeParams::default(), true) {
                Ok(_) => true,
                Err(e) => {
                    log::debug!("{} is not a usable raw TIFF: {e:?}", path.display());
                    false
                }
            }
        }
        Err(e) => {
            log::debug!("{} is an ordinary TIFF: {e:?}", path.display());
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    use rawler::decoders::RawDecodeParams;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("argentum-tif-raw-tests");
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir.join(name)
    }

    /// An ordinary TIFF — the thing this must not break — written by the same
    /// image crate the app loads it with.
    fn write_an_ordinary_tiff(name: &str) -> std::path::PathBuf {
        let path = temp(name);
        let buffer = image::RgbImage::from_pixel(64, 48, image::Rgb([200, 120, 40]));
        image::DynamicImage::ImageRgb8(buffer)
            .save_with_format(&path, image::ImageFormat::Tiff)
            .expect("write tiff");
        path
    }

    /// A normal TIFF exported from a Canon photo can retain Canon camera
    /// metadata while containing no sensor RAW payload. rawler's decoder
    /// selection accepts this from Make/Model, but its raw-image probe must
    /// reject it so the normal TIFF loader gets the file.
    fn write_canon_tiff_without_raw(name: &str) -> std::path::PathBuf {
        let path = temp(name);
        let make = b"Canon\0";
        let model = b"Canon EOS-1DS\0";

        // Little-endian TIFF with two ASCII fields: Make and Model. The IFD
        // ends at byte 38, so the strings follow immediately after it.
        let make_offset = 38u32;
        let model_offset = make_offset + make.len() as u32;
        let mut bytes = Vec::with_capacity(model_offset as usize + model.len());
        bytes.extend_from_slice(b"II");
        bytes.extend_from_slice(&42u16.to_le_bytes());
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());

        for (tag, count, offset) in [
            (0x010f_u16, make.len() as u32, make_offset),
            (0x0110_u16, model.len() as u32, model_offset),
        ] {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&2u16.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            bytes.extend_from_slice(&offset.to_le_bytes());
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(make);
        bytes.extend_from_slice(model);
        std::fs::write(&path, bytes).expect("write Canon TIFF");
        path
    }

    /// The case the whole module exists for, stated as a test: a TIFF that no
    /// raw decoder will open is not a raw, however it is spelled.
    #[test]
    fn an_ordinary_tiff_is_not_a_raw() {
        let path = write_an_ordinary_tiff("ordinary.tif");
        assert!(!is_camera_raw(&path));

        let upper = temp("ORDINARY.TIFF");
        std::fs::copy(&path, &upper).expect("copy");
        assert!(
            !is_camera_raw(&upper),
            "case and .tiff must behave the same"
        );
    }

    #[test]
    fn a_canon_tiff_without_raw_payload_is_not_a_raw() {
        let path = write_canon_tiff_without_raw("canon-export.tif");
        let source = rawler::rawsource::RawSource::new(&path).expect("source");
        let decoder =
            rawler::get_decoder(&source).expect("Canon Make/Model should select the CR2 decoder");
        assert!(
            decoder
                .raw_image(&source, &RawDecodeParams::default(), true)
                .is_err(),
            "the fixture must have no raw payload"
        );
        assert!(!is_camera_raw(&path));
    }

    /// Nothing but a `.tif` is ever sniffed, and a path that does not exist is
    /// not an error — `is_raw_file` is asked about export targets.
    #[test]
    fn only_ambiguous_extensions_are_sniffed() {
        assert!(!is_camera_raw("a.jpg"));
        assert!(
            !is_camera_raw("a.cr2"),
            "already raw by extension, not ours"
        );
        assert!(!is_camera_raw("no-extension"));
        assert!(!is_camera_raw(temp("does-not-exist.tif")));
    }

    /// Garbage named `.tif` is declined rather than crashing the caller.
    #[test]
    fn a_file_that_is_not_a_tiff_at_all_is_declined() {
        let path = temp("garbage.tif");
        let mut f = std::fs::File::create(&path).expect("create");
        f.write_all(b"this is not a tiff, or anything else")
            .expect("write");
        drop(f);
        assert!(!is_camera_raw(&path));
    }

    /// The answer is remembered, and forgotten when the file changes.
    #[test]
    fn the_answer_follows_the_file() {
        let path = write_an_ordinary_tiff("cached.tif");
        assert!(!is_camera_raw(&path));
        assert!(
            !is_camera_raw(&path),
            "second call is served from the cache"
        );

        // A different file at the same path must be re-read, not remembered.
        std::fs::write(&path, b"replaced").expect("replace");
        assert!(!is_camera_raw(&path));
    }
}

/// What the sniff says about a real file, for checking by hand.
#[cfg(test)]
mod facts {
    #[test]
    #[ignore = "reads a file named by AG_RAW; run by hand"]
    fn is_this_file_a_raw() {
        let path = std::env::var("AG_RAW").expect("set AG_RAW");
        println!("\n{path}\n  camera raw: {}", super::is_camera_raw(&path));
    }
}
