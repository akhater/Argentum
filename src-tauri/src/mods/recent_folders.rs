//! The folders opened most recently, newest first, for the Recent list in the
//! Modern layout's folder column. Ours.
//!
//! Kept in argentum-processing.json through `ag_settings`, beside the editor
//! layout. Only paths are kept; what a folder holds is read from disk when it
//! is opened, as for any other folder.

use std::path::Path;

use serde_json::Value;

use super::ag_settings;

const KEY: &str = "recentFolders";

/// How many the list keeps.
pub const MAX_RECENT: usize = 6;

/// The list as saved, newest first, whether or not each folder is there now.
pub fn stored(library: &Path) -> Vec<String> {
    ag_settings::get(library, KEY)
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

/// Put `folder` at the top, dropping any older entry for the same folder and
/// whatever falls past `MAX_RECENT`. Returns the list as saved.
pub fn add(library: &Path, folder: &str) -> Result<Vec<String>, String> {
    let mut list = stored(library);
    list.retain(|f| !same_folder(f, folder));
    list.insert(0, folder.to_string());
    list.truncate(MAX_RECENT);
    ag_settings::set(library, KEY, Value::from(list.clone()))?;
    Ok(list)
}

/// The same folder written two ways: a trailing separator, and on Windows
/// the case of the letters and the direction of the slashes, do not count.
fn same_folder(a: &str, b: &str) -> bool {
    let trim = |s: &str| s.trim_end_matches(['/', '\\']).to_string();
    if cfg!(windows) {
        trim(a)
            .replace('/', "\\")
            .eq_ignore_ascii_case(&trim(b).replace('/', "\\"))
    } else {
        trim(a) == trim(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("argentum-recent-folders-{label}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    #[test]
    fn empty_until_a_folder_is_opened() {
        assert!(stored(&scratch("empty")).is_empty());
    }

    #[test]
    fn newest_first_and_each_folder_once() {
        let dir = scratch("order");
        add(&dir, "/photos/a").expect("add");
        add(&dir, "/photos/b").expect("add");
        let list = add(&dir, "/photos/a/").expect("add");
        assert_eq!(list, vec!["/photos/a/", "/photos/b"]);
        assert_eq!(stored(&dir), list);
    }

    #[test]
    fn keeps_only_the_most_recent() {
        let dir = scratch("cap");
        for i in 0..MAX_RECENT + 3 {
            add(&dir, &format!("/photos/{i}")).expect("add");
        }
        let list = stored(&dir);
        assert_eq!(list.len(), MAX_RECENT);
        assert_eq!(list[0], format!("/photos/{}", MAX_RECENT + 2));
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_match_whatever_their_case_and_slashes() {
        let dir = scratch("windows");
        add(&dir, r"E:\2024\Muir Woods").expect("add");
        let list = add(&dir, "e:/2024/muir woods").expect("add");
        assert_eq!(list, vec!["e:/2024/muir woods"]);
    }

    #[test]
    fn saving_leaves_the_other_preferences_alone() {
        let dir = scratch("neighbours");
        ag_settings::set(&dir, "editorLayout", Value::from("modern")).expect("set");
        add(&dir, "/photos/a").expect("add");
        assert_eq!(
            ag_settings::get(&dir, "editorLayout"),
            Some(Value::from("modern"))
        );
    }
}
