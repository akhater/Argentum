//! Exposure, in stops.
//!
//! Every channel times 2 to the power of the exposure: RapidRAW's
//! `apply_linear_exposure`, run in the pixel pass. The app's slider moves in
//! steps of 1.25 stops (`SCALES.exposure`); this one is in stops.
//!
//! It runs on linear RGB after the develop and capture sharpening, before
//! white balance, where the app runs it.

use serde::{Deserialize, Serialize};

use crate::{Context, Field, Pixel, Section, Stage, Tool};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// +1 doubles the light.
    pub exposure_ev: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { exposure_ev: 0.0 }
    }
}

impl crate::Settings for Settings {
    const VERSION: u32 = 1;
    const FIELDS: &'static [Field] = &[Field::new("exposure_ev")
        .range(-5.0, 5.0)
        .step(0.01)
        .unit("EV")];
}

#[derive(Default)]
pub struct AgExposure;

impl Tool for AgExposure {
    const ID: &'static str = "ag_exposure";
    const STAGE: Stage = Stage::SceneLinear;
    const SECTION: Section = Section::Basic;
    const PIXEL: Option<Pixel> = Some(Pixel {
        wgsl: WGSL,
        params: 1,
        detail: false,
    });
    type Settings = Settings;

    fn pixel_params(&self, settings: &Settings, _cx: &Context) -> Option<Vec<[f32; 4]>> {
        (settings.exposure_ev != 0.0).then(|| vec![[settings.exposure_ev, 0.0, 0.0, 0.0]])
    }
}

const WGSL: &str = r#"
// apply_linear_exposure, src-tauri/src/shaders/shader.wgsl @ c786c146
// (RapidRAW), as written there.
fn apply_linear_exposure(color_in: vec3<f32>, exposure_adj: f32) -> vec3<f32> {
    if (exposure_adj == 0.0) {
        return color_in;
    }
    return color_in * pow(2.0, exposure_adj);
}

fn ag_exposure(c: vec3<f32>, px: Px) -> vec3<f32> {
    return apply_linear_exposure(c, ag_params[px.p].x);
}
"#;
