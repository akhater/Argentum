// ============================================================================
// Argentum's harvested shader maths.
//
// Ours alone — upstream RapidRAW has never seen this file, so it can never
// conflict on a merge. It is glued in front of their shader.wgsl at build time
// by gpu_processing.rs, so anything declared here is visible to their code.
//
// Their file gets one call line per tool, nothing more. See CLAUDE.md,
// "HARD RULE: mergeability".
//
// Naming: functions carry the prefix of where the maths came from —
//   dt_    darktable
//   rt_    RawTherapee
//   gimp_  GIMP / GEGL
//   paper_ a published paper with no reference implementation
// ============================================================================


// ---------------------------------------------------------------------------
// Colour space constants
// ---------------------------------------------------------------------------

// D65 — the white of linear sRGB, which is the space this pipeline works in.
// A correctly balanced image lands here.
const AG_D65_X: f32 = 0.3127269;
const AG_D65_Y: f32 = 0.3290232;

const AG_NORM_MIN: f32 = 1.52587890625e-05;

// Linear sRGB <-> CIE XYZ.
const AG_SRGB_TO_XYZ = mat3x3<f32>(
    vec3<f32>(0.4124564, 0.2126729, 0.0193339),
    vec3<f32>(0.3575761, 0.7151522, 0.1191920),
    vec3<f32>(0.1804375, 0.0721750, 0.9503041),
);

const AG_XYZ_TO_SRGB = mat3x3<f32>(
    vec3<f32>( 3.2404542, -0.9692660,  0.0556434),
    vec3<f32>(-1.5371385,  1.8760108, -0.2040259),
    vec3<f32>(-0.4985314,  0.0415560,  1.0572252),
);

// Bradford cone response — the transform underneath every modern chromatic
// adaptation. Working in this space is what makes an adaptation preserve hue
// instead of just scaling channels.
const AG_XYZ_TO_LMS = mat3x3<f32>(
    vec3<f32>( 0.8951000, -0.7502000,  0.0389000),
    vec3<f32>( 0.2664000,  1.7135000, -0.0685000),
    vec3<f32>(-0.1614000,  0.0367000,  1.0296000),
);

const AG_LMS_TO_XYZ = mat3x3<f32>(
    vec3<f32>( 0.9869929,  0.4323053, -0.0085287),
    vec3<f32>(-0.1470543,  0.5183603,  0.0400428),
    vec3<f32>( 0.1599627,  0.0492912,  0.9684867),
);


// ---------------------------------------------------------------------------
// dt_white_balance — real chromatic adaptation
//
// Replaces RapidRAW's apply_white_balance, which was three invented
// multipliers:
//
//   temp_mult = (1 + t*0.2, 1 + t*0.05, 1 - t*0.2)
//   tint_mult = (1 + n*0.25, 1 - n*0.25, 1 + n*0.25)
//
// No kelvin, no white point, no adaptation — a fudge that looks roughly right
// near neutral and drifts badly at the ends. Notably it scales channels
// independently, which shifts hue as a side effect: push temperature far and
// skin goes wrong in a way no amount of tint fixes.
//
// This does what darktable's colour calibration does in principle: turn the
// slider into an actual illuminant, then adapt the image from that illuminant
// to the working white in Bradford cone space.
//
// Source of the approach: darktable src/iop/channelmixerrgb.c, illuminant_to_xy
// and chroma_adapt_pixel. Simplified — darktable offers CAT16, Bradford linear
// and non-linear, and several illuminant models; this takes Bradford linear on
// the daylight locus, which is the sane default for photographic use.
// ---------------------------------------------------------------------------

// The encoding the pipeline works in.
//
// mods/preview_encode.rs encodes a RAW before it reaches this shader: gamma
// 2.38, then a 1.28 contrast slope above a knee, and a quadratic toe below it
// so shadows reach zero smoothly instead of being clipped. shader.wgsl then
// treats the result as linear, which is fine for maths that only scales
// channels and wrong for colour science - so dt_white_balance undoes it,
// adapts, and puts it back.
//
// KEEP IN STEP with mods/preview_encode.rs, which owns this curve. The toe
// exists because the original straight line, y = 1.28g - 0.14, reaches zero at
// g = 0.109 and clamped everything below scene-linear 0.00516 to black - 11% of
// a backlit frame, thrown away before anything could be done about it.
const AG_RAW_GAMMA: f32 = 2.38;
const AG_RAW_CONTRAST: f32 = 1.28;
const AG_KNEE: f32 = 0.25;
const AG_TOE_A: f32 = 2.24;   // 0.14 / knee^2
const AG_TOE_B: f32 = 0.16;   // contrast - 2*a*knee

fn ag_encode_channel(linear: f32) -> f32 {
    let g = pow(max(linear, 0.0), 1.0 / AG_RAW_GAMMA);
    var y: f32;
    if (g >= AG_KNEE) {
        y = (g - 0.5) * AG_RAW_CONTRAST + 0.5;
    } else {
        y = AG_TOE_A * g * g + AG_TOE_B * g;
    }
    return clamp(y, 0.0, 1.0);
}

fn ag_decode_channel(encoded: f32) -> f32 {
    let y = clamp(encoded, 0.0, 1.0);
    let knee_y = (AG_KNEE - 0.5) * AG_RAW_CONTRAST + 0.5;
    var g: f32;
    if (y >= knee_y) {
        g = (y - 0.5) / AG_RAW_CONTRAST + 0.5;
    } else {
        let disc = AG_TOE_B * AG_TOE_B + 4.0 * AG_TOE_A * y;
        g = (-AG_TOE_B + sqrt(max(disc, 0.0))) / (2.0 * AG_TOE_A);
    }
    return pow(max(g, 0.0), AG_RAW_GAMMA);
}

fn ag_to_scene_linear(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(ag_decode_channel(c.r), ag_decode_channel(c.g), ag_decode_channel(c.b));
}

fn ag_from_scene_linear(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(ag_encode_channel(c.r), ag_encode_channel(c.g), ag_encode_channel(c.b));
}

// Slider (-100..100, already divided by SCALES.temperature = 25, so roughly
// -4..4) to the assumed scene illuminant, in kelvin.
//
// Exponential rather than linear so each step feels the same size at 3000K and
// at 9000K — the perceptual spacing of colour temperature is closer to
// logarithmic. Neutral sits at D65, the working white.
//
// SIGN CONVENTION, and it is the opposite of what feels obvious:
// sliding right makes the picture *warmer*, matching every other editor. To
// warm a picture you tell the pipeline the scene was lit by something *bluer*
// than it really was — adapting from a blue illuminant to D65 adds warmth. So
// positive slider means higher assumed kelvin.
fn ag_slider_to_kelvin(temp: f32) -> f32 {
    return clamp(6500.0 * exp(temp * 0.28), 1800.0, 20000.0);
}

// Kelvin to CIE xy on the daylight locus, with a Planckian fallback below
// 4000K where daylight is not defined.
//
// CIE daylight approximation — the standard cubic fits.
fn ag_kelvin_to_xy(kelvin: f32) -> vec2<f32> {
    let t = clamp(kelvin, 1800.0, 20000.0);
    let inv = 1000.0 / t;
    var x: f32;

    if (t < 4000.0) {
        // Planckian locus, low end. Incandescent, candlelight.
        x = -0.2661239 * inv * inv * inv
            - 0.2343589 * inv * inv
            + 0.8776956 * inv
            + 0.179910;
    } else if (t <= 7000.0) {
        x = 0.244063
            + 0.09911 * inv
            + 2.9678 * inv * inv
            - 4.6070 * inv * inv * inv;
    } else {
        x = 0.237040
            + 0.24748 * inv
            + 1.9018 * inv * inv
            - 2.0064 * inv * inv * inv;
    }

    let y = -3.000 * x * x + 2.870 * x - 0.275;
    return vec2<f32>(x, y);
}

// Tint shifts the illuminant perpendicular to the temperature axis — the
// green/magenta correction. Positive pushes magenta, matching the direction of
// the old implementation so existing edits move in a familiar direction.
fn ag_apply_tint(xy: vec2<f32>, tint: f32) -> vec2<f32> {
    // The daylight locus runs roughly along +x/+y; perpendicular to it is the
    // green-magenta axis. Scale is small because y has a narrow useful range.
    // SIGN: same trap as temperature. Positive tint must make the *picture*
    // magenta, matching their old tint_mult = (1+t*.25, 1-t*.25, 1+t*.25) and
    // every other editor. To do that we assume a *greener* illuminant and adapt
    // it away - so positive tint raises y rather than lowering it. Backwards
    // here inverts the slider, and auto-WB with it.
    return vec2<f32>(xy.x, xy.y + tint * 0.05);
}

fn ag_xy_to_xyz(xy: vec2<f32>) -> vec3<f32> {
    let y = max(xy.y, AG_NORM_MIN);
    return vec3<f32>(xy.x / y, 1.0, (1.0 - xy.x - xy.y) / y);
}

// Bradford chromatic adaptation: XYZ under `from` -> XYZ under `to`.
//
// Both white points go to cone space, the image goes to cone space, each cone
// channel is scaled by the ratio of the two whites, and everything comes back.
// That per-cone scaling is the part that keeps hues intact — it is a model of
// how the eye adapts, not an arbitrary channel gain.
fn ag_chromatic_adapt(xyz: vec3<f32>, from_xy: vec2<f32>, to_xy: vec2<f32>) -> vec3<f32> {
    let lms_from = AG_XYZ_TO_LMS * ag_xy_to_xyz(from_xy);
    let lms_to = AG_XYZ_TO_LMS * ag_xy_to_xyz(to_xy);
    let lms = AG_XYZ_TO_LMS * xyz;

    let ratio = vec3<f32>(
        lms_to.x / max(lms_from.x, AG_NORM_MIN),
        lms_to.y / max(lms_from.y, AG_NORM_MIN),
        lms_to.z / max(lms_from.z, AG_NORM_MIN),
    );

    return AG_LMS_TO_XYZ * (lms * ratio);
}

// The replacement for apply_white_balance. Same signature, real maths.
fn dt_white_balance(color: vec3<f32>, temp: f32, tnt: f32) -> vec3<f32> {
    // Nothing to do at neutral — and worth short-circuiting, since this runs
    // per pixel and most images sit at or near zero on one of the two.
    if (abs(temp) < 1e-6 && abs(tnt) < 1e-6) {
        return color;
    }

    let kelvin = ag_slider_to_kelvin(temp);
    let illuminant = ag_apply_tint(ag_kelvin_to_xy(kelvin), tnt);

    // Adapt in scene-linear, not in whatever space the pipeline hands us.
    //
    // For a RAW file this shader is fed the output of
    // apply_cpu_default_raw_processing — gamma 2.38 then a 1.28 contrast boost —
    // and treats it as linear anyway (shader.wgsl: `if (is_raw == 0u)` only
    // linearises for non-RAW). That costs their three-multiplier white balance
    // nothing, because scaling channels is scale-invariant to a curve. It costs
    // ours plenty: an sRGB→XYZ matrix and a cone-space adaptation are only
    // meaningful on linear data.
    //
    // Left uncorrected it showed up as a fixed ~3% green residual after picking
    // a neutral patch — the illuminant is solved in true scene-linear (see
    // `to_scene_linear` in mods/auto_wb.rs) while the correction was applied in
    // the encoded space. Solve in one space, correct in another, and the two
    // never quite meet.
    // Only RAW arrives encoded. A JPEG has already been through
    // srgb_to_linear at the top of main(), so it is genuinely linear and
    // decoding it again would correct twice.
    let is_raw_input = adjustments.global.is_raw_image != 0u;
    var lin = color;
    if (is_raw_input) {
        lin = ag_to_scene_linear(color);
    }

    let xyz = AG_SRGB_TO_XYZ * lin;
    let adapted = ag_chromatic_adapt(xyz, illuminant, vec2<f32>(AG_D65_X, AG_D65_Y));

    // Negatives are possible when adapting far from the working white — a
    // colour that exists under one illuminant may fall outside sRGB under
    // another. Clip rather than let them poison later stages.
    let out_lin = max(AG_XYZ_TO_SRGB * adapted, vec3<f32>(0.0));

    // Back into the encoding the rest of the pipeline expects, so every tool
    // after this one still sees what it was tuned for.
    if (is_raw_input) {
        return ag_from_scene_linear(out_lin);
    }
    return out_lin;
}


// ============================================================================
// THE PIPELINE ANCHOR
//
// One call into shader.wgsl, forever.
//
// Every harvested tool used to cost a line of their shader: one call, one more
// line, one more place an upstream release can collide with us. Ten tools is
// ten lines and looks harmless. A hundred is a hundred lines in the file that
// *is* RapidRAW's image engine, and the merge stops being worth doing.
//
// So their shader calls this and nothing else. Adding a tool means adding a
// line *here*, in our file, which costs their side nothing.
//
// STAGES
//
// Two of them, because tools genuinely belong at different points and pretending
// otherwise would be worse than the line it saves:
//
//   ag_stage_scene_linear  before the tone curve, on linear data. Colour work
//                          lives here: white balance, calibration, anything
//                          involving a matrix.
//   ag_stage_display       after tone mapping, on display-referred data.
//
// Only the first is wired today. The second exists so that adding the first
// display-referred tool does not have to touch their file to do it, which is the
// whole point.
//
// ORDER IS THE CONTRACT
//
// Tools run top to bottom as written. A tool that needs another's output goes
// below it. Keep that explicit rather than relying on where a call happens to
// have been inserted.
// ============================================================================

/// Scene-linear stage: everything Argentum does before the tone curve.
fn ag_stage_scene_linear(color: vec3<f32>, temp: f32, tnt: f32) -> vec3<f32> {
    var c = color;
    c = dt_white_balance(c, temp, tnt);
    return c;
}

/// Display-referred stage: everything Argentum does after tone mapping.
///
/// Empty on purpose. It is the anchor for the first tool that needs to run
/// here, so that tool costs nothing upstream.
fn ag_stage_display(color: vec3<f32>) -> vec3<f32> {
    return color;
}
