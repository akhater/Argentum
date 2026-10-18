//! Every type the Browse view sees, defined here and nowhere else.
//!
//! `src/shell/browse/browse.generated.ts` is generated from this file, with
//! one of each in `src/shell/browse/browse.fixtures.json` for both sides to
//! test against. The test at the bottom fails when either is stale; to write
//! them, in src-tauri:
//!
//!     AG_WRITE_CONTRACT=1 cargo test --lib mods::browse::contract

use serde::{Deserialize, Serialize};
#[cfg(test)]
use ts_rs::TS;

/// The long edge of a thumbnail, in pixels: a grid cell's size at 2x.
pub const THUMB_PX: u32 = 400;

/// A folder the tree shows.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(test, derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    /// Its own name, or a drive's, as the tree shows it.
    pub name: String,
    pub path: String,
    /// Whether it holds folders, so the tree offers to open it.
    pub has_folders: bool,
}

/// Where browsing starts: the computer's drives and folders, and the folders
/// remembered.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(test, derive(TS))]
pub struct Places {
    /// The drives on Windows; the root elsewhere.
    pub drives: Vec<Folder>,
    /// Pictures and the home folder, where there are such.
    pub standard: Vec<Folder>,
    /// Pinned folders, in the order they were pinned.
    pub favourites: Vec<String>,
    /// Folders opened, the latest first.
    pub recent: Vec<String>,
}

/// A photo in a folder: one stat of the file, and whether a sidecar sits
/// beside it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(test, derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct Photo {
    pub name: String,
    pub path: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes: u64,
    /// When the file was last written, in milliseconds since 1970.
    #[cfg_attr(test, ts(type = "number"))]
    pub modified_ms: u64,
    /// An Argentum sidecar (`.agdata`) is beside it. Never read here.
    pub edited: bool,
}

/// Names one listing of a folder. A newer listing stops every older one.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(test, derive(TS))]
pub struct ListingId(pub u32);

/// What went wrong, as a kind the interface can act on.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(TS))]
#[serde(rename_all = "camelCase")]
pub enum BrowseErrorKind {
    /// No such file or folder.
    NotFound,
    /// A file where a folder was asked for.
    NotAFolder,
    /// The system refused to let it be read.
    Denied,
    /// A photo with no picture that can be read without developing it.
    NoPreview,
    /// Any other failure to read or write.
    Io,
    /// A fault on this side, not of what was asked.
    Internal,
}

/// A refused call: what went wrong, and in words.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(test, derive(TS))]
pub struct BrowseError {
    pub kind: BrowseErrorKind,
    pub message: String,
}

/// What browsing tells the interface, as it happens.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(test, derive(TS))]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum BrowseEvent {
    /// Some of a folder's photos, in the order the disk gives them. The last
    /// part of a listing says it is `done`.
    Listed {
        listing: ListingId,
        photos: Vec<Photo>,
        done: bool,
    },
    /// A thumbnail is ready: `browse_thumb` hands over its bytes.
    Thumb { path: String },
    /// No thumbnail can be made for this photo.
    NoThumb { path: String, error: BrowseError },
}

impl BrowseError {
    pub fn new(kind: BrowseErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// An I/O failure on `path`, by its kind.
    pub fn io(path: &std::path::Path, e: &std::io::Error) -> Self {
        let kind = match e.kind() {
            std::io::ErrorKind::NotFound => BrowseErrorKind::NotFound,
            std::io::ErrorKind::PermissionDenied => BrowseErrorKind::Denied,
            std::io::ErrorKind::NotADirectory => BrowseErrorKind::NotAFolder,
            _ => BrowseErrorKind::Io,
        };
        Self::new(kind, format!("{}: {e}", path.display()))
    }
}

impl std::fmt::Display for BrowseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn browse_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/shell/browse")
    }

    fn decl<T: TS>() -> String {
        format!("{}export {}\n", T::DOCS.unwrap_or(""), T::decl())
    }

    fn typescript() -> String {
        let mut out = String::from(
            "// Generated from src-tauri/src/mods/browse/contract.rs by its test. Do not\n\
             // edit: change contract.rs, then run, in src-tauri,\n\
             // `AG_WRITE_CONTRACT=1 cargo test --lib mods::browse::contract`.\n\
             \n",
        );
        out += &format!(
            "/** The long edge of a thumbnail, in pixels: a grid cell's size at 2x. */\n\
             export const THUMB_PX = {THUMB_PX};\n\n"
        );
        for d in [
            decl::<Folder>(),
            decl::<Places>(),
            decl::<Photo>(),
            decl::<ListingId>(),
            decl::<BrowseErrorKind>(),
            decl::<BrowseError>(),
            decl::<BrowseEvent>(),
        ] {
            out += &d;
            out += "\n";
        }
        out.truncate(out.trim_end().len());
        out.push('\n');
        out
    }

    /// One of each, for both sides' tests.
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Fixtures {
        places: Places,
        folders: Vec<Folder>,
        events: Vec<BrowseEvent>,
        error: BrowseError,
    }

    fn fixtures() -> Fixtures {
        let photo = |name: &str, edited| Photo {
            name: name.into(),
            path: format!("C:/Photos/2026-10-09/{name}"),
            bytes: 31_457_280,
            modified_ms: 1_760_000_000_000,
            edited,
        };
        let folder = |name: &str, has_folders| Folder {
            name: name.into(),
            path: format!("C:/Photos/{name}"),
            has_folders,
        };
        let missing = BrowseError::new(BrowseErrorKind::NotFound, "C:/Photos/gone: not found");
        Fixtures {
            places: Places {
                drives: vec![Folder {
                    name: "C:".into(),
                    path: "C:/".into(),
                    has_folders: true,
                }],
                standard: vec![folder("Pictures", true)],
                favourites: vec!["C:/Photos/2026-10-09".into()],
                recent: vec!["C:/Photos/2026-10-09".into(), "C:/Photos/2026-09-26".into()],
            },
            folders: vec![folder("2026-09-26", false), folder("2026-10-09", true)],
            events: vec![
                BrowseEvent::Listed {
                    listing: ListingId(1),
                    photos: vec![photo("IMG_0001.CR3", true), photo("IMG_0002.CR3", false)],
                    done: false,
                },
                BrowseEvent::Listed {
                    listing: ListingId(1),
                    photos: vec![photo("IMG_0003.JPG", false)],
                    done: true,
                },
                BrowseEvent::Thumb {
                    path: "C:/Photos/2026-10-09/IMG_0001.CR3".into(),
                },
                BrowseEvent::NoThumb {
                    path: "C:/Photos/2026-10-09/IMG_0002.CR3".into(),
                    error: BrowseError::new(
                        BrowseErrorKind::NoPreview,
                        "IMG_0002.CR3: no embedded picture",
                    ),
                },
            ],
            error: missing,
        }
    }

    /// The interface's types and fixtures are what this file makes now.
    #[test]
    fn generated_typescript_and_fixtures_are_current() {
        let ts = typescript();
        let json = serde_json::to_string_pretty(&fixtures()).unwrap() + "\n";
        let ts_path = browse_dir().join("browse.generated.ts");
        let json_path = browse_dir().join("browse.fixtures.json");
        if std::env::var_os("AG_WRITE_CONTRACT").is_some() {
            std::fs::write(&ts_path, &ts).unwrap();
            std::fs::write(&json_path, &json).unwrap();
            return;
        }
        let stale = "is stale: run `AG_WRITE_CONTRACT=1 cargo test --lib mods::browse::contract`";
        let on_disk = |p: &PathBuf| {
            std::fs::read_to_string(p)
                .unwrap_or_default()
                .replace("\r\n", "\n")
        };
        assert_eq!(on_disk(&ts_path), ts, "{} {stale}", ts_path.display());
        assert_eq!(on_disk(&json_path), json, "{} {stale}", json_path.display());
    }

    /// The fixtures read back as the types they were written from.
    #[test]
    fn fixtures_read_back() {
        let json = serde_json::to_string(&fixtures()).unwrap();
        assert_eq!(serde_json::from_str::<Fixtures>(&json).unwrap(), fixtures());
    }

    #[test]
    fn io_errors_keep_their_kind() {
        let p = std::path::Path::new("x");
        let e = |k| BrowseError::io(p, &std::io::Error::from(k)).kind;
        assert_eq!(e(std::io::ErrorKind::NotFound), BrowseErrorKind::NotFound);
        assert_eq!(
            e(std::io::ErrorKind::PermissionDenied),
            BrowseErrorKind::Denied
        );
        assert_eq!(e(std::io::ErrorKind::Other), BrowseErrorKind::Io);
    }
}
