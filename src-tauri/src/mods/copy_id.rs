//! The shape of a virtual copy's id. Ours.
//!
//! The app makes an id of six lowercase hex digits (`a1b2c3`) and reads
//! sidecar names back by the same rule (`parse_sidecar_name`), so no other
//! id can belong to a real copy. The id becomes part of a file name, so a
//! string from the interface is checked before it is used as one.

/// What stands in a file name for an id that is not valid: it names a file
/// beside the photo that no copy owns, and never one elsewhere.
pub const INVALID_STAND_IN: &str = "invalid";

/// Whether `id` is the shape the app generates.
pub fn is_valid(id: &str) -> bool {
    id.len() == 6 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A virtual path's photo and copy id, or an error when the path names a
/// copy whose id is not valid.
pub fn checked_split(virtual_path: &str) -> Result<(&str, Option<&str>), String> {
    let (photo, id) = crate::file_management::split_virtual_path(virtual_path);
    match id {
        Some(id) if !is_valid(id) => Err("Invalid virtual copy id".to_string()),
        _ => Ok((photo, id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_management::{VIRTUAL_COPY, parse_sidecar_name, sidecar_for};
    use std::path::Path;

    #[test]
    fn the_ids_the_app_makes_are_valid() {
        for id in ["a1b2c3", "000000", "ffffff", "abc123"] {
            assert!(is_valid(id), "{id}");
        }
        for _ in 0..200 {
            let id = uuid::Uuid::new_v4().to_string()[..6].to_string();
            assert!(is_valid(&id), "{id}");
        }
    }

    #[test]
    fn every_id_in_the_compat_fixtures_is_valid() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/compat");
        for entry in walkdir::WalkDir::new(root).into_iter().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some((_, Some(id))) = parse_sidecar_name(&name) {
                assert!(is_valid(id), "{name}");
            }
            if let Ok(text) = std::fs::read_to_string(entry.path()) {
                for part in text.split(VIRTUAL_COPY).skip(1) {
                    let id: String = part
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric())
                        .collect();
                    assert!(is_valid(&id), "{id}");
                }
            }
        }
    }

    #[test]
    fn other_shapes_are_refused() {
        let long = "a".repeat(300);
        for id in [
            "", "..", "../x", "a/b123", "a\\b123", "/etc/x", "C:\\x", "a1b2c", "a1b2c3d", "A1B2C3",
            "a1b2c.", "a1 2c3", "é1b2c3", &long, "a1b2c3\0",
        ] {
            assert!(!is_valid(id), "{id:?}");
        }
    }

    #[test]
    fn an_invalid_id_names_no_file_outside_the_photos_folder() {
        let photo = Path::new("/p/IMG.CR2");
        for id in ["../x", "..", "a/b", "a\\b", "/abs/path", ""] {
            let sidecar = sidecar_for(photo, Some(id));
            assert_eq!(sidecar.parent(), photo.parent(), "{id:?}");
            assert_ne!(sidecar, sidecar_for(photo, None), "{id:?}");
        }
        assert_eq!(
            sidecar_for(photo, Some("a1b2c3")),
            Path::new("/p/IMG.CR2.a1b2c3.agdata")
        );
    }

    #[test]
    fn a_checked_split_refuses_an_invalid_copy_and_keeps_the_rest() {
        assert_eq!(
            checked_split("/p/IMG.CR2?vc=a1b2c3"),
            Ok(("/p/IMG.CR2", Some("a1b2c3")))
        );
        assert_eq!(checked_split("/p/IMG.CR2"), Ok(("/p/IMG.CR2", None)));
        for bad in [
            "/p/IMG.CR2?vc=../x",
            "/p/IMG.CR2?vc=",
            "/p/IMG.CR2?vc=/abs",
            "/p/IMG.CR2?vc=a1b2c3d",
        ] {
            assert!(checked_split(bad).is_err(), "{bad}");
        }
    }
}
