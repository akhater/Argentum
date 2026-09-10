//! Which camera profile one photo asked for.
//!
//! WHY THE CHOICE IS PER PHOTO
//!
//! I argued for per camera, on the grounds that a profile is a calibration —
//! a measurement of how one body responds to light — and calibrations are
//! properties of equipment, not of pictures. That is true of the physics and
//! wrong for the product. Profiles differ in *look* as well as accuracy, only
//! one applies at a time, and the thing a person actually wants to do is try
//! one on the photo in front of them and see. A setting you cannot compare on
//! the picture you care about is not much of a setting.
//!
//! WHERE IT LIVES
//!
//! In the photo's own adjustments, so it is saved to the sidecar, undone,
//! copied and pasted like every other adjustment, with no machinery of ours to
//! keep in step. The decode reads that sidecar directly, because it runs long
//! before the frontend has an opinion about anything.
//!
//! Absent means the built-in matrix. Every photo ever edited is in that state,
//! and it has to keep rendering exactly as it did.

use std::path::Path;

/// The key in the photo's adjustments. Written by `src/argentum/CameraProfile.tsx`.
const KEY: &str = "cameraProfile";

/// Read the profile file name this photo asked for, if any.
///
/// Silent on every failure — no sidecar, unreadable, not JSON, key absent, key
/// not a string. All of those mean "no profile chosen", which is the normal
/// case for almost every photo, and none of them is worth failing a decode.
pub fn chosen_for(photo_path: &str) -> Option<String> {
    let sidecar = sidecar_of(photo_path)?;
    let text = std::fs::read_to_string(sidecar).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;

    let chosen = json
        .get("adjustments")
        .and_then(|a| a.get(KEY))
        .or_else(|| json.get(KEY))?
        .as_str()?
        .trim();

    (!chosen.is_empty()).then(|| chosen.to_string())
}

/// `photo.CR2` -> `photo.CR2.agdata`, the way file_management writes it.
fn sidecar_of(photo_path: &str) -> Option<std::path::PathBuf> {
    let path = Path::new(photo_path);
    let mut name = path.file_name()?.to_os_string();
    name.push(".agdata");
    Some(path.with_file_name(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("argentum-photo-profile-{label}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    fn photo_with(dir: &Path, sidecar: Option<&str>) -> String {
        let photo = dir.join("shot.CR2");
        std::fs::write(&photo, b"not really a raw").expect("photo");
        if let Some(text) = sidecar {
            std::fs::write(dir.join("shot.CR2.agdata"), text).expect("sidecar");
        }
        photo.to_string_lossy().to_string()
    }

    #[test]
    fn it_reads_the_choice_out_of_the_adjustments() {
        let dir = scratch("nested");
        let photo = photo_with(
            &dir,
            Some(r#"{"adjustments":{"exposure":1,"cameraProfile":"Canon EOS 5D Mark II.dcp"}}"#),
        );
        assert_eq!(chosen_for(&photo).as_deref(), Some("Canon EOS 5D Mark II.dcp"));
    }

    /// Some sidecars hold the adjustments at the top level.
    #[test]
    fn it_also_reads_a_flat_sidecar() {
        let dir = scratch("flat");
        let photo = photo_with(&dir, Some(r#"{"cameraProfile":"Faithful.dcp"}"#));
        assert_eq!(chosen_for(&photo).as_deref(), Some("Faithful.dcp"));
    }

    /// The normal case, and the one that must never change how a photo renders.
    #[test]
    fn no_sidecar_means_no_profile() {
        let dir = scratch("none");
        let photo = photo_with(&dir, None);
        assert_eq!(chosen_for(&photo), None);
    }

    #[test]
    fn an_absent_or_empty_key_means_no_profile() {
        let dir = scratch("absent");
        let photo = photo_with(&dir, Some(r#"{"adjustments":{"exposure":1}}"#));
        assert_eq!(chosen_for(&photo), None);

        std::fs::write(dir.join("shot.CR2.agdata"), r#"{"adjustments":{"cameraProfile":"  "}}"#)
            .expect("sidecar");
        assert_eq!(chosen_for(&photo), None);
    }

    /// A corrupt sidecar must not stop a photo opening.
    #[test]
    fn rubbish_is_not_an_error() {
        let dir = scratch("rubbish");
        let photo = photo_with(&dir, Some("this is not json"));
        assert_eq!(chosen_for(&photo), None);

        std::fs::write(dir.join("shot.CR2.agdata"), r#"{"adjustments":{"cameraProfile":42}}"#)
            .expect("sidecar");
        assert_eq!(chosen_for(&photo), None);
    }
}

#[cfg(test)]
mod end_to_end {
    use super::*;

    /// Does choosing a profile actually change the pixels?
    ///
    /// Everything else about this feature has been verified a piece at a time —
    /// the parser, the matching, the download, the derivation — and it was
    /// still possible for the whole to do nothing, because the pieces are
    /// joined by a sidecar file and a decode parameter that no unit test
    /// crosses. This crosses them: same photo, same call, one sidecar, and the
    /// rendered bytes have to differ.
    #[test]
    #[ignore = "reads a photo and the installed library; run by hand"]
    fn choosing_a_profile_changes_the_render() {
        let raw = std::env::var("AG_RAW").expect("set AG_RAW");
        let library = std::env::var("AG_PROFILES").expect("set AG_PROFILES");
        let file = std::env::var("AG_DCP_FILE").expect("set AG_DCP_FILE to a file in the library");

        crate::mods::profiles::set_library(std::path::PathBuf::from(&library));

        // A copy, so the real photo's sidecar is never touched.
        let dir = std::env::temp_dir().join("argentum-profile-e2e");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        let copy = dir.join("shot.CR2");
        std::fs::copy(&raw, &copy).expect("copy raw");
        let copy_str = copy.to_string_lossy().to_string();

        let develop = || {
            crate::raw_processing::develop_raw_image(
                &std::fs::read(&copy).expect("read"),
                false,
                2.5,
                "off".to_string(),
                None,
                Some(&copy_str),
            )
            .expect("develop")
            .to_rgb8()
        };

        let without = develop();
        assert_eq!(chosen_for(&copy_str), None, "a fresh copy must have no choice");

        std::fs::write(
            dir.join("shot.CR2.agdata"),
            format!(r#"{{"adjustments":{{"cameraProfile":"{file}"}}}}"#),
        )
        .expect("sidecar");
        assert_eq!(chosen_for(&copy_str).as_deref(), Some(file.as_str()), "the sidecar was not read");

        let with = develop();

        let changed = without
            .pixels()
            .zip(with.pixels())
            .filter(|(a, b)| a != b)
            .count();
        let total = without.pixels().len();
        println!(
            "pixels changed by the profile: {changed} of {total} ({:.1}%)",
            changed as f64 / total as f64 * 100.0
        );
        assert!(changed > total / 100, "the profile changed nothing");
    }
}
