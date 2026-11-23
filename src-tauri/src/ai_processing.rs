use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::mods::ai_session::session_builder; // Argentum: sessions without the CPU memory arena
use crate::mods::{model_catalog, model_download}; // Argentum: one list, one downloader
use anyhow::Result;
use image::imageops::{self, FilterType};
use image::{DynamicImage, GenericImageView, GrayImage, ImageBuffer, Luma, Rgb32FImage, RgbaImage};
use ndarray::{Array, Array4, IxDyn};
use ort::session::Session;
use ort::value::Tensor;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tokenizers::Tokenizer;
use tokio::sync::Mutex as TokioMutex;

const SAM_INPUT_SIZE: u32 = 1024;
const U2NETP_INPUT_SIZE: u32 = 320;
const SKYSEG_INPUT_SIZE: u32 = 320;
const DEPTH_INPUT_SIZE: u32 = 518;
const NORMAL_MAX_SIZE: u32 = 768;
const NORMAL_NUM_TOKENS: i64 = 2500;

pub struct AiModels {
    pub sam_encoder: Mutex<Session>,
    pub sam_decoder: Mutex<Session>,
    pub u2netp: Mutex<Session>,
    pub sky_seg: Mutex<Session>,
    pub depth_anything: Mutex<Session>,
}

pub struct ClipModels {
    pub model: Mutex<Session>,
    pub tokenizer: Tokenizer,
}

#[derive(Clone)]
pub struct ImageEmbeddings {
    pub path_hash: String,
    pub embeddings: Array<f32, IxDyn>,
    pub original_size: (u32, u32),
}

#[derive(Clone)]
pub struct CachedDepthMap {
    pub path_hash: String,
    pub depth_image: GrayImage,
    pub original_size: (u32, u32),
}

pub struct AiState {
    pub models: Option<Arc<AiModels>>,
    pub denoise_model: Option<Arc<Mutex<Session>>>,
    pub clip_models: Option<Arc<ClipModels>>,
    pub lama_model: Option<Arc<Mutex<Session>>>,
    pub normal_model: Option<Arc<Mutex<Session>>>,
    pub embeddings: Option<ImageEmbeddings>,
    pub depth_map: Option<CachedDepthMap>,
}

fn edt_1d(f: &mut [f32], v: &mut [usize], z: &mut [f32], d: &mut [f32]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let mut k = 0;
    v[0] = 0;
    z[0] = f32::NEG_INFINITY;
    z[1] = f32::INFINITY;
    for q in 1..n {
        let mut s = ((f[q] + (q * q) as f32) - (f[v[k]] + (v[k] * v[k]) as f32))
            / (2.0 * (q as f32 - v[k] as f32));
        while s <= z[k] {
            if k == 0 {
                break;
            }
            k -= 1;
            s = ((f[q] + (q * q) as f32) - (f[v[k]] + (v[k] * v[k]) as f32))
                / (2.0 * (q as f32 - v[k] as f32));
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f32::INFINITY;
    }
    k = 0;
    for (q, d_q) in d[..n].iter_mut().enumerate() {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let diff = q as f32 - v[k] as f32;
        *d_q = diff * diff + f[v[k]];
    }
    f.copy_from_slice(&d[..n]);
}

fn edt_2d(grid: &[bool], width: usize, height: usize) -> Vec<f32> {
    let area = width * height;
    let mut f = vec![0.0; area];
    for i in 0..area {
        f[i] = if grid[i] { 1e10 } else { 0.0 };
    }

    let max_dim = width.max(height);
    let mut v = vec![0; max_dim];
    let mut z = vec![0.0; max_dim + 1];
    let mut d = vec![0.0; max_dim];

    for y in 0..height {
        let start = y * width;
        let end = start + width;
        edt_1d(&mut f[start..end], &mut v, &mut z, &mut d);
    }

    let mut col = vec![0.0; height];
    for x in 0..width {
        for y in 0..height {
            col[y] = f[y * width + x];
        }
        edt_1d(&mut col, &mut v, &mut z, &mut d);
        for y in 0..height {
            f[y * width + x] = col[y];
        }
    }

    f.into_iter().map(|v| v.sqrt()).collect()
}

fn box_filter_horiz(src: &[f32], dest: &mut [f32], w: usize, r: usize) {
    dest.par_chunks_exact_mut(w)
        .zip(src.par_chunks_exact(w))
        .for_each(|(dest_row, src_row)| {
            let mut prefix = vec![0.0; w + 1];
            for x in 0..w {
                prefix[x + 1] = prefix[x] + src_row[x];
            }
            for (x, dest_val) in dest_row.iter_mut().enumerate() {
                let x_min = x.saturating_sub(r);
                let x_max = (x + r).min(w - 1);
                let count = (x_max - x_min + 1) as f32;
                *dest_val = (prefix[x_max + 1] - prefix[x_min]) / count;
            }
        });
}

fn transpose(src: &[f32], dest: &mut [f32], w: usize, h: usize) {
    dest.par_chunks_exact_mut(h)
        .enumerate()
        .for_each(|(x, dest_col)| {
            for y in 0..h {
                dest_col[y] = src[y * w + x];
            }
        });
}

// Argentum: shared with mods/esam3.rs's colour guided filter.
pub(crate) fn box_filter_opt(
    src: &[f32],
    dest: &mut [f32],
    temp1: &mut [f32],
    temp2: &mut [f32],
    w: usize,
    h: usize,
    r: usize,
) {
    box_filter_horiz(src, temp1, w, r);
    transpose(temp1, temp2, w, h);
    box_filter_horiz(temp2, temp1, h, r);
    transpose(temp1, dest, h, w);
}

pub fn fast_guided_filter(
    guide_hr: &GrayImage,
    mask_lr: &GrayImage,
    r: usize,
    eps: f32,
) -> GrayImage {
    let (hr_w, hr_h) = guide_hr.dimensions();
    let (lr_w, lr_h) = mask_lr.dimensions();

    let w = lr_w as usize;
    let h = lr_h as usize;
    let area = w * h;

    let guide_lr = imageops::resize(guide_hr, lr_w, lr_h, FilterType::Triangle);

    let mut i_lr = vec![0.0; area];
    let mut p_lr = vec![0.0; area];
    let mut ip_lr = vec![0.0; area];
    let mut ii_lr = vec![0.0; area];

    let guide_lr_raw = guide_lr.as_raw();
    let mask_lr_raw = mask_lr.as_raw();

    i_lr.par_iter_mut()
        .zip(p_lr.par_iter_mut())
        .zip(ip_lr.par_iter_mut())
        .zip(ii_lr.par_iter_mut())
        .zip(guide_lr_raw.par_iter())
        .zip(mask_lr_raw.par_iter())
        .for_each(|(((((i_out, p_out), ip_out), ii_out), &g_val), &m_val)| {
            let i_val = g_val as f32 / 255.0;
            let p_val = m_val as f32 / 255.0;
            *i_out = i_val;
            *p_out = p_val;
            *ip_out = i_val * p_val;
            *ii_out = i_val * i_val;
        });

    let mut temp1 = vec![0.0; area];
    let mut temp2 = vec![0.0; area];

    let mut mean_i = vec![0.0; area];
    let mut mean_p = vec![0.0; area];
    let mut mean_ip = vec![0.0; area];
    let mut mean_ii = vec![0.0; area];

    box_filter_opt(&i_lr, &mut mean_i, &mut temp1, &mut temp2, w, h, r);
    box_filter_opt(&p_lr, &mut mean_p, &mut temp1, &mut temp2, w, h, r);
    box_filter_opt(&ip_lr, &mut mean_ip, &mut temp1, &mut temp2, w, h, r);
    box_filter_opt(&ii_lr, &mut mean_ii, &mut temp1, &mut temp2, w, h, r);

    let mut a = vec![0.0; area];
    let mut b = vec![0.0; area];

    a.par_iter_mut()
        .zip(b.par_iter_mut())
        .zip(mean_i.par_iter())
        .zip(mean_p.par_iter())
        .zip(mean_ip.par_iter())
        .zip(mean_ii.par_iter())
        .for_each(|(((((a_out, b_out), &m_i), &m_p), &m_ip), &m_ii)| {
            let cov_ip = m_ip - m_i * m_p;
            let var_i = m_ii - m_i * m_i;
            let a_val = cov_ip / (var_i + eps);
            *a_out = a_val;
            *b_out = m_p - a_val * m_i;
        });

    let mut mean_a = vec![0.0; area];
    let mut mean_b = vec![0.0; area];

    box_filter_opt(&a, &mut mean_a, &mut temp1, &mut temp2, w, h, r);
    box_filter_opt(&b, &mut mean_b, &mut temp1, &mut temp2, w, h, r);

    let mean_a_img = ImageBuffer::<Luma<f32>, Vec<f32>>::from_raw(lr_w, lr_h, mean_a).unwrap();
    let mean_b_img = ImageBuffer::<Luma<f32>, Vec<f32>>::from_raw(lr_w, lr_h, mean_b).unwrap();

    let mean_a_hr = imageops::resize(&mean_a_img, hr_w, hr_h, FilterType::Triangle);
    let mean_b_hr = imageops::resize(&mean_b_img, hr_w, hr_h, FilterType::Triangle);

    let mean_a_hr_raw = mean_a_hr.as_raw();
    let mean_b_hr_raw = mean_b_hr.as_raw();
    let guide_hr_raw = guide_hr.as_raw();

    let mut final_mask_raw = vec![0u8; (hr_w * hr_h) as usize];

    final_mask_raw
        .par_iter_mut()
        .zip(mean_a_hr_raw.par_iter())
        .zip(mean_b_hr_raw.par_iter())
        .zip(guide_hr_raw.par_iter())
        .for_each(|(((out, &a_val), &b_val), &i_val)| {
            let i_v = i_val as f32 / 255.0;
            let q = a_val * i_v + b_val;
            *out = (q.clamp(0.0, 1.0) * 255.0).round() as u8;
        });

    GrayImage::from_raw(hr_w, hr_h, final_mask_raw).unwrap()
}

// Argentum: the files, their addresses and checksums are `model_catalog`'s,
// fetched and checked by `model_download` (the sky model's old name included).
async fn ensure_model(app_handle: &tauri::AppHandle, id: &str, notice: &str) -> Result<PathBuf> {
    model_download::ensure_in_app(app_handle, id, notice)
        .await
        .map_err(anyhow::Error::msg)
}

pub async fn get_or_init_ai_models(
    app_handle: &tauri::AppHandle,
    ai_state_mutex: &Mutex<Option<AiState>>,
    ai_init_lock: &TokioMutex<()>,
) -> Result<Arc<AiModels>> {
    if let Some(models) = ai_state_mutex
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|state| state.models.clone())
    {
        return Ok(models);
    }

    let _guard = ai_init_lock.lock().await;

    if let Some(models) = ai_state_mutex
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|state| state.models.clone())
    {
        return Ok(models);
    }

    let models_dir = ensure_model(app_handle, model_catalog::SUBJECT, "SAM Model").await?;
    ensure_model(app_handle, model_catalog::FOREGROUND, "Foreground Model").await?;
    ensure_model(app_handle, model_catalog::SKY, "Sky Model").await?;
    ensure_model(app_handle, model_catalog::DEPTH, "Depth Model").await?;

    let encoder_path = models_dir.join(model_catalog::SAM_ENCODER.name);
    let decoder_path = models_dir.join(model_catalog::SAM_DECODER.name);
    let u2netp_path = models_dir.join(model_catalog::U2NET.name);
    let sky_seg_path = models_dir.join(model_catalog::SKYSEG.name);
    let depth_path = models_dir.join(model_catalog::DEPTH_ANYTHING.name);

    let sam_encoder = session_builder()?.commit_from_file(encoder_path)?;
    let sam_decoder = session_builder()?.commit_from_file(decoder_path)?;
    let u2netp = session_builder()?.commit_from_file(u2netp_path)?;
    let sky_seg = session_builder()?.commit_from_file(sky_seg_path)?;
    let depth_anything = session_builder()?.commit_from_file(depth_path)?;

    crate::register_exit_handler();

    let models = Arc::new(AiModels {
        sam_encoder: Mutex::new(sam_encoder),
        sam_decoder: Mutex::new(sam_decoder),
        u2netp: Mutex::new(u2netp),
        sky_seg: Mutex::new(sky_seg),
        depth_anything: Mutex::new(depth_anything),
    });

    let mut ai_state_lock = ai_state_mutex.lock().unwrap();
    if let Some(state) = ai_state_lock.as_mut() {
        state.models = Some(models.clone());
    } else {
        *ai_state_lock = Some(AiState {
            models: Some(models.clone()),
            denoise_model: None,
            clip_models: None,
            lama_model: None,
            normal_model: None,
            embeddings: None,
            depth_map: None,
        });
    }

    Ok(models)
}

pub async fn get_or_init_denoise_model(
    app_handle: &tauri::AppHandle,
    ai_state_mutex: &Mutex<Option<AiState>>,
    ai_init_lock: &TokioMutex<()>,
) -> Result<Arc<Mutex<Session>>> {
    if let Some(denoise_model) = ai_state_mutex
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|state| state.denoise_model.clone())
    {
        return Ok(denoise_model);
    }

    let _guard = ai_init_lock.lock().await;

    if let Some(denoise_model) = ai_state_mutex
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|state| state.denoise_model.clone())
    {
        return Ok(denoise_model);
    }

    let models_dir = ensure_model(app_handle, model_catalog::DENOISE, "NIND Denoise Model").await?;

    let model_path = models_dir.join(model_catalog::NIND_DENOISE.name);
    let session = session_builder()?.commit_from_file(model_path)?;
    let denoise_model = Arc::new(Mutex::new(session));

    crate::register_exit_handler();

    let mut ai_state_lock = ai_state_mutex.lock().unwrap();
    if let Some(state) = ai_state_lock.as_mut() {
        state.denoise_model = Some(denoise_model.clone());
    } else {
        *ai_state_lock = Some(AiState {
            models: None,
            denoise_model: Some(denoise_model.clone()),
            clip_models: None,
            lama_model: None,
            normal_model: None,
            embeddings: None,
            depth_map: None,
        });
    }

    Ok(denoise_model)
}

pub async fn get_or_init_clip_models(
    app_handle: &tauri::AppHandle,
    ai_state_mutex: &Mutex<Option<AiState>>,
    ai_init_lock: &TokioMutex<()>,
) -> Result<Arc<ClipModels>> {
    if let Some(clip_models) = ai_state_mutex
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|state| state.clip_models.clone())
    {
        return Ok(clip_models);
    }

    let _guard = ai_init_lock.lock().await;

    if let Some(clip_models) = ai_state_mutex
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|state| state.clip_models.clone())
    {
        return Ok(clip_models);
    }

    let models_dir = ensure_model(app_handle, model_catalog::TAGGING, "CLIP Model").await?;
    let clip_tokenizer_path = models_dir.join(model_catalog::CLIP_TOKENIZER.name);

    let clip_model_path = models_dir.join(model_catalog::CLIP_MODEL.name);
    let model = Mutex::new(session_builder()?.commit_from_file(clip_model_path)?);
    let tokenizer =
        Tokenizer::from_file(clip_tokenizer_path).map_err(|e| anyhow::anyhow!(e.to_string()))?;

    crate::register_exit_handler();

    let clip_models = Arc::new(ClipModels { model, tokenizer });

    let mut ai_state_lock = ai_state_mutex.lock().unwrap();
    if let Some(state) = ai_state_lock.as_mut() {
        state.clip_models = Some(clip_models.clone());
    } else {
        *ai_state_lock = Some(AiState {
            models: None,
            denoise_model: None,
            clip_models: Some(clip_models.clone()),
            lama_model: None,
            normal_model: None,
            embeddings: None,
            depth_map: None,
        });
    }

    Ok(clip_models)
}

pub async fn get_or_init_lama_model(
    app_handle: &tauri::AppHandle,
    ai_state_mutex: &Mutex<Option<AiState>>,
    ai_init_lock: &TokioMutex<()>,
) -> Result<Arc<Mutex<Session>>> {
    if let Some(lama_model) = ai_state_mutex
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|state| state.lama_model.clone())
    {
        return Ok(lama_model);
    }

    let _guard = ai_init_lock.lock().await;

    if let Some(lama_model) = ai_state_mutex
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|state| state.lama_model.clone())
    {
        return Ok(lama_model);
    }

    let models_dir =
        ensure_model(app_handle, model_catalog::INPAINTING, "Inpainting Model").await?;

    let model_path = models_dir.join(model_catalog::LAMA.name);
    let session = session_builder()?.commit_from_file(model_path)?;
    let lama_model = Arc::new(Mutex::new(session));

    crate::register_exit_handler();

    let mut ai_state_lock = ai_state_mutex.lock().unwrap();
    if let Some(state) = ai_state_lock.as_mut() {
        state.lama_model = Some(lama_model.clone());
    } else {
        *ai_state_lock = Some(AiState {
            models: None,
            denoise_model: None,
            clip_models: None,
            lama_model: Some(lama_model.clone()),
            normal_model: None,
            embeddings: None,
            depth_map: None,
        });
    }

    Ok(lama_model)
}

pub async fn get_or_init_normal_model(
    app_handle: &tauri::AppHandle,
    ai_state_mutex: &Mutex<Option<AiState>>,
    ai_init_lock: &TokioMutex<()>,
) -> Result<Arc<Mutex<Session>>> {
    if let Some(normal_model) = ai_state_mutex
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|state| state.normal_model.clone())
    {
        return Ok(normal_model);
    }

    let _guard = ai_init_lock.lock().await;

    if let Some(normal_model) = ai_state_mutex
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|state| state.normal_model.clone())
    {
        return Ok(normal_model);
    }

    let models_dir = ensure_model(app_handle, model_catalog::RELIGHT, "Normal Model").await?;

    let model_path = models_dir.join(model_catalog::MOGE_NORMAL.name);
    let session = crate::mods::ai_session::builder()?.commit_from_file(model_path)?;
    let normal_model = Arc::new(Mutex::new(session));

    crate::register_exit_handler();

    let mut ai_state_lock = ai_state_mutex.lock().unwrap();
    if let Some(state) = ai_state_lock.as_mut() {
        state.normal_model = Some(normal_model.clone());
    } else {
        *ai_state_lock = Some(AiState {
            models: None,
            denoise_model: None,
            clip_models: None,
            lama_model: None,
            normal_model: Some(normal_model.clone()),
            embeddings: None,
            depth_map: None,
        });
    }

    Ok(normal_model)
}

#[derive(Clone, Copy)]
struct TileParams {
    cs: usize,
    margin: usize,
    overlap: usize,
}

const TILE_FASTER: TileParams = TileParams {
    cs: 504,
    margin: 8,
    overlap: 32,
};
const TILE_BALANCED: TileParams = TileParams {
    cs: 504,
    margin: 12,
    overlap: 64,
};
const TILE_HIGHER_QUALITY: TileParams = TileParams {
    cs: 504,
    margin: 16,
    overlap: 128,
};

fn select_tile_params(quality_0_1: f32) -> TileParams {
    let q = quality_0_1.clamp(0.0, 1.0);
    if q <= 0.25 {
        TILE_FASTER
    } else if q >= 0.75 {
        TILE_HIGHER_QUALITY
    } else {
        TILE_BALANCED
    }
}

#[inline]
fn mirror_coord(c: i32, size: i32) -> i32 {
    if c < 0 {
        (-c).min(size - 1)
    } else if c >= size {
        (2 * size - 1 - c).max(0)
    } else {
        c
    }
}

fn extract_tile_mirror(img: &Rgb32FImage, x0: i32, y0: i32, cs: usize) -> Array4<f32> {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let mut arr = Array4::zeros((1, 3, cs, cs));
    for dy in 0..cs as i32 {
        for dx in 0..cs as i32 {
            let sx = mirror_coord(x0 + dx, w);
            let sy = mirror_coord(y0 + dy, h);
            let px = img.get_pixel(sx as u32, sy as u32);
            arr[[0, 0, dy as usize, dx as usize]] = px[0];
            arr[[0, 1, dy as usize, dx as usize]] = px[1];
            arr[[0, 2, dy as usize, dx as usize]] = px[2];
        }
    }
    arr
}

fn tile_origins(len: usize, params: TileParams) -> Vec<i32> {
    let valid = params.cs - 2 * params.margin;
    let step = (valid - params.overlap).max(1);
    let mut origins = Vec::new();
    let mut start = 0;
    loop {
        origins.push(start as i32 - params.margin as i32);
        if start + valid >= len {
            break;
        }
        start += step;
    }
    origins
}

fn tile_blend_weights(params: TileParams) -> Vec<f32> {
    let valid = params.cs - 2 * params.margin;
    let ramp = |d: usize| ((d as f32 + 0.5) / params.overlap.max(1) as f32).min(1.0);
    (0..valid)
        .map(|i| {
            let t = ramp(i).min(ramp(valid - 1 - i));
            t * t * (3.0 - 2.0 * t)
        })
        .collect()
}

fn run_native_denoise(
    img: &Rgb32FImage,
    session: &Mutex<Session>,
    params: TileParams,
    on_progress: &dyn Fn(f32),
) -> Result<Rgb32FImage> {
    let (width, height) = (img.width() as usize, img.height() as usize);
    let valid = params.cs - 2 * params.margin;
    let weights = tile_blend_weights(params);
    let xs = tile_origins(width, params);
    let ys = tile_origins(height, params);
    let total = xs.len() * ys.len();

    let mut accumulator = vec![0.0f32; width * height * 3];
    let mut weight_sum = vec![0.0f32; width * height];

    for (i, (&y0, &x0)) in ys
        .iter()
        .flat_map(|y| xs.iter().map(move |x| (y, x)))
        .enumerate()
    {
        on_progress(i as f32 / total as f32);

        let crop = extract_tile_mirror(img, x0, y0, params.cs);
        let t_input = Tensor::from_array(crop.as_standard_layout().to_owned())?;

        let tile = {
            let mut sess = session.lock().unwrap();
            let outputs = sess.run(ort::inputs![t_input])?;
            outputs[0]
                .try_extract_array::<f32>()?
                .to_owned()
                .into_dimensionality::<ndarray::Ix4>()
                .map_err(|e| anyhow::anyhow!("Unexpected output shape: {}", e))?
        };

        for ty in 0..valid {
            let gy = y0 + (params.margin + ty) as i32;
            if gy < 0 || gy >= height as i32 {
                continue;
            }
            for tx in 0..valid {
                let gx = x0 + (params.margin + tx) as i32;
                if gx < 0 || gx >= width as i32 {
                    continue;
                }
                let w = weights[ty] * weights[tx];
                let idx = gy as usize * width + gx as usize;
                for c in 0..3 {
                    let v = tile[[0, c, params.margin + ty, params.margin + tx]].clamp(0.0, 1.0);
                    accumulator[idx * 3 + c] += v * w;
                }
                weight_sum[idx] += w;
            }
        }
    }

    let mut out = Rgb32FImage::new(width as u32, height as u32);
    out.par_chunks_exact_mut(3)
        .zip(accumulator.par_chunks_exact(3))
        .zip(weight_sum.par_iter())
        .for_each(|((px, acc), &w)| {
            let inv = if w > 0.0 { 1.0 / w } else { 0.0 };
            for c in 0..3 {
                px[c] = (acc[c] * inv).clamp(0.0, 1.0);
            }
        });
    Ok(out)
}

pub fn run_ai_denoise(
    rgb_img: &Rgb32FImage,
    intensity: f32,
    session: &Mutex<Session>,
    on_progress: &dyn Fn(f32),
) -> Result<DynamicImage> {
    let params = select_tile_params(intensity);
    let out = run_native_denoise(rgb_img, session, params, on_progress)?;
    Ok(DynamicImage::ImageRgb32F(out))
}

// RapidRAW b407d348: src-tauri/src/ai_processing.rs.
const LAMA_PIXEL_BUDGET: f32 = 768.0 * 768.0;
const LAMA_PATCH_RADIUS: i32 = 3;
const LAMA_PATCH_MATCH_ITERS: u32 = 6;

fn dilate_mask(mask: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    let mut horiz = vec![false; w * h];
    horiz.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let src = &mask[y * w..(y + 1) * w];
        let mut prefix = vec![0u32; w + 1];
        for x in 0..w {
            prefix[x + 1] = prefix[x] + src[x] as u32;
        }
        for (x, out) in row.iter_mut().enumerate() {
            *out = prefix[(x + r + 1).min(w)] > prefix[x.saturating_sub(r)];
        }
    });
    let mut dilated = vec![false; w * h];
    dilated.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let y0 = y.saturating_sub(r);
        let y1 = (y + r).min(h - 1);
        for (x, out) in row.iter_mut().enumerate() {
            *out = (y0..=y1).any(|yy| horiz[yy * w + x]);
        }
    });
    dilated
}

fn catmull_rom_sample(img: &[[f32; 3]], w: usize, h: usize, x: f32, y: f32) -> [f32; 3] {
    let weights = |t: f32| {
        let t2 = t * t;
        let t3 = t2 * t;
        [
            -0.5 * t3 + t2 - 0.5 * t,
            1.5 * t3 - 2.5 * t2 + 1.0,
            -1.5 * t3 + 2.0 * t2 + 0.5 * t,
            0.5 * t3 - 0.5 * t2,
        ]
    };
    let (fx, fy) = (x.floor(), y.floor());
    let wx = weights(x - fx);
    let wy = weights(y - fy);
    let mut acc = [0.0f32; 3];
    for (j, wyj) in wy.iter().enumerate() {
        let sy = (fy as i32 + j as i32 - 1).clamp(0, h as i32 - 1) as usize;
        for (i, wxi) in wx.iter().enumerate() {
            let sx = (fx as i32 + i as i32 - 1).clamp(0, w as i32 - 1) as usize;
            let p = img[sy * w + sx];
            let wgt = wyj * wxi;
            acc[0] += p[0] * wgt;
            acc[1] += p[1] * wgt;
            acc[2] += p[2] * wgt;
        }
    }
    acc
}

fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

fn patch_distance(
    target: &[[f32; 3]],
    source: &[[f32; 3]],
    w: usize,
    h: usize,
    t: (i32, i32),
    s: (i32, i32),
    limit: f32,
) -> f32 {
    let mut d = 0.0f32;
    for dy in -LAMA_PATCH_RADIUS..=LAMA_PATCH_RADIUS {
        let ty = (t.1 + dy).clamp(0, h as i32 - 1) as usize;
        let srow = (s.1 + dy) as usize * w;
        for dx in -LAMA_PATCH_RADIUS..=LAMA_PATCH_RADIUS {
            let tx = (t.0 + dx).clamp(0, w as i32 - 1) as usize;
            let a = target[ty * w + tx];
            let b = source[srow + (s.0 + dx) as usize];
            let (e0, e1, e2) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
            d += e0 * e0 + e1 * e1 + e2 * e2;
        }
        if d >= limit {
            return d;
        }
    }
    d
}

fn lama_patch_match(
    target: &[[f32; 3]],
    source: &[[f32; 3]],
    w: usize,
    h: usize,
    is_target: &[bool],
    is_source: &[bool],
) -> Option<Vec<Option<(i32, i32)>>> {
    let sources: Vec<(i32, i32)> = (0..w * h)
        .filter(|&i| is_source[i])
        .map(|i| ((i % w) as i32, (i / w) as i32))
        .collect();
    let targets: Vec<usize> = (0..w * h).filter(|&i| is_target[i]).collect();
    if sources.is_empty() || targets.is_empty() {
        return None;
    }

    let valid = |p: (i32, i32)| {
        p.0 >= 0
            && p.1 >= 0
            && (p.0 as usize) < w
            && (p.1 as usize) < h
            && is_source[p.1 as usize * w + p.0 as usize]
    };
    let random_source = |seed: u64| sources[(seed % sources.len() as u64) as usize];

    let mut nnf = vec![(0i32, 0i32); w * h];
    let mut cost = vec![f32::MAX; w * h];
    let init: Vec<((i32, i32), f32)> = targets
        .par_iter()
        .map(|&i| {
            let t = ((i % w) as i32, (i / w) as i32);
            let s = random_source(splitmix64(i as u64));
            (s, patch_distance(target, source, w, h, t, s, f32::MAX))
        })
        .collect();
    for (&i, (s, c)) in targets.iter().zip(init) {
        nnf[i] = s;
        cost[i] = c;
    }

    for iter in 0..LAMA_PATCH_MATCH_ITERS {
        let jump = 1i32 << (LAMA_PATCH_MATCH_ITERS - 1 - iter).min(3);
        let updates: Vec<((i32, i32), f32)> = targets
            .par_iter()
            .map(|&i| {
                let t = ((i % w) as i32, (i / w) as i32);
                let mut best = nnf[i];
                let mut best_cost = cost[i];
                let consider = |c: (i32, i32), best: &mut (i32, i32), best_cost: &mut f32| {
                    if valid(c) && c != *best {
                        let d = patch_distance(target, source, w, h, t, c, *best_cost);
                        if d < *best_cost {
                            *best = c;
                            *best_cost = d;
                        }
                    }
                };

                for (dx, dy) in [(-jump, 0), (jump, 0), (0, -jump), (0, jump)] {
                    let (nx, ny) = (t.0 + dx, t.1 + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                        let n = ny as usize * w + nx as usize;
                        if is_target[n] {
                            consider((nnf[n].0 - dx, nnf[n].1 - dy), &mut best, &mut best_cost);
                        }
                    }
                }

                let mut seed = splitmix64(i as u64 ^ ((iter as u64 + 1) << 40));
                let mut radius = w.max(h) as i32;
                while radius >= 1 {
                    seed = splitmix64(seed);
                    let span = (2 * radius + 1) as u64;
                    let c = (
                        best.0 + (seed % span) as i32 - radius,
                        best.1 + ((seed >> 32) % span) as i32 - radius,
                    );
                    consider(c, &mut best, &mut best_cost);
                    radius /= 2;
                }
                consider(random_source(splitmix64(seed)), &mut best, &mut best_cost);

                (best, best_cost)
            })
            .collect();
        for (&i, (s, c)) in targets.iter().zip(updates) {
            nnf[i] = s;
            cost[i] = c;
        }
    }

    Some((0..w * h).map(|i| is_target[i].then_some(nnf[i])).collect())
}

pub fn run_lama_inpainting(
    image: &DynamicImage,
    mask: &GrayImage,
    lama_session: &Mutex<Session>,
) -> Result<RgbaImage> {
    let (w, h) = image.dimensions();

    let (mut min_x, mut min_y) = (w, h);
    let (mut max_x, mut max_y) = (0u32, 0u32);
    let mut has_mask = false;

    for (x, y, p) in mask.enumerate_pixels() {
        if p[0] > 0 {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            has_mask = true;
        }
    }

    if !has_mask {
        return Ok(image.to_rgba8());
    }

    let mask_w = max_x - min_x + 1;
    let mask_h = max_y - min_y + 1;

    let pad_x = 128.max((mask_w as f32 * 1.5) as u32);
    let pad_y = 128.max((mask_h as f32 * 1.5) as u32);

    let x0 = min_x.saturating_sub(pad_x);
    let y0 = min_y.saturating_sub(pad_y);
    let x1 = (max_x + pad_x).min(w.saturating_sub(1));
    let y1 = (max_y + pad_y).min(h.saturating_sub(1));

    let crop_w = x1 - x0 + 1;
    let crop_h = y1 - y0 + 1;

    // Argentum: one full-size copy, the result, which the crop is taken from first.
    let mut final_image = image.to_rgba8();

    let cropped_img = imageops::crop_imm(&final_image, x0, y0, crop_w, crop_h).to_image();
    let cropped_mask = imageops::crop_imm(mask, x0, y0, crop_w, crop_h).to_image();

    // RapidRAW b407d348: src-tauri/src/ai_processing.rs. The fill is made at up
    // to 768 x 768 pixels of area; when the crop is larger, each filled pixel
    // takes the fine texture of the place the fill matches, at full size.
    let scale = (LAMA_PIXEL_BUDGET / (crop_w as f32 * crop_h as f32))
        .sqrt()
        .min(1.0);
    let fw = ((crop_w as f32 * scale).round() as u32).max(1);
    let fh = ((crop_h as f32 * scale).round() as u32).max(1);
    let needs_downscale = fw != crop_w || fh != crop_h;

    let (inf_img, inf_mask) = if needs_downscale {
        (
            imageops::resize(&cropped_img, fw, fh, FilterType::Lanczos3),
            imageops::resize(&cropped_mask, fw, fh, FilterType::Triangle),
        )
    } else {
        (cropped_img.clone(), cropped_mask.clone())
    };

    let align = 32u32;
    let tensor_w = fw.div_ceil(align) * align;
    let tensor_h = fh.div_ceil(align) * align;

    let mut img_tensor = Array::<f32, _>::zeros((1, 3, tensor_h as usize, tensor_w as usize));
    let mut msk_tensor = Array::<f32, _>::zeros((1, 1, tensor_h as usize, tensor_w as usize));

    for y in 0..tensor_h {
        for x in 0..tensor_w {
            let p = inf_img.get_pixel(x.min(fw - 1), y.min(fh - 1));
            let m = inf_mask.get_pixel(x.min(fw - 1), y.min(fh - 1))[0];
            let (y, x) = (y as usize, x as usize);

            img_tensor[[0, 0, y, x]] = p[0] as f32 / 255.0;
            img_tensor[[0, 1, y, x]] = p[1] as f32 / 255.0;
            img_tensor[[0, 2, y, x]] = p[2] as f32 / 255.0;
            msk_tensor[[0, 0, y, x]] = if m > 0 { 1.0 } else { 0.0 };
        }
    }

    let t_img = Tensor::from_array(img_tensor.into_dyn().as_standard_layout().into_owned())?;
    let t_msk = Tensor::from_array(msk_tensor.into_dyn().as_standard_layout().into_owned())?;

    let output_tensor = {
        let mut session = lama_session.lock().unwrap();
        let outputs = session.run(ort::inputs!["image" => t_img, "mask" => t_msk])?;
        outputs[0].try_extract_array::<f32>()?.to_owned()
    };

    let (iw, ih) = (fw as usize, fh as usize);
    let target: Vec<[f32; 3]> = (0..iw * ih)
        .map(|i| {
            let (x, y) = (i % iw, i / iw);
            [0, 1, 2].map(|c| (output_tensor[[0, c, y, x]] / 255.0).clamp(0.0, 1.0))
        })
        .collect();
    let source: Vec<[f32; 3]> = inf_img
        .pixels()
        .map(|p| {
            [
                p[0] as f32 / 255.0,
                p[1] as f32 / 255.0,
                p[2] as f32 / 255.0,
            ]
        })
        .collect();

    let nnf = if needs_downscale {
        let hole: Vec<bool> = inf_mask.pixels().map(|p| p[0] > 0).collect();
        let is_target = dilate_mask(&hole, iw, ih, 1);
        let blocked = dilate_mask(&hole, iw, ih, LAMA_PATCH_RADIUS as usize + 2);
        let r = LAMA_PATCH_RADIUS;
        let is_source: Vec<bool> = (0..iw * ih)
            .map(|i| {
                let (x, y) = ((i % iw) as i32, (i / iw) as i32);
                !blocked[i] && x >= r && y >= r && x < iw as i32 - r && y < ih as i32 - r
            })
            .collect();
        lama_patch_match(&target, &source, iw, ih, &is_target, &is_source)
    } else {
        None
    };

    let sx = fw as f32 / crop_w as f32;
    let sy = fh as f32 / crop_h as f32;
    let mut final_crop = cropped_img.clone();

    final_crop
        .par_chunks_mut(crop_w as usize * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..crop_w as usize {
                let m = cropped_mask.get_pixel(x as u32, y as u32)[0];
                if m == 0 {
                    continue;
                }
                let u = (x as f32 + 0.5) * sx - 0.5;
                let v = (y as f32 + 0.5) * sy - 0.5;
                let mut value = catmull_rom_sample(&target, iw, ih, u, v);

                if let Some(nnf) = &nnf {
                    let (u0, v0) = (u.floor(), v.floor());
                    let (fu, fv) = (u - u0, v - v0);
                    let mut groups = [((0i32, 0i32), 0.0f32, [0.0f32; 3]); 4];
                    let mut group_count = 0;
                    for (cu, cv, wgt) in [
                        (u0, v0, (1.0 - fu) * (1.0 - fv)),
                        (u0 + 1.0, v0, fu * (1.0 - fv)),
                        (u0, v0 + 1.0, (1.0 - fu) * fv),
                        (u0 + 1.0, v0 + 1.0, fu * fv),
                    ] {
                        let cu = (cu as i32).clamp(0, iw as i32 - 1);
                        let cv = (cv as i32).clamp(0, ih as i32 - 1);
                        let Some((su, sv)) = nnf[cv as usize * iw + cu as usize] else {
                            continue;
                        };
                        let qx = x as i32 + ((su - cu) as f32 / sx).round() as i32;
                        let qy = y as i32 + ((sv - cv) as f32 / sy).round() as i32;
                        if qx < 0 || qy < 0 || qx >= crop_w as i32 || qy >= crop_h as i32 {
                            continue;
                        }
                        if cropped_mask.get_pixel(qx as u32, qy as u32)[0] > 0 {
                            continue;
                        }
                        let o = cropped_img.get_pixel(qx as u32, qy as u32);
                        let low = catmull_rom_sample(
                            &source,
                            iw,
                            ih,
                            (qx as f32 + 0.5) * sx - 0.5,
                            (qy as f32 + 0.5) * sy - 0.5,
                        );
                        let detail = [0, 1, 2].map(|c| o[c] as f32 / 255.0 - low[c]);
                        let shift = (qx - x as i32, qy - y as i32);
                        match groups[..group_count].iter_mut().find(|g| g.0 == shift) {
                            Some(g) => g.1 += wgt,
                            None => {
                                groups[group_count] = (shift, wgt, detail);
                                group_count += 1;
                            }
                        }
                    }
                    let norm = groups[..group_count]
                        .iter()
                        .map(|g| g.1 * g.1)
                        .sum::<f32>()
                        .sqrt();
                    if norm > 0.0 {
                        for (_, wgt, detail) in &groups[..group_count] {
                            for c in 0..3 {
                                value[c] += wgt * detail[c] / norm;
                            }
                        }
                    }
                }

                let alpha = m as f32 / 255.0;
                let px = &mut row[x * 4..x * 4 + 3];
                for c in 0..3 {
                    px[c] = (value[c].clamp(0.0, 1.0) * 255.0 * alpha
                        + px[c] as f32 * (1.0 - alpha))
                        .round() as u8;
                }
            }
        });

    // Argentum: the crop goes back into the full picture (theirs returns it alone).
    imageops::replace(&mut final_image, &final_crop, x0 as i64, y0 as i64);

    Ok(final_image)
}

pub fn generate_image_embeddings(
    image: &DynamicImage,
    encoder: &Mutex<Session>,
) -> Result<ImageEmbeddings> {
    let (orig_width, orig_height) = image.dimensions();

    let long_side = orig_width.max(orig_height) as f32;
    let scale = SAM_INPUT_SIZE as f32 / long_side;
    let new_width = (orig_width as f32 * scale).round() as u32;
    let new_height = (orig_height as f32 * scale).round() as u32;

    let resized_image = image.resize(new_width, new_height, FilterType::Triangle);
    let rgb_image = resized_image.into_rgb8();
    let (actual_width, actual_height) = rgb_image.dimensions();
    let raw_pixels = rgb_image.as_raw();

    let mut input_tensor: Array<u8, _> =
        Array::zeros((1, 3, SAM_INPUT_SIZE as usize, SAM_INPUT_SIZE as usize));

    let w_usize = actual_width as usize;
    for y in 0..(actual_height as usize) {
        for x in 0..w_usize {
            let idx = (y * w_usize + x) * 3;
            input_tensor[[0, 0, y, x]] = raw_pixels[idx];
            input_tensor[[0, 1, y, x]] = raw_pixels[idx + 1];
            input_tensor[[0, 2, y, x]] = raw_pixels[idx + 2];
        }
    }

    let input_tensor_dyn = input_tensor.into_dyn();
    let input_values = input_tensor_dyn.as_standard_layout();
    let input_tensor_ort = Tensor::from_array(input_values.into_owned())?;
    let mut session = encoder.lock().unwrap();
    let outputs = session.run(ort::inputs![input_tensor_ort])?;

    let embeddings = outputs[0].try_extract_array::<f32>()?.to_owned();

    Ok(ImageEmbeddings {
        path_hash: "".to_string(),
        embeddings: embeddings.into_dyn(),
        original_size: (orig_width, orig_height),
    })
}

pub fn run_sam_decoder(
    decoder: &Mutex<Session>,
    embeddings: &ImageEmbeddings,
    start_point: (f64, f64),
    end_point: (f64, f64),
    original_image: Option<&DynamicImage>,
) -> Result<GrayImage> {
    let (orig_width, orig_height) = embeddings.original_size;
    let long_side = orig_width.max(orig_height) as f64;
    let scale = SAM_INPUT_SIZE as f64 / long_side;

    let iters = 2;

    let is_point =
        (start_point.0 - end_point.0).abs() < 1e-6 && (start_point.1 - end_point.1).abs() < 1e-6;
    let mut point_coords = Vec::new();
    let mut point_labels = Vec::new();

    if is_point {
        point_coords.push((
            (start_point.0 * scale) as f32,
            (start_point.1 * scale) as f32,
        ));
        point_labels.push(1.0f32);
    } else {
        let x1 = (start_point.0.min(end_point.0) * scale) as f32;
        let y1 = (start_point.1.min(end_point.1) * scale) as f32;
        let x2 = (start_point.0.max(end_point.0) * scale) as f32;
        let y2 = (start_point.1.max(end_point.1) * scale) as f32;
        point_coords.push((x1, y1));
        point_coords.push((x2, y2));
        point_labels.push(2.0f32);
        point_labels.push(3.0f32);
    }

    let mut mask_input = Array::zeros((1, 1, 256, 256)).into_dyn();
    let mut has_mask_input = 0.0f32;

    let orig_im_size =
        Array::from_shape_vec((2,), vec![orig_height as f32, orig_width as f32])?.into_dyn();

    let mut final_mask_data: Vec<u8> = Vec::new();
    let mut final_w = 0;
    let mut final_h = 0;

    for i in 0..iters {
        let pc_len = point_coords.len();
        let pl_len = point_labels.len();

        let coords_flat: Vec<f32> = point_coords.iter().flat_map(|&(x, y)| vec![x, y]).collect();
        let coords_array = Array::from_shape_vec((1, pc_len, 2), coords_flat)?.into_dyn();
        let labels_array = Array::from_shape_vec((1, pl_len), point_labels.clone())?.into_dyn();

        let t_embeddings = Tensor::from_array(
            embeddings
                .embeddings
                .clone()
                .as_standard_layout()
                .into_owned(),
        )?;
        let t_point_coords = Tensor::from_array(coords_array.as_standard_layout().into_owned())?;
        let t_point_labels = Tensor::from_array(labels_array.as_standard_layout().into_owned())?;
        let t_mask_input =
            Tensor::from_array(mask_input.clone().as_standard_layout().into_owned())?;
        let t_has_mask = Tensor::from_array(
            Array::from_elem((1,), has_mask_input)
                .into_dyn()
                .as_standard_layout()
                .into_owned(),
        )?;
        let t_orig_im_size =
            Tensor::from_array(orig_im_size.clone().as_standard_layout().into_owned())?;

        let mask_tensor = {
            let mut session = decoder.lock().unwrap();
            let outputs = session.run(ort::inputs![
                t_embeddings,
                t_point_coords,
                t_point_labels,
                t_mask_input,
                t_has_mask,
                t_orig_im_size
            ])?;
            outputs[0].try_extract_array::<f32>()?.to_owned()
        };

        let mask_dims = mask_tensor.shape();
        let h = mask_dims[2];
        let w = mask_dims[3];
        let area = h * w;

        let mask_slice = mask_tensor.as_slice().unwrap();
        let first_mask_slice = &mask_slice[0..area];

        if i == iters - 1 {
            final_mask_data = first_mask_slice
                .iter()
                .map(|&val| if val > 0.0 { 255 } else { 0 })
                .collect();
            final_w = w;
            final_h = h;
            break;
        }

        let mut binary_mask = vec![false; area];
        let mut mask_area = 0.0;
        let mut min_x = w;
        let mut min_y = h;
        let mut max_x = 0;
        let mut max_y = 0;

        for (idx, &val) in first_mask_slice.iter().enumerate() {
            if val > 0.0 {
                binary_mask[idx] = true;
                let x = idx % w;
                let y = idx / w;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
                mask_area += 1.0;
            }
        }

        if mask_area == 0.0 || min_x > max_x {
            final_mask_data = first_mask_slice
                .iter()
                .map(|&val| if val > 0.0 { 255 } else { 0 })
                .collect();
            final_w = w;
            final_h = h;
            break;
        }

        let dt_in = edt_2d(&binary_mask, w, h);
        let mut max_in = 0.0;
        let mut pos_idx = 0;
        for (idx, &v) in dt_in.iter().enumerate() {
            if v > max_in {
                max_in = v;
                pos_idx = idx;
            }
        }
        let pos_y = pos_idx / w;
        let pos_x = pos_idx % w;

        let mut rev_mask = vec![false; area];
        for (idx, is_true) in binary_mask.iter().enumerate() {
            rev_mask[idx] = !is_true;
        }
        let mut dt_out = edt_2d(&rev_mask, w, h);

        for y in 0..h {
            for x in 0..w {
                if x < min_x || x > max_x || y < min_y || y > max_y {
                    dt_out[y * w + x] = 0.0;
                }
            }
        }

        let mut max_out = 0.0;
        let mut neg_idx = 0;
        for (idx, &v) in dt_out.iter().enumerate() {
            if v > max_out {
                max_out = v;
                neg_idx = idx;
            }
        }
        let neg_y = neg_idx / w;
        let neg_x = neg_idx % w;

        point_coords.clear();
        point_labels.clear();

        point_coords.push(((pos_x as f64 * scale) as f32, (pos_y as f64 * scale) as f32));
        point_labels.push(1.0);
        point_coords.push(((neg_x as f64 * scale) as f32, (neg_y as f64 * scale) as f32));
        point_labels.push(0.0);
        point_coords.push(((min_x as f64 * scale) as f32, (min_y as f64 * scale) as f32));
        point_labels.push(2.0);
        point_coords.push(((max_x as f64 * scale) as f32, (max_y as f64 * scale) as f32));
        point_labels.push(3.0);

        let mut gaus_dt = vec![0.0f32; area];
        let variance = (mask_area / 4.0_f32).max(1.0_f32);
        for (idx, &is_true) in binary_mask.iter().enumerate() {
            if is_true {
                let diff = dt_in[idx] - max_in;
                gaus_dt[idx] = (-(diff * diff) / variance).exp();
            }
        }

        let mask_f32_vec: Vec<f32> = first_mask_slice
            .iter()
            .map(|&v| if v > 0.0 { 15.0 } else { -15.0 })
            .collect();

        let img_mask_f32 =
            ImageBuffer::<Luma<f32>, Vec<f32>>::from_raw(w as u32, h as u32, mask_f32_vec).unwrap();
        let img_gaus_f32 =
            ImageBuffer::<Luma<f32>, Vec<f32>>::from_raw(w as u32, h as u32, gaus_dt).unwrap();

        let resized_mask = imageops::resize(&img_mask_f32, 256, 256, FilterType::Triangle);
        let resized_gaus = imageops::resize(&img_gaus_f32, 256, 256, FilterType::Triangle);

        let rm_raw = resized_mask.as_raw();
        let rg_raw = resized_gaus.as_raw();
        let mut mask_input_flat = vec![0.0f32; 256 * 256];

        for i in 0..(256 * 256) {
            let m_val = rm_raw[i];
            let mut g_val = rg_raw[i];
            if g_val <= 0.0 {
                g_val = 1.0;
            }
            mask_input_flat[i] = m_val * g_val;
        }

        mask_input = Array::from_shape_vec((1, 1, 256, 256), mask_input_flat)
            .unwrap()
            .into_dyn();
        has_mask_input = 1.0;
    }

    let gray_mask = GrayImage::from_raw(final_w as u32, final_h as u32, final_mask_data)
        .ok_or_else(|| anyhow::anyhow!("Failed to create mask image from raw data"))?;

    if let Some(img) = original_image {
        let (orig_width, orig_height) = img.dimensions();
        let guide_hr = img.to_luma8();

        let long_edge = orig_width.max(orig_height) as f32;
        let radius = (long_edge * 0.0075).clamp(8.0, 24.0);

        let resized_coarse_mask =
            image::imageops::resize(&gray_mask, orig_width, orig_height, FilterType::Triangle);

        let feathered_coarse_mask = image::imageops::blur(&resized_coarse_mask, radius / 3.0);

        let mut refined_mask =
            fast_guided_filter(&guide_hr, &feathered_coarse_mask, radius as usize, 0.01);

        for p in refined_mask.pixels_mut() {
            let v = p[0] as f32 / 255.0;
            p[0] = (((v - 0.03) / 0.94).clamp(0.0, 1.0) * 255.0).round() as u8;
        }

        Ok(refined_mask)
    } else {
        let feathered_mask = image::imageops::blur(&gray_mask, 2.0);
        let (orig_width, orig_height) = embeddings.original_size;
        let final_mask = image::imageops::resize(
            &feathered_mask,
            orig_width,
            orig_height,
            FilterType::Triangle,
        );

        Ok(final_mask)
    }
}

pub fn run_sky_seg_model(
    image: &DynamicImage,
    sky_seg_session: &Mutex<Session>,
) -> Result<GrayImage> {
    let (orig_width, orig_height) = image.dimensions();

    let resized_image = image.resize(SKYSEG_INPUT_SIZE, SKYSEG_INPUT_SIZE, FilterType::Triangle);
    let (resized_w, resized_h) = resized_image.dimensions();
    let resized_rgb = resized_image.into_rgb8();
    let raw_pixels = resized_rgb.as_raw();

    let paste_x = ((SKYSEG_INPUT_SIZE - resized_w) / 2) as usize;
    let paste_y = ((SKYSEG_INPUT_SIZE - resized_h) / 2) as usize;

    let mut input_tensor: Array<f32, _> =
        Array::zeros((1, 3, SKYSEG_INPUT_SIZE as usize, SKYSEG_INPUT_SIZE as usize));

    let mean = [0.485, 0.456, 0.406];
    let std = [0.229, 0.224, 0.225];

    let rw = resized_w as usize;
    let rh = resized_h as usize;

    for y in 0..rh {
        for x in 0..rw {
            let idx = (y * rw + x) * 3;
            let dest_y = y + paste_y;
            let dest_x = x + paste_x;

            input_tensor[[0, 0, dest_y, dest_x]] =
                (raw_pixels[idx] as f32 / 255.0 - mean[0]) / std[0];
            input_tensor[[0, 1, dest_y, dest_x]] =
                (raw_pixels[idx + 1] as f32 / 255.0 - mean[1]) / std[1];
            input_tensor[[0, 2, dest_y, dest_x]] =
                (raw_pixels[idx + 2] as f32 / 255.0 - mean[2]) / std[2];
        }
    }

    let input_tensor_dyn = input_tensor.into_dyn();
    let t_input = Tensor::from_array(input_tensor_dyn.as_standard_layout().into_owned())?;

    let mut session = sky_seg_session.lock().unwrap();
    let outputs = session.run(ort::inputs![t_input])?;
    let output_tensor = outputs[0].try_extract_array::<f32>()?.to_owned();
    let out_slice = output_tensor.as_slice().unwrap();

    let mut min_val = f32::MAX;
    let mut max_val = f32::MIN;
    for &v in out_slice {
        min_val = min_val.min(v);
        max_val = max_val.max(v);
    }

    let range = max_val - min_val;
    let scale = if range > 1e-6 { 255.0 / range } else { 0.0 };

    let usize_size = SKYSEG_INPUT_SIZE as usize;
    let mut cropped_mask_data = Vec::with_capacity(rw * rh);

    for y in 0..rh {
        let src_y = y + paste_y;
        for x in 0..rw {
            let src_x = x + paste_x;
            let val = out_slice[src_y * usize_size + src_x];
            let pixel = if range > 1e-6 {
                ((val - min_val) * scale) as u8
            } else {
                0
            };
            cropped_mask_data.push(pixel);
        }
    }

    let cropped_mask = GrayImage::from_raw(resized_w, resized_h, cropped_mask_data)
        .ok_or_else(|| anyhow::anyhow!("Failed to create mask from Sky Segmentation output"))?;

    let final_mask = imageops::resize(&cropped_mask, orig_width, orig_height, FilterType::Triangle);

    Ok(final_mask)
}

pub fn run_u2netp_model(
    image: &DynamicImage,
    u2netp_session: &Mutex<Session>,
) -> Result<GrayImage> {
    let (orig_width, orig_height) = image.dimensions();

    let resized_image = image.resize(U2NETP_INPUT_SIZE, U2NETP_INPUT_SIZE, FilterType::Triangle);
    let (resized_w, resized_h) = resized_image.dimensions();
    let resized_rgb = resized_image.into_rgb8();
    let raw_pixels = resized_rgb.as_raw();

    let paste_x = ((U2NETP_INPUT_SIZE - resized_w) / 2) as usize;
    let paste_y = ((U2NETP_INPUT_SIZE - resized_h) / 2) as usize;

    let mut input_tensor: Array<f32, _> =
        Array::zeros((1, 3, U2NETP_INPUT_SIZE as usize, U2NETP_INPUT_SIZE as usize));

    let mean = [0.485, 0.456, 0.406];
    let std = [0.229, 0.224, 0.225];

    let rw = resized_w as usize;
    let rh = resized_h as usize;

    for y in 0..rh {
        for x in 0..rw {
            let idx = (y * rw + x) * 3;
            let dest_y = y + paste_y;
            let dest_x = x + paste_x;

            input_tensor[[0, 0, dest_y, dest_x]] =
                (raw_pixels[idx] as f32 / 255.0 - mean[0]) / std[0];
            input_tensor[[0, 1, dest_y, dest_x]] =
                (raw_pixels[idx + 1] as f32 / 255.0 - mean[1]) / std[1];
            input_tensor[[0, 2, dest_y, dest_x]] =
                (raw_pixels[idx + 2] as f32 / 255.0 - mean[2]) / std[2];
        }
    }

    let input_tensor_dyn = input_tensor.into_dyn();
    let t_input = Tensor::from_array(input_tensor_dyn.as_standard_layout().into_owned())?;

    let mut session = u2netp_session.lock().unwrap();
    let outputs = session.run(ort::inputs![t_input])?;
    let output_tensor = outputs[0].try_extract_array::<f32>()?.to_owned();
    let out_slice = output_tensor.as_slice().unwrap();

    let mut min_val = f32::MAX;
    let mut max_val = f32::MIN;
    for &v in out_slice {
        min_val = min_val.min(v);
        max_val = max_val.max(v);
    }

    let range = max_val - min_val;
    let scale = if range > 1e-6 { 255.0 / range } else { 0.0 };

    let usize_size = U2NETP_INPUT_SIZE as usize;
    let mut cropped_mask_data = Vec::with_capacity(rw * rh);

    for y in 0..rh {
        let src_y = y + paste_y;
        for x in 0..rw {
            let src_x = x + paste_x;
            let val = out_slice[src_y * usize_size + src_x];
            let pixel = if range > 1e-6 {
                ((val - min_val) * scale) as u8
            } else {
                0
            };
            cropped_mask_data.push(pixel);
        }
    }

    let cropped_mask = GrayImage::from_raw(resized_w, resized_h, cropped_mask_data)
        .ok_or_else(|| anyhow::anyhow!("Failed to create mask from U-2-Netp output"))?;

    let final_mask = imageops::resize(&cropped_mask, orig_width, orig_height, FilterType::Triangle);

    Ok(final_mask)
}

pub fn run_depth_anything_model(
    image: &DynamicImage,
    depth_session: &Mutex<Session>,
) -> Result<GrayImage> {
    let resized_image = image.resize(DEPTH_INPUT_SIZE, DEPTH_INPUT_SIZE, FilterType::Triangle);
    let (resized_w, resized_h) = resized_image.dimensions();
    let resized_rgb = resized_image.into_rgb8();
    let raw_pixels = resized_rgb.as_raw();

    let paste_x = ((DEPTH_INPUT_SIZE - resized_w) / 2) as usize;
    let paste_y = ((DEPTH_INPUT_SIZE - resized_h) / 2) as usize;

    let mut input_tensor: Array<f32, _> =
        Array::zeros((1, 3, DEPTH_INPUT_SIZE as usize, DEPTH_INPUT_SIZE as usize));

    let mean = [0.485, 0.456, 0.406];
    let std = [0.229, 0.224, 0.225];

    let rw = resized_w as usize;
    let rh = resized_h as usize;

    for y in 0..rh {
        for x in 0..rw {
            let idx = (y * rw + x) * 3;
            let dest_y = y + paste_y;
            let dest_x = x + paste_x;

            input_tensor[[0, 0, dest_y, dest_x]] =
                (raw_pixels[idx] as f32 / 255.0 - mean[0]) / std[0];
            input_tensor[[0, 1, dest_y, dest_x]] =
                (raw_pixels[idx + 1] as f32 / 255.0 - mean[1]) / std[1];
            input_tensor[[0, 2, dest_y, dest_x]] =
                (raw_pixels[idx + 2] as f32 / 255.0 - mean[2]) / std[2];
        }
    }

    let input_tensor_dyn = input_tensor.into_dyn();
    let t_input = Tensor::from_array(input_tensor_dyn.as_standard_layout().into_owned())?;

    let mut session = depth_session.lock().unwrap();
    let outputs = session.run(ort::inputs![t_input])?;
    let output_tensor = outputs[0].try_extract_array::<f32>()?.to_owned();
    let out_slice = output_tensor.as_slice().unwrap();

    let usize_size = DEPTH_INPUT_SIZE as usize;

    let mut min_val = f32::MAX;
    let mut max_val = f32::MIN;
    for y in 0..rh {
        let src_y = y + paste_y;
        for x in 0..rw {
            let src_x = x + paste_x;
            let val = out_slice[src_y * usize_size + src_x];
            min_val = min_val.min(val);
            max_val = max_val.max(val);
        }
    }

    let range = max_val - min_val;
    let scale = if range > 1e-6 { 255.0 / range } else { 0.0 };

    let mut cropped_depth_data = Vec::with_capacity(rw * rh);

    for y in 0..rh {
        let src_y = y + paste_y;
        for x in 0..rw {
            let src_x = x + paste_x;
            let val = out_slice[src_y * usize_size + src_x];
            let pixel = if range > 1e-6 {
                ((val - min_val) * scale) as u8
            } else {
                0
            };
            cropped_depth_data.push(pixel);
        }
    }

    let depth_map = GrayImage::from_raw(resized_w, resized_h, cropped_depth_data)
        .ok_or_else(|| anyhow::anyhow!("Failed to create mask from Depth output"))?;

    Ok(depth_map)
}

fn build_normal_depth(points: &[f32], mask: &[f32]) -> Vec<u8> {
    let valid = |point: &[f32; 3], m: f32| {
        m >= 0.5 && point.iter().all(|v| v.is_finite()) && point[2] > 0.0
    };

    let mut depths: Vec<f32> = points
        .as_chunks::<3>()
        .0
        .iter()
        .zip(mask)
        .filter(|(point, m)| valid(point, **m))
        .map(|(point, _)| point[2])
        .collect();

    if depths.len() < 16 {
        return vec![255; mask.len()];
    }

    depths.sort_unstable_by(|a, b| a.total_cmp(b));
    let percentile = |p: f32| depths[((depths.len() - 1) as f32 * p) as usize];
    let near = percentile(0.005);
    let range = (percentile(0.995) - near).max(1e-6);

    points
        .as_chunks::<3>()
        .0
        .iter()
        .zip(mask)
        .map(|(point, m)| {
            if valid(point, *m) {
                (((point[2] - near) / range).clamp(0.0, 1.0) * 255.0).round() as u8
            } else {
                255
            }
        })
        .collect()
}

pub fn run_normal_model(
    image: &DynamicImage,
    normal_session: &Mutex<Session>,
) -> Result<RgbaImage> {
    let resized_image = if image.width().max(image.height()) > NORMAL_MAX_SIZE {
        image.resize(NORMAL_MAX_SIZE, NORMAL_MAX_SIZE, FilterType::Triangle)
    } else {
        image.clone()
    };
    let (resized_w, resized_h) = resized_image.dimensions();
    let resized_rgb = resized_image.into_rgb8();
    let raw_pixels = resized_rgb.as_raw();

    let rw = resized_w as usize;
    let rh = resized_h as usize;

    let mut input_tensor: Array<f32, _> = Array::zeros((1, 3, rh, rw));
    for y in 0..rh {
        for x in 0..rw {
            let idx = (y * rw + x) * 3;
            input_tensor[[0, 0, y, x]] = raw_pixels[idx] as f32 / 255.0;
            input_tensor[[0, 1, y, x]] = raw_pixels[idx + 1] as f32 / 255.0;
            input_tensor[[0, 2, y, x]] = raw_pixels[idx + 2] as f32 / 255.0;
        }
    }

    let t_image = Tensor::from_array(input_tensor.into_dyn())?;
    let t_num_tokens = Tensor::from_array(ndarray::arr0(NORMAL_NUM_TOKENS))?;

    let mut session = normal_session.lock().unwrap();
    let outputs = session.run(ort::inputs![
        "image" => t_image,
        "num_tokens" => t_num_tokens,
    ])?;

    let normal_tensor = outputs["normal"].try_extract_array::<f32>()?;
    let mask_tensor = outputs["mask"].try_extract_array::<f32>()?;
    let points_tensor = outputs["points"].try_extract_array::<f32>()?;

    if normal_tensor.shape() != [1, rh, rw, 3]
        || mask_tensor.shape() != [1, rh, rw]
        || points_tensor.shape() != [1, rh, rw, 3]
    {
        return Err(anyhow::anyhow!(
            "Unexpected normal model output shape: {:?} / {:?} / {:?}",
            normal_tensor.shape(),
            mask_tensor.shape(),
            points_tensor.shape()
        ));
    }

    let normals = normal_tensor.as_standard_layout();
    let normals = normals.as_slice().unwrap();
    let mask = mask_tensor.as_standard_layout();
    let mask = mask.as_slice().unwrap();
    let points = points_tensor.as_standard_layout();
    let depth = build_normal_depth(points.as_slice().unwrap(), mask);

    let pack = |v: f32| ((v * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;

    let mut normal_data = Vec::with_capacity(rw * rh * 4);
    for (n, d) in normals.as_chunks::<3>().0.iter().zip(depth) {
        let (nx, ny, nz) = (n[0], -n[1], -n[2]);
        let len = (nx * nx + ny * ny + nz * nz).sqrt();
        if len.is_finite() && len > 1e-6 {
            normal_data.extend_from_slice(&[pack(nx / len), pack(ny / len), pack(nz / len)]);
        } else {
            normal_data.extend_from_slice(&[128, 128, 255]);
        }
        normal_data.push(d);
    }

    RgbaImage::from_raw(resized_w, resized_h, normal_data)
        .ok_or_else(|| anyhow::anyhow!("Failed to create normal map from model output"))
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiSubjectMaskParameters {
    pub start_x: f64,
    pub start_y: f64,
    pub end_x: f64,
    pub end_y: f64,
    #[serde(default)]
    pub mask_data_base64: Option<String>,
    #[serde(default)]
    pub rotation: Option<f32>,
    #[serde(default)]
    pub flip_horizontal: Option<bool>,
    #[serde(default)]
    pub flip_vertical: Option<bool>,
    #[serde(default)]
    pub orientation_steps: Option<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiSkyMaskParameters {
    #[serde(default)]
    pub mask_data_base64: Option<String>,
    #[serde(default)]
    pub rotation: Option<f32>,
    #[serde(default)]
    pub flip_horizontal: Option<bool>,
    #[serde(default)]
    pub flip_vertical: Option<bool>,
    #[serde(default)]
    pub orientation_steps: Option<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiForegroundMaskParameters {
    #[serde(default)]
    pub mask_data_base64: Option<String>,
    #[serde(default)]
    pub rotation: Option<f32>,
    #[serde(default)]
    pub flip_horizontal: Option<bool>,
    #[serde(default)]
    pub flip_vertical: Option<bool>,
    #[serde(default)]
    pub orientation_steps: Option<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiDepthMaskParameters {
    #[serde(default)]
    pub min_depth: f32,
    #[serde(default)]
    pub max_depth: f32,
    #[serde(default)]
    pub min_fade: f32,
    #[serde(default)]
    pub max_fade: f32,
    #[serde(default)]
    pub feather: f32,
    #[serde(default)]
    pub mask_data_base64: Option<String>,
    #[serde(default)]
    pub rotation: Option<f32>,
    #[serde(default)]
    pub flip_horizontal: Option<bool>,
    #[serde(default)]
    pub flip_vertical: Option<bool>,
    #[serde(default)]
    pub orientation_steps: Option<u8>,
}

#[cfg(test)]
mod lama_texture_tests {
    use super::*;

    /// Between pixels Catmull-Rom blends; on a pixel it is that pixel, so a fill
    /// made at full size comes back unchanged.
    #[test]
    fn catmull_rom_on_a_pixel_is_that_pixel() {
        let (w, h) = (6, 5);
        let img: Vec<[f32; 3]> = (0..w * h)
            .map(|i| [i as f32 / 30.0, (i % 7) as f32 / 7.0, 0.5])
            .collect();
        for y in 0..h {
            for x in 0..w {
                let got = catmull_rom_sample(&img, w, h, x as f32, y as f32);
                for c in 0..3 {
                    assert!((got[c] - img[y * w + x][c]).abs() < 1e-6, "{x},{y}");
                }
            }
        }
    }

    #[test]
    fn dilating_grows_a_pixel_into_a_square() {
        let (w, h) = (9, 9);
        let mut mask = vec![false; w * h];
        mask[4 * w + 4] = true;
        let grown = dilate_mask(&mask, w, h, 2);
        for y in 0..h {
            for x in 0..w {
                let inside = (2..=6).contains(&x) && (2..=6).contains(&y);
                assert_eq!(grown[y * w + x], inside, "{x},{y}");
            }
        }
    }

    /// The texture a filled pixel takes always comes from outside the hole.
    #[test]
    fn every_filled_pixel_takes_its_texture_from_outside_the_hole() {
        let (w, h) = (48, 40);
        let img: Vec<[f32; 3]> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let v = ((x * 7 + y * 13) % 11) as f32 / 11.0;
                [v, 1.0 - v, (x % 5) as f32 / 5.0]
            })
            .collect();
        let hole: Vec<bool> = (0..w * h)
            .map(|i| (16..30).contains(&(i % w)) && (12..26).contains(&(i / w)))
            .collect();
        let is_target = dilate_mask(&hole, w, h, 1);
        let blocked = dilate_mask(&hole, w, h, LAMA_PATCH_RADIUS as usize + 2);
        let r = LAMA_PATCH_RADIUS;
        let is_source: Vec<bool> = (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as i32, (i / w) as i32);
                !blocked[i] && x >= r && y >= r && x < w as i32 - r && y < h as i32 - r
            })
            .collect();

        let nnf = lama_patch_match(&img, &img, w, h, &is_target, &is_source).unwrap();
        for i in 0..w * h {
            match nnf[i] {
                Some((sx, sy)) => {
                    assert!(is_target[i]);
                    assert!(
                        is_source[sy as usize * w + sx as usize],
                        "pixel {i} took {sx},{sy}"
                    );
                }
                None => assert!(!is_target[i]),
            }
        }
        // Run twice, same answer: a heal made again gives the same pixels.
        assert_eq!(
            nnf,
            lama_patch_match(&img, &img, w, h, &is_target, &is_source).unwrap()
        );
    }

    #[test]
    fn no_source_left_means_no_texture_step() {
        let (w, h) = (8, 8);
        let img = vec![[0.5f32; 3]; w * h];
        let all = vec![true; w * h];
        let none = vec![false; w * h];
        assert!(lama_patch_match(&img, &img, w, h, &all, &none).is_none());
    }
}
