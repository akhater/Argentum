//! `ag_exposure` against the app's `apply_linear_exposure` (shader.wgsl):
//! the engine's through its pixel pass, the app's as it is, on the same GPU
//! and the same half-float input.
//!
//! Stated tolerance: 4 f32 steps of the value. Both run the same WGSL in
//! different shaders, which the GPU's compiler may arrange differently.

mod common;

use argentum_engine::tools::ag_dng_standard::{self, AgDngStandard};
use argentum_engine::tools::ag_exposure::{self, AgExposure};
use argentum_engine::tools::rt_capture_sharpen;
use argentum_engine::{Context, GpuSlot, Pipeline, Size, Source, Tool};
use common::app_shader::{AppShader, Job};

const CASES_EV: [f32; 6] = [1.0, -2.5, 0.5, 3.3, -5.0, 1.0 / 3.0];

#[test]
fn the_apps_shader_agrees() {
    let Some(gpu) = common::gpu("the_apps_shader_agrees") else {
        return;
    };
    let shader = AppShader::new(
        gpu.clone(),
        "return vec4<f32>(apply_linear_exposure(v.xyz, ag_test_params[0].x), 0.0);",
    );
    let rgb = common::colours();
    let source = Source::new(0, common::bayer(16, 16, |_, _| 0));
    for exposure_ev in CASES_EV {
        let theirs = shader.run(&Job {
            inputs: &common::elements(&rgb),
            params: &[[exposure_ev, 0.0, 0.0, 0.0]],
            ..Default::default()
        });
        let ours = common::only::<AgExposure>(
            gpu.clone(),
            serde_json::json!({ "exposure_ev": exposure_ev }),
        )
        .run_pixel_tools(&source, &rgb)
        .unwrap();
        for (a, b) in ours.pixels.iter().zip(&theirs) {
            for c in 0..3 {
                let tolerance = b[c].abs() * 4.0 * f32::EPSILON + f32::MIN_POSITIVE;
                assert!(
                    (a[c] - b[c]).abs() <= tolerance,
                    "{exposure_ev} EV: ours {}, the app's {}",
                    a[c],
                    b[c]
                );
            }
        }
    }
}

#[test]
fn no_exposure_is_nothing_to_do() {
    let source = Source::new(0, common::bayer(16, 16, |_, _| 0));
    let slot = GpuSlot::new();
    let cx = Context::new::<AgExposure>(&source, 0, &slot);
    assert!(
        AgExposure
            .pixel_params(&ag_exposure::Settings::default(), &cx)
            .is_none()
    );
}

/// Through the pipeline, exposure multiplies the picture before it by 2 to
/// the exposure, within a half-float step: the pass reads its input as the
/// app does, in half floats.
#[test]
fn through_the_pipeline_it_multiplies() {
    let Some(gpu) = common::gpu("through_the_pipeline_it_multiplies") else {
        return;
    };
    let source = common::source(320, 240);
    let mut p = Pipeline::with_gpu(
        std::sync::Arc::new(argentum_engine::standard()),
        GpuSlot::with(gpu),
    )
    .unwrap();
    p.set::<rt_capture_sharpen::RtCaptureSharpen>(rt_capture_sharpen::Settings {
        capture: false,
        ..Default::default()
    })
    .unwrap();
    // Every tool after exposure at the photo's own defaults or off, so the
    // render is exposure's own result.
    p.set::<AgDngStandard>(ag_dng_standard::Settings {
        enabled: false,
        ..Default::default()
    })
    .unwrap();
    let before = p.render(&source, Size::Full).unwrap();
    p.set::<AgExposure>(ag_exposure::Settings { exposure_ev: 0.7 })
        .unwrap();
    let after = p.render(&source, Size::Full).unwrap();
    let gain = 2.0f32.powf(0.7);
    for (a, b) in after
        .as_rgb()
        .unwrap()
        .pixels
        .iter()
        .zip(&before.as_rgb().unwrap().pixels)
    {
        for c in 0..3 {
            let exact = b[c] * gain;
            assert!(
                (a[c] - exact).abs() <= exact.abs() / 1024.0 + 1e-6,
                "{} vs {exact}",
                a[c]
            );
        }
    }
}
