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
// White balance is RapidRAW's, from 1.6.5.
//
// dt_white_balance lived here from 26.37.2: the slider turned into an
// illuminant on the daylight locus and the image was adapted from it to D65 in
// Bradford cone space. RapidRAW 1.6.5 shipped the same idea, done further -
// Bradford gains in log-LMS, a Kelvin mode read from the camera's as-shot data,
// white balance per mask, and a picker that averages an area of the original -
// so theirs runs and this was retired rather than kept as a second engine.
//
// Ours also decoded the RAW a second time before adapting, on the belief that
// the shader was fed the preview encode. It is not: the render input is the
// decoder's scene-linear output, which is what theirs assumes. See
// docs/UPSTREAM_CATCHUP.md, 2026-10-07.
//
// Auto white balance survives, in mods/auto_wb.rs: it detects the illuminant
// and hands it to their white_balance::pick_white_balance, so the wand and
// their picker answer in the same units. Old edits made with the retired
// sliders are converted on load by mods/wb_legacy.rs.
// ---------------------------------------------------------------------------


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
// Five of them, because tools genuinely belong at different points and pretending
// otherwise would be worse than the line it saves:
//
//   ag_stage_scene_linear  before the tone curve, on linear data. Colour work
//                          lives here: white balance, calibration, anything
//                          involving a matrix.
//   ag_stage_color         after their colour controls, before their Color
//                          Grading: B&W.
//   ag_stage_tone          right after their tone mapper, which it can replace
//                          for a RAW: the RAW tone renderings (see below).
//   ag_stage_grade         after the Curves panel and a LUT, before grain:
//                          Lightroom's colour grading (see below).
//   ag_stage_display       after tone mapping, on display-referred data.
//
// A tool joins the stage it belongs to here, so adding it never touches their
// file, which is the whole point.
//
// ORDER IS THE CONTRACT
//
// Tools run top to bottom as written. A tool that needs another's output goes
// below it. Keep that explicit rather than relying on where a call happens to
// have been inserted.
// ============================================================================

/// Sharpening's settings, as `mods/sharpen.rs` lays them out.
///
/// This shader never reads them: sharpening is whole-image passes, run by
/// `mods/input_stage.rs` before this shader starts, and what arrives here as
/// `input_texture` is already sharpened. The struct exists so their
/// `GlobalAdjustments` has the same layout on both sides of the buffer -
/// 560 bytes, field for field with `sharpen::Params`.
struct AgSharpen {
    capture_amount: f32,
    capture_radius: f32,
    capture_corner: f32,
    capture_iterations: u32,
    usm_amount: f32,
    usm_radius: f32,
    usm_threshold: f32,
    contrast: f32,
    flags: u32,
    usm_contrast: f32,
    local_auto: u32,
    local_show: u32,
    local: array<f32, 32>,
    local_radius: array<f32, 32>,
    local_threshold: array<f32, 32>,
    local_contrast: array<f32, 32>,
}

/// B&W's settings, as `mods/black_white.rs` lays them out (`Params`, 48
/// bytes). The shader reads `enabled`; the grid made from `mix` is in
/// `ag_gray_grid`.
struct AgBlackWhite {
    enabled: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
    mix: array<f32, 8>,
}

/// The camera profile, as a correction on already-decoded pixels.
///
/// The decode has already turned camera RGB into sRGB using rawler's own matrix
/// for this body. This is the difference between that and the profile's — see
/// mods/profile_correction.rs for the derivation — so applying it here gives
/// the same result as decoding with the profile, without decoding again.
///
/// Identity when no profile is chosen, which is not a special case: the matrix
/// really is the identity and this really does multiply by it, so a photo
/// without a profile is bit-for-bit what it was.
fn ag_camera_profile(color: vec3<f32>, r0: vec4<f32>, r1: vec4<f32>, r2: vec4<f32>) -> vec3<f32> {
    return vec3<f32>(
        dot(r0.xyz, color),
        dot(r1.xyz, color),
        dot(r2.xyz, color),
    );
}

/// Scene-linear stage: everything Argentum does before the tone curve.
///
/// Order matters. The profile comes first because it decides what the colours
/// *are*; white balance then decides what the light was. Doing it the other way
/// would balance one rendering and then swap the rendering underneath it.
fn ag_stage_scene_linear(
    color: vec3<f32>,
    profile_r0: vec4<f32>,
    profile_r1: vec4<f32>,
    profile_r2: vec4<f32>,
) -> vec3<f32> {
    var c = color;
    c = ag_camera_profile(c, profile_r0, profile_r1, profile_r2);
    return c;
}


// ============================================================================
// THE COLOUR ANCHOR
//
// After their colour controls (Color Mixer, Hue, Saturation, Vibrance) and
// before their Color Grading: where Lightroom turns a photo black and white,
// so that grading tints the grey. The fourth call into shader.wgsl, taken
// 2026-10-20 for B&W, once their shader had joined the render path (DEC-36).
// ============================================================================

/// B&W's grid (mods/black_white.rs `gray_grid`): for each cell of the mix's
/// 36 hues x 6 saturations x 6 values, log2 of the grey's luminance over the
/// colour's. Written by gpu_processing.rs when a render asks for B&W.
@group(0) @binding(14) var<storage, read> ag_gray_grid: array<f32>;

// from RAWmakase crates/rawmakase-engine/src/develop/gpu/develop.wgsl @ a8c5d6d7
// (v0.4.0, MIT), `rem_euclid`.
fn rm_rem_euclid(a: f32, b: f32) -> f32 {
    return a - b * floor(a / b);
}

// from RAWmakase crates/rawmakase-engine/src/develop/gpu/develop.wgsl @ a8c5d6d7
// (v0.4.0, MIT): `mixer`, with only the value change its black & white grid
// holds, and `adjust`'s black & white step. The colour (linear sRGB) is placed
// in the grid by its hue, saturation and value in linear ProPhoto, and its
// luminance is scaled by the cell it falls in. Returns linear luminance.
// mods/black_white.rs `gray` is its CPU twin.
fn rm_gray(rgb_in: vec3<f32>) -> f32 {
    let rgb = max(rgb_in, vec3<f32>(0.0));
    let luminance = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let q = max(AG_SRGB_TO_PROPHOTO * rgb, vec3<f32>(0.0));
    let max_v = max(max(q.x, q.y), q.z);
    if (max_v <= 1.0e-6) {
        return luminance;
    }
    let d = max_v - min(min(q.x, q.y), q.z);
    var hue = 0.0;
    if (d >= 1.0e-9) {
        if (max_v == q.x) {
            hue = rm_rem_euclid((q.y - q.z) / d, 6.0) / 6.0;
        } else if (max_v == q.y) {
            hue = ((q.z - q.x) / d + 2.0) / 6.0;
        } else {
            hue = ((q.x - q.y) / d + 4.0) / 6.0;
        }
    }
    let fh = rm_rem_euclid(hue, 1.0) * 36.0 - 0.5;
    let fs = clamp(sqrt(clamp(d / max_v, 0.0, 1.0)) * 6.0 - 0.5, 0.0, 5.0);
    let fv = clamp(pow(clamp(max_v, 0.0, 1.0), 0.45) * 6.0 - 0.5, 0.0, 5.0);
    let h0 = floor(fh);
    let th = fh - h0;
    let ts = fract(fs);
    let tv = fract(fv);
    let s0 = u32(fs);
    let v0 = u32(fv);
    let s1 = min(s0 + 1u, 5u);
    let v1 = min(v0 + 1u, 5u);
    var dv = 0.0;
    for (var dh = 0; dh < 2; dh++) {
        let hi = u32(rm_rem_euclid(h0 + f32(dh), 36.0));
        let wh = select(1.0 - th, th, dh == 1);
        for (var k = 0u; k < 2u; k++) {
            let si = select(s0, s1, k == 1u);
            let ws = select(1.0 - ts, ts, k == 1u);
            for (var m = 0u; m < 2u; m++) {
                let vi = select(v0, v1, m == 1u);
                let wv = select(1.0 - tv, tv, m == 1u);
                dv += ag_gray_grid[(hi * 6u + si) * 6u + vi] * (wh * ws * wv);
            }
        }
    }
    return max(luminance * exp2(dv), 0.0);
}

/// Colour stage: everything Argentum does between their colour controls and
/// their Color Grading. B&W (mods/black_white.rs) turns the photo grey; the
/// renderer has already left the colour controls out while it is on.
fn ag_stage_color(color: vec3<f32>) -> vec3<f32> {
    if (adjustments.global.ag_black_white.enabled == 0u) {
        return color;
    }
    return vec3<f32>(rm_gray(color));
}


// ============================================================================
// THE TONE ANCHOR
//
// Their tone mapper runs first, untouched, then this. For a RAW with a RAW tone
// rendering chosen (mods/raw_tone.rs) ours replaces its result; for anything
// else theirs passes straight through.
//
// The third call into shader.wgsl, taken 2026-10-17 by a recorded decision
// (DEC-27).
// Until then the RAW tone rendering lived as two blocks inside their main(),
// a condition on their tone-mapper chain and a per-channel curve after it, and
// DNG Standard needs more than a per-channel curve can give. The blocks came
// out; their chain is theirs again, line for line.
//
//   mode 1 Base Curve, 2 Auto-Matched   sRGB, then the curve on each channel
//   mode 3 DNG Standard                 the curve applied as Adobe applies it
//
// Their struct carries the mode and the photo's 16 points; this reads them
// from `adjustments` directly, so the call line in their file never has to
// change when a mode is added.
// ============================================================================

fn ag_stage_tone(theirs: vec3<f32>, linear: vec3<f32>) -> vec3<f32> {
    let mode = adjustments.global.raw_tone_mode;
    let count = adjustments.global.raw_tone_curve_count;
    if (adjustments.global.is_raw_image != 1u || mode == 0u || count < 2u) {
        return theirs;
    }
    if (mode == 3u) {
        return ag_dng_standard(linear, count);
    }
    let encoded = linear_to_srgb(linear);
    return vec3<f32>(
        apply_curve(encoded.r, CURVE_SRC_RAW_TONE, 0u, count),
        apply_curve(encoded.g, CURVE_SRC_RAW_TONE, 0u, count),
        apply_curve(encoded.b, CURVE_SRC_RAW_TONE, 0u, count),
    );
}

// Linear sRGB (D65) and linear ProPhoto RGB (ROMM, D50), Bradford-adapted:
// sRGB -> XYZ -> D65 to D50 -> ProPhoto, with Lindbloom's matrices. Each row
// sums to one, so white stays white. Built column by column, as WGSL does.
const AG_SRGB_TO_PROPHOTO = mat3x3<f32>(
    vec3<f32>(0.5293458, 0.0983744, 0.0168832),
    vec3<f32>(0.3300728, 0.8734610, 0.1176725),
    vec3<f32>(0.1405813, 0.0281647, 0.8654443),
);
const AG_PROPHOTO_TO_SRGB = mat3x3<f32>(
    vec3<f32>(2.0340764, -0.2288134, -0.0085698),
    vec3<f32>(-0.7273343, 1.2317302, -0.1532866),
    vec3<f32>(-0.3067417, -0.0029169, 1.1618564),
);

fn ag_srgb_encode(v: f32) -> f32 {
    let c = clamp(v, 0.0, 1.0);
    return select(1.055 * pow(c, 1.0 / 2.4) - 0.055, c * 12.92, c <= 0.0031308);
}

fn ag_srgb_decode(e: f32) -> f32 {
    return select(pow((e + 0.055) / 1.055, 2.4), e / 12.92, e <= 0.04045);
}

/// One channel through the photo's curve, linear light in and out. The points
/// are on sRGB-encoded values, as every RAW tone curve is stored.
fn ag_dng_tone(v: f32, count: u32) -> f32 {
    return ag_srgb_decode(apply_curve(ag_srgb_encode(v), CURVE_SRC_RAW_TONE, 0u, count));
}

// from RawTherapee rtengine/curves.h @ 46b92d8e, AdobeToneCurve::Apply and
// AdobeToneCurve::RGBTone - their port of the DNG SDK's RefBaselineRGBTone.
//
// The largest and smallest channel go through the curve; the middle one keeps
// its place between them. RawTherapee writes out the six orderings; one line
// does all of them, because c - lo, scaled by how much the span grew, sends the
// largest to its toned value, the smallest to its own, and the middle where
// RGBTone puts it. Equal channels are a grey and take the curve directly.
fn rt_rgb_tone(c: vec3<f32>, count: u32) -> vec3<f32> {
    let hi = max(c.r, max(c.g, c.b));
    let lo = min(c.r, min(c.g, c.b));
    let hi_toned = ag_dng_tone(hi, count);
    if (hi - lo <= 1.0e-6) {
        return vec3<f32>(hi_toned);
    }
    let lo_toned = ag_dng_tone(lo, count);
    return vec3<f32>(lo_toned) + (c - vec3<f32>(lo)) * ((hi_toned - lo_toned) / (hi - lo));
}

/// DNG Standard (mods/dng_standard.rs): the curve applied in linear ProPhoto,
/// as Adobe and RawTherapee apply a profile's tone curve, then back to sRGB.
/// Colours beyond sRGB are clipped channel by channel by linear_to_srgb.
fn ag_dng_standard(linear: vec3<f32>, count: u32) -> vec3<f32> {
    let prophoto = clamp(AG_SRGB_TO_PROPHOTO * linear, vec3<f32>(0.0), vec3<f32>(1.0));
    let toned = rt_rgb_tone(prophoto, count);
    return linear_to_srgb(AG_PROPHOTO_TO_SRGB * toned);
}

// ============================================================================
// THE GRADING ANCHOR
//
// Lightroom's colour grading (mods/color_grade.rs): after the tone curve, the
// Curves panel and a LUT, where Camera Raw grades, on display-referred linear
// sRGB. Blocks not marked as Lightroom's keep RapidRAW's grading, earlier in
// their main(); for a marked block Rust has taken the settings out of their
// uniforms, so theirs adds nothing.
// ============================================================================

/// Which blocks are Lightroom's, as `color_grade::Params` lays them out.
struct AgGrade {
    global: u32,  // 1 when the photo's own block is
    masks: u32,   // bit i: mask i's
    _pad0: u32,
    _pad1: u32,
}

/// The marked blocks' curves (mods/color_grade.rs `move_to_curves`): a slot
/// per block, the photo's then each mask's, of 64 gains for each channel of
/// linear ProPhoto RGB, sample by sample. Written when a render has a
/// Lightroom block.
@group(0) @binding(15) var<storage, read> ag_grade_curves: array<f32>;

// from RAWmakase crates/rawmakase-engine/src/develop/gpu/develop.wgsl @ 5a2414b4
// (v0.4.1, MIT), `grade`, with its curves in slot `slot`. Each channel of
// linear ProPhoto RGB is scaled by its gain at its own value, sampled at
// x^(1/2.2). The engine's rm_color_grade.rs `apply` is its CPU twin.
fn rm_grade(rgb: vec3<f32>, slot: u32) -> vec3<f32> {
    let bins = 64u;
    let base = slot * bins * 3u;
    let q = AG_SRGB_TO_PROPHOTO * rgb;
    var out: vec3<f32>;
    for (var c = 0u; c < 3u; c++) {
        let f = pow(clamp(q[c], 0.0, 1.0), 1.0 / 2.2) * f32(bins - 1u);
        let i = min(u32(f), bins - 2u);
        let t = f - f32(i);
        let g = ag_grade_curves[base + i * 3u + c] * (1.0 - t)
            + ag_grade_curves[base + (i + 1u) * 3u + c] * t;
        out[c] = q[c] * g;
    }
    return AG_PROPHOTO_TO_SRGB * out;
}

/// Grading stage: `srgb` is display-encoded, as the Curves panel leaves it.
/// The photo's block grades it, then each marked mask's block is mixed in by
/// the mask's influence, as their grading does.
fn ag_stage_grade(srgb: vec3<f32>, coord: vec2<u32>) -> vec3<f32> {
    let marked = adjustments.global.ag_grade;
    if (marked.global == 0u && marked.masks == 0u) {
        return srgb;
    }
    var linear = srgb_to_linear(srgb);
    if (marked.global != 0u) {
        linear = rm_grade(linear, 0u);
    }
    for (var i = 0u; i < adjustments.mask_count; i = i + 1u) {
        if (((marked.masks >> i) & 1u) == 0u) {
            continue;
        }
        let influence = get_mask_influence(i, coord);
        if (influence > 0.001) {
            linear = mix(linear, rm_grade(linear, i + 1u), influence);
        }
    }
    return linear_to_srgb_extended(linear);
}


/// The clipping view, in whichever mode the button has been cycled to.
///
/// Red is a blown highlight and blue is a crushed shadow, in every mode. What
/// changes is which channels are being asked: mode 1 looks at all three, and
/// modes 2, 3 and 4 look at red, green and blue on their own.
///
/// That matters because a highlight where only green has gone still holds two
/// channels of real data, and one where all three have gone holds nothing.
/// Stepping through the channels is how you tell those apart.
fn ag_clipping_view(color: vec3<f32>, mode: u32) -> vec3<f32> {
    if (mode == 0u) {
        return color;
    }

    let high = 0.998;
    let low = 0.002;

    // Setting a white or black point. Everything that is not at the limit goes
    // away, so the only thing on screen is what you are about to lose — which
    // is the whole reason to look.
    //
    // The colour is which channels went, because on the way to a white point
    // the channels do not arrive together: red first on warm light, blue first
    // on cold. Seeing one channel go is the signal to stop.
    if (mode == 5u) {
        if (color.r > high || color.g > high || color.b > high) {
            var out = vec3<f32>(0.0, 0.0, 0.0);
            if (color.r > high) { out.r = 1.0; }
            if (color.g > high) { out.g = 1.0; }
            if (color.b > high) { out.b = 1.0; }
            return out;
        }
        return vec3<f32>(0.0, 0.0, 0.0);
    }
    if (mode == 6u) {
        // On white, a crushed channel is *taken away* rather than added.
        //
        // The first version added them, the same way the white point view does
        // on black — so a pixel with all three channels crushed came out white,
        // on a white background, and setting a black point showed nothing at
        // all. Removing them means all three gone is black, which is the thing
        // you are looking for, and one gone is its complement: lose red and the
        // pixel goes cyan.
        if (color.r < low || color.g < low || color.b < low) {
            var out = vec3<f32>(1.0, 1.0, 1.0);
            if (color.r < low) { out.r = 0.0; }
            if (color.g < low) { out.g = 0.0; }
            if (color.b < low) { out.b = 0.0; }
            return out;
        }
        return vec3<f32>(1.0, 1.0, 1.0);
    }

    var v = 0.0;
    var check_all = false;
    if (mode == 1u) {
        check_all = true;
    } else if (mode == 2u) {
        v = color.r;
    } else if (mode == 3u) {
        v = color.g;
    } else {
        v = color.b;
    }

    var over = v > high;
    var under = v < low;
    if (check_all) {
        over = color.r > high || color.g > high || color.b > high;
        under = color.r < low || color.g < low || color.b < low;
    }

    if (over) {
        return vec3<f32>(1.0, 0.0, 0.0);
    }
    if (under) {
        return vec3<f32>(0.0, 0.0, 1.0);
    }
    return color;
}


/// Display-referred stage: everything Argentum does after tone mapping.
///
/// It was empty, held open for the first tool that needed to run here so that
/// tool would cost nothing upstream. The clipping view is that tool, and the
/// one line in their file is the whole of what it cost. The next one costs
/// nothing.
///
/// `mode` is their `show_clipping` uniform, which was already a `u32` holding
/// nothing but 0 and 1 — see mods/clipping.rs.
///
/// Modes 7, 8 and 9 are the sharpening masks: capture's, the manual
/// sharpen's and a photo mask's own. The input stage has already written the
/// mask into this render's input in place of the photo, so it is read back
/// here, at this pixel, exactly as written: every adjustment in between has
/// been run on it and is thrown away, because a mask that went through the
/// tone curve would no longer say how strongly each pixel is sharpened.
fn ag_stage_display(color: vec3<f32>, mode: u32, coord: vec2<u32>) -> vec3<f32> {
    if (mode == 7u || mode == 8u || mode == 9u) {
        return textureLoad(input_texture, coord, 0).rgb;
    }
    return ag_clipping_view(color, mode);
}
