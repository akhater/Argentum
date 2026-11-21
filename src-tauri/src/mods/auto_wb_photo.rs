//! Auto white balance for a photo that is not open.
//!
//! Auto measures the photo it is pressed on (`commands::detect_auto_white_balance`,
//! on the editor's picture). A paste carries that answer to other photos, where
//! it is the wrong light, so each of them is measured here instead
//! (argentum/autoWhiteBalance.ts). The picture is the one the editor would
//! measure (warped_photo.rs).

use std::path::Path;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::auto_wb::{DetectMode, auto_white_balance};
use crate::app_settings::AppSettings;
use crate::white_balance::WhiteBalance;

/// How long a paste may take to reach a photo's sidecar. Their paste writes
/// every sidecar before it draws a single thumbnail, so this is generous.
pub const PASTE_WAIT: Duration = Duration::from_secs(10);

/// The camera's white balance and the one Auto found, in kelvin and tint.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Measured {
    pub as_shot: WhiteBalance,
    pub white_balance: WhiteBalance,
}

/// Measure one photo. With `pasted_from`, the photo the pasted Auto was
/// measured on, it first waits for the paste to reach this photo's sidecar,
/// so the answer is written after the paste rather than under it.
pub fn measure(
    path: &str,
    pasted_from: Option<&str>,
    settings: &AppSettings,
    wait: Duration,
) -> Result<Measured, String> {
    let (source_path, sidecar_path) = crate::file_management::parse_virtual_path(path);
    if let Some(from) = pasted_from {
        wait_for_paste(&sidecar_path, from, wait)?;
    }
    let source = source_path.to_string_lossy();
    let (picture, _) = super::warped_photo::load(path, settings)?;

    let as_shot = crate::as_shot::as_shot(path).white_balance;
    let found = auto_white_balance(&picture, DetectMode::Surfaces, true, as_shot)
        .ok_or_else(|| format!("Could not detect an illuminant in {source}"))?;
    Ok(Measured {
        as_shot,
        white_balance: found.white_balance,
    })
}

/// Has the paste reached this sidecar? Its Auto then names the photo it was
/// measured on.
fn wait_for_paste(sidecar: &Path, from: &str, wait: Duration) -> Result<(), String> {
    let deadline = Instant::now() + wait;
    loop {
        let adjustments = crate::exif_processing::load_sidecar(sidecar).adjustments;
        if adjustments["whiteBalanceAuto"]["photo"].as_str() == Some(from) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("The paste did not reach this photo".to_string());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgb, RgbImage};

    /// A frame of coloured texture under a warm light, as a PNG beside its
    /// sidecar. The texture is auto_wb.rs's own test frame.
    fn warm_photo(dir: &Path, adjustments: serde_json::Value) -> String {
        let (width, height) = (128u32, 96u32);
        let light = [1.0f32, 0.8, 0.55];
        let mut img = RgbImage::new(width, height);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            let fx = x as f32 / width as f32;
            let fy = y as f32 / height as f32;
            let base = 0.25 + 0.5 * ((fx * 9.0).sin() * (fy * 7.0).cos()).abs();
            let surface = [
                base * (0.7 + 0.3 * (fx * 5.0).sin().abs()),
                base * (0.7 + 0.3 * (fy * 4.0).cos().abs()),
                base * (0.7 + 0.3 * ((fx + fy) * 3.0).sin().abs()),
            ];
            let encode = |v: f32| ((v.clamp(0.0, 1.0)).powf(1.0 / 2.2) * 255.0).round() as u8;
            *pixel = Rgb([
                encode(surface[0] * light[0]),
                encode(surface[1] * light[1]),
                encode(surface[2] * light[2]),
            ]);
        }
        let photo = dir.join("warm.png");
        DynamicImage::ImageRgb8(img).save(&photo).unwrap();
        let path = photo.to_string_lossy().to_string();
        let (_, sidecar_path) = crate::file_management::parse_virtual_path(&path);
        let metadata = crate::image_processing::ImageMetadata {
            adjustments,
            ..Default::default()
        };
        std::fs::write(sidecar_path, serde_json::to_string(&metadata).unwrap()).unwrap();
        path
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ag-auto-wb-photo-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Pasted onto: measured once the paste is there, and it finds the warm
    /// light, so the answer cools the photo below its as-shot temperature.
    #[test]
    fn a_pasted_photo_is_measured_for_its_own_light() {
        let dir = scratch("measured");
        let photo = warm_photo(
            &dir,
            serde_json::json!({ "whiteBalanceAuto": { "temperature": 5000.0, "tint": 0.0, "photo": "elsewhere.CR2" } }),
        );
        let measured = measure(
            &photo,
            Some("elsewhere.CR2"),
            &AppSettings::default(),
            Duration::from_secs(1),
        )
        .expect("measured");
        assert!(
            measured.white_balance.temperature < measured.as_shot.temperature,
            "{measured:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A paste that never arrives is not waited for forever, and nothing is measured.
    #[test]
    fn a_paste_that_does_not_arrive_is_given_up_on() {
        let dir = scratch("missing");
        let photo = warm_photo(&dir, serde_json::json!({}));
        let started = Instant::now();
        let result = measure(
            &photo,
            Some("elsewhere.CR2"),
            &AppSettings::default(),
            Duration::from_millis(300),
        );
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
