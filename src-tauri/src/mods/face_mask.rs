//! The Face mask: every face in the photo, with the parts picked (face skin,
//! body skin, eyebrows, eye whites, iris, lips, teeth, nose, hair).
//!
//! From RapidRAW 1.6.7, `src-tauri/src/face_masking.rs` (07ef9211, with
//! 49beb431's smoother edges), copied below the glue whole but for three
//! things: the sessions are ours rather than shared behind their locks, the
//! analysis is keyed by `selection::picture_key` rather than a hash it
//! carries, and nothing a user can reach unwraps. Their finding of the faces
//! is kept too (ai_commands.rs `cached_face_analysis`): EfficientSAM3 with
//! their stored "face" prompt gives the boxes (`esam3::boxes`, threshold
//! `ESAM3_FACE_SCORE_THRESHOLD`), MediaPipe's face landmarks (478 points) and
//! selfie multiclass segmenter (hair, skin, body) draw the parts, and the
//! colour guided filter fits them to the photo's edges. MediaPipe's models are
//! Google's, Apache 2.0.
//!
//! The answer is stored as the Subject mask stores its own
//! (`selection::Selection`). The five files (226 MB as Windows counts it, the
//! 205 MB part shared with the Text mask) are fetched only when asked, after
//! saying their size, into the models folder under RapidRAW 1.6.7's names;
//! Settings > General > AI Models lists them as "Face masks". One analysis is
//! kept for the last photo and geometry, so picking another part only draws.

use std::f32::consts::FRAC_PI_2;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
#[cfg(test)]
use std::time::Instant;

use anyhow::Result;
use image::{DynamicImage, GenericImageView, GrayImage, ImageBuffer, Luma, RgbImage, imageops};
use ndarray::Array4;
use ort::session::Session;
use ort::value::Tensor;
use rayon::prelude::*;

use super::ai_session::IdleSlot;
use super::esam3::{self, Prompt, fast_color_guided_filter};
use super::model_catalog::{FACE_MASK_FILES, FACE_MASKS};
use super::model_download::{self, Progress, Status};
use super::selection::{Frame, Selection, png_data_url};

const LANDMARK_INPUT_SIZE: usize = 256;
const LANDMARK_COUNT: usize = 478;
const LANDMARK_MARGIN: f32 = 1.5;
const LANDMARK_PASSES: usize = 4;
const LANDMARK_CONVERGED_SCORE: f32 = 12.0;
const FACE_PRESENCE_THRESHOLD: f32 = 6.0;
const DUPLICATE_BOX_OVERLAP: f32 = 0.5;
const DUPLICATE_FACE_DISTANCE: f32 = 0.3;
const SEGMENTER_INPUT_SIZE: usize = 256;
const SEGMENTER_CLASSES: usize = 6;
const SEGMENTER_SPAN: f32 = 2.6;
const SEGMENTER_OFFSET: f32 = 0.2;
const SEGMENTER_OUTPUTS: [usize; 3] = [1, 3, 2];
const BODY_SPAN: f32 = 6.0;
const BODY_OFFSET: f32 = 2.0;
const BODY_WORK_SIZE: f32 = 1024.0;
const BODY_REFINE_DIVISOR: f32 = 8.0;
const BODY_REFINE_RADIUS: [f32; 2] = [2.0, 16.0];
const SEGMENT_LOW: f32 = 0.3;
const SEGMENT_HIGH: f32 = 0.7;
const CROP_MARGIN: f32 = 0.6;
const CROP_MARGIN_BOTTOM: f32 = 1.2;
const WORK_SIZE: f32 = 768.0;
const OVAL_EXPANSION: f32 = 1.12;
const OVAL_SOFTNESS: f32 = 0.03;
const EDGE_FADE: f32 = 0.03;
const REFINE_DIVISOR: f32 = 60.0;
const REFINE_EPS: f32 = 0.001;
const SUPERSAMPLING: usize = 4;

const RIGHT_EYE_CORNER: usize = 33;
const LEFT_EYE_CORNER: usize = 263;
const LIPS_OUTER: [usize; 20] = [
    61, 185, 40, 39, 37, 0, 267, 269, 270, 409, 291, 375, 321, 405, 314, 17, 84, 181, 91, 146,
];
const LIPS_INNER: [usize; 20] = [
    78, 191, 80, 81, 82, 13, 312, 311, 310, 415, 308, 324, 318, 402, 317, 14, 87, 178, 88, 95,
];
const LEFT_EYE: [usize; 16] = [
    263, 249, 390, 373, 374, 380, 381, 382, 362, 398, 384, 385, 386, 387, 388, 466,
];
const RIGHT_EYE: [usize; 16] = [
    33, 7, 163, 144, 145, 153, 154, 155, 133, 173, 157, 158, 159, 160, 161, 246,
];
const LEFT_BROW: [usize; 10] = [276, 283, 282, 295, 285, 336, 296, 334, 293, 300];
const RIGHT_BROW: [usize; 10] = [46, 53, 52, 65, 55, 107, 66, 105, 63, 70];
const NOSE: [usize; 26] = [
    168, 6, 197, 195, 5, 4, 1, 19, 94, 2, 98, 97, 326, 327, 294, 278, 344, 440, 275, 45, 220, 115,
    48, 64, 193, 417,
];
const FACE_OVAL: [usize; 36] = [
    10, 338, 297, 332, 284, 251, 389, 356, 454, 323, 361, 288, 397, 365, 379, 378, 400, 377, 152,
    148, 176, 149, 150, 136, 172, 58, 132, 93, 234, 127, 162, 21, 54, 103, 67, 109,
];
const LEFT_IRIS: [usize; 5] = [473, 474, 475, 476, 477];
const RIGHT_IRIS: [usize; 5] = [468, 469, 470, 471, 472];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FacePart {
    FaceSkin,
    BodySkin,
    Eyebrows,
    EyeWhites,
    Iris,
    Lips,
    Teeth,
    Nose,
    Hair,
}

impl FacePart {
    fn from_id(id: &str) -> Option<Self> {
        match id {
            "faceSkin" => Some(FacePart::FaceSkin),
            "bodySkin" => Some(FacePart::BodySkin),
            "eyebrows" => Some(FacePart::Eyebrows),
            "eyeWhites" => Some(FacePart::EyeWhites),
            "iris" => Some(FacePart::Iris),
            "lips" => Some(FacePart::Lips),
            "teeth" => Some(FacePart::Teeth),
            "nose" => Some(FacePart::Nose),
            "hair" => Some(FacePart::Hair),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
struct FaceFrame {
    center: [f32; 2],
    side: f32,
    angle: f32,
}

fn rotate([x, y]: [f32; 2], angle: f32) -> [f32; 2] {
    let (sin, cos) = angle.sin_cos();
    [cos * x - sin * y, sin * x + cos * y]
}

impl FaceFrame {
    fn to_image(self, size: usize, x: f32, y: f32) -> [f32; 2] {
        let k = self.side / size as f32;
        let half = size as f32 / 2.0;
        let [dx, dy] = rotate([(x - half) * k, (y - half) * k], self.angle);
        [self.center[0] + dx, self.center[1] + dy]
    }

    fn to_frame(self, size: usize, p: [f32; 2]) -> [f32; 2] {
        let k = size as f32 / self.side;
        let half = size as f32 / 2.0;
        let [dx, dy] = rotate([p[0] - self.center[0], p[1] - self.center[1]], -self.angle);
        [dx * k + half, dy * k + half]
    }
}

struct DetectedFace {
    landmarks: Vec<[f32; 2]>,
    center: [f32; 2],
    angle: f32,
    size: f32,
    segment_frame: FaceFrame,
    hair: Vec<f32>,
    skin: Vec<f32>,
}

struct FaceAnalysis {
    faces: Vec<DetectedFace>,
    body: Vec<(FaceFrame, Vec<f32>)>,
}

fn inside(x: f32, y: f32, width: usize, height: usize) -> bool {
    x >= -0.5 && y >= -0.5 && x <= width as f32 - 0.5 && y <= height as f32 - 0.5
}

fn bilinear(x: f32, y: f32, width: usize, height: usize) -> [(usize, f32); 4] {
    let x = x.clamp(0.0, (width - 1) as f32);
    let y = y.clamp(0.0, (height - 1) as f32);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    [
        (y0 * width + x0, (1.0 - fx) * (1.0 - fy)),
        (y0 * width + x1, fx * (1.0 - fy)),
        (y1 * width + x0, (1.0 - fx) * fy),
        (y1 * width + x1, fx * fy),
    ]
}

fn sample_rgb(image: &RgbImage, x: f32, y: f32, replicate: bool) -> [f32; 3] {
    let (w, h) = (image.width() as usize, image.height() as usize);
    if !replicate && !inside(x, y, w, h) {
        return [0.0; 3];
    }
    let taps = bilinear(x, y, w, h);
    let raw = image.as_raw();
    std::array::from_fn(|c| taps.iter().map(|&(i, k)| raw[i * 3 + c] as f32 * k).sum())
}

fn frame_tensor(
    image: &RgbImage,
    frame: &FaceFrame,
    size: usize,
    replicate: bool,
    normalize: impl Fn(f32) -> f32 + Sync,
) -> Array4<f32> {
    let mut tensor = Array4::<f32>::zeros((1, size, size, 3));
    let Some(values) = tensor.as_slice_mut() else {
        return tensor;
    };
    values
        .par_chunks_mut(size * 3)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..size {
                let [sx, sy] = frame.to_image(size, x as f32, y as f32);
                let rgb = sample_rgb(image, sx, sy, replicate);
                for c in 0..3 {
                    row[x * 3 + c] = normalize(rgb[c]);
                }
            }
        });
    tensor
}

fn run_landmarks(
    session: &mut Session,
    image: &RgbImage,
    frame: &FaceFrame,
) -> Result<(Vec<[f32; 2]>, f32)> {
    let tensor = frame_tensor(image, frame, LANDMARK_INPUT_SIZE, false, |v| v / 255.0);
    let outputs = session.run(ort::inputs![Tensor::from_array(tensor)?])?;
    let points = outputs[0].try_extract_array::<f32>()?;
    let score = outputs[1]
        .try_extract_array::<f32>()?
        .iter()
        .next()
        .copied()
        .unwrap_or(f32::MIN);
    let points: Vec<f32> = points.iter().copied().collect();
    let landmarks = (0..LANDMARK_COUNT)
        .map(|i| frame.to_image(LANDMARK_INPUT_SIZE, points[i * 3], points[i * 3 + 1]))
        .collect();
    Ok((landmarks, score))
}

fn upright_extent(landmarks: &[[f32; 2]], angle: f32) -> ([f32; 2], f32) {
    let (min, max) = bounds(landmarks);
    let center = [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0];
    let upright: Vec<[f32; 2]> = landmarks
        .iter()
        .map(|p| rotate([p[0] - center[0], p[1] - center[1]], -angle))
        .collect();
    let (lo, hi) = bounds(&upright);
    (center, (hi[0] - lo[0]).max(hi[1] - lo[1]))
}

fn bounds(points: &[[f32; 2]]) -> ([f32; 2], [f32; 2]) {
    points
        .iter()
        .fold(([f32::MAX; 2], [f32::MIN; 2]), |(min, max), p| {
            (
                [min[0].min(p[0]), min[1].min(p[1])],
                [max[0].max(p[0]), max[1].max(p[1])],
            )
        })
}

fn fit_landmarks(
    session: &mut Session,
    image: &RgbImage,
    face_box: [f32; 4],
    angle: f32,
) -> Result<(Vec<[f32; 2]>, f32, f32)> {
    let mut frame = FaceFrame {
        center: [
            (face_box[0] + face_box[2]) / 2.0,
            (face_box[1] + face_box[3]) / 2.0,
        ],
        side: (face_box[2] - face_box[0]).max(face_box[3] - face_box[1]) * LANDMARK_MARGIN,
        angle,
    };
    let (landmarks, score) = run_landmarks(session, image, &frame)?;
    let mut best = (landmarks, score, angle);
    let mut current = best.0.clone();
    for _ in 1..LANDMARK_PASSES {
        let (right, left) = (current[RIGHT_EYE_CORNER], current[LEFT_EYE_CORNER]);
        let angle = (left[1] - right[1]).atan2(left[0] - right[0]);
        let (center, extent) = upright_extent(&current, angle);
        frame = FaceFrame {
            center,
            side: extent * LANDMARK_MARGIN,
            angle,
        };
        let (landmarks, score) = run_landmarks(session, image, &frame)?;
        current = landmarks;
        if score > best.1 {
            best = (current.clone(), score, angle);
        }
        if score > LANDMARK_CONVERGED_SCORE {
            break;
        }
    }
    Ok(best)
}

fn run_segmenter(
    session: &mut Session,
    image: &RgbImage,
    frame: &FaceFrame,
) -> Result<[Vec<f32>; 3]> {
    let tensor = frame_tensor(image, frame, SEGMENTER_INPUT_SIZE, true, |v| {
        v / 127.5 - 1.0
    });
    let outputs = session.run(ort::inputs![Tensor::from_array(tensor)?])?;
    let logits = outputs[0].try_extract_array::<f32>()?;
    let logits = logits
        .as_slice()
        .ok_or_else(|| anyhow::anyhow!("Unexpected face segmentation output layout"))?;
    let mut probabilities: [Vec<f32>; 3] = Default::default();
    for classes in logits.as_chunks::<SEGMENTER_CLASSES>().0 {
        let max = classes.iter().fold(f32::MIN, |m, &v| m.max(v));
        let sum: f32 = classes.iter().map(|&v| (v - max).exp()).sum();
        for (out, &class) in probabilities.iter_mut().zip(&SEGMENTER_OUTPUTS) {
            out.push((classes[class] - max).exp() / sum);
        }
    }
    Ok(probabilities)
}

fn box_overlap(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let w = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let h = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let smaller = ((a[2] - a[0]) * (a[3] - a[1])).min((b[2] - b[0]) * (b[3] - b[1]));
    w * h / smaller.max(f32::EPSILON)
}

fn analyze_faces(
    image: &DynamicImage,
    detections: &[(f32, [f32; 4])],
    landmark_session: &mut Session,
    segmenter_session: &mut Session,
    base_angle: f32,
) -> Result<FaceAnalysis> {
    let (width, height) = image.dimensions();
    let mut boxes: Vec<[f32; 4]> = Vec::new();
    for (_, b) in detections {
        let b = [
            b[0] * width as f32,
            b[1] * height as f32,
            b[2] * width as f32,
            b[3] * height as f32,
        ];
        if !boxes
            .iter()
            .any(|kept| box_overlap(kept, &b) > DUPLICATE_BOX_OVERLAP)
        {
            boxes.push(b);
        }
    }
    let mut analysis = FaceAnalysis {
        faces: Vec::new(),
        body: Vec::new(),
    };
    if boxes.is_empty() {
        return Ok(analysis);
    }

    let rgb = image.to_rgb8();
    let faces = &mut analysis.faces;
    for face_box in boxes {
        let mut fitted = None;
        for quarter in 0..4 {
            let angle = base_angle + quarter as f32 * FRAC_PI_2;
            let (landmarks, score, angle) = fit_landmarks(landmark_session, &rgb, face_box, angle)?;
            if score >= FACE_PRESENCE_THRESHOLD {
                fitted = Some((landmarks, angle));
                break;
            }
        }
        let Some((landmarks, angle)) = fitted else {
            continue;
        };

        let (center, size) = upright_extent(&landmarks, angle);
        if faces.iter().any(|face| {
            (face.center[0] - center[0]).hypot(face.center[1] - center[1])
                < DUPLICATE_FACE_DISTANCE * size.max(face.size)
        }) {
            continue;
        }

        let [dx, dy] = rotate([0.0, SEGMENTER_OFFSET * size], angle);
        let segment_frame = FaceFrame {
            center: [center[0] + dx, center[1] + dy],
            side: size * SEGMENTER_SPAN,
            angle,
        };
        let [hair, skin, _] = run_segmenter(segmenter_session, &rgb, &segment_frame)?;
        let [dx, dy] = rotate([0.0, BODY_OFFSET * size], angle);
        let body_frame = FaceFrame {
            center: [center[0] + dx, center[1] + dy],
            side: size * BODY_SPAN,
            angle,
        };
        let [_, _, body] = run_segmenter(segmenter_session, &rgb, &body_frame)?;
        analysis.body.push((body_frame, body));
        faces.push(DetectedFace {
            landmarks,
            center,
            angle,
            size,
            segment_frame,
            hair,
            skin,
        });
    }
    if !analysis.faces.is_empty() {
        let frame = FaceFrame {
            center: [width as f32 / 2.0, height as f32 / 2.0],
            side: width.max(height) as f32,
            angle: base_angle,
        };
        let [_, _, body] = run_segmenter(segmenter_session, &rgb, &frame)?;
        analysis.body.push((frame, body));
    }
    Ok(analysis)
}

fn rasterize(
    width: usize,
    height: usize,
    (top, bottom): (f32, f32),
    spans: impl Fn(f32) -> Vec<[f32; 2]> + Sync,
) -> Vec<f32> {
    let mut mask = vec![0.0; width * height];
    let y0 = (top.floor().max(0.0) as usize).min(height);
    let y1 = (bottom.ceil().max(0.0) as usize + 1).min(height);
    let samples = (width * SUPERSAMPLING) as f32;
    let scale = SUPERSAMPLING as f32;
    mask.par_chunks_mut(width)
        .enumerate()
        .skip(y0)
        .take(y1.saturating_sub(y0))
        .for_each(|(y, row)| {
            for sy in 0..SUPERSAMPLING {
                for [a, b] in spans(y as f32 + (sy as f32 + 0.5) / scale - 0.5) {
                    let start = ((a + 0.5) * scale - 0.5).ceil().clamp(0.0, samples) as usize;
                    let end = ((b + 0.5) * scale - 0.5).ceil().clamp(0.0, samples) as usize;
                    for n in start..end {
                        row[n / SUPERSAMPLING] += 1.0 / (scale * scale);
                    }
                }
            }
        });
    mask
}

fn polygon_mask(width: usize, height: usize, polygon: &[[f32; 2]]) -> Vec<f32> {
    let (min, max) = bounds(polygon);
    let n = polygon.len();
    rasterize(width, height, (min[1], max[1]), |y| {
        let mut crossings: Vec<f32> = (0..n)
            .map(|i| (polygon[i], polygon[(i + n - 1) % n]))
            .filter(|(a, b)| (a[1] > y) != (b[1] > y))
            .map(|(a, b)| a[0] + (y - a[1]) * (b[0] - a[0]) / (b[1] - a[1]))
            .collect();
        crossings.sort_by(f32::total_cmp);
        crossings.as_chunks::<2>().0.to_vec()
    })
}

fn convex_hull(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut points = points.to_vec();
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut hull: Vec<[f32; 2]> = Vec::with_capacity(points.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let ordered: Box<dyn Iterator<Item = &[f32; 2]>> = if pass == 0 {
            Box::new(points.iter())
        } else {
            Box::new(points.iter().rev())
        };
        for &p in ordered {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
            {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

fn scaled_polygon(points: &[[f32; 2]], scale: f32) -> Vec<[f32; 2]> {
    let n = points.len() as f32;
    let cx = points.iter().map(|p| p[0]).sum::<f32>() / n;
    let cy = points.iter().map(|p| p[1]).sum::<f32>() / n;
    points
        .iter()
        .map(|p| [cx + (p[0] - cx) * scale, cy + (p[1] - cy) * scale])
        .collect()
}

fn disc_mask(width: usize, height: usize, landmarks: &[[f32; 2]], iris: &[usize]) -> Vec<f32> {
    let c = landmarks[iris[0]];
    let r = iris[1..]
        .iter()
        .map(|&i| (landmarks[i][0] - c[0]).hypot(landmarks[i][1] - c[1]))
        .sum::<f32>()
        / (iris.len() - 1) as f32;
    rasterize(width, height, (c[1] - r, c[1] + r), |y| {
        let half = (r * r - (y - c[1]) * (y - c[1])).max(0.0).sqrt();
        vec![[c[0] - half, c[0] + half]]
    })
}

fn to_gray(values: &[f32], width: usize, height: usize) -> GrayImage {
    GrayImage::from_raw(
        width as u32,
        height as u32,
        values
            .iter()
            .map(|&v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect(),
    )
    .unwrap_or_default()
}

fn from_gray(image: &GrayImage) -> Vec<f32> {
    image.as_raw().iter().map(|&v| v as f32 / 255.0).collect()
}

fn refine(guide: &RgbImage, mask: &[f32], radius: usize) -> Vec<f32> {
    let (width, height) = guide.dimensions();
    from_gray(&fast_color_guided_filter(
        guide,
        &to_gray(mask, width as usize, height as usize),
        radius.max(1),
        REFINE_EPS,
    ))
}

fn sharpen(probabilities: &[f32]) -> Vec<f32> {
    probabilities
        .iter()
        .map(|&p| (p - SEGMENT_LOW) / (SEGMENT_HIGH - SEGMENT_LOW))
        .collect()
}

fn maximum(a: &mut [f32], b: &[f32]) {
    a.iter_mut().zip(b).for_each(|(a, &b)| *a = a.max(b));
}

fn median(mut values: Vec<f32>) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    let mid = values.len() / 2;
    values.select_nth_unstable_by(mid, f32::total_cmp);
    Some(values[mid])
}

fn teeth_mask(rgb: &RgbImage, luma: &[f32], lips: &[f32], mouth: &[f32]) -> Vec<f32> {
    let saturation: Vec<f32> = rgb
        .pixels()
        .map(|p| {
            let max = p.0.iter().copied().max().unwrap_or(0) as f32;
            let min = p.0.iter().copied().min().unwrap_or(0) as f32;
            if max > 0.0 { (max - min) / max } else { 0.0 }
        })
        .collect();
    let lip_pixels = || {
        lips.iter()
            .enumerate()
            .filter(|&(_, &v)| v > 0.5)
            .map(|(i, _)| i)
    };
    let (Some(lip_luma), Some(lip_saturation)) = (
        median(lip_pixels().map(|i| luma[i]).collect()),
        median(lip_pixels().map(|i| saturation[i]).collect()),
    ) else {
        return vec![0.0; mouth.len()];
    };
    mouth
        .iter()
        .zip(luma)
        .zip(&saturation)
        .map(|((&m, &l), &s)| {
            let bright = ((l - lip_luma - 0.04) / 0.12).clamp(0.0, 1.0);
            let pale = ((lip_saturation * 0.9 + 0.05 - s) / 0.15).clamp(0.0, 1.0);
            m * bright * pale
        })
        .collect()
}

fn face_region(face: &DetectedFace, width: u32, height: u32) -> (u32, u32, u32, u32) {
    let near = (0.5 + CROP_MARGIN) * face.size;
    let far = (0.5 + CROP_MARGIN_BOTTOM) * face.size;
    let corners: Vec<[f32; 2]> = [[-near, -near], [near, -near], [-near, far], [near, far]]
        .into_iter()
        .map(|corner| {
            let [dx, dy] = rotate(corner, face.angle);
            [face.center[0] + dx, face.center[1] + dy]
        })
        .collect();
    let (min, max) = bounds(&corners);
    (
        min[0].floor().clamp(0.0, width as f32) as u32,
        min[1].floor().clamp(0.0, height as f32) as u32,
        max[0].ceil().clamp(0.0, width as f32) as u32,
        max[1].ceil().clamp(0.0, height as f32) as u32,
    )
}

fn face_parts_mask(
    image: &DynamicImage,
    face: &DetectedFace,
    parts: &[FacePart],
    region: (u32, u32, u32, u32),
) -> GrayImage {
    let (x0, y0, x1, y1) = region;
    let (crop_w, crop_h) = (x1 - x0, y1 - y0);
    let scale = (WORK_SIZE / crop_w.max(crop_h) as f32).min(1.0);
    let w = ((crop_w as f32 * scale).round() as usize).max(1);
    let h = ((crop_h as f32 * scale).round() as usize).max(1);

    let crop = image.crop_imm(x0, y0, crop_w, crop_h);
    let guide_full = crop.to_rgb8();
    let work = crop.resize_exact(w as u32, h as u32, imageops::FilterType::Triangle);
    let work_rgb = work.to_rgb8();
    let luma = from_gray(&work.to_luma8());

    let points: Vec<[f32; 2]> = face
        .landmarks
        .iter()
        .map(|p| [(p[0] - x0 as f32) * scale, (p[1] - y0 as f32) * scale])
        .collect();
    let select = |indices: &[usize]| indices.iter().map(|&i| points[i]).collect::<Vec<_>>();
    let size = face.size * scale;
    let radius = (size / REFINE_DIVISOR).round().max(1.0) as usize;
    let fine_radius = (radius / 2).max(1);

    let pair = |mut left: Vec<f32>, right: &[f32]| {
        maximum(&mut left, right);
        left
    };
    let polygons = |a: &[usize], b: &[usize]| {
        pair(
            polygon_mask(w, h, &select(a)),
            &polygon_mask(w, h, &select(b)),
        )
    };
    let eyes = polygons(&LEFT_EYE, &RIGHT_EYE);
    let brows = polygons(&LEFT_BROW, &RIGHT_BROW);
    let mut iris = pair(
        disc_mask(w, h, &points, &LEFT_IRIS),
        &disc_mask(w, h, &points, &RIGHT_IRIS),
    );
    iris.iter_mut().zip(&eyes).for_each(|(i, &e)| *i *= e);
    let lips_outer = polygon_mask(w, h, &select(&LIPS_OUTER));
    let mouth = polygon_mask(w, h, &select(&LIPS_INNER));
    let lips: Vec<f32> = lips_outer
        .iter()
        .zip(&mouth)
        .map(|(&o, &i)| (o - i).max(0.0))
        .collect();

    let mut combined = vec![0.0; w * h];
    for &part in parts {
        let mask = match part {
            FacePart::EyeWhites => {
                let refined = refine(&work_rgb, &eyes, fine_radius);
                let iris = refine(&work_rgb, &iris, fine_radius);
                refined
                    .iter()
                    .zip(&iris)
                    .map(|(&e, &i)| (e - i).max(0.0))
                    .collect()
            }
            FacePart::Iris => refine(&work_rgb, &iris, fine_radius),
            FacePart::Eyebrows => refine(&work_rgb, &brows, radius),
            FacePart::Lips => refine(&work_rgb, &lips, radius),
            FacePart::Teeth => {
                let inner = polygon_mask(w, h, &scaled_polygon(&select(&LIPS_INNER), 0.95));
                let teeth = teeth_mask(&work_rgb, &luma, &lips, &inner);
                refine(&work_rgb, &teeth, fine_radius)
                    .iter()
                    .zip(&inner)
                    .map(|(&t, &m)| t * m)
                    .collect()
            }
            FacePart::Nose => refine(
                &work_rgb,
                &polygon_mask(w, h, &convex_hull(&select(&NOSE))),
                radius * 2,
            ),
            FacePart::BodySkin => continue,
            FacePart::Hair => refine(
                &work_rgb,
                &sharpen(&segment_probabilities(
                    &face.segment_frame,
                    &face.hair,
                    w,
                    h,
                    x0,
                    y0,
                    scale,
                )),
                radius * 2,
            ),
            FacePart::FaceSkin => {
                let oval = scaled_polygon(&select(&FACE_OVAL), OVAL_EXPANSION);
                let oval = ImageBuffer::<Luma<f32>, Vec<f32>>::from_raw(
                    w as u32,
                    h as u32,
                    polygon_mask(w, h, &oval),
                )
                .unwrap_or_default();
                let oval = imageops::blur(&oval, (size * OVAL_SOFTNESS).max(0.5)).into_raw();
                let skin: Vec<f32> =
                    segment_probabilities(&face.segment_frame, &face.skin, w, h, x0, y0, scale)
                        .iter()
                        .zip(&oval)
                        .map(|(&s, &o)| s * o)
                        .collect();
                let features = refine(
                    &work_rgb,
                    &pair(pair(brows.clone(), &eyes), &lips_outer),
                    fine_radius,
                );
                refine(&work_rgb, &sharpen(&skin), radius * 2)
                    .iter()
                    .zip(&features)
                    .map(|(&s, &f)| s * (1.0 - f))
                    .collect()
            }
        };
        maximum(&mut combined, &mask);
    }

    let fade = EDGE_FADE * size;
    let fades = [x0 > 0, y0 > 0, x1 < image.width(), y1 < image.height()];
    combined.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, v) in row.iter_mut().enumerate() {
            let distances = [x as f32, y as f32, (w - 1 - x) as f32, (h - 1 - y) as f32];
            let edge = distances
                .iter()
                .zip(fades)
                .filter(|&(_, faded)| faded)
                .fold(1.0f32, |acc, (&d, _)| acc.min(d / fade.max(1.0)));
            *v *= edge.min(1.0);
        }
    });

    let combined = to_gray(&combined, w, h);
    if (w as u32, h as u32) == (crop_w, crop_h) {
        combined
    } else {
        fast_color_guided_filter(&guide_full, &combined, fine_radius, REFINE_EPS)
    }
}

fn segment_probabilities(
    frame: &FaceFrame,
    grid: &[f32],
    width: usize,
    height: usize,
    x0: u32,
    y0: u32,
    scale: f32,
) -> Vec<f32> {
    let mut values = vec![0.0; width * height];
    values
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, out) in row.iter_mut().enumerate() {
                let p = [x0 as f32 + x as f32 / scale, y0 as f32 + y as f32 / scale];
                let [u, v] = frame.to_frame(SEGMENTER_INPUT_SIZE, p);
                if inside(u, v, SEGMENTER_INPUT_SIZE, SEGMENTER_INPUT_SIZE) {
                    *out = bilinear(u, v, SEGMENTER_INPUT_SIZE, SEGMENTER_INPUT_SIZE)
                        .iter()
                        .map(|&(i, k)| grid[i] * k)
                        .sum();
                }
            }
        });
    values
}

fn compose_face_mask(
    image: &DynamicImage,
    analysis: &FaceAnalysis,
    parts: &[FacePart],
) -> GrayImage {
    let (width, height) = image.dimensions();
    let mut mask = GrayImage::new(width, height);
    if parts.contains(&FacePart::BodySkin) {
        let scale = (BODY_WORK_SIZE / width.max(height) as f32).min(1.0);
        let w = ((width as f32 * scale).round() as usize).max(1);
        let h = ((height as f32 * scale).round() as usize).max(1);
        let mut body = vec![0.0; w * h];
        for (frame, grid) in &analysis.body {
            maximum(
                &mut body,
                &segment_probabilities(frame, grid, w, h, 0, 0, scale),
            );
        }
        let smallest_face = analysis
            .faces
            .iter()
            .map(|face| face.size)
            .fold(f32::MAX, f32::min);
        let radius = (smallest_face * scale / BODY_REFINE_DIVISOR)
            .clamp(BODY_REFINE_RADIUS[0], BODY_REFINE_RADIUS[1]);
        mask = fast_color_guided_filter(
            &image.to_rgb8(),
            &to_gray(&sharpen(&body), w, h),
            radius.round() as usize,
            REFINE_EPS,
        );
    }
    if parts.iter().all(|&part| part == FacePart::BodySkin) {
        return mask;
    }
    let regions: Vec<_> = analysis
        .faces
        .iter()
        .map(|face| face_region(face, width, height))
        .collect();
    let face_masks: Vec<GrayImage> = analysis
        .faces
        .par_iter()
        .zip(&regions)
        .map(|(face, &region)| face_parts_mask(image, face, parts, region))
        .collect();
    for (face_mask, &(x0, y0, _, _)) in face_masks.iter().zip(&regions) {
        for (x, y, pixel) in face_mask.enumerate_pixels() {
            let out = mask.get_pixel_mut(x0 + x, y0 + y);
            out[0] = out[0].max(pixel[0]);
        }
    }
    mask
}

// ---------------------------------------------------------------- Argentum

/// RapidRAW's `ESAM3_FACE_SCORE_THRESHOLD`.
const FACE_SCORE_THRESHOLD: f32 = 0.3;

const FACE_PROMPT: &str = "efficientsam3_face_prompt.bin";
const LANDMARKS: &str = "mediapipe_face_landmarks.onnx";
const SEGMENTER: &str = "mediapipe_selfie_multiclass.onnx";

/// Whether the files are there, and how much is left to fetch. By size alone:
/// cheap enough to ask before every mask.
pub fn status(dir: &Path) -> Status {
    model_download::status(dir, FACE_MASK_FILES)
}

/// The folder last found whole (`model_download::verify`).
static VERIFIED: Mutex<Option<PathBuf>> = Mutex::new(None);

fn verify(dir: &Path) -> Result<(), String> {
    model_download::verify(dir, FACE_MASK_FILES, &VERIFIED)
}

/// Fetch whatever is missing or wrong into the models folder `dir`, then check all five.
pub async fn download(dir: &Path, progress: impl Fn(&Progress)) -> Result<(), String> {
    model_download::download(dir, FACE_MASKS, progress).await?;
    *VERIFIED.lock().map_err(|e| e.to_string())? = None;
    verify(dir)
}

/// MediaPipe's two models and the stored prompt; the image encoder and decoder
/// are `esam3`'s.
struct Loaded {
    landmarks: Session,
    segmenter: Session,
    prompt: Prompt,
    analysis: Option<(u64, FaceAnalysis)>,
}

static LOADED: IdleSlot<Loaded> = IdleSlot::new("text and face mask models");

fn load(dir: &Path) -> Result<Loaded, String> {
    let session = |name: &str| esam3::load(&dir.join(name)).map_err(|e| format!("{name}: {e}"));
    Ok(Loaded {
        landmarks: session(LANDMARKS)?,
        segmenter: session(SEGMENTER)?,
        prompt: Prompt::from_file(&dir.join(FACE_PROMPT))
            .map_err(|e| format!("{FACE_PROMPT}: {e}"))?,
        analysis: None,
    })
}

/// Draw `parts` (their ids, as the Masks panel names them) of every face in
/// `picture` (the warped working image), with the models in the models folder
/// `dir`. `key` names the photo and its geometry.
pub fn select(
    dir: &Path,
    picture: &Arc<DynamicImage>,
    key: u64,
    parts: &[String],
    frame: Frame,
) -> Result<Selection, String> {
    verify(dir)?;
    let parts: Vec<FacePart> = parts.iter().filter_map(|p| FacePart::from_id(p)).collect();
    LOADED.with(
        dir,
        || load(dir),
        |loaded| find_and_draw(loaded, dir, picture, key, &parts, frame),
    )
}

/// The faces of `picture` (found first unless `loaded` has them for `key`),
/// drawn as `parts`.
fn find_and_draw(
    loaded: &mut Loaded,
    dir: &Path,
    picture: &Arc<DynamicImage>,
    key: u64,
    parts: &[FacePart],
    frame: Frame,
) -> Result<Selection, String> {
    if loaded.analysis.as_ref().map(|(k, _)| *k) != Some(key) {
        loaded.analysis = None;
        let prompt = &loaded.prompt;
        let detections = esam3::with_picture(dir, picture, key, |_, decoder, features| {
            esam3::boxes(decoder, features, prompt, FACE_SCORE_THRESHOLD)
        })?;
        // Their base angle: a photo turned a quarter is searched turned back.
        let base_angle = -(frame.orientation_steps as f32) * FRAC_PI_2;
        let analysis = analyze_faces(
            picture,
            &detections,
            &mut loaded.landmarks,
            &mut loaded.segmenter,
            base_angle,
        )
        .map_err(|e| e.to_string())?;
        loaded.analysis = Some((key, analysis));
    }
    let (_, analysis) = loaded.analysis.as_ref().ok_or("the faces were not found")?;

    let found = !analysis.faces.is_empty();
    let mask = if found && !parts.is_empty() {
        compose_face_mask(picture, analysis, parts)
    } else {
        // An empty mask, as their AI masks store one (1 x 1).
        GrayImage::new(1, 1)
    };
    Ok(Selection {
        mask_data_base64: png_data_url(&mask)?,
        rotation: frame.rotation,
        flip_horizontal: frame.flip_horizontal,
        flip_vertical: frame.flip_vertical,
        orientation_steps: frame.orientation_steps,
        found,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ids the Masks panel sends are the nine parts, and nothing else.
    #[test]
    fn the_panel_names_every_part() {
        let ids = [
            "faceSkin",
            "bodySkin",
            "eyebrows",
            "eyeWhites",
            "iris",
            "lips",
            "teeth",
            "nose",
            "hair",
        ];
        for id in ids {
            assert!(FacePart::from_id(id).is_some(), "{id}");
        }
        assert!(FacePart::from_id("ears").is_none());
    }

    /// The five files, 237 MB, by the names the loaders open.
    #[test]
    fn an_empty_folder_needs_every_file() {
        let dir = std::env::temp_dir().join(format!("ag-face-mask-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let status = status(&dir);
        assert!(!status.installed);
        assert_eq!(status.missing_bytes, 236_687_121);
        let names: Vec<&str> = FACE_MASK_FILES.iter().map(|f| f.name).collect();
        assert_eq!(names[2..], [FACE_PROMPT, LANDMARKS, SEGMENTER]);
        assert!(verify(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A polygon covers its inside and only that, and a hull wraps its points.
    #[test]
    fn shapes_cover_their_inside() {
        let square = [[2.0, 2.0], [8.0, 2.0], [8.0, 8.0], [2.0, 8.0]];
        let mask = polygon_mask(10, 10, &square);
        assert!(mask[5 * 10 + 5] > 0.99);
        assert!(mask[10 + 1] < 0.01);
        let hull = convex_hull(&[[0.0, 0.0], [4.0, 0.0], [2.0, 1.0], [4.0, 4.0], [0.0, 4.0]]);
        assert_eq!(hull.len(), 4);
    }

    /// The real models on real photos, to see the masks and time them: set
    /// AG_FACE_MODELS to a folder holding the five files, AG_FACE_PHOTOS to RAW
    /// files (';' between them), AG_FACE_PARTS to groups of part ids (';'
    /// between groups, ',' within one), AG_OUT to a folder, and ORT_DYLIB_PATH
    /// to onnxruntime.dll, then run with --ignored. Each photo's embedded
    /// preview gets `<n>-<parts>.png`, the mask in red over the photo.
    #[test]
    #[ignore = "needs the models, a runtime and photos"]
    fn parts_are_drawn_on_real_photos() {
        let dir = PathBuf::from(std::env::var("AG_FACE_MODELS").unwrap());
        let out = PathBuf::from(std::env::var("AG_OUT").unwrap());
        std::fs::create_dir_all(&out).unwrap();
        let groups = std::env::var("AG_FACE_PARTS").unwrap();
        let frame = Frame {
            rotation: 0.0,
            flip_horizontal: false,
            flip_vertical: false,
            orientation_steps: 0,
        };
        let photos = std::env::var("AG_FACE_PHOTOS").unwrap();
        for (n, photo) in photos.split(';').filter(|p| !p.is_empty()).enumerate() {
            let bytes = std::fs::read(photo).unwrap();
            let picture =
                Arc::new(crate::raw_processing::extract_embedded_preview(&bytes).unwrap());
            for group in groups.split(';').filter(|g| !g.is_empty()) {
                let parts: Vec<String> = group.split(',').map(String::from).collect();
                let started = Instant::now();
                let selection = select(&dir, &picture, n as u64, &parts, frame).unwrap();
                println!(
                    "{n} {group}: faces {} in {:.2} s ({}x{})",
                    selection.found,
                    started.elapsed().as_secs_f32(),
                    picture.width(),
                    picture.height()
                );
                let b64 = selection.mask_data_base64.split(',').nth(1).unwrap();
                let png = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
                    .unwrap();
                let mask = image::load_from_memory(&png).unwrap().to_luma8();
                let mask = imageops::resize(
                    &mask,
                    picture.width(),
                    picture.height(),
                    imageops::FilterType::Triangle,
                );
                let mut shown = picture.to_rgb8();
                for (p, m) in shown.pixels_mut().zip(mask.pixels()) {
                    let a = m[0] as f32 / 255.0 * 0.6;
                    p[0] = (p[0] as f32 * (1.0 - a) + 255.0 * a) as u8;
                    p[1] = (p[1] as f32 * (1.0 - a)) as u8;
                    p[2] = (p[2] as f32 * (1.0 - a)) as u8;
                }
                let small = imageops::resize(
                    &shown,
                    1200,
                    1200 * shown.height() / shown.width(),
                    imageops::FilterType::Triangle,
                );
                small
                    .save(out.join(format!("{n}-{}.png", group.replace(',', "+"))))
                    .unwrap();
            }
        }
    }
}
