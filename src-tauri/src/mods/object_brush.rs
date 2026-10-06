//! The object brush: paint roughly over something and get a mask that fits it.
//!
//! WHAT IT IS
//!
//! Lightroom's "Select Object" in brush mode. RapidRAW's Subject tool takes a
//! box or a click and hands it to SAM. This takes brush strokes instead: the
//! painted area says *which* thing, SAM says where its edges are.
//!
//! WHAT IT ASKS SAM
//!
//! SAM's decoder accepts any number of points (label 1 for "this", 0 for "not
//! this"), a box (labels 2 and 3), and a previous mask to refine. A stroke is
//! turned into all three:
//!
//! - points spread evenly along the painted path, so a long stroke over a long
//!   object is not described by its first click alone;
//! - a box around the paint, grown by a margin, which fixes the *scale* of the
//!   answer — without it, three points on a shirt can come back as the shirt, the
//!   person, or a single fold;
//! - and the decoder's own low-resolution answer fed back once, the refinement
//!   step SAM was trained with.
//!
//! The box is the one guess. Paint that only covers part of an object would be
//! cut off at the box, so each request is also decoded without it, and whichever
//! answer agrees better with the paint wins — see `pick`.
//!
//! Strokes painted with Alt held are exclusions: their points go in with label
//! 0, and an answer that spills over them loses.
//!
//! WHAT IT REUSES OF THEIRS
//!
//! The models, the embedding cache, the warped image, and the edge refinement
//! that makes a Subject mask follow hair. A painted mask is stored as an
//! ordinary `ai-subject` mask, so rendering, export and the grow and feather
//! controls are theirs, unchanged. Only the prompt is ours.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::sync::Mutex;

use anyhow::{Result, anyhow};
use base64::{Engine as _, engine::general_purpose};
use image::imageops::{self, FilterType};
use image::{DynamicImage, GenericImageView, GrayImage, ImageFormat, Luma};
use imageproc::region_labelling::{Connectivity, connected_components};
use ndarray::Array;
use ort::session::Session;
use ort::value::Tensor;
use serde::Deserialize;

use crate::ai_processing::{
    AiSubjectMaskParameters, ImageEmbeddings, fast_guided_filter, generate_image_embeddings,
    get_or_init_ai_models,
};
use crate::app_state::AppState;
use crate::cache_utils::GEOMETRY_KEYS;
use crate::get_cached_full_warped_image;
use crate::mods::matting;

/// SAM's input is a 1024-pixel square; the photo is scaled so its long side
/// fills it. Theirs is a private constant in ai_processing.rs; it is fixed by
/// the model, not by either of us.
const SAM_SIZE: f64 = 1024.0;

/// The side of SAM's low-resolution mask, which is also its mask input.
const LOW_RES: usize = 256;

/// How many "this" points a request carries at most, spread over all strokes.
const MAX_POSITIVE: usize = 12;

/// How many "not this" points.
const MAX_NEGATIVE: usize = 6;

/// How far the box around the paint is grown, as a share of its larger side.
/// Rough paint stops short of edges; the margin lets the answer reach them.
const BOX_MARGIN: f64 = 0.12;

/// An answer covering more of the frame than this is not an object.
const MAX_AREA: f64 = 0.6;

/// How much better another answer must score to displace the boxed one.
const TIE: f64 = 0.08;

/// How much of its box's edge the boxed answer may run into before it counts
/// as cut off: the paint slices across something rather than covering it.
///
/// Measured on real photos. Paint over a whole thing: 0.01 to 0.09 (mug, shirt,
/// shoe, lamp). Paint over an eye: 0.22 and 0.27, because an eye fills its own
/// box corner to corner — and an eye is what was meant. A line across a mug,
/// a hand with only its fingers painted, one stroke down a lawn: 0.39 to 0.53.
/// The first version used 0.12 and an eye came back as the eye and eyebrow,
/// and on AK's photo as the whole face.
const CUT_LIMIT: f64 = 0.35;

/// How much of the exclusions the boxed answer may cover before the others
/// are asked.
const TRESPASS_LIMIT: f64 = 0.1;

/// The part of an exclusion stroke an answer is held to, as a share of the
/// brush radius.
const EXCLUSION_CORE: f64 = 0.3;

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// One brush stroke, as the frontend records it.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Stroke {
    /// In the same space as RapidRAW's own mask coordinates: the rotated,
    /// uncropped photo.
    pub points: Vec<Point>,
    /// Brush radius, in the same pixels as the points.
    pub radius: f64,
    /// Painted with Alt: what the mask must leave out.
    #[serde(default)]
    pub exclude: bool,
}

/// How the photo on screen relates to the image SAM was shown.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub rotation: f32,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub orientation_steps: u8,
    /// The warped source image, before any of the above.
    pub width: u32,
    pub height: u32,
}

impl View {
    /// A point on screen, in the source image SAM embedded.
    ///
    /// This is their mapping, from `generate_ai_subject_mask` in
    /// ai_commands.rs, applied to one point rather than to four box corners:
    /// undo the fine rotation about the centre, then the flips, then the
    /// quarter turns. It has to stay identical to theirs, or a painted mask and
    /// a boxed one land in different places.
    pub fn to_source(self, (x, y): (f64, f64)) -> (f64, f64) {
        let (w, h) = (self.width as f64, self.height as f64);
        let (cw, ch) = if self.orientation_steps % 2 == 1 {
            (h, w)
        } else {
            (w, h)
        };
        let (cx, cy) = (cw / 2.0, ch / 2.0);

        let a = (self.rotation as f64).to_radians();
        let (px, py) = (x - cx, y - cy);
        let (mut x, mut y) = (
            px * a.cos() + py * a.sin() + cx,
            -px * a.sin() + py * a.cos() + cy,
        );

        if self.flip_horizontal {
            x = cw - x;
        }
        if self.flip_vertical {
            y = ch - y;
        }

        match self.orientation_steps {
            1 => (y, h - x),
            2 => (w - x, h - y),
            3 => (w - y, x),
            _ => (x, y),
        }
    }
}

/// A stroke in SAM's input frame: the source scaled so its long side is 1024.
#[derive(Clone, Debug)]
struct Path {
    points: Vec<(f64, f64)>,
    radius: f64,
    exclude: bool,
}

impl Path {
    fn length(&self) -> f64 {
        self.points
            .windows(2)
            .map(|p| ((p[1].0 - p[0].0).powi(2) + (p[1].1 - p[0].1).powi(2)).sqrt())
            .sum()
    }
}

fn to_sam_frame(strokes: &[Stroke], view: &View) -> Vec<Path> {
    to_frame(strokes, view, SAM_SIZE / view.width.max(view.height) as f64)
}

/// The strokes in the source image, scaled by `scale`.
fn to_frame(strokes: &[Stroke], view: &View, scale: f64) -> Vec<Path> {
    strokes
        .iter()
        .filter(|s| !s.points.is_empty())
        .map(|s| Path {
            points: s
                .points
                .iter()
                .map(|p| {
                    let (x, y) = view.to_source((p.x, p.y));
                    (x * scale, y * scale)
                })
                .collect(),
            radius: (s.radius * scale).max(1.0),
            exclude: s.exclude,
        })
        .collect()
}

/// `n` points evenly spaced along a path by distance travelled, ends included.
fn sample_along(points: &[(f64, f64)], n: usize) -> Vec<(f64, f64)> {
    if points.is_empty() || n == 0 {
        return Vec::new();
    }
    let segments: Vec<f64> = points
        .windows(2)
        .map(|p| ((p[1].0 - p[0].0).powi(2) + (p[1].1 - p[0].1).powi(2)).sqrt())
        .collect();
    let total: f64 = segments.iter().sum();
    if n == 1 || total <= f64::EPSILON {
        // A dab: its middle, not wherever the pen came down.
        return vec![points[points.len() / 2]];
    }

    let mut out = Vec::with_capacity(n);
    let mut segment = 0;
    let mut walked = 0.0;
    for i in 0..n {
        let target = total * i as f64 / (n - 1) as f64;
        while segment < segments.len() - 1 && walked + segments[segment] < target {
            walked += segments[segment];
            segment += 1;
        }
        let len = segments[segment];
        let t = if len > 0.0 {
            ((target - walked) / len).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (a, b) = (points[segment], points[segment + 1]);
        out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
    }
    out
}

/// Spread a point budget over several paths in proportion to their length,
/// every path getting at least one.
fn sample_paths<'a>(paths: impl Iterator<Item = &'a Path>, budget: usize) -> Vec<(f64, f64)> {
    let paths: Vec<&Path> = paths.collect();
    if paths.is_empty() {
        return Vec::new();
    }
    let lengths: Vec<f64> = paths.iter().map(|p| p.length()).collect();
    let total: f64 = lengths.iter().sum();
    let spare = budget.saturating_sub(paths.len()) as f64;
    paths
        .iter()
        .zip(&lengths)
        .flat_map(|(path, len)| {
            let share = if total > 0.0 {
                (spare * len / total).round() as usize
            } else {
                0
            };
            sample_along(&path.points, 1 + share)
        })
        .collect()
}

/// One short touch of the brush, rather than paint over an area.
fn is_dab(paths: &[Path]) -> bool {
    let positive: Vec<&Path> = paths.iter().filter(|p| !p.exclude).collect();
    positive.len() == 1 && positive[0].length() < 2.0 * positive[0].radius
}

/// The paint's bounding box, brush width included, grown by `BOX_MARGIN` and
/// kept inside the image.
fn paint_box(paths: &[Path], frame: (f64, f64), grow: f64) -> Option<[f64; 4]> {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for path in paths.iter().filter(|p| !p.exclude) {
        for &(x, y) in &path.points {
            b[0] = b[0].min(x - path.radius);
            b[1] = b[1].min(y - path.radius);
            b[2] = b[2].max(x + path.radius);
            b[3] = b[3].max(y + path.radius);
        }
    }
    if b[0] > b[2] {
        return None;
    }
    let margin = grow * (b[2] - b[0]).max(b[3] - b[1]);
    Some([
        (b[0] - margin).max(0.0),
        (b[1] - margin).max(0.0),
        (b[2] + margin).min(frame.0),
        (b[3] + margin).min(frame.1),
    ])
}

/// Paint the strokes into a `w`×`h` grid whose pixels are `cell` SAM pixels
/// wide. Each stroke is a chain of capsules: every pixel within `reach` times
/// the brush radius of the path.
fn rasterise(
    paths: &[Path],
    exclude: bool,
    reach: f64,
    w: usize,
    h: usize,
    cell: f64,
) -> Vec<bool> {
    let mut grid = vec![false; w * h];
    for path in paths.iter().filter(|p| p.exclude == exclude) {
        let r = (path.radius * reach / cell).max(0.75);
        let pts: Vec<(f64, f64)> = path
            .points
            .iter()
            .map(|&(x, y)| (x / cell, y / cell))
            .collect();
        let pairs: Vec<((f64, f64), (f64, f64))> = if pts.len() == 1 {
            vec![(pts[0], pts[0])]
        } else {
            pts.windows(2).map(|p| (p[0], p[1])).collect()
        };
        for (a, b) in pairs {
            let x0 = (a.0.min(b.0) - r).floor().max(0.0) as usize;
            let y0 = (a.1.min(b.1) - r).floor().max(0.0) as usize;
            let x1 = ((a.0.max(b.0) + r).ceil() as usize).min(w.saturating_sub(1));
            let y1 = ((a.1.max(b.1) + r).ceil() as usize).min(h.saturating_sub(1));
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len2 = dx * dx + dy * dy;
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                    let t = if len2 > 0.0 {
                        (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let (qx, qy) = (a.0 + dx * t - px, a.1 + dy * t - py);
                    if qx * qx + qy * qy <= r * r {
                        grid[y * w + x] = true;
                    }
                }
            }
        }
    }
    grid
}

/// What one request to the decoder says.
#[derive(Clone, Debug, Default)]
struct Prompt {
    coords: Vec<(f32, f32)>,
    labels: Vec<f32>,
}

impl Prompt {
    fn new(positive: &[(f64, f64)], negative: &[(f64, f64)], bx: Option<[f64; 4]>) -> Self {
        let mut p = Prompt::default();
        for &(x, y) in positive {
            p.coords.push((x as f32, y as f32));
            p.labels.push(1.0);
        }
        for &(x, y) in negative {
            p.coords.push((x as f32, y as f32));
            p.labels.push(0.0);
        }
        match bx {
            Some(b) => {
                p.coords.push((b[0] as f32, b[1] as f32));
                p.labels.push(2.0);
                p.coords.push((b[2] as f32, b[3] as f32));
                p.labels.push(3.0);
            }
            // Without a box SAM expects a padding point in its place; this is
            // how the reference ONNX example calls it, and leaving it out
            // makes the last real point behave like half a box.
            None => {
                p.coords.push((0.0, 0.0));
                p.labels.push(-1.0);
            }
        }
        p
    }
}

/// One answer from the decoder.
struct Answer {
    /// Logits at the size asked for; positive means inside.
    mask: Vec<f32>,
    width: usize,
    height: usize,
    /// SAM's own estimate of how good the mask is.
    score: f32,
    /// The 256×256 logits, which is what goes back in to refine.
    low_res: Vec<f32>,
}

/// Run the decoder once. `size` is (width, height) of the mask wanted back.
fn decode(
    decoder: &Mutex<Session>,
    embeddings: &ImageEmbeddings,
    prompt: &Prompt,
    previous: Option<&[f32]>,
    size: (u32, u32),
) -> Result<Answer> {
    let n = prompt.coords.len();
    let coords: Vec<f32> = prompt.coords.iter().flat_map(|&(x, y)| [x, y]).collect();
    let coords = Array::from_shape_vec((1, n, 2), coords)?.into_dyn();
    let labels = Array::from_shape_vec((1, n), prompt.labels.clone())?.into_dyn();
    let mask_input = match previous {
        Some(m) => Array::from_shape_vec((1, 1, LOW_RES, LOW_RES), m.to_vec())?,
        None => Array::zeros((1, 1, LOW_RES, LOW_RES)),
    }
    .into_dyn();
    let has_mask = Array::from_elem((1,), if previous.is_some() { 1.0f32 } else { 0.0 }).into_dyn();
    let orig_size = Array::from_shape_vec((2,), vec![size.1 as f32, size.0 as f32])?.into_dyn();

    let mut session = decoder.lock().unwrap();
    let outputs = session.run(ort::inputs![
        Tensor::from_array(
            embeddings
                .embeddings
                .clone()
                .as_standard_layout()
                .into_owned()
        )?,
        Tensor::from_array(coords)?,
        Tensor::from_array(labels)?,
        Tensor::from_array(mask_input)?,
        Tensor::from_array(has_mask)?,
        Tensor::from_array(orig_size)?
    ])?;

    let masks = outputs[0].try_extract_array::<f32>()?;
    let shape = masks.shape().to_vec();
    let (height, width) = (shape[2], shape[3]);
    let mask = masks.iter().take(width * height).copied().collect();
    let score = outputs[1]
        .try_extract_array::<f32>()?
        .iter()
        .next()
        .copied()
        .unwrap_or(0.0);
    let low_res = outputs[2]
        .try_extract_array::<f32>()?
        .iter()
        .take(LOW_RES * LOW_RES)
        .copied()
        .collect();

    Ok(Answer {
        mask,
        width,
        height,
        score,
        low_res,
    })
}

/// How each way of asking scored, and whether it won.
type Report = Vec<(Ask, Fit, bool)>;

/// The three ways a request is put to SAM.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ask {
    /// Points along the paint, inside a box around it. The usual answer.
    Boxed,
    /// The same points in a box round the brush's path rather than its paint:
    /// for rough paint that spilled past the thing. See `tighter_is_meant`.
    Tight,
    /// The same points in a box half as big again on every side: for paint
    /// that covers only part of the thing. Bounded, so it can reach past the
    /// paint but never jump to everything around it.
    Grown,
    /// The same points, no box at all. Only for a dab, which has no extent
    /// to grow from: a dab is a click, and this is SAM's answer to a click.
    /// Unbounded, it took a whole face for a line drawn along an eye.
    Open,
    /// The box alone: for paint that crosses several parts of one thing, where
    /// a point on each part pulls the answer apart.
    BoxOnly,
}

/// How well an answer agrees with what was painted.
#[derive(Clone, Copy, Debug)]
struct Fit {
    /// Share of the painted area the mask covers.
    covered: f64,
    /// Share of the excluded area the mask covers anyway.
    trespass: f64,
    /// Share of the whole frame the mask covers.
    area: f64,
    /// Share of the box's edge the mask runs into. A mask cut off by its box
    /// carries on past it, so the box was too small for the thing.
    cut: f64,
    /// SAM's own estimate of the mask's quality.
    confidence: f64,
}

fn fit(
    mask: &[f32],
    width: usize,
    height: usize,
    include: &[bool],
    exclude: &[bool],
    bx: Option<[f64; 4]>,
) -> Fit {
    let mut inside = 0usize;
    let mut area = 0usize;
    let mut painted = 0usize;
    let mut excluded = 0usize;
    let mut trespass = 0usize;
    for (i, &v) in mask.iter().enumerate() {
        let m = v > 0.0;
        area += m as usize;
        if include[i] {
            painted += 1;
            inside += m as usize;
        }
        if exclude[i] {
            excluded += 1;
            trespass += m as usize;
        }
    }
    Fit {
        covered: inside as f64 / painted.max(1) as f64,
        trespass: trespass as f64 / excluded.max(1) as f64,
        area: area as f64 / (width * height).max(1) as f64,
        cut: bx.map_or(0.0, |b| box_cut(mask, width, height, b)),
        confidence: 0.0,
    }
}

/// How much of the box's edge lies on the mask, counting only the sides that
/// are inside the frame — a box against the photo's border cuts nothing off.
fn box_cut(mask: &[f32], w: usize, h: usize, b: [f64; 4]) -> f64 {
    if w < 2 || h < 2 {
        return 0.0;
    }
    let x0 = (b[0].ceil() as usize).min(w - 1);
    let y0 = (b[1].ceil() as usize).min(h - 1);
    let x1 = (b[2].floor().max(0.0) as usize).min(w - 1);
    let y1 = (b[3].floor().max(0.0) as usize).min(h - 1);
    if x0 >= x1 || y0 >= y1 {
        return 0.0;
    }
    let on = |x: usize, y: usize| (mask[y * w + x] > 0.0) as usize;
    let (mut hits, mut seen) = (0usize, 0usize);
    if x0 > 0 {
        hits += (y0..=y1).map(|y| on(x0, y)).sum::<usize>();
        seen += y1 - y0 + 1;
    }
    if x1 < w - 1 {
        hits += (y0..=y1).map(|y| on(x1, y)).sum::<usize>();
        seen += y1 - y0 + 1;
    }
    if y0 > 0 {
        hits += (x0..=x1).map(|x| on(x, y0)).sum::<usize>();
        seen += x1 - x0 + 1;
    }
    if y1 < h - 1 {
        hits += (x0..=x1).map(|x| on(x, y1)).sum::<usize>();
        seen += x1 - x0 + 1;
    }
    hits as f64 / seen.max(1) as f64
}

/// The mask for a set of strokes, at the size of the source image.
fn segment(
    decoder: &Mutex<Session>,
    embeddings: &ImageEmbeddings,
    strokes: &[Stroke],
    view: &View,
) -> Result<GrayImage> {
    segment_reporting(decoder, embeddings, strokes, view).map(|(mask, _)| mask)
}

/// `segment`, also saying how each way of asking scored and which won.
///
/// Every candidate is decoded at SAM's own resolution, where it is cheap; only
/// the winner is decoded again at full size, refined by its own first answer.
fn segment_reporting(
    decoder: &Mutex<Session>,
    embeddings: &ImageEmbeddings,
    strokes: &[Stroke],
    view: &View,
) -> Result<(GrayImage, Report)> {
    let paths = to_sam_frame(strokes, view);
    if !paths.iter().any(|p| !p.exclude) {
        return Err(anyhow!("paint over the object first"));
    }

    let (w, h) = (view.width as f64, view.height as f64);
    let scale = SAM_SIZE / w.max(h);
    let frame = ((w * scale).round(), (h * scale).round());
    let small = (frame.0 as u32, frame.1 as u32);
    let (sw, sh) = (small.0 as usize, small.1 as usize);

    let positive = sample_paths(paths.iter().filter(|p| !p.exclude), MAX_POSITIVE);
    let negative = sample_paths(paths.iter().filter(|p| p.exclude), MAX_NEGATIVE);
    let include = rasterise(&paths, false, 1.0, sw, sh, 1.0);
    // Only the middle of an exclusion. Rough paint over the fingers holding a
    // mug clips the mug's edge too; that edge was never meant.
    let exclude = rasterise(&paths, true, EXCLUSION_CORE, sw, sh, 1.0);
    let bx = paint_box(&paths, frame, BOX_MARGIN);
    let grown = paint_box(&paths, frame, GROWN_MARGIN);

    let tight = path_box(&paths, frame);
    let mut candidates = Vec::new();
    for ask in [Ask::Boxed, Ask::Tight, Ask::Grown, Ask::Open, Ask::BoxOnly] {
        let (prompt, its_box) = match ask {
            Ask::Boxed => (Prompt::new(&positive, &negative, bx), bx),
            Ask::Tight => (Prompt::new(&positive, &negative, tight), tight),
            Ask::Grown => (Prompt::new(&positive, &negative, grown), grown),
            Ask::Open => (Prompt::new(&positive, &negative, None), None),
            Ask::BoxOnly => (Prompt::new(&[], &negative, bx), bx),
        };
        let answer = decode(decoder, embeddings, &prompt, None, small)?;
        let mut f = fit(
            &answer.mask,
            answer.width,
            answer.height,
            &include,
            &exclude,
            its_box,
        );
        f.confidence = answer.score as f64;
        candidates.push((ask, f, prompt, answer));
    }

    let fits: Vec<(Ask, Fit)> = candidates.iter().map(|(a, f, _, _)| (*a, *f)).collect();
    let mut chosen = pick(&fits, is_dab(&paths));
    if chosen == Ask::Boxed {
        let line = rasterise(&paths, false, 0.0, sw, sh, 1.0);
        let mask = |ask: Ask| {
            candidates
                .iter()
                .find(|(a, ..)| *a == ask)
                .map(|(.., answer)| &answer.mask)
        };
        if let (Some(boxed), Some(tight)) = (mask(Ask::Boxed), mask(Ask::Tight))
            && tighter_is_meant(boxed, tight, &include, &line)
        {
            chosen = Ask::Tight;
        }
    }
    let (_, _, prompt, first) = candidates.into_iter().find(|(a, ..)| *a == chosen).unwrap();
    let report = fits.into_iter().map(|(a, f)| (a, f, a == chosen)).collect();

    let refined = decode(
        decoder,
        embeddings,
        &prompt,
        Some(&first.low_res),
        (view.width, view.height),
    )?;
    let pixels = refined
        .mask
        .iter()
        .map(|&v| if v > 0.0 { 255 } else { 0 })
        .collect();
    let mask = GrayImage::from_raw(refined.width as u32, refined.height as u32, pixels)
        .ok_or_else(|| anyhow!("the decoder returned a mask of the wrong size"))?;
    Ok((mask, report))
}

/// Which way of asking agrees best with the paint.
///
/// The brush selects what was painted. The boxed answer — the thing inside the
/// paint's own extent — stands unless there is evidence against it: its box
/// plainly cut it off (`CUT_LIMIT`), or it runs over an exclusion. A dab has
/// no extent to go by, so it is a click, and the open answer is SAM's answer
/// to a click.
///
/// Covering more of the paint is *not* evidence. Rough paint over a lamp is
/// mostly the wall behind it, and the whole wall covers all of it — AK's first
/// test picked the wall exactly that way.
///
/// When the boxed answer is in doubt, every answer scores what it covers of
/// the paint, less twice what it covers of the exclusions, less how far its
/// box cut it off. One that takes most of the frame is answering a different
/// question. The boxed answer keeps the choice on a near-tie.
fn pick(fits: &[(Ask, Fit)], dab: bool) -> Ask {
    if dab
        && fits
            .iter()
            .any(|(a, f)| *a == Ask::Open && f.area <= MAX_AREA)
    {
        return Ask::Open;
    }
    let Some(boxed) = fits.iter().find(|(a, _)| *a == Ask::Boxed).map(|(_, f)| *f) else {
        return Ask::Boxed;
    };
    if boxed.cut <= CUT_LIMIT && boxed.trespass <= TRESPASS_LIMIT {
        return Ask::Boxed;
    }
    let score =
        |f: &Fit| f.covered - 2.0 * f.trespass - f.cut - if f.area > MAX_AREA { 1.0 } else { 0.0 };
    let boxed = score(&boxed);
    fits.iter()
        .filter(|(a, _)| *a != Ask::Open && *a != Ask::Tight)
        .map(|(a, f)| (*a, score(f)))
        .filter(|&(a, s)| a == Ask::Boxed || s > boxed + TIE)
        .max_by(|x, y| x.1.total_cmp(&y.1))
        .map_or(Ask::Boxed, |(a, _)| a)
}

/// SAM's mask, made to follow the photo's own edges.
///
/// Their refinement, from `run_sam_decoder` in ai_processing.rs, so a painted
/// mask and a boxed one look alike: soften, then a guided filter steered by the
/// photo, then trim the faint fringe the filter leaves.
fn follow_edges(mask: &GrayImage, image: &DynamicImage) -> GrayImage {
    let (w, h) = image.dimensions();
    let guide = image.to_luma8();
    let radius = (w.max(h) as f32 * 0.0075).clamp(8.0, 24.0);
    let coarse = if mask.dimensions() == (w, h) {
        mask.clone()
    } else {
        imageops::resize(mask, w, h, FilterType::Triangle)
    };
    let soft = imageops::blur(&coarse, radius / 3.0);
    let mut refined = fast_guided_filter(&guide, &soft, radius as usize, 0.01);
    for p in refined.pixels_mut() {
        let v = p[0] as f32 / 255.0;
        p[0] = (((v - 0.03) / 0.94).clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    refined
}

macro_rules! timed {
    ($label:expr, $e:expr) => {{
        #[cfg(test)]
        let t = std::time::Instant::now();
        let r = $e;
        #[cfg(test)]
        eprintln!("    {}: {:?}", $label, t.elapsed());
        r
    }};
}

/// The models the brush runs on. Matting is optional: until its download has
/// arrived, or if it failed, edges are refined the way a Subject mask's are.
pub struct Models<'a> {
    pub encoder: &'a Mutex<Session>,
    pub decoder: &'a Mutex<Session>,
    pub matting: Option<&'a Mutex<Session>>,
}

/// How far the second box reaches past the paint on every side, as a share
/// of the paint's larger side. Half again: a line across a mug grows to the
/// mug, a stroke down a lawn to the lawn, a line along an eye to the eye and
/// its lids — not to the face.
const GROWN_MARGIN: f64 = 0.5;

/// Things smaller than this share of the photo get a second look on a crop.
/// To SAM the whole photo is 1024 pixels, so an eye or a bracelet is a few
/// dozen of them and its mask is a few cells of a 256-pixel grid: the first
/// answer can be wrong about *what* it is, not just where its edge is, and no
/// edge model fixes that. The crop costs a second SAM encode, several seconds
/// on a CPU, so bigger things — already plenty of pixels — skip it.
const ZOOM_BELOW: f64 = 0.2;

/// Context around a small thing for its second look, as a share of its size.
const ZOOM_MARGIN: f64 = 0.25;

/// How much of the brush's path an answer on the close-up must cover to
/// count as the thing painted along.
const PATH_COVERAGE: f64 = 0.8;

/// How much less of the paint the tighter answer may cover than the bigger
/// one and still be taken.
const COVER_SLACK: f64 = 0.1;

/// How well the second look must agree with the first to replace it.
const ZOOM_AGREEMENT: f64 = 0.5;

/// The band the matting model decides: this many of SAM's grid cells either
/// side of SAM's edge, which is where the true edge can be. No wider. A band
/// of 3% of the object let the model take most of a face into a hair mask —
/// it separates a thing from its background, and inside a person it cannot
/// tell hair from skin.
const EDGE_CELLS: f64 = 1.5;

/// The whole brush: which thing, then where exactly its edge is.
///
/// 1. SAM on the whole photo picks the thing (`segment`).
/// 2. SAM again on a crop around it. The whole photo is 1024 pixels to SAM, so
///    a bracelet on a 6000-pixel photo is about 30 of them, and its mask is
///    drawn on a 256-pixel grid. Cropped, the bracelet gets the 1024 to
///    itself, and a box that fits it exactly.
/// 3. The matting model draws the final edge on the crop at full resolution.
pub fn select(
    models: &Models,
    embeddings: &ImageEmbeddings,
    strokes: &[Stroke],
    view: &View,
    image: &DynamicImage,
) -> Result<GrayImage> {
    let coarse = timed!(
        "segment",
        segment(models.decoder, embeddings, strokes, view)?
    );
    let painted = sample_paths(
        to_frame(strokes, view, 1.0).iter().filter(|p| !p.exclude),
        64,
    );
    let coarse = keep_painted(coarse, &painted);
    match timed!("refine", refine(models, strokes, view, image, &coarse)) {
        Ok(mask) => Ok(mask),
        Err(e) => {
            log::warn!("object brush: refinement failed, using the first answer: {e}");
            Ok(follow_edges(&coarse, image))
        }
    }
}

/// Only the parts of a mask the paint touches.
///
/// SAM's answer can carry specks elsewhere in the photo — a second eye, a
/// fleck of the same colour. They are not what was painted, and one far away
/// makes a small thing look as big as the photo, so it never gets its closer
/// look. Paint more to add a part.
fn keep_painted(mut mask: GrayImage, painted: &[(f64, f64)]) -> GrayImage {
    let labels = connected_components(&mask, Connectivity::Eight, Luma([0u8]));
    let (w, h) = mask.dimensions();
    let mut keep: Vec<u32> = painted
        .iter()
        .filter(|(x, y)| *x >= 0.0 && *y >= 0.0 && (*x as u32) < w && (*y as u32) < h)
        .map(|&(x, y)| labels.get_pixel(x as u32, y as u32)[0])
        .filter(|&l| l != 0)
        .collect();
    if keep.is_empty() {
        // The paint missed every part: keep the largest rather than nothing.
        let mut sizes = std::collections::HashMap::new();
        for p in labels.pixels().filter(|p| p[0] != 0) {
            *sizes.entry(p[0]).or_insert(0usize) += 1;
        }
        keep.extend(sizes.into_iter().max_by_key(|&(_, n)| n).map(|(l, _)| l));
    }
    for (p, l) in mask.pixels_mut().zip(labels.pixels()) {
        if !keep.contains(&l[0]) {
            p[0] = 0;
        }
    }
    mask
}

fn bounding_box(mask: &GrayImage) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, p) in mask.enumerate_pixels() {
        if p[0] > 127 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    (x0 <= x1).then_some((x0, y0, x1 + 1, y1 + 1))
}

fn refine(
    models: &Models,
    strokes: &[Stroke],
    view: &View,
    image: &DynamicImage,
    coarse: &GrayImage,
) -> Result<GrayImage> {
    let (w, h) = image.dimensions();
    let long = w.max(h) as f64;
    let (bx0, by0, bx1, by1) =
        bounding_box(coarse).ok_or_else(|| anyhow!("nothing was selected"))?;
    let size = (bx1 - bx0).max(by1 - by0) as f64;
    let small = size < ZOOM_BELOW * long;

    // SAM's edge can be off by about one cell of its 256-pixel grid, so the
    // band the matting model decides must reach that far either side. A
    // second look on a crop has a much
    // finer grid.
    let margin = if small { ZOOM_MARGIN * size } else { 0.0 };
    let reach = |cell: f64| (EDGE_CELLS * cell).clamp(3.0, 255.0);
    let pad = (margin + 2.0 * reach(long / 256.0) + 8.0) as u32;
    let (cx0, cy0) = (bx0.saturating_sub(pad), by0.saturating_sub(pad));
    let (cx1, cy1) = ((bx1 + pad).min(w), (by1 + pad).min(h));
    let (cw, ch) = (cx1 - cx0, cy1 - cy0);
    let crop = image.crop_imm(cx0, cy0, cw, ch);
    let coarse_crop = imageops::crop_imm(coarse, cx0, cy0, cw, ch).to_image();

    let (mask, cell) = if small {
        let found = (bx0 - cx0, by0 - cy0, bx1 - cx0, by1 - cy0);
        let zoomed = timed!(
            "zoom",
            zoom(models, strokes, view, &crop, (cx0, cy0), found)?
        );
        // A second look that disagrees badly with the first has found
        // something else; the first is the one the paint chose.
        if iou(&zoomed, &coarse_crop) >= ZOOM_AGREEMENT {
            (zoomed, cw.max(ch) as f64 / 256.0)
        } else {
            (coarse_crop, long / 256.0)
        }
    } else {
        (coarse_crop, long / 256.0)
    };

    let edge = match models.matting {
        Some(session) => {
            let trimap = matting::trimap(&mask, reach(cell) as u8);
            timed!("matte", matting::matte(session, &crop.to_rgb8(), &trimap)?)
        }
        None => follow_edges(&mask, &crop),
    };

    let mut out = GrayImage::new(w, h);
    imageops::replace(&mut out, &edge, cx0 as i64, cy0 as i64);
    Ok(out)
}

/// SAM on the crop alone, with a box that fits the first answer and, unless
/// it was a dab, with a box round the brush's path; `tighter_is_meant`
/// decides between them.
fn zoom(
    models: &Models,
    strokes: &[Stroke],
    view: &View,
    crop: &DynamicImage,
    origin: (u32, u32),
    found: (u32, u32, u32, u32),
) -> Result<GrayImage> {
    let embeddings = generate_image_embeddings(crop, models.encoder)?;
    let (cw, ch) = crop.dimensions();
    let scale = SAM_SIZE / cw.max(ch) as f64;

    // The strokes in the crop's SAM frame.
    let paths: Vec<Path> = to_frame(strokes, view, 1.0)
        .into_iter()
        .map(|p| Path {
            points: p
                .points
                .iter()
                .map(|&(x, y)| ((x - origin.0 as f64) * scale, (y - origin.1 as f64) * scale))
                .collect(),
            radius: (p.radius * scale).max(1.0),
            exclude: p.exclude,
        })
        .collect();
    let frame = ((cw as f64 * scale).round(), (ch as f64 * scale).round());
    let (sw, sh) = (frame.0 as usize, frame.1 as usize);
    let inside = |&(x, y): &(f64, f64)| x >= 0.0 && y >= 0.0 && x < frame.0 && y < frame.1;
    let positive: Vec<(f64, f64)> = sample_paths(paths.iter().filter(|p| !p.exclude), MAX_POSITIVE)
        .into_iter()
        .filter(inside)
        .collect();
    let negative: Vec<(f64, f64)> = sample_paths(paths.iter().filter(|p| p.exclude), MAX_NEGATIVE)
        .into_iter()
        .filter(inside)
        .collect();

    let (fx0, fy0, fx1, fy1) = found;
    let generous = [fx0 as f64, fy0 as f64, fx1 as f64, fy1 as f64].map(|v| v * scale);
    let mut prompts = vec![Prompt::new(&positive, &negative, Some(generous))];
    if !is_dab(&paths)
        && let Some(tight) = path_box(&paths, frame)
    {
        prompts.push(Prompt::new(&positive, &negative, Some(tight)));
    }

    // Chosen at SAM's own resolution, then the winner again at full size,
    // refined by its first answer.
    let line = rasterise(&paths, false, 0.0, sw, sh, 1.0);
    let paint = rasterise(&paths, false, 1.0, sw, sh, 1.0);
    let size = (sw as u32, sh as u32);
    let mut answers = Vec::new();
    for prompt in prompts {
        let answer = decode(models.decoder, &embeddings, &prompt, None, size)?;
        answers.push((prompt, answer));
    }
    let take_tight = answers.len() > 1
        && tighter_is_meant(&answers[0].1.mask, &answers[1].1.mask, &paint, &line);
    let (prompt, first) = answers.swap_remove(if take_tight { 1 } else { 0 });

    let second = decode(
        models.decoder,
        &embeddings,
        &prompt,
        Some(&first.low_res),
        (cw, ch),
    )?;
    let pixels = second
        .mask
        .iter()
        .map(|&v| if v > 0.0 { 255 } else { 0 })
        .collect();
    GrayImage::from_raw(second.width as u32, second.height as u32, pixels)
        .ok_or_else(|| anyhow!("the decoder returned a mask of the wrong size"))
}

/// Whether the tighter of two answers is the one meant.
///
/// Rough paint spills past what it is over: a brush run along an eye also
/// covers some lid with its edge, and SAM, given a box that holds that spill,
/// returns the eye and the lid. Given a box round the brush's *path*, it
/// returns the eye. The tighter answer is taken when it is smaller, still
/// covers the path, and explains nearly as much of the paint as the bigger
/// one.
///
/// The last condition is what keeps a part from winning. A stroke across a
/// shoe passes over its strap and charms; tight round the path, SAM returned
/// those alone. They cover the path but leave most of the painted shoe out.
/// Measured: the eye with its lids and the eye alone differ by 0.05 and 0.06
/// of the paint; the shoe and its strap by much more.
fn tighter_is_meant(bigger: &[f32], tighter: &[f32], paint: &[bool], line: &[bool]) -> bool {
    let (mut on_line, mut length) = (0usize, 0usize);
    let (mut big, mut tight, mut painted) = (0usize, 0usize, 0usize);
    let (mut big_on_paint, mut tight_on_paint) = (0usize, 0usize);
    for i in 0..bigger.len() {
        let (b, t) = (bigger[i] > 0.0, tighter[i] > 0.0);
        big += b as usize;
        tight += t as usize;
        if line[i] {
            length += 1;
            on_line += t as usize;
        }
        if paint[i] {
            painted += 1;
            big_on_paint += b as usize;
            tight_on_paint += t as usize;
        }
    }
    let painted = painted.max(1) as f64;
    tight > 0
        && tight < big
        && on_line as f64 >= PATH_COVERAGE * length.max(1) as f64
        && tight_on_paint as f64 / painted >= big_on_paint as f64 / painted - COVER_SLACK
}

/// A box round the brush's path, reaching half a brush width either side of
/// it: the rough paint's own spill is left out.
fn path_box(paths: &[Path], frame: (f64, f64)) -> Option<[f64; 4]> {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for path in paths.iter().filter(|p| !p.exclude) {
        let r = path.radius * 0.5;
        for &(x, y) in &path.points {
            b = [
                b[0].min(x - r),
                b[1].min(y - r),
                b[2].max(x + r),
                b[3].max(y + r),
            ];
        }
    }
    (b[0] < b[2]).then(|| {
        [
            b[0].max(0.0),
            b[1].max(0.0),
            b[2].min(frame.0),
            b[3].min(frame.1),
        ]
    })
}

fn iou(a: &GrayImage, b: &GrayImage) -> f64 {
    let (mut both, mut either) = (0usize, 0usize);
    for (p, q) in a.pixels().zip(b.pixels()) {
        let (x, y) = (p[0] > 127, q[0] > 127);
        both += (x && y) as usize;
        either += (x || y) as usize;
    }
    if either == 0 {
        1.0
    } else {
        both as f64 / either as f64
    }
}

fn to_data_url(image: &GrayImage) -> Result<String, String> {
    let mut buf = Cursor::new(Vec::new());
    image
        .write_to(&mut buf, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "data:image/png;base64,{}",
        general_purpose::STANDARD.encode(buf.get_ref())
    ))
}

/// The key their embedding cache is filed under. It has to be byte-for-byte
/// theirs (`generate_ai_subject_mask` and `precompute_ai_subject_mask`): their
/// precompute warms the cache the moment the mask is selected, and a different
/// key would throw that away and encode the photo again.
fn embedding_key(path: &str, js_adjustments: &serde_json::Value) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(path.as_bytes());
    let mut geo_hasher = DefaultHasher::new();
    for key in GEOMETRY_KEYS {
        if let Some(val) = js_adjustments.get(key) {
            key.hash(&mut geo_hasher);
            val.to_string().hash(&mut geo_hasher);
        }
    }
    hasher.update(&geo_hasher.finish().to_le_bytes());
    hasher.finalize().to_hex().to_string()
}

/// The bounding box of what was painted, in screen coordinates. Stored as the
/// mask's start and end so their overlay outlines the painted region.
fn painted_extent(strokes: &[Stroke]) -> ((f64, f64), (f64, f64)) {
    let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for s in strokes.iter().filter(|s| !s.exclude) {
        for p in &s.points {
            b = (
                b.0.min(p.x - s.radius),
                b.1.min(p.y - s.radius),
                b.2.max(p.x + s.radius),
                b.3.max(p.y + s.radius),
            );
        }
    }
    ((b.0, b.1), (b.2, b.3))
}

#[allow(clippy::too_many_arguments)]
pub async fn generate(
    js_adjustments: serde_json::Value,
    path: String,
    strokes: Vec<Stroke>,
    rotation: f32,
    flip_horizontal: bool,
    flip_vertical: bool,
    orientation_steps: u8,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<AiSubjectMaskParameters, String> {
    let models = get_or_init_ai_models(&app_handle, &state.ai_state, &state.ai_init_lock)
        .await
        .map_err(|e| e.to_string())?;
    let key = embedding_key(&path, &js_adjustments);
    let warped = get_cached_full_warped_image(&state, &js_adjustments)?;

    // The same cache, filled the same way, as their Subject tool.
    let embeddings = {
        let mut lock = state.ai_state.lock().unwrap();
        let ai_state = lock.as_mut().ok_or("AI models are not loaded")?;
        match &ai_state.embeddings {
            Some(cached) if cached.path_hash == key => cached.clone(),
            _ => {
                let mut fresh = generate_image_embeddings(warped.as_ref(), &models.sam_encoder)
                    .map_err(|e| e.to_string())?;
                fresh.path_hash = key.clone();
                ai_state.embeddings = Some(fresh.clone());
                fresh
            }
        }
    };

    let (width, height) = embeddings.original_size;
    let view = View {
        rotation,
        flip_horizontal,
        flip_vertical,
        orientation_steps,
        width,
        height,
    };
    // First use downloads it, about 100 MB. If that fails the brush still
    // works, with the old edge.
    let matting = match matting::session(&app_handle).await {
        Ok(session) => Some(session),
        Err(e) => {
            log::warn!("object brush: no matting model, using the plain edge: {e}");
            None
        }
    };
    let brush = Models {
        encoder: &models.sam_encoder,
        decoder: &models.sam_decoder,
        matting: matting.as_deref(),
    };
    let mask =
        select(&brush, &embeddings, &strokes, &view, warped.as_ref()).map_err(|e| e.to_string())?;

    let (start, end) = painted_extent(&strokes);
    Ok(AiSubjectMaskParameters {
        start_x: start.0,
        start_y: start.1,
        end_x: end.0,
        end_y: end.1,
        mask_data_base64: Some(to_data_url(&mask)?),
        rotation: Some(rotation),
        flip_horizontal: Some(flip_horizontal),
        flip_vertical: Some(flip_vertical),
        orientation_steps: Some(orientation_steps),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(rotation: f32, fh: bool, fv: bool, steps: u8) -> View {
        View {
            rotation,
            flip_horizontal: fh,
            flip_vertical: fv,
            orientation_steps: steps,
            width: 600,
            height: 400,
        }
    }

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
    }

    #[test]
    fn an_untouched_view_maps_a_point_to_itself() {
        assert!(close(
            view(0.0, false, false, 0).to_source((120.0, 75.0)),
            (120.0, 75.0)
        ));
    }

    /// Quarter turns: what is on screen at a corner came from a known corner
    /// of the source. The screen is 400×600 after one or three turns.
    #[test]
    fn quarter_turns_undo_to_the_right_corner() {
        // One turn clockwise: the screen's top-left came from the source's
        // bottom-left.
        assert!(close(
            view(0.0, false, false, 1).to_source((0.0, 0.0)),
            (0.0, 400.0)
        ));
        assert!(close(
            view(0.0, false, false, 2).to_source((0.0, 0.0)),
            (600.0, 400.0)
        ));
        assert!(close(
            view(0.0, false, false, 3).to_source((0.0, 0.0)),
            (600.0, 0.0)
        ));
    }

    #[test]
    fn flips_mirror_about_the_frame() {
        assert!(close(
            view(0.0, true, false, 0).to_source((100.0, 50.0)),
            (500.0, 50.0)
        ));
        assert!(close(
            view(0.0, false, true, 0).to_source((100.0, 50.0)),
            (100.0, 350.0)
        ));
    }

    #[test]
    fn rotation_turns_about_the_centre() {
        let v = view(90.0, false, false, 0);
        assert!(close(v.to_source((300.0, 200.0)), (300.0, 200.0)));
        // 90 degrees: a point right of centre came from above it.
        let (x, y) = v.to_source((400.0, 200.0));
        assert!(
            (x - 300.0).abs() < 1e-6 && (y - 100.0).abs() < 1e-6,
            "{x} {y}"
        );
    }

    /// The four corners of a box, mapped one by one, must land where their
    /// box mapping puts them — this is the same transform, on a point.
    #[test]
    fn matches_their_box_mapping_on_its_corners() {
        let v = view(17.0, true, false, 1);
        let (img_w, img_h) = (600.0, 400.0);
        let (cw, ch) = (img_h, img_w);
        let center = (cw / 2.0, ch / 2.0);
        let a = 17f64.to_radians();
        let theirs = |p: (f64, f64)| {
            let (px, py) = (p.0 - center.0, p.1 - center.1);
            let r = (
                px * a.cos() + py * a.sin() + center.0,
                -px * a.sin() + py * a.cos() + center.1,
            );
            let f = (cw - r.0, r.1);
            (f.1, img_h - f.0)
        };
        for p in [(10.0, 20.0), (390.0, 20.0), (10.0, 580.0), (200.0, 300.0)] {
            assert!(close(v.to_source(p), theirs(p)), "{p:?}");
        }
    }

    #[test]
    fn sampling_spreads_points_evenly_with_both_ends() {
        let pts = sample_along(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], 5);
        let want = [
            (0.0, 0.0),
            (5.0, 0.0),
            (10.0, 0.0),
            (10.0, 5.0),
            (10.0, 10.0),
        ];
        assert_eq!(pts.len(), 5);
        for (a, b) in pts.iter().zip(want) {
            assert!(close(*a, b), "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn a_dab_is_one_point() {
        assert_eq!(sample_along(&[(3.0, 4.0)], 6), vec![(3.0, 4.0)]);
        assert_eq!(sample_along(&[(3.0, 4.0), (3.0, 4.0)], 6).len(), 1);
    }

    #[test]
    fn the_budget_goes_to_the_longer_stroke() {
        let long = Path {
            points: vec![(0.0, 0.0), (90.0, 0.0)],
            radius: 5.0,
            exclude: false,
        };
        let short = Path {
            points: vec![(0.0, 50.0), (10.0, 50.0)],
            radius: 5.0,
            exclude: false,
        };
        let pts = sample_paths([&long, &short].into_iter(), 12);
        let on_long = pts.iter().filter(|p| p.1 == 0.0).count();
        assert!(
            on_long > 2 * (pts.len() - on_long),
            "{on_long} of {}",
            pts.len()
        );
        assert!(pts.len() <= 13);
    }

    #[test]
    fn the_box_holds_the_brush_and_a_margin_and_stays_in_frame() {
        let p = Path {
            points: vec![(100.0, 100.0), (200.0, 150.0)],
            radius: 10.0,
            exclude: false,
        };
        let b = paint_box(std::slice::from_ref(&p), (1024.0, 683.0), BOX_MARGIN).unwrap();
        let margin = BOX_MARGIN * 120.0;
        assert!((b[0] - (90.0 - margin)).abs() < 1e-9);
        assert!((b[3] - (160.0 + margin)).abs() < 1e-9);

        let edge = Path {
            points: vec![(2.0, 2.0)],
            radius: 10.0,
            exclude: false,
        };
        let b = paint_box(&[edge], (1024.0, 683.0), BOX_MARGIN).unwrap();
        assert_eq!((b[0], b[1]), (0.0, 0.0));
    }

    #[test]
    fn exclusions_do_not_count_towards_the_box() {
        let p = Path {
            points: vec![(500.0, 500.0)],
            radius: 4.0,
            exclude: true,
        };
        assert!(paint_box(&[p], (1024.0, 1024.0), BOX_MARGIN).is_none());
    }

    #[test]
    fn rasterising_fills_a_capsule() {
        let p = Path {
            points: vec![(5.0, 5.0), (15.0, 5.0)],
            radius: 2.0,
            exclude: false,
        };
        let g = rasterise(&[p], false, 1.0, 20, 10, 1.0);
        assert!(g[5 * 20 + 10]); // on the path
        assert!(g[6 * 20 + 4]); // inside the rounded end
        assert!(!g[9 * 20 + 10]); // beyond the radius
        assert!(!g[5 * 20 + 19]); // past the end
    }

    /// A 1-D world is enough: cells are pixels in a row.
    fn row(on: std::ops::Range<usize>, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| if on.contains(&i) { 1.0 } else { -1.0 })
            .collect()
    }
    fn flags(on: std::ops::Range<usize>, len: usize) -> Vec<bool> {
        (0..len).map(|i| on.contains(&i)).collect()
    }

    /// Paint over an eye spills onto the lid; the eye alone covers the path
    /// and nearly all the paint, so it is the one meant.
    #[test]
    fn the_eye_without_its_lid_is_meant() {
        let eye_and_lid = row(10..60, 100);
        let eye = row(20..55, 100);
        // The lid takes 2 of 37 painted cells: about the 0.06 measured.
        let paint = flags(19..56, 100);
        let path = flags(25..50, 100);
        assert!(tighter_is_meant(&eye_and_lid, &eye, &paint, &path));
    }

    /// A stroke across a shoe crosses its strap; the strap covers the path
    /// but leaves most of the painted shoe out, so the shoe stands.
    #[test]
    fn a_part_on_the_path_is_not_meant() {
        let shoe = row(10..80, 100);
        let strap = row(30..50, 100);
        let paint = flags(15..75, 100);
        let path = flags(32..48, 100);
        assert!(!tighter_is_meant(&shoe, &strap, &paint, &path));
    }

    #[test]
    fn a_tighter_answer_off_the_path_is_not_meant() {
        let thing = row(10..80, 100);
        let corner = row(10..30, 100);
        let paint = flags(10..35, 100);
        let path = flags(12..60, 100);
        assert!(!tighter_is_meant(&thing, &corner, &paint, &path));
    }

    #[test]
    fn only_the_painted_parts_are_kept() {
        let mut mask = GrayImage::new(30, 10);
        for x in 2..8 {
            mask.put_pixel(x, 5, Luma([255]));
        }
        for x in 20..28 {
            mask.put_pixel(x, 5, Luma([255]));
        }
        let kept = keep_painted(mask.clone(), &[(4.0, 5.0)]);
        assert_eq!(kept.get_pixel(5, 5)[0], 255);
        assert_eq!(kept.get_pixel(24, 5)[0], 0);
        // Missing every part keeps the biggest.
        let kept = keep_painted(mask, &[(15.0, 1.0)]);
        assert_eq!(kept.get_pixel(24, 5)[0], 255);
        assert_eq!(kept.get_pixel(5, 5)[0], 0);
    }

    #[test]
    fn a_prompt_without_a_box_carries_the_padding_point() {
        let p = Prompt::new(&[(1.0, 2.0)], &[(3.0, 4.0)], None);
        assert_eq!(p.labels, vec![1.0, 0.0, -1.0]);
        let p = Prompt::new(&[(1.0, 2.0)], &[], Some([0.0, 0.0, 9.0, 9.0]));
        assert_eq!(p.labels, vec![1.0, 2.0, 3.0]);
    }

    fn f(covered: f64, trespass: f64, area: f64, cut: f64) -> Fit {
        Fit {
            covered,
            trespass,
            area,
            cut,
            confidence: 0.9,
        }
    }

    #[test]
    fn the_boxed_answer_holds_a_near_tie() {
        let fits = [
            (Ask::Boxed, f(0.90, 0.0, 0.1, 0.4)),
            (Ask::Open, f(0.45, 0.0, 0.1, 0.0)),
            (Ask::BoxOnly, f(0.55, 0.0, 0.1, 0.0)),
        ];
        assert_eq!(pick(&fits, false), Ask::Boxed);
    }

    /// AK's lamp: rough paint over a see-through lamp is mostly wall, so the
    /// whole wall panel covers all of it. Covering more is not evidence; the
    /// lamp was not cut off by its box, so the lamp stands.
    /// An eye fills its own box, so it touches every side of it: 0.27 on a
    /// real photo. That is not the eye being cut off; the eye stands.
    #[test]
    fn an_eye_touching_its_box_is_not_cut_off() {
        let fits = [
            (Ask::Boxed, f(0.80, 0.0, 0.004, 0.27)),
            (Ask::Open, f(0.78, 0.0, 0.007, 0.0)),
            (Ask::BoxOnly, f(0.45, 0.0, 0.002, 0.04)),
        ];
        assert_eq!(pick(&fits, false), Ask::Boxed);
    }

    #[test]
    fn a_dab_is_a_click() {
        let fits = [
            (Ask::Boxed, f(1.0, 0.0, 0.001, 0.31)),
            (Ask::Open, f(1.0, 0.0, 0.07, 0.0)),
            (Ask::BoxOnly, f(0.9, 0.0, 0.001, 0.06)),
        ];
        assert_eq!(pick(&fits, true), Ask::Open);
    }

    #[test]
    fn a_dab_is_one_short_touch() {
        let dab = Path {
            points: vec![(10.0, 10.0), (14.0, 12.0)],
            radius: 5.0,
            exclude: false,
        };
        let stroke = Path {
            points: vec![(10.0, 10.0), (40.0, 10.0)],
            radius: 5.0,
            exclude: false,
        };
        assert!(is_dab(std::slice::from_ref(&dab)));
        assert!(!is_dab(&[stroke]));
        assert!(!is_dab(&[dab.clone(), dab]));
    }

    #[test]
    fn covering_more_of_the_paint_does_not_displace_an_uncut_answer() {
        let fits = [
            (Ask::Boxed, f(0.60, 0.0, 0.03, 0.02)),
            (Ask::Open, f(1.0, 0.0, 0.35, 0.0)),
            (Ask::BoxOnly, f(0.62, 0.0, 0.03, 0.02)),
        ];
        assert_eq!(pick(&fits, false), Ask::Boxed);
    }

    #[test]
    fn running_over_an_exclusion_puts_the_boxed_answer_in_doubt() {
        let fits = [
            (Ask::Boxed, f(0.85, 0.4, 0.07, 0.03)),
            (Ask::Open, f(0.84, 0.4, 0.07, 0.0)),
            (Ask::BoxOnly, f(0.80, 0.0, 0.06, 0.03)),
        ];
        assert_eq!(pick(&fits, false), Ask::BoxOnly);
    }

    /// A line across a mug: the boxed answer is a band the width of the line,
    /// filling the box edge to edge. The grown one is the mug.
    #[test]
    fn a_box_that_cut_the_object_off_loses() {
        let fits = [
            (Ask::Boxed, f(1.0, 0.0, 0.02, 0.5)),
            (Ask::Grown, f(0.98, 0.0, 0.08, 0.02)),
            (Ask::Open, f(0.98, 0.0, 0.08, 0.0)),
            (Ask::BoxOnly, f(1.0, 0.0, 0.02, 0.5)),
        ];
        assert_eq!(pick(&fits, false), Ask::Grown);
    }

    /// AK's close-up: a line along an eye looks cut off by its own thin box,
    /// and the unbounded answer is the whole face, which covers all the paint.
    /// The unbounded answer is for a click only.
    #[test]
    fn a_line_along_an_eye_never_becomes_the_face() {
        let fits = [
            (Ask::Boxed, f(0.83, 0.0, 0.004, 0.36)),
            (Ask::Grown, f(0.90, 0.0, 0.01, 0.05)),
            (Ask::Open, f(1.0, 0.0, 0.45, 0.0)),
            (Ask::BoxOnly, f(0.50, 0.0, 0.002, 0.15)),
        ];
        assert_ne!(pick(&fits, false), Ask::Open);
    }

    #[test]
    fn covering_the_exclusion_or_the_whole_frame_does_not_win() {
        let fits = [
            (Ask::Boxed, f(0.90, 0.0, 0.1, 0.4)),
            (Ask::Open, f(0.95, 0.5, 0.2, 0.0)),
            (Ask::BoxOnly, f(1.0, 0.0, 0.9, 0.0)),
        ];
        assert_eq!(pick(&fits, false), Ask::Boxed);
    }

    #[test]
    fn box_cut_counts_only_sides_inside_the_frame() {
        // A 10x10 mask fully on; a box from (2,2) to (7,7) is cut on every side.
        let mask = vec![1.0f32; 100];
        assert_eq!(box_cut(&mask, 10, 10, [2.0, 2.0, 7.0, 7.0]), 1.0);
        // The same mask against a box on the frame's edges: nothing cut off.
        assert_eq!(box_cut(&mask, 10, 10, [0.0, 0.0, 10.0, 10.0]), 0.0);
        // A mask inside the box does not touch it.
        let mut inner = vec![0.0f32; 100];
        inner[4 * 10 + 4] = 1.0;
        assert_eq!(box_cut(&inner, 10, 10, [2.0, 2.0, 7.0, 7.0]), 0.0);
    }

    #[test]
    fn strokes_deserialise_as_the_frontend_sends_them() {
        let v = serde_json::json!([
            { "points": [{ "x": 1.5, "y": 2.0 }], "radius": 12.0 },
            { "points": [{ "x": 3.0, "y": 4.0 }], "radius": 8.0, "exclude": true }
        ]);
        let s: Vec<Stroke> = serde_json::from_value(v).unwrap();
        assert_eq!(s[0].points[0], Point { x: 1.5, y: 2.0 });
        assert!(!s[0].exclude && s[1].exclude);
    }

    /// Against the real model, on a real photo. Writes overlays to look at.
    ///
    ///     ORT_DYLIB_PATH=…/onnxruntime.dll \
    ///     AG_SAM_DIR=…/models AG_SAM_IMAGE=photo.jpg AG_SAM_OUT=dir \
    ///     cargo test --lib object_brush::tests::on_a_real_photo -- --ignored --nocapture
    #[test]
    #[ignore]
    fn on_a_real_photo() {
        let dir = std::path::PathBuf::from(std::env::var("AG_SAM_DIR").expect("AG_SAM_DIR"));
        let image = image::open(std::env::var("AG_SAM_IMAGE").expect("AG_SAM_IMAGE")).unwrap();
        let out = std::path::PathBuf::from(std::env::var("AG_SAM_OUT").expect("AG_SAM_OUT"));
        let encoder = Mutex::new(
            Session::builder()
                .unwrap()
                .commit_from_file(dir.join("sam_vit_b_01ec64_encoder.onnx"))
                .unwrap(),
        );
        let decoder = Mutex::new(
            Session::builder()
                .unwrap()
                .commit_from_file(dir.join("sam_vit_b_01ec64_decoder.onnx"))
                .unwrap(),
        );
        for o in &decoder.lock().unwrap().outputs {
            println!("decoder output: {}", o.name);
        }
        let embeddings = generate_image_embeddings(&image, &encoder).unwrap();
        // Optional: a local copy of the matting model.
        let matting = std::env::var("AG_MATTE_MODEL")
            .ok()
            .map(|p| matting::load(std::path::Path::new(&p)).unwrap());
        let (width, height) = image.dimensions();
        let view = View {
            rotation: 0.0,
            flip_horizontal: false,
            flip_vertical: false,
            orientation_steps: 0,
            width,
            height,
        };

        let cases: Vec<(String, Vec<Stroke>)> = serde_json::from_str(
            &std::fs::read_to_string(std::env::var("AG_SAM_CASES").expect("AG_SAM_CASES")).unwrap(),
        )
        .unwrap();

        for (name, strokes) in cases {
            let t = std::time::Instant::now();
            let (_, report) = segment_reporting(&decoder, &embeddings, &strokes, &view).unwrap();
            let models = Models {
                encoder: &encoder,
                decoder: &decoder,
                matting: matting.as_deref(),
            };
            let mask = select(&models, &embeddings, &strokes, &view, &image).unwrap();
            println!("{name}: {:?}", t.elapsed());
            for (ask, fit, won) in report {
                println!("  {}{ask:?}: {fit:.2?}", if won { "* " } else { "  " });
            }
            let mut overlay = image.to_rgb8();
            for (x, y, p) in overlay.enumerate_pixels_mut() {
                let m = mask.get_pixel(x, y)[0] as f32 / 255.0;
                p[0] = (p[0] as f32 * (1.0 - 0.55 * m) + 255.0 * 0.55 * m) as u8;
                p[1] = (p[1] as f32 * (1.0 - 0.55 * m)) as u8;
                p[2] = (p[2] as f32 * (1.0 - 0.55 * m)) as u8;
            }
            for s in &strokes {
                for p in &s.points {
                    let c = if s.exclude {
                        [40, 40, 255]
                    } else {
                        [40, 255, 40]
                    };
                    for dy in -3i32..=3 {
                        for dx in -3i32..=3 {
                            let (x, y) = (p.x as i32 + dx, p.y as i32 + dy);
                            if x >= 0 && y >= 0 && (x as u32) < width && (y as u32) < height {
                                overlay.put_pixel(x as u32, y as u32, image::Rgb(c));
                            }
                        }
                    }
                }
            }
            overlay.save(out.join(format!("{name}.jpg"))).unwrap();
            if let Some((x0, y0, x1, y1)) = bounding_box(&mask) {
                let pad = ((x1 - x0).max(y1 - y0) / 4).max(20);
                let (cx0, cy0) = (x0.saturating_sub(pad), y0.saturating_sub(pad));
                let (cx1, cy1) = ((x1 + pad).min(width), (y1 + pad).min(height));
                let close = imageops::crop_imm(&overlay, cx0, cy0, cx1 - cx0, cy1 - cy0).to_image();
                let k = (600.0 / (cx1 - cx0).max(cy1 - cy0) as f64).max(1.0);
                let close = imageops::resize(
                    &close,
                    ((cx1 - cx0) as f64 * k) as u32,
                    ((cy1 - cy0) as f64 * k) as u32,
                    FilterType::Nearest,
                );
                close.save(out.join(format!("{name}_close.jpg"))).unwrap();
            }
        }
    }
}
