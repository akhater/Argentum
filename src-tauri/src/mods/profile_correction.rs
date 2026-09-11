//! Applying a camera profile after the decode instead of during it.
//!
//! WHY THIS EXISTS
//!
//! The profile used to be applied while the RAW was decoded, by substituting
//! its matrix for rawler's. That is the natural place for it and it does not
//! work here, for a reason that has nothing to do with colour: every other
//! adjustment in this app is applied on the GPU, per frame, so the app has no
//! notion of a setting that needs the file read again. Four attempts to bolt
//! one on each broke something else — a black flash on every switch, a
//! debounced save racing the choice and reverting it, a preview worker finding
//! no image because the decode had cleared it.
//!
//! So: decode once, normally, with rawler's own matrix. Then apply the
//! *difference* between that matrix and the profile's, where every other
//! adjustment already happens. Same arithmetic, different place, and switching
//! becomes instant because nothing has to be read again.
//!
//! THE DIFFERENCE
//!
//! The decode leaves scene-linear sRGB, having applied
//!
//! ```text
//! builtin = pinv(normalize(builtin_xyz2cam · sRGB→XYZ))
//! ```
//!
//! and what we want applied is the same expression with the profile's matrix.
//! Going from one to the other is
//!
//! ```text
//! correction = wanted · inv(builtin)
//! ```
//!
//! which is a single 3x3 on already-decoded pixels. Identity when no profile is
//! chosen, so a photo without one is untouched — not approximately untouched,
//! exactly: the matrix is the identity and the shader multiplies by it.

use std::sync::{Mutex, OnceLock};

use super::profile_matrix::{invert, multiply};

/// sRGB primaries to XYZ D65, matching rawler's own constant.
const SRGB_TO_XYZ_D65: [[f32; 3]; 3] = [
    [0.412_456_4, 0.357_576_1, 0.180_437_5],
    [0.212_672_9, 0.715_152_2, 0.072_175_0],
    [0.019_333_9, 0.119_192_0, 0.950_304_1],
];

pub const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// The matrix rawler used, per photo.
///
/// WHY IT IS NOT ONE SLOT
///
/// It was, with a comment saying one photo is open at a time. True of the
/// editor, false of export, which runs four workers each loading its own RAW.
/// A worker decoding a second photo between another worker's decode and render
/// handed that render the wrong matrix, and the correction inverted a transform
/// that was never applied. Silent, and wrong in colour rather than behaviour.
///
/// WHY IT IS NOT KEYED BY CAMERA EITHER
///
/// That was the second attempt: the matrix belongs to the body, not the frame.
/// True of a CR2, where rawler reads it from its own camera table; false of a
/// DNG, which carries its own. One camera, several matrices.
///
/// WHY THE RENDER IS TOLD WHICH PHOTO IT IS, RATHER THAN GUESSING
///
/// The third attempt inferred it: the last photo decoded on this thread, and
/// failing that whatever the editor had open. It reads well and it is guesswork.
/// Export of the *open* photo reuses the editor's pixels and never decodes, so
/// a worker that decoded something else in a previous job answered with that
/// photo's path and beat the correct fallback. An audit reproduced it.
///
/// So a render says what it is rendering. `rendering()` returns a guard that
/// holds the path for as long as the render lasts and clears it on the way out,
/// which is the one arrangement that cannot go stale: it is tied to a scope
/// rather than to the history of a thread.
///
/// It costs two lines in their files, at the two places a render begins without
/// a decode of its own — the preview worker and the export worker. That is a
/// deliberate exception to the anchor rule, taken because the alternative was a
/// heuristic that is wrong in a case a person would hit.
static BUILT_IN: Mutex<Option<Vec<(String, [f32; 9])>>> = Mutex::new(None);

pub fn remember_built_in(path: Option<&str>, matrix: [f32; 9]) {
    let Some(path) = path.map(str::to_string) else {
        return;
    };
    if let Ok(mut slot) = BUILT_IN.lock() {
        let known = slot.get_or_insert_with(Vec::new);
        if let Some(entry) = known.iter_mut().find(|(p, _)| *p == path) {
            entry.1 = matrix;
        } else {
            known.push((path, matrix));
        }
        evict_if_crowded(known);
    }
}

/// Least *recently used*, and never the photo the app has open.
///
/// Insertion order was the first rule, and an export of a few hundred photos
/// walked the open photo out of the table — after which every frame in the
/// editor rendered without its profile and nothing said so.
///
/// Least-recently-used fixed that only while the editor was drawing. Leave the
/// app sitting on a photo, start an export of three hundred, and the open photo
/// is read by nobody and evicted anyway — pointed out by an audit after I had
/// written a test that read it every fifty inserts and so could never see it.
/// A photo that is open is in use whether or not anyone is looking at it, so it
/// is skipped outright.
fn evict_if_crowded(known: &mut Vec<(String, [f32; 9])>) {
    evict_keeping(known, open_photo().as_deref());
}

/// The rule on its own, so it can be tested without an app to ask.
fn evict_keeping(known: &mut Vec<(String, [f32; 9])>, open: Option<&str>) {
    const KEEP: usize = 256;
    if known.len() <= KEEP {
        return;
    }
    if let Some(at) = known.iter().position(|(path, _)| open != Some(path.as_str())) {
        known.remove(at);
    }
}

/// The app handle, kept so eviction can ask which photo is open.
///
/// Set once at startup. `None` in tests and in the offline harnesses, which is
/// why every read falls back rather than unwrapping.
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

pub fn remember_app_handle(handle: tauri::AppHandle) {
    let _ = APP.set(handle);
}

/// The photo the editor has open, if the app is running.
fn open_photo() -> Option<String> {
    let handle = APP.get()?;
    let state = <tauri::AppHandle as tauri::Manager<tauri::Wry>>::state::<crate::AppState>(handle);
    // `get_original_image` clones the Arc and drops the guard, so this lock is
    // never held across a render.
    let open = state.original_image.lock().ok()?;
    open.as_ref().map(|loaded| loaded.path.clone())
}

/// Move an entry to the end, marking it as just used.
fn touch(known: &mut Vec<(String, [f32; 9])>, at: usize) {
    let entry = known.remove(at);
    known.push(entry);
}

pub fn forget_built_in() {
    if let Ok(mut slot) = BUILT_IN.lock() {
        *slot = None;
    }
}

/// The matrix the photo being rendered was decoded with.
///
/// `None` when it is not known, which is the honest answer: a correction
/// derived from a *different* photo's matrix is worse than no correction.
fn built_in_for_this_render(photo: Option<&str>) -> Option<[f32; 9]> {
    // `None` means no photo was named, and that is the end of it.
    //
    // It used to mean "the one the app has open", which reads like a kindness
    // and is a guess wearing a default's clothes: a caller that captured a
    // photo, queued the work and rendered it later would silently be told about
    // whichever photo had been opened in the meantime. Every render that has a
    // photo now says so, and the only caller left passing `None` is the check
    // that asks whether adjustments differ from neutral — which renders nothing
    // and has no photo to name.
    let path = photo?.to_string();
    let mut known = BUILT_IN.lock().ok()?;
    let known = known.as_mut()?;
    let at = known.iter().position(|(p, _)| *p == path)?;
    touch(known, at);
    Some(known[known.len() - 1].1)
}

fn as_rows(flat: &[f32; 9]) -> [[f32; 3]; 3] {
    [
        [flat[0], flat[1], flat[2]],
        [flat[3], flat[4], flat[5]],
        [flat[6], flat[7], flat[8]],
    ]
}

/// Camera RGB to sRGB, the way rawler derives it from an `xyz2cam`.
///
/// Reproduced rather than imported because it is not public, and because the
/// whole correction is only correct if it matches what the decode actually did
/// — `normalize` scaling each row to sum to one is the part that matters, since
/// it is what makes white map to white.
fn cam_to_srgb(xyz2cam: &[f32; 9]) -> Option<[[f32; 3]; 3]> {
    let mut rgb2cam = multiply(&as_rows(xyz2cam), &SRGB_TO_XYZ_D65);
    for row in rgb2cam.iter_mut() {
        let sum: f32 = row.iter().sum();
        if sum.abs() > 1e-9 {
            for cell in row.iter_mut() {
                *cell /= sum;
            }
        }
    }
    invert(&rgb2cam)
}

/// What to multiply decoded pixels by so they look as though the profile had
/// been used instead of rawler's matrix.
pub fn correction_for(photo: Option<&str>, profile_xyz2cam: &[f32; 9]) -> Option<[[f32; 3]; 3]> {
    let built_in = built_in_for_this_render(photo)?;
    let from = cam_to_srgb(&built_in)?;
    let to = cam_to_srgb(profile_xyz2cam)?;
    Some(multiply(&to, &invert(&from)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These tests all write the same global, and Rust runs tests in parallel,
    /// so one calling `forget_built_in` can empty the table another is halfway
    /// through using. They pass alone and fail together, which is the most
    /// annoying way for a test to be wrong. One at a time.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    /// Held for the length of a test. Poisoning is ignored: a failing test has
    /// already reported itself, and it must not turn every later test red too.
    fn serialised() -> std::sync::MutexGuard<'static, ()> {
        ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Canon EOS 5D Mark II as rawler holds it, and as the published profile
    /// gives it after the ForwardMatrix derivation.
    const BUILT_IN_5D2: [f32; 9] = [
        0.4716, 0.0603, -0.083, -0.7798, 1.5474, 0.248, -0.1496, 0.1937, 0.6651,
    ];

    fn apply(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
        [
            m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
            m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
            m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
        ]
    }

    /// The same matrix on both sides must change nothing. This is the case that
    /// protects every photo without a profile.
    #[test]
    fn correcting_to_itself_is_the_identity() {
        let _guard = serialised();
        remember_built_in(Some("A.CR2"), BUILT_IN_5D2);
        let c = correction_for(Some("A.CR2"), &BUILT_IN_5D2).expect("derivable");
        for i in 0..3 {
            for j in 0..3 {
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((c[i][j] - want).abs() < 1e-3, "not the identity: {c:?}");
            }
        }
        forget_built_in();
    }

    /// Neutral must stay neutral. Both matrices map white to white by
    /// construction, so the correction between them has to as well — if it does
    /// not, choosing a profile would shift the white balance, which is not what
    /// a profile is for.
    #[test]
    fn neutral_survives_the_correction() {
        let _guard = serialised();
        remember_built_in(Some("A.CR2"), BUILT_IN_5D2);
        // The profile's, derived from its ForwardMatrix.
        let profile = super::super::profile_matrix::xyz2cam_from_forward(&[
            0.6399, 0.1294, 0.1949, 0.2827, 0.6579, 0.0594, 0.0001, 0.0051, 0.8199,
        ])
        .expect("derivable");

        let c = correction_for(Some("A.CR2"), &profile).expect("derivable");
        let out = apply(&c, [1.0, 1.0, 1.0]);
        assert!(
            (out[0] - out[1]).abs() < 0.02 && (out[1] - out[2]).abs() < 0.02,
            "white became {out:?}"
        );
        forget_built_in();
    }

    /// And it has to actually do something, or this is an elaborate identity.
    #[test]
    fn a_different_profile_changes_saturated_colour() {
        let _guard = serialised();
        remember_built_in(Some("A.CR2"), BUILT_IN_5D2);
        // Red and blue swapped, the test profile.
        let swapped = super::super::profile_matrix::xyz2cam_from_forward(&[
            0.1949, 0.1294, 0.6399, 0.0594, 0.6579, 0.2827, 0.8199, 0.0051, 0.0001,
        ])
        .expect("derivable");

        let c = correction_for(Some("A.CR2"), &swapped).expect("derivable");
        let red = apply(&c, [1.0, 0.2, 0.2]);
        assert!(
            (red[0] - 1.0).abs() > 0.1 || (red[2] - 0.2).abs() > 0.1,
            "a swapped profile left red alone: {red:?}"
        );
        forget_built_in();
    }

    /// Choosing a profile counts as editing the photo.
    ///
    /// It is invisible to the struct comparison that decides this, because that
    /// comparison names no photo and so gets the identity correction on both
    /// sides — so a photo whose only change was its profile came out as
    /// untouched, and would have been treated as one everywhere that matters.
    #[test]
    fn a_chosen_profile_makes_a_photo_edited() {
        use serde_json::json;
        let edited = |adj| crate::image_processing::is_image_edited(&adj, true, None);

        assert!(edited(json!({ "cameraProfile": "Canon EOS 5D Mark II.dcp" })));
        assert!(!edited(json!({ "cameraProfile": serde_json::Value::Null })));
        assert!(!edited(json!({ "cameraProfile": "" })));
        assert!(!edited(json!({ "cameraProfile": "   " })));
        assert!(!edited(json!({})));
    }

    /// A virtual copy renders the physical file, so that is the path it has to
    /// ask about.
    ///
    /// The preview for a virtual copy decoded `photo.CR2` and then asked for
    /// `photo.CR2?vc=1`, which nothing had ever recorded — so the lookup missed
    /// and the profile silently stopped applying to every virtual copy.
    #[test]
    fn a_virtual_copy_asks_about_the_file_on_disk() {
        let _guard = serialised();
        forget_built_in();
        remember_built_in(Some("photo.CR2"), BUILT_IN_5D2);

        assert!(
            correction_for(Some("photo.CR2"), &BUILT_IN_5D2).is_some(),
            "the physical path is what the decode recorded"
        );
        assert!(
            correction_for(Some("photo.CR2?vc=1"), &BUILT_IN_5D2).is_none(),
            "a virtual path was never recorded, so asking with one finds nothing"
        );
        forget_built_in();
    }

    /// Naming no photo means no correction, not a guess at the open one.
    #[test]
    fn naming_no_photo_corrects_nothing() {
        let _guard = serialised();
        forget_built_in();
        remember_built_in(Some("photo.CR2"), BUILT_IN_5D2);
        assert!(correction_for(None, &BUILT_IN_5D2).is_none());
        forget_built_in();
    }

    /// The export bug, as a test.
    ///
    /// Two photos decoded, in the order that used to break it: the second
    /// decode overwrote the first photo's matrix, so the first photo's render
    /// got the second's. Deliberately two files from the *same* camera with
    /// different matrices, because keying by camera was the first fix and this
    /// is the case it still got wrong — a DNG carries its own matrix, so one
    /// body can produce several.
    #[test]
    fn a_second_photo_does_not_take_the_first_ones_matrix() {
        let _guard = serialised();
        forget_built_in();
        // A deliberately different matrix, so using the wrong one cannot
        // accidentally give the right answer.
        const OTHER: [f32; 9] = [0.9, -0.2, -0.05, -0.4, 1.3, 0.1, -0.1, 0.2, 0.7];

        remember_built_in(Some("A.CR2"), BUILT_IN_5D2);
        remember_built_in(Some("B.DNG"), OTHER);

        // Now render A. In the app this is the thread that decoded it, so it
        // says so; the point is that B's decode came in between and A still
        // gets its own matrix.
        // Correcting A to its own matrix is the identity, and stays so even
        // though another photo was decoded afterwards.
        let c = correction_for(Some("A.CR2"), &BUILT_IN_5D2).expect("derivable");
        for i in 0..3 {
            for j in 0..3 {
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((c[i][j] - want).abs() < 1e-3, "photo A got somebody else's matrix: {c:?}");
            }
        }

        let n = correction_for(Some("B.DNG"), &OTHER).expect("derivable");
        for i in 0..3 {
            for j in 0..3 {
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((n[i][j] - want).abs() < 1e-3, "photo B got somebody else's matrix: {n:?}");
            }
        }
        forget_built_in();
    }

    /// The stale-thread bug, as a test.
    ///
    /// This used to be a guard held for the length of a render, and it worked
    /// wherever it was placed — which turned out to be three of the many places
    /// a render begins. A render now takes the photo as an argument, so a
    /// caller cannot forget to say and there is no thread history to go stale.
    #[test]
    fn a_render_gets_its_own_photos_matrix_whatever_was_decoded_last() {
        let _guard = serialised();
        forget_built_in();
        const OTHER: [f32; 9] = [0.9, -0.2, -0.05, -0.4, 1.3, 0.1, -0.1, 0.2, 0.7];

        remember_built_in(Some("OPEN.CR2"), BUILT_IN_5D2);
        // Something else decoded afterwards, on this very thread.
        remember_built_in(Some("A.CR2"), OTHER);

        let c = correction_for(Some("OPEN.CR2"), &BUILT_IN_5D2).expect("derivable");
        for i in 0..3 {
            for j in 0..3 {
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((c[i][j] - want).abs() < 1e-3, "got the other photo's matrix: {c:?}");
            }
        }
        forget_built_in();
    }

    /// The eviction bug, as a test.
    ///
    /// A long export walked the open photo out of a table that dropped whatever
    /// was inserted first, after which every frame in the editor rendered
    /// without its profile and nothing said so. Read order decides now, and the
    /// open photo is read every frame.
    #[test]
    fn a_long_export_does_not_evict_the_photo_being_edited() {
        let _guard = serialised();
        forget_built_in();
        remember_built_in(Some("OPEN.CR2"), BUILT_IN_5D2);

        for n in 0..1000 {
            // Every so often the editor draws a frame, which is what keeps its
            // photo alive.
            if n % 50 == 0 {
                assert!(
                    correction_for(Some("OPEN.CR2"), &BUILT_IN_5D2).is_some(),
                    "the open photo was thrown away after {n} exports"
                );
            }
            remember_built_in(Some(&format!("export-{n}.CR2")), [0.5; 9]);
        }

        assert!(
            correction_for(Some("OPEN.CR2"), &BUILT_IN_5D2).is_some(),
            "the open photo was thrown away by the end of the export"
        );
        forget_built_in();
    }

    /// The idle-editor bug, as a test.
    ///
    /// The rule before this was least-recently-used, which protects the open
    /// photo only while somebody is dragging a slider. Leave the app sitting on
    /// a photo, start an export of three hundred, and nothing reads it — so it
    /// was evicted anyway, and every frame afterwards rendered without its
    /// profile.
    ///
    /// My own test for the previous fix read the open photo every fifty inserts
    /// and so could never have caught this. This one never reads it at all.
    #[test]
    fn an_open_photo_survives_an_export_even_if_nobody_looks_at_it() {
        let mut known: Vec<(String, [f32; 9])> = vec![("OPEN.CR2".to_string(), [1.0; 9])];

        for n in 0..1000 {
            known.push((format!("export-{n}.CR2"), [0.5; 9]));
            evict_keeping(&mut known, Some("OPEN.CR2"));
        }

        assert!(
            known.iter().any(|(p, _)| p == "OPEN.CR2"),
            "the open photo was evicted while nobody was looking at it"
        );
        assert!(known.len() <= 257, "the table grew without bound: {}", known.len());
    }

    /// And with nothing open — every offline harness, and the app before a photo
    /// is loaded — it is plain least-recently-used with no special case.
    #[test]
    fn with_no_photo_open_the_oldest_goes() {
        let mut known: Vec<(String, [f32; 9])> = (0..300)
            .map(|n| (format!("photo-{n}.CR2"), [0.5; 9]))
            .collect();
        evict_keeping(&mut known, None);
        assert_eq!(known.len(), 299);
        assert!(!known.iter().any(|(p, _)| p == "photo-0.CR2"));
    }

    /// A render whose photo was never decoded gets no correction at all.
    /// Silently using another photo's matrix is how the bug above looked from
    /// the outside, and it must not be reachable by any route.
    #[test]
    fn a_render_of_an_unknown_photo_corrects_nothing() {
        let _guard = serialised();
        forget_built_in();
        remember_built_in(Some("A.CR2"), BUILT_IN_5D2);
        assert!(correction_for(Some("never-decoded.CR2"), &BUILT_IN_5D2).is_none());
        forget_built_in();
    }

    /// With nothing decoded there is nothing to correct against, and that must
    /// be a `None` rather than a wrong answer.
    #[test]
    fn without_a_decode_there_is_no_correction() {
        let _guard = serialised();
        forget_built_in();
        assert!(correction_for(Some("A.CR2"), &BUILT_IN_5D2).is_none());
    }
}

/// The three shader rows for whatever profile this photo asked for.
///
/// Identity whenever there is no profile, no library, or anything at all goes
/// wrong. A photo without a profile must render exactly as it always has, and
/// "exactly" here means multiplied by the identity, not skipped.
pub fn rows_for(
    js_adjustments: &serde_json::Value,
    photo: Option<&str>,
) -> ([f32; 4], [f32; 4], [f32; 4]) {
    let m = chosen_correction(js_adjustments, photo).unwrap_or(IDENTITY);
    (
        [m[0][0], m[0][1], m[0][2], 0.0],
        [m[1][0], m[1][1], m[1][2], 0.0],
        [m[2][0], m[2][1], m[2][2], 0.0],
    )
}

fn chosen_correction(
    js_adjustments: &serde_json::Value,
    photo: Option<&str>,
) -> Option<[[f32; 3]; 3]> {
    let file = js_adjustments.get("cameraProfile")?.as_str()?.trim();
    if file.is_empty() {
        return None;
    }

    let library = super::profiles::library()?;
    let profile = super::profiles::load(library, file)?;
    let (_, _, forward) = super::decode::daylight_matrix(&profile)?;
    let xyz2cam = super::profile_matrix::xyz2cam_from_forward(&forward?)?;

    correction_for(photo, &xyz2cam)
}
