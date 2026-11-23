//! Which clipping view the photo is being shown in. Ours.
//!
//! RapidRAW has one clipping warning: any channel over the line goes red, any
//! pixel under it goes blue. That answers "is something clipped" and not the
//! question you ask next, which is *what* clipped — because a highlight where
//! only red has gone is recoverable and one where all three have gone is not.
//!
//! So the button cycles rather than toggles: off, all channels, then red, green
//! and blue on their own.
//!
//! Three more modes are not on that cycle because they are *held*, not chosen:
//! Ctrl while dragging Whites or Blacks empties the picture to black or white
//! and shows only what is being blown or crushed, which is how a white point
//! and a black point are actually set. Lightroom does this with Alt; Alt and
//! Shift on a slider here already mean fine adjustment, so it is Ctrl. With a
//! film look on, what the photo loses on its way into the look shows too: a
//! faded look never prints black or white, so the screen alone would show
//! nothing (`ag_point_view` in modules.wgsl). The third is the sharpening
//! mask, held from the Sharpening card the same way.
//!
//! WHY NOT ONE VIEW WITH THE CHANNELS COLOUR-CODED
//!
//! That was the first version, and it was better on paper: one look tells you
//! whether a highlight is savable, with no remembering. It was also misread
//! within seconds of being shown, because in the first mode red already means
//! *blown highlight* and in that one it would have meant *the red channel*.
//! Two meanings on one button. Red is blown and blue is crushed in every mode
//! here, and which channel you are looking at is the thing the button says.
//!
//! WHY THE FIELD IS STILL CALLED `showClipping`
//!
//! Because it is theirs, and it is stored per photo alongside every other
//! adjustment. Their button reads it as a truthy value and highlights when it is
//! set, which a mode number satisfies without them knowing anything about modes.
//! Adding a second field of our own would have meant a new key in the saved
//! adjustments — and every key there is part of the thumbnail cache hash, so a
//! new one rebuilds the user's whole library once for nothing.

// The mode constants are a contract with two things Rust cannot see: the WGSL
// in shaders/modules.wgsl, which compares against the same numbers, and the
// frontend, which cycles through them. Naming them here is what keeps those
// three in step, so they are not dead — they are the definition.
#![allow(dead_code)]

/// Off. The photo as it is.
pub const OFF: u32 = 0;
/// Any channel: red for clipped, blue for crushed. RapidRAW's original.
pub const ALL: u32 = 1;
/// One channel at a time, same two colours.
pub const RED: u32 = 2;
pub const GREEN: u32 = 3;
pub const BLUE: u32 = 4;
/// How many steps the button cycles through, off included.
pub const COUNT: u32 = 5;

/// Setting the white point: the picture goes black and only what is being
/// blown shows. Not part of the button's cycle — it is held, while dragging.
pub const WHITE_POINT: u32 = 5;
/// Setting the black point: the picture goes white and only what is being
/// crushed shows.
pub const BLACK_POINT: u32 = 6;
/// What sharpening touches: white where it sharpens, black where the contrast
/// mask leaves the picture alone - RawTherapee's "show contrast mask". Held
/// from the Sharpening card, never saved; see `mods/sharpen.rs`.
pub const SHARPEN_MASK: u32 = 7;
/// The same for the manual sharpen's mask, when it has its own.
pub const SHARPEN_MASK_USM: u32 = 8;
/// The same for a photo mask's own Sharpen, inside that mask - the one
/// `agSharpenMask` names.
pub const SHARPEN_MASK_LOCAL: u32 = 9;
/// Every mode, including the held ones.
pub const LAST: u32 = 9;

/// Read the mode out of the adjustments the frontend sent.
///
/// Accepts the old `true`/`false` as well as a number, because sidecars written
/// before this existed hold a boolean and must keep meaning what they meant.
pub fn mode(js_adjustments: &serde_json::Value) -> u32 {
    let value = &js_adjustments["showClipping"];
    if let Some(n) = value.as_u64() {
        return if n <= LAST as u64 { n as u32 } else { OFF };
    }
    if value.as_bool().unwrap_or(false) {
        ALL
    } else {
        OFF
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_missing_setting_is_off() {
        assert_eq!(mode(&json!({})), OFF);
        assert_eq!(mode(&json!({ "showClipping": null })), OFF);
    }

    /// Sidecars written before modes existed hold a boolean, and it has to keep
    /// meaning exactly what it meant.
    #[test]
    fn the_old_boolean_still_works() {
        assert_eq!(mode(&json!({ "showClipping": true })), ALL);
        assert_eq!(mode(&json!({ "showClipping": false })), OFF);
    }

    #[test]
    fn the_modes_come_through_as_numbers() {
        assert_eq!(mode(&json!({ "showClipping": 0 })), OFF);
        assert_eq!(mode(&json!({ "showClipping": 1 })), ALL);
        assert_eq!(mode(&json!({ "showClipping": 2 })), RED);
        assert_eq!(mode(&json!({ "showClipping": 3 })), GREEN);
        assert_eq!(mode(&json!({ "showClipping": 4 })), BLUE);
    }

    /// A number nobody wrote on purpose must land somewhere real rather than
    /// reaching past the end of the list.
    #[test]
    fn rubbish_lands_on_a_real_mode() {
        assert_eq!(mode(&json!({ "showClipping": 5 })), WHITE_POINT);
        assert_eq!(mode(&json!({ "showClipping": 6 })), BLACK_POINT);
        assert_eq!(mode(&json!({ "showClipping": 7 })), SHARPEN_MASK);
        assert_eq!(mode(&json!({ "showClipping": 8 })), SHARPEN_MASK_USM);
        assert_eq!(mode(&json!({ "showClipping": 9 })), SHARPEN_MASK_LOCAL);
        assert_eq!(mode(&json!({ "showClipping": 99 })), OFF);
        assert_eq!(mode(&json!({ "showClipping": "yes" })), OFF);
    }
}

/// The white and black point views through the real shader, on a GPU:
/// `cargo test --lib clipping::gpu -- --ignored`. What they light with a film
/// look on is `ag_point_view` in modules.wgsl.
#[cfg(test)]
mod gpu {
    use super::*;
    use crate::image_processing::GpuContext;
    use crate::lut_processing::Lut;
    use crate::mods::dng_standard;
    use crate::mods::dng_standard::gpu::{centre, device, image_of, render_with_lut};
    use crate::mods::film_lut::KEY as DECLARED_INPUT;
    use serde_json::{Value, json};
    use std::sync::Arc;

    /// A faded look: everything printed between 10% and 90%, so on screen
    /// nothing is ever black or white - the photo that showed nothing had one
    /// printing black as 17% grey.
    fn faded() -> Arc<Lut> {
        let size = 17u32;
        let step = 1.0 / (size - 1) as f32;
        let mut data = Vec::with_capacity((size * size * size * 3) as usize);
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    data.extend([r, g, b].map(|v| 0.1 + 0.8 * v as f32 * step));
                }
            }
        }
        Arc::new(Lut { size, data })
    }

    /// The two places a look goes on.
    #[derive(Clone, Copy, Debug)]
    enum Look {
        /// A film look that comes with the app: scene-referred, fed its
        /// declared input.
        BuiltIn,
        /// A LUT imported into Looks: after the Curves, on the display picture.
        Imported,
    }

    /// An edit with the faded look on as `look` at `intensity`, plus `extra`.
    fn with_look(look: Look, intensity: u32, extra: Value) -> Value {
        let mut js = json!({
            "rawToneRendering": "default",
            "agSharpen": { "capture": false, "amount": 0 },
            "lutPath": "faded",
            "lutIntensity": intensity,
            "lutIsSceneReferred": matches!(look, Look::BuiltIn),
            // Looks marks every LUT it puts on (filmLookInput.ts).
            DECLARED_INPUT: true,
        });
        for (k, v) in extra.as_object().expect("an object") {
            js[k] = v.clone();
        }
        js
    }

    fn render(
        ctx: &GpuContext,
        patches: &[[f32; 3]],
        js: &Value,
        view: u32,
        lut: Option<Arc<Lut>>,
    ) -> Vec<[f32; 3]> {
        let mut js = js.clone();
        js["showClipping"] = json!(view);
        let out = render_with_lut(
            ctx,
            &image_of(patches),
            js,
            crate::white_balance::WhiteBalance::reference(),
            lut,
        );
        (0..patches.len()).map(|i| centre(&out, i)).collect()
    }

    const LIT_WHITE: [f32; 3] = [1.0, 1.0, 1.0];
    const LIT_BLACK: [f32; 3] = [0.0, 0.0, 0.0];

    fn same(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    /// Without a look the views light exactly what is blown or crushed on
    /// screen, as they always did - and the same with either kind of look at
    /// 0%, or a film look put on before looks were fed their declared input.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn gpu_without_a_look_the_point_views_judge_the_screen() {
        let ctx = device().expect("no wgpu adapter");
        let patches = [
            [0.0, 0.0, 0.0],
            [0.0005, 0.0005, 0.0005],
            [0.01, 0.01, 0.01],
            [0.18, 0.18, 0.18],
            [0.9, 0.9, 0.9],
            [3.0, 3.0, 3.0],
            [0.9, 0.05, 0.0],
            [0.0, 0.3, 0.9],
        ];
        let edits = [
            (
                json!({ "rawToneRendering": "default", "agSharpen": { "capture": false, "amount": 0 } }),
                None,
            ),
            (with_look(Look::BuiltIn, 0, json!({})), Some(faded())),
            (with_look(Look::Imported, 0, json!({})), Some(faded())),
            (
                with_look(Look::BuiltIn, 100, json!({ DECLARED_INPUT: false })),
                Some(faded()),
            ),
        ];
        for (js, lut) in edits {
            let screen = render(&ctx, &patches, &js, OFF, lut.clone());
            let white = render(&ctx, &patches, &js, WHITE_POINT, lut.clone());
            let black = render(&ctx, &patches, &js, BLACK_POINT, lut);
            let (mut blown, mut crushed) = (0, 0);
            for (i, s) in screen.iter().enumerate() {
                let want_white = s.map(|c| if c > 0.998 { 1.0 } else { 0.0 });
                let want_black = s.map(|c| if c < 0.002 { 0.0 } else { 1.0 });
                assert!(
                    same(white[i], want_white),
                    "{js}: patch {i} on screen {s:?}, white point view {:?}",
                    white[i]
                );
                assert!(
                    same(black[i], want_black),
                    "{js}: patch {i} on screen {s:?}, black point view {:?}",
                    black[i]
                );
                blown += usize::from(want_white != LIT_BLACK);
                crushed += usize::from(want_black != LIT_WHITE);
            }
            // Something must be at each limit on screen, or this proves nothing.
            if js["lutIntensity"].as_u64().unwrap_or(0) == 0 {
                assert!(
                    blown > 0 && crushed > 0,
                    "{js}: no blown or no crushed patch on screen"
                );
            }
        }
    }

    /// The reported case: a faded look, Exposure -2 and Blacks -100. What the
    /// photo crushes never reaches the screen as black, and the view shows it.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn gpu_a_look_shows_what_the_photo_crushes_before_it() {
        let ctx = device().expect("no wgpu adapter");
        let patches = [
            [0.0, 0.0, 0.0],
            [0.004, 0.004, 0.004],
            [0.18, 0.18, 0.18],
            [0.6, 0.6, 0.6],
        ];
        for look in [Look::BuiltIn, Look::Imported] {
            let js = with_look(look, 100, json!({ "exposure": -2.0, "blacks": -100.0 }));
            let screen = render(&ctx, &patches, &js, OFF, Some(faded()));
            let black = render(&ctx, &patches, &js, BLACK_POINT, Some(faded()));
            // The first two patches are crushed before the look; the rest are not.
            for (i, (s, b)) in screen.iter().zip(&black).enumerate() {
                if i < 2 {
                    assert!(
                        s.iter().all(|&c| c > 0.05),
                        "{look:?}: patch {i} is black on screen {s:?}: the look did not fade it"
                    );
                    assert!(
                        same(*b, LIT_BLACK),
                        "{look:?}: patch {i}, crushed before the look, not lit: {b:?}"
                    );
                } else {
                    assert!(
                        same(*b, LIT_WHITE),
                        "{look:?}: patch {i} lit though nothing crushed it: {b:?}"
                    );
                }
            }
        }
    }

    /// A curve that crushes black still shows: after a film look the screen
    /// is asked as well as the look's input, and an imported LUT is handed
    /// the picture after the Curves.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn gpu_a_curve_that_crushes_black_still_shows() {
        let ctx = device().expect("no wgpu adapter");
        let patches = [[0.1, 0.1, 0.1], [0.9, 0.9, 0.9]];
        let identity = json!([{ "x": 0, "y": 0 }, { "x": 255, "y": 255 }]);
        let curves = json!({
            "luma": [{ "x": 0, "y": 0 }, { "x": 128, "y": 0 }, { "x": 255, "y": 255 }],
            "red": identity, "green": identity, "blue": identity,
        });
        for look in [Look::BuiltIn, Look::Imported] {
            let js = with_look(look, 100, json!({ "curves": curves }));
            let screen = render(&ctx, &patches, &js, OFF, Some(faded()));
            let black = render(&ctx, &patches, &js, BLACK_POINT, Some(faded()));
            if matches!(look, Look::BuiltIn) {
                assert!(
                    screen[0].iter().all(|&c| c < 0.002),
                    "the curve after the film look did not crush the patch: {:?}",
                    screen[0]
                );
            }
            assert!(
                same(black[0], LIT_BLACK),
                "{look:?}: crushed by the curve, not lit: {:?}",
                black[0]
            );
            assert!(
                same(black[1], LIT_WHITE),
                "{look:?}: lit though nothing crushed it: {:?}",
                black[1]
            );
        }
    }

    /// A RAW in Linear reaches the look at the camera's offset, applied after
    /// the cap and into V-Log, which keeps it: a highlight between 2^-offset
    /// and the cap is printed with its detail and must not light. One past the
    /// cap is lost, and does.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn gpu_highlights_the_film_look_keeps_are_not_lit() {
        let ctx = device().expect("no wgpu adapter");
        let ev = 0.5;
        let patches = [[0.18, 0.18, 0.18], [0.95, 0.95, 0.95], [1.2, 1.2, 1.2]];
        let js = with_look(
            Look::BuiltIn,
            100,
            json!({
                "rawToneRendering": dng_standard::LINEAR_MODE,
                "rawToneCurveMode": dng_standard::LINEAR_MODE,
                "rawToneCurve": dng_standard::linear_points(ev),
            }),
        );
        assert!(
            0.95 > (-ev).exp2(),
            "the kept highlight must sit above 2^-offset"
        );
        let screen = render(&ctx, &patches, &js, OFF, Some(faded()));
        let white = render(&ctx, &patches, &js, WHITE_POINT, Some(faded()));
        assert!(
            screen.iter().flatten().all(|&c| c < 0.95),
            "the look printed white: {screen:?}"
        );
        assert!(same(white[0], LIT_BLACK), "mid grey lit: {:?}", white[0]);
        assert!(
            same(white[1], LIT_BLACK),
            "a highlight the look keeps lit: {:?}",
            white[1]
        );
        assert!(
            same(white[2], LIT_WHITE),
            "a highlight past the look's cap not lit: {:?}",
            white[2]
        );
    }
}
