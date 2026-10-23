//! The camera's built-in lens correction, in today's app. Ours.
//!
//! The tables are read from the RAW by the engine (`rm_lens_builtin.rs`,
//! from RAWmakase) and travel in the photo's edit the way lensfun's numbers
//! do (`lensDistortionParams`), so the preview, the export, the masks and the
//! thumbnail all see the same correction with no path of their own:
//!
//! | Key | What |
//! |---|---|
//! | `lensBuiltin` | the switch in Crop > Lens Correction; absent until the photo is opened |
//! | `lensBuiltinParams` | the photo's own tables, as `rm_lens_builtin::LensCorrection` |
//!
//! `argentum/lensBuiltin.ts` fills both in when a photo opens, from
//! `lens_builtin` below. While the switch is on, the tables replace the
//! lensfun profile (`get_geometry_params_from_json` in `image_processing.rs`),
//! so nothing is corrected twice, and the panel's Distortion, Chromatic
//! Aberration and Vignetting switches and amounts act on them.
//!
//! WHICH PHOTOS START WITH IT ON
//!
//! A correction Lightroom applies by default (`default_on`: Fujifilm's) is on
//! for a photo with no edit yet. A photo edited before this existed keeps the
//! look it was edited with until the switch is turned on. Sony's is not known
//! to be applied by Lightroom, so it starts off: the switch is there, off.
//! A DNG's opcodes are what Lightroom applies, so they start on, as
//! Fujifilm's do.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use argentum_engine::rm_lens_builtin::{Amounts, LensCorrection, LensMap};
use serde::Serialize;

/// The edit's keys for it, which the preview's geometry caches must see.
pub const KEYS: [&str; 2] = ["lensBuiltin", "lensBuiltinParams"];

/// The correction one render applies, and how much of each part.
#[derive(Clone, Debug)]
pub struct Builtin {
    pub correction: Arc<LensCorrection>,
    pub amounts: Amounts,
}

impl Builtin {
    /// The correction the edit turns on, or `None` when it is off, the photo
    /// has none, or its tables do not read back.
    pub fn from_adjustments(adjustments: &serde_json::Value) -> Option<Builtin> {
        if adjustments.get(KEYS[0])?.as_bool() != Some(true) {
            return None;
        }
        let correction: LensCorrection =
            serde_json::from_value(adjustments.get(KEYS[1])?.clone()).ok()?;
        if correction.is_empty() || !correction.validate() {
            return None;
        }
        // A part switched off is an amount of zero; the sliders run 0 to 200.
        let amount = |enabled: &str, amount: &str| {
            if adjustments[enabled].as_bool().unwrap_or(true) {
                adjustments[amount].as_f64().unwrap_or(100.0) as f32 / 100.0
            } else {
                0.0
            }
        };
        Some(Builtin {
            correction: Arc::new(correction),
            amounts: Amounts {
                distortion: amount("lensDistortionEnabled", "lensDistortionAmount"),
                chromatic: amount("lensTcaEnabled", "lensTcaAmount"),
                vignetting: amount("lensVignetteEnabled", "lensVignetteAmount"),
            },
        })
    }

    /// The correction over an image of `width` by `height` pixels.
    pub fn map(&self, width: f32, height: f32) -> LensMap<'_> {
        LensMap::new(&self.correction, width, height, self.amounts)
    }
}

/// A geometry cache's `hash` with the edit's keys for it folded in. Their
/// `GEOMETRY_KEYS` (cache_utils.rs) has no room for ours, so the preview's
/// caches in lib.rs fold them in here.
pub fn fold(hash: u64, adjustments: &serde_json::Value) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hash.hash(&mut hasher);
    hash_into(adjustments, &mut hasher);
    hasher.finish()
}

fn hash_into(adjustments: &serde_json::Value, hasher: &mut impl Hasher) {
    for key in KEYS {
        if let Some(value) = adjustments.get(key) {
            key.hash(hasher);
            value.to_string().hash(hasher);
        }
    }
}

/// What the editor needs when a photo opens.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    /// The photo's own tables; `None` when it carries none.
    pub correction: Option<LensCorrection>,
    /// The switch's starting position, for an edit that has none yet.
    pub default_on: bool,
}

/// The `ag` command `lens_builtin`: the photo's correction, read from its
/// file, and whether the switch starts on.
pub fn lens_builtin(path: &str) -> Result<Found, String> {
    let (source, sidecar) = crate::file_management::parse_virtual_path(path);
    let correction = {
        let bytes = crate::file_management::read_file_mapped(&source).map_err(|e| e.to_string())?;
        argentum_engine::rm_lens_builtin::read(&bytes)
    };
    let edited_before = || {
        let saved = crate::exif_processing::load_sidecar(&sidecar).adjustments;
        saved.as_object().is_some_and(|edit| {
            !edit.is_empty() && edit.get("is_null").is_none() && !edit.contains_key(KEYS[0])
        })
    };
    let default_on = correction.as_ref().is_some_and(|c| c.default_on) && !edited_before();
    Ok(Found {
        correction,
        default_on,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use argentum_engine::rm_lens_builtin::Radial;
    use serde_json::json;

    fn correction() -> serde_json::Value {
        serde_json::to_value(LensCorrection {
            source: "Fujifilm built-in".into(),
            default_on: true,
            vignetting: Some(Radial {
                knots: vec![0., 1.],
                values: vec![1., 1.5],
            }),
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn only_a_switched_on_edit_with_tables_applies_it() {
        let on = json!({ "lensBuiltin": true, "lensBuiltinParams": correction() });
        assert!(Builtin::from_adjustments(&on).is_some());
        let off = json!({ "lensBuiltin": false, "lensBuiltinParams": correction() });
        assert!(Builtin::from_adjustments(&off).is_none());
        assert!(Builtin::from_adjustments(&json!({ "lensBuiltin": true })).is_none());
        assert!(Builtin::from_adjustments(&json!({})).is_none());
        let garbage = json!({ "lensBuiltin": true, "lensBuiltinParams": { "knots": 3 } });
        assert!(Builtin::from_adjustments(&garbage).is_none());
    }

    #[test]
    fn the_panels_switches_and_amounts_set_how_much() {
        let edit = json!({
            "lensBuiltin": true,
            "lensBuiltinParams": correction(),
            "lensDistortionAmount": 150,
            "lensTcaEnabled": false,
            "lensVignetteAmount": 50,
        });
        let b = Builtin::from_adjustments(&edit).unwrap();
        assert_eq!(
            b.amounts,
            Amounts {
                distortion: 1.5,
                chromatic: 0.,
                vignetting: 0.5,
            }
        );
    }

    /// A drone DNG (`AG_RAW`) developed, warped and rendered as the app does,
    /// with its built-in correction off, on without OpcodeList3's gain map
    /// (as before the map was read) and on: `AG_OUT/<which>.jpg`, 1200 wide,
    /// and `<which>-corner.jpg`, the bottom left 480 x 320 pixels at 200%.
    /// Prints each one's corners against the middle of their edge, per
    /// channel, in linear light before the tone.
    #[test]
    #[ignore = "reads a local DNG named by AG_RAW; run by hand"]
    fn gain_map_sheet() {
        use image::{DynamicImage, GenericImageView, imageops::FilterType};
        let path = std::env::var("AG_RAW").expect("AG_RAW");
        let out = std::path::PathBuf::from(std::env::var("AG_OUT").expect("AG_OUT"));
        std::fs::create_dir_all(&out).unwrap();
        let bytes = std::fs::read(&path).expect("read");
        let as_shot = crate::raw_processing::read_as_shot(&bytes)
            .unwrap_or_else(crate::as_shot::AsShot::reference);
        let linear = crate::raw_processing::develop_raw_image(
            &bytes,
            false,
            2.5,
            "default".to_string(),
            None,
            Some(&path),
        )
        .expect("decodes");
        let full = argentum_engine::rm_lens_builtin::read(&bytes).expect("a correction");
        assert!(full.gain_map.is_some(), "no gain map");
        let before = LensCorrection {
            gain_map: None,
            ..full.clone()
        };
        let ctx = crate::mods::dng_standard::gpu::device().expect("no wgpu adapter");
        let save = |image: &DynamicImage, name: &str| {
            let file = std::fs::File::create(out.join(format!("{name}.jpg"))).expect("create");
            image::codecs::jpeg::JpegEncoder::new_with_quality(file, 95)
                .encode_image(&DynamicImage::ImageRgb8(image.to_rgb8()))
                .expect("write jpeg");
        };
        for (name, lens) in [
            ("off", None),
            ("before", Some(before)),
            ("after", Some(full)),
        ] {
            let edit = match &lens {
                Some(c) => json!({ "lensBuiltin": true, "lensBuiltinParams": c }),
                None => json!({ "lensBuiltin": false }),
            };
            let warped = crate::image_processing::apply_geometry_warp(&linear, &edit);
            let rgb = warped.to_rgb32f();
            let (w, h) = rgb.dimensions();
            let mean = |x0: u32, y0: u32| -> [f32; 3] {
                let mut sum = [0f64; 3];
                for y in y0..y0 + 64 {
                    for x in x0..x0 + 64 {
                        let p = rgb.get_pixel(x, y);
                        (0..3).for_each(|c| sum[c] += f64::from(p[c]));
                    }
                }
                sum.map(|s| (s / 4096.) as f32)
            };
            // Against the middle of the same edge, so a sky is weighed against sky.
            let corners = [(16, 16), (w - 80, 16), (16, h - 80), (w - 80, h - 80)].map(|(x, y)| {
                let middle = mean(w / 2 - 32, y);
                std::array::from_fn::<f32, 3, _>(|c| mean(x, y)[c] / middle[c])
            });
            eprintln!("{name}: corner / its edge's middle, RGB, TL TR BL BR: {corners:.2?}");
            let adjustments = crate::image_processing::get_all_adjustments_from_json(
                &edit,
                true,
                as_shot,
                Default::default(),
                Some(&path),
            );
            let rendered = crate::mods::export_precision::render_high_precision(
                &ctx,
                &warped,
                crate::gpu_processing::RenderRequest {
                    adjustments,
                    mask_bitmaps: &[],
                    lut: None,
                    roi: None,
                },
            )
            .expect("renders");
            let (rw, rh) = rendered.dimensions();
            save(
                &rendered.resize(1200, 1200 * rh / rw, FilterType::Triangle),
                name,
            );
            let corner = rendered.crop_imm(0, rh - 320, 480, 320);
            save(
                &corner.resize_exact(960, 640, FilterType::Nearest),
                &format!("{name}-corner"),
            );
        }
    }

    #[test]
    fn its_keys_change_the_geometry_hash() {
        let on = json!({ "lensBuiltin": true, "lensBuiltinParams": correction() });
        let off = json!({ "lensBuiltin": false, "lensBuiltinParams": correction() });
        assert_ne!(fold(7, &on), fold(7, &off));
        assert_eq!(fold(7, &on), fold(7, &on.clone()));
        assert_ne!(fold(7, &on), fold(8, &on));
    }
}
