//! RawTherapee's capture sharpening, on the engine's own GPU device.
//!
//! Richardson-Lucy deconvolution of the luminance with a gaussian PSF: it
//! undoes the blur of the lens and the sensor rather than adding edge
//! contrast. RawTherapee `rtengine/capturesharpening.cc`
//! @ c6d04960e14fed89e531cbbdf680021d131b4685, with its contrast mask from
//! `rtengine/rt_algo.cc` @ 1c2b3f33772eaf491b7bd4d66323ac36d13aa344, so flat
//! areas - sky, skin, noise - are left alone.
//!
//! Copied from the app @ 2bcb953d: the settings, the radius read from the
//! RAW (`radius.rs`) and the automatic contrast threshold (`contrast.rs`)
//! from `src-tauri/src/mods/sharpen.rs`, the passes (`gpu.rs`,
//! `passes.wgsl`) from `mods/sharpen_gpu.rs` and `shaders/sharpen.wgsl`.
//! Capture sharpening only - the app's darktable unsharp mask, each photo
//! mask's own Sharpen and the mask view are not part of this tool.
//!
//! WHAT CHANGED IN THE COPY
//!
//! - Its own device (`crate::Gpu`), not RapidRAW's processor.
//! - The pipeline's f32 pixels go up as an rgba32float texture, and what
//!   comes back is the factor each pixel's luminance moved by, which is
//!   applied to the f32 pixels here. The app reads and writes half floats.
//!   Given the same input, the two agree to within one half-float step.
//! - The whole picture every time. The pipeline keeps the result, so the
//!   app's kept textures, regions of interest and panning have no job here.
//! - The automatic contrast threshold is measured on the picture this tool
//!   is given. The app measures on the full-size decode even for a preview;
//!   at a preview's size the engine measures the preview. At fit-to-screen
//!   capture sharpening is below a pixel and skipped either way.
//!
//! RADII ARE IN FULL-RESOLUTION PIXELS
//!
//! The radius means the same thing at every size: it is scaled by the
//! picture's render scale, as darktable scales by `roi_in->scale` and
//! RawTherapee divides by its preview scale.

mod contrast;
mod gpu;
mod radius;

use std::sync::{Arc, Mutex};

use rawler::rawimage::RawImage;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{Context, Equals, Error, Field, Gpu, Image, Result, Rgb, Section, Stage, Tool};
pub use contrast::measure_contrast;
use gpu::{Passes, Plan};
pub use radius::measure_radius;

/// RawTherapee's defaults, from `rtgui/tools/pdsharpening.cc`
/// @ c777f374b4db9ec5ecec3dcfea2e857382208ea9: contrast 10 (auto on),
/// radius 0.75 (auto on), corner boost 0, 20 iterations.
pub const DEFAULT_CONTRAST: f32 = 10.0;
pub const DEFAULT_RADIUS: f32 = 0.75;
pub const DEFAULT_ITERATIONS: u32 = 20;

/// The widest radius, in full-resolution pixels, anywhere: RawTherapee's
/// ceiling on a measured one (`maxSigma`), on a manual one, and on the radius
/// the corner boost reaches.
pub const MAX_RADIUS_PX: f32 = 2.0;

/// The settings, named as the app's `agSharpen` keys for the same things.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// On for every RAW unless turned off.
    pub capture: bool,
    /// 0..100: how much of the deconvolved luminance is blended in.
    pub capture_amount: f32,
    /// Read the radius from the RAW's own sharpest edge.
    pub auto_radius: bool,
    /// Gaussian sigma of the PSF, full-resolution pixels, when not automatic.
    pub radius: f32,
    /// RawTherapee's corner boost: sigma added at the corners.
    pub corner_boost: f32,
    pub iterations: u32,
    /// RawTherapee's check that stops a 32 px block once deconvolution starts
    /// to ring.
    pub iter_check: bool,
    /// Find the contrast threshold from the flattest part of the photo.
    pub auto_contrast: bool,
    /// RawTherapee's contrast threshold, when not automatic. 0 sharpens
    /// everywhere.
    pub contrast: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            capture: true,
            capture_amount: 100.0,
            auto_radius: true,
            radius: DEFAULT_RADIUS,
            corner_boost: 0.0,
            iterations: DEFAULT_ITERATIONS,
            iter_check: true,
            auto_contrast: true,
            contrast: DEFAULT_CONTRAST,
        }
    }
}

impl crate::Settings for Settings {
    const VERSION: u32 = 1;
    // In the order of the app's Sharpening card, with the app's clamps from
    // `sharpen::from_json`. Settings are clamped to these when they are read
    // or set, so `process` takes them as they are.
    const FIELDS: &'static [Field] = &[
        Field::new("capture"),
        Field::new("captureAmount")
            .range(0.0, 100.0)
            .step(1.0)
            .unit("%"),
        Field::new("autoRadius"),
        Field::new("radius")
            .range(0.4, MAX_RADIUS_PX as f64)
            .step(0.01)
            .unit("px")
            .enabled_when("autoRadius", Equals::Bool(false)),
        Field::new("autoContrast"),
        Field::new("contrast")
            .range(0.0, 200.0)
            .step(1.0)
            .enabled_when("autoContrast", Equals::Bool(false)),
        Field::new("cornerBoost")
            .range(-0.5, 0.5)
            .step(0.01)
            .unit("px")
            .advanced(),
        Field::new("iterations").range(1.0, 100.0).advanced(),
        Field::new("iterCheck").advanced(),
    ];
}

#[derive(Default)]
pub struct RtCaptureSharpen {
    /// The radius the last photo's mosaic gave, by source key. A pass over
    /// the whole mosaic, made once per photo.
    radius: Mutex<Option<(u64, Option<f32>)>>,
    /// The contrast threshold last measured, by input key.
    contrast: Mutex<Option<(u64, f32)>>,
    passes: Mutex<Option<Passes>>,
}

impl Tool for RtCaptureSharpen {
    const ID: &'static str = "rt_capture_sharpen";
    const STAGE: Stage = Stage::SceneLinear;
    const SECTION: Section = Section::Detail;
    const GPU: bool = true;
    type Settings = Settings;

    fn process(&self, settings: &Settings, input: &Image, cx: &Context) -> Result<Option<Image>> {
        let rgb = input.as_rgb().ok_or_else(|| Error::Tool {
            tool: Self::ID,
            message: format!("was handed {}", input.describe()),
        })?;
        if !settings.capture {
            return Ok(None);
        }
        let radius = if settings.auto_radius {
            self.auto_radius(cx.source.key, &cx.source.raw)
                .unwrap_or(DEFAULT_RADIUS)
        } else {
            settings.radius
        };
        let resolved = Resolved {
            amount: settings.capture_amount / 100.0,
            radius,
            corner: settings.corner_boost,
            iterations: settings.iterations,
            iter_check: settings.iter_check,
            contrast: 0.0,
        };
        // Asked before the threshold is measured, since that is a pass over
        // the whole photo: at fit-to-screen there is nothing to do.
        if gpu::plan(&resolved, rgb.width, rgb.height, rgb.scale()).is_none() {
            return Ok(None);
        }
        let contrast = if settings.auto_contrast {
            self.auto_contrast(cx.input_key, rgb)
        } else {
            settings.contrast / 100.0
        };
        let resolved = Resolved {
            contrast,
            ..resolved
        };
        let Some(plan) = gpu::plan(&resolved, rgb.width, rgb.height, rgb.scale()) else {
            return Ok(None);
        };
        let gpu = cx.gpu()?;
        let pixels = self.sharpen(&gpu, rgb, &plan)?;
        Ok(Some(Image::Rgb(rgb.with_pixels(pixels))))
    }
}

impl RtCaptureSharpen {
    fn auto_radius(&self, key: u64, raw: &RawImage) -> Option<f32> {
        let mut memo = self.radius.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((k, r)) = *memo
            && k == key
        {
            return r;
        }
        let r = measure_radius(raw);
        *memo = Some((key, r));
        r
    }

    fn auto_contrast(&self, key: u64, rgb: &Rgb) -> f32 {
        let mut memo = self.contrast.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((k, c)) = *memo
            && k == key
        {
            return c;
        }
        let c = measure_contrast(rgb);
        *memo = Some((key, c));
        c
    }

    /// Run the passes and apply what they found to `rgb`'s own pixels.
    fn sharpen(&self, gpu: &Arc<Gpu>, rgb: &Rgb, plan: &Plan) -> Result<Vec<[f32; 3]>> {
        let mut slot = self.passes.lock().unwrap_or_else(|e| e.into_inner());
        if slot.as_ref().is_none_or(|p| !Arc::ptr_eq(&p.gpu, gpu)) {
            *slot = Some(Passes::new(gpu));
        }
        let passes = slot.get_or_insert_with(|| Passes::new(gpu));
        let ratios = passes.ratios(rgb, plan)?;
        Ok(rgb
            .pixels
            .par_iter()
            .zip(ratios.par_iter())
            .map(|(p, &r)| {
                // Exactly 1 where nothing moved: copied, not multiplied.
                if r == 1.0 {
                    *p
                } else {
                    [p[0] * r, p[1] * r, p[2] * r]
                }
            })
            .collect())
    }
}

/// The settings with everything automatic worked out.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Resolved {
    /// 0..1.
    amount: f32,
    radius: f32,
    corner: f32,
    iterations: u32,
    iter_check: bool,
    /// 0..1. 0 sharpens everywhere.
    contrast: f32,
}
