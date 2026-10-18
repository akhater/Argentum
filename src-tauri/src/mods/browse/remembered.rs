//! The only things browsing remembers: favourite folders and recent ones,
//! two keys in Argentum's own preferences file (`ag_settings.rs`). No
//! catalog, no ratings, no last photo (DEC-30).

use std::path::Path;
use std::sync::Mutex;

use serde_json::Value;

use super::contract::{BrowseError, BrowseErrorKind};
use crate::mods::ag_settings;

const FAVOURITES: &str = "browseFavourites";
const RECENT: &str = "browseRecent";
/// Recent folders kept, at most.
pub const RECENT_FOLDERS: usize = 12;

/// Each change reads the list, changes it and writes it back: one at a time.
static CHANGING: Mutex<()> = Mutex::new(());

fn read(prefs: &Path, key: &str) -> Vec<String> {
    ag_settings::get(prefs, key)
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

fn change(
    prefs: &Path,
    key: &str,
    edit: impl FnOnce(&mut Vec<String>),
) -> Result<Vec<String>, BrowseError> {
    let _one_at_a_time = CHANGING.lock().unwrap_or_else(|p| p.into_inner());
    let mut list = read(prefs, key);
    edit(&mut list);
    ag_settings::set(prefs, key, Value::from(list.clone()))
        .map_err(|e| BrowseError::new(BrowseErrorKind::Io, e))?;
    Ok(list)
}

/// Pinned folders, in the order they were pinned.
pub fn favourites(prefs: &Path) -> Vec<String> {
    read(prefs, FAVOURITES)
}

/// Pin `folder`, or unpin it; the favourites after.
pub fn set_favourite(prefs: &Path, folder: &str, on: bool) -> Result<Vec<String>, BrowseError> {
    change(prefs, FAVOURITES, |list| {
        list.retain(|f| f != folder);
        if on {
            list.push(folder.to_string());
        }
    })
}

/// Folders opened, the latest first.
pub fn recent(prefs: &Path) -> Vec<String> {
    read(prefs, RECENT)
}

/// Put `folder` first among the recent ones; the list after.
pub fn add_recent(prefs: &Path, folder: &str) -> Result<Vec<String>, BrowseError> {
    change(prefs, RECENT, |list| {
        list.retain(|f| f != folder);
        list.insert(0, folder.to_string());
        list.truncate(RECENT_FOLDERS);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favourites_pin_unpin_and_survive_a_restart() {
        let prefs = tempfile::tempdir().unwrap();
        assert!(favourites(prefs.path()).is_empty());
        set_favourite(prefs.path(), "C:/A", true).unwrap();
        set_favourite(prefs.path(), "C:/B", true).unwrap();
        // Pinning twice keeps one.
        assert_eq!(
            set_favourite(prefs.path(), "C:/A", true).unwrap(),
            ["C:/B", "C:/A"]
        );
        assert_eq!(
            set_favourite(prefs.path(), "C:/B", false).unwrap(),
            ["C:/A"]
        );
        assert_eq!(favourites(prefs.path()), ["C:/A"]);
    }

    #[test]
    fn recent_folders_are_latest_first_without_repeats_and_bounded() {
        let prefs = tempfile::tempdir().unwrap();
        for i in 0..(RECENT_FOLDERS + 5) {
            add_recent(prefs.path(), &format!("C:/{i}")).unwrap();
        }
        let got = add_recent(prefs.path(), "C:/3").unwrap();
        assert_eq!(got.len(), RECENT_FOLDERS);
        assert_eq!(got[0], "C:/3");
        assert_eq!(got[1], format!("C:/{}", RECENT_FOLDERS + 4));
        assert_eq!(got.iter().filter(|f| *f == "C:/3").count(), 1);
        assert_eq!(recent(prefs.path()), got);
    }

    #[test]
    fn remembering_leaves_the_other_preferences_alone() {
        let prefs = tempfile::tempdir().unwrap();
        ag_settings::set(prefs.path(), "tiffBitDepth", Value::from(8)).unwrap();
        add_recent(prefs.path(), "C:/A").unwrap();
        set_favourite(prefs.path(), "C:/A", true).unwrap();
        assert_eq!(
            ag_settings::get(prefs.path(), "tiffBitDepth"),
            Some(Value::from(8))
        );
    }
}
