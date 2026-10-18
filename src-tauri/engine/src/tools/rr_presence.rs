//! Vibrance and saturation: RapidRAW's, as the app runs them.
//!
//! Both scale each pixel's distance from its own luminance. Saturation does
//! it evenly. Vibrance raises muted colours more than vivid ones and holds
//! back on skin tones; turned down, it takes the most from the most
//! saturated.
//!
//! From the app @ c786c146: `apply_creative_color` in
//! `src-tauri/src/shaders/shader.wgsl`, run in the pixel pass as written,
//! and the sliders' scale (`SCALES` in `src-tauri/src/image_processing.rs`).
//! The settings are the app's slider values, -100 to 100.

use serde::{Deserialize, Serialize};

use crate::{Context, Field, Pixel, Section, Stage, Tool};

/// The app's slider value that means a factor of 1 (`SCALES`).
const SLIDER_SCALE: f32 = 100.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub vibrance: f32,
    pub saturation: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            vibrance: 0.0,
            saturation: 0.0,
        }
    }
}

impl crate::Settings for Settings {
    const VERSION: u32 = 1;
    const FIELDS: &'static [Field] = &[
        Field::new("vibrance").range(-100.0, 100.0).step(1.0),
        Field::new("saturation").range(-100.0, 100.0).step(1.0),
    ];
}

#[derive(Default)]
pub struct RrPresence;

impl Tool for RrPresence {
    const ID: &'static str = "rr_presence";
    const STAGE: Stage = Stage::SceneLinear;
    const SECTION: Section = Section::Basic;
    const PIXEL: Option<Pixel> = Some(Pixel {
        wgsl: WGSL,
        params: 1,
        detail: false,
    });
    type Settings = Settings;

    /// Saturation and vibrance as the shader takes them, the slider value
    /// over 100.
    fn pixel_params(&self, settings: &Settings, _cx: &Context) -> Option<Vec<[f32; 4]>> {
        let sat = settings.saturation / SLIDER_SCALE;
        let vib = settings.vibrance / SLIDER_SCALE;
        (sat != 0.0 || vib != 0.0).then(|| vec![[sat, vib, 0.0, 0.0]])
    }
}

const WGSL: &str = r#"
// apply_creative_color, src-tauri/src/shaders/shader.wgsl @ c786c146
// (RapidRAW), as written there.
fn apply_creative_color(color: vec3<f32>, sat: f32, vib: f32) -> vec3<f32> {
    if (sat == 0.0 && vib == 0.0) {
        return color;
    }

    var processed = color;
    let luma = get_luma(max(processed, vec3<f32>(0.0)));

    let srgb = linear_to_srgb_extended(max(processed, vec3<f32>(0.0)));
    let hsv = rgb_to_hsv(srgb);
    let current_sat = hsv.y;
    let hue = hsv.x;

    var vib_factor: f32 = 0.0;
    if (vib != 0.0) {
        if (vib > 0.0) {
            let sat_weight = pow(max(1.0 - current_sat, 0.0), 1.25);
            let skin_center = 25.0;
            let hue_dist = min(abs(hue - skin_center), 360.0 - abs(hue - skin_center));
            let is_skin = 1.0 - smoothstep(12.0, 38.0, hue_dist);
            let skin_dampener = mix(1.0, 0.50, is_skin);

            vib_factor = vib * sat_weight * skin_dampener * 1.5;
        } else {
            let desat_weight = smoothstep(0.05, 0.75, current_sat);
            vib_factor = vib * desat_weight;
        }
    }

    let sat_scale = max(1.0 + sat, 0.0);
    let vib_scale = max(1.0 + vib_factor, 0.0);
    let final_mult = sat_scale * vib_scale;

    processed = mix(vec3<f32>(luma), processed, final_mult);

    return max(processed, vec3<f32>(0.0));
}

fn rr_presence(c: vec3<f32>, px: Px) -> vec3<f32> {
    let a = ag_params[px.p];
    return apply_creative_color(c, a.x, a.y);
}
"#;
