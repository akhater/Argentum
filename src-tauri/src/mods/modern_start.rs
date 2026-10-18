//! Where the Modern layout opens. Ours.
//!
//! No start screen: the window opens on the folder column, with the disks
//! under "This computer" and the folder last open selected if it is still
//! there. A first run, or a folder on a disk that is unplugged, opens on the
//! column with nothing selected.

use std::path::Path;

use serde::Serialize;

use super::drives::Drive;

/// What the interface needs to draw the folder column the moment it opens.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModernStart {
    pub drives: Vec<Drive>,
    /// Recent folders that are there now, newest first.
    pub recent: Vec<String>,
    /// The folder to open, or none.
    pub start_folder: Option<String>,
    /// The folders to show unfolded: those saved, plus every folder above
    /// `start_folder` up to its disk, so that it shows.
    pub expanded: Vec<String>,
}

/// The library's own name for an album in `currentFolderPath`.
const ALBUM_PREFIX: &str = "Album: ";

/// Decide it. `is_dir` is the only look at the disk, so the choice can be
/// tested without one.
pub fn plan(
    drives: Vec<Drive>,
    recent: Vec<String>,
    last_folder: Option<&str>,
    saved_expanded: &[String],
    is_dir: impl Fn(&str) -> bool,
) -> ModernStart {
    let start = last_folder
        .filter(|f| !f.starts_with(ALBUM_PREFIX) && is_dir(f))
        .and_then(|f| {
            drives
                .iter()
                .find(|d| is_within(f, &d.path))
                .map(|d| (f, d.path.as_str()))
        });

    let mut expanded: Vec<String> = Vec::new();
    let mut add = |p: String| {
        if !expanded.contains(&p) {
            expanded.push(p);
        }
    };
    saved_expanded.iter().cloned().for_each(&mut add);
    if let Some((folder, disk)) = start {
        above(folder, disk).into_iter().for_each(&mut add);
    }

    ModernStart {
        recent: recent.into_iter().filter(|f| is_dir(f)).collect(),
        start_folder: start.map(|(f, _)| f.to_string()),
        expanded,
        drives,
    }
}

/// `folder` is the disk `disk` or inside it.
fn is_within(folder: &str, disk: &str) -> bool {
    let disk = disk.trim_end_matches(['/', '\\']);
    let (Some(head), Some(rest)) = (folder.get(..disk.len()), folder.get(disk.len()..)) else {
        return false;
    };
    let starts = if cfg!(windows) {
        head.eq_ignore_ascii_case(disk)
    } else {
        head == disk
    };
    starts && matches!(rest.chars().next(), None | Some('/' | '\\'))
}

/// Every folder above `folder` within `disk`, nearest first, ending with the
/// disk written exactly as the folder tree has it.
fn above(folder: &str, disk: &str) -> Vec<String> {
    let disk_len = disk.trim_end_matches(['/', '\\']).len();
    let mut out: Vec<String> = Path::new(folder)
        .ancestors()
        .skip(1)
        .map(|p| p.to_string_lossy().into_owned())
        .take_while(|p| p.trim_end_matches(['/', '\\']).len() > disk_len)
        .collect();
    out.push(disk.to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk(path: &str) -> Drive {
        Drive {
            path: path.into(),
            label: String::new(),
            removable: false,
            free_bytes: 0,
            total_bytes: 0,
        }
    }

    const SEP: &str = std::path::MAIN_SEPARATOR_STR;

    /// A path on the disk `root` written the way this system writes them.
    fn on(root: &str, parts: &[&str]) -> String {
        format!("{root}{}", parts.join(SEP))
    }

    fn root() -> &'static str {
        if cfg!(windows) {
            r"E:\"
        } else {
            "/media/photos/"
        }
    }

    #[test]
    fn opens_the_last_folder_and_unfolds_everything_above_it() {
        let folder = on(root(), &["2024", "06 San Francisco", "Muir Woods"]);
        let start = plan(vec![disk(root())], vec![], Some(&folder), &[], |_| true);
        assert_eq!(start.start_folder.as_deref(), Some(folder.as_str()));
        assert_eq!(
            start.expanded,
            vec![
                on(root(), &["2024", "06 San Francisco"]),
                on(root(), &["2024"]),
                root().to_string()
            ],
        );
    }

    #[test]
    fn keeps_what_was_unfolded_without_repeating_it() {
        let folder = on(root(), &["2024", "Chicago"]);
        let saved = vec![on(root(), &["2024"]), on(root(), &["2023"])];
        let start = plan(vec![disk(root())], vec![], Some(&folder), &saved, |_| true);
        assert_eq!(
            start.expanded,
            vec![
                on(root(), &["2024"]),
                on(root(), &["2023"]),
                root().to_string()
            ]
        );
    }

    #[test]
    fn nothing_selected_on_a_first_run() {
        let start = plan(vec![disk(root())], vec![], None, &[], |_| true);
        assert_eq!(start.start_folder, None);
        assert!(start.expanded.is_empty());
    }

    #[test]
    fn nothing_selected_when_the_folder_has_gone() {
        let folder = on(root(), &["2024"]);
        let start = plan(vec![disk(root())], vec![], Some(&folder), &[], |_| false);
        assert_eq!(start.start_folder, None);
    }

    #[test]
    fn nothing_selected_for_an_album_or_a_folder_on_no_listed_disk() {
        let album = plan(vec![disk(root())], vec![], Some("Album: Trip"), &[], |_| {
            true
        });
        assert_eq!(album.start_folder, None);
        let elsewhere = if cfg!(windows) {
            r"Z:\photos"
        } else {
            "/elsewhere/photos"
        };
        let other = plan(vec![disk(root())], vec![], Some(elsewhere), &[], |_| true);
        assert_eq!(other.start_folder, None);
    }

    #[test]
    fn a_disk_name_that_only_starts_the_same_is_another_disk() {
        let (a, b) = if cfg!(windows) {
            (r"C:\Mounts\Disk1\", r"C:\Mounts\Disk10\photos")
        } else {
            ("/media/disk1/", "/media/disk10/photos")
        };
        assert!(!is_within(b, a));
    }

    #[test]
    fn recent_folders_that_are_gone_are_left_out() {
        let here = on(root(), &["here"]);
        let gone = on(root(), &["gone"]);
        let start = plan(
            vec![disk(root())],
            vec![here.clone(), gone.clone()],
            None,
            &[],
            |f| f == here,
        );
        assert_eq!(start.recent, vec![here]);
    }

    #[cfg(windows)]
    #[test]
    fn drive_letters_match_whatever_their_case() {
        let start = plan(vec![disk(r"E:\")], vec![], Some(r"e:\2024"), &[], |_| true);
        assert_eq!(start.start_folder.as_deref(), Some(r"e:\2024"));
        assert_eq!(start.expanded, vec![r"E:\".to_string()]);
    }
}
