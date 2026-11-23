//! A path that must stay inside a library folder. Ours.
//!
//! A command that deletes a library file by a path from the interface first
//! resolves it here: both sides are made canonical (so `..` and links are
//! followed), then compared component by component, never as strings.

use std::path::{Path, PathBuf};

/// The canonical path of `target` when it lies inside `root`, else an error.
/// `target` must exist; a link that leads out of `root` is refused.
pub fn inside(root: &Path, target: &Path) -> Result<PathBuf, String> {
    let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let target = std::fs::canonicalize(target).map_err(|e| e.to_string())?;
    if target != root && target.starts_with(&root) {
        Ok(target)
    } else {
        Err("Access denied: Cannot remove files outside the user LUT directory".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("luts");
        let outside = dir.path().join("luts-other");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        (dir, root, outside)
    }

    #[test]
    fn a_file_in_the_library_resolves() {
        let (_d, root, _) = library();
        let lut = root.join("a.cube");
        std::fs::write(&lut, b"x").unwrap();
        assert_eq!(
            inside(&root, &lut).unwrap(),
            std::fs::canonicalize(&lut).unwrap()
        );
        let sub = root.join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("b.cube"), b"x").unwrap();
        assert!(inside(&root, &sub.join("b.cube")).is_ok());
    }

    #[test]
    fn a_file_outside_the_library_is_refused() {
        let (d, root, outside) = library();
        let other = outside.join("a.cube");
        std::fs::write(&other, b"x").unwrap();
        // Shares a string prefix with the library folder, not a component.
        assert!(inside(&root, &other).is_err());
        let around = root.join("..").join("luts-other").join("a.cube");
        assert!(inside(&root, &around).is_err());
        assert!(inside(&root, &root).is_err());
        assert!(inside(&root, &d.path().join("missing.cube")).is_err());
    }

    #[test]
    fn a_link_leading_out_is_refused() {
        let (_d, root, outside) = library();
        let other = outside.join("a.cube");
        std::fs::write(&other, b"x").unwrap();
        let link = root.join("link.cube");
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&other, &link).is_ok();
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&other, &link).is_ok();
        if made {
            assert!(inside(&root, &link).is_err());
        }
        // A junction needs no privilege on Windows.
        #[cfg(windows)]
        {
            let junction = root.join("junction");
            let ok = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&junction)
                .arg(&outside)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if ok {
                assert!(inside(&root, &junction.join("a.cube")).is_err());
                let _ = std::fs::remove_dir(&junction);
            }
        }
    }
}
