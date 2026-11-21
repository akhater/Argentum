//! The size of a photo's frame, as choosing its lens reads it. Ours, on
//! RAWmakase's rule.
//!
//! A body that records another aspect ratio (a Panasonic in 16:9 or 1:1, an
//! LX3 in any of its four modes) records a smaller part of its sensor, of
//! another crop factor. RAWmakase reads the frame as its decoder's default crop
//! (`lens/lensfun/matching.rs::Photo::frame` @ v0.7.1, 26267b16, MIT); Argentum
//! reads the default crop rawler gives, without decoding the pixels, when it
//! first reads a raw's EXIF, and keeps it in the EXIF map under [`KEY`] as
//! `WIDTHxHEIGHT`. Any other file gives its frame in EXIF's own
//! PixelXDimension and PixelYDimension.

use std::collections::HashMap;

/// The EXIF map's key for a raw's frame, `WIDTHxHEIGHT` in pixels.
pub const KEY: &str = "PhotoFrame";

/// Add a raw's frame to its EXIF map.
pub fn fill(map: &mut HashMap<String, String>, file_bytes: &[u8]) {
    if let Some([w, h]) = raw_frame(file_bytes) {
        map.insert(KEY.to_string(), format!("{w}x{h}"));
    }
}

/// The default crop of a raw, else its active area, else its whole image, in
/// pixels: what rawler's development keeps.
fn raw_frame(file_bytes: &[u8]) -> Option<[usize; 2]> {
    let source = rawler::rawsource::RawSource::new_from_slice(file_bytes);
    let loader = rawler::RawLoader::new();
    let decoder = loader.get_decoder(&source).ok()?;
    let image = decoder
        .raw_image(&source, &rawler::decoders::RawDecodeParams::default(), true)
        .ok()?;
    let [w, h] = match image.crop_area.or(image.active_area) {
        Some(rect) => [rect.d.w, rect.d.h],
        None => [image.width, image.height],
    };
    (w > 0 && h > 0).then_some([w, h])
}

/// The frame `exif` records, width and height in pixels.
pub fn read(exif: &HashMap<String, String>) -> Option<[f32; 2]> {
    let number = |key: &str| {
        let text = exif.get(key)?.trim();
        let value: f32 = text.split_whitespace().next()?.parse().ok()?;
        (value.is_finite() && value > 0.).then_some(value)
    };
    let frame = match exif.get(KEY) {
        Some(text) => {
            let (w, h) = text.trim().split_once('x')?;
            [w.trim().parse().ok()?, h.trim().parse().ok()?]
        }
        None => [number("PixelXDimension")?, number("PixelYDimension")?],
    };
    frame
        .iter()
        .all(|v: &f32| v.is_finite() && *v > 0.)
        .then_some(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exif(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn a_raw_s_frame_wins_over_exif_s_dimensions() {
        let both = exif(&[
            (KEY, "3968x2232"),
            ("PixelXDimension", "1920"),
            ("PixelYDimension", "1080"),
        ]);
        assert_eq!(read(&both), Some([3968., 2232.]));
        let jpeg = exif(&[("PixelXDimension", "6000"), ("PixelYDimension", "4000")]);
        assert_eq!(read(&jpeg), Some([6000., 4000.]));
    }

    #[test]
    fn a_frame_not_recorded_or_unreadable_is_none() {
        assert_eq!(read(&exif(&[])), None);
        assert_eq!(read(&exif(&[(KEY, "3968")])), None);
        assert_eq!(read(&exif(&[(KEY, "0x2232")])), None);
        assert_eq!(read(&exif(&[("PixelXDimension", "6000")])), None);
    }

    /// The photos named in AG_RAWS, read as the app reads them: the frame each
    /// gives, its lens, and whether the terms sent to the renderer stay exactly
    /// what they were without the frame. A photo of its sensor's shape must.
    ///
    /// ```text
    /// AG_RAWS=...;... cargo test --lib photo_frame::tests::named_raws_keep_their_correction -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "reads AG_RAWS"]
    fn named_raws_keep_their_correction() {
        use crate::lens_correction::{find_best_lens_match, resolve_lens_params};
        let db = crate::lens_correction::read_lensfun_dir(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/lensfun_db"
        )));
        let raws = std::env::var("AG_RAWS").expect("AG_RAWS");
        for photo in raws.split(';').filter(|p| !p.is_empty()) {
            let bytes = std::fs::read(photo).expect("a photo");
            let mut before = crate::mods::lens_name::with_lens(
                crate::exif_processing::read_exif_data_from_bytes(photo, &bytes),
                &bytes,
            );
            crate::mods::canon_crop_mode::fill(&mut before, &bytes);
            let mut after = before.clone();
            fill(&mut after, &bytes);
            let get = |k: &str| before.get(k).cloned().unwrap_or_default();
            let sent = |exif: &HashMap<String, String>| {
                find_best_lens_match(
                    &db,
                    &get("Make"),
                    &get("LensModel"),
                    &get("Model"),
                    &crate::mods::lens_shot::MatchShot::from_exif(exif),
                )
                .map(|(maker, display)| {
                    let terms = resolve_lens_params(&db, &maker, &display, exif);
                    format!("{display} {}", serde_json::to_string(&terms).unwrap())
                })
            };
            let (was, now) = (sent(&before), sent(&after));
            let name = photo.rsplit(['/', '\\']).next().unwrap_or(photo);
            println!(
                "{name}: frame {:?}, {}: {}",
                after.get(KEY),
                was.as_deref().unwrap_or("no lens"),
                if was == now { "identical" } else { "CHANGED" }
            );
            assert_eq!(was, now, "{name}");
        }
    }

    #[test]
    fn a_file_rawler_cannot_open_adds_nothing() {
        let mut map = HashMap::new();
        fill(&mut map, b"not a raw");
        assert!(map.is_empty());
    }
}
