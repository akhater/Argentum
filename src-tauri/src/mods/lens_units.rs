//! A Lensfun profile's distortion terms in the renderer's units. Ours, on
//! Lensfun's rule.
//!
//! The renderer (`image_processing::warp_image_geometry` and the functions
//! beside it, from RapidRAW) reads the source radius over the output radius as
//!
//! - `1 + G·(k₁r² + k₂r⁴ + k₃r⁶)` for poly3 and poly5,
//! - `1 + G·(a·r³ + b·r² + c·r − a − b − c)` for ptlens (the frame's corner
//!   stays where it is),
//!
//! with `r` the radius over the photo's half-diagonal and `G` = [`GAIN`] times
//! the Distortion amount. The terms went to it as Lensfun gives them, which
//! read a radius `u` over half the short edge of the profile's frame: about
//! 1.8 times the photo's half-diagonal for a 3:2 frame, so each term was off by
//! a different power of 1.8. The gain of 2.5 is near the 3.25 a second power
//! needs on a 3:2 frame; ptlens profiles, whose terms are of the first, second
//! and third powers and often of both signs, came out a different shape.
//!
//! Lensfun (`libs/lensfun/mod-coord.cpp`, `rescale_polynomial_coefficients`,
//! and `modifier.cpp`, `lfModifier::lfModifier`, @ 52cf1a7b) corrects with
//! `1 + k₁u²/d³` (poly3, `d = 1 − k₁`), `1 + k₁u² + k₂u⁴` (poly5) and
//! `1 + a·u³/d⁴ + b·u²/d³ + c·u/d²` (ptlens, `d = 1 − a − b − c`): PanoTools'
//! polynomials rescaled so the centre keeps its magnification. `u` is the
//! radius in millimetres over half the profile frame's short edge,
//! `diagonal(36 × 24) / crop / √(aspect² + 1) / 2`; on a photo of the
//! profile's own frame, `u = r·√(aspect² + 1)`.
//!
//! So each term is put on the photo's radius and divided by the gain: poly3
//! and poly5 then correct exactly as Lensfun does at the default amount.
//! ptlens comes out exactly Lensfun's shape times a uniform magnification,
//! `1 / (1 + A + B + C)`, as the renderer keeps the corner fixed where Lensfun
//! keeps the centre: the terms are multiplied by that same factor, which the
//! renderer's `− a − b − c` then turns back into the constant term.

use crate::lens_correction::{CalibrationElement, Lens, LensDistortionParams};

/// The renderer's gain on the distortion terms at the default amount.
pub const GAIN: f64 = 2.5;

/// Lensfun's aspect ratio of a profile's frame when it gives none
/// (`lfLensCalibAttributes`).
const DEFAULT_ASPECT: f64 = 1.5;

/// `params`, `lens`'s terms at `focal_mm` as Lensfun gives them, in the
/// renderer's units. `frame_scale` is the photo's radius over the profile
/// frame's (1 on the profile's own frame).
pub fn in_renderer_units(
    params: LensDistortionParams,
    lens: &Lens,
    focal_mm: f32,
    frame_scale: f64,
) -> LensDistortionParams {
    let u = frame_scale * (aspect(lens).powi(2) + 1.).sqrt();
    let [k1, k2, k3] = [params.k1, params.k2, params.k3];
    let terms = match (params.model, distortion_model(lens, focal_mm)) {
        (1, _) => {
            let d = 1. - k1 - k2 - k3;
            let (a, b, c) = (
                k1 * u.powi(3) / d.powi(4),
                k2 * u * u / d.powi(3),
                k3 * u / (d * d),
            );
            let m = 1. / (1. + a + b + c);
            [a * m, b * m, c * m]
        }
        (_, Some("poly3")) => [k1 * u * u / (1. - k1).powi(3), 0., 0.],
        _ => [k1 * u.powi(2), k2 * u.powi(4), k3 * u.powi(6)],
    };
    if [k1, k2, k3] == [0.; 3] || terms.iter().any(|t| !t.is_finite()) {
        return params;
    }
    LensDistortionParams {
        k1: terms[0] / GAIN,
        k2: terms[1] / GAIN,
        k3: terms[2] / GAIN,
        ..params
    }
}

/// The aspect ratio of `lens`'s profile frame, long edge over short.
fn aspect(lens: &Lens) -> f64 {
    let ratio = lens
        .aspect_ratio
        .as_deref()
        .and_then(|t| match t.split_once(':') {
            Some((a, b)) => Some(a.trim().parse::<f64>().ok()? / b.trim().parse::<f64>().ok()?),
            None => t.trim().parse().ok(),
        })
        .filter(|r| r.is_finite() && *r > 0.);
    ratio.map_or(DEFAULT_ASPECT, |r| r.max(1. / r))
}

/// The distortion model of `lens`'s calibration nearest `focal_mm`, as
/// `Lens::get_distortion_params` takes its terms.
fn distortion_model(lens: &Lens, focal_mm: f32) -> Option<&str> {
    lens.calibration
        .as_ref()?
        .elements
        .iter()
        .filter_map(|e| match e {
            CalibrationElement::Distortion(d) => Some(d),
            _ => None,
        })
        .min_by(|a, b| {
            (a.focal - focal_mm)
                .abs()
                .total_cmp(&(b.focal - focal_mm).abs())
        })
        .map(|d| d.model.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lens_correction::{LensDatabase, resolve_lens_params};
    use std::collections::HashMap;

    /// Lensfun's correction, the source radius over the output radius at `r` of
    /// the photo's half-diagonal, written from `mod-coord.cpp` @ 52cf1a7b
    /// (`rescale_polynomial_coefficients` and `ModifyCoord_Dist_*`), on a photo of
    /// the profile's frame of aspect `aspect`.
    fn lensfun(model: &str, k: [f64; 3], aspect: f64, r: f64) -> f64 {
        let u = r * (aspect * aspect + 1.).sqrt();
        let [a, b, c] = k;
        match model {
            "poly3" => 1. + a / (1. - a).powi(3) * u * u,
            "poly5" => 1. + a * u.powi(2) + b * u.powi(4),
            _ => {
                let d = 1. - a - b - c;
                1. + a / d.powi(4) * u.powi(3) + b / d.powi(3) * u * u + c / (d * d) * u
            }
        }
    }

    /// The renderer's, at the default amount, as `warp_image_geometry` reads
    /// the terms it is sent.
    fn renderer(ptlens: bool, k: [f64; 3], r: f64) -> f64 {
        let [a, b, c] = k;
        if ptlens {
            1. + GAIN * (a * r.powi(3) + b * r * r + c * r - a - b - c)
        } else {
            1. + GAIN * (a * r * r + b * r.powi(4) + c * r.powi(6))
        }
    }

    const DB: &str = r#"<lensdatabase version="2">
  <lens><maker>Test</maker><model>Poly3 24mm</model><mount>Test</mount><cropfactor>1</cropfactor>
    <calibration><distortion model="poly3" focal="24" k1="-0.02"/></calibration></lens>
  <lens><maker>Test</maker><model>Poly5 24mm</model><mount>Test</mount><cropfactor>1</cropfactor><aspect-ratio>4:3</aspect-ratio>
    <calibration><distortion model="poly5" focal="24" k1="-0.03" k2="0.01"/></calibration></lens>
  <lens><maker>Test</maker><model>PTLens 70mm</model><mount>Test</mount><cropfactor>1</cropfactor>
    <calibration><distortion model="ptlens" focal="70" a="-0.00831" b="0.04241" c="-0.04789"/></calibration></lens>
</lensdatabase>"#;

    fn sent(model: &str) -> ([f64; 3], bool) {
        let db: LensDatabase = quick_xml::de::from_str(DB).unwrap();
        let lens = db
            .lenses
            .iter()
            .find(|l| l.get_full_model_name() == model)
            .unwrap();
        let (maker, name) = crate::lens_correction::lens_result(&db, lens);
        let exif = HashMap::from([("FocalLength".to_string(), "24".to_string())]);
        let p = serde_json::to_value(resolve_lens_params(&db, &maker, &name, &exif)).unwrap();
        let get = |k: &str| p[k].as_f64().unwrap();
        ([get("k1"), get("k2"), get("k3")], p["model"] == 1)
    }

    /// poly3 and poly5 correct as Lensfun does, at every radius.
    #[test]
    fn polynomial_profiles_correct_as_lensfun_does() {
        let (k, _) = sent("Poly3 24mm");
        for r in [0.2, 0.5, 0.8, 1.] {
            let want = lensfun("poly3", [f64::from(-0.02f32), 0., 0.], 1.5, r);
            assert!((renderer(false, k, r) - want).abs() < 1e-9, "poly3 at {r}");
        }
        let (k, _) = sent("Poly5 24mm");
        for r in [0.2, 0.5, 0.8, 1.] {
            let want = lensfun(
                "poly5",
                [f64::from(-0.03f32), f64::from(0.01f32), 0.],
                4. / 3.,
                r,
            );
            assert!((renderer(false, k, r) - want).abs() < 1e-9, "poly5 at {r}");
        }
    }

    /// A ptlens profile with terms of both signs (the EF 24-105mm f/3.5-5.6 IS
    /// STM at 70 mm) is Lensfun's shape, times one magnification for the whole
    /// frame. Before, it bent the corner by 0.15% where Lensfun bends it by 1.71%.
    #[test]
    fn a_mixed_sign_ptlens_profile_is_lensfun_s_shape() {
        let (k, ptlens) = sent("PTLens 70mm");
        assert!(ptlens);
        let terms = [-0.00831f32, 0.04241, -0.04789].map(f64::from);
        let m = renderer(true, k, 1.) / lensfun("ptlens", terms, 1.5, 1.);
        for r in [0.1, 0.3, 0.5, 0.7, 0.9] {
            let ratio = renderer(true, k, r) / lensfun("ptlens", terms, 1.5, r);
            assert!((ratio - m).abs() < 1e-9, "at {r}: {ratio} against {m}");
        }
        let bend = |d: &dyn Fn(f64) -> f64| d(1.) / d(0.5) - 1.;
        let lf = bend(&|r| lensfun("ptlens", terms, 1.5, r));
        assert!((bend(&|r| renderer(true, k, r)) - lf).abs() < 1e-9);
        assert!((lf - 0.01713).abs() < 1e-4, "{lf}");
    }

    /// How much more a correction stretches the radius at the frame's corner than
    /// half way out (`D(1)/D(½) − 1`): what bends a straight line through the
    /// corner, whatever uniform magnification goes with it.
    fn bend(d: impl Fn(f64) -> f64) -> f64 {
        d(1.) / d(0.5) - 1.
    }

    /// Lensfun's terms as `lensfun` takes them, and its model's name, of a
    /// calibration row.
    fn row_terms(d: &crate::lens_correction::Distortion) -> Option<(&'static str, [f64; 3])> {
        let t = |v: Option<f32>| f64::from(v.unwrap_or(0.));
        match d.model.as_str() {
            "poly3" => Some(("poly3", [t(d.k1), 0., 0.])),
            "poly5" => Some(("poly5", [t(d.k1), t(d.k2), 0.])),
            "ptlens" => Some(("ptlens", [t(d.a), t(d.b), t(d.c)])),
            _ => None,
        }
    }

    /// Every distortion profile of the bundled database, and the photos named in
    /// AG_RAWS (';'-separated, the sheet RAWs), before and after:
    ///
    /// - each calibration row, on a photo of its own frame: the bend at the
    ///   corner by Lensfun's model and by the renderer with the terms it was sent
    ///   before and is sent now (`profiles.tsv` in AG_OUT);
    /// - each photo: its lens, the same bends at its focal length, and how far
    ///   the corrected picture moves: ΔE2000 per pixel between the two warps of
    ///   the decoded photo (linear, at 1600 px), mean, 99th percentile and
    ///   largest, and the share of pixels over 1.
    ///
    /// ```text
    /// AG_RAWS=...;... AG_OUT=.../dir cargo test --lib lens_units::tests::every_profile_against_lensfun -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "writes to AG_OUT; reads AG_RAWS"]
    fn every_profile_against_lensfun() {
        use crate::lens_correction::{CalibrationElement as E, find_best_lens_match, lens_named};
        use std::fmt::Write as _;
        let db = crate::lens_correction::read_lensfun_dir(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/lensfun_db"
        )));
        let out = std::path::PathBuf::from(std::env::var("AG_OUT").expect("AG_OUT"));
        std::fs::create_dir_all(&out).unwrap();
        let bends = |lens: &Lens, focal: f32, model: &str, terms: [f64; 3]| {
            let before = lens.get_distortion_params(focal, None, None).unwrap();
            let ptlens = before.model == 1;
            let kb = [before.k1, before.k2, before.k3];
            let after = in_renderer_units(before, lens, focal, 1.);
            let ka = [after.k1, after.k2, after.k3];
            let at = aspect(lens);
            (
                bend(|r| lensfun(model, terms, at, r)),
                bend(|r| renderer(ptlens, kb, r)),
                bend(|r| renderer(ptlens, ka, r)),
            )
        };

        // 1. Every profile.
        let mut report = String::from("lens\tfocal\tmodel\tLensfun\tbefore\tafter\n");
        let (mut rows, mut closer, mut further, mut same) = (0, 0, 0, 0);
        let (mut max_before, mut max_after) = (0f64, 0f64);
        for lens in &db.lenses {
            let Some(cal) = &lens.calibration else {
                continue;
            };
            for e in &cal.elements {
                let E::Distortion(d) = e else { continue };
                let Some((model, terms)) = row_terms(d) else {
                    continue;
                };
                if terms == [0.; 3] {
                    continue;
                }
                rows += 1;
                let (lf, before, after) = bends(lens, d.focal, model, terms);
                let (eb, ea) = ((before - lf).abs() * 100., (after - lf).abs() * 100.);
                max_before = max_before.max(eb);
                max_after = max_after.max(ea);
                if ea < eb - 0.01 {
                    closer += 1;
                } else if ea > eb + 0.01 {
                    further += 1;
                } else {
                    same += 1;
                }
                writeln!(
                    report,
                    "{}\t{}\t{model}\t{:.4}%\t{:.4}%\t{:.4}%",
                    lens.get_full_model_name(),
                    d.focal,
                    lf * 100.,
                    before * 100.,
                    after * 100.
                )
                .unwrap();
            }
        }
        std::fs::write(out.join("profiles.tsv"), &report).unwrap();
        println!(
            "profiles: {rows} calibration rows; the corner bend against Lensfun's, % of the radius: {closer} closer, {further} further, {same} within 0.01; largest error {max_before:.3} before, {max_after:.6} after"
        );

        // 2. Named photos.
        let Ok(raws) = std::env::var("AG_RAWS") else {
            return;
        };
        for photo in raws.split(';').filter(|p| !p.is_empty()) {
            let bytes = std::fs::read(photo).expect("a photo");
            let exif = crate::mods::lens_name::with_lens(
                crate::exif_processing::read_exif_data_from_bytes(photo, &bytes),
                &bytes,
            );
            let get = |k: &str| exif.get(k).cloned().unwrap_or_default();
            let name = photo.rsplit(['/', '\\']).next().unwrap_or(photo);
            let shot = crate::mods::lens_shot::LensShot::from_exif(&exif);
            let Some((maker, display)) = find_best_lens_match(
                &db,
                &get("Make"),
                &get("LensModel"),
                &get("Model"),
                &crate::mods::lens_shot::MatchShot::from_exif(&exif),
            ) else {
                println!("{name}: no lens");
                continue;
            };
            let lens = lens_named(&db, &maker, &display).unwrap();
            let row = lens
                .calibration
                .iter()
                .flat_map(|c| &c.elements)
                .filter_map(|e| match e {
                    E::Distortion(d) => Some(d),
                    _ => None,
                })
                .min_by(|a, b| {
                    (a.focal - shot.focal_mm)
                        .abs()
                        .total_cmp(&(b.focal - shot.focal_mm).abs())
                })
                .and_then(row_terms);
            let bend_line = row.map_or("no distortion profile".to_string(), |(model, _)| {
                // Lensfun's terms at the photo's focal length, as Argentum
                // interpolates them.
                let p = lens
                    .get_distortion_params(shot.focal_mm, None, None)
                    .unwrap();
                let (lf, before, after) = bends(lens, shot.focal_mm, model, [p.k1, p.k2, p.k3]);
                format!(
                    "{model} at {} mm: corner bend Lensfun {:.3}%, before {:.3}%, after {:.3}%",
                    shot.focal_mm,
                    lf * 100.,
                    before * 100.,
                    after * 100.
                )
            });
            // The two warps of the decoded photo.
            let image = crate::image_loader::load_base_image_from_bytes(
                &bytes,
                photo,
                false,
                &crate::app_settings::AppSettings::default(),
                None,
            )
            .expect("decodes")
            .resize(1600, 1600, image::imageops::FilterType::Triangle);
            let warp = |params: LensDistortionParams| {
                let edit = serde_json::json!({
                    "lensMaker": maker,
                    "lensModel": display,
                    "lensCorrectionMode": "auto",
                    "lensDistortionParams": params,
                });
                let geometry = crate::image_processing::get_geometry_params_from_json(&edit);
                crate::image_processing::warp_image_geometry(&image, geometry).to_rgb32f()
            };
            let raw = lens
                .get_distortion_params(shot.focal_mm, shot.f_number, shot.distance_m)
                .unwrap();
            let today = warp(raw);
            let now = warp(
                crate::lens_correction::resolve_lens_params(&db, &maker, &display, &exif).unwrap(),
            );
            let mut de: Vec<f64> = today
                .pixels()
                .zip(now.pixels())
                .map(|(a, b)| {
                    let c = |p: &image::Rgb<f32>| p.0.map(|v| v.clamp(0., 1.));
                    argentum_engine::colour::delta_e2000(
                        argentum_engine::colour::lab(c(a)),
                        argentum_engine::colour::lab(c(b)),
                    )
                })
                .collect();
            de.sort_by(f64::total_cmp);
            let n = de.len() as f64;
            println!(
                "{name}: {} | {} -> {}; {bend_line}; ΔE2000 against today: mean {:.3}, p99 {:.3}, max {:.3}, {:.1}% of pixels over 1",
                get("Model"),
                get("LensModel"),
                lens.get_full_model_name(),
                de.iter().sum::<f64>() / n,
                de[((n * 0.99) as usize).min(de.len() - 1)],
                de[de.len() - 1],
                de.iter().filter(|v| **v > 1.).count() as f64 * 100. / n,
            );
        }
    }
}
