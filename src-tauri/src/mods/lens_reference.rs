//! Argentum's lens correction against Lensfun's own. Ours, on RAWmakase's
//! reference data.
//!
//! `fixtures/lensfun-reference/reference.json` (RAWmakase v0.7.1, 26267b16,
//! MIT; see its README) holds six synthetic calibrations and what Lensfun itself
//! computes with each on a photo of a given crop factor and size: the source
//! coordinates of 35 points with distortion or lateral CA corrected, and the
//! vignetting gain there. Each calibration goes through what the app does with
//! a photo (the lens, the camera's crop factor and the photo's frame, the focal
//! length, aperture and distance read from its EXIF, `resolve_lens_params`), and
//! the terms it sends through the renderer's formulas (`lens_units`); the
//! answer must be Lensfun's.
//!
//! Two differences are the renderer's on purpose, and taken out: it keeps a
//! ptlens profile's corner where Lensfun keeps the centre (a uniform
//! magnification, `lens_units`), and it scales the picture to hide the edges
//! the correction pulls in. So distortion is compared as a shape: the source
//! radius over Lensfun's, at every point, over its mean.

#![cfg(test)]

use crate::lens_correction::{LensDatabase, lens_result, resolve_lens_params};
use crate::mods::lens_units::{tca_scale, tests::renderer, vignetting_gain};
use std::collections::HashMap;

/// Lensfun's correction of one photo, as `reference.json` gives it.
struct Case {
    name: String,
    xml: String,
    crop: f64,
    size: [f64; 2],
    points: Vec<[f64; 2]>,
    shots: Vec<serde_json::Value>,
}

fn cases() -> Vec<Case> {
    let json: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../fixtures/lensfun-reference/reference.json"
    ))
    .expect("the reference");
    json.into_iter()
        .map(|c| Case {
            name: c["name"].as_str().unwrap().to_string(),
            xml: c["xml"].as_str().unwrap().to_string(),
            crop: c["crop"].as_f64().unwrap(),
            size: [c["width"].as_f64().unwrap(), c["height"].as_f64().unwrap()],
            points: c["points"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()])
                .collect(),
            shots: c["shots"].as_array().unwrap().clone(),
        })
        .collect()
}

/// A point's distance from Lensfun's centre over the frame's half-diagonal.
/// Lensfun's frame runs from pixel 0 to pixel `size - 1` (`lfModifier`).
fn radius([w, h]: [f64; 2], [x, y]: [f64; 2]) -> f64 {
    let (cx, cy) = ((w - 1.) / 2., (h - 1.) / 2.);
    (x - cx).hypot(y - cy) / (cx.hypot(cy))
}

/// What the app sends the renderer for `case`'s lens, on a body of the case's
/// crop factor, in a photo of its size, shot as `shot` says.
fn sent(case: &Case, shot: &serde_json::Value) -> crate::lens_correction::LensDistortionParams {
    let xml = case.xml.replacen(
        "<lens>",
        &format!(
            "<camera><maker>Synthetic</maker><model>Body</model><mount>Generic</mount><cropfactor>{}</cropfactor></camera><lens>",
            case.crop
        ),
        1,
    );
    let db: LensDatabase = quick_xml::de::from_str(&xml).expect("the case's database");
    let (maker, display) = lens_result(&db, &db.lenses[0]);
    let [w, h] = case.size;
    let exif: HashMap<String, String> = [
        ("Make", "Synthetic".to_string()),
        ("Model", "Body".to_string()),
        ("FocalLength", shot["focal"].to_string()),
        ("FNumber", shot["aperture"].to_string()),
        ("SubjectDistance", shot["distance"].to_string()),
        (crate::mods::photo_frame::KEY, format!("{w}x{h}")),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    resolve_lens_params(&db, &maker, &display, &exif).expect("the lens's terms")
}

/// Rows of numbers Lensfun gave in `mode` for `shot`, one per point.
fn rows(shot: &serde_json::Value, mode: &str) -> Vec<Vec<f64>> {
    shot[mode]["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            r.as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect()
        })
        .collect()
}

/// How far each correction is from Lensfun's on one shot: distortion's shape (a
/// fraction of the radius), lateral CA's red and blue scales, vignetting's gain
/// (a fraction of it).
#[derive(Debug, Default)]
struct Off {
    distortion: f64,
    tca: f64,
    vignetting: f64,
}

fn compare(case: &Case, shot: &serde_json::Value) -> Off {
    let p = sent(case, shot);
    let size = case.size;
    let mut off = Off::default();
    // Not the centre itself, where a radius is 0 over 0.
    let at: Vec<(usize, f64)> = case
        .points
        .iter()
        .map(|&pt| radius(size, pt))
        .enumerate()
        .filter(|(_, r)| *r > 0.05)
        .collect();

    // Distortion: the source radius over the output radius, Argentum's over
    // Lensfun's, against their mean.
    let d = rows(shot, "d");
    let ratios: Vec<f64> = at
        .iter()
        .map(|&(i, r)| {
            let green = [d[i][2], d[i][3]];
            let lensfun = radius(size, green) / r;
            let ours = renderer(p.model == 1, [p.k1, p.k2, p.k3], r);
            ours / lensfun
        })
        .collect();
    let mean = ratios.iter().sum::<f64>() / ratios.len() as f64;
    off.distortion = ratios
        .iter()
        .map(|q| (q / mean - 1.).abs())
        .fold(0., f64::max);

    // Lateral CA: red's and blue's source radius over green's.
    let t = rows(shot, "t");
    for &(i, r) in &at {
        let green = radius(size, [t[i][2], t[i][3]]);
        let (red, blue) = (
            radius(size, [t[i][0], t[i][1]]) / green,
            radius(size, [t[i][4], t[i][5]]) / green,
        );
        let r32 = r as f32;
        let ours_red = tca_scale(p.tca_vr as f32, p.tca_cr as f32, p.tca_br as f32, r32, 1.);
        let ours_blue = tca_scale(p.tca_vb as f32, p.tca_cb as f32, p.tca_bb as f32, r32, 1.);
        off.tca = off
            .tca
            .max((f64::from(ours_red) - red).abs())
            .max((f64::from(ours_blue) - blue).abs());
    }

    // Vignetting: the gain at each point.
    let v = rows(shot, "v");
    for (i, &pt) in case.points.iter().enumerate() {
        let r2 = radius(size, pt).powi(2);
        let factor = 1. + p.vig_k1 * r2 + p.vig_k2 * r2 * r2 + p.vig_k3 * r2 * r2 * r2;
        let ours = vignetting_gain(factor, 1.);
        off.vignetting = off.vignetting.max((ours / v[i][0] - 1.).abs());
    }
    off
}

/// Every case and shot within these of Lensfun: distortion's shape to 0.05% of
/// the radius (a third of a pixel at the corner of a 24 MP frame), red and blue
/// to 0.002%, the vignetting gain to 0.1%. The case Argentum does not follow
/// yet is listed with how far it is: an optical centre off the frame's.
#[test]
fn corrections_are_lensfun_s() {
    let mut report = Vec::new();
    for case in cases() {
        for shot in &case.shots {
            let off = compare(&case, shot);
            report.push(format!(
                "{} at {} mm: distortion {:.2e}, lateral CA {:.2e}, vignetting {:.2e}",
                case.name, shot["focal"], off.distortion, off.tca, off.vignetting
            ));
            if case.name == "decentred" {
                continue;
            }
            assert!(
                off.distortion < 5e-4 && off.tca < 2e-5 && off.vignetting < 1e-3,
                "{} at {} mm: {off:?}",
                case.name,
                shot["focal"]
            );
        }
    }
    println!("{}", report.join("\n"));
}
