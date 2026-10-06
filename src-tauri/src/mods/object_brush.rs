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
use image::{DynamicImage, GenericImageView, GrayImage, ImageFormat};
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
/// as cut off. A cord leaving the top of a lamp touches the edge; a line drawn
/// across a mug fills it. Measured: 0.03 or less on paint that covers the
/// thing, 0.17 and up on paint that covers only part of it.
const CUT_LIMIT: f64 = 0.12;

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
    pub fn to_source(&self, (x, y): (f64, f64)) -> (f64, f64) {
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
    let scale = SAM_SIZE / view.width.max(view.height) as f64;
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

/// The paint's bounding box, brush width included, grown by `BOX_MARGIN` and
/// kept inside the image.
fn paint_box(paths: &[Path], frame: (f64, f64)) -> Option<[f64; 4]> {
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
    let margin = BOX_MARGIN * (b[2] - b[0]).max(b[3] - b[1]);
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
        let r = (path.radius * reach / cell).max(0.5);
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

/// The three ways a request is put to SAM.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ask {
    /// Points along the paint, inside a box around it. The usual answer.
    Boxed,
    /// The same points, no box: for paint that covers only part of the thing.
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
) -> Result<(GrayImage, Vec<(Ask, Fit, bool)>)> {
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
    let bx = paint_box(&paths, frame);

    let mut candidates = Vec::new();
    for ask in [Ask::Boxed, Ask::Open, Ask::BoxOnly] {
        let prompt = match ask {
            Ask::Boxed => Prompt::new(&positive, &negative, bx),
            Ask::Open => Prompt::new(&positive, &negative, None),
            Ask::BoxOnly => Prompt::new(&[], &negative, bx),
        };
        let answer = decode(decoder, embeddings, &prompt, None, small)?;
        let mut f = fit(
            &answer.mask,
            answer.width,
            answer.height,
            &include,
            &exclude,
            if ask == Ask::Open { None } else { bx },
        );
        f.confidence = answer.score as f64;
        candidates.push((ask, f, prompt, answer));
    }

    let fits: Vec<(Ask, Fit)> = candidates.iter().map(|(a, f, _, _)| (*a, *f)).collect();
    let chosen = pick(&fits);
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
/// The boxed answer stands unless there is evidence against it: its box cut
/// it off, so the thing carries on past the paint, or it runs over an
/// exclusion. Covering more of the paint is *not* such evidence. Rough paint
/// over a lamp is mostly the wall behind it, and the whole wall covers all of
/// it — AK's first test picked the wall exactly that way.
///
/// When the boxed answer is in doubt, every answer scores what it covers of
/// the paint, less twice what it covers of the exclusions, less how far its
/// box cut it off. One that takes most of the frame is answering a different
/// question. The boxed answer keeps the choice on a near-tie.
fn pick(fits: &[(Ask, Fit)]) -> Ask {
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
    let mask =
        segment(&models.sam_decoder, &embeddings, &strokes, &view).map_err(|e| e.to_string())?;
    let mask = follow_edges(&mask, warped.as_ref());

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
        let b = paint_box(&[p.clone()], (1024.0, 683.0)).unwrap();
        let margin = BOX_MARGIN * 120.0;
        assert!((b[0] - (90.0 - margin)).abs() < 1e-9);
        assert!((b[3] - (160.0 + margin)).abs() < 1e-9);

        let edge = Path {
            points: vec![(2.0, 2.0)],
            radius: 10.0,
            exclude: false,
        };
        let b = paint_box(&[edge], (1024.0, 683.0)).unwrap();
        assert_eq!((b[0], b[1]), (0.0, 0.0));
    }

    #[test]
    fn exclusions_do_not_count_towards_the_box() {
        let p = Path {
            points: vec![(500.0, 500.0)],
            radius: 4.0,
            exclude: true,
        };
        assert!(paint_box(&[p], (1024.0, 1024.0)).is_none());
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
            (Ask::Boxed, f(0.90, 0.0, 0.1, 0.2)),
            (Ask::Open, f(0.75, 0.0, 0.1, 0.0)),
            (Ask::BoxOnly, f(0.72, 0.0, 0.1, 0.0)),
        ];
        assert_eq!(pick(&fits), Ask::Boxed);
    }

    /// AK's lamp: rough paint over a see-through lamp is mostly wall, so the
    /// whole wall panel covers all of it. Covering more is not evidence; the
    /// lamp was not cut off by its box, so the lamp stands.
    #[test]
    fn covering_more_of_the_paint_does_not_displace_an_uncut_answer() {
        let fits = [
            (Ask::Boxed, f(0.60, 0.0, 0.03, 0.02)),
            (Ask::Open, f(1.0, 0.0, 0.35, 0.0)),
            (Ask::BoxOnly, f(0.62, 0.0, 0.03, 0.02)),
        ];
        assert_eq!(pick(&fits), Ask::Boxed);
    }

    #[test]
    fn running_over_an_exclusion_puts_the_boxed_answer_in_doubt() {
        let fits = [
            (Ask::Boxed, f(0.85, 0.4, 0.07, 0.03)),
            (Ask::Open, f(0.84, 0.4, 0.07, 0.0)),
            (Ask::BoxOnly, f(0.80, 0.0, 0.06, 0.03)),
        ];
        assert_eq!(pick(&fits), Ask::BoxOnly);
    }

    /// A line across a mug: the boxed answer is a band the width of the line,
    /// filling the box edge to edge. The open one is the mug.
    #[test]
    fn a_box_that_cut_the_object_off_loses() {
        let fits = [
            (Ask::Boxed, f(1.0, 0.0, 0.02, 0.5)),
            (Ask::Open, f(0.98, 0.0, 0.08, 0.0)),
            (Ask::BoxOnly, f(1.0, 0.0, 0.02, 0.5)),
        ];
        assert_eq!(pick(&fits), Ask::Open);
    }

    #[test]
    fn covering_the_exclusion_or_the_whole_frame_does_not_win() {
        let fits = [
            (Ask::Boxed, f(0.90, 0.0, 0.1, 0.2)),
            (Ask::Open, f(0.95, 0.5, 0.2, 0.0)),
            (Ask::BoxOnly, f(1.0, 0.0, 0.9, 0.0)),
        ];
        assert_eq!(pick(&fits), Ask::Boxed);
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
            let (mask, report) = segment_reporting(&decoder, &embeddings, &strokes, &view).unwrap();
            let mask = follow_edges(&mask, &image);
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
        }
    }
}
