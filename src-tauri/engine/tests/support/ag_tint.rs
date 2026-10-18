//! A per-pixel tool, written the way any new one is: this one file, with its
//! WGSL, and one line where the registry is built. Nothing in the engine -
//! the pixel pass least of all - knows it exists.
//!
//! It scales red and blue, each by one plus its setting: zeros leave the
//! colour alone, as every per-pixel tool's must.

use argentum_engine::{Context, Field, Pixel, Section, Stage, Tool};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub red: f32,
    pub blue: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            red: 0.0,
            blue: 0.0,
        }
    }
}

impl argentum_engine::Settings for Settings {
    const VERSION: u32 = 1;
    const FIELDS: &'static [Field] = &[
        Field::new("red").range(-1.0, 1.0).step(0.01),
        Field::new("blue").range(-1.0, 1.0).step(0.01),
    ];
}

#[derive(Default)]
pub struct AgTint;

impl Tool for AgTint {
    const ID: &'static str = "ag_tint";
    const STAGE: Stage = Stage::SceneLinear;
    const SECTION: Section = Section::Color;
    const PIXEL: Option<Pixel> = Some(Pixel {
        wgsl: WGSL,
        params: 1,
        detail: false,
    });
    type Settings = Settings;

    fn pixel_params(&self, s: &Settings, _cx: &Context) -> Option<Vec<[f32; 4]>> {
        (s.red != 0.0 || s.blue != 0.0).then(|| vec![[s.red, 0.0, s.blue, 0.0]])
    }
}

const WGSL: &str = r#"
fn ag_tint(c: vec3<f32>, px: Px) -> vec3<f32> {
    return c * (vec3<f32>(1.0) + ag_params[px.p].xyz);
}
"#;
