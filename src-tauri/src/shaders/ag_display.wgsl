// ============================================================================
// ARGENTUM — display colour management
//
// Concatenated in front of their display.wgsl, the same way modules.wgsl is
// concatenated in front of shader.wgsl, so everything of ours lives in a file
// of ours and their shader gains one call.
//
// WHAT THIS IS FOR
//
// The pipeline produces sRGB. The native preview handed those numbers to the
// panel untouched, which is right only if the panel is sRGB. On a wide-gamut
// display the same numbers drive more saturated primaries, so every photo was
// shown over-saturated — invisibly, because nothing sat beside it to compare
// against. The crop view did compare: it goes out as a JPEG through WebView2,
// which converts properly, and the two disagreed.
//
// See mods/display_profile.rs for where the matrix comes from. It is read from
// the monitor the window is on, so it follows the screen rather than describing
// one particular laptop.
// ============================================================================

/// One sRGB channel to linear light. The standard piecewise curve.
fn ag_srgb_to_linear_channel(c: f32) -> f32 {
    if (c <= 0.04045) {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

fn ag_linear_to_srgb_channel(c: f32) -> f32 {
    if (c <= 0.0031308) {
        return c * 12.92;
    }
    return 1.055 * pow(c, 1.0 / 2.4) - 0.055;
}

/// Convert a colour from sRGB into whatever the display actually shows.
///
/// The matrix works in linear light, because that is where mixing primaries
/// means anything, so the value is decoded, converted and encoded again.
///
/// `rows[0].w` says whether there is anything to do. An sRGB screen gets a
/// matrix within rounding of the identity, and the Rust side reports that as
/// "no conversion" rather than passing it — so those displays keep the exact
/// numbers the pipeline produced, with no decode/encode round trip to add error
/// to. A conversion that is arithmetically nothing should also be nothing in
/// practice.
///
/// The result is clamped because a colour outside the display's gamut has no
/// representation on it. That is not a loss introduced here: it is what the
/// screen can show, said honestly, and the alternative is a negative number
/// that becomes something arbitrary at the end of the pipe.
fn ag_to_display(colour: vec3<f32>, rows: array<vec4<f32>, 3>) -> vec3<f32> {
    if (rows[0].w < 0.5) {
        return colour;
    }

    let linear = vec3<f32>(
        ag_srgb_to_linear_channel(colour.r),
        ag_srgb_to_linear_channel(colour.g),
        ag_srgb_to_linear_channel(colour.b),
    );

    let converted = clamp(
        vec3<f32>(
            dot(rows[0].xyz, linear),
            dot(rows[1].xyz, linear),
            dot(rows[2].xyz, linear),
        ),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );

    return vec3<f32>(
        ag_linear_to_srgb_channel(converted.r),
        ag_linear_to_srgb_channel(converted.g),
        ag_linear_to_srgb_channel(converted.b),
    );
}

/// The display stage: everything Argentum does on the way to the screen.
///
/// One call in their fragment shader, so the next thing that has to happen at
/// presentation time costs nothing there.
fn ag_stage_present(colour: vec4<f32>, rows: array<vec4<f32>, 3>) -> vec4<f32> {
    return vec4<f32>(ag_to_display(colour.rgb, rows), colour.a);
}
