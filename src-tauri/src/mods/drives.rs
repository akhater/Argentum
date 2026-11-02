//! The disks the Modern layout's folder column lists under "This computer":
//! every local fixed or removable volume, by the path it is mounted at. Ours.
//!
//! Adapted from Shoebox, src-tauri/src/volumes.rs @ c34c8bc (`mounted`,
//! `query`, `quiet_errors`): the same walk over the volumes, keeping what the
//! folder column shows (name, free and total space) and leaving out what
//! Shoebox's catalog needs (serial numbers, file systems).
//!
//! Network drives are left out on Windows: asking an offline share for its name
//! can hang for seconds, and the column is read every time Argentum opens. For
//! the same reason macOS and Linux take their disks from a folder's listing and
//! never ask the mount behind an entry.

use serde::Serialize;

/// A disk as the folder column lists it.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Drive {
    /// Where it is mounted, as the folder tree writes paths: `E:\` on
    /// Windows, `/Volumes/Photos` on macOS.
    pub path: String,
    /// The volume's own name, empty when it has none.
    pub label: String,
    /// A memory card or USB stick rather than a fixed disk.
    pub removable: bool,
    /// Zero where this system does not report it.
    pub free_bytes: u64,
    /// Zero where this system does not report it.
    pub total_bytes: u64,
}

/// Every mounted local disk: drive letters first, in order, then disks
/// mounted in a folder.
pub fn list() -> Vec<Drive> {
    let mut drives = platform::list();
    drives.sort_by_key(|d| sort_key(&d.path));
    drives
}

/// Drive letters (`E:\`) before folder mounts (`C:\Mounts\Disk1\`), each
/// group alphabetical whatever the case.
fn sort_key(path: &str) -> (bool, String) {
    (path.len() > 3, path.to_lowercase())
}

/// One entry of a folder disks are mounted in, as its listing gives it.
#[cfg(any(not(windows), test))]
struct Listed {
    path: String,
    name: String,
    is_dir: bool,
    /// A link to `/`, which is how `/Volumes` names a Mac's system disk.
    is_system_link: bool,
}

/// The system disk at `/`, under the name its link gives it if it has one,
/// then every disk listed. The link itself is not a disk: through it, every
/// folder on the system disk would have a second path.
#[cfg(any(not(windows), test))]
fn disks_in(listed: &[Listed]) -> Vec<Drive> {
    let system_name = listed.iter().find(|l| l.is_system_link);
    let mut out = vec![Drive {
        path: "/".to_string(),
        label: system_name.map(|l| l.name.clone()).unwrap_or_default(),
        removable: false,
        free_bytes: 0,
        total_bytes: 0,
    }];
    out.extend(
        listed
            .iter()
            .filter(|l| l.is_dir && !l.name.starts_with('.'))
            .map(|l| Drive {
                path: l.path.clone(),
                label: l.name.clone(),
                removable: true,
                free_bytes: 0,
                total_bytes: 0,
            }),
    );
    out
}

#[cfg(windows)]
mod platform {
    use super::Drive;
    use windows::Win32::Storage::FileSystem::{
        FindFirstVolumeW, FindNextVolumeW, FindVolumeClose, GetDiskFreeSpaceExW, GetDriveTypeW,
        GetVolumeInformationW, GetVolumePathNamesForVolumeNameW,
    };
    use windows::Win32::System::Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SetErrorMode};
    use windows::core::PCWSTR;

    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;

    pub fn list() -> Vec<Drive> {
        // Without this an empty card reader pops "There is no disk in the
        // drive" while it is asked for its name.
        unsafe {
            SetErrorMode(SEM_FAILCRITICALERRORS);
        }
        let mut out = Vec::new();
        let mut name = vec![0u16; 64];
        let Ok(find) = (unsafe { FindFirstVolumeW(&mut name) }) else {
            return out;
        };
        loop {
            // `\\?\Volume{guid}\`
            let guid = wide(&from_wide(&name));
            let kind = unsafe { GetDriveTypeW(PCWSTR(guid.as_ptr())) };
            if kind == DRIVE_FIXED || kind == DRIVE_REMOVABLE {
                for mount in mounts(&guid) {
                    out.extend(query(&mount, kind == DRIVE_REMOVABLE));
                }
            }
            if unsafe { FindNextVolumeW(find, &mut name) }.is_err() {
                break;
            }
        }
        let _ = unsafe { FindVolumeClose(find) };
        out
    }

    /// Every place a volume is mounted: a drive letter, a folder, or both.
    fn mounts(guid: &[u16]) -> Vec<String> {
        let mut paths = vec![0u16; 2048];
        let mut len = 0u32;
        let listed = unsafe {
            GetVolumePathNamesForVolumeNameW(PCWSTR(guid.as_ptr()), Some(&mut paths), &mut len)
        };
        if listed.is_err() {
            return Vec::new();
        }
        paths[..(len as usize).min(paths.len())]
            .split(|&c| c == 0)
            .filter(|s| !s.is_empty())
            .map(String::from_utf16_lossy)
            .collect()
    }

    /// The disk at `mount`, or none when it has no medium in it.
    fn query(mount: &str, removable: bool) -> Option<Drive> {
        let w = wide(mount);
        let mut label = vec![0u16; 261];
        let mut free = 0u64;
        let mut total = 0u64;
        unsafe {
            GetVolumeInformationW(PCWSTR(w.as_ptr()), Some(&mut label), None, None, None, None)
                .ok()?;
            GetDiskFreeSpaceExW(
                PCWSTR(w.as_ptr()),
                Some(&mut free as *mut u64),
                Some(&mut total as *mut u64),
                None,
            )
            .ok()?;
        }
        Some(Drive {
            path: mount.to_string(),
            label: from_wide(&label),
            removable,
            free_bytes: free,
            total_bytes: total,
        })
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn from_wide(buf: &[u16]) -> String {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }
}

/// Elsewhere: the system disk, and whatever is mounted where macOS and Linux
/// put disks. Free space is not read here yet.
#[cfg(not(windows))]
mod platform {
    use super::{Drive, Listed, disks_in};
    use std::path::Path;

    pub fn list() -> Vec<Drive> {
        let user = std::env::var("USER").unwrap_or_default();
        let places: Vec<String> = if cfg!(target_os = "macos") {
            vec!["/Volumes".into()]
        } else {
            vec![
                format!("/media/{user}"),
                format!("/run/media/{user}"),
                "/mnt".into(),
            ]
        };
        let mut listed = Vec::new();
        for place in places {
            let Ok(entries) = std::fs::read_dir(&place) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                let path = entry.path();
                listed.push(Listed {
                    is_dir: kind.is_dir(),
                    is_system_link: kind.is_symlink()
                        && std::fs::read_link(&path).is_ok_and(|to| to == Path::new("/")),
                    path: path.to_string_lossy().into_owned(),
                    name: entry.file_name().to_string_lossy().into_owned(),
                });
            }
        }
        disks_in(&listed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(path: &str, is_dir: bool, is_system_link: bool) -> Listed {
        Listed {
            path: path.to_string(),
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            is_dir,
            is_system_link,
        }
    }

    #[test]
    fn a_macs_system_disk_is_at_its_root_not_behind_its_link() {
        let drives = disks_in(&[
            listed("/Volumes/Macintosh HD", false, true),
            listed("/Volumes/Trips", true, false),
        ]);
        let paths: Vec<&str> = drives.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, vec!["/", "/Volumes/Trips"]);
        assert_eq!(drives[0].label, "Macintosh HD");
        assert!(!drives[0].removable && drives[1].removable);
    }

    #[test]
    fn the_system_disk_is_listed_with_no_link_to_name_it() {
        let drives = disks_in(&[listed("/mnt/photos", true, false)]);
        assert_eq!(drives[0].path, "/");
        assert_eq!(drives[0].label, "");
        assert_eq!(drives[1].path, "/mnt/photos");
    }

    #[test]
    fn a_hidden_folder_or_another_link_is_not_a_disk() {
        let drives = disks_in(&[
            listed("/Volumes/.timemachine", true, false),
            listed("/Volumes/Shortcut", false, false),
        ]);
        assert_eq!(drives.len(), 1);
    }

    #[test]
    fn the_system_disk_comes_before_the_others() {
        let mut paths = vec!["/Volumes/Trips", "/"];
        paths.sort_by_key(|p| sort_key(p));
        assert_eq!(paths, vec!["/", "/Volumes/Trips"]);
    }

    #[test]
    fn drive_letters_come_first_and_in_order() {
        let mut paths = vec![r"C:\Mounts\Disk1\", r"e:\", r"C:\", r"D:\"];
        paths.sort_by_key(|p| sort_key(p));
        assert_eq!(paths, vec![r"C:\", r"D:\", r"e:\", r"C:\Mounts\Disk1\"]);
    }

    #[cfg(windows)]
    #[test]
    fn the_system_disk_is_listed_with_its_size() {
        let system = format!(
            "{}\\",
            std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into())
        );
        let drives = list();
        let found = drives.iter().find(|d| d.path.eq_ignore_ascii_case(&system));
        let found = found.unwrap_or_else(|| panic!("{system} not in {drives:?}"));
        assert!(!found.removable);
        assert!(found.total_bytes > 0 && found.free_bytes <= found.total_bytes);
    }

    #[test]
    fn crosses_with_camel_case_names() {
        let d = Drive {
            path: "E:\\".into(),
            label: "Photos".into(),
            removable: true,
            free_bytes: 1,
            total_bytes: 2,
        };
        let v = serde_json::to_value(d).expect("serialise");
        assert_eq!(v["freeBytes"], 1);
        assert_eq!(v["totalBytes"], 2);
        assert_eq!(v["removable"], true);
    }
}
