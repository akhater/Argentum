//! `ag_dng_standard` against the app's DNG Standard.
//!
//! The app's side is its own files: `mods/dng_standard.rs` and
//! `mods/profiles.rs`, compiled unchanged by the reference crate, and
//! `ag_dng_standard` in `shaders/modules.wgsl`, run on the GPU. The engine's
//! side is its pixel pass, on the same GPU and the same half-float input.
//! Its RGBTone and ProPhoto matrices are the app's WGSL as written; its
//! curve is the exact one where the app's shader draws 16 points of it.
//!
//! - Each body's exposure offset, by make and model: identical.
//! - The curve: the app makes 16 points of it for its shader; through the
//!   engine's pass a grey at each of them comes out within a tenth of an
//!   8-bit level of that point, the half-float input included.
//! - The picture: between its 16 points the app's shader draws a spline,
//!   which its own test holds within half an 8-bit level of the exact curve
//!   the engine reads. On screen that half level grows where a channel is
//!   near black and the sRGB curve is steepest - a saturated colour's
//!   smallest channel. Measured on the CPU copy before the pass: 99.5% of
//!   channels within half a level, none more than 4.9 levels. Stated: 98%
//!   of channels within half a level, none more than 6.

mod common;

use argentum_engine::colour::{srgb_decode, srgb_encode};
use argentum_engine::tools::ag_dng_standard::{self, AgDngStandard};
use argentum_engine::{Gpu, Rgb, Source};
use argentum_engine_reference::mods::dng_standard as app;
use common::app_shader::{AppShader, Job};

fn app_offset(make: &str, model: &str) -> f32 {
    app::baseline(&app::Facts {
        make: make.into(),
        model: model.into(),
        ..Default::default()
    })
    .ev
}

#[test]
fn each_bodys_offset_is_the_apps() {
    let table = include_str!("../../src/mods/dng_baseline.tsv");
    let mut asked = vec![
        ("Nobody".to_string(), "X1".to_string()),
        ("Canon".to_string(), "EOS 9999D".to_string()),
        ("NIKON CORPORATION".to_string(), "NIKON D750".to_string()),
        (String::new(), String::new()),
    ];
    for line in table
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
    {
        let fields: Vec<&str> = line.split('\t').collect();
        for name in fields[1].split('|') {
            asked.push((fields[0].to_string(), name.to_string()));
            asked.push((fields[0].to_uppercase(), format!("{} {name}", fields[0])));
        }
    }
    for (make, model) in &asked {
        assert_eq!(
            ag_dng_standard::baseline(make, model).to_bits(),
            app_offset(make, model).to_bits(),
            "{make} / {model}"
        );
    }
    eprintln!("\n  {} make and model pairs, all the app's", asked.len());
}

#[test]
fn a_photos_offset_is_its_bodys() {
    eprintln!();
    for path in common::raws("a_photos_offset_is_its_bodys") {
        let bytes = std::fs::read(&path).unwrap();
        let source = Source::decode(&bytes).unwrap();
        let edit = argentum_engine::standard().defaults_for(&source);
        let ours = edit.tools["ag_dng_standard"].settings["baseline_exposure_ev"]
            .as_f64()
            .unwrap() as f32;
        let facts = app::facts(&bytes).unwrap();
        let app_full = app::baseline(&facts).ev;
        let name = path.file_name().unwrap().to_string_lossy();
        eprintln!(
            "  {name:<52} {} {}, ISO {:?}: ours {ours:+.2} EV, the app's {app_full:+.2} EV",
            facts.make, facts.model, facts.iso
        );
        // By make and model, the app's; ISO and a DNG's own offset are not
        // read by the engine yet.
        assert_eq!(ours, app_offset(&facts.make, &facts.model), "{name}");
    }
}

/// The engine's DNG Standard at `offset_ev`, on `rgb`: linear light out.
fn ours(gpu: &std::sync::Arc<Gpu>, offset_ev: f32, rgb: &Rgb) -> Rgb {
    let source = Source::new(0, common::bayer(16, 16, |_, _| 0));
    common::only::<AgDngStandard>(
        gpu.clone(),
        serde_json::json!({ "enabled": true, "baseline_exposure_ev": offset_ev }),
    )
    .run_pixel_tools(&source, rgb)
    .unwrap()
}

#[test]
fn the_curve_is_the_apps_at_every_point_it_gives_its_shader() {
    let Some(gpu) = common::gpu("the_curve_is_the_apps_at_every_point_it_gives_its_shader") else {
        return;
    };
    for ev in [-1.5, -0.5, -0.15, 0.0, 0.3, 0.5, 0.85, 1.5] {
        let points = app::curve_points(ev);
        let greys: Vec<[f32; 3]> = (0..16)
            .map(|i| common::half_exact([srgb_decode((i as f32 / 15.0).powf(1.3)); 3]))
            .collect();
        let out = ours(&gpu, ev, &Rgb::new(16, 1, greys));
        for (i, (point, grey)) in points.iter().zip(&out.pixels).enumerate() {
            let levels = (srgb_encode(grey[1]) * 255.0 - point.y).abs();
            assert!(
                levels <= 0.1,
                "{ev:+} EV, point {i}: {levels} of a level off"
            );
        }
    }
}

#[test]
fn the_picture_is_the_apps_within_its_spline() {
    let Some(gpu) = common::gpu("the_picture_is_the_apps_within_its_spline") else {
        return;
    };
    let shader = AppShader::new(
        gpu.clone(),
        r#"
        var points: array<Point, 16>;
        for (var k = 0u; k < 16u; k = k + 1u) {
            let q = ag_test_params[k];
            points[k] = Point(q.x, q.y, 0.0, 0.0);
        }
        return vec4<f32>(ag_dng_standard(v.xyz, points, 16u), 0.0);
        "#,
    );
    let rgb = common::colours();
    let inputs = common::elements(&rgb);
    eprintln!();
    for ev in [-0.5, 0.0, 0.3, 0.85] {
        let params: Vec<[f32; 4]> = app::curve_points(ev)
            .iter()
            .map(|p| [p.x, p.y, 0.0, 0.0])
            .collect();
        let theirs = shader.run(&Job {
            inputs: &inputs,
            params: &params,
            ..Default::default()
        });
        let ours = ours(&gpu, ev, &rgb);
        let mut worst = (0.0f32, [0.0; 3]);
        let mut over_half = 0;
        for ((p, ours), theirs) in rgb.pixels.iter().zip(&ours.pixels).zip(&theirs) {
            for c in 0..3 {
                let levels = (srgb_encode(ours[c]) - theirs[c]).abs() * 255.0;
                if levels > worst.0 {
                    worst = (levels, *p);
                }
                over_half += usize::from(levels > 0.5);
            }
        }
        let channels = rgb.pixels.len() * 3;
        eprintln!(
            "  {ev:+.2} EV: worst {:.2} of an 8-bit level at {:?}; {over_half} of {channels} channels over half a level",
            worst.0, worst.1
        );
        assert!(worst.0 <= 6.0, "{ev:+} EV: {worst:?}");
        assert!(
            over_half * 50 <= channels,
            "{ev:+} EV: {over_half} channels over half a level"
        );
    }
}
