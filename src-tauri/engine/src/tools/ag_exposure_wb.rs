//! Exposure and white balance, as three multipliers. Ours.
//!
//! Each channel of each pixel is multiplied by one factor: 2 to the power of
//! the exposure in stops, times that channel's white balance multiplier. The
//! factors are worked out once, in f32, and every pixel takes exactly one
//! multiplication - so the result can be checked exactly, not approximately.
//!
//! It runs on linear RGB after the develop, which has already applied the
//! camera's as-shot white balance with its colour matrix. The multipliers are
//! a correction on top of that: all ones is the camera's own balance.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{Context, Error, Field, Image, Result, Section, Stage, Tool};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Stops: +1 doubles the light.
    pub exposure: f32,
    /// White balance multipliers. All ones leaves the balance as shot.
    pub red: f32,
    pub green: f32,
    pub blue: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            red: 1.0,
            green: 1.0,
            blue: 1.0,
        }
    }
}

impl crate::Settings for Settings {
    const VERSION: u32 = 1;
    const FIELDS: &'static [Field] = &[
        Field::new("exposure").range(-10.0, 10.0).step(0.01).unit("EV"),
        Field::new("red").range(0.05, 20.0).step(0.001).log(),
        Field::new("green").range(0.05, 20.0).step(0.001).log(),
        Field::new("blue").range(0.05, 20.0).step(0.001).log(),
    ];
}

/// What each channel is multiplied by.
pub fn factors(settings: &Settings) -> [f32; 3] {
    let gain = 2.0f32.powf(settings.exposure);
    [
        gain * settings.red,
        gain * settings.green,
        gain * settings.blue,
    ]
}

#[derive(Default)]
pub struct AgExposureWb;

impl Tool for AgExposureWb {
    const ID: &'static str = "ag_exposure_wb";
    const STAGE: Stage = Stage::SceneLinear;
    const SECTION: Section = Section::Basic;
    type Settings = Settings;

    fn process(&self, settings: &Settings, input: &Image, _cx: &Context) -> Result<Option<Image>> {
        let rgb = input.as_rgb().ok_or_else(|| Error::Tool {
            tool: Self::ID,
            message: format!("was handed {}", input.describe()),
        })?;
        let f = factors(settings);
        if f == [1.0; 3] {
            return Ok(None);
        }
        let pixels = rgb
            .pixels
            .par_iter()
            .map(|p| [p[0] * f[0], p[1] * f[1], p[2] * f[2]])
            .collect();
        Ok(Some(Image::Rgb(rgb.with_pixels(pixels))))
    }
}
