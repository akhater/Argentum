//! Where the tree starts: the drives, and the folders every computer has.

use std::path::{Path, PathBuf};

use super::contract::Folder;
use super::listing;

/// The drives with something in them, as `C:`, `D:`; the root elsewhere.
/// Not looked inside: a drive is offered to open, and opening says.
pub fn drives() -> Vec<Folder> {
    #[cfg(windows)]
    {
        ('A'..='Z')
            .map(|letter| format!("{letter}:\\"))
            .filter(|root| Path::new(root).exists())
            .map(|root| Folder {
                name: root.trim_end_matches('\\').to_string(),
                path: root,
                has_folders: true,
            })
            .collect()
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
