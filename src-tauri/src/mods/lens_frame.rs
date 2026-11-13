//! A Lensfun calibration put on the photo's frame. Ours, on Lensfun's rule.
//!
//! Lensfun measures a lens on one camera and gives its coefficients as
//! functions of the radius on that camera's frame (the lens's `<cropfactor>`).
//! Until now they were applied as if every photo had that frame, so a lens
//! calibrated on a full-frame camera and used on a 1.6x body treated the APS-C
//! corner as the full-frame corner, and corrected the distortion there about 2.6
//! times too strongly.
//!
//! Lensfun puts a calibration on an image of another crop factor by scaling the
//! image's coordinates (`lfModifier`, given the image's crop factor; darktable's
//! `src/iop/lens.cc` passes it the camera's Lensfun crop factor). A point at a
//! radius `r` of the photo's half-diagonal lies at `s·r` of the calibration
//! frame's, with `s` the calibration's crop factor over the photo's (diagonals
//! go as one over the crop factor):
//!
//! - distortion: `mods/lens_units.rs` takes `s` with the rest of Lensfun's
//!   radius (`u = r·s·√(aspect² + 1)`);
//! - pa vignetting, `1 + k₁r² + k₂r⁴ + k₃r⁶` over the half-diagonal: `k₁s²`,
//!   `k₂s⁴`, `k₃s⁶` ([`vignetting_on_photo_frame`]);
//! - lateral CA as Argentum reads it (its constant term) is the same on any frame.
//!
//! A photo of the calibration's crop factor (`s = 1`) gets its coefficients
//! back bit for bit. A camera Lensfun doesn't know, or a lens with no crop
//! factor, is left as it was.

use crate::lens_correction::{Lens, LensDatabase, LensDistortionParams};
use std::collections::HashMap;

/// The scale from the photo's frame to `lens`'s calibration frame: the lens's
/// crop factor over the photo's, which is the camera's Lensfun crop factor
/// (as darktable takes it). 1 when either is unknown.
pub fn frame_scale(db: &LensDatabase, lens: &Lens, exif: &HashMap<String, String>) -> f64 {
    let text = |key: &str| {
        exif.get(key)
            .map_or("", |v| v.trim().trim_matches('"').trim())
    };
    let photo_crop =
        super::lens_match::camera(db, text("Make"), text("Model")).map(|c| c.cropfactor);
    match (lens.cropfactor, photo_crop) {
        (Some(lens_crop), Some(photo_crop)) if lens_crop > 0. && photo_crop > 0. => {
            f64::from(lens_crop) / f64::from(photo_crop)
        }
        _ => 1.,
    }
}

/// `params` with its vignetting terms, given on the calibration's frame, on a
/// photo frame `scale` times its radius ([`frame_scale`]).
pub fn vignetting_on_photo_frame(params: LensDistortionParams, scale: f64) -> LensDistortionParams {
    let s2 = scale * scale;
    let (s4, s6) = (s2 * s2, s2 * s2 * s2);
    LensDistortionParams {
        vig_k1: params.vig_k1 * s2,
        vig_k2: params.vig_k2 * s4,
        vig_k3: params.vig_k3 * s6,
        ..params
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lens_correction::resolve_lens_params;

    /// A full-frame body and an APS-C one of one mount; a lens calibrated on
    /// each, and one with no crop factor.
    const DB: &str = r#"<lensdatabase version="2">
  <camera><maker>Canon</maker><model>Canon EOS 5D Mark III</model><mount>Canon EF</mount><cropfactor>1</cropfactor></camera>
  <camera><maker>Canon</maker><model>Canon EOS 80D</model><mount>Canon EF-S</mount><cropfactor>1.6</cropfactor></camera>
  <lens><maker>Canon</maker><model>Canon EF 24-70mm f/2.8L USM</model><mount>Canon EF</mount><cropfactor>1</cropfactor>
    <calibration><distortion model="poly3" focal="24" k1="-0.02"/>
      <tca model="poly3" focal="24" vr="1.0004" vb="0.9997"/>
      <vignetting model="pa" focal="24" aperture="2.8" distance="1000" k1="-0.5" k2="0.1" k3="-0.05"/></calibration></lens>
  <lens><maker>Canon</maker><model>Canon EF 70-200mm f/2.8L IS II USM</model><mount>Canon EF</mount><cropfactor>1.6</cropfactor>
    <calibration><distortion model="ptlens" focal="70" a="0.01" b="-0.02" c="0.03"/></calibration></lens>
  <lens><maker>Test</maker><model>Test 35mm</model><mount>Canon EF</mount>
    <calibration><distortion model="poly5" focal="35" k1="-0.03" k2="0.01"/></calibration></lens>
</lensdatabase>"#;

    fn db() -> LensDatabase {
        quick_xml::de::from_str(DB).expect("the test database")
    }

    fn exif(make: &str, model: &str, focal: &str) -> HashMap<String, String> {
        [
            ("Make", make),
            ("Model", model),
            ("FocalLength", focal),
            ("FNumber", "f/2.8"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    fn params(lens: &str, make: &str, model: &str, focal: &str) -> serde_json::Value {
        let db = db();
        let lens = db
            .lenses
            .iter()
            .find(|l| l.get_full_model_name() == lens)
            .expect("the lens");
        let (maker, name) = crate::lens_correction::lens_result(&db, lens);
        serde_json::to_value(resolve_lens_params(
            &db,
            &maker,
            &name,
            &exif(make, model, focal),
        ))
        .unwrap()
    }

    /// `lens`'s terms at `focal` in the renderer's units on a frame `scale`
    /// times the calibration's radius, as `lens_units` converts them.
    fn converted(lens: &Lens, focal: f32, scale: f64) -> serde_json::Value {
        let raw = lens.get_distortion_params(focal, Some(2.8), None).unwrap();
        serde_json::to_value(crate::mods::lens_units::in_renderer_units(
            raw, lens, focal, scale,
        ))
        .unwrap()
    }

    /// On the camera it was calibrated on, a lens's coefficients are as they were.
    #[test]
    fn the_calibration_s_own_frame_is_unchanged_bit_for_bit() {
        let on_5d = params(
            "Canon EF 24-70mm f/2.8L USM",
            "Canon",
            "Canon EOS 5D Mark III",
            "24",
        );
        assert_eq!(on_5d, converted(&db().lenses[0], 24., 1.));
    }

    /// A full-frame calibration on a 1.6x body: the frame's corner is 1/1.6 of the
    /// calibration's radius. Distortion takes it in Lensfun's radius; a vignetting
    /// term of degree n takes (1/1.6)ⁿ.
    #[test]
    fn a_full_frame_calibration_on_a_crop_body_is_scaled_to_its_frame() {
        let s = 1. / f64::from(1.6f32);
        let on_80d = params(
            "Canon EF 24-70mm f/2.8L USM",
            "Canon",
            "Canon EOS 80D",
            "24",
        );
        let at_scale = converted(&db().lenses[0], 24., s);
        assert_eq!(on_80d["k1"], at_scale["k1"]);
        assert_ne!(on_80d["k1"], converted(&db().lenses[0], 24., 1.)["k1"]);
        let near = |key: &str, want: f64| {
            let got = on_80d[key].as_f64().unwrap();
            assert!((got - want).abs() < 1e-7, "{key}: {got} against {want}");
        };
        near("vig_k1", f64::from(-0.5f32) * s.powi(2));
        near("vig_k2", f64::from(0.1f32) * s.powi(4));
        near("vig_k3", f64::from(-0.05f32) * s.powi(6));
        // Lateral CA as Argentum reads it (its constant term) is the same on any frame.
        near("tca_vr", f64::from(1.0004f32));
        near("tca_vb", f64::from(0.9997f32));
    }

    /// A calibration made on a 1.6x body put on a full-frame one reaches past
    /// what it measured, as Lensfun takes it too.
    #[test]
    fn a_crop_calibration_on_a_full_frame_body_reaches_further() {
        let on_5d = params(
            "Canon EF 70-200mm f/2.8L IS II USM",
            "Canon",
            "Canon EOS 5D Mark III",
            "70",
        );
        let at_scale = converted(&db().lenses[1], 70., f64::from(1.6f32));
        for key in ["k1", "k2", "k3", "model"] {
            assert_eq!(on_5d[key], at_scale[key], "{key}");
        }
    }

    /// No crop factor for the lens, or a camera Lensfun doesn't know: as before.
    #[test]
    fn an_unknown_frame_leaves_the_calibration_alone() {
        let db = db();
        let lens = &db.lenses[2];
        assert_eq!(
            frame_scale(&db, lens, &exif("Canon", "Canon EOS 80D", "35")),
            1.
        );
        let ff = &db.lenses[0];
        assert_eq!(frame_scale(&db, ff, &exif("Leica", "M11", "24")), 1.);
        assert_eq!(frame_scale(&db, ff, &HashMap::new()), 1.);
        assert_eq!(
            frame_scale(&db, ff, &exif("\"Canon\"", " Canon EOS 80D ", "24")),
            1. / f64::from(1.6f32)
        );
    }

    /// The bundled database, read as the app reads it.
    fn bundled() -> LensDatabase {
        crate::lens_correction::read_lensfun_dir(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/lensfun_db"
        )))
    }

    /// The source radius over the output radius at `r` of the photo's
    /// half-diagonal, for a distortion calibration of `model` and coefficients
    /// `k`:
    /// - Lensfun's (`lensfun`), from `libs/lensfun/mod-coord.cpp` @ 52cf1a7b
    ///   (`rescale_polynomial_coefficients`), the calibration on a frame
    ///   of aspect `aspect` and `scale` times the photo's radius, `u` the radius
    ///   in half that frame's short edge;
    /// - Argentum's renderer at the default amount (`warp_image_geometry`), with
    ///   the coefficients it is sent.
    fn ratio(lensfun: bool, model: &str, k: [f64; 3], r: f64, aspect: f64, scale: f64) -> f64 {
        let [a, b, c] = k;
        if lensfun {
            let u = r * scale * (aspect * aspect + 1.).sqrt();
            match model {
                "poly3" => 1. + a * u * u / (1. - a).powi(3),
                "poly5" => 1. + a * u.powi(2) + b * u.powi(4),
                _ => {
                    let d = 1. - a - b - c;
                    1. + a * u.powi(3) / d.powi(4) + b * u * u / d.powi(3) + c * u / (d * d)
                }
            }
        } else if model == "ptlens" {
            1. + crate::mods::lens_units::GAIN * (a * r.powi(3) + b * r * r + c * r - a - b - c)
        } else {
            1. + crate::mods::lens_units::GAIN * (a * r.powi(2) + b * r.powi(4) + c * r.powi(6))
        }
    }

    /// How much more a correction stretches the radius at the frame's corner
    /// than at half way out (`D(1)/D(½) − 1`): what bends a straight line
    /// through the corner, whatever uniform magnification goes with it.
    fn bend(d: impl Fn(f64) -> f64) -> f64 {
        d(1.) / d(0.5) - 1.
    }

    /// The Lensfun distortion model of `lens`'s calibration.
    fn model_of(lens: &Lens) -> Option<&str> {
        lens.calibration
            .as_ref()?
            .elements
            .iter()
            .find_map(|e| match e {
                crate::lens_correction::CalibrationElement::Distortion(d) => Some(d.model.as_str()),
                _ => None,
            })
    }

    /// A lens's calibration aspect ratio, long edge over short (Lensfun's
    /// default 3:2).
    fn aspect(lens: &Lens) -> f64 {
        let ratio = lens
            .aspect_ratio
            .as_deref()
            .and_then(|t| match t.split_once(':') {
                Some((a, b)) => Some(a.trim().parse::<f64>().ok()? / b.trim().parse::<f64>().ok()?),
                None => t.trim().parse().ok(),
            });
        ratio.map_or(1.5, |r| r.max(1. / r))
    }

    /// What the frame scale changes, written out for review:
    ///
    /// - every interchangeable-lens camera and lens of its mount (or one Lensfun
    ///   says fits it) in the bundled database: how many calibrations are of
    ///   another crop factor than the body's;
    /// - the real files (AG_LENS_CORPUS, RAWmakase's record of raw.pixls.us's
    ///   files): each camera, lens and focal length the app matches automatically,
    ///   its scale, and for a poly3 calibration the distortion at the frame's
    ///   corner by Lensfun and by Argentum before and after (`frame-scale.tsv`);
    /// - photos named in AG_RAWS (';'-separated, the sheet RAWs): their lens,
    ///   scale, and whether the correction the app sends the renderer is the
    ///   same bit for bit.
    ///
    /// ```text
    /// AG_LENS_CORPUS=... AG_RAWS=...;... AG_OUT=.../dir cargo test --lib lens_frame::tests::what_the_frame_scale_changes -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "reads AG_LENS_CORPUS and AG_RAWS, writes to AG_OUT"]
    fn what_the_frame_scale_changes() {
        use crate::lens_correction::{find_best_lens_match, lens_named};
        use crate::mods::lens_match::{fits_mount, fixed_lens};
        use crate::mods::lens_shot::recorded_focal_mm;
        use std::fmt::Write as _;
        let db = bundled();
        let out = std::path::PathBuf::from(std::env::var("AG_OUT").expect("AG_OUT"));
        std::fs::create_dir_all(&out).unwrap();

        // 1. The database.
        let (mut pairs, mut larger, mut smaller) = (0, 0, 0);
        for cam in db.cameras.iter().filter(|c| !fixed_lens(c)) {
            for lens in db.lenses.iter().filter(|l| fits_mount(&db, cam, l)) {
                let Some(crop) = lens.cropfactor else {
                    continue;
                };
                pairs += 1;
                let s = f64::from(crop) / f64::from(cam.cropfactor);
                larger += usize::from(s < 0.99);
                smaller += usize::from(s > 1.01);
            }
        }
        println!(
            "database: {pairs} camera and lens pairs; {larger} with a calibration of a larger frame than the body's, {smaller} of a smaller one, {} within 1%",
            pairs - larger - smaller
        );

        // 2. Real files.
        if let Ok(path) = std::env::var("AG_LENS_CORPUS") {
            let json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            let mut seen = std::collections::BTreeSet::new();
            let mut report = String::from(
                "make\tmodel\tlens name\tfocal\tlens\tlens crop\tcamera crop\tscale\tLensfun bend\tbefore\tafter\n",
            );
            let (mut cases, mut matched, mut scaled) = (0, 0, 0);
            let (mut err_before, mut err_after) = (Vec::new(), Vec::new());
            let (mut compared, mut worse) = (0, 0);
            for file in json["files"].as_array().unwrap() {
                let md = &file["header"]["metadata"];
                let name = md["lens_model"].as_str().unwrap_or("");
                let focal = md["focal"].as_f64().filter(|f| *f > 0.);
                let (make, model) = (
                    md["make"].as_str().unwrap_or(""),
                    md["model"].as_str().unwrap_or(""),
                );
                // The Lensfun entry's strings are what the file writes.
                let Some(cam) = db.cameras.iter().find(|c| {
                    let (m, n) = (c.get_maker().to_lowercase(), c.get_model().to_lowercase());
                    m.contains(&make.to_lowercase())
                        && (crate::mods::lens_names::same_text(&n, model)
                            || n.ends_with(&format!(" {}", model.to_lowercase())))
                }) else {
                    continue;
                };
                let (make, model) = (
                    crate::lens_correction::untranslated(&cam.maker).to_string(),
                    crate::lens_correction::untranslated(&cam.model).to_string(),
                );
                let key = (
                    make.clone(),
                    model.clone(),
                    name.to_string(),
                    focal.map(|f| (f * 10.).round() as i64),
                );
                if !seen.insert(key) {
                    continue;
                }
                cases += 1;
                let mut exif: HashMap<String, String> = [("Make", &make), ("Model", &model)]
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();
                if let Some(f) = focal {
                    exif.insert("FocalLength".into(), f.to_string());
                }
                let Some((maker, display)) =
                    find_best_lens_match(&db, &make, name, &model, recorded_focal_mm(&exif))
                else {
                    continue;
                };
                let Some(lens) = lens_named(&db, &maker, &display) else {
                    continue;
                };
                matched += 1;
                let s = frame_scale(&db, lens, &exif);
                if s == 1. {
                    continue;
                }
                scaled += 1;
                let focal_mm = focal.unwrap_or(50.) as f32;
                let own = lens.get_distortion_params(focal_mm, None, None);
                let mut lensfun = "-".to_string();
                let (mut before, mut after) = ("-".to_string(), "-".to_string());
                if let (Some(p), Some(model)) = (own, model_of(lens))
                    && (p.k1, p.k2, p.k3) != (0., 0., 0.)
                {
                    let k = [p.k1, p.k2, p.k3];
                    let at = aspect(lens);
                    let lf = bend(|r| ratio(true, model, k, r, at, s));
                    // What the renderer is sent: Lensfun's terms in its units, on
                    // the calibration's frame (before) and on the photo's (after).
                    let sent = |scale: f64| {
                        let raw = lens.get_distortion_params(focal_mm, None, None).unwrap();
                        let p =
                            crate::mods::lens_units::in_renderer_units(raw, lens, focal_mm, scale);
                        [p.k1, p.k2, p.k3]
                    };
                    let ag = bend(|r| ratio(false, model, sent(1.), r, at, s));
                    let ag_after = bend(|r| ratio(false, model, sent(s), r, at, s));
                    compared += 1;
                    if (ag_after - lf).abs() > (ag - lf).abs() {
                        worse += 1;
                    }
                    err_before.push((ag - lf).abs() * 100.);
                    err_after.push((ag_after - lf).abs() * 100.);
                    lensfun = format!("{:.3}%", lf * 100.);
                    before = format!("{:.3}%", ag * 100.);
                    after = format!("{:.3}%", ag_after * 100.);
                }
                writeln!(
                    report,
                    "{make}\t{model}\t{name}\t{focal_mm}\t{}\t{:?}\t{}\t{s:.3}\t{lensfun}\t{before}\t{after}",
                    lens.get_full_model_name(),
                    lens.cropfactor,
                    cam.cropfactor
                )
                .unwrap();
            }
            std::fs::write(out.join("frame-scale.tsv"), &report).unwrap();
            let median = |v: &mut Vec<f64>| {
                v.sort_by(f64::total_cmp);
                v.get(v.len() / 2).copied().unwrap_or(0.)
            };
            let max = |v: &Vec<f64>| v.iter().copied().fold(0., f64::max);
            println!(
                "real files: {cases} camera, lens and focal-length cases of cameras Lensfun knows; {matched} matched automatically; {scaled} change scale. Of those, {compared} with distortion: the bend at the corner against Lensfun's, off by, % of the radius: median {:.3} -> {:.3}, max {:.3} -> {:.3}; {worse} further from Lensfun's",
                median(&mut err_before.clone()),
                median(&mut err_after.clone()),
                max(&err_before),
                max(&err_after),
            );
        }

        // 3. Named photos.
        if let Ok(raws) = std::env::var("AG_RAWS") {
            for photo in raws.split(';').filter(|p| !p.is_empty()) {
                let bytes = std::fs::read(photo).expect("a photo");
                let exif = crate::mods::lens_name::with_lens(
                    crate::exif_processing::read_exif_data_from_bytes(photo, &bytes),
                    &bytes,
                );
                let get = |k: &str| exif.get(k).cloned().unwrap_or_default();
                let found = find_best_lens_match(
                    &db,
                    &get("Make"),
                    &get("LensModel"),
                    &get("Model"),
                    recorded_focal_mm(&exif),
                );
                let Some((maker, display)) = found else {
                    println!("{photo}: {} | {}: no lens", get("Model"), get("LensModel"));
                    continue;
                };
                let lens = lens_named(&db, &maker, &display).unwrap();
                let shot = crate::mods::lens_shot::LensShot::from_exif(&exif);
                // What main sends: the terms in the renderer's units on the profile's frame.
                let own = lens
                    .get_distortion_params(shot.focal_mm, shot.f_number, shot.distance_m)
                    .map(|p| {
                        crate::mods::lens_units::in_renderer_units(p, lens, shot.focal_mm, 1.)
                    });
                let now = crate::lens_correction::resolve_lens_params(&db, &maker, &display, &exif);
                let same =
                    serde_json::to_string(&own).unwrap() == serde_json::to_string(&now).unwrap();
                println!(
                    "{photo}: {} | {} -> {} (crop {:?}), scale {}, correction the same bit for bit: {same}",
                    get("Model"),
                    get("LensModel"),
                    lens.get_full_model_name(),
                    lens.cropfactor,
                    frame_scale(&db, lens, &exif)
                );
            }
        }
    }
}
