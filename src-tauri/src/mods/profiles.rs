//! The camera profile library: where `.dcp` files live, and which one a photo gets.
//!
//! WHY IMPORTED RATHER THAN BUNDLED
//!
//! The obvious move is to ship a folder of profiles so everything works out of
//! the box. It is not available to us. RawTherapee's collection — the best free
//! one — is bundled there with the individual authors' permission, not under a
//! blanket licence, so redistributing it from this project would be assuming a
//! right nobody granted. Adobe's, which come with the free DNG Converter, are
//! Adobe's.
//!
//! So Argentum reads profiles and does not ship them. The user points at a file
//! once; it is copied into the library and matched automatically from then on.
//! That also happens to be the only design that works for cameras nobody here
//! owns, which is most of them.
//!
//! MATCHING
//!
//! A profile names the body it was made for in `UniqueCameraModel`, and the
//! photo names its own in EXIF. Both are messy in the same ways — "Canon EOS 5D
//! Mark II" against "Canon Canon EOS 5D Mark II" against "EOS 5D Mark II" — so
//! comparison is done on a normalised form rather than exactly. A wrong match
//! is worse than none, so it is the *whole* normalised name that has to agree.
//!
//! The filename is used only as a fallback, for profiles that leave
//! `UniqueCameraModel` empty. What the file is called is not evidence.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::dcp::{self, Profile};

/// A profile in the library, with where it came from.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Installed {
    /// File name in the library, which is also its id.
    pub file: String,
    /// The profile's own name, if it has one.
    pub name: Option<String>,
    /// The body it is for, as matched against.
    pub camera: Option<String>,
}

/// Lower-case, alphanumeric only, and the maker's name not repeated.
///
/// Canon writes "Canon EOS 5D Mark II" in one place and "Canon Canon EOS 5D
/// Mark II" in another; Nikon writes "NIKON CORPORATION NIKON D750". Stripping
/// punctuation and spaces handles most of it, and collapsing a doubled first
/// word handles the rest.
fn normalise(model: &str) -> String {
    let lower = model.trim().to_ascii_lowercase();

    // Company words carry no information about which body this is, and makers
    // are inconsistent about including them: "NIKON CORPORATION NIKON D750" in
    // EXIF against "NIKON D750" in a profile.
    const FILLER: [&str; 7] = ["corporation", "corp", "company", "co", "ltd", "inc", "co."];
    let mut words: Vec<&str> = lower
        .split_whitespace()
        .filter(|w| !FILLER.contains(w))
        .collect();

    // "canon canon eos 5d" -> "canon eos 5d". A loop, not a single check:
    // removing a filler word can leave a fresh duplicate behind, which is how
    // the Nikon case slipped through the first version of this.
    while words.len() >= 2 && words[0] == words[1] {
        words.remove(0);
    }

    words
        .join("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

/// Do two names refer to the same body?
///
/// Whole normalised names, never a prefix: EOS 7D must not match EOS 7D Mark II.
pub fn same_model(a: &str, b: &str) -> bool {
    let (a, b) = (normalise(a), normalise(b));
    !a.is_empty() && a == b
}

/// Does this profile belong to this camera?
///
/// The two sides disagree about the maker and there is no fixing that at the
/// source: rawler reports the model alone — "EOS 5D Mark II" — while a profile
/// is named for the whole camera, "Canon EOS 5D Mark II.dcp". Comparing the
/// model alone found nothing, which is what made "Find one" report that a
/// profile it was looking straight at did not exist.
///
/// So both readings are tried, and each still has to match in full. Prefixes
/// are no more acceptable here than anywhere else: a profile for the wrong body
/// renders worse than none and looks like nothing is wrong.
pub fn matches(profile_camera: &str, make: &str, model: &str) -> bool {
    if same_model(profile_camera, model) {
        return true;
    }

    // The maker in front of the model, when we know it.
    if !make.trim().is_empty()
        && same_model(profile_camera, &format!("{} {}", make.trim(), model.trim()))
    {
        return true;
    }

    // The maker stripped off the profile's name, when we do not.
    //
    // This is the reading that matters in practice. The maker is only known
    // once a photo from that body has been opened *since the field existed*,
    // and a gear list written before that has none — which is how "Find one"
    // failed a second time after the first fix. Depending on data that may not
    // be there is the bug; this does not.
    //
    // Exactly one leading word, and the remainder must still match in full, so
    // it cannot turn into prefix matching: "Canon EOS 7D" with its first word
    // removed is "EOS 7D", which is still not "EOS 7D Mark II".
    let without_maker: String = profile_camera
        .trim()
        .split_whitespace()
        .skip(1)
        .collect::<Vec<_>>()
        .join(" ");
    !without_maker.is_empty() && same_model(&without_maker, model)
}

/// Where the library lives, once startup has resolved it.
///
/// A global because the decode path has no app handle to ask — it is handed a
/// decoded image and a file, and has to answer "is there a profile for this?"
/// on its own. Set once, never changed.
static LIBRARY: OnceLock<PathBuf> = OnceLock::new();

pub fn set_library(dir: PathBuf) {
    let _ = LIBRARY.set(dir);
}

/// The library path, or `None` before startup has run — which is the case in
/// tests and in the offline measurement harness.
pub fn library() -> Option<&'static Path> {
    LIBRARY.get().map(|p| p.as_path())
}

/// The library directory, created if it is not there yet.
pub fn library_dir(app_data: &Path) -> std::io::Result<PathBuf> {
    let dir = app_data.join("camera-profiles");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Every readable profile in the library.
///
/// A file that will not parse is skipped rather than failing the listing: one
/// bad download should not hide the rest.
pub fn installed(library: &Path) -> Vec<Installed> {
    let Ok(entries) = std::fs::read_dir(library) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dcp")) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(profile) = dcp::parse(&bytes) else { continue };
        out.push(Installed {
            file: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
            name: profile.name,
            camera: profile.camera.or_else(|| stem_as_camera(&path)),
        });
    }
    out.sort_by(|a, b| a.file.cmp(&b.file));
    out
}

/// A profile's file name, for profiles that name no camera inside.
fn stem_as_camera(path: &Path) -> Option<String> {
    path.file_stem().map(|s| s.to_string_lossy().to_string())
}

/// Load one profile by file name.
pub fn load(library: &Path, file: &str) -> Option<Profile> {
    let name = Path::new(file).file_name()?;
    let bytes = std::fs::read(library.join(name)).ok()?;
    dcp::parse(&bytes).ok()
}

/// Every profile in the library that fits this body, by file name.
pub fn matching(library: &Path, make: &str, model: &str) -> Vec<Installed> {
    installed(library)
        .into_iter()
        .filter(|i| {
            let claimed = i.camera.clone().unwrap_or_default();
            matches(&claimed, make, model)
        })
        .collect()
}



/// Copy a profile into the library.
///
/// Parsed before it is copied, so an unusable file is refused at the point the
/// user can still do something about it rather than silently ignored later.
pub fn import(library: &Path, source: &Path) -> Result<Installed, String> {
    let bytes = std::fs::read(source).map_err(|e| format!("could not read that file: {e}"))?;
    let profile = dcp::parse(&bytes).map_err(|e| format!("not a usable camera profile: {e}"))?;

    let file = source
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "profile.dcp".to_string());

    std::fs::create_dir_all(library).map_err(|e| e.to_string())?;

    // A camera can have several profiles, so importing adds one. Overwriting a
    // different profile because it happened to share a file name would lose
    // work — re-importing the *same* file is still just a no-op rewrite.
    let file = free_name(library, &file, &bytes);
    std::fs::write(library.join(&file), &bytes)
        .map_err(|e| format!("could not save it: {e}"))?;

    Ok(Installed {
        camera: profile.camera.clone().or_else(|| stem_as_camera(source)),
        name: profile.name,
        file,
    })
}

/// Remove a profile from the library, by file name.
///
/// The name is treated as a name, not a path: a caller cannot reach outside
/// the library with it.
pub fn remove(library: &Path, file: &str) -> Result<(), String> {
    let name = Path::new(file)
        .file_name()
        .ok_or_else(|| "no such profile".to_string())?;
    std::fs::remove_file(library.join(name)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The case that shipped broken: rawler reports the model on its own,
    /// "EOS 5D Mark II", while the published profile is named for the whole
    /// camera, "Canon EOS 5D Mark II.dcp". Matching on the model alone found
    /// nothing, so "Find one" reported that a profile it was looking straight
    /// at did not exist.
    #[test]
    fn a_profile_named_for_the_whole_camera_matches_a_bare_model() {
        assert!(matches("Canon EOS 5D Mark II", "Canon", "EOS 5D Mark II"));
        assert!(matches("Nikon D750", "NIKON CORPORATION", "D750"));
    }

    /// And with no maker known at all, which is the state a gear list written
    /// before the maker was recorded is in. This is what made the first fix
    /// look like it had done nothing.
    #[test]
    fn it_matches_even_when_the_maker_is_unknown() {
        assert!(matches("Canon EOS 5D Mark II", "", "EOS 5D Mark II"));
        assert!(matches("Nikon D750", "", "D750"));
        assert!(matches("SONY ILCE-7M3", "", "ILCE-7M3"));
    }

    /// Stripping the maker must not become prefix matching.
    #[test]
    fn stripping_the_maker_still_requires_the_whole_model() {
        assert!(!matches("Canon EOS 7D", "", "EOS 7D Mark II"));
        assert!(!matches("Canon EOS 7D Mark II", "", "EOS 7D"));
        assert!(!matches("Canon EOS R", "", "EOS R5"));
        assert!(!matches("Canon EOS R5", "", "EOS R"));
    }

    #[test]
    fn the_same_camera_written_differently_still_matches() {
        assert!(matches("Canon EOS 5D Mark II", "Canon", "Canon EOS 5D Mark II"));
        assert!(matches("Canon EOS 5D Mark II", "Canon", "Canon Canon EOS 5D Mark II"));
        assert!(matches("canon eos 5d mark ii", "Canon", "EOS 5D Mark II"));
        assert!(matches("NIKON CORPORATION NIKON D750", "NIKON", "D750"));
    }

    /// The important half. A profile for the wrong body renders worse than no
    /// profile at all, and nothing about it looks like an error.
    #[test]
    fn different_cameras_never_match() {
        assert!(!matches("Canon EOS 5D Mark II", "Canon", "EOS 5D Mark III"));
        assert!(!matches("Canon EOS 5D", "Canon", "EOS 5D Mark II"));
        assert!(!matches("Canon EOS 5D Mark II", "Nikon", "D750"));
        assert!(!matches("", "Canon", "EOS 5D Mark II"));
        assert!(!matches("Canon EOS 5D Mark II", "Canon", ""));
    }

    /// A model number that is a prefix of another must not match it — this is
    /// the failure the whole-name rule exists to prevent.
    /// Adding the maker as a second reading must not weaken this: a profile for
    /// the wrong body renders worse than none, and nothing about it looks wrong.
    #[test]
    fn a_prefix_is_not_a_match() {
        assert!(!matches("Canon EOS 7D", "Canon", "EOS 7D Mark II"));
        assert!(!matches("Canon EOS 7D Mark II", "Canon", "EOS 7D"));
        assert!(!matches("Canon EOS R", "Canon", "EOS R5"));
        assert!(!matches("Canon EOS R5", "Canon", "EOS R"));
    }

    #[test]
    fn an_empty_library_yields_nothing() {
        let dir = std::env::temp_dir().join("argentum-profiles-empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        assert!(installed(&dir).is_empty());
        assert!(matching(&dir, "Canon", "EOS 5D Mark II").is_empty());
    }

    #[test]
    fn a_missing_library_is_not_an_error() {
        let dir = std::env::temp_dir().join("argentum-profiles-absent");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(installed(&dir).is_empty());
        assert!(matching(&dir, "Canon", "EOS 5D Mark II").is_empty());
    }

    #[test]
    fn importing_something_that_is_not_a_profile_says_so() {
        let dir = std::env::temp_dir().join("argentum-profiles-bad");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        let junk = dir.join("junk.dcp");
        std::fs::write(&junk, b"this is not a profile").expect("write");

        let err = import(&dir, &junk).expect_err("must refuse");
        assert!(err.contains("not a usable camera profile"), "{err}");
    }

    /// A profile name cannot be used to delete something elsewhere.
    #[test]
    fn removing_cannot_escape_the_library() {
        let dir = std::env::temp_dir().join("argentum-profiles-escape");
        let outside = std::env::temp_dir().join("argentum-must-survive.txt");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        std::fs::write(&outside, b"keep me").expect("write");

        let _ = remove(&dir, "../argentum-must-survive.txt");
        assert!(outside.exists(), "a path escaped the library");
    }
}

/// What the UI needs to show for one photo: which camera, and whether the
/// library has a profile for it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Status {
    /// The body, as the file reports it. `None` if the file cannot be read.
    pub camera: Option<String>,
    /// The maker, for display.
    pub make: Option<String>,
    /// Every profile the library holds for this body.
    pub available: Vec<Installed>,
}

/// Read the camera maker and model from a photo without decoding its pixels.
///
/// Metadata only — a status line must not cost a RAW decode, because it is
/// asked for every time a photo is opened.
pub fn camera_of(path: &Path) -> Option<(String, String)> {
    let source = rawler::rawsource::RawSource::new(path).ok()?;
    let decoder = rawler::get_decoder(&source).ok()?;
    let metadata = decoder
        .raw_metadata(&source, &rawler::decoders::RawDecodeParams::default())
        .ok()?;
    (!metadata.model.is_empty()).then_some((metadata.make, metadata.model))
}

/// Camera and matched profile for one photo, remembering the body as it goes.
///
/// The write is deliberate rather than a side effect that slipped in: opening a
/// photo is the only moment the app reliably learns which cameras the user
/// actually owns, and the gear list would otherwise stay empty until they went
/// looking for it.
pub fn status_for(library: &Path, path: &Path) -> Status {
    let found = camera_of(path);
    if let Some((make, model)) = found.as_ref() {
        remember_camera(library, make, model);
    }

    let available = found
        .as_ref()
        .map(|(make, model)| matching(library, make, model))
        .unwrap_or_default();

    Status {
        make: found.as_ref().map(|(make, _)| make.clone()),
        camera: found.map(|(_, model)| model),
        available,
    }
}

// ---------------------------------------------------------------------------
// Cameras seen
//
// The lens list ("My Lenses") is RapidRAW's, stored in their settings, and a
// detected lens is added to it. Cameras have no equivalent, so this keeps one:
// every body a photo has been opened from, so the gear list can show which of
// them still needs a profile.
//
// Kept in our own file rather than their settings because it is bookkeeping for
// a feature that is entirely ours, and because it belongs beside the profiles
// it is about.
// ---------------------------------------------------------------------------

/// A camera the user has actually opened a photo from.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Camera {
    /// The maker, shown the way My Lenses shows a lens maker.
    #[serde(default)]
    pub make: String,
    pub model: String,
}

fn cameras_file(library: &Path) -> PathBuf {
    library.join("cameras.json")
}

fn read_cameras(library: &Path) -> Vec<Camera> {
    std::fs::read_to_string(cameras_file(library))
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<Camera>>(&t).ok())
        .unwrap_or_default()
}

/// Record a body, if it is not already known. Returns true when it was new.
///
/// Best-effort: failing to remember a camera must never interfere with opening
/// a photo, so a write error is dropped rather than surfaced.
pub fn remember_camera(library: &Path, make: &str, model: &str) -> bool {
    let (make, model) = (make.trim(), model.trim());
    if model.is_empty() {
        return false;
    }

    let mut known = read_cameras(library);
    if let Some(existing) = known.iter_mut().find(|c| same_model(&c.model, model)) {
        // An older list has no maker; fill it in rather than leaving it blank.
        if existing.make.is_empty() && !make.is_empty() {
            existing.make = make.to_string();
            if let Ok(text) = serde_json::to_string_pretty(&known) {
                let _ = std::fs::write(cameras_file(library), text);
            }
        }
        return false;
    }

    known.push(Camera { make: make.to_string(), model: model.to_string() });
    known.sort_by(|a, b| a.model.cmp(&b.model));

    if let Ok(text) = serde_json::to_string_pretty(&known) {
        let _ = std::fs::create_dir_all(library);
        let _ = std::fs::write(cameras_file(library), text);
    }
    true
}

/// Every remembered camera, each with the profile it would use.
pub fn cameras(library: &Path) -> Vec<Camera> {
    read_cameras(library)
}


/// Forget a camera. The profile, if any, is left alone.
pub fn forget_camera(library: &Path, model: &str) {
    let kept: Vec<Camera> = read_cameras(library)
        .into_iter()
        .filter(|c| !same_model(&c.model, model))
        .collect();
    if let Ok(text) = serde_json::to_string_pretty(&kept) {
        let _ = std::fs::write(cameras_file(library), text);
    }
}


#[cfg(test)]
mod real_file_tests {
    use super::*;

    /// The maker has to actually come back, or the gear list shows a blank line
    /// where the brand should be — which is exactly what it did.
    #[test]
    #[ignore = "reads a local photo; run by hand with AG_RAW"]
    fn a_real_raw_reports_its_maker_and_model() {
        let Ok(raw) = std::env::var("AG_RAW") else {
            println!("set AG_RAW to a photo to run this");
            return;
        };
        let found = camera_of(Path::new(&raw)).expect("should read the camera");
        println!("make {:?}  model {:?}", found.0, found.1);
        assert!(!found.0.trim().is_empty(), "no maker came back");
        assert!(!found.1.trim().is_empty(), "no model came back");
    }
}

/// Fill in a camera's maker if it is blank, from a name that carries it.
///
/// A gear list written before the maker was recorded shows a blank line where
/// the brand should be, and the only thing that fixes it is opening a photo
/// from that body again. A profile is named for the whole camera, so installing
/// one tells us the maker without waiting for that.
pub fn learn_make_from(library: &Path, model: &str, full_name: &str) {
    let Some(first) = full_name.split_whitespace().next() else {
        return;
    };
    // Only when the rest of the name is the model, or the first word is
    // something else entirely and would be wrong.
    let rest: String = full_name.split_whitespace().skip(1).collect::<Vec<_>>().join(" ");
    if !same_model(&rest, model) {
        return;
    }

    let mut known = read_cameras(library);
    let mut touched = false;
    for camera in known.iter_mut() {
        if same_model(&camera.model, model) && camera.make.is_empty() {
            camera.make = first.to_string();
            touched = true;
        }
    }
    if touched && let Ok(text) = serde_json::to_string_pretty(&known) {
        let _ = std::fs::write(cameras_file(library), text);
    }
}

#[cfg(test)]
mod learn_make_tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("argentum-learn-make-{label}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    #[test]
    fn a_profile_name_supplies_a_missing_maker() {
        let dir = scratch("fill");
        std::fs::write(dir.join("cameras.json"), r#"[{"model":"EOS 5D Mark II"}]"#).expect("write");

        learn_make_from(&dir, "EOS 5D Mark II", "Canon EOS 5D Mark II");

        assert_eq!(cameras(&dir)[0].make, "Canon");
    }

    /// A maker already recorded from the photo is the better source; a file
    /// name must not overwrite it.
    #[test]
    fn it_never_overwrites_a_known_maker() {
        let dir = scratch("keep");
        remember_camera(&dir, "Canon", "EOS 5D Mark II");

        learn_make_from(&dir, "EOS 5D Mark II", "Nonsense EOS 5D Mark II");

        assert_eq!(cameras(&dir)[0].make, "Canon");
    }

    /// A name whose remainder is not the model says nothing about the maker.
    #[test]
    fn an_unrelated_name_teaches_nothing() {
        let dir = scratch("unrelated");
        std::fs::write(dir.join("cameras.json"), r#"[{"model":"EOS 5D Mark II"}]"#).expect("write");

        learn_make_from(&dir, "EOS 5D Mark II", "Some Other Camera");

        assert_eq!(cameras(&dir)[0].make, "");
    }
}


/// A name that does not overwrite a different profile.
///
/// The same bytes under the same name is the same profile, so that keeps its
/// name. Different bytes get a numbered neighbour rather than replacing it.
fn free_name(library: &Path, wanted: &str, bytes: &[u8]) -> String {
    let taken = |name: &str| library.join(name).exists();
    if !taken(wanted) {
        return wanted.to_string();
    }
    if std::fs::read(library.join(wanted)).is_ok_and(|existing| existing == bytes) {
        return wanted.to_string();
    }

    let (stem, ext) = wanted
        .rsplit_once('.')
        .map_or((wanted, "dcp"), |(s, e)| (s, e));
    for n in 2..100 {
        let candidate = format!("{stem} ({n}).{ext}");
        if !taken(&candidate) {
            return candidate;
        }
    }
    wanted.to_string()
}

#[cfg(test)]
mod import_naming_tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("argentum-import-name-{label}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    #[test]
    fn a_fresh_name_is_used_as_is() {
        let dir = scratch("fresh");
        assert_eq!(free_name(&dir, "Canon EOS 5D Mark II.dcp", b"x"), "Canon EOS 5D Mark II.dcp");
    }

    /// Re-importing the same file must not litter the library with copies.
    #[test]
    fn the_same_profile_keeps_its_name() {
        let dir = scratch("same");
        std::fs::write(dir.join("p.dcp"), b"same").expect("write");
        assert_eq!(free_name(&dir, "p.dcp", b"same"), "p.dcp");
    }

    /// A different profile under the same name must not replace it.
    #[test]
    fn a_different_profile_gets_its_own_name() {
        let dir = scratch("different");
        std::fs::write(dir.join("p.dcp"), b"first").expect("write");
        assert_eq!(free_name(&dir, "p.dcp", b"second"), "p (2).dcp");

        std::fs::write(dir.join("p (2).dcp"), b"second").expect("write");
        assert_eq!(free_name(&dir, "p.dcp", b"third"), "p (3).dcp");
    }
}
