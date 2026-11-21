//! Camera Look: a colour look fitted to one photo's embedded JPEG, the
//! camera's own rendering of it, over DNG Standard.
//!
//! The look is an HSV table (72 hues x 8 saturations x 8 values, value
//! sRGB-encoded, in linear ProPhoto RGB, as a DNG look table is) and a tone
//! curve after DNG Standard's, with gains on grey that the table cannot make
//! (it has no hue to work on there). `look.rs` applies it; the RAW tone stage
//! (`ag_stage_tone`, mode 5, in the app's modules.wgsl) is its GPU twin.
//!
//! The app hands `fit` the photo at the proxy's size (`pairs::PROXY_EDGE` on
//! the long edge, reduced in boxes from `pairs::RENDER_FACTOR` times that) as
//! the RAW tone stage receives it, the embedded JPEG, and the photo's DNG
//! Standard curve. The JPEG is aligned with the photo and masked where it
//! can't be learned from (`pairs`); a third of the photo, in whole tiles, is
//! held out. A look is used only when it beats DNG Standard on the held-out
//! tiles (`gate`): a colour look, else one of the curve alone. The one used
//! is then fitted again to every pair, the held-out ones too. Fitting is
//! deterministic: every loop runs in a fixed order.
//!
//! FROM RAWMAKASE
//!
//! RAWmakase `v0.7.1` @ 78069aa3 (MIT), Camera Look (fitted), first
//! shipped in v0.4.10 (60371ad1, f653fcb7, e1c80bd9, 19ca2ce2):
//! - `crates/rawmakase-decode/src/camera_look/solve.rs`: the quantile curve,
//!   the curve refit, the table's weighted least squares with tied
//!   neighbours, the slope limits and the grey gains, as written (3d1e35a6,
//!   "fit skin closer to the camera's JPEG", is in it).
//! - `.../camera_look/gate.rs`: the scores and the refusals, as written.
//! - `.../camera_look/pairs.rs`: the alignment search, the masks, the
//!   held-out tiles and the box reductions, as written.
//! - `.../camera_look/mod.rs`: `fit_pairs`, the order of it all.
//! - `crates/rawmakase-model/src/camera_profiles/mod.rs`: `Table::apply` and
//!   `Table::lookup` (the DNG SDK's HSV table), `CameraProfile::finish`
//!   (the tone curve on the darkest and brightest channels); `enhanced.rs`
//!   `apply_curve`; `fitted.rs` `FittedLook` and `TABLE_DIMS`.
//!
//! WHAT CHANGED IN THE COPY
//!
//! - The base is the app's DNG Standard for the photo, not RAWmakase
//!   Standard: its tone (the camera's exposure offset and Adobe's curve,
//!   `Tone`) is handed in, and the grey gains are applied to the stage's
//!   ProPhoto input instead of being folded into a forward matrix.
//! - No matrix is fitted: every photo the app opens has a colour matrix
//!   (RAWmakase 0ddfb5f3 fits one for a camera without, which the app's
//!   decoder doesn't open).
//! - No black and white look: a monochrome JPEG is refused, and the app
//!   falls back to Auto-Matched.
//! - The preview's colour space, the decode and the cache are the app's
//!   (src-tauri/src/mods/camera_look.rs).
//! - No Profile Amount, no XMP carrying: the fit is the photo's, cached.

mod gate;
mod look;
pub mod pairs;
mod solve;

pub use gate::Scores;
pub use look::{CURVE_SAMPLES, Look, TABLE_DIMS, Tone};
pub use pairs::{Alignment, Preview, Proxy};

use crate::colour::{PROPHOTO_TO_SRGB, mul};
use serde::{Deserialize, Serialize};

pub type Rgb = [f32; 3];

/// The fit's version: part of a fit cache's key, raised whenever what a fit
/// makes of a photo changes.
pub const FIT_VERSION: u32 = 1;

/// Which look a fit settled on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Colour,
    ToneOnly,
}

/// A look a fit tried, how it scored on the held-out tiles, and why it was not
/// used, if it was not.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub kind: Kind,
    pub scores: Scores,
    pub refused: Option<String>,
}

/// What a fit measured, used or refused.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Report {
    pub alignment: Alignment,
    pub fit_pairs: usize,
    pub holdout_pairs: usize,
    /// The share of held-out pairs in colour.
    pub coloured: f32,
    /// DNG Standard, the render the look replaces.
    pub baseline: Scores,
    pub candidates: Vec<Candidate>,
    pub kind: Option<Kind>,
    /// What the base's grey was tinted by to match the camera's.
    pub grey_gains: Option<[f32; 3]>,
}

/// Why a photo gets no fitted look, with what was measured up to then.
#[derive(Clone, Debug)]
pub struct Refusal {
    pub reason: String,
    pub report: Box<Report>,
}

fn refuse(reason: impl Into<String>) -> Refusal {
    Refusal {
        reason: reason.into(),
        report: Box::default(),
    }
}

/// The photo as the RAW tone stage receives it, at the stage's size
/// (`pairs::RENDER_FACTOR` times the proxy's): linear sRGB, NaN outside the
/// photo, and whether the sensor clipped each pixel.
pub struct Stage {
    pub width: u32,
    pub height: u32,
    pub linear: Vec<Rgb>,
    pub clipped: Vec<bool>,
}

/// DNG Standard's render of linear sRGB `linear` through `base`: the stage's
/// mode 3, linear sRGB out.
pub fn dng_standard(base: &Tone, linear: Rgb) -> Rgb {
    let pro = mul(crate::colour::SRGB_TO_PROPHOTO, linear).map(|v| v.clamp(0., 1.));
    mul(PROPHOTO_TO_SRGB, base.rgb_tone(pro))
}

/// Fits a camera look to `stage`, the photo, from `preview`, its embedded JPEG
/// as linear ProPhoto, over DNG Standard's `base` tone; or says why it can't.
pub fn fit(stage: &Stage, preview: Preview, base: &Tone) -> Result<(Look, Report), Refusal> {
    let mut report = Report::default();
    fit_into(stage, preview, base, &mut report)
        .map_err(|mut e| {
            e.report = Box::new(report.clone());
            e
        })
        .map(|look| (look, report))
}

fn fit_into(
    stage: &Stage,
    preview: Preview,
    base: &Tone,
    report: &mut Report,
) -> Result<Look, Refusal> {
    let proxy = pairs::proxy(stage);
    let (w, h) = (proxy.width, proxy.height);
    let preview = pairs::to_aspect(preview, w, h);
    let (sampled, alignment) = pairs::align(&preview, &proxy)?;
    report.alignment = alignment;
    let usable = pairs::usable(&proxy, &sampled);
    let held = pairs::holdout(w, h);
    let target = sampled.pro;
    let fitting: Vec<bool> = usable.iter().zip(&held).map(|(u, h)| *u && !h).collect();
    let testing: Vec<bool> = usable.iter().zip(&held).map(|(u, h)| *u && *h).collect();
    report.fit_pairs = fitting.iter().filter(|v| **v).count();
    report.holdout_pairs = testing.iter().filter(|v| **v).count();
    let target_srgb: Vec<Rgb> = target.iter().map(|p| mul(PROPHOTO_TO_SRGB, *p)).collect();
    report.coloured = gate::coloured(&target_srgb, &testing);
    if pairs::monochrome(&target_srgb, &usable) {
        return Err(refuse("The camera's JPEG is black and white"));
    }
    if report.fit_pairs < 256 {
        return Err(refuse(format!("Only {} pairs to fit", report.fit_pairs)));
    }
    let pick = |v: &[Rgb], mask: &[bool]| -> Vec<Rgb> {
        v.iter()
            .zip(mask)
            .filter(|(_, m)| **m)
            .map(|(p, _)| *p)
            .collect()
    };
    // What the look's table receives, before the grey gains: the stage's
    // ProPhoto, at the proxy's size and at the stage's.
    let to_pro = |p: &Rgb| mul(crate::colour::SRGB_TO_PROPHOTO, *p);
    let scene: Vec<Rgb> = proxy.linear.iter().map(to_pro).collect();
    let full: Vec<Rgb> = stage.linear.iter().map(to_pro).collect();
    let fit_target = pick(&target, &fitting);
    // The camera's grey, which a look's table can't move, is the gains' to match.
    let gains = solve::grey_gains(&pick(&scene, &fitting), &fit_target);
    report.grey_gains = gains;
    let gains = gains.unwrap_or([1.; 3]);
    let scene: Vec<Rgb> = scene.iter().map(|p| look::gained(*p, gains)).collect();
    // Renders judge a look at the stage's size, reduced in the proxy's boxes.
    let render = |f: &dyn Fn(Rgb) -> Rgb, pixels: &[Rgb]| -> Vec<Rgb> {
        let made: Vec<Rgb> = pixels.iter().map(|p| f(*p)).collect();
        pairs::reduce(&made, stage.width, stage.height).2
    };
    let baseline = render(&|p| dng_standard(base, p), &stage.linear);
    report.baseline = gate::score(&baseline, &target_srgb, &testing, w, h);
    let fit_scene = pick(&scene, &fitting);
    // The look of `kind` that takes `scene` to `target`.
    let look_of = |kind: Kind, scene: &[Rgb], target: &[Rgb]| {
        let mut look = match kind {
            Kind::Colour => solve::colour_look(base, scene, target),
            Kind::ToneOnly => solve::tone_look(base, scene, target),
        };
        look.gains = gains;
        look
    };
    // The tone-only look is the fallback: only a refused colour look needs it.
    for kind in [Kind::Colour, Kind::ToneOnly] {
        let look = look_of(kind, &fit_scene, &fit_target);
        let made = render(&|p| mul(PROPHOTO_TO_SRGB, look.render(base, p)), &full);
        let scores = gate::score(&made, &target_srgb, &testing, w, h);
        let refused = match kind {
            Kind::Colour => gate::colour_refusal(&scores, &report.baseline, report.coloured),
            Kind::ToneOnly => gate::tone_refusal(&scores, &report.baseline),
        };
        let accepted = refused.is_none();
        report.candidates.push(Candidate {
            kind,
            scores,
            refused,
        });
        if accepted {
            report.kind = Some(kind);
            // The held-out third has judged the look; the one used learns from
            // it too. A small face may lie whole in a held-out tile, and its
            // colours would otherwise be guessed from the rest of the photo's.
            return Ok(look_of(
                kind,
                &pick(&scene, &usable),
                &pick(&target, &usable),
            ));
        }
    }
    let why = report
        .candidates
        .iter()
        .filter_map(|c| c.refused.clone())
        .collect::<Vec<_>>()
        .join("; ");
    Err(refuse(format!("No fitted look beats DNG Standard: {why}")))
}

/// How each of `renders` of `stage` (linear sRGB at the stage's size)
/// compares with `preview`, lined up as a fit lines it up: on every usable
/// pixel, and on the held-out ones alone. For measuring a fit against other
/// renderings.
pub fn measure(
    stage: &Stage,
    preview: Preview,
    renders: &[Vec<Rgb>],
) -> Result<Vec<[Scores; 2]>, Refusal> {
    let proxy = pairs::proxy(stage);
    let (w, h) = (proxy.width, proxy.height);
    let preview = pairs::to_aspect(preview, w, h);
    let (sampled, _) = pairs::align(&preview, &proxy)?;
    let usable = pairs::usable(&proxy, &sampled);
    let held: Vec<bool> = pairs::holdout(w, h)
        .iter()
        .zip(&usable)
        .map(|(h, u)| *h && *u)
        .collect();
    let target: Vec<Rgb> = sampled
        .pro
        .iter()
        .map(|p| mul(PROPHOTO_TO_SRGB, *p))
        .collect();
    Ok(renders
        .iter()
        .map(|made| {
            let made = pairs::reduce(made, stage.width, stage.height).2;
            [
                gate::score(&made, &target, &usable, w, h),
                gate::score(&made, &target, &held, w, h),
            ]
        })
        .collect())
}

/// How far each of `renders` (as `measure` takes them) turns skin from the
/// JPEG's hue: the mean absolute difference of CIELAB hue angle, in degrees,
/// on the usable pixels whose colour in the JPEG is skin's (hue 20 to 80
/// degrees, chroma 8 to 50, lightness 20 to 90), and how many there were.
pub fn skin_hue_error(
    stage: &Stage,
    preview: Preview,
    renders: &[Vec<Rgb>],
) -> Result<(usize, Vec<f32>), Refusal> {
    use crate::colour::lab;
    let proxy = pairs::proxy(stage);
    let (w, h) = (proxy.width, proxy.height);
    let preview = pairs::to_aspect(preview, w, h);
    let (sampled, _) = pairs::align(&preview, &proxy)?;
    let usable = pairs::usable(&proxy, &sampled);
    let hue_chroma = |p: Rgb| {
        let l = lab(p.map(|v| v.clamp(0., 1.)));
        (
            l[0],
            l[2].atan2(l[1]).to_degrees().rem_euclid(360.),
            l[1].hypot(l[2]),
        )
    };
    let skin: Vec<(usize, f64)> = sampled
        .pro
        .iter()
        .enumerate()
        .filter(|(i, _)| usable[*i])
        .filter_map(|(i, p)| {
            let (l, hue, chroma) = hue_chroma(mul(PROPHOTO_TO_SRGB, *p));
            ((20.0..90.).contains(&l) && (20.0..80.).contains(&hue) && (8.0..50.).contains(&chroma))
                .then_some((i, hue))
        })
        .collect();
    let errors = renders
        .iter()
        .map(|made| {
            let made = pairs::reduce(made, stage.width, stage.height).2;
            let sum: f64 = skin
                .iter()
                .map(|(i, hue)| {
                    let (_, got, _) = hue_chroma(made[*i]);
                    ((got - hue + 180.).rem_euclid(360.) - 180.).abs()
                })
                .sum();
            (sum / skin.len().max(1) as f64) as f32
        })
        .collect();
    Ok((skin.len(), errors))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camera that renders as DNG Standard with a known look on top: the
    /// fit finds a look that beats DNG Standard, and the same one twice.
    #[test]
    fn a_camera_look_is_found_and_found_again_to_the_bit() {
        let base = look::tests::base();
        let camera = look::tests::known_look();
        let stage = pairs::tests::stage_of_blobs(768, 512);
        let preview = pairs::tests::preview_through(&stage, |p| {
            mul(
                PROPHOTO_TO_SRGB,
                camera.render(&base, mul(crate::colour::SRGB_TO_PROPHOTO, p)),
            )
        });
        let (fitted, report) =
            fit(&stage, preview.clone(), &base).unwrap_or_else(|e| panic!("{}", e.reason));
        assert_eq!(report.kind, Some(Kind::Colour), "{report:?}");
        let used = &report.candidates[0].scores;
        assert!(used.de00 < 0.5 * report.baseline.de00, "{report:?}");
        let (again, _) = fit(&stage, preview, &base).unwrap_or_else(|e| panic!("{}", e.reason));
        assert_eq!(fitted, again);
    }

    /// A JPEG of another picture is refused, with the reason.
    #[test]
    fn another_picture_is_refused() {
        let base = look::tests::base();
        let stage = pairs::tests::stage_of_blobs(768, 512);
        let other = pairs::tests::stage_of_stripes(768, 512);
        let preview = pairs::tests::preview_through(&other, |p| p);
        let refused = fit(&stage, preview, &base).expect_err("refused");
        assert!(refused.reason.contains("line up"), "{}", refused.reason);
    }
}
