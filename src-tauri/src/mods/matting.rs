//! Edge matting: turn a mask's rough boundary into the real one. Ours.
//!
//! WHY
//!
//! SAM decides *what* the object is, at 256×256 for the whole frame. Its edge
//! is that grid scaled up, which is why a small thing came back with a halo.
//! A matting model answers a narrower question at full resolution: given a
//! band where the edge must be, how much of each pixel is object? That is
//! what makes hair, a bracelet against a wrist, or the frame of a lamp come
//! out as themselves.
//!
//! THE MODEL
//!
//! ViTMatte-S, trained on Composition-1k, by hustvl (Apache-2.0):
//! https://huggingface.co/hustvl/vitmatte-small-composition-1k
//! The ONNX export used is Xenova's, unchanged:
//! https://huggingface.co/Xenova/vitmatte-small-composition-1k
//!
//! It is downloaded the first time the object brush is used, about 100 MB,
//! verified by SHA-256 and kept beside RapidRAW's own models. Until it is
//! there, or if the download fails, masks fall back to the old edge.
//!
//! Input is the photo (scaled to [-1, 1]) and a trimap (0 = background,
//! 0.5 = unknown, 1 = object) as a fourth channel, padded to a multiple of 32
//! — VitMatteImageProcessor's preprocessing. Output is the alpha.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result, anyhow};
use image::imageops::{self, FilterType};
use image::{GrayImage, Luma, RgbImage};
use imageproc::distance_transform::Norm;
use imageproc::morphology::{dilate, erode};
use ndarray::Array4;
use ort::session::Session;
use ort::value::Tensor;
use sha2::{Digest, Sha256};
use tauri::{Emitter, Manager};
use tokio::sync::Mutex as TokioMutex;

const URL: &str = "https://huggingface.co/Xenova/vitmatte-small-composition-1k/resolve/main/onnx/model.onnx?download=true";
const FILENAME: &str = "vitmatte_small_composition_1k.onnx";
const SHA256: &str = "bf28d2e0be2c073286e88d60ad649d7123da2749a2d99133fd1098d5887e0225";
/// What the download notice calls it.
const LABEL: &str = "Object edge model (100 MB)";

/// The long side a matte is computed at. Larger crops are matted at this size
/// and brought back up guided by the photo.
pub const MATTE_SIZE: u32 = 640;

static SESSION: OnceLock<Arc<Mutex<Session>>> = OnceLock::new();
static INIT: OnceLock<TokioMutex<()>> = OnceLock::new();

fn models_dir(app_handle: &tauri::AppHandle) -> Result<PathBuf> {
    let dir = app_handle.path().app_data_dir()?.join("models");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn verified(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1 << 16];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex::encode(hasher.finalize()) == SHA256)
}

async fn ensure_model(app_handle: &tauri::AppHandle) -> Result<PathBuf> {
    let path = models_dir(app_handle)?.join(FILENAME);
    if verified(&path)? {
        return Ok(path);
    }
    if path.exists() {
        fs::remove_file(&path)?;
    }

    // Their listener shows these as a download notice.
    let _ = app_handle.emit("ai-model-download-start", LABEL);
    let download = async {
        let bytes = reqwest::get(URL)
            .await
            .context("Could not download the object edge model; check your internet connection")?
            .error_for_status()
            .context("The object edge model server returned an error")?
            .bytes()
            .await
            .context("Could not read the downloaded object edge model")?;
        let temporary = path.with_extension("onnx.download");
        {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        if path.exists() {
            fs::remove_file(&path)?;
        }
        fs::rename(&temporary, &path)?;
        Ok::<(), anyhow::Error>(())
    }
    .await;
    let _ = app_handle.emit("ai-model-download-finish", LABEL);
    download?;

    if !verified(&path)? {
        let _ = fs::remove_file(&path);
        return Err(anyhow!("The object edge model failed its integrity check"));
    }
    Ok(path)
}

/// The matting model, downloaded and loaded on first use.
pub async fn session(app_handle: &tauri::AppHandle) -> Result<Arc<Mutex<Session>>> {
    if let Some(s) = SESSION.get() {
        return Ok(s.clone());
    }
    let _guard = INIT.get_or_init(|| TokioMutex::new(())).lock().await;
    if let Some(s) = SESSION.get() {
        return Ok(s.clone());
    }
    let path = ensure_model(app_handle).await?;
    let _ = ort::init().with_name("AI-Matting").commit();
    let session = load(&path)?;
    let _ = SESSION.set(session.clone());
    Ok(session)
}

/// Load the model from a file. Separate so tests can use a local copy.
pub fn load(path: &Path) -> Result<Arc<Mutex<Session>>> {
    let session = Session::builder()
        .context("Could not start the object edge model")?
        .commit_from_file(path)
        .context("Could not load the object edge model")?;
    Ok(Arc::new(Mutex::new(session)))
}

/// The trimap for a binary mask: object, background, and a band of
/// `band` pixels either side of the edge where the model decides.
pub fn trimap(mask: &GrayImage, band: u8) -> GrayImage {
    let sure = erode(mask, Norm::L2, band);
    let reach = dilate(mask, Norm::L2, band);
    let mut out = GrayImage::new(mask.width(), mask.height());
    for (x, y, p) in out.enumerate_pixels_mut() {
        p[0] = if sure.get_pixel(x, y)[0] > 0 {
            255
        } else if reach.get_pixel(x, y)[0] > 0 {
            128
        } else {
            0
        };
    }
    out
}

/// The alpha for `image` given its `trimap`, both the same size. Matted at
/// up to `MATTE_SIZE` on the long side; a larger image is matted smaller and
/// brought back up guided by its own edges. Definite object and definite
/// background stay exactly that.
pub fn matte(session: &Mutex<Session>, image: &RgbImage, trimap: &GrayImage) -> Result<GrayImage> {
    let (w, h) = image.dimensions();
    if (w, h) != trimap.dimensions() || w == 0 || h == 0 {
        return Err(anyhow!("matte: image and trimap differ in size"));
    }
    let scale = (MATTE_SIZE as f64 / w.max(h) as f64).min(1.0);
    let (mw, mh) = (
        ((w as f64 * scale).round() as u32).max(1),
        ((h as f64 * scale).round() as u32).max(1),
    );
    let (small_image, small_trimap) = if scale < 1.0 {
        (
            imageops::resize(image, mw, mh, FilterType::Triangle),
            imageops::resize(trimap, mw, mh, FilterType::Nearest),
        )
    } else {
        (image.clone(), trimap.clone())
    };

    // Padded to a multiple of 32 at the bottom and right, as the model expects.
    let (pw, ph) = (mw.div_ceil(32) * 32, mh.div_ceil(32) * 32);
    let mut input = Array4::<f32>::zeros((1, 4, ph as usize, pw as usize));
    for (x, y, p) in small_image.enumerate_pixels() {
        let (x, y) = (x as usize, y as usize);
        for c in 0..3 {
            input[[0, c, y, x]] = p[c] as f32 / 127.5 - 1.0;
        }
        input[[0, 3, y, x]] = small_trimap.get_pixel(x as u32, y as u32)[0] as f32 / 255.0;
    }

    let alpha: Vec<f32> = {
        let mut session = session.lock().unwrap();
        let outputs = session.run(ort::inputs![Tensor::from_array(input)?])?;
        outputs[0]
            .try_extract_array::<f32>()?
            .iter()
            .copied()
            .collect()
    };
    let mut small = GrayImage::new(mw, mh);
    for (x, y, p) in small.enumerate_pixels_mut() {
        let a = alpha[(y * pw + x) as usize];
        p[0] = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
    }

    let mut out = if scale < 1.0 {
        // The guided filter carries the matte's soft edge back up onto the
        // photo's own, rather than stretching it.
        let guide = imageops::grayscale(image);
        crate::ai_processing::fast_guided_filter(&guide, &small, 2, 1e-4)
    } else {
        small
    };
    for (x, y, p) in out.enumerate_pixels_mut() {
        match trimap.get_pixel(x, y)[0] {
            255 => *p = Luma([255]),
            0 => *p = Luma([0]),
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trimap_has_a_band_either_side_of_the_edge() {
        let mut mask = GrayImage::new(40, 40);
        for y in 10..30 {
            for x in 10..30 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
        let t = trimap(&mask, 3);
        assert_eq!(t.get_pixel(20, 20)[0], 255, "deep inside is object");
        assert_eq!(t.get_pixel(2, 2)[0], 0, "far outside is background");
        assert_eq!(t.get_pixel(10, 20)[0], 128, "on the edge is unknown");
        assert_eq!(t.get_pixel(8, 20)[0], 128, "just outside is unknown");
        assert_eq!(t.get_pixel(12, 20)[0], 128, "just inside is unknown");
    }
}
