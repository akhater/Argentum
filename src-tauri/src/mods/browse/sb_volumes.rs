//! sb_: copied from Shoebox, `src-tauri/src/volumes.rs` @ 4ae3a3a (the photo
//! library built beside Argentum): the disks plugged in, as the tree's roots.
//! Only what Browse needs is taken - the list of mounted volumes, each with
//! its mount point and label, and the guard against "no disk in the drive"
//! dialogs - not the serial, file system and size by which Shoebox
//! recognises a disk whatever its letter. Windows only, as in Shoebox; the
//! tree starts at the root elsewhere.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountedVolume {
    /// Mount point, e.g. `D:\`.
    pub mount: String,
    pub label: String,
}

/// Stops Windows from popping "There is no disk in the drive" dialogs when an
/// empty card reader is queried. Call once at startup.
pub fn quiet_errors() {
    unsafe {
        use windows::Win32::System::Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SetErrorMode};
        SetErrorMode(SEM_FAILCRITICALERRORS);
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Every local fixed and removable volume, once per place it is mounted: a
/// drive letter (`D:\`) or a folder (`C:\Mounts\Disk1\`). A disk
/// mounted both ways appears twice, letters first; it is the same disk.
pub fn mounted() -> Vec<MountedVolume> {
    use windows::Win32::Storage::FileSystem::{
        FindFirstVolumeW, FindNextVolumeW, FindVolumeClose, GetDriveTypeW,
        GetVolumePathNamesForVolumeNameW,
    };
    use windows::core::PCWSTR;

    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;

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
            let mut paths = vec![0u16; 2048];
            let mut len = 0u32;
            let listed = unsafe {
                GetVolumePathNamesForVolumeNameW(PCWSTR(guid.as_ptr()), Some(&mut paths), &mut len)
            };
            if listed.is_ok() {
                let mut mounts: Vec<String> = paths[..(len as usize).min(paths.len())]
                    .split(|&c| c == 0)
                    .filter(|s| !s.is_empty())
                    .map(String::from_utf16_lossy)
                    .collect();
                mounts.sort_by_key(|m| m.len() > 3); // drive letters first
                out.extend(mounts.iter().filter_map(|m| query(m)));
            }
        }
        if unsafe { FindNextVolumeW(find, &mut name) }.is_err() {
            break;
        }
    }
    let _ = unsafe { FindVolumeClose(find) };
    out
}

fn query(mount: &str) -> Option<MountedVolume> {
    use windows::Win32::Storage::FileSystem::GetVolumeInformationW;
    use windows::core::PCWSTR;

    let w = wide(mount);
    let mut label = vec![0u16; 261];
    unsafe {
        GetVolumeInformationW(PCWSTR(w.as_ptr()), Some(&mut label), None, None, None, None).ok()?;
    }
    Some(MountedVolume {
        mount: mount.to_string(),
        label: from_wide(&label),
    })
}
