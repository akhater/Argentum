//! One-click Subject, Sky and Background masks, from RAWmakase.
//!
//! The models and the code that runs them are RAWmakase's (`src-tauri/rm_selection`,
//! crates/rawmakase-inference @ 7c3c81a, MIT): DETR panoptic finds the photo's
//! people, animals and sky, and SAM 2.1 draws their outlines. Argentum's part
//! is here: the picture they are given, the answer in the form our AI masks
//! already store, and when the models are let go.
//!
//! The picture is the one their mask models read (lib.rs
//! `get_cached_full_warped_image`: the RAW developed, its geometry warp
//! applied, before crop and rotation). It is scaled to a 1280 px long edge,
//! which is what RAWmakase gives its models (src/app/subject_mask/worker.rs:30).
//! The answer comes back at that size and is scaled to the working image's,
//! the frame a stored AI mask is drawn from (mask_generation.rs
//! `generate_ai_bitmap_from_full_mask`). It is stored as their Foreground or
//! Sky mask stores its own (ai_processing.rs `AiForegroundMaskParameters`), so
//! it renders through their code unchanged.
//!
//! One analysis is kept, for the last photo and geometry: Subject, Sky and
//! Background on the same photo answer from it without running a model again.
//! Object, from clicks and a box, answers from the same photo's embedding.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
#[cfg(test)]
use std::time::Instant;

use super::ai_session::IdleSlot;

use base64::{Engine as _, engine::general_purpose};
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, GrayImage};
use rm_selection::{Analysis, Embedding, LoadOptions, Point, Prompt, Session, Subject};
use serde::{Deserialize, Serialize};

/// RAWmakase's working size for these models.
const LONG_EDGE: u32 = 1280;

/// Which part of the photo to select.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Subject,
    Sky,
    Background,
}

/// What the mask stores: the fields of their `AiForegroundMaskParameters`, and
/// whether anything was found at all.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub mask_data_base64: String,
    pub rotation: f32,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub orientation_steps: u8,
    pub found: bool,
}

/// The photo's frame as the mask stores it, passed through unchanged.
#[derive(Debug, Clone, Copy, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    #[serde(default)]
    pub rotation: f32,
    #[serde(default)]
    pub flip_horizontal: bool,
    #[serde(default)]
    pub flip_vertical: bool,
    #[serde(default)]
    pub orientation_steps: u8,
}

struct Loaded {
    session: Session,
    analysis: Option<(u64, Analysis)>,
    /// The photo's embedding alone, when Object came before any analysis.
    embedding: Option<(u64, Embedding)>,
}

static LOADED: IdleSlot<Loaded> = IdleSlot::new("selection models");

/// Which photo and geometry a picture is: the analysis is reused while it stays the same.
pub fn picture_key(path: &str, js_adjustments: &serde_json::Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    crate::cache_utils::calculate_geometry_hash(js_adjustments).hash(&mut hasher);
    hasher.finish()
}

/// Select `kind` in `picture` (the warped working image) with the models in `dir`.
pub fn select(
    dir: &Path,
    picture: &Arc<DynamicImage>,
    key: u64,
    kind: Kind,
    frame: Frame,
) -> Result<Selection, String> {
    super::selection_models::verify(dir)?;
    let input = model_input(picture);
    let coverage = with_models(dir, |loaded| {
        if loaded.analysis.as_ref().map(|(k, _)| *k) != Some(key) {
            let cancel = AtomicBool::new(false);
            let analysis = loaded
                .session
                .analyze(&input, &cancel)
                .map_err(|e| format!("{e:?}"))?;
            loaded.analysis = Some((key, analysis));
        }
        let (_, analysis) = loaded.analysis.as_ref().ok_or("no analysis")?;
        match kind {
            Kind::Subject | Kind::Background => analysis.subject(),
            Kind::Sky => analysis.sky(),
        }
        .map_err(|e| format!("{e:?}"))
    })?;
    finish(coverage, picture, kind == Kind::Background, frame)
}

/// One click on the photo: where, in the canvas's frame of the whole photo
/// (rotated, flipped and turned as it is shown, before the crop), and whether
/// it adds or leaves out.
#[derive(Debug, Clone, Copy, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Click {
    pub x: f64,
    pub y: f64,
    pub positive: bool,
}

/// A box dragged on the photo, in the same frame as a click.
#[derive(Debug, Clone, Copy, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Bounds {
    pub start_x: f64,
    pub start_y: f64,
    pub end_x: f64,
    pub end_y: f64,
}

/// Object: whatever the clicks and the box say, on SAM 2.1. RAWmakase's click
/// selection (`Session::segment`), answered from the photo's embedding, which
/// an analysis for Subject or Sky has already made when there is one.
pub fn select_object(
    dir: &Path,
    picture: &Arc<DynamicImage>,
    key: u64,
    clicks: &[Click],
    bounds: Option<Bounds>,
    frame: Frame,
) -> Result<Selection, String> {
    super::selection_models::verify(dir)?;
    let (width, height) = picture.dimensions();
    let prompt = object_prompt(clicks, bounds, width as f64, height as f64, frame)?;
    let input = model_input(picture);
    let coverage = with_models(dir, |loaded| {
        let cancel = AtomicBool::new(false);
        let analysed = loaded.analysis.as_ref().is_some_and(|(k, _)| *k == key);
        if !analysed && loaded.embedding.as_ref().is_none_or(|(k, _)| *k != key) {
            let embedding = loaded
                .session
                .embed(&input, &cancel)
                .map_err(|e| format!("{e:?}"))?;
            loaded.embedding = Some((key, embedding));
        }
        let embedding = if analysed {
            loaded.analysis.as_ref().map(|(_, a)| a.embedding())
        } else {
            loaded.embedding.as_ref().map(|(_, e)| e)
        }
        .ok_or("no embedding")?;
        loaded
            .session
            .segment(embedding, &prompt, &cancel)
            .map_err(|e| format!("{e:?}"))
    })?;
    finish(coverage, picture, false, frame)
}

/// The clicks and box as RAWmakase's prompt: fractions of the picture. Only the
/// last clicks are kept when there are more than a prompt takes.
fn object_prompt(
    clicks: &[Click],
    bounds: Option<Bounds>,
    width: f64,
    height: f64,
    frame: Frame,
) -> Result<Prompt, String> {
    let fraction = |p: (f64, f64)| {
        let (x, y) = to_picture(p, width, height, frame);
        (
            (x / width).clamp(0.0, 1.0) as f32,
            (y / height).clamp(0.0, 1.0) as f32,
        )
    };
    let keep = clicks
        .len()
        .saturating_sub(rm_selection::process::MAX_POINTS);
    let points: Vec<Point> = clicks[keep..]
        .iter()
        .map(|c| {
            let (x, y) = fraction((c.x, c.y));
            Point {
                x,
                y,
                positive: c.positive,
            }
        })
        .collect();
    let bounds = bounds.map(|b| {
        let corners = [
            fraction((b.start_x, b.start_y)),
            fraction((b.start_x, b.end_y)),
            fraction((b.end_x, b.end_y)),
            fraction((b.end_x, b.start_y)),
        ];
        let xs = corners.map(|c| c.0);
        let ys = corners.map(|c| c.1);
        [
            xs.iter().copied().fold(1.0, f32::min),
            ys.iter().copied().fold(1.0, f32::min),
            xs.iter().copied().fold(0.0, f32::max),
            ys.iter().copied().fold(0.0, f32::max),
        ]
    });
    if points.is_empty() && bounds.is_none() {
        return Err("Click on the photo or drag a box".to_string());
    }
    Ok(Prompt { points, bounds })
}

/// From the canvas's frame to the picture's: the inverse of how the photo is
/// shown. Their Object mask's own mapping (ai_commands.rs
/// `generate_ai_subject_mask`): the fine rotation about the turned photo's
/// centre undone, then the flips, then the quarter turns.
fn to_picture(p: (f64, f64), width: f64, height: f64, frame: Frame) -> (f64, f64) {
    let steps = frame.orientation_steps % 4;
    let (turned_w, turned_h) = if steps % 2 == 1 {
        (height, width)
    } else {
        (width, height)
    };
    let (cx, cy) = (turned_w / 2.0, turned_h / 2.0);
    let (sin, cos) = (frame.rotation as f64).to_radians().sin_cos();
    let (px, py) = (p.0 - cx, p.1 - cy);
    let mut q = (px * cos + py * sin + cx, -px * sin + py * cos + cy);
    if frame.flip_horizontal {
        q.0 = turned_w - q.0;
    }
    if frame.flip_vertical {
        q.1 = turned_h - q.1;
    }
    match steps {
        1 => (q.1, height - q.0),
        2 => (width - q.0, height - q.1),
        3 => (width - q.1, q.0),
        _ => q,
    }
}

/// The picture at RAWmakase's working size, as the models take it.
fn model_input(picture: &DynamicImage) -> rm_selection::RgbImage {
    let small = picture
        .resize(LONG_EDGE, LONG_EDGE, FilterType::Triangle)
        .to_rgb8();
    rm_selection::RgbImage {
        width: small.width() as usize,
        height: small.height() as usize,
        data: small.into_raw(),
    }
}

/// The loaded models, loading them first if they are not.
fn with_models<T>(
    dir: &Path,
    work: impl FnOnce(&mut Loaded) -> Result<T, String>,
) -> Result<T, String> {
    LOADED.with(
        dir,
        || {
            let options = LoadOptions {
                // The runtime the app's other models already use (lib.rs sets it).
                runtime_library: std::env::var_os("ORT_DYLIB_PATH").map(PathBuf::from),
                ..LoadOptions::default()
            };
            let session = Subject::load_with(dir, &options).map_err(|e| format!("{e:?}"))?;
            Ok(Loaded {
                session,
                analysis: None,
                embedding: None,
            })
        },
        work,
    )
}

/// The coverage at the working image's size, as the mask stores it.
fn finish(
    coverage: rm_selection::Coverage,
    picture: &DynamicImage,
    invert: bool,
    frame: Frame,
) -> Result<Selection, String> {
    let (width, height) = picture.dimensions();
    let found = coverage.data.iter().any(|v| *v > 0);
    let mut mask =
        GrayImage::from_raw(coverage.width as u32, coverage.height as u32, coverage.data)
            .ok_or("the selection came back the wrong size")?;
    if invert {
        image::imageops::invert(&mut mask);
    }
    let full = image::imageops::resize(&mask, width, height, FilterType::Triangle);
    Ok(Selection {
        mask_data_base64: png_data_url(&full)?,
        rotation: frame.rotation,
        flip_horizontal: frame.flip_horizontal,
        flip_vertical: frame.flip_vertical,
        orientation_steps: frame.orientation_steps,
        found,
    })
}

/// As their AI masks store theirs (ai_commands.rs `encode_to_base64_png`), but
/// with the fastest compression: a mask is long runs of black and white, so it
/// stays small, and an Object click does not wait on a full-size PNG.
pub(super) fn png_data_url(mask: &GrayImage) -> Result<String, String> {
    use image::ImageEncoder;
    use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
    let mut bytes = Cursor::new(Vec::new());
    PngEncoder::new_with_quality(&mut bytes, CompressionType::Fast, PngFilter::Sub)
        .write_image(
            mask.as_raw(),
            mask.width(),
            mask.height(),
            image::ExtendedColorType::L8,
        )
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "data:image/png;base64,{}",
        general_purpose::STANDARD.encode(bytes.get_ref())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real models on a real photo: set AG_SELECTION_MODELS to the folder of
    /// the five files, AG_SELECTION_PHOTO to a RAW with a person in it, and
    /// ORT_DYLIB_PATH to onnxruntime.dll, then run with --ignored.
    #[test]
    #[ignore = "needs the models, a runtime and a photo"]
    fn a_person_is_found_and_background_is_the_rest() {
        let dir = PathBuf::from(std::env::var("AG_SELECTION_MODELS").unwrap());
        let photo = std::env::var("AG_SELECTION_PHOTO").unwrap();
        let bytes = std::fs::read(&photo).unwrap();
        let picture = Arc::new(crate::raw_processing::extract_embedded_preview(&bytes).unwrap());
        let frame = Frame {
            rotation: 0.0,
            flip_horizontal: false,
            flip_vertical: false,
            orientation_steps: 0,
        };
        let started = Instant::now();
        let subject = select(&dir, &picture, 1, Kind::Subject, frame).unwrap();
        let first = started.elapsed();
        let started = Instant::now();
        let background = select(&dir, &picture, 1, Kind::Background, frame).unwrap();
        let again = started.elapsed();
        eprintln!("subject {first:?}, background from the same analysis {again:?}");
        assert!(subject.found && background.found);
        assert!(
            subject
                .mask_data_base64
                .starts_with("data:image/png;base64,")
        );
        assert!(again < first, "the second selection reused the analysis");
    }

    /// A selection's mask back as pixels.
    fn decode(selection: &Selection) -> GrayImage {
        let data = selection
            .mask_data_base64
            .trim_start_matches("data:image/png;base64,");
        let bytes = general_purpose::STANDARD.decode(data).unwrap();
        image::load_from_memory(&bytes).unwrap().to_luma8()
    }

    /// The real models again: one click in the middle of the person Subject
    /// found selects the same person as Object. Same variables as above.
    #[test]
    #[ignore = "needs the models, a runtime and a photo"]
    fn a_click_on_the_person_selects_the_person() {
        let dir = PathBuf::from(std::env::var("AG_SELECTION_MODELS").unwrap());
        let photo = std::env::var("AG_SELECTION_PHOTO").unwrap();
        let bytes = std::fs::read(&photo).unwrap();
        let picture = Arc::new(crate::raw_processing::extract_embedded_preview(&bytes).unwrap());
        let subject = decode(&select(&dir, &picture, 2, Kind::Subject, PLAIN).unwrap());
        let (mut sx, mut sy, mut n) = (0f64, 0f64, 0f64);
        for (x, y, p) in subject.enumerate_pixels() {
            if p[0] > 127 {
                sx += x as f64;
                sy += y as f64;
                n += 1.0;
            }
        }
        assert!(n > 0.0, "Subject found no one to click on");
        let click = Click {
            x: sx / n,
            y: sy / n,
            positive: true,
        };
        let started = Instant::now();
        let object = decode(&select_object(&dir, &picture, 2, &[click], None, PLAIN).unwrap());
        eprintln!(
            "object from the analysis's embedding {:?}",
            started.elapsed()
        );
        let (mut both, mut either) = (0f64, 0f64);
        for (a, b) in subject.pixels().zip(object.pixels()) {
            let (a, b) = (a[0] > 127, b[0] > 127);
            both += (a && b) as u8 as f64;
            either += (a || b) as u8 as f64;
        }
        let overlap = both / either.max(1.0);
        eprintln!("Object and Subject overlap: {overlap:.2}");
        assert!(overlap > 0.5, "overlap {overlap:.2}");
    }

    /// The real models again: a box drawn around the person Subject found, with
    /// no click, selects the same person. Same variables as above.
    #[test]
    #[ignore = "needs the models, a runtime and a photo"]
    fn a_box_around_the_person_selects_the_person() {
        let dir = PathBuf::from(std::env::var("AG_SELECTION_MODELS").unwrap());
        let photo = std::env::var("AG_SELECTION_PHOTO").unwrap();
        let bytes = std::fs::read(&photo).unwrap();
        let picture = Arc::new(crate::raw_processing::extract_embedded_preview(&bytes).unwrap());
        let subject = decode(&select(&dir, &picture, 3, Kind::Subject, PLAIN).unwrap());
        let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0, 0);
        for (x, y, p) in subject.enumerate_pixels() {
            if p[0] > 127 {
                (left, top) = (left.min(x), top.min(y));
                (right, bottom) = (right.max(x), bottom.max(y));
            }
        }
        assert!(left < right && top < bottom, "Subject found no one to box");
        let bounds = Bounds {
            start_x: left as f64,
            start_y: top as f64,
            end_x: right as f64,
            end_y: bottom as f64,
        };
        let object = decode(&select_object(&dir, &picture, 3, &[], Some(bounds), PLAIN).unwrap());
        let (mut both, mut either) = (0f64, 0f64);
        for (a, b) in subject.pixels().zip(object.pixels()) {
            let (a, b) = (a[0] > 127, b[0] > 127);
            both += (a && b) as u8 as f64;
            either += (a || b) as u8 as f64;
        }
        let overlap = both / either.max(1.0);
        eprintln!("boxed Object and Subject overlap: {overlap:.2}");
        assert!(overlap > 0.5, "overlap {overlap:.2}");
    }

    const PLAIN: Frame = Frame {
        rotation: 0.0,
        flip_horizontal: false,
        flip_vertical: false,
        orientation_steps: 0,
    };

    /// As shown, a click is where it lands on the photo.
    #[test]
    fn a_click_on_a_photo_shown_as_it_is_lands_where_it_was_made() {
        let (x, y) = to_picture((100.0, 40.0), 400.0, 300.0, PLAIN);
        assert!((x - 100.0).abs() < 1e-9 && (y - 40.0).abs() < 1e-9);
    }

    /// Flipped, turned a quarter, or tilted, the click goes back through each.
    #[test]
    fn a_click_is_taken_back_through_the_flip_the_turn_and_the_tilt() {
        let flipped = Frame {
            flip_horizontal: true,
            ..PLAIN
        };
        let (x, _) = to_picture((100.0, 40.0), 400.0, 300.0, flipped);
        assert!((x - 300.0).abs() < 1e-9);

        // Turned a quarter, the shown photo is 300 wide and 400 high; its
        // top-left corner is the photo's bottom-left.
        let turned = Frame {
            orientation_steps: 1,
            ..PLAIN
        };
        let (x, y) = to_picture((0.0, 0.0), 400.0, 300.0, turned);
        assert!(x.abs() < 1e-9 && (y - 300.0).abs() < 1e-9);

        // Tilted, the centre stays the centre.
        let tilted = Frame {
            rotation: 7.0,
            ..PLAIN
        };
        let (x, y) = to_picture((200.0, 150.0), 400.0, 300.0, tilted);
        assert!((x - 200.0).abs() < 1e-9 && (y - 150.0).abs() < 1e-9);
    }

    /// The prompt is in fractions of the photo; a box keeps its corners in order.
    #[test]
    fn clicks_and_a_box_become_fractions_of_the_photo() {
        let clicks = [
            Click {
                x: 200.0,
                y: 150.0,
                positive: true,
            },
            Click {
                x: 40.0,
                y: 30.0,
                positive: false,
            },
        ];
        let bounds = Bounds {
            start_x: 300.0,
            start_y: 240.0,
            end_x: 100.0,
            end_y: 60.0,
        };
        let prompt = object_prompt(&clicks, Some(bounds), 400.0, 300.0, PLAIN).unwrap();
        assert_eq!(prompt.points.len(), 2);
        assert!((prompt.points[0].x - 0.5).abs() < 1e-6 && prompt.points[0].positive);
        assert!(!prompt.points[1].positive);
        let [l, t, r, b] = prompt.bounds.unwrap();
        assert!((l - 0.25).abs() < 1e-6 && (t - 0.2).abs() < 1e-6);
        assert!((r - 0.75).abs() < 1e-6 && (b - 0.8).abs() < 1e-6);
        assert!(object_prompt(&[], None, 400.0, 300.0, PLAIN).is_err());
    }

    /// Another geometry is another picture; the same one is the same.
    #[test]
    fn the_analysis_is_reused_only_for_the_same_photo_and_geometry() {
        let flat = serde_json::json!({});
        let tilted = serde_json::json!({ "transformRotate": 5.0 });
        assert_eq!(picture_key("a.CR3", &flat), picture_key("a.CR3", &flat));
        assert_ne!(picture_key("a.CR3", &flat), picture_key("b.CR3", &flat));
        assert_ne!(picture_key("a.CR3", &flat), picture_key("a.CR3", &tilted));
    }
}
