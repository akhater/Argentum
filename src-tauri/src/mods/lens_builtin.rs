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
//! `lens_builtin` below. While the switch is on, each part (distortion,
//! lateral CA, vignetting) comes from one source, the camera's tables or
//! lensfun's profile (`with_lensfun`, used by `get_geometry_params_from_json`
//! in `image_processing.rs`), so nothing is corrected twice, and the panel's
//! Distortion, Chromatic Aberration and Vignetting switches and amounts act
//! on whichever corrects it.
//!
//! WHICH PHOTOS START WITH IT ON
//!
//! A correction Lightroom applies by default (`default_on`: Fujifilm's, a
//! DNG's opcodes, Sony's lateral CA, Nikon Z's where the lens needs it,
//! Coolpix's) is on for a photo with no edit yet. One Lightroom uses only in
//! place of a lens profile (Nikon Z's optional one) starts off: the switch is
//! there, off. A photo edited before this existed keeps the look it was
//! edited with until the switch is turned on.

use std::sync::Arc;

use argentum_engine::rm_lens_builtin::{Amounts, LensCorrection, LensMap, Parts};
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
    /// The correction the edit turns on, or `None` when it is off, the Lens
    /// Correction eye is closed, the photo has none, or its tables do not read back.
    pub fn from_adjustments(adjustments: &serde_json::Value) -> Option<Builtin> {
        if adjustments.get(KEYS[0])?.as_bool() != Some(true)
            || !crate::image_processing::is_lens_correction_shown(adjustments)
        {
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

/// Each part's keys in lensfun's numbers (`lensDistortionParams`), and the
/// value that corrects nothing.
const LENSFUN_PARTS: [(&[&str], f64); 3] = [
    (&["k1", "k2", "k3"], 0.),
    (&["tca_vr", "tca_vb"], 1.),
    (&["vig_k1", "vig_k2", "vig_k3"], 0.),
];

/// The camera's correction a render applies and lensfun's numbers beside it,
/// one source per part (distortion, lateral CA, vignetting): the camera's
/// own data for a part it always corrects (`LensCorrection::always`, all
/// three for every correction stored before that field), else lensfun's
/// profile, else the camera's data standing in for a profile while the edit
/// asks for one (Auto) and the profile has none for that part. Nothing is
/// corrected twice.
pub fn with_lensfun(
    adjustments: &serde_json::Value,
) -> (
    Option<Builtin>,
    Option<serde_json::Map<String, serde_json::Value>>,
) {
    let mut lensfun = adjustments
        .get("lensDistortionParams")
        .and_then(|v| v.as_object())
        .cloned();
    let Some(mut builtin) = Builtin::from_adjustments(adjustments) else {
        return (None, lensfun);
    };
    let always = builtin.correction.always.unwrap_or(Parts::ALL);
    let auto = adjustments["lensCorrectionMode"].as_str() == Some("auto");
    let c = &builtin.correction;
    let parts = [
        (
            always.distortion,
            c.distortion.is_some(),
            &mut builtin.amounts.distortion,
        ),
        (
            always.chromatic,
            c.chromatic.is_some(),
            &mut builtin.amounts.chromatic,
        ),
        (
            always.vignetting,
            c.vignetting.is_some(),
            &mut builtin.amounts.vignetting,
        ),
    ];
    for ((owned, camera_has, amount), (keys, neutral)) in parts.into_iter().zip(LENSFUN_PARTS) {
        let profile_has = lensfun.as_ref().is_some_and(|p| {
            keys.iter().any(|k| {
                p.get(*k)
                    .and_then(|v| v.as_f64())
                    .is_some_and(|v| (v - neutral).abs() > 1e-9)
            })
        });
        if (owned || !profile_has)
            && let Some(p) = lensfun.as_mut()
        {
            keys.iter().for_each(|k| drop(p.remove(*k)));
        }
        if !owned && (profile_has || !(auto && camera_has)) {
            *amount = 0.;
        }
    }
    (Some(builtin), lensfun)
}

/// What the editor needs when a photo opens.
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename = "BuiltinLens"))]
#[serde(rename_all = "camelCase")]
pub struct Found {
    /// The photo's own tables; `None` when it carries none.
    pub correction: Option<LensCorrection>,
    /// The switch's starting position, for an edit that has none yet.
    pub default_on: bool,
    /// The photo has no saved edit, so My Lenses' "Correct new photos"
    /// decides how it opens (argentum/newPhotoDefaults.ts).
    pub fresh: bool,
}

/// The `ag` command `lens_builtin`: the photo's correction, read from its
/// file, and whether the switch starts on.
pub fn lens_builtin(path: &str) -> Result<Found, String> {
    let (source, sidecar) = crate::file_management::parse_virtual_path(path);
    let correction = {
        let bytes = crate::file_management::read_file_mapped(&source).map_err(|e| e.to_string())?;
        argentum_engine::rm_lens_builtin::read(&bytes)
    };
    let saved = crate::exif_processing::load_sidecar(&sidecar).adjustments;
    let edit = saved
        .as_object()
        .filter(|edit| !edit.is_empty() && edit.get("is_null").is_none());
    let edited_before = edit.is_some_and(|edit| !edit.contains_key(KEYS[0]));
    let default_on = correction.as_ref().is_some_and(|c| c.default_on) && !edited_before;
    Ok(Found {
        correction,
        default_on,
        fresh: edit.is_none(),
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
    fn the_lens_correction_eye_closed_applies_none() {
        let hidden = json!({
            "lensBuiltin": true,
            "lensBuiltinParams": correction(),
            "sectionVisibility": { "lens": false },
        });
        assert!(Builtin::from_adjustments(&hidden).is_none());
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

    /// An image's values, each to five significant digits, as one FNV-1a
    /// hash: exact enough to catch any change to a warp, loose enough to
    /// ignore a platform's last-bit difference in the maths.
    fn digest(image: &image::DynamicImage) -> u64 {
        let text: String = (image.to_rgb32f().iter())
            .map(|v| format!("{v:.4e},"))
            .collect();
        text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
        })
    }

    /// Today's warp and its inverse, pinned on a small gradient, with the
    /// camera's tables (Fujifilm's fill and Sony's), with lensfun's numbers,
    /// and with both, where the camera's replace lensfun's. A change meant to
    /// move one updates its line here; any other change fails.
    #[test]
    fn todays_warps_are_pinned() {
        use crate::image_processing::{
            get_geometry_params_from_json, unwarp_image_geometry, warp_image_geometry,
        };
        let image =
            image::DynamicImage::ImageRgb32F(image::Rgb32FImage::from_fn(48, 32, |x, y| {
                image::Rgb([x as f32 / 48., y as f32 / 32., (x + y) as f32 / 80.])
            }));
        let radial = |values: Vec<f32>| Radial {
            knots: vec![0., 0.5, 1.],
            values,
        };
        // Tables stored before the optical centre was read (`centre` None),
        // or read with one.
        let tables_at = |source: &str, centre: Option<[f32; 2]>| {
            serde_json::to_value(LensCorrection {
                source: source.into(),
                default_on: true,
                vignetting: Some(radial(vec![1., 1.2, 1.6])),
                // A ZV-1-like table: 1.077 at the centre, so the fills differ.
                distortion: Some(radial(vec![1.077, 1.04, 0.93])),
                chromatic: Some([radial(vec![1., 1., 1.0006]), radial(vec![1., 1., 0.9995])]),
                always: None,
                centre,
            })
            .unwrap()
        };
        let tables = |source: &str| tables_at(source, None);
        let lensfun = json!({
            "k1": -0.02, "k2": 0.004, "k3": 0.0, "model": 0,
            "tca_vr": 1.0004, "tca_vb": 0.9997, "vig_k1": -0.3, "vig_k2": 0.05, "vig_k3": 0.0,
        });
        let pinned = |edit: serde_json::Value| {
            let params = get_geometry_params_from_json(&edit);
            let warped = warp_image_geometry(&image, params.clone());
            [
                digest(&warped),
                digest(&unwarp_image_geometry(&warped, params)),
            ]
        };
        let fuji = json!({ "lensBuiltin": true, "lensBuiltinParams": tables("Fujifilm built-in") });
        for (name, edit, want) in [
            (
                "camera",
                fuji.clone(),
                [8_378_816_467_656_231_270, 8_125_105_830_291_545_259],
            ),
            (
                "sony",
                json!({ "lensBuiltin": true, "lensBuiltinParams": tables(LensCorrection::SONY_BUILTIN) }),
                [6_215_859_517_951_140_240, 12_902_846_181_271_203_747],
            ),
            (
                "lensfun",
                json!({ "lensDistortionParams": lensfun }),
                [12_547_794_342_902_090_592, 11_362_195_639_387_376_797],
            ),
            // Read with its optical centre: framed by green's edge, and an
            // off-centre one turning about its own centre.
            (
                "camera, read now",
                json!({ "lensBuiltin": true, "lensBuiltinParams": tables_at("Fujifilm built-in", Some([0.5, 0.5])) }),
                [8_056_836_662_080_894_814, 6_411_524_091_815_424_525],
            ),
            (
                "camera, off-centre",
                json!({ "lensBuiltin": true, "lensBuiltinParams": tables_at("Nikon built-in", Some([0.46, 0.53])) }),
                [17_173_643_604_237_575_065, 14_472_619_652_440_237_098],
            ),
        ] {
            assert_eq!(pinned(edit), want, "{name}");
        }
        let mut both = fuji.clone();
        both["lensDistortionParams"] = lensfun.clone();
        assert_eq!(
            pinned(both),
            pinned(fuji),
            "the camera's tables replace lensfun's"
        );
        // A camera that always corrects only its CA, beside lensfun's
        // distortion and vignetting.
        let mut ca_only = tables("Sony built-in");
        ca_only["always"] = json!({ "distortion": false, "chromatic": true, "vignetting": false });
        let edit = json!({ "lensBuiltin": true, "lensBuiltinParams": ca_only, "lensDistortionParams": lensfun });
        assert_eq!(
            pinned(edit),
            [2_546_953_749_071_934_686, 3_693_751_942_350_972_944],
            "camera CA, lensfun distortion and vignetting"
        );
    }

    /// A correction off the image centre turns about its optical centre in
    /// the app's warp: the pixel there samples itself in every channel, and
    /// the inverse brings the picture back.
    #[test]
    fn the_warp_turns_about_the_optical_centre() {
        use crate::image_processing::{
            get_geometry_params_from_json, unwarp_image_geometry, warp_image_geometry,
        };
        let (w, h) = (120u32, 80u32);
        let coords = image::DynamicImage::ImageRgb32F(image::Rgb32FImage::from_fn(w, h, |x, y| {
            image::Rgb([x as f32, y as f32, (x + y) as f32])
        }));
        let radial = |values: Vec<f32>| Radial {
            knots: vec![0., 0.6, 1.2],
            values,
        };
        // Optical centre at pixel (48, 44): 0.4 and 0.55 of 120 x 80.
        let lens = LensCorrection {
            source: "Nikon built-in".into(),
            default_on: true,
            distortion: Some(radial(vec![1., 0.98, 0.93])),
            chromatic: Some([
                radial(vec![1., 1.002, 1.004]),
                radial(vec![1., 0.998, 0.996]),
            ]),
            centre: Some([48.5 / 120., 44.5 / 80.]),
            ..Default::default()
        };
        let edit = json!({ "lensBuiltin": true, "lensBuiltinParams": lens });
        let params = get_geometry_params_from_json(&edit);
        let warped = warp_image_geometry(&coords, params.clone()).into_rgb32f();
        let at = |img: &image::Rgb32FImage, x: u32, y: u32| img.get_pixel(x, y).0;
        let [x, y, s] = at(&warped, 48, 44);
        assert!((x - 48.).abs() < 0.02 && (y - 44.).abs() < 0.02, "{x} {y}");
        assert!((s - 92.).abs() < 0.04, "every channel samples it: {s}");
        // Away from it, the picture moves.
        let [x, _, _] = at(&warped, 110, 44);
        assert!((x - 110.).abs() > 0.5, "{x}");
        let back = unwarp_image_geometry(&warped.into(), params).into_rgb32f();
        for (px, py) in [(30, 30), (60, 40), (90, 50)] {
            let [x, y, _] = at(&back, px, py);
            assert!(
                (x - px as f32).abs() < 0.3 && (y - py as f32).abs() < 0.3,
                "{px},{py}: {x} {y}"
            );
        }
    }

    /// Which source corrects each part, and what is left of lensfun's numbers.
    #[test]
    fn each_part_comes_from_one_source() {
        let radial = |v: f32| Radial {
            knots: vec![0., 1.],
            values: vec![1., v],
        };
        let camera = |always: Option<Parts>| {
            serde_json::to_value(LensCorrection {
                source: "Sony built-in".into(),
                default_on: true,
                vignetting: Some(radial(1.4)),
                distortion: Some(radial(0.98)),
                chromatic: Some([radial(1.0005), radial(0.9996)]),
                always,
                centre: None,
            })
            .unwrap()
        };
        let ca_only = Some(Parts {
            chromatic: true,
            ..Parts::default()
        });
        let full = json!({
            "k1": -0.02, "k2": 0.0, "k3": 0.0, "model": 0,
            "tca_vr": 1.0004, "tca_vb": 0.9997, "vig_k1": -0.3, "vig_k2": 0.0, "vig_k3": 0.0,
        });
        let distortion_only =
            json!({ "k1": -0.02, "k2": 0.0, "k3": 0.0, "model": 0, "tca_vr": 1.0, "tca_vb": 1.0 });
        let resolve = |always: Option<Parts>, lensfun: Option<&serde_json::Value>, mode: &str| {
            let mut edit = json!({ "lensBuiltin": true, "lensBuiltinParams": camera(always), "lensCorrectionMode": mode });
            if let Some(l) = lensfun {
                edit["lensDistortionParams"] = l.clone();
            }
            let (builtin, lensfun) = with_lensfun(&edit);
            let a = builtin.unwrap().amounts;
            let mut keys: Vec<String> = lensfun
                .map(|m| m.keys().cloned().collect())
                .unwrap_or_default();
            keys.sort();
            ([a.distortion, a.chromatic, a.vignetting], keys)
        };
        let keys = |k: &[&str]| k.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        // Stored before `always`: the camera decides all three, lensfun none.
        assert_eq!(
            resolve(None, Some(&full), "auto"),
            ([1., 1., 1.], keys(&["model"]))
        );
        // CA from the camera; distortion and vignetting from lensfun.
        assert_eq!(
            resolve(ca_only, Some(&full), "auto"),
            (
                [0., 1., 0.],
                keys(&["k1", "k2", "k3", "model", "vig_k1", "vig_k2", "vig_k3"])
            )
        );
        // No profile asked for: the camera's other tables stay unused.
        assert_eq!(resolve(ca_only, None, "manual"), ([0., 1., 0.], keys(&[])));
        // Auto with no profile: they stand in for one.
        assert_eq!(resolve(ca_only, None, "auto"), ([1., 1., 1.], keys(&[])));
        // Auto with a profile for distortion only: the camera's vignetting stands in.
        assert_eq!(
            resolve(ca_only, Some(&distortion_only), "auto"),
            ([0., 1., 1.], keys(&["k1", "k2", "k3", "model"]))
        );
        // Switched off: lensfun's numbers as they are.
        let off = json!({ "lensBuiltin": false, "lensBuiltinParams": camera(ca_only), "lensDistortionParams": full });
        let (builtin, lensfun) = with_lensfun(&off);
        assert!(builtin.is_none());
        assert_eq!(serde_json::Value::Object(lensfun.unwrap()), full);
    }

    /// A photo is fresh while its sidecar holds no edit: no sidecar, an
    /// empty edit, or a reset one. Any saved value makes it edited.
    #[test]
    fn a_photo_without_a_saved_edit_is_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("a.jpg");
        std::fs::write(&photo, b"not a raw").unwrap();
        let path = photo.to_string_lossy().into_owned();
        let sidecar = crate::file_management::parse_virtual_path(&path).1;
        let fresh = |adjustments: Option<serde_json::Value>| {
            match adjustments {
                Some(a) => std::fs::write(
                    &sidecar,
                    json!({ "version": 1, "rating": 0, "adjustments": a }).to_string(),
                )
                .unwrap(),
                None => drop(std::fs::remove_file(&sidecar)),
            }
            lens_builtin(&path).unwrap().fresh
        };
        assert!(fresh(None));
        assert!(fresh(Some(json!({}))));
        assert!(fresh(Some(json!({ "is_null": true }))));
        assert!(!fresh(Some(json!({ "exposure": 0.5 }))));
        assert!(!fresh(Some(json!({ "lensBuiltin": false }))));
    }

    #[test]
    fn its_keys_change_the_geometry_hash() {
        let on = json!({ "lensBuiltin": true, "lensBuiltinParams": correction() });
        let off = json!({ "lensBuiltin": false, "lensBuiltinParams": correction() });
        let hash = crate::cache_utils::calculate_geometry_hash;
        assert_ne!(hash(&on), hash(&off));
        assert_eq!(hash(&on), hash(&on.clone()));
    }
}
