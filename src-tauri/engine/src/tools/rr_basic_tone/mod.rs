//! Contrast, highlights, shadows, whites and blacks: RapidRAW's, as the app
//! runs them.
//!
//! Each works on luminance and keeps colour: a pixel is scaled by how far
//! its luminance moved. Highlights, shadows, whites and blacks move the light
//! a pixel sits in rather than its texture - RapidRAW's guided filter on log
//! luminance says which is which - and put the texture back on top, so
//! lifting the shadows does not flatten them. Contrast is an S-curve around
//! middle grey, leaving values above white alone.
//!
//! From the app @ c786c146: `apply_tonal_adjustments`,
//! `apply_highlights_adjustment`, `shape_detail` and `sample_gf_tex` in
//! `src-tauri/src/shaders/shader.wgsl`, and the part of its `main` that
//! measures each pixel's detail, in `tone.wgsl`, run in the pixel pass; the
//! sliders' scale (`SCALES` in `src-tauri/src/image_processing.rs`). The
//! settings are the app's slider values, -100 to 100.
//!
//! The detail is measured as the app measures it: the guided filter of the
//! pixel pass's input, before exposure and white balance, kept on the GPU
//! for as long as that input is the same. The app filters its input before
//! capture sharpening; this engine after it.

use serde::{Deserialize, Serialize};

use crate::{Context, Field, Pixel, Section, Stage, Tool};

/// The app's slider value that means 1 in the shader, per slider (`SCALES`).
const CONTRAST_SCALE: f32 = 100.0;
const HIGHLIGHTS_SCALE: f32 = 120.0;
const SHADOWS_SCALE: f32 = 120.0;
const WHITES_SCALE: f32 = 40.0;
const BLACKS_SCALE: f32 = 40.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
        }
    }
}

impl crate::Settings for Settings {
    const VERSION: u32 = 1;
    const FIELDS: &'static [Field] = &[
        Field::new("contrast").range(-100.0, 100.0).step(1.0),
        Field::new("highlights").range(-100.0, 100.0).step(1.0),
        Field::new("shadows").range(-100.0, 100.0).step(1.0),
        Field::new("whites").range(-100.0, 100.0).step(1.0),
        Field::new("blacks").range(-100.0, 100.0).step(1.0),
    ];
}

#[derive(Default)]
pub struct RrBasicTone;

impl Tool for RrBasicTone {
    const ID: &'static str = "rr_basic_tone";
    const STAGE: Stage = Stage::SceneLinear;
    const SECTION: Section = Section::Basic;
    const PIXEL: Option<Pixel> = Some(Pixel {
        wgsl: include_str!("tone.wgsl"),
        params: 2,
        detail: true,
    });
    type Settings = Settings;

    /// The sliders as the shader takes them: contrast, shadows, whites and
    /// blacks, then highlights.
    fn pixel_params(&self, s: &Settings, _cx: &Context) -> Option<Vec<[f32; 4]>> {
        if *s == Settings::default() {
            return None;
        }
        Some(vec![
            [
                s.contrast / CONTRAST_SCALE,
                s.shadows / SHADOWS_SCALE,
                s.whites / WHITES_SCALE,
                s.blacks / BLACKS_SCALE,
            ],
            [s.highlights / HIGHLIGHTS_SCALE, 0.0, 0.0, 0.0],
        ])
    }
}
