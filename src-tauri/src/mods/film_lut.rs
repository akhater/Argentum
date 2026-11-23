//! What a film look that comes with the app is fed. Ours.
//!
//! THE LUTS
//!
//! The film looks (`resources/film_luts`) are spektrafilm LUTs, which RapidRAW
//! ships and applies in place of the rendering: their scene-referred path in
//! shader.wgsl encodes the scene-linear picture with the V-Log curve and looks
//! it up. Each file names its input in its header, "Panasonic V-Log". In
//! spektrafilm's LUT creator that is the V-Log curve over V-Gamut primaries
//! (`spektrafilm_lut_creator/color_spaces.py` @ 3bb2c2d2, the 0.3.4 the LUTs
//! were made with: `"Panasonic V-Log", "V-Gamut", "V-Log"`), the code values
//! decoded and handed to the film as V-Gamut RGB.
//!
//! WHAT RAPIDRAW FEEDS THEM
//!
//! The curve over Argentum's working RGB, which is linear sRGB. V-Gamut's
//! primaries lie far outside sRGB's, so every colour reaches the film as a
//! stronger one: 1.4 to 1.6 times the chroma (OkLab) for skin, sky and
//! foliage, skin turned 11 degrees toward red and blues 15 toward cyan.
//!
//! It also feeds them a RAW at the exposure the sensor left it, without the
//! camera's exposure offset that DNG Standard and Linear add (dng_standard.rs).
//! The film look replaces the rendering whole, so that offset, which rides in
//! the rendering's curve, never reached it, and every RAW went to the film a
//! fraction of a stop dark. spektrafilm takes linear 0.18 as middle grey for a
//! V-Log input (no exposure gain since 0.3.4, its commit 3bb2c2d2 "fix: vlog
//! midgray exposure"), and the offset is what puts a RAW's middle grey there.
//!
//! WHAT THIS FEEDS THEM
//!
//! What the file declares: the picture converted to V-Gamut before the curve
//! (`ag_film_lut_input` in modules.wgsl), and, for a RAW in DNG Standard or
//! Linear, at the offset its rendering carries, read back from the edit's own
//! curve (`dng_standard::offset_of`). The rendering's tone curve stays out: the
//! film's own curve is the rendering. Their curve and lookup are untouched.
//!
//! Blown highlights reach the film as DNG Standard shows them, each channel
//! capped at the sensor's white in ProPhoto (`ag_prophoto_to_white`).
//! `highlights::settle_blown` turns a blown area neutral in the decode, but
//! only where the file's white level is where the sensor really clipped; on
//! some 5D Mark II frames Canon's SpecularWhiteLevel sits above the point red
//! saturates, the area keeps the magenta white balance gives it, and the
//! renderings hide that by capping at white. Uncapped, the film printed it
//! pink.
//!
//! An enlargement (mods/super_resolution.rs) is an ordinary image at its RAW's
//! exposure, with no rendering of its own, so its edit carries the offset as a
//! number, `agLutExposureEv`.
//!
//! WHICH EDITS
//!
//! An edit that put a film look on before this keeps its look. A LUT is fed
//! what its file declares when the edit carries `"agLutDeclaredInput": true`,
//! which the Looks tab writes whenever it puts a LUT on or previews one
//! (src/argentum/looks/filmLookInput.ts), and Copy settings carries with the
//! LUT. Their Looks swatches (`generate_lut_previews`) show a LUT as it goes
//! on now. Only the scene-referred path reads it; a LUT of the user's goes
//! after the rendering, as before.

use serde_json::Value;

use crate::mods::dng_standard;
use crate::mods::raw_tone::{self, ToneCurvePoint};

/// The key in an edit that marks its LUT as fed what the LUT's file declares.
pub const KEY: &str = "agLutDeclaredInput";

/// The key in an enlargement's edit that carries the offset its RAW's
/// rendering gave a film look, in EV.
pub const CARRIED_EV_KEY: &str = "agLutExposureEv";

/// The `lutPath` of their LUT swatches (`generate_lut_previews` in
/// lut_processing.rs), which are rendered from no edit but this.
const SWATCH_LUT_PATH: &str = "preview";

/// Whether the edit's LUT is fed what its file declares, as the shader reads
/// it (`ag_film_lut_input` in their `GlobalAdjustments`): 1 or 0. A swatch
/// shows the LUT as it goes on now.
pub fn declared_input(js: &Value) -> u32 {
    u32::from(js[KEY].as_bool() == Some(true) || js["lutPath"].as_str() == Some(SWATCH_LUT_PATH))
}

/// The exposure a film look takes the picture at, in EV, as the shader reads
/// it (`ag_film_lut_ev`): for an edit fed the declared input, the offset an
/// enlargement carries or else its RAW's rendering's; 0 for anything else.
/// `raw_tone_mode` and `raw_tone_points` as `get_all_adjustments_from_json`
/// reads them.
pub fn exposure_ev(js: &Value, raw_tone_mode: u32, raw_tone_points: &[Value]) -> f32 {
    if declared_input(js) == 0 {
        return 0.0;
    }
    js[CARRIED_EV_KEY].as_f64().map_or_else(
        || rendering_ev(raw_tone_mode, raw_tone_points),
        |ev| ev as f32,
    )
}

/// The camera's exposure offset a RAW's rendering carries, in EV: DNG
/// Standard's and Linear's, read back from their curve; 0 for the others,
/// which have none, and for an ordinary image (`raw_tone_mode` 0).
pub fn rendering_ev(raw_tone_mode: u32, raw_tone_points: &[Value]) -> f32 {
    if !raw_tone::is_camera_raw_style(raw_tone_mode) {
        return 0.0;
    }
    let points: Vec<ToneCurvePoint> = raw_tone_points
        .iter()
        .filter_map(|p| serde_json::from_value(p.clone()).ok())
        .collect();
    let linear = raw_tone_mode == raw_tone::mode_index(dng_standard::LINEAR_MODE);
    dng_standard::offset_of(&points, linear).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::dng_standard::gpu::{centre, device, image_of, render_with_lut};
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn only_a_marked_edit_is_fed_the_declared_input() {
        assert_eq!(declared_input(&json!({ KEY: true })), 1);
        assert_eq!(declared_input(&json!({ KEY: false })), 0);
        assert_eq!(declared_input(&json!({ "lutIsSceneReferred": true })), 0);
        assert_eq!(declared_input(&Value::Null), 0);
        // Their swatch, as generate_lut_previews writes it.
        let swatch =
            json!({ "lutPath": "preview", "lutIntensity": 100, "lutIsSceneReferred": true });
        assert_eq!(declared_input(&swatch), 1);
    }

    /// A LUT whose output is its input, so the render shows what the LUT was
    /// handed.
    fn identity(size: u32) -> crate::lut_processing::Lut {
        let step = 1.0 / (size - 1) as f32;
        let mut data = Vec::with_capacity((size * size * size * 3) as usize);
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    data.extend([r as f32 * step, g as f32 * step, b as f32 * step]);
                }
            }
        }
        crate::lut_processing::Lut { size, data }
    }

    /// Their `linear_to_vlog` (shader.wgsl), Panasonic's V-Log curve.
    fn vlog(v: f32) -> f32 {
        let v = v.max(0.0);
        if v <= 0.01 {
            5.6 * v + 0.125
        } else {
            0.241514 * (v + 0.00873).log10() + 0.598206
        }
    }

    /// Linear sRGB to linear V-Gamut, made from the two sets of primaries by
    /// their `primaries_to_xyz_matrix` (image_processing.rs), the definition
    /// the shader's constant is checked against.
    fn srgb_to_v_gamut() -> glam::Mat3 {
        use crate::image_processing::{PRIMARIES_SRGB, WP_D65, primaries_to_xyz_matrix};
        use glam::Vec2;
        // Panasonic, "V-Log/V-Gamut Reference Manual", 2014: V-Gamut's
        // primaries; its white is D65.
        let v_gamut = [
            Vec2::new(0.730, 0.280),
            Vec2::new(0.165, 0.840),
            Vec2::new(0.100, -0.030),
        ];
        primaries_to_xyz_matrix(&v_gamut, WP_D65).inverse()
            * primaries_to_xyz_matrix(&PRIMARIES_SRGB, WP_D65)
    }

    /// The shader's constant is the conversion the primaries define, to the
    /// precision it is written in.
    #[test]
    fn the_shaders_matrix_is_v_gamuts() {
        const SOURCE: &str = include_str!("../shaders/modules.wgsl");
        let start = SOURCE
            .find("const AG_SRGB_TO_V_GAMUT")
            .expect("the constant is in modules.wgsl");
        let body = &SOURCE[start..start + SOURCE[start..].find(");").expect("its end")];
        let columns: Vec<f32> = body
            .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .filter(|s| s.contains('.'))
            .map(|s| s.parse().expect("a number"))
            .collect();
        assert_eq!(columns.len(), 9, "{body}");
        let written = glam::Mat3::from_cols_slice(&columns);
        let defined = srgb_to_v_gamut();
        for (w, d) in written.to_cols_array().iter().zip(defined.to_cols_array()) {
            assert!((w - d).abs() < 1e-6, "{written} is not {defined}");
        }
        for row in 0..3 {
            let sum: f32 = (0..3).map(|col| written.col(col)[row]).sum();
            assert!((sum - 1.0).abs() < 1e-6, "row {row} sums to {sum}");
        }
    }

    /// Greys, and colours at full and partial strength, in linear sRGB; then
    /// a blown area the decode settled to neutral, and one it could not, in
    /// the magenta white balance leaves (measured on a 5D Mark II frame).
    const PATCHES: [[f32; 3]; 12] = [
        [0.0, 0.0, 0.0],
        [0.18, 0.18, 0.18],
        [0.9, 0.9, 0.9],
        [0.6, 0.05, 0.03],
        [0.05, 0.5, 0.08],
        [0.04, 0.12, 0.7],
        [0.2, 0.35, 0.7],
        [0.7, 0.4, 0.25],
        [0.55, 0.1, 0.6],
        [1.0, 0.85, 0.1],
        [2.26, 2.26, 2.26],
        [3.34, 0.675, 2.09],
    ];

    /// What a marked film look is fed: capped at white in ProPhoto as DNG
    /// Standard caps it, at `ev`, in V-Gamut.
    fn fed_now(c: [f32; 3], ev: f32) -> [f32; 3] {
        use crate::mods::dng_standard::gpu::{FROM, TO, mul};
        let at_white = mul(FROM, mul(TO, c).map(|v| v.clamp(0.0, 1.0)));
        (srgb_to_v_gamut() * (glam::Vec3::from_array(at_white) * ev.exp2())).to_array()
    }

    /// How far the render of PATCHES with `js` and a LUT that hands back what
    /// it is given lands from V-Log over `fed(patch)`, in 1/255.
    fn worst_miss(js: Value, fed: impl Fn([f32; 3]) -> [f32; 3]) -> f32 {
        let ctx = device().expect("no wgpu adapter");
        let got = render_with_lut(
            &ctx,
            &image_of(&PATCHES),
            js,
            crate::white_balance::WhiteBalance::reference(),
            Some(Arc::new(identity(65))),
        );
        let mut worst = 0.0f32;
        for (i, c) in PATCHES.iter().enumerate() {
            let want = fed(*c).map(vlog);
            let g = centre(&got, i);
            let miss = (0..3).map(|k| (g[k] - want[k]).abs()).fold(0.0, f32::max) * 255.0;
            if miss > 1.0 {
                eprintln!("patch {c:?}: shader {g:?}, expected {want:?}");
            }
            worst = worst.max(miss);
        }
        worst
    }

    fn film_look(marked: bool, rendering: &str, curve: Vec<ToneCurvePoint>) -> Value {
        json!({
            "rawToneRendering": rendering,
            "rawToneCurve": curve,
            "rawToneCurveMode": rendering,
            "agSharpen": { "capture": false, "amount": 0 },
            "lutPath": "identity",
            "lutIntensity": 100,
            "lutIsSceneReferred": true,
            KEY: marked,
        })
    }

    /// The film look's input, through the whole render: V-Log over V-Gamut,
    /// blown highlights capped at white, for a marked edit; V-Log over the
    /// working RGB, as it comes, for one put on before.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn gpu_the_shader_feeds_a_marked_film_look_v_gamut() {
        let miss = worst_miss(film_look(true, "default", vec![]), |c| fed_now(c, 0.0));
        assert!(miss < 1.5, "marked: {miss:.2}/255 from the reference");
        let miss = worst_miss(film_look(false, "default", vec![]), |c| c);
        assert!(miss < 1.5, "unmarked: {miss:.2}/255 from the reference");
    }

    /// A RAW in DNG Standard or Linear reaches a marked film look at the
    /// offset its rendering carries, and without the rendering's tone curve;
    /// one put on before, at the RAW's own exposure.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn gpu_a_marked_film_look_takes_a_raw_at_its_offset() {
        for ev in [-0.4f32, 0.5] {
            let renderings = [
                (dng_standard::MODE, dng_standard::curve_points(ev)),
                (dng_standard::LINEAR_MODE, dng_standard::linear_points(ev)),
            ];
            for (rendering, curve) in renderings {
                let miss = worst_miss(film_look(true, rendering, curve.clone()), |c| {
                    fed_now(c, ev)
                });
                assert!(miss < 1.5, "{rendering} {ev:+}: {miss:.2}/255");
                let miss = worst_miss(film_look(false, rendering, curve), |c| c);
                assert!(miss < 1.5, "{rendering} {ev:+} unmarked: {miss:.2}/255");
            }
        }
    }

    #[test]
    fn only_a_marked_film_look_takes_the_offset() {
        let curve: Vec<Value> = dng_standard::linear_points(0.35)
            .into_iter()
            .map(|p| serde_json::to_value(p).unwrap())
            .collect();
        let linear = raw_tone::mode_index(dng_standard::LINEAR_MODE);
        let marked = json!({ KEY: true });
        assert!((exposure_ev(&marked, linear, &curve) - 0.35).abs() < 0.005);
        assert_eq!(exposure_ev(&json!({}), linear, &curve), 0.0);
        // RapidRAW's rendering, and an ordinary image, carry no offset.
        assert_eq!(exposure_ev(&marked, 0, &curve), 0.0);
        // An enlargement carries its RAW's.
        let enlarged = json!({ KEY: true, CARRIED_EV_KEY: 0.6 });
        assert!((exposure_ev(&enlarged, 0, &[]) - 0.6).abs() < 1e-6);
        assert_eq!(
            rendering_ev(linear, &curve),
            exposure_ev(&marked, linear, &curve)
        );
    }
}
