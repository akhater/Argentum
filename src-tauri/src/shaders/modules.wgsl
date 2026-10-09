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

/// Sharpening's settings, as `mods/sharpen.rs` lays them out.
///
/// This shader never reads them: sharpening is whole-image passes, run by
/// `mods/input_stage.rs` before this shader starts, and what arrives here as
/// `input_texture` is already sharpened. The struct exists so their
/// `GlobalAdjustments` has the same layout on both sides of the buffer -
/// 48 bytes, field for field with `sharpen::Params`.
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
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
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
/// Mode 7 is the sharpening mask. The input stage has already written the
/// mask into this render's input in place of the photo, so it is read back
/// here, at this pixel, exactly as written: every adjustment in between has
/// been run on it and is thrown away, because a mask that went through the
/// tone curve would no longer say how strongly each pixel is sharpened.
fn ag_stage_display(color: vec3<f32>, mode: u32, coord: vec2<u32>) -> vec3<f32> {
    if (mode == 7u) {
        return textureLoad(input_texture, coord, 0).rgb;
    }
    return ag_clipping_view(color, mode);
}
