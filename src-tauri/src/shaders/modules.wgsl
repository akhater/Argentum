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
// decoder's scene-linear output, which is what theirs assumes. See the
// RapidRAW 1.6.5 review of 2026-10-07 (docs/UPSTREAM_CATCHUP.md, in git
// history).
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
//                          a provider look's grain, then Lightroom's colour
//                          grading (see below).
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
//   mode 4 Linear                       the same, its curve the exposure offset alone
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
    if (mode == 3u || mode == 4u) {
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

/// The picture in linear ProPhoto, each channel capped at the sensor's white:
/// where DNG Standard's curve starts, and what makes a blown highlight white
/// whatever colour the decode left in it. A film look starts here too.
fn ag_prophoto_to_white(linear: vec3<f32>) -> vec3<f32> {
    return clamp(AG_SRGB_TO_PROPHOTO * linear, vec3<f32>(0.0), vec3<f32>(1.0));
}

/// DNG Standard (mods/dng_standard.rs): the curve applied in linear ProPhoto,
/// as Adobe and RawTherapee apply a profile's tone curve, then back to sRGB.
/// Colours beyond sRGB are clipped channel by channel by linear_to_srgb.
fn ag_dng_standard(linear: vec3<f32>, count: u32) -> vec3<f32> {
    let toned = rt_rgb_tone(ag_prophoto_to_white(linear), count);
    return linear_to_srgb(AG_PROPHOTO_TO_SRGB * toned);
}

// ============================================================================
// THE FILM LOOK INPUT
//
// A film look that comes with the app is a spektrafilm LUT, made for V-Log
// over V-Gamut primaries (mods/film_lut.rs). Their scene-referred LUT path
// encodes the scene-linear picture with the V-Log curve; this hands it the
// picture in V-Gamut first, and a RAW at the camera's exposure offset, as its
// rendering would have shown it. An edit that put a film look on before this
// carries no mark, and is handed the working RGB, as it was.
// ============================================================================

// Linear sRGB (D65) to linear V-Gamut (D65), through XYZ, from V-Gamut's
// primaries in Panasonic's V-Log/V-Gamut Reference Manual (2014). Each row
// sums to one, so white stays white. Built column by column, as WGSL does;
// mods/film_lut.rs checks it against the primaries.
const AG_SRGB_TO_V_GAMUT = mat3x3<f32>(
    vec3<f32>(0.5851961, 0.0785886, 0.0227942),
    vec3<f32>(0.3226416, 0.8196271, 0.1142170),
    vec3<f32>(0.0921622, 0.1017843, 0.8629887),
);

/// Blown highlights reach the film as DNG Standard shows them: capped at the
/// sensor's white (`ag_prophoto_to_white`), so a clipped area the decode
/// could not settle to neutral (highlights::settle_blown) prints white, not
/// in the magenta white balance left in it. Then the camera's offset, then
/// V-Gamut.
fn ag_film_lut_input(linear: vec3<f32>) -> vec3<f32> {
    if (adjustments.global.ag_film_lut_input == 0u) {
        return linear;
    }
    let at_white = AG_PROPHOTO_TO_SRGB * ag_prophoto_to_white(linear);
    return AG_SRGB_TO_V_GAMUT * (at_white * exp2(adjustments.global.ag_film_lut_ev));
}

/// Where a look takes the picture in, for the white and black point views
/// (`ag_point_view`), sRGB-encoded so the views' limits mean the same here as
/// on screen. A channel at 1 is detail no setting of the look brings back; one
/// at 0 is black before the look lifts it to its own darkest.
///
/// - A LUT after the Curves - every LUT imported into Looks - is handed the
///   display picture, `display`, and loses what lies outside 0 to 1.
/// - A film look that comes with the app, fed its declared input, is handed
///   `scene_linear` capped at white in ProPhoto (`ag_prophoto_to_white`). The
///   camera's offset is left out: it is applied after the cap, into V-Log,
///   which keeps what it lifts.
///
/// Mid grey, which no limit catches, with no look, with one at 0%, or for a
/// film look put on before the mark, which is handed the working RGB uncapped.
fn ag_look_entry(scene_linear: vec3<f32>, display: vec3<f32>) -> vec3<f32> {
    if (adjustments.global.has_lut != 1u || adjustments.global.lut_intensity <= 0.0) {
        return vec3<f32>(0.5);
    }
    if (adjustments.global.lut_is_scene_referred == 0u) {
        return display;
    }
    if (adjustments.global.ag_film_lut_input == 0u) {
        return vec3<f32>(0.5);
    }
    let prophoto = AG_SRGB_TO_PROPHOTO * scene_linear;
    return vec3<f32>(ag_srgb_encode(prophoto.r), ag_srgb_encode(prophoto.g), ag_srgb_encode(prophoto.b));
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

/// Gains per channel in each slot of `ag_grade_curves`.
const RM_GRADE_BINS = 64u;

// from RAWmakase crates/rawmakase-engine/src/develop/gpu/develop.wgsl @ 5a2414b4
// (v0.4.1, MIT), `grade`, with its curves in slot `slot`. Each channel of
// linear ProPhoto RGB is scaled by its gain at its own value, sampled at
// x^(1/2.2). The engine's rm_color_grade.rs `apply` is its CPU twin.
//
// Changed from theirs: their loop over the channels wrote `out[c]`, and FXC,
// the HLSL compiler DirectX 12 uses, cannot write a vector through an index
// known only when it runs. It unrolled that loop and then could not unroll
// the per-mask loop in ag_stage_grade, so the shader failed to build on
// DirectX 12. Here each channel is a call and the vector is built from the
// three; the arithmetic and its order are theirs.
fn rm_grade_channel(v: f32, c: u32, base: u32) -> f32 {
    let f = pow(clamp(v, 0.0, 1.0), 1.0 / 2.2) * f32(RM_GRADE_BINS - 1u);
    let i = min(u32(f), RM_GRADE_BINS - 2u);
    let t = f - f32(i);
    let g = ag_grade_curves[base + i * 3u + c] * (1.0 - t)
        + ag_grade_curves[base + (i + 1u) * 3u + c] * t;
    return v * g;
}

fn rm_grade(rgb: vec3<f32>, slot: u32) -> vec3<f32> {
    let base = slot * RM_GRADE_BINS * 3u;
    let q = AG_SRGB_TO_PROPHOTO * rgb;
    return AG_PROPHOTO_TO_SRGB * vec3<f32>(
        rm_grade_channel(q.x, 0u, base),
        rm_grade_channel(q.y, 1u, base),
        rm_grade_channel(q.z, 2u, base),
    );
}

// ============================================================================
// A PROVIDER LOOK'S GRAIN - ours (mods/look_grain.rs)
//
// Three layers of normal noise on a lattice tied to the frame, smoothed with
// [1, 7, 1] / sqrt(51), enlarged with bicubic and scaled to measure 1 at the
// lattice's size; the map, indexed by the colour right after the LUT, says how
// much of each layer each channel gets. The smoothed lattice is made once for a
// photo's seed and size (shaders/look_grain.wgsl), so every render of a photo
// has the same grain and a render reads 16 of its nodes a pixel.
// ============================================================================

/// As `look_grain::Params` lays it out, 48 bytes.
struct AgLookGrain {
    on: u32,
    seed: u32,
    amount: f32,
    norm: f32,
    frame_x: f32,
    frame_y: f32,
    cells_x: f32,
    cells_y: f32,
    map: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

/// Both cubes of the map, cube 2 (e, f, g) above cube 1 (a, b, d) in depth,
/// in output units per unit of noise.
@group(0) @binding(16) var ag_grain_map: texture_3d<f32>;
@group(0) @binding(17) var ag_grain_sampler: sampler;
/// The smoothed lattice, layers A, B and C; texel (0, 0) is node (-2, -2).
@group(0) @binding(18) var ag_grain_lattice: texture_2d<f32>;

/// `look_grain::bicubic`: OpenCV's (A = -0.75), the weights of the nodes at
/// -1, 0, 1 and 2 for `t` past a node.
fn ag_grain_bicubic(t: f32) -> array<f32, 4> {
    let a = -0.75;
    let n0 = t + 1.0;
    let n1 = t;
    let n2 = 1.0 - t;
    let c0 = ((a * n0 - 5.0 * a) * n0 + 8.0 * a) * n0 - 4.0 * a;
    let c1 = ((a + 2.0) * n1 - (a + 3.0)) * n1 * n1 + 1.0;
    let c2 = ((a + 2.0) * n2 - (a + 3.0)) * n2 * n2 + 1.0;
    return array<f32, 4>(c0, c1, c2, 1.0 - c0 - c1 - c2);
}

/// Layers A, B and C at pixel `coord` of the frame, each measuring 1.
fn ag_grain_noise(coord: vec2<u32>, g: AgLookGrain) -> vec3<f32> {
    let cells = vec2<f32>(g.cells_x, g.cells_y);
    let at = (vec2<f32>(coord) + 0.5) * cells / vec2<f32>(g.frame_x, g.frame_y) - 0.5;
    let node = floor(at);
    var wx = ag_grain_bicubic(at.x - node.x);
    var wy = ag_grain_bicubic(at.y - node.y);
    // Node floor(at) - 1 is texel floor(at) + 1.
    let first = vec2<i32>(node) + 1;
    var sum = vec3<f32>(0.0);
    for (var j = 0; j < 4; j = j + 1) {
        var row = vec3<f32>(0.0);
        for (var i = 0; i < 4; i = i + 1) {
            row += wx[i] * textureLoad(ag_grain_lattice, first + vec2<i32>(i, j), 0).rgb;
        }
        sum += wy[j] * row;
    }
    return sum * g.norm;
}

/// The grain over `srgb`, the colour right after the LUT, then a clip:
/// R += k a A, G += k (b A + d B), B += k (e A + f B + g C).
fn ag_look_grain(srgb: vec3<f32>, coord: vec2<u32>) -> vec3<f32> {
    let g = adjustments.global.ag_look_grain;
    if (g.on == 0u) {
        return srgb;
    }
    let n = ag_grain_noise(coord, g);
    // Texel centres of each 64-step cube, so cube 1 and cube 2 never blend.
    let at = (clamp(srgb, vec3<f32>(0.0), vec3<f32>(1.0)) * 63.0 + 0.5) / vec3<f32>(64.0, 64.0, 128.0);
    let abd = textureSampleLevel(ag_grain_map, ag_grain_sampler, at, 0.0).rgb;
    let efg = textureSampleLevel(ag_grain_map, ag_grain_sampler, at + vec3<f32>(0.0, 0.0, 0.5), 0.0).rgb;
    let grain = vec3<f32>(abd.x * n.x, abd.y * n.x + abd.z * n.y, efg.x * n.x + efg.y * n.y + efg.z * n.z);
    return clamp(srgb + g.amount * grain, vec3<f32>(0.0), vec3<f32>(1.0));
}

/// Grading stage: `color` is display-encoded, as the Curves panel and a LUT
/// leave it. A provider look's grain goes on first, right after its LUT. Then
/// the photo's block grades it, and each marked mask's block is mixed in by
/// the mask's influence, as their grading does.
fn ag_stage_grade(color: vec3<f32>, coord: vec2<u32>) -> vec3<f32> {
    let srgb = ag_look_grain(color, coord);
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


/// Where every clipping view draws the line, on sRGB-encoded values: above
/// `AG_CLIP_HIGH` a channel is blown, below `AG_CLIP_LOW` it is crushed.
const AG_CLIP_HIGH: f32 = 0.998;
const AG_CLIP_LOW: f32 = 0.002;

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

    let high = AG_CLIP_HIGH;
    let low = AG_CLIP_LOW;

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

/// Setting a white point (mode 5) or a black point (mode 6), held with Ctrl
/// while dragging Whites or Blacks (useThresholdPreview.ts). Everything that
/// is not at the limit goes away, so the only thing on screen is what you are
/// about to lose — which is the whole reason to look.
///
/// A channel is at the limit on screen (`color`), or where a look takes the
/// picture in (`entry`, from `ag_look_entry`; mid grey without one). A faded
/// look never prints black or white — one prints black as 17% grey — so
/// judged on screen alone these views showed nothing at any setting.
/// Lightroom's shows nothing there too; this shows what the photo loses
/// before the look does. The screen is still asked as well, so a curve after
/// the look that crushes black still shows.
///
/// The colour is which channels went, because on the way to a white point
/// the channels do not arrive together: red first on warm light, blue first
/// on cold. Seeing one channel go is the signal to stop. Where it is a
/// built-in film look's input that went, the channels are ProPhoto's: near
/// enough to say which went first.
fn ag_point_view(color: vec3<f32>, entry: vec3<f32>, mode: u32) -> vec3<f32> {
    if (mode == 5u) {
        let over = (color > vec3<f32>(AG_CLIP_HIGH)) | (entry > vec3<f32>(AG_CLIP_HIGH));
        return select(vec3<f32>(0.0), vec3<f32>(1.0), over);
    }
    // On white, a crushed channel is *taken away* rather than added.
    //
    // The first version added them, the same way the white point view does
    // on black — so a pixel with all three channels crushed came out white,
    // on a white background, and setting a black point showed nothing at
    // all. Removing them means all three gone is black, which is the thing
    // you are looking for, and one gone is its complement: lose red and the
    // pixel goes cyan.
    let under = (color < vec3<f32>(AG_CLIP_LOW)) | (entry < vec3<f32>(AG_CLIP_LOW));
    return select(vec3<f32>(1.0), vec3<f32>(0.0), under);
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
///
/// `scene_linear` and `lut_input` are what a film look that comes with the
/// app and a LUT after the Curves are handed, for the white and black point
/// views (`ag_point_view`); read only in those modes, so no other render pays.
fn ag_stage_display(color: vec3<f32>, mode: u32, coord: vec2<u32>, scene_linear: vec3<f32>, lut_input: vec3<f32>) -> vec3<f32> {
    if (mode == 7u || mode == 8u || mode == 9u) {
        return textureLoad(input_texture, coord, 0).rgb;
    }
    if (mode == 5u || mode == 6u) {
        return ag_point_view(color, ag_look_entry(scene_linear, lut_input), mode);
    }
    return ag_clipping_view(color, mode);
}
