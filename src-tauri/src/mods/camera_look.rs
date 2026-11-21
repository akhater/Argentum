//! Camera Look: a look fitted to the camera's own JPEG of a photo, over DNG
//! Standard. The fitting is the engine's (`argentum_engine::rm_camera_look`,
//! from RAWmakase); this module hands it the photo and keeps what it found.
//!
//! - The photo is decoded as the app opens it
//!   (`image_loader::load_base_image_from_bytes`), reduced to the stage's
//!   size and corrected to the camera's colour as every render is first
//!   (`profile_correction::rows_for`): what the RAW tone stage receives,
//!   before any edit. `tests::sheet` holds the GPU's stage to it.
//! - The JPEG is the largest one the file embeds, upright
//!   (`image_loader::embedded_preview_fallback`), in sRGB or Adobe RGB as the
//!   file's EXIF says (`preview`); any other colour space is refused.
//! - The base is the photo's DNG Standard curve (`dng_standard`), evaluated
//!   as the GPU evaluates it (`super_resolution::apply_curve`).
//! - A refused fit keeps Auto-Matched's curve for the photo
//!   (`raw_tone::curve_for`), which the photo renders with instead, and why.
//!
//! A photo is fitted once: the outcome is cached on disk in the app's cache
//! folder (`camera_look/`), keyed as the decoded photo is in memory
//! (`cache_utils::DecodeFingerprint`: the file's path, length and time, and
//! the decode settings) and by the fit's version, and kept in memory for the
//! few photos last asked about. One caller fits a file at a time; another
//! waits for it and reads its result (RAWmakase `camera_look/mod.rs`
//! `claim`, v0.7.1 @ 78069aa3, MIT: 5b0b9eee, da906057), except on a Rayon
//! thread, which never waits.

use crate::app_settings::AppSettings;
use crate::cache_utils::DecodeFingerprint;
use crate::mods::raw_tone::ToneCurvePoint;
use argentum_engine::rm_camera_look::{
    self as engine, FIT_VERSION, Kind, Look, Preview, Report, Stage, Tone, pairs, pairs::Space,
};
use image::{DynamicImage, GenericImageView};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

/// The RAW tone rendering's name for it, as edits store it.
pub const MODE: &str = "cameraLook";

/// What fitting a photo came to.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "outcome")]
pub enum Outcome {
    /// A look that beats DNG Standard on the photo's held-out tiles.
    Fitted {
        look: Look,
        kind: Kind,
        report: Box<Report>,
    },
    /// No look did, or the photo could not be fitted: it renders with
    /// Auto-Matched's curve, `fallback` (empty when that failed too).
    Refused {
        reason: String,
        fallback: Vec<ToneCurvePoint>,
    },
}

/// The cache folder, set once the app knows it (`startup::init`).
static CACHE_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn initialize_cache_dir(app_cache: &Path) {
    let _ = CACHE_DIR.set(app_cache.join("camera_look"));
}

/// An outcome kept in memory: its cache key, the file as it was (`file_stamp`),
/// its look's number (0 for none).
struct Kept {
    key: String,
    stamp: String,
    number: u32,
    outcome: Arc<Outcome>,
}

/// The photos last asked about, newest last, and the most kept: an outcome
/// is about 75 KB.
static KEPT: Mutex<VecDeque<Kept>> = Mutex::new(VecDeque::new());
const KEPT_MOST: usize = 8;

/// The photo at `path`'s outcome: kept, cached, or fitted now and cached.
/// Slow on a first call (a decode and a few seconds of fitting): never call
/// it on the UI thread.
pub fn outcome_for(path: &str, settings: &AppSettings) -> Result<Arc<Outcome>, String> {
    let (source, _) = crate::file_management::parse_virtual_path(path);
    let key = key_of(&source, settings).ok_or("The photo can't be read")?;
    let stamp = file_stamp(&source).unwrap_or_default();
    if let Some(found) = kept(&key) {
        return Ok(found);
    }
    let dir = CACHE_DIR.get().cloned();
    if let Some(found) = dir.as_deref().and_then(|d| load(d, &key)) {
        return Ok(keep(key, stamp, found));
    }
    // One fit of a file at a time where waiting is safe; another caller's
    // result is read from the cache once it is done.
    let _turn = claim(&key);
    if let Some(found) = kept(&key) {
        return Ok(found);
    }
    if let Some(found) = dir.as_deref().and_then(|d| load(d, &key)) {
        return Ok(keep(key, stamp, found));
    }
    // A fit that panics must not stop the photo rendering: it renders as
    // it would without one, and is tried again next time.
    let source_text = source.to_string_lossy().to_string();
    let Ok(outcome) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        fit_file(&source_text, settings)
    })) else {
        // Kept, so renders don't try again at every slider tick; not cached,
        // so the next run of the app does.
        let reason = format!("Fitting the camera look of {source_text} stopped unexpectedly");
        log::error!("Camera Look: {reason}");
        return Ok(keep(
            key,
            stamp,
            Outcome::Refused {
                reason,
                fallback: Vec::new(),
            },
        ));
    };
    // A file replaced while it was fitted: the outcome may be of either.
    if key_of(&source, settings).as_ref() != Some(&key) {
        return Err("The photo changed while it was fitted".into());
    }
    if let Some(dir) = dir
        && let Err(e) = store(&dir, &key, &outcome)
    {
        log::warn!("Camera Look: the fit of {source_text} isn't cached: {e}");
    }
    Ok(keep(key, stamp, outcome))
}

/// What the interface says about a photo's Camera Look: whether the look is
/// used, and if not, why, the photo rendering with Auto-Matched instead.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "CameraLookStatus"))]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub fitted: bool,
    pub reason: Option<String>,
}

/// The photo's Camera Look, fitted now if it never was.
pub fn status_of(path: &str, settings: &AppSettings) -> Result<Status, String> {
    Ok(match outcome_for(path, settings)?.as_ref() {
        Outcome::Fitted { .. } => Status {
            fitted: true,
            reason: None,
        },
        Outcome::Refused { reason, .. } => Status {
            fitted: false,
            reason: Some(reason.clone()),
        },
    })
}

/// The settings file, so a render can read the settings a fit decodes with
/// (`startup::init`).
static SETTINGS_FILE: OnceLock<PathBuf> = OnceLock::new();

pub fn remember_settings_file(path: PathBuf) {
    let _ = SETTINGS_FILE.set(path);
}

/// The app's settings as saved: the defaults when there is no file yet or it
/// does not read, none when the app never said where it is (tests) or the
/// file can't be opened.
fn saved_settings() -> Option<AppSettings> {
    let path = SETTINGS_FILE.get()?;
    match std::fs::read_to_string(path) {
        // A file the app would salvage (mods/salvage.rs) decodes with the defaults.
        Ok(text) => Some(serde_json::from_str(&text).unwrap_or_default()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(AppSettings::default()),
        Err(_) => None,
    }
}

/// Which look a render uses, as the shader's `GlobalAdjustments` carries it
/// (modules.wgsl `AgCameraLook`): the kept look's number, 0 for none. The
/// look itself goes to the GPU in its own buffer (`gpu_data`). 16 bytes.
#[repr(C)]
#[derive(
    Serialize,
    Deserialize,
    Debug,
    Clone,
    Copy,
    bytemuck::Pod,
    bytemuck::Zeroable,
    Default,
    PartialEq,
)]
pub struct Params {
    pub look: u32,
    _pad: [u32; 3],
}

impl Params {
    pub fn of(look: u32) -> Self {
        Self {
            look,
            ..Self::default()
        }
    }
}

/// How a render draws Camera Look for a photo.
pub enum Rendering {
    /// The fitted look, by number (`Params::look`).
    Look(u32),
    /// Auto-Matched's curve, the fit having been refused.
    Fallback(Vec<ToneCurvePoint>),
    /// Nothing to draw it with (no photo, no app, a fit that failed to run
    /// and no fallback): DNG Standard, the base it would have sat on.
    Standard,
}

/// How to render Camera Look for `photo`: the outcome kept for it, else read
/// from the cache or fitted now with the app's settings. A render on a photo
/// nobody fitted yet (a pasted rendering, an export) waits for its fit.
pub fn rendering_for(photo: Option<&str>) -> Rendering {
    let Some(photo) = photo else {
        return Rendering::Standard;
    };
    let outcome = kept_for_photo(photo).or_else(|| {
        let settings = saved_settings()?;
        outcome_for(photo, &settings)
            .map_err(|e| log::warn!("Camera Look: {e}"))
            .ok()
    });
    match outcome.as_deref() {
        Some(o @ Outcome::Fitted { .. }) => Rendering::Look(number_of(o)),
        Some(Outcome::Refused { fallback, .. }) if fallback.len() >= 2 => {
            Rendering::Fallback(fallback.clone())
        }
        _ => Rendering::Standard,
    }
}

/// The look numbered `number` among those kept, as the shader's buffer holds
/// it.
pub fn gpu_data(number: u32) -> Option<Vec<f32>> {
    let kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    kept.iter()
        .find(|k| k.number == number && number != 0)
        .and_then(|k| match k.outcome.as_ref() {
            Outcome::Fitted { look, .. } => Some(look.to_gpu()),
            Outcome::Refused { .. } => None,
        })
}

/// The floats `gpu_data` hands the shader.
pub const GPU_LEN: usize =
    engine::TABLE_DIMS[0] * engine::TABLE_DIMS[1] * engine::TABLE_DIMS[2] * 3
        + engine::CURVE_SAMPLES
        + 3;

/// An outcome's look's number: a hash of the look, never 0; 0 for none.
fn number_of(outcome: &Outcome) -> u32 {
    let Outcome::Fitted { look, .. } = outcome else {
        return 0;
    };
    let bytes: Vec<u8> = look.to_gpu().iter().flat_map(|v| v.to_le_bytes()).collect();
    let hash = blake3::hash(&bytes);
    u32::from_le_bytes(hash.as_bytes()[..4].try_into().expect("4 bytes")).max(1)
}

/// The outcome kept for the file at `photo` as it is now (its length and
/// time), if any: what a render asks for at every slider tick.
fn kept_for_photo(photo: &str) -> Option<Arc<Outcome>> {
    let (source, _) = crate::file_management::parse_virtual_path(photo);
    let stamp = file_stamp(&source)?;
    let kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    kept.iter()
        .rev()
        .find(|k| k.stamp == stamp)
        .map(|k| k.outcome.clone())
}

/// A file's path, length and time: which photo, as it is now.
fn file_stamp(source: &Path) -> Option<String> {
    let meta = std::fs::metadata(source).ok()?;
    let nanos = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    Some(format!(
        "{}\0{}\0{nanos}",
        source.to_string_lossy(),
        meta.len()
    ))
}

/// The cache's name for a photo: its decode's fingerprint and the fit's version.
fn key_of(source: &Path, settings: &AppSettings) -> Option<String> {
    let fingerprint = DecodeFingerprint::for_file(source, settings)?;
    let text = format!(
        "v{FIT_VERSION}\0{}\0{}",
        source.to_string_lossy(),
        fingerprint.key_text()
    );
    Some(blake3::hash(text.as_bytes()).to_hex().to_string())
}

fn kept(key: &str) -> Option<Arc<Outcome>> {
    let kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    kept.iter()
        .find(|k| k.key == key)
        .map(|k| k.outcome.clone())
}

fn keep(key: String, stamp: String, outcome: Outcome) -> Arc<Outcome> {
    let number = number_of(&outcome);
    let outcome = Arc::new(outcome);
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    kept.retain(|k| k.key != key && k.stamp != stamp);
    if kept.len() >= KEPT_MOST {
        kept.pop_front();
    }
    kept.push_back(Kept {
        key,
        stamp,
        number,
        outcome: outcome.clone(),
    });
    outcome
}

fn file_in(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("{key}.json"))
}

/// A cached outcome; none for one that can't be read or whose look is not
/// whole.
fn load(dir: &Path, key: &str) -> Option<Outcome> {
    let bytes = std::fs::read(file_in(dir, key)).ok()?;
    match serde_json::from_slice::<Outcome>(&bytes).ok()? {
        Outcome::Fitted { look, .. } if !look.is_valid() => None,
        outcome => Some(outcome),
    }
}

fn store(dir: &Path, key: &str, outcome: &Outcome) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_vec(outcome).map_err(|e| e.to_string())?;
    crate::mods::atomic_write::write_atomic(&file_in(dir, key), json).map_err(|e| e.to_string())
}

/// The files being fitted, by key, and a wake-up for each fit that ends.
static FITTING: (Mutex<Vec<String>>, Condvar) = (Mutex::new(Vec::new()), Condvar::new());

/// A file's fit, claimed by one caller until dropped.
struct Claim(String);
impl Drop for Claim {
    fn drop(&mut self) {
        let mut fitting = FITTING.0.lock().unwrap_or_else(|e| e.into_inner());
        fitting.retain(|key| *key != self.0);
        FITTING.1.notify_all();
    }
}

/// Claims the fit of `key`, waiting while another caller fits the same file;
/// `None` for a caller on a Rayon thread while another fits it, which fits it
/// too instead of waiting: a fit waits on its pool's work, and if a thread
/// running part of that work waited for the fit, neither would end.
fn claim(key: &str) -> Option<Claim> {
    let may_wait = rayon::current_thread_index().is_none();
    let mut fitting = FITTING.0.lock().unwrap_or_else(|e| e.into_inner());
    while fitting.iter().any(|k| k == key) {
        if !may_wait {
            return None;
        }
        fitting = FITTING.1.wait(fitting).unwrap_or_else(|e| e.into_inner());
    }
    fitting.push(key.to_string());
    Some(Claim(key.to_string()))
}

/// Fits the photo at `path`, falling back to Auto-Matched with the reason
/// when no look is used.
fn fit_file(path: &str, settings: &AppSettings) -> Outcome {
    let started = std::time::Instant::now();
    let result = std::fs::read(path)
        .map_err(|e| e.to_string())
        .and_then(|bytes| fit_bytes(&bytes, path, settings).map(|fit| (fit, bytes)));
    let outcome = match result {
        Ok((Ok((look, report)), _)) => Outcome::Fitted {
            look,
            kind: report.kind.unwrap_or(Kind::Colour),
            report: Box::new(report),
        },
        Ok((Err(reason), bytes)) => Outcome::Refused {
            fallback: auto_matched(&bytes, path, settings),
            reason,
        },
        Err(reason) => Outcome::Refused {
            reason,
            fallback: Vec::new(),
        },
    };
    log::info!(
        "Camera Look: {path} {} in {:.1} s",
        match &outcome {
            Outcome::Fitted { kind, .. } => format!("fitted ({kind:?})"),
            Outcome::Refused { reason, .. } => format!("refused ({reason})"),
        },
        started.elapsed().as_secs_f32()
    );
    outcome
}

fn auto_matched(bytes: &[u8], path: &str, settings: &AppSettings) -> Vec<ToneCurvePoint> {
    crate::mods::raw_tone::curve_for(bytes, path, "autoMatched", settings).unwrap_or_else(|e| {
        log::warn!("Camera Look: no Auto-Matched curve for {path} either: {e}");
        Vec::new()
    })
}

/// The engine's fit of a RAW's bytes: the look and what was measured, or why
/// there is none.
pub fn fit_bytes(
    bytes: &[u8],
    path: &str,
    settings: &AppSettings,
) -> Result<Result<(Look, Report), String>, String> {
    let preview = preview(bytes, path)?;
    let decoded =
        crate::image_loader::load_base_image_from_bytes(bytes, path, false, settings, None)
            .map_err(|e| format!("The photo can't be decoded: {e}"))?;
    let stage = stage_of(&decoded, path);
    let base = base_tone(path);
    Ok(engine::fit(&stage, preview, &base).map_err(|e| e.reason))
}

/// The photo's DNG Standard tone: its curve, as the GPU evaluates it, on
/// linear light.
pub fn base_tone(path: &str) -> Tone {
    let points: Vec<(f32, f32)> = crate::mods::dng_standard::curve_for_photo(Some(path))
        .into_iter()
        .map(|p| (p.x, p.y))
        .collect();
    let curve = |x: f32| {
        use argentum_engine::colour::{srgb_decode, srgb_encode};
        srgb_decode(crate::mods::super_resolution::apply_curve(
            srgb_encode(x),
            &points,
        ))
    };
    Tone::from_fn(curve)
}

/// The decoded photo reduced to the stage's size (`pairs::PROXY_EDGE` times
/// `pairs::RENDER_FACTOR` on the long edge), as the RAW tone stage receives
/// it with no edit: corrected to the camera's colour the DNG way, as every
/// render does first (`profile_correction::rows_for`); and which pixels reach
/// the sensor's white.
pub fn stage_of(decoded: &DynamicImage, path: &str) -> Stage {
    let long = pairs::PROXY_EDGE * pairs::RENDER_FACTOR;
    let (w, h) = decoded.dimensions();
    let (sw, sh) = if w >= h {
        (long, (h as f32 * long as f32 / w as f32).round() as u32)
    } else {
        ((w as f32 * long as f32 / h as f32).round() as u32, long)
    };
    let small = crate::image_processing::downscale_f32_image(decoded, sw, sh).to_rgb32f();
    let (sw, sh) = small.dimensions();
    let (r0, r1, r2) =
        crate::mods::profile_correction::rows_for(&serde_json::json!({}), Some(path));
    let rows = [r0, r1, r2].map(|r| [r[0], r[1], r[2]]);
    let linear: Vec<[f32; 3]> = small
        .pixels()
        .map(|p| argentum_engine::colour::mul(rows, p.0))
        .collect();
    // A stage pixel is clipped when any decoded pixel it covers reached the
    // decode's white, 1, where DNG Standard caps each channel: averaged, a
    // clipped speck would pass for an ordinary colour.
    let mut clipped = vec![false; linear.len()];
    let converted;
    let full = match decoded.as_rgb32f() {
        Some(full) => full,
        None => {
            converted = decoded.to_rgb32f();
            &converted
        }
    };
    for (x, y, p) in full.enumerate_pixels() {
        if p.0.iter().any(|v| *v >= 0.99) {
            let (sx, sy) = (
                (x as u64 * sw as u64 / w as u64) as u32,
                (y as u64 * sh as u64 / h as u64) as u32,
            );
            clipped[(sy.min(sh - 1) * sw + sx.min(sw - 1)) as usize] = true;
        }
    }
    Stage {
        width: sw,
        height: sh,
        clipped,
        linear,
    }
}

/// The camera's JPEG of the RAW, upright, or why it can't be used: none,
/// too small, or in a colour space the file names that is neither sRGB nor
/// "uncalibrated", which a camera set to Adobe RGB writes (the DCF rule
/// RAWmakase's `stated_space` follows). A file that names none is sRGB.
fn preview(bytes: &[u8], path: &str) -> Result<Preview, String> {
    let exif = crate::exif_processing::read_exif_data_from_bytes(path, bytes);
    let space = match exif
        .get("ColorSpace")
        .map(|s| s.trim().to_ascii_lowercase())
    {
        None => Space::Srgb,
        Some(s) if s == "1" || s == "srgb" => Space::Srgb,
        Some(s) if s == "65535" || s == "uncalibrated" => Space::AdobeRgb,
        Some(s) => {
            return Err(format!(
                "The camera's JPEG is in an unknown colour space ({s})"
            ));
        }
    };
    let jpeg = crate::image_loader::safe_embedded_preview_fallback(bytes, path)
        .ok_or("The photo has no embedded JPEG")?
        .to_rgb8();
    let (w, h) = jpeg.dimensions();
    if w.max(h) < 2 * pairs::PROXY_EDGE {
        return Err(format!("The camera's JPEG is only {w} x {h}"));
    }
    Ok(Preview::from_rgb8(w, h, jpeg.as_raw(), space))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fitted() -> Outcome {
        Outcome::Fitted {
            look: Look::identity(),
            kind: Kind::Colour,
            report: Box::default(),
        }
    }

    #[test]
    fn an_outcome_comes_back_from_the_cache_as_it_went_in() {
        let dir = std::env::temp_dir().join(format!("argentum-camera-look-{}", std::process::id()));
        store(&dir, "a", &fitted()).unwrap();
        assert!(matches!(
            load(&dir, "a"),
            Some(Outcome::Fitted {
                kind: Kind::Colour,
                ..
            })
        ));
        let refused = Outcome::Refused {
            reason: "no JPEG".into(),
            fallback: vec![
                ToneCurvePoint { x: 0., y: 0. },
                ToneCurvePoint { x: 255., y: 255. },
            ],
        };
        store(&dir, "b", &refused).unwrap();
        assert!(
            matches!(load(&dir, "b"), Some(Outcome::Refused { fallback, .. }) if fallback.len() == 2)
        );
        // A look that is not whole is no outcome: the photo is fitted again.
        let mut broken = Look::identity();
        broken.curve.pop();
        store(
            &dir,
            "c",
            &Outcome::Fitted {
                look: broken,
                kind: Kind::Colour,
                report: Box::default(),
            },
        )
        .unwrap();
        assert!(load(&dir, "c").is_none());
        assert!(load(&dir, "missing").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The key follows the file and the decode settings, as the decoded
    /// photo's does.
    #[test]
    fn a_rewritten_file_or_other_decode_settings_is_another_key() {
        let dir =
            std::env::temp_dir().join(format!("argentum-camera-look-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("photo.cr3");
        std::fs::write(&path, b"first shot").unwrap();
        let settings = AppSettings::default();
        let first = key_of(&path, &settings).unwrap();
        assert_eq!(key_of(&path, &settings).unwrap(), first);
        std::fs::write(&path, b"second shot, longer").unwrap();
        assert_ne!(key_of(&path, &settings).unwrap(), first);
        let other = AppSettings {
            raw_highlight_compression: Some(1.0),
            ..AppSettings::default()
        };
        assert_ne!(key_of(&path, &other), key_of(&path, &settings));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// While a file is fitted, another caller for it waits, and goes on once
    /// the first has finished; a Rayon thread never waits.
    #[test]
    fn a_file_is_fitted_by_one_caller_at_a_time() {
        let key = format!("one-at-a-time-{}", std::process::id());
        let first = claim(&key).expect("free");
        let waiting = std::thread::spawn({
            let key = key.clone();
            move || claim(&key).is_some()
        });
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert!(
            !waiting.is_finished(),
            "a second fit started beside the first"
        );
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap();
        let on_rayon = pool.install(|| claim(&key).is_none());
        assert!(on_rayon, "a Rayon thread waited");
        drop(first);
        assert!(waiting.join().unwrap());
    }

    /// The fit on real RAWs, against the renderings it sits beside. For each:
    /// how long the decode and the fit take and what the fit decided; ΔE00 to
    /// the camera's JPEG of the app's own renders (GPU, default sharpening) in
    /// DNG Standard, Auto-Matched and Camera Look, on every usable pixel and
    /// on the held-out ones; the skin hue error, on the JPEG's skin-coloured
    /// pixels; banding, as the largest step a fine ramp of skin tones makes
    /// through the look against DNG Standard's; and the GPU's Camera Look
    /// against the CPU model the fit is solved with (sharpening off).
    ///
    /// ```text
    /// AG_RAWS="a.CR3;b.CR2" cargo test --lib mods::camera_look::tests::sheet -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore]
    fn sheet() {
        use crate::mods::look_check::render_tier;
        use argentum_engine::colour::{
            PROPHOTO_TO_SRGB, SRGB_TO_PROPHOTO, delta_e2000, lab, mul, srgb_decode,
        };
        let raws = std::env::var("AG_RAWS").expect("AG_RAWS");
        let settings = AppSettings::default();
        let (ctx, _) = render_tier::device().expect("a GPU");
        let lut = Arc::new(crate::lut_processing::Lut {
            size: 0,
            data: Vec::new(),
        });
        for path in raws.split(';').filter(|p| !p.is_empty()) {
            let name = Path::new(path).file_name().unwrap().to_string_lossy();
            let bytes = std::fs::read(path).unwrap();
            let started = std::time::Instant::now();
            let decoded = crate::image_loader::load_base_image_from_bytes(
                &bytes, path, false, &settings, None,
            )
            .unwrap();
            let decode_s = started.elapsed().as_secs_f32();
            let started = std::time::Instant::now();
            let stage = stage_of(&decoded, path);
            let base = base_tone(path);
            let fit = preview(&bytes, path).map(|p| engine::fit(&stage, p, &base));
            let fit_s = started.elapsed().as_secs_f32();
            let look = match &fit {
                Ok(Ok((look, report))) => {
                    println!(
                        "{name}: {:?}, decode {decode_s:.1} s, fit {fit_s:.1} s",
                        report.kind
                    );
                    look.clone()
                }
                Ok(Err(e)) => {
                    println!("{name}: refused: {}", e.reason);
                    continue;
                }
                Err(e) => {
                    println!("{name}: refused: {e}");
                    continue;
                }
            };
            // Kept as a render finds it.
            let source = Path::new(path);
            keep(
                key_of(source, &settings).unwrap(),
                file_stamp(source).unwrap(),
                Outcome::Fitted {
                    look: look.clone(),
                    kind: Kind::Colour,
                    report: Box::default(),
                },
            );
            // The app's renders of the photo at the stage's size.
            let small =
                crate::image_processing::downscale_f32_image(&decoded, stage.width, stage.height);
            assert_eq!((small.width(), small.height()), (stage.width, stage.height));
            let renderer = crate::mods::export_precision::HighPrecisionRenderer::new(
                &ctx,
                stage.width,
                stage.height,
            )
            .unwrap();
            let auto =
                crate::mods::raw_tone::curve_for(&bytes, path, "autoMatched", &settings).unwrap();
            let render = |keys: serde_json::Value| -> Vec<[f32; 3]> {
                let edit = render_tier::merged(&render_tier::opening_edit(&bytes), &keys);
                let out = render_tier::render(
                    &renderer,
                    &small,
                    render_tier::request(&edit, &bytes, path, &settings, &lut),
                )
                .unwrap();
                out.pixels()
                    .map(|p| [p[0], p[1], p[2]].map(srgb_decode))
                    .collect()
            };
            let renders = [
                render(serde_json::json!({ "rawToneRendering": "dngStandard" })),
                render(serde_json::json!({
                    "rawToneRendering": "autoMatched",
                    "rawToneCurve": auto,
                    "rawToneCurveMode": "autoMatched",
                })),
                render(serde_json::json!({ "rawToneRendering": MODE })),
            ];
            let jpeg = preview(&bytes, path).unwrap();
            let s = engine::measure(&stage, jpeg.clone(), &renders).unwrap();
            println!(
                "  ΔE00 to the JPEG, all/held-out: DNG Standard {:.2}/{:.2}, Auto-Matched {:.2}/{:.2}, Camera Look {:.2}/{:.2} ({} px)",
                s[0][0].de00,
                s[0][1].de00,
                s[1][0].de00,
                s[1][1].de00,
                s[2][0].de00,
                s[2][1].de00,
                s[0][0].pairs
            );
            let skin = engine::skin_hue_error(&stage, jpeg, &renders).unwrap();
            println!(
                "  skin hue error (degrees, {} px): DNG Standard {:.2}, Auto-Matched {:.2}, Camera Look {:.2}",
                skin.0, skin.1[0], skin.1[1], skin.1[2]
            );
            // Banding: smooth ramps of skin tones, each 2048 steps: lightness
            // at three saturations, and saturation at three lightnesses. A
            // look that steps between table nodes shows as a largest step
            // well above DNG Standard's along the same ramp.
            let skin = |v: f32, s: f32| {
                mul(
                    SRGB_TO_PROPHOTO,
                    [v, v * (1. - 0.45 * s), v * (1. - 0.6 * s)],
                )
            };
            let steps = 2048;
            let ramps: Vec<Vec<[f32; 3]>> = [0.15f32, 0.3, 0.45]
                .iter()
                .map(|s| {
                    (0..steps)
                        .map(|i| skin(0.02 + 0.6 * i as f32 / (steps - 1) as f32, *s))
                        .collect()
                })
                .chain([0.08f32, 0.2, 0.45].iter().map(|v| {
                    (0..steps)
                        .map(|i| skin(*v, 0.05 + 0.6 * i as f32 / (steps - 1) as f32))
                        .collect()
                }))
                .collect();
            let worst_step = |f: &dyn Fn([f32; 3]) -> [f32; 3]| {
                ramps
                    .iter()
                    .map(|ramp| {
                        let made: Vec<[f64; 3]> = ramp
                            .iter()
                            .map(|p| lab(mul(PROPHOTO_TO_SRGB, f(*p)).map(|v| v.clamp(0., 1.))))
                            .collect();
                        made.windows(2)
                            .map(|w| delta_e2000(w[0], w[1]))
                            .fold(0f64, f64::max)
                    })
                    .fold(0f64, f64::max)
            };
            let identity = Look::identity();
            println!(
                "  banding, largest ΔE00 step along skin ramps of {steps}: DNG Standard {:.3}, Camera Look {:.3}",
                worst_step(&|p| identity.render(&base, p)),
                worst_step(&|p| look.render(&base, p)),
            );
            // The GPU's Camera Look against the CPU model, sharpening off.
            let gpu = render(
                serde_json::json!({ "rawToneRendering": MODE, "agSharpen": { "enabled": false } }),
            );
            let mut des: Vec<f64> = gpu
                .iter()
                .zip(&stage.linear)
                .map(|(g, q)| {
                    let model = mul(
                        PROPHOTO_TO_SRGB,
                        look.render(&base, mul(SRGB_TO_PROPHOTO, *q)),
                    );
                    delta_e2000(lab(*g), lab(model.map(|v| v.clamp(0., 1.))))
                })
                .collect();
            let mean = des.iter().sum::<f64>() / des.len() as f64;
            des.sort_by(f64::total_cmp);
            println!(
                "  GPU Camera Look vs the CPU model: ΔE00 mean {mean:.2}, median {:.2}, p99 {:.2}",
                des[des.len() / 2],
                des[des.len() * 99 / 100],
            );
        }
    }

    /// The shader reads the look where `Look::to_gpu` puts it, and the
    /// buffer holds it all.
    #[test]
    fn the_shader_reads_the_look_where_it_is_laid_out() {
        let wgsl = include_str!("../shaders/modules.wgsl");
        let nodes = engine::TABLE_DIMS.iter().product::<usize>();
        assert!(wgsl.contains(&format!("const AG_LOOK_CURVE_AT = {}u;", nodes * 3)));
        assert!(wgsl.contains(&format!(
            "const AG_LOOK_GAINS_AT = {}u;",
            nodes * 3 + engine::CURVE_SAMPLES
        )));
        assert!(wgsl.contains(&format!(
            "const AG_LOOK_CURVE_STEPS = {}u;",
            engine::CURVE_SAMPLES - 1
        )));
        assert_eq!(Look::identity().to_gpu().len(), GPU_LEN);
        assert_eq!(std::mem::size_of::<Params>(), 16);
    }

    #[test]
    fn the_stage_is_the_proxy_s_size_times_four_and_marks_white() {
        let mut image = image::Rgb32FImage::from_pixel(3000, 2000, image::Rgb([0.2, 0.3, 0.4]));
        image.put_pixel(0, 0, image::Rgb([1.2, 0.3, 0.4]));
        let stage = stage_of(&DynamicImage::ImageRgb32F(image), "no-photo.cr3");
        assert_eq!((stage.width, stage.height), (768, 512));
        assert!(stage.clipped[0] && !stage.clipped[1000]);
    }
}
