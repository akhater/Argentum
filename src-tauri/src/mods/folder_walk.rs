//! What the folder column may read without being asked. Ours.
//!
//! Opening a folder is the user's act: that read may ask for permission, fetch
//! from the cloud or wait for a disk, because they asked for it. Looking one
//! level further, for a folder's arrow and its photo count, is ours, so it
//! happens only where it can do none of those. macOS answers a plain read of
//! Desktop, Music or the Photos library with a permission prompt, whatever the
//! app meant by it, and a read on a mount that has gone away can hang for good.
//!
//! RapidRAW's tree (`src-tauri/src/file_management.rs` `has_subdirs`,
//! @ 79c2a46b) looks inside every folder it lists, which was safe when its
//! roots were folders added by hand; Argentum's roots are whole disks.
//! RawTherapee (`rtgui/dirbrowser.cc`, `addDir`) and darktable
//! (`src/libs/import.c`, `_get_folders_list`) never look, and give every folder
//! an arrow. This keeps the arrows and counts wherever looking is free, which
//! neither has: the rule below is ours.
//!
//! The decision takes only what listing the folder above already gave, so it
//! reads nothing itself, and `System` is a value so every system's rule can be
//! tested on any of them.

use std::fs::Metadata;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum System {
    Windows,
    Mac,
    Linux,
}

pub const THIS_SYSTEM: System = if cfg!(windows) {
    System::Windows
} else if cfg!(target_os = "macos") {
    System::Mac
} else {
    System::Linux
};

/// What may be done with a folder that was listed but not opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// Not a folder to the person using it: not listed, never read.
    Hide,
    /// Listed, and reading it for its arrow and count costs nothing.
    Peek,
    /// Listed and left unread until it is opened: it may have an arrow it does
    /// not need, and it has no count.
    Leave,
}

/// Folders on a Mac that are one thing in Finder and hold another app's data.
/// One missing here only shows as a folder, and is read only if it is opened.
const PACKAGES: [&str; 12] = [
    "aplibrary",
    "app",
    "bundle",
    "fcpbundle",
    "framework",
    "imovielibrary",
    "kext",
    "musiclibrary",
    "photolibrary",
    "photoslibrary",
    "plugin",
    "tvlibrary",
];

fn is_package(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PACKAGES.iter().any(|p| e.eq_ignore_ascii_case(p)))
}

/// `folder` holds folders that a read may ask about or wait on: a home folder
/// on a Mac, whatever Apple protects in it this year, and the folders disks
/// are mounted in, whose entries may be network shares that are gone. A home
/// folder is matched by its place, so it is found through a disk's own mount
/// point as well.
fn guards_its_folders(system: System, folder: &Path) -> bool {
    let above = folder.parent().unwrap_or(folder);
    match system {
        System::Windows => false,
        System::Mac => folder.ends_with("Volumes") || above.ends_with("Users"),
        System::Linux => {
            folder == Path::new("/mnt")
                || above == Path::new("/media")
                || above == Path::new("/run/media")
        }
    }
}

/// What may be done with the folder `name`, listed inside `parent`.
pub fn look_at(system: System, parent: &Path, name: &str, online_only: bool) -> Look {
    if system == System::Mac && is_package(name) {
        Look::Hide
    } else if online_only || guards_its_folders(system, parent) {
        Look::Leave
    } else {
        Look::Peek
    }
}

/// Windows file attributes that say the contents are not on this computer.
#[cfg(any(windows, test))]
fn attributes_say_online_only(attributes: u32) -> bool {
    const FILE_ATTRIBUTE_OFFLINE: u32 = 0x0000_1000;
    const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x0004_0000;
    const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
    attributes
        & (FILE_ATTRIBUTE_OFFLINE
            | FILE_ATTRIBUTE_RECALL_ON_OPEN
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
        != 0
}

/// A file or folder whose contents are not on this computer, so reading it
/// would fetch them. On Windows `meta` holds the attributes, so it answers
/// without reading anything; a listing has them without opening the folder.
#[cfg(windows)]
pub fn is_online_only(_path: &Path, meta: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    attributes_say_online_only(meta.file_attributes())
}

#[cfg(not(windows))]
pub fn is_online_only(path: &Path, _meta: &Metadata) -> bool {
    crate::file_management::is_cloud_placeholder(path)
}

/// A walk through everything below a folder, which someone asked for, goes
/// everywhere but into what is not a folder to them. It does not stop at what
/// a listing leaves unread: an export from it would silently lose photos.
pub fn walks_into(system: System, entry: &walkdir::DirEntry) -> bool {
    entry.depth() == 0
        || !entry.file_type().is_dir()
        || !(system == System::Mac && is_package(&entry.file_name().to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_management::{FolderNode, scan_dir_lazy};
    use std::collections::HashSet;
    use std::fs;
    use std::path::PathBuf;
    use walkdir::WalkDir;

    fn look(system: System, parent: &str, name: &str) -> Look {
        look_at(system, Path::new(parent), name, false)
    }

    #[test]
    fn a_photos_library_is_not_a_folder_on_a_mac() {
        let pictures = "/Users/me/Pictures";
        let library = "Photos Library.photoslibrary";
        assert_eq!(look(System::Mac, pictures, library), Look::Hide);
        assert_eq!(look(System::Windows, pictures, library), Look::Peek);
    }

    #[test]
    fn music_and_tv_libraries_and_apps_are_not_folders_on_a_mac() {
        for name in ["Music Library.musiclibrary", "TV.TVLIBRARY", "Argentum.app"] {
            assert_eq!(look(System::Mac, "/Users/me/x", name), Look::Hide, "{name}");
        }
    }

    #[test]
    fn a_folder_with_a_dot_in_its_name_is_still_a_folder() {
        assert_eq!(look(System::Mac, "/Users/me/x", "2024.06 Trip"), Look::Peek);
    }

    #[test]
    fn a_folder_in_a_home_folder_is_left_alone_on_a_mac() {
        for parent in ["/Users/me", "/Volumes/Macintosh HD/Users/me"] {
            for name in ["Desktop", "Music", "Pictures", "Something Apple adds"] {
                assert_eq!(look(System::Mac, parent, name), Look::Leave, "{name}");
            }
        }
    }

    #[test]
    fn a_folder_below_one_in_a_home_folder_is_peeked() {
        assert_eq!(look(System::Mac, "/Users/me/Pictures", "2026"), Look::Peek);
        assert_eq!(look(System::Mac, "/Users", "me"), Look::Peek);
    }

    #[test]
    fn a_disk_found_by_browsing_is_left_alone() {
        assert_eq!(look(System::Mac, "/Volumes", "NAS"), Look::Leave);
        assert_eq!(look(System::Linux, "/mnt", "nas"), Look::Leave);
        assert_eq!(look(System::Linux, "/media/me", "card"), Look::Leave);
        assert_eq!(look(System::Linux, "/run/media/me", "card"), Look::Leave);
    }

    #[test]
    fn folders_elsewhere_are_peeked_on_linux_and_windows() {
        assert_eq!(look(System::Linux, "/home/me", "Pictures"), Look::Peek);
        assert_eq!(
            look(System::Windows, r"C:\Users\me", "Pictures"),
            Look::Peek
        );
    }

    #[test]
    fn an_online_only_folder_is_left_alone_on_every_system() {
        for system in [System::Windows, System::Mac, System::Linux] {
            let look = look_at(system, Path::new("/data/photos"), "2026", true);
            assert_eq!(look, Look::Leave, "{system:?}");
        }
    }

    #[test]
    fn each_attribute_of_an_online_only_file_says_so() {
        for attribute in [0x0000_1000, 0x0004_0000, 0x0040_0000] {
            assert!(attributes_say_online_only(attribute), "{attribute:#x}");
        }
    }

    #[test]
    fn a_onedrive_listing_reads_as_it_should() {
        const ONLINE_ONLY: u32 = 0x0040_1620;
        const DOWNLOADED: u32 = 0x0000_0420;
        const HIDDEN_SYSTEM_FILE: u32 = 0x0018_0026;
        assert!(attributes_say_online_only(ONLINE_ONLY));
        assert!(!attributes_say_online_only(DOWNLOADED));
        assert!(!attributes_say_online_only(HIDDEN_SYSTEM_FILE));
    }

    #[cfg(windows)]
    #[test]
    fn a_folder_on_this_disk_is_not_online_only() {
        let dir = tempfile::tempdir().unwrap();
        let meta = fs::metadata(dir.path()).unwrap();
        assert!(!is_online_only(dir.path(), &meta));
    }

    /// A Mac's home folder on a disk, with the folders inside empty unless a
    /// test fills them: a row that says it has subfolders when it has none
    /// proves nothing looked.
    fn a_home_folder() -> (tempfile::TempDir, PathBuf) {
        let disk = tempfile::tempdir().unwrap();
        let home = disk.path().join("Users").join("me");
        fs::create_dir_all(home.join("Desktop")).unwrap();
        fs::create_dir_all(home.join("Music")).unwrap();
        fs::create_dir_all(
            home.join("Pictures")
                .join("Photos Library.photoslibrary")
                .join("originals"),
        )
        .unwrap();
        (disk, home)
    }

    /// The rows of an opened folder, as unfolding it reads them.
    fn rows(folder: &Path, expanded: &[&str]) -> Vec<FolderNode> {
        let expanded: HashSet<&str> = expanded.iter().copied().collect();
        scan_dir_lazy(folder, &expanded, false, false, System::Mac)
            .unwrap()
            .0
    }

    /// The rows of a root at start, which reads one level ahead.
    fn rows_at_start(root: &Path, expanded: &[&str]) -> Vec<FolderNode> {
        let expanded: HashSet<&str> = expanded.iter().copied().collect();
        scan_dir_lazy(root, &expanded, false, true, System::Mac)
            .unwrap()
            .0
    }

    fn row<'a>(rows: &'a [FolderNode], name: &str) -> &'a FolderNode {
        rows.iter().find(|r| r.name == name).unwrap()
    }

    #[test]
    fn opening_home_on_a_mac_reads_nothing_inside_it() {
        let (_disk, home) = a_home_folder();
        let rows = rows(&home, &[]);
        let desktop = row(&rows, "Desktop");
        assert!(
            desktop.unread && desktop.has_subdirs,
            "the empty Desktop was read"
        );
        assert!(row(&rows, "Pictures").unread);
    }

    #[test]
    fn opening_a_folder_in_home_reads_it_and_never_a_library() {
        let (_disk, home) = a_home_folder();
        let pictures = rows(&home.join("Pictures"), &[]);
        assert!(pictures.is_empty(), "{pictures:?}");
    }

    #[test]
    fn starting_with_home_unfolded_reads_home_and_nothing_below() {
        let (disk, home) = a_home_folder();
        let users = disk.path().join("Users");
        let (users_str, home_str) = (users.to_string_lossy(), home.to_string_lossy());
        let at_disk = rows_at_start(disk.path(), &[&users_str, &home_str]);
        let users_row = row(&at_disk, "Users");
        let home_row = row(&users_row.children, "me");
        assert!(home_row.children.iter().all(|c| c.unread));
        assert!(home_row.children.iter().all(|c| c.children.is_empty()));
    }

    #[test]
    fn a_folder_that_was_opened_is_read_at_the_next_start() {
        let (_disk, home) = a_home_folder();
        let desktop = home.join("Desktop");
        fs::create_dir(desktop.join("shoot")).unwrap();
        let rows = rows_at_start(&home, &[&desktop.to_string_lossy()]);
        let desktop = row(&rows, "Desktop");
        assert!(!desktop.unread);
        assert_eq!(desktop.children.len(), 1);
    }

    #[test]
    fn a_read_ahead_goes_where_looking_is_free_and_no_further() {
        let (_disk, home) = a_home_folder();
        fs::create_dir_all(home.join("Pictures").join("2026").join("June")).unwrap();
        let in_pictures = rows_at_start(&home.join("Pictures"), &[]);
        let year = row(&in_pictures, "2026");
        assert!(!year.unread);
        assert_eq!(year.children.len(), 1);

        let in_home = rows_at_start(&home, &[]);
        assert!(row(&in_home, "Pictures").children.is_empty());
    }

    #[test]
    fn a_walk_goes_everywhere_but_into_a_library() {
        let (_disk, home) = a_home_folder();
        fs::write(home.join("Desktop").join("a.jpg"), b"").unwrap();
        let library = home.join("Pictures").join("Photos Library.photoslibrary");
        fs::write(library.join("originals").join("b.jpg"), b"").unwrap();

        let walked = |from: &Path| -> Vec<String> {
            WalkDir::new(from)
                .into_iter()
                .filter_entry(|e| walks_into(System::Mac, e))
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(walked(&home), vec!["a.jpg"]);
        assert!(walked(&home.join("Pictures")).is_empty());
    }
}
