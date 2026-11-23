//! EfficientSAM3, the promptable mask model behind RapidRAW 1.6.7's Text mask:
//! its image encoder, its decoder, and the colour guided filter that fits the
//! decoder's coarse answer to the photo's edges.
//!
//! Copied from RapidRAW 1.6.7, `src-tauri/src/ai_processing.rs` (211ef559,
//! with ebe1d4a2's edges and zoom): `run_esam3_image_encoder`,
//! `run_esam3_queries`, `run_esam3_decoder`, `run_esam3_zoomed_decoder`,
//! `fast_color_guided_filter`, `zoom_region`, `mask_iou` and `blend_feathered`,
//! with their constants. The model is EfficientSAM3 EV-M
//! (github.com/SimonZeng7108/efficientsam3, Apache 2.0), distilled from Meta's
//! SAM 3. Their sessions run on the graphics card when it is allowed; ours run
//! on the CPU, built as every model of theirs here is (`ai_session`).
//!
//! What asks the question is the caller's: `text_mask` turns typed words into
//! a `Prompt`, `face_mask` reads RapidRAW's stored "face" prompt and asks for
//! boxes (`run_esam3_boxes`). The encoder and decoder are loaded once for
//! both, with the last picture encoded, and let go when idle.

use super::ai_session::IdleSlot;
use std::path::Path;

use anyhow::{Result, anyhow};
use image::imageops::{self, FilterType};
use image::{DynamicImage, GenericImageView, GrayImage, Luma, RgbImage};
use ndarray::{Array, Array4, ArrayD, ArrayViewD, IxDyn, s};
use ort::session::Session;
use ort::value::{Tensor, TensorRef};
use rayon::prelude::*;

use crate::ai_processing::box_filter_opt;

const INPUT_SIZE: u32 = 1008;
const INPUT_PADDING: u32 = 30;
const EXEMPLAR_SLOTS: usize = 8;
const MASK_LOW: f32 = 0.3;
const MASK_HIGH: f32 = 0.7;
const REFINE_SIZE: u32 = 1024;
const REFINE_RADIUS: usize = 16;
const ZOOM_MIN_IOU: f32 = 0.3;
const ZOOM_MARGIN: f32 = 0.15;
const ZOOM_MAX_FRACTION: f32 = 0.6;
const ZOOM_MIN_SIDE: f32 = 1024.0;
const ZOOM_FEATHER: f32 = 0.1;

/// RapidRAW's `ESAM3_PROMPT_TOKENS` and `ESAM3_EMBED_DIM`: a prompt's shape.
pub const PROMPT_TOKENS: usize = 16;
const EMBED_DIM: usize = 256;

const IMAGE_ENCODER: &str = "efficientsam3_ev_m_image_encoder.onnx";
const DECODER: &str = "efficientsam3_ev_m_decoder.onnx";

/// The picture, encoded once: every prompt on it is answered from these.
pub struct Features {
    fpn: [ArrayD<f32>; 3],
    pos: ArrayD<f32>,
}

/// What to look for, as the decoder takes it.
pub struct Prompt {
    pub features: ArrayD<f32>,
    pub padding: ArrayD<f32>,
}

impl Prompt {
    /// RapidRAW's `stored_prompt`: a prompt they worked out once and saved,
    /// as little-endian floats, the features and then the padding.
    pub fn from_file(path: &Path) -> Result<Prompt> {
        let bytes = std::fs::read(path)?;
        let values: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&chunk| f32::from_le_bytes(chunk))
            .collect();
        let feature_len = PROMPT_TOKENS * EMBED_DIM;
        if values.len() != feature_len + PROMPT_TOKENS {
            return Err(anyhow!("Invalid prompt embedding {}", path.display()));
        }
        Ok(Prompt {
            features: Array::from_shape_vec(
                IxDyn(&[PROMPT_TOKENS, 1, EMBED_DIM]),
                values[..feature_len].to_vec(),
            )?,
            padding: Array::from_shape_vec(
                IxDyn(&[1, PROMPT_TOKENS]),
                values[feature_len..].to_vec(),
            )?,
        })
    }
}

/// A session for one of the models' files.
pub fn load(path: &Path) -> Result<Session> {
    Ok(super::ai_session::session_builder()?.commit_from_file(path)?)
}

/// The encoder and decoder, with the last picture encoded.
pub struct Sessions {
    pub encoder: Session,
    pub decoder: Session,
    features: Option<(u64, Features)>,
}

static LOADED: IdleSlot<Sessions> = IdleSlot::new("text and face mask models");

/// Run `work` with the encoder, the decoder and `picture`'s encoding, loading
/// them from the models folder `dir` and encoding the picture first when they
/// are not already there. `key` names the photo and its geometry
/// (`selection::picture_key`).
pub fn with_picture<T>(
    dir: &Path,
    picture: &DynamicImage,
    key: u64,
    work: impl FnOnce(&mut Session, &mut Session, &Features) -> Result<T>,
) -> Result<T, String> {
    LOADED.with(
        dir,
        || {
            let session = |name: &str| load(&dir.join(name)).map_err(|e| format!("{name}: {e}"));
            Ok(Sessions {
                encoder: session(IMAGE_ENCODER)?,
                decoder: session(DECODER)?,
                features: None,
            })
        },
        |sessions| {
            if sessions.features.as_ref().map(|(k, _)| *k) != Some(key) {
                sessions.features = None;
                let features = encode(picture, &mut sessions.encoder).map_err(|e| e.to_string())?;
                sessions.features = Some((key, features));
            }
            let Sessions {
                encoder,
                decoder,
                features,
            } = sessions;
            let (_, features) = features.as_ref().ok_or("the picture was not encoded")?;
            work(encoder, decoder, features).map_err(|e| e.to_string())
        },
    )
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

/// RapidRAW's `run_esam3_image_encoder`.
pub fn encode(image: &DynamicImage, encoder: &mut Session) -> Result<Features> {
    let size = INPUT_SIZE as usize;
    let padding = INPUT_PADDING as i64;
    let inner = INPUT_SIZE - 2 * INPUT_PADDING;
    let resized_rgb = image
        .resize_exact(inner, inner, FilterType::Triangle)
        .into_rgb8();

    let reflect = |i: usize| -> u32 {
        let (i, n) = (i as i64 - padding, inner as i64);
        let i = if i < 0 { -i } else { i };
        (if i >= n { 2 * n - 2 - i } else { i }) as u32
    };
    let mut input_tensor = Array4::<f32>::zeros((1, 3, size, size));
    for y in 0..size {
        let src_y = reflect(y);
        for x in 0..size {
            let pixel = resized_rgb.get_pixel(reflect(x), src_y);
            for c in 0..3 {
                input_tensor[[0, c, y, x]] = pixel[c] as f32 / 127.5 - 1.0;
            }
        }
    }

    let outputs = encoder.run(ort::inputs!["image" => Tensor::from_array(input_tensor)?])?;
    let extract = |name: &str| -> Result<ArrayD<f32>> {
        Ok(outputs[name].try_extract_array::<f32>()?.to_owned())
    };
    Ok(Features {
        fpn: [extract("fpn_0")?, extract("fpn_1")?, extract("fpn_2")?],
        pos: extract("pos_2")?,
    })
}

/// RapidRAW's `run_esam3_queries`: each query's score, box and mask.
fn queries<T>(
    decoder: &mut Session,
    features: &Features,
    prompt: &Prompt,
    read: impl FnOnce(Vec<f32>, ArrayViewD<f32>, ArrayViewD<f32>) -> T,
) -> Result<T> {
    let exemplar_boxes = Array::<f32, _>::zeros((EXEMPLAR_SLOTS, 1, 4));
    let exemplar_labels = Array::<i64, _>::zeros((EXEMPLAR_SLOTS, 1));
    let exemplar_mask = Array::<f32, _>::ones((1, EXEMPLAR_SLOTS));

    let outputs = decoder.run(ort::inputs![
        "fpn_0" => TensorRef::from_array_view(&features.fpn[0])?,
        "fpn_1" => TensorRef::from_array_view(&features.fpn[1])?,
        "fpn_2" => TensorRef::from_array_view(&features.fpn[2])?,
        "pos_2" => TensorRef::from_array_view(&features.pos)?,
        "language_features" => TensorRef::from_array_view(&prompt.features)?,
        "language_padding" => TensorRef::from_array_view(&prompt.padding)?,
        "exemplar_boxes" => Tensor::from_array(exemplar_boxes)?,
        "exemplar_labels" => Tensor::from_array(exemplar_labels)?,
        "exemplar_mask" => Tensor::from_array(exemplar_mask)?,
    ])?;

    let presence = outputs["presence_logit"]
        .try_extract_array::<f32>()?
        .iter()
        .next()
        .map_or(0.0, |&v| sigmoid(v));
    let scores = outputs["pred_logits"]
        .try_extract_array::<f32>()?
        .iter()
        .map(|&logit| sigmoid(logit) * presence)
        .collect();
    Ok(read(
        scores,
        outputs["pred_boxes"].try_extract_array::<f32>()?,
        outputs["pred_masks"].try_extract_array::<f32>()?,
    ))
}

/// RapidRAW's `run_esam3_boxes`: what scores above `score_threshold`, best
/// first, each as `[left, top, right, bottom]` in fractions of the picture.
pub fn boxes(
    decoder: &mut Session,
    features: &Features,
    prompt: &Prompt,
    score_threshold: f32,
) -> Result<Vec<(f32, [f32; 4])>> {
    let mut detections = queries(decoder, features, prompt, |scores, boxes, _| {
        scores
            .iter()
            .enumerate()
            .filter(|&(_, &score)| score > score_threshold)
            .map(|(query, &score)| {
                let size = INPUT_SIZE as f32;
                let padding = INPUT_PADDING as f32;
                let inner = size - 2.0 * padding;
                let [cx, cy, w, h]: [f32; 4] = std::array::from_fn(|i| {
                    let v = boxes[[0, query, i]] * size;
                    if i < 2 {
                        (v - padding) / inner
                    } else {
                        v / inner
                    }
                });
                (
                    score,
                    [cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0],
                )
            })
            .collect::<Vec<_>>()
    })?;
    detections.sort_by(|a, b| b.0.total_cmp(&a.0));
    Ok(detections)
}

/// RapidRAW's `run_esam3_decoder`: every query above `score_threshold`, joined,
/// fitted to the picture's edges at the picture's size. None when nothing
/// scores high enough.
fn decode(
    decoder: &mut Session,
    features: &Features,
    prompt: &Prompt,
    image: &DynamicImage,
    score_threshold: f32,
    refine_eps: f32,
) -> Result<Option<GrayImage>> {
    let (coarse_mask, mask_w, mask_h) = queries(decoder, features, prompt, |scores, _, masks| {
        let (mask_h, mask_w) = (masks.shape()[2], masks.shape()[3]);
        let mut coarse_mask: Option<Vec<f32>> = None;
        for (query, &score) in scores.iter().enumerate() {
            if score <= score_threshold {
                continue;
            }
            let combined = coarse_mask.get_or_insert_with(|| vec![0.0; mask_h * mask_w]);
            let query_mask = masks.slice(s![0, query, .., ..]);
            for (out, &value) in combined.iter_mut().zip(query_mask.iter()) {
                *out = out.max(sigmoid(value));
            }
        }
        (coarse_mask, mask_w, mask_h)
    })?;

    let Some(coarse_mask) = coarse_mask else {
        return Ok(None);
    };

    let coarse_mask = GrayImage::from_raw(
        mask_w as u32,
        mask_h as u32,
        coarse_mask
            .iter()
            .map(|&v| {
                let v = ((v - MASK_LOW) / (MASK_HIGH - MASK_LOW)).clamp(0.0, 1.0);
                (v * 255.0).round() as u8
            })
            .collect(),
    )
    .ok_or_else(|| anyhow!("Failed to create mask from EfficientSAM3 output"))?;

    let input_mask = imageops::resize(&coarse_mask, INPUT_SIZE, INPUT_SIZE, FilterType::Triangle);
    let inner = INPUT_SIZE - 2 * INPUT_PADDING;
    let image_mask =
        imageops::crop_imm(&input_mask, INPUT_PADDING, INPUT_PADDING, inner, inner).to_image();

    let (orig_width, orig_height) = image.dimensions();
    let scale = (REFINE_SIZE as f32 / orig_width.max(orig_height) as f32).min(1.0);
    let refine_w = ((orig_width as f32 * scale).round() as u32).max(1);
    let refine_h = ((orig_height as f32 * scale).round() as u32).max(1);
    let refine_mask = imageops::resize(&image_mask, refine_w, refine_h, FilterType::Triangle);

    Ok(Some(fast_color_guided_filter(
        &image.to_rgb8(),
        &refine_mask,
        REFINE_RADIUS,
        refine_eps,
    )))
}

/// RapidRAW's `run_esam3_zoomed_decoder`: `decode`, then, when what was found
/// is small in the picture, the same prompt again on a crop around it, blended
/// in where the two agree.
pub fn find(
    encoder: &mut Session,
    decoder: &mut Session,
    features: &Features,
    prompt: &Prompt,
    image: &DynamicImage,
    score_threshold: f32,
    refine_eps: f32,
) -> Result<Option<GrayImage>> {
    let Some(mut mask) = decode(
        decoder,
        features,
        prompt,
        image,
        score_threshold,
        refine_eps,
    )?
    else {
        return Ok(None);
    };

    let (width, height) = image.dimensions();
    let grid_size = ZOOM_MIN_SIDE as u32;
    let grid = imageops::resize(&mask, grid_size, grid_size, FilterType::Triangle);
    if let Some((x, y, crop_width, crop_height)) = zoom_region(&grid, width, height) {
        let crop = image.crop_imm(x, y, crop_width, crop_height);
        let crop_features = encode(&crop, encoder)?;
        if let Some(detail) = decode(
            decoder,
            &crop_features,
            prompt,
            &crop,
            score_threshold,
            refine_eps,
        )? && mask_iou(&mask, &detail, x, y) >= ZOOM_MIN_IOU
        {
            blend_feathered(&mut mask, &detail, x, y);
        }
    }

    Ok(Some(mask))
}

fn mask_iou(mask: &GrayImage, detail: &GrayImage, x0: u32, y0: u32) -> f32 {
    let (mut intersection, mut union) = (0u64, 0u64);
    for (u, v, refined) in detail.enumerate_pixels() {
        let base = mask.get_pixel(x0 + u, y0 + v)[0] >= 128;
        let refined = refined[0] >= 128;
        intersection += (base && refined) as u64;
        union += (base || refined) as u64;
    }
    if union == 0 {
        0.0
    } else {
        intersection as f32 / union as f32
    }
}

fn blend_feathered(mask: &mut GrayImage, detail: &GrayImage, x0: u32, y0: u32) {
    let (width, height) = mask.dimensions();
    let (crop_width, crop_height) = detail.dimensions();
    let feather = (crop_width.min(crop_height) as f32 * ZOOM_FEATHER).max(1.0);
    let ramp = |start: u32, length: u32, extent: u32| -> Vec<f32> {
        (0..length)
            .map(|i| {
                let head = if start == 0 { f32::MAX } else { i as f32 + 0.5 };
                let tail = if start + length == extent {
                    f32::MAX
                } else {
                    (length - i) as f32 - 0.5
                };
                (head.min(tail) / feather).min(1.0)
            })
            .collect()
    };
    let weights_x = ramp(x0, crop_width, width);
    let weights_y = ramp(y0, crop_height, height);

    for (v, weight_y) in weights_y.iter().enumerate() {
        for (u, weight_x) in weights_x.iter().enumerate() {
            let weight = weight_x.min(*weight_y);
            let base = mask.get_pixel(x0 + u as u32, y0 + v as u32)[0] as f32;
            let refined = detail.get_pixel(u as u32, v as u32)[0] as f32;
            mask.put_pixel(
                x0 + u as u32,
                y0 + v as u32,
                Luma([(base + (refined - base) * weight).round() as u8]),
            );
        }
    }
}

fn zoom_region(coarse: &GrayImage, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, pixel) in coarse.enumerate_pixels() {
        if pixel[0] >= 128 {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x + 1);
            max_y = max_y.max(y + 1);
        }
    }
    if min_x == u32::MAX {
        return None;
    }

    let scale_x = width as f32 / coarse.width() as f32;
    let scale_y = height as f32 / coarse.height() as f32;
    let long_side = width.max(height) as f32;
    let subject_side = ((max_x - min_x) as f32 * scale_x).max((max_y - min_y) as f32 * scale_y);
    let side = (subject_side * (1.0 + 2.0 * ZOOM_MARGIN))
        .max(ZOOM_MIN_SIDE)
        .min(long_side);
    if side > ZOOM_MAX_FRACTION * long_side {
        return None;
    }

    let place = |min: u32, max: u32, scale: f32, extent: u32| {
        let center = (min + max) as f32 * 0.5 * scale;
        let length = side.min(extent as f32);
        let start = (center - length * 0.5).clamp(0.0, extent as f32 - length);
        (start as u32, (length as u32).min(extent - start as u32))
    };
    let (x, crop_width) = place(min_x, max_x, scale_x, width);
    let (y, crop_height) = place(min_y, max_y, scale_y, height);
    Some((x, y, crop_width, crop_height))
}

fn bilinear_sample(i: usize, scale: f32, n: usize) -> (usize, usize, f32) {
    let f = ((i as f32 + 0.5) * scale - 0.5).max(0.0);
    let i0 = (f.floor() as usize).min(n - 1);
    let i1 = (i0 + 1).min(n - 1);
    (i0, i1, (f - i0 as f32).clamp(0.0, 1.0))
}

/// RapidRAW's `fast_color_guided_filter`: the guided filter worked out on the
/// small mask with the photo's three colours as the guide, then applied at the
/// photo's size, so the mask follows edges a grey guide cannot see.
pub(super) fn fast_color_guided_filter(
    guide_hr: &RgbImage,
    mask_lr: &GrayImage,
    r: usize,
    eps: f32,
) -> GrayImage {
    let (hr_w, hr_h) = guide_hr.dimensions();
    let (lr_w, lr_h) = mask_lr.dimensions();
    let (w, h) = (lr_w as usize, lr_h as usize);
    let area = w * h;

    let guide_lr = imageops::resize(guide_hr, lr_w, lr_h, FilterType::Triangle);
    let guide: [Vec<f32>; 3] =
        std::array::from_fn(|c| guide_lr.pixels().map(|p| p[c] as f32 / 255.0).collect());
    drop(guide_lr);
    let p: Vec<f32> = mask_lr.as_raw().iter().map(|&v| v as f32 / 255.0).collect();

    let mut temp1 = vec![0.0; area];
    let mut temp2 = vec![0.0; area];
    let mut box_filter = |src: &[f32]| {
        let mut mean = vec![0.0; area];
        box_filter_opt(src, &mut mean, &mut temp1, &mut temp2, w, h, r);
        mean
    };
    let product = |a: &[f32], b: &[f32]| -> Vec<f32> {
        a.par_iter().zip(b.par_iter()).map(|(x, y)| x * y).collect()
    };

    let mean_i = guide.each_ref().map(|c| box_filter(c));
    let mean_p = box_filter(&p);
    let mean_ip: [Vec<f32>; 3] = std::array::from_fn(|c| box_filter(&product(&guide[c], &p)));
    let mean_ii = [(0, 0), (0, 1), (0, 2), (1, 1), (1, 2), (2, 2)]
        .map(|(i, j)| box_filter(&product(&guide[i], &guide[j])));
    drop((guide, p));

    let coefficients: Vec<[f32; 4]> = (0..area)
        .into_par_iter()
        .map(|k| {
            let m = [mean_i[0][k], mean_i[1][k], mean_i[2][k]];
            let cov = [0, 1, 2].map(|c| mean_ip[c][k] - m[c] * mean_p[k]);
            let s00 = mean_ii[0][k] - m[0] * m[0] + eps;
            let s01 = mean_ii[1][k] - m[0] * m[1];
            let s02 = mean_ii[2][k] - m[0] * m[2];
            let s11 = mean_ii[3][k] - m[1] * m[1] + eps;
            let s12 = mean_ii[4][k] - m[1] * m[2];
            let s22 = mean_ii[5][k] - m[2] * m[2] + eps;
            let c00 = s11 * s22 - s12 * s12;
            let c01 = s02 * s12 - s01 * s22;
            let c02 = s01 * s12 - s02 * s11;
            let c11 = s00 * s22 - s02 * s02;
            let c12 = s01 * s02 - s00 * s12;
            let c22 = s00 * s11 - s01 * s01;
            let det = s00 * c00 + s01 * c01 + s02 * c02;
            let a0 = (c00 * cov[0] + c01 * cov[1] + c02 * cov[2]) / det;
            let a1 = (c01 * cov[0] + c11 * cov[1] + c12 * cov[2]) / det;
            let a2 = (c02 * cov[0] + c12 * cov[1] + c22 * cov[2]) / det;
            [a0, a1, a2, mean_p[k] - a0 * m[0] - a1 * m[1] - a2 * m[2]]
        })
        .collect();
    drop((mean_i, mean_p, mean_ip, mean_ii));

    let mean_coefficients: [Vec<f32>; 4] = std::array::from_fn(|i| {
        box_filter(&coefficients.iter().map(|c| c[i]).collect::<Vec<f32>>())
    });
    drop(coefficients);

    let (hr_w, hr_h) = (hr_w as usize, hr_h as usize);
    let x_samples: Vec<(usize, usize, f32)> = (0..hr_w)
        .map(|x| bilinear_sample(x, w as f32 / hr_w as f32, w))
        .collect();
    let mut final_mask_raw = vec![0u8; hr_w * hr_h];

    final_mask_raw
        .par_chunks_exact_mut(hr_w)
        .zip(guide_hr.as_raw().par_chunks_exact(hr_w * 3))
        .enumerate()
        .for_each(|(y, (out_row, guide_row))| {
            let (y0, y1, ty) = bilinear_sample(y, h as f32 / hr_h as f32, h);
            for (x, (out, pixel)) in out_row
                .iter_mut()
                .zip(guide_row.as_chunks::<3>().0)
                .enumerate()
            {
                let (x0, x1, tx) = x_samples[x];
                let sample = |values: &[f32]| {
                    let top = values[y0 * w + x0] * (1.0 - tx) + values[y0 * w + x1] * tx;
                    let bottom = values[y1 * w + x0] * (1.0 - tx) + values[y1 * w + x1] * tx;
                    top * (1.0 - ty) + bottom * ty
                };
                let q = (0..3)
                    .map(|c| sample(&mean_coefficients[c]) * pixel[c] as f32 / 255.0)
                    .sum::<f32>()
                    + sample(&mean_coefficients[3]);
                *out = (q.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        });

    GrayImage::from_raw(hr_w as u32, hr_h as u32, final_mask_raw).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing to zoom to, or what was found already fills most of the photo:
    /// no second pass. A small find gets a crop around it, inside the photo.
    #[test]
    fn the_zoom_crops_only_around_something_small() {
        let empty = GrayImage::new(64, 64);
        assert_eq!(zoom_region(&empty, 6000, 4000), None);

        let mut large = GrayImage::new(64, 64);
        for (x, y, p) in large.enumerate_pixels_mut() {
            if (8..56).contains(&x) && (8..56).contains(&y) {
                *p = Luma([255]);
            }
        }
        assert_eq!(zoom_region(&large, 6000, 4000), None);

        let mut small = GrayImage::new(64, 64);
        for (x, y, p) in small.enumerate_pixels_mut() {
            if (60..63).contains(&x) && (2..5).contains(&y) {
                *p = Luma([255]);
            }
        }
        let (x, y, w, h) = zoom_region(&small, 6000, 4000).expect("a crop");
        assert!(w >= 1024 && h >= 1024, "{w}x{h}");
        assert!(x + w <= 6000 && y + h <= 4000, "{x},{y} {w}x{h}");
    }

    /// The colour guide keeps an edge the mask already follows, and the answer
    /// comes back at the photo's size.
    #[test]
    fn the_guided_filter_keeps_an_edge_at_the_photo_size() {
        let guide = RgbImage::from_fn(200, 100, |x, _| {
            if x < 100 {
                image::Rgb([20, 30, 200])
            } else {
                image::Rgb([230, 200, 40])
            }
        });
        let mask = GrayImage::from_fn(50, 25, |x, _| Luma([if x < 25 { 255 } else { 0 }]));
        let out = fast_color_guided_filter(&guide, &mask, 4, 0.01);
        assert_eq!(out.dimensions(), (200, 100));
        assert!(
            out.get_pixel(20, 50)[0] > 230,
            "{}",
            out.get_pixel(20, 50)[0]
        );
        assert!(
            out.get_pixel(180, 50)[0] < 25,
            "{}",
            out.get_pixel(180, 50)[0]
        );
    }
}
