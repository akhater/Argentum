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
//!
//! Lateral CA (`mod-subpix.cpp`, `ModifyCoord_TCA_Poly3` @ 52cf1a7b) scales
//! red's and blue's source radius by `v + c·u + b·u²` of the radius after
//! distortion, `u` the same radius over half the profile frame's short edge:
//! `c` and `b` take `u/r` and its square, `v` none. The renderer applies it
//! through [`tca_scale`].

use crate::lens_correction::{CalibrationElement, Lens, LensDistortionParams};

/// The renderer's gain on the distortion terms at the default amount.
pub const GAIN: f64 = 2.5;

/// Lensfun's aspect ratio of a profile's frame when it gives none
/// (`lfLensCalibAttributes`).
const DEFAULT_ASPECT: f64 = 1.5;

/// `params`, `lens`'s terms as Lensfun gives them, in the renderer's units.
/// `frame_scale` is the photo's radius over the profile frame's (1 on the
/// profile's own frame).
pub fn in_renderer_units(
    params: LensDistortionParams,
    lens: &Lens,
    frame_scale: f64,
) -> LensDistortionParams {
    let u = frame_scale * (aspect(lens).powi(2) + 1.).sqrt();
    let params = LensDistortionParams {
        tca_cr: params.tca_cr * u,
        tca_cb: params.tca_cb * u,
        tca_br: params.tca_br * u * u,
        tca_bb: params.tca_bb * u * u,
        ..params
    };
    let [k1, k2, k3] = [params.k1, params.k2, params.k3];
    let terms = match (params.model, distortion_model(lens)) {
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

/// The scale of red's or blue's source radius against green's at `r` of the
/// photo's half-diagonal (after distortion), with the Lateral CA `amount` (1 at
/// 100%): `v_blended` is the renderer's constant term at that amount, and `c`
/// and `b` the terms in the photo's radius ([`in_renderer_units`]).
pub fn tca_scale(v_blended: f32, c: f32, b: f32, r: f32, amount: f32) -> f32 {
    v_blended + (c * r + b * r * r) * amount
}

/// The gain that corrects a pa vignetting factor `v` (`1 + k₁r² + k₂r⁴ + k₃r⁶`
/// of the radius over the photo's half-diagonal) at the Vignetting `amount` (1
/// at 100%): Lensfun's `1/v` at 100%, none at 0. Until now the renderer took
/// 0.8 of the amount, a factor RapidRAW added with its distortion gain
/// ("hack to align it", 147d52e6), so 100% corrected a fifth too little.
pub fn vignetting_gain(v: f64, amount: f64) -> f64 {
    1.0 + (1.0 / v - 1.0) * amount
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

/// The distortion model of `lens`'s calibration: the first met, as Lensfun
/// takes it (`lens_interp`).
fn distortion_model(lens: &Lens) -> Option<&str> {
    lens.calibration
        .as_ref()?
        .elements
        .iter()
        .find_map(|e| match e {
            CalibrationElement::Distortion(d) if d.model != "none" => Some(d.model.as_str()),
            _ => None,
        })
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
  <lens><maker>Test</maker><model>TCA poly3 24mm</model><mount>Test</mount><cropfactor>1</cropfactor>
    <calibration><tca model="poly3" focal="24" vr="1.0003" vb="0.9998" cr="-0.0002" cb="0.0001" br="0.0004" bb="-0.0003"/></calibration></lens>
  <lens><maker>Test</maker><model>TCA linear 24mm</model><mount>Test</mount><cropfactor>1</cropfactor>
    <calibration><tca model="linear" focal="24" kr="1.0005" kb="0.9995"/></calibration></lens>
</lensdatabase>"#;

    /// What `resolve_lens_params` sends the renderer for the test lens `model`.
    fn sent_json(model: &str) -> serde_json::Value {
        let db: LensDatabase = quick_xml::de::from_str(DB).unwrap();
        let lens = db
            .lenses
            .iter()
            .find(|l| l.get_full_model_name() == model)
            .unwrap();
        let (maker, name) = crate::lens_correction::lens_result(&db, lens);
        let exif = HashMap::from([("FocalLength".to_string(), "24".to_string())]);
        serde_json::to_value(resolve_lens_params(&db, &maker, &name, &exif)).unwrap()
    }

    /// Lateral CA with terms of the radius scales red and blue as Lensfun does
    /// (`ModifyCoord_TCA_Poly3`: `v + c·u + b·u²`) at every radius; before, the
    /// renderer took `v` alone.
    #[test]
    fn poly3_lateral_ca_scales_as_lensfun_does() {
        let p = sent_json("TCA poly3 24mm");
        let get = |k: &str| p[k].as_f64().unwrap() as f32;
        let q = (1.5f64 * 1.5 + 1.).sqrt();
        for r in [0.2f64, 0.5, 0.8, 1.] {
            let u = r * q;
            let lensfun =
                |v: f32, c: f32, b: f32| f64::from(v) + f64::from(c) * u + f64::from(b) * u * u;
            let red = tca_scale(get("tca_vr"), get("tca_cr"), get("tca_br"), r as f32, 1.);
            let blue = tca_scale(get("tca_vb"), get("tca_cb"), get("tca_bb"), r as f32, 1.);
            assert!(
                (f64::from(red) - lensfun(1.0003, -0.0002, 0.0004)).abs() < 1e-6,
                "red at {r}"
            );
            assert!(
                (f64::from(blue) - lensfun(0.9998, 0.0001, -0.0003)).abs() < 1e-6,
                "blue at {r}"
            );
        }
        // At half the amount, half the difference from green.
        let half = tca_scale(1.0, get("tca_cr"), get("tca_br"), 1., 0.5);
        assert!(
            (f64::from(half) - (1. + 0.5 * (lensfun_poly3_rb(-0.0002, 0.0004, q)))).abs() < 1e-6
        );
    }

    fn lensfun_poly3_rb(c: f64, b: f64, u: f64) -> f64 {
        c * u + b * u * u
    }

    /// Lensfun's linear model writes its two terms as `kr` and `kb`.
    #[test]
    fn the_linear_lateral_ca_model_is_read() {
        let p = sent_json("TCA linear 24mm");
        assert!((p["tca_vr"].as_f64().unwrap() - f64::from(1.0005f32)).abs() < 1e-9);
        assert!((p["tca_vb"].as_f64().unwrap() - f64::from(0.9995f32)).abs() < 1e-9);
        assert_eq!(p["tca_cr"], 0.);
    }

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
            let after = in_renderer_units(before, lens, 1.);
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

    /// Lateral CA against Lensfun, before and after the terms of the radius:
    ///
    /// - every TCA row of the bundled database, on a photo of its own frame: how
    ///   far red's and blue's corner lands from Lensfun's, in pixels at the corner
    ///   of a 24 MP 3:2 frame (3,606 px from the centre);
    /// - the photos named in AG_RAWS: the same at their focal length, and ΔE2000
    ///   per pixel between today's warp and the new one (decoded, linear, 1600 px).
    ///
    /// ```text
    /// AG_RAWS=...;... cargo test --lib lens_units::tests::lateral_ca_against_lensfun -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "reads AG_RAWS"]
    fn lateral_ca_against_lensfun() {
        use crate::lens_correction::{CalibrationElement as E, find_best_lens_match, lens_named};
        const CORNER_PX: f64 = 3606.;
        let db = crate::lens_correction::read_lensfun_dir(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/lensfun_db"
        )));
        let t = |v: Option<f32>, none: f32| f64::from(v.unwrap_or(none));
        let (mut rows, mut closer, mut further, mut same, mut second) = (0, 0, 0, 0, 0);
        let (mut before, mut after) = (Vec::new(), Vec::new());
        for lens in &db.lenses {
            let q = (aspect(lens).powi(2) + 1.).sqrt();
            let mut focals = Vec::new();
            for e in lens.calibration.iter().flat_map(|c| &c.elements) {
                let E::Tca(row) = e else { continue };
                // A second calibration at a focal length the lens already has is
                // not the one Argentum takes (`get_distortion_params`, the first).
                if focals.contains(&row.focal) {
                    second += 1;
                    continue;
                }
                focals.push(row.focal);
                rows += 1;
                // Lensfun's scale at the corner, red and blue.
                let (vr, vb) = (t(row.vr.or(row.kr), 1.), t(row.vb.or(row.kb), 1.));
                let (cr, cb, br, bb) = (t(row.cr, 0.), t(row.cb, 0.), t(row.br, 0.), t(row.bb, 0.));
                let lensfun = [vr + cr * q + br * q * q, vb + cb * q + bb * q * q];
                // Main read vr and vb only, and not the linear model's kr and kb.
                let was = [t(row.vr, 1.), t(row.vb, 1.)];
                // Now: as sent, through the renderer's scale.
                let p = in_renderer_units(
                    lens.get_distortion_params(row.focal, None, None).unwrap(),
                    lens,
                    1.,
                );
                let now = [
                    f64::from(tca_scale(
                        p.tca_vr as f32,
                        p.tca_cr as f32,
                        p.tca_br as f32,
                        1.,
                        1.,
                    )),
                    f64::from(tca_scale(
                        p.tca_vb as f32,
                        p.tca_cb as f32,
                        p.tca_bb as f32,
                        1.,
                        1.,
                    )),
                ];
                for i in 0..2 {
                    let (eb, ea) = (
                        (was[i] - lensfun[i]).abs() * CORNER_PX,
                        (now[i] - lensfun[i]).abs() * CORNER_PX,
                    );
                    before.push(eb);
                    after.push(ea);
                    if ea < eb - 0.01 {
                        closer += 1;
                    } else if ea > eb + 0.01 {
                        further += 1;
                    } else {
                        same += 1;
                    }
                }
            }
        }
        let stats = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            let n = v.len();
            format!(
                "median {:.3}, p95 {:.3}, max {:.3}",
                v[n / 2],
                v[(n * 95 / 100).min(n - 1)],
                v[n - 1]
            )
        };
        println!(
            "TCA rows: {rows} (and {second} second calibrations at a focal length already there, left out); red and blue at a 24 MP corner, px from Lensfun's: before {}, after {}; {closer} closer, {further} further, {same} within 0.01 px",
            stats(&mut before),
            stats(&mut after)
        );

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
            let now =
                crate::lens_correction::resolve_lens_params(&db, &maker, &display, &exif).unwrap();
            let mut was = serde_json::to_value(&now).unwrap();
            for key in ["tca_cr", "tca_cb", "tca_br", "tca_bb"] {
                was[key] = 0.0.into();
            }
            let now = serde_json::to_value(&now).unwrap();
            let corner = |p: &serde_json::Value, c: &str| {
                let g = |k: &str| p[k].as_f64().unwrap() as f32;
                let (v, cc, b) = match c {
                    "red" => (g("tca_vr"), g("tca_cr"), g("tca_br")),
                    _ => (g("tca_vb"), g("tca_cb"), g("tca_bb")),
                };
                f64::from(tca_scale(v, cc, b, 1., 1.))
            };
            let moved = |c: &str| (corner(&now, c) - corner(&was, c)).abs() * CORNER_PX;
            let image = crate::image_loader::load_base_image_from_bytes(
                &bytes,
                photo,
                false,
                &crate::app_settings::AppSettings::default(),
                None,
            )
            .expect("decodes")
            .resize(1600, 1600, image::imageops::FilterType::Triangle);
            let warp = |params: &serde_json::Value| {
                let edit = serde_json::json!({
                    "lensMaker": maker,
                    "lensModel": display,
                    "lensCorrectionMode": "auto",
                    "lensDistortionParams": params,
                });
                let geometry = crate::image_processing::get_geometry_params_from_json(&edit);
                crate::image_processing::warp_image_geometry(&image, geometry).to_rgb32f()
            };
            let (a, b) = (warp(&was), warp(&now));
            let mut de: Vec<f64> = a
                .pixels()
                .zip(b.pixels())
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
                "{name}: {} -> {}; red moves {:.2} px, blue {:.2} px at a 24 MP corner; ΔE2000 against today: mean {:.4}, p99 {:.3}, max {:.3}",
                get("LensModel"),
                lens.get_full_model_name(),
                moved("red"),
                moved("blue"),
                de.iter().sum::<f64>() / n,
                de[((n * 0.99) as usize).min(de.len() - 1)],
                de[de.len() - 1],
            );
        }
    }

    /// Between calibrated focal lengths, against Lensfun's interpolation written
    /// out here a second time (rows sorted by focal length, the spline through
    /// the two on either side, `lens.cpp` and `auxfun.cpp` @ 52cf1a7b):
    ///
    /// - every lens of one distortion model and no two rows at one focal length,
    ///   halfway between each pair of calibrated focal lengths: the bend at the
    ///   corner Lensfun's model gives with Lensfun's interpolated terms, and with
    ///   Argentum's before (linear in the raw terms) and now;
    /// - the same for lateral CA, red's and blue's corner at 24 MP;
    /// - the photos named in AG_RAWS: whether their focal length is a calibrated
    ///   one (then nothing is interpolated and nothing changes).
    ///
    /// ```text
    /// AG_RAWS=...;... cargo test --lib lens_units::tests::interpolation_against_lensfun -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "measurement over the bundled database"]
    fn interpolation_against_lensfun() {
        use crate::lens_correction::{CalibrationElement as E, Distortion, Tca};
        let db = crate::lens_correction::read_lensfun_dir(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/lensfun_db"
        )));
        let spline = |rows: &[(f64, f64)], i: usize, f: f64, by_focal: bool| {
            // rows sorted by focal length; i and i + 1 bracket f.
            let s = |j: usize| if by_focal { rows[j].0 } else { 1. };
            let y = |j: Option<usize>| j.map(|j| rows[j].1 * s(j));
            let t = (f - rows[i].0) / (rows[i + 1].0 - rows[i].0);
            let h = |y0: Option<f64>, y1: f64, y2: f64, y3: Option<f64>| {
                let tg1 = y0.map_or(y2 - y1, |y0| (y2 - y0) * 0.5);
                let tg2 = y3.map_or(y2 - y1, |y3| (y3 - y1) * 0.5);
                let (t2, t3) = (t * t, t * t * t);
                (2. * t3 - 3. * t2 + 1.) * y1
                    + (t3 - 2. * t2 + t) * tg1
                    + (-2. * t3 + 3. * t2) * y2
                    + (t3 - t2) * tg2
            };
            h(
                i.checked_sub(1).and_then(|j| y(Some(j))),
                y(Some(i)).unwrap(),
                y(Some(i + 1)).unwrap(),
                y((i + 2 < rows.len()).then_some(i + 2)),
            ) / if by_focal { f } else { 1. }
        };
        let stats = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            let n = v.len();
            format!(
                "median {:.4}, p95 {:.4}, max {:.4}",
                v[n / 2],
                v[(n * 95 / 100).min(n - 1)],
                v[n - 1]
            )
        };
        let (mut dist_before, mut dist_after, mut dist_further, mut dist_closer) =
            (Vec::new(), Vec::new(), 0, 0);
        let (mut tca_before, mut tca_after, mut tca_further, mut tca_closer) =
            (Vec::new(), Vec::new(), 0, 0);
        for lens in &db.lenses {
            let at = aspect(lens);
            let q = (at * at + 1.).sqrt();
            let elements = lens.calibration.iter().flat_map(|c| &c.elements);
            let mut dists: Vec<&Distortion> = elements
                .clone()
                .filter_map(|e| {
                    if let E::Distortion(d) = e {
                        Some(d)
                    } else {
                        None
                    }
                })
                .collect();
            let mut tcas: Vec<&Tca> = elements
                .filter_map(|e| if let E::Tca(t) = e { Some(t) } else { None })
                .collect();
            dists.sort_by(|a, b| a.focal.total_cmp(&b.focal));
            tcas.sort_by(|a, b| a.focal.total_cmp(&b.focal));
            let clean = |focals: Vec<f32>, models: Vec<&str>| {
                focals.windows(2).all(|w| w[0] != w[1]) && models.windows(2).all(|w| w[0] == w[1])
            };
            if dists.len() >= 2
                && clean(
                    dists.iter().map(|d| d.focal).collect(),
                    dists.iter().map(|d| d.model.as_str()).collect(),
                )
                && ["poly3", "poly5", "ptlens"].contains(&dists[0].model.as_str())
            {
                let model = dists[0].model.as_str();
                let terms = |d: &Distortion| {
                    let t = |v: Option<f32>| f64::from(v.unwrap_or(0.));
                    match model {
                        "ptlens" => [t(d.a), t(d.b), t(d.c)],
                        _ => [t(d.k1), t(d.k2), 0.],
                    }
                };
                for i in 0..dists.len() - 1 {
                    let f = (dists[i].focal + dists[i + 1].focal) / 2.;
                    let lf_terms: [f64; 3] = std::array::from_fn(|k| {
                        let rows: Vec<(f64, f64)> = dists
                            .iter()
                            .map(|d| (f64::from(d.focal), terms(d)[k]))
                            .collect();
                        spline(&rows, i, f64::from(f), true)
                    });
                    let lf = bend(|r| lensfun(model, lf_terms, at, r));
                    // Before: linear in the raw terms between the two rows.
                    let (a, b) = (terms(dists[i]), terms(dists[i + 1]));
                    let was: [f64; 3] = std::array::from_fn(|k| (a[k] + b[k]) / 2.);
                    let was_bend = bend(|r| lensfun(model, was, at, r));
                    // Now: what get_distortion_params gives.
                    let p = lens.get_distortion_params(f, None, None).unwrap();
                    let now_bend = bend(|r| lensfun(model, [p.k1, p.k2, p.k3], at, r));
                    let (eb, ea) = ((was_bend - lf).abs() * 100., (now_bend - lf).abs() * 100.);
                    dist_before.push(eb);
                    dist_after.push(ea);
                    dist_closer += usize::from(ea < eb - 0.001);
                    dist_further += usize::from(ea > eb + 0.001);
                }
            }
            if tcas.len() >= 2
                && clean(
                    tcas.iter().map(|t| t.focal).collect(),
                    tcas.iter().map(|t| t.model.as_str()).collect(),
                )
            {
                fn v(x: Option<f32>, none: f32) -> f64 {
                    f64::from(x.unwrap_or(none))
                }
                let corner = |vv: f64, c: f64, b: f64| vv + c * q + b * q * q;
                for i in 0..tcas.len() - 1 {
                    let f = (tcas[i].focal + tcas[i + 1].focal) / 2.;
                    let p = lens.get_distortion_params(f, None, None).unwrap();
                    for (vf, cf, bf, now_v, now_c, now_b) in [
                        (
                            (|t: &Tca| v(t.vr.or(t.kr), 1.)) as fn(&Tca) -> f64,
                            (|t: &Tca| v(t.cr, 0.)) as fn(&Tca) -> f64,
                            (|t: &Tca| v(t.br, 0.)) as fn(&Tca) -> f64,
                            p.tca_vr,
                            p.tca_cr,
                            p.tca_br,
                        ),
                        (
                            |t: &Tca| v(t.vb.or(t.kb), 1.),
                            |t: &Tca| v(t.cb, 0.),
                            |t: &Tca| v(t.bb, 0.),
                            p.tca_vb,
                            p.tca_cb,
                            p.tca_bb,
                        ),
                    ] {
                        let rows = |g: fn(&Tca) -> f64| -> Vec<(f64, f64)> {
                            tcas.iter().map(|t| (f64::from(t.focal), g(t))).collect()
                        };
                        let ff = f64::from(f);
                        let lf = corner(
                            spline(&rows(vf), i, ff, false),
                            spline(&rows(cf), i, ff, true),
                            spline(&rows(bf), i, ff, true),
                        );
                        let mid = |g: fn(&Tca) -> f64| (g(tcas[i]) + g(tcas[i + 1])) / 2.;
                        let was = corner(mid(vf), mid(cf), mid(bf));
                        let now = corner(now_v, now_c, now_b);
                        let (eb, ea) = ((was - lf).abs() * 3606., (now - lf).abs() * 3606.);
                        tca_before.push(eb);
                        tca_after.push(ea);
                        tca_closer += usize::from(ea < eb - 0.01);
                        tca_further += usize::from(ea > eb + 0.01);
                    }
                }
            }
        }
        println!(
            "distortion, {} midpoints: corner bend off Lensfun's, % of the radius: before {}, after {}; {dist_closer} closer, {dist_further} further",
            dist_before.len(),
            stats(&mut dist_before),
            stats(&mut dist_after)
        );
        println!(
            "lateral CA, {} midpoints (red and blue): corner off Lensfun's at 24 MP, px: before {}, after {}; {tca_closer} closer, {tca_further} further",
            tca_before.len(),
            stats(&mut tca_before),
            stats(&mut tca_after)
        );

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
            let Some((maker, display)) = crate::lens_correction::find_best_lens_match(
                &db,
                &get("Make"),
                &get("LensModel"),
                &get("Model"),
                &crate::mods::lens_shot::MatchShot::from_exif(&exif),
            ) else {
                println!("{name}: no lens");
                continue;
            };
            let lens = crate::lens_correction::lens_named(&db, &maker, &display).unwrap();
            let focal = crate::mods::lens_shot::LensShot::from_exif(&exif).focal_mm;
            let calibrated = |pick: &dyn Fn(&E) -> Option<f32>| {
                let focals: Vec<f32> = lens
                    .calibration
                    .iter()
                    .flat_map(|c| &c.elements)
                    .filter_map(pick)
                    .collect();
                focals.is_empty() || focals.contains(&focal) || focals.len() == 1
            };
            let dist = calibrated(&|e| {
                if let E::Distortion(d) = e {
                    Some(d.focal)
                } else {
                    None
                }
            });
            let tca = calibrated(&|e| {
                if let E::Tca(t) = e {
                    Some(t.focal)
                } else {
                    None
                }
            });
            println!(
                "{name}: {} at {focal} mm; distortion at a calibrated focal length (or one row): {dist}; lateral CA: {tca}",
                lens.get_full_model_name()
            );
        }
    }

    /// Neither Lensfun (`mod-color.cpp`, `apply_multiplier`: no upper bound on a
    /// float pixel @ 52cf1a7b) nor darktable (`src/iop/lens.cc`, which multiplies
    /// by Lensfun's factor @ f30f439f) caps the gain where a profile's factor
    /// nears zero, and neither does the renderer, beyond leaving a factor of 1e-6
    /// or less uncorrected. Against the 0.8 it had, the whole gain lifts a pixel
    /// by at most a quarter (0.32 EV), wherever the factor goes.
    #[test]
    fn the_whole_gain_lifts_at_most_a_quarter_over_four_fifths_of_it() {
        let mut highest: f64 = 0.;
        for i in 1..=100_000 {
            let v = f64::from(i) / 100_000.;
            highest = highest.max(vignetting_gain(v, 1.) / vignetting_gain(v, 0.8));
        }
        assert!(highest <= 1.25 && highest > 1.249, "{highest}");
    }

    /// A flat grey frame through the renderer with a vignetting profile alone:
    /// at 100% the corner gets Lensfun's whole gain, `1/v`, and the centre none;
    /// at 50%, half the difference.
    #[test]
    fn the_renderer_corrects_vignetting_by_lensfun_s_whole_gain() {
        let image = image::DynamicImage::ImageRgb32F(image::Rgb32FImage::from_pixel(
            201,
            135,
            image::Rgb([0.25, 0.25, 0.25]),
        ));
        let corner = |amount: f64| {
            let edit = serde_json::json!({
                "lensMaker": "Test",
                "lensModel": "Vignetting",
                "lensCorrectionMode": "auto",
                "lensVignetteAmount": amount,
                "lensDistortionParams": {
                    "k1": 0.0, "k2": 0.0, "k3": 0.0, "model": 0,
                    "tca_vr": 1.0, "tca_vb": 1.0,
                    "vig_k1": -0.5, "vig_k2": 0.0, "vig_k3": 0.0,
                },
            });
            let geometry = crate::image_processing::get_geometry_params_from_json(&edit);
            let out = crate::image_processing::warp_image_geometry(&image, geometry).to_rgb32f();
            let (w, h) = (out.width(), out.height());
            // The renderer places a pixel at its top-left coordinate: (0, 0) is the
            // corner, a half-diagonal from the frame's centre.
            let r: f64 = 1.;
            let v = 1. - 0.5 * r * r;
            (
                f64::from(out.get_pixel(0, 0)[0]) / 0.25,
                1. / v,
                f64::from(out.get_pixel(w / 2, h / 2)[0]),
            )
        };
        let (got, want, centre) = corner(100.);
        assert!((got - want).abs() < 2e-3, "{got} against Lensfun's {want}");
        assert!((centre - 0.25).abs() < 1e-3, "{centre}");
        let (half, want, _) = corner(50.);
        assert!((half - (1. + (want - 1.) / 2.)).abs() < 2e-3, "{half}");
    }

    /// Vignetting against Lensfun: every pa row of the bundled database at its
    /// own frame's corner, the gain at 100% before (0.8 of Lensfun's) and now,
    /// in EV from Lensfun's `1/v`; and for the photos named in AG_RAWS, how much
    /// their corner's correction changes, in EV.
    ///
    /// ```text
    /// AG_RAWS=...;... cargo test --lib lens_units::tests::vignetting_against_lensfun -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "measurement over the bundled database"]
    fn vignetting_against_lensfun() {
        use crate::lens_correction::CalibrationElement as E;
        let db = crate::lens_correction::read_lensfun_dir(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/lensfun_db"
        )));
        let (mut before, mut after, mut skipped) = (Vec::new(), Vec::new(), 0);
        for lens in &db.lenses {
            for e in lens.calibration.iter().flat_map(|c| &c.elements) {
                let E::Vignetting(row) = e else { continue };
                let k = [row.k1, row.k2, row.k3].map(|x| f64::from(x.unwrap_or(0.)));
                let v = 1. + k[0] + k[1] + k[2];
                if k == [0.; 3] {
                    continue;
                }
                if v <= 0. {
                    // A row whose factor reaches 0 before the corner: no gain there.
                    skipped += 1;
                    continue;
                }
                let lensfun = 1. / v;
                let was = 1. + (lensfun - 1.) * 0.8;
                let now = vignetting_gain(v, 1.);
                before.push((lensfun / was).log2().abs());
                after.push((lensfun / now).log2().abs());
            }
        }
        let stats = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            let n = v.len();
            format!(
                "median {:.4}, p95 {:.4}, max {:.4}",
                v[n / 2],
                v[(n * 95 / 100).min(n - 1)],
                v[n - 1]
            )
        };
        println!(
            "vignetting rows: {} ({skipped} reaching 0 before the corner left out); corner gain off Lensfun's, EV: before {}, after {}",
            before.len(),
            stats(&mut before),
            stats(&mut after)
        );

        let Ok(raws) = std::env::var("AG_RAWS") else {
            return;
        };
        for photo in raws.split(';').filter(|p| !p.is_empty()) {
            let bytes = std::fs::read(photo).expect("a photo");
            let name = photo.rsplit(['/', '\\']).next().unwrap_or(photo);
            let exif = crate::mods::lens_name::with_lens(
                crate::exif_processing::read_exif_data_from_bytes(photo, &bytes),
                &bytes,
            );
            let get = |k: &str| exif.get(k).cloned().unwrap_or_default();
            let Some((maker, display)) = crate::lens_correction::find_best_lens_match(
                &db,
                &get("Make"),
                &get("LensModel"),
                &get("Model"),
                &crate::mods::lens_shot::MatchShot::from_exif(&exif),
            ) else {
                println!("{name}: no lens");
                continue;
            };
            let p =
                crate::lens_correction::resolve_lens_params(&db, &maker, &display, &exif).unwrap();
            let v = 1. + p.vig_k1 + p.vig_k2 + p.vig_k3;
            let (now, was) = (vignetting_gain(v, 1.), vignetting_gain(v, 0.8));
            println!(
                "{name}: {display} at {} mm {}, distance {:?}: corner gain {was:.3} -> {now:.3}, {:+.3} EV; at 100% {:.5} EV",
                get("FocalLength"),
                get("FNumber"),
                get("SubjectDistance"),
                (now / was).log2(),
                now.log2()
            );
        }
    }
}
