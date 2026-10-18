//! Where the tree starts: the disks, and the folders every computer has.

use std::path::PathBuf;

use super::contract::Folder;
use super::listing;

/// The disks plugged in, by drive letter (Shoebox's list, `sb_volumes.rs`),
/// named as Windows names them: `EOS_DIGITAL (D:)`, or `C:` with no label.
/// The root elsewhere. Not looked inside: a disk is offered to open, and
/// opening says.
pub fn drives() -> Vec<Folder> {
    #[cfg(windows)]
    {
        static QUIET: std::sync::Once = std::sync::Once::new();
        QUIET.call_once(super::sb_volumes::quiet_errors);
        let mut found: Vec<Folder> = super::sb_volumes::mounted()
            .into_iter()
            // A disk mounted in a folder is reached through that folder.
            .filter(|v| v.mount.len() <= 3)
            .map(|v| {
                let letter = v.mount.trim_end_matches('\\');
                let label = v.label.trim();
                Folder {
                    name: if label.is_empty() {
                        letter.to_string()
                    } else {
                        format!("{label} ({letter})")
                    },
                    path: v.mount.clone(),
                    has_folders: true,
                }
            })
            .collect();
        found.sort_by(|a, b| a.path.cmp(&b.path));
        found
    }
    #[cfg(not(windows))]
    {
        vec![Folder {
            name: "/".into(),
            path: "/".into(),
            has_folders: true,
        }]
    }
}

/// Pictures, then home, of those that exist.
pub fn standard(pictures: Option<PathBuf>, home: Option<PathBuf>) -> Vec<Folder> {
    [pictures, home]
        .into_iter()
        .flatten()
        .filter(|p| p.is_dir())
        .map(|p| listing::folder(&p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn there_is_always_somewhere_to_start() {
        let drives = drives();
        assert!(!drives.is_empty());
        assert!(drives.iter().all(|d| Path::new(&d.path).is_dir()));
    }

    #[test]
    fn standard_places_that_do_not_exist_are_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let got = standard(
            Some(dir.path().join("gone")),
            Some(dir.path().to_path_buf()),
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].path, dir.path().to_string_lossy());
    }
}
