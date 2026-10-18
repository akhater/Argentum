//! Browsing folders of photos, for the new interface's Browse view
//! (`src/shell/browse/`). A file browser, not a library (DEC-30): no
//! catalog, no ratings, nothing about a photo kept but its thumbnail.
//! Favourite and recent folders are all it remembers.
//!
//! The interface calls it through the one `ag` command (`dispatch.rs`):
//!
//! | `ag` name           | What                                                         |
//! |---------------------|--------------------------------------------------------------|
//! | `browse_places`     | drives, Pictures and home, favourite and recent folders      |
//! | `browse_folders`    | a folder's folders, for the tree                             |
//! | `browse_list`       | a `ListingId` at once; the photos follow as `listed` events  |
//! | `browse_want`       | the thumbnails wanted, the visible first; replaces the last  |
//! | `browse_thumb`      | a thumbnail's JPEG bytes once its `thumb` event has come     |
//! | `browse_favourite`  | pin or unpin a folder; the favourites after                  |
//! | `browse_add_recent` | a folder opened; the recent folders after                    |
//!
//! It answers through one Tauri event, `ag-browse`, carrying each
//! `BrowseEvent` as `contract.rs` writes it.
//!
//! NOTHING HERE WRITES BESIDE A PHOTO. It reads folders and files, and writes
//! only its own: thumbnails in the app's cache folder and the two lists in
//! Argentum's preferences file.

#[cfg(test)]
mod budgets;
pub mod cache;
pub mod contract;
pub mod embedded;
pub mod listing;
pub mod places;
pub mod queue;
pub mod remembered;
pub mod thumbnail;
pub mod thumbnails;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, Manager};

use cache::{DISK_BUDGET_BYTES, DiskCache, MEMORY_BUDGET_BYTES, MemoryCache};
use contract::{BrowseError, BrowseErrorKind, BrowseEvent, ListingId, Places, THUMB_PX};
use listing::Listings;
use thumbnails::Thumbnails;

/// The Tauri event browse events go out as.
pub const EVENT: &str = "ag-browse";

/// Where on disk thumbnails are kept, under the app's cache folder.
const CACHE_FOLDER: &str = "browse-thumbnails";

pub struct Browse {
    listings: Listings,
    thumbnails: Arc<Thumbnails>,
    emit: Arc<dyn Fn(&BrowseEvent) + Send + Sync>,
}

/// Threads making thumbnails: all but one of the cores, within reason.
fn workers() -> usize {
    std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .saturating_sub(1)
        .clamp(2, 8)
}

impl Browse {
    /// Browsing with its thumbnails kept in `cache_dir`, or in memory only,
    /// and its events handed to `emit`.
    pub fn new(
        cache_dir: Option<PathBuf>,
        emit: impl Fn(&BrowseEvent) + Send + Sync + 'static,
    ) -> Result<Self, BrowseError> {
        let thumbnails = Arc::new(Thumbnails::new(
            THUMB_PX,
            MemoryCache::new(MEMORY_BUDGET_BYTES),
            cache_dir.map(|d| DiskCache::new(d, DISK_BUDGET_BYTES)),
        ));
        let emit: Arc<dyn Fn(&BrowseEvent) + Send + Sync> = Arc::new(emit);
        thumbnails
            .start(workers(), emit.clone())
            .map_err(|e| BrowseError::new(BrowseErrorKind::Internal, e.to_string()))?;
        Ok(Self {
            listings: Listings::default(),
            thumbnails,
            emit,
        })
    }

    /// List `dir`'s photos: refused at once if it cannot be read, else its id
    /// now and its photos as `listed` events from another thread.
    pub fn list(&self, dir: &Path) -> Result<ListingId, BrowseError> {
        let listing = self.listings.start(dir)?;
        let id = listing.id;
        let emit = self.emit.clone();
        std::thread::Builder::new()
            .name("ag-browse-list".into())
            .spawn(move || {
                listing.stream(|photos, done| {
                    emit(&BrowseEvent::Listed {
                        listing: id,
                        photos,
                        done,
                    })
                })
            })
            .map_err(|e| BrowseError::new(BrowseErrorKind::Internal, e.to_string()))?;
        Ok(id)
    }

    pub fn thumbnails(&self) -> &Thumbnails {
        &self.thumbnails
    }
}

/// The app's one `Browse`, made the first time the interface asks.
fn browse(app: &AppHandle) -> Result<&'static Browse, BrowseError> {
    static BROWSE: OnceLock<Result<Browse, BrowseError>> = OnceLock::new();
    BROWSE
        .get_or_init(|| {
            let cache = app
                .path()
                .app_cache_dir()
                .ok()
                .map(|d| d.join(CACHE_FOLDER));
            let app = app.clone();
            Browse::new(cache, move |event| {
                if let Err(e) = app.emit(EVENT, event) {
                    log::warn!("[browse] event not sent: {e}");
                }
            })
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Argentum's preferences folder, where the remembered folders are kept.
fn prefs() -> Result<&'static Path, BrowseError> {
    crate::mods::profiles::library().ok_or_else(|| {
        BrowseError::new(
            BrowseErrorKind::Internal,
            "the preferences folder is not ready yet",
        )
    })
}

fn internal(e: impl ToString) -> BrowseError {
    BrowseError::new(BrowseErrorKind::Internal, e.to_string())
}

/// Arguments are sent as the frontend writes them, so camelCase throughout.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PathArgs {
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PathsArgs {
    paths: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FavouriteArgs {
    path: String,
    on: bool,
}

fn args_for<T: for<'de> Deserialize<'de>>(
    name: &str,
    args: serde_json::Value,
) -> Result<T, BrowseError> {
    serde_json::from_value(args).map_err(|e| internal(format!("{name}: bad arguments: {e}")))
}

fn json(value: impl Serialize) -> Result<Response, BrowseError> {
    serde_json::to_string(&value)
        .map(Response::new)
        .map_err(internal)
}

/// Off the async runtime: anything that waits on a disk.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, BrowseError> + Send + 'static,
) -> Result<T, BrowseError> {
    tokio::task::spawn_blocking(work).await.map_err(internal)?
}

/// Every `browse_` command; `dispatch.rs` routes them here.
pub async fn call(
    name: &str,
    args: serde_json::Value,
    app: &AppHandle,
) -> Result<Response, BrowseError> {
    match name {
        "browse_places" => {
            let pictures = app.path().picture_dir().ok();
            let home = app.path().home_dir().ok();
            let prefs = prefs()?;
            json(
                blocking(move || {
                    Ok(Places {
                        drives: places::drives(),
                        standard: places::standard(pictures, home),
                        favourites: remembered::favourites(prefs),
                        recent: remembered::recent(prefs),
                    })
                })
                .await?,
            )
        }
        "browse_folders" => {
            let a: PathArgs = args_for(name, args)?;
            json(blocking(move || listing::folders(Path::new(&a.path))).await?)
        }
        "browse_list" => {
            let a: PathArgs = args_for(name, args)?;
            let browse = browse(app)?;
            json(blocking(move || browse.list(Path::new(&a.path))).await?)
        }
        "browse_want" => {
            let a: PathsArgs = args_for(name, args)?;
            browse(app)?
                .thumbnails()
                .want(a.paths.into_iter().map(PathBuf::from).collect());
            json(())
        }
        "browse_thumb" => {
            let a: PathArgs = args_for(name, args)?;
            Ok(match browse(app)?.thumbnails().held(Path::new(&a.path)) {
                Some(jpeg) => Response::new(jpeg.to_vec()),
                None => Response::new("null".to_string()),
            })
        }
        "browse_favourite" => {
            let a: FavouriteArgs = args_for(name, args)?;
            json(remembered::set_favourite(prefs()?, &a.path, a.on)?)
        }
        "browse_add_recent" => {
            let a: PathArgs = args_for(name, args)?;
            json(remembered::add_recent(prefs()?, &a.path)?)
        }
        other => Err(internal(format!("unknown Argentum command: {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every call the Browse view makes, with the arguments its client sends
    /// (src/shell/browse/wire.fixtures.json, whose own test checks the client
    /// sends exactly these).
    #[test]
    fn the_browse_calls_deserialise_as_its_client_sends_them() {
        let wire: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../src/shell/browse/wire.fixtures.json"
        ))
        .unwrap();
        let calls = wire["calls"].as_object().unwrap();
        assert_eq!(calls.len(), 7);
        for (name, args) in calls {
            let args = args.clone();
            match name.as_str() {
                "browse_places" => assert_eq!(args, serde_json::json!({})),
                "browse_folders" | "browse_list" | "browse_thumb" | "browse_add_recent" => {
                    args_for::<PathArgs>(name, args).unwrap();
                }
                "browse_want" => {
                    assert!(!args_for::<PathsArgs>(name, args).unwrap().paths.is_empty());
                }
                "browse_favourite" => {
                    args_for::<FavouriteArgs>(name, args).unwrap();
                }
                other => panic!("{other}: no arguments for it here"),
            }
        }
    }

    #[test]
    fn a_listing_streams_its_photos_as_events() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..10 {
            std::fs::write(dir.path().join(format!("IMG_{i}.CR3")), b"x").unwrap();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let browse =
            Browse::new(None, move |e| tx.lock().unwrap().send(e.clone()).unwrap()).unwrap();
        let id = browse.list(dir.path()).unwrap();
        let mut photos = 0;
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap() {
                BrowseEvent::Listed {
                    listing,
                    photos: part,
                    done,
                } => {
                    assert_eq!(listing, id);
                    photos += part.len();
                    if done {
                        break;
                    }
                }
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(photos, 10);
        assert_eq!(
            browse.list(&dir.path().join("gone")).unwrap_err().kind,
            BrowseErrorKind::NotFound
        );
    }
}
