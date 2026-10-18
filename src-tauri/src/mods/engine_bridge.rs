//! The engine (`src-tauri/engine`, DEC-29) inside the app, for the new
//! editor interface in `src/shell`.
//!
//! The interface calls it through the one `ag` command (`dispatch.rs`), so
//! their `lib.rs` gets nothing:
//!
//! | `ag` name         | What                                                 |
//! |-------------------|------------------------------------------------------|
//! | `engine_sections` | the engine's `sections()`                            |
//! | `engine_tools`    | `tools()`                                            |
//! | `engine_open`     | `open(path)`, decoded off the async runtime          |
//! | `engine_apply`    | `apply(photo, patch)`                                |
//! | `engine_view`     | `view(photo, view)`                                  |
//! | `engine_frame`    | `frame(ticket)`, delivered by `engine_frame.rs`      |
//! | `shell_folder`    | the RAWs beside a photo (`shell_photos.rs`)          |
//! | `shell_pick`      | a file dialog for one RAW (`shell_photos.rs`)        |
//! | `shell_launch_photo` | the photo the app was started with (`shell_photos.rs`) |
//! | `shell_show_window`  | shows the window, which the app builds hidden        |
//!
//! It hears the engine through one Tauri event, `ag-engine`, which carries
//! each engine event exactly as the contract writes it. The engine sends
//! events for the latest render only, so nothing is filtered here.
//!
//! NOTHING HERE WRITES A FILE. Edits stay in memory until the engine has a
//! sidecar of its own; today's `.agdata` files are never read or written.
//!
//! MEMORY. The engine keeps an open photo's decoded sensor data - 65 MB for
//! a 32 MP mosaic - until it is closed. So open photos are held to
//! `OPEN_PHOTOS_BYTES`, oldest let go first: a photo let go leaves its edit
//! here, and opening it again decodes it again with that edit. The engine's
//! cache of results has its own budget. The engine is made the first time
//! the interface asks for it, so today's interface never pays for it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use argentum_engine::contract::{EditDocument, Event, Opened, PhotoId, RenderTicket};
use argentum_engine::engine::Frame;
use argentum_engine::{Engine, Error, Result};
use tauri::{AppHandle, Emitter};

/// The Tauri event engine events go out as.
pub const EVENT: &str = "ag-engine";

/// Decoded sensor data kept for open photos, in bytes: seven 32 MP mosaics,
/// two of 100 MP. The photo opened last is kept whatever its size.
const OPEN_PHOTOS_BYTES: usize = 512 << 20;

pub struct Bridge {
    engine: Engine,
    photos: Mutex<Photos>,
    /// Decoded sensor data kept for open photos, in bytes.
    budget_bytes: usize,
    /// Opens run one at a time, so one file is never decoded twice at once.
    opening: Mutex<()>,
    /// Opens asked for so far: one that waited while a newer one was asked
    /// for gives way to it before decoding.
    asked: AtomicU64,
}

#[derive(Default)]
struct Photos {
    /// Open photos, the least recently opened first.
    open: Vec<Open>,
    /// The edits of photos let go, by path.
    kept: HashMap<PathBuf, EditDocument>,
}

struct Open {
    path: PathBuf,
    photo: PhotoId,
    width: u32,
    height: u32,
    decoded_bytes: usize,
}

/// The app's one engine, made the first time the new interface asks for it.
/// Its events go to every window as `ag-engine`.
pub fn bridge(app: &AppHandle) -> Result<&'static Bridge> {
    static BRIDGE: OnceLock<Result<Bridge>> = OnceLock::new();
    BRIDGE
        .get_or_init(|| {
            let app = app.clone();
            Bridge::new(OPEN_PHOTOS_BYTES, move |event| {
                if let Err(e) = app.emit(EVENT, &event) {
                    log::warn!("[engine] event not sent: {e}");
                }
            })
        })
        .as_ref()
        .map_err(Clone::clone)
}

impl Bridge {
    /// An engine whose events go to `sink`, keeping `budget_bytes` of
    /// decoded photos.
    fn new(budget_bytes: usize, sink: impl Fn(Event) + Send + Sync + 'static) -> Result<Bridge> {
        Ok(Bridge {
            engine: Engine::new(sink)?,
            photos: Mutex::default(),
            budget_bytes,
            opening: Mutex::new(()),
            asked: AtomicU64::new(0),
        })
    }

    /// The engine, for the calls that pass straight through: sections, tools,
    /// apply, view.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Open a RAW file - or the photo already open from it, with its edit as
    /// it stands - and let the oldest open photos go past the budget. Blocks
    /// while it decodes. None when a newer open was asked for while this one
    /// waited its turn: it gives way without decoding.
    pub fn open(&self, path: &Path) -> Result<Option<Opened>> {
        let path = std::path::absolute(path)
            .map_err(|e| Error::Decode(format!("{}: {e}", path.display())))?;
        let mine = self.asked.fetch_add(1, Ordering::SeqCst) + 1;
        let _one_at_a_time = self.opening.lock().unwrap_or_else(|e| e.into_inner());
        if self.asked.load(Ordering::SeqCst) != mine {
            return Ok(None);
        }

        let mut photos = self.photos();
        if let Some(i) = photos.open.iter().position(|o| o.path == path) {
            let open = photos.open.remove(i);
            let opened = Opened {
                photo_id: open.photo,
                width: open.width,
                height: open.height,
                edit: self.engine.edit(open.photo)?,
            };
            photos.open.push(open);
            return Ok(Some(opened));
        }
        let kept = photos.kept.get(&path).cloned();
        // Not held while decoding: renders of the open photo go on meanwhile.
        drop(photos);

        let opened = match &kept {
            Some(edit) => self.engine.open_with(&path, edit),
            None => self.engine.open(&path),
        }?;
        let decoded_bytes = self.engine.decoded_bytes(opened.photo_id)?;

        let mut photos = self.photos();
        photos.kept.remove(&path);
        photos.open.push(Open {
            path,
            photo: opened.photo_id,
            width: opened.width,
            height: opened.height,
            decoded_bytes,
        });
        let held = |photos: &Photos| photos.open.iter().map(|o| o.decoded_bytes).sum::<usize>();
        while photos.open.len() > 1 && held(&photos) > self.budget_bytes {
            let old = photos.open.remove(0);
            if let Ok(edit) = self.engine.close(old.photo) {
                photos.kept.insert(old.path, edit);
            }
        }
        Ok(Some(opened))
    }

    /// The frame a `frame` event announced, while it is the latest.
    pub fn frame(&self, ticket: RenderTicket) -> Option<Arc<Frame>> {
        self.engine.frame(ticket)
    }

    fn photos(&self) -> MutexGuard<'_, Photos> {
        self.photos.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::engine_frame;
    use crate::mods::shell_photos;
    use argentum_engine::contract::{EditPatch, View};
    use std::sync::mpsc::{Receiver, channel};
    use std::time::{Duration, Instant};

    fn patch(json: serde_json::Value) -> EditPatch {
        serde_json::from_value(json).unwrap()
    }

    fn with_budget(budget_bytes: usize) -> (Bridge, Receiver<Event>) {
        let (tx, rx) = channel();
        let bridge = Bridge::new(budget_bytes, move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        (bridge, rx)
    }

    /// Opened, and not given way to a newer open.
    fn open(bridge: &Bridge, path: &Path) -> Opened {
        bridge
            .open(path)
            .unwrap()
            .expect("not replaced by a newer open")
    }

    /// The RAW named by `AG_RAW`, as the app's other real-photo tests take it.
    fn raw(test: &str) -> Option<PathBuf> {
        match std::env::var("AG_RAW") {
            Ok(p) if !p.trim().is_empty() => Some(PathBuf::from(p)),
            _ => {
                eprintln!("{test}: SKIPPED - set AG_RAW to a RAW file to run it");
                None
            }
        }
    }

    /// Events until `ticket`'s histogram, in order.
    fn until_done(rx: &Receiver<Event>, ticket: RenderTicket) -> Vec<Event> {
        let mut seen = Vec::new();
        loop {
            let event = rx
                .recv_timeout(Duration::from_secs(120))
                .unwrap_or_else(|_| panic!("nothing for {ticket:?}; saw {seen:?}"));
            if let Event::Error { message, .. } = &event {
                panic!("{message}");
            }
            let done = matches!(&event, Event::Histogram { ticket: t, .. } if *t == ticket);
            seen.push(event);
            if done {
                return seen;
            }
        }
    }

    fn fit(opened: &Opened, width: u32, height: u32) -> View {
        View {
            zoom: (width as f32 / opened.width as f32).min(height as f32 / opened.height as f32),
            center_x: 0.5,
            center_y: 0.5,
            width,
            height,
        }
    }

    #[test]
    fn a_file_that_is_not_a_photo_is_an_error_not_a_photo() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.cr3");
        std::fs::write(&path, b"not a raw").unwrap();
        let (bridge, _rx) = with_budget(usize::MAX);
        assert!(matches!(bridge.open(&path), Err(Error::Decode(_))));
        assert!(matches!(
            bridge.open(&dir.path().join("missing.cr3")),
            Err(Error::Decode(_))
        ));
    }

    /// Open, apply, view and frame through the bridge, the events as the
    /// interface reads them, and latest-wins.
    #[test]
    fn open_apply_view_frame_and_latest_wins() {
        let Some(path) = raw("open_apply_view_frame_and_latest_wins") else {
            return;
        };
        let (bridge, rx) = with_budget(usize::MAX);
        assert_eq!(bridge.engine().tools().len(), 3);
        assert!(!bridge.engine().sections().is_empty());

        let opened = open(&bridge, &path);
        let view = fit(&opened, 800, 600);
        let t = bridge.engine().view(opened.photo_id, view).unwrap();
        let events = until_done(&rx, t);
        let Some(Event::Frame { width, height, .. }) = events.first() else {
            panic!("{events:?}")
        };
        // As the interface receives it: camelCase, with the photo.
        let json = serde_json::to_value(&events[0]).unwrap();
        assert_eq!(json["event"], "frame");
        assert_eq!(json["photoId"], opened.photo_id.0);
        assert_eq!(json["ticket"], t.0);

        let shown = bridge.frame(t).expect("the latest frame");
        assert_eq!((shown.width, shown.height), (*width, *height));
        assert_eq!(shown.rgba.len(), (*width * *height * 4) as usize);

        // A patch renders again; a bad one is refused and renders nothing.
        let brighter = bridge
            .engine()
            .apply(
                opened.photo_id,
                &patch(serde_json::json!({ "ag_exposure_wb": { "exposure": 1.0 } })),
            )
            .unwrap()
            .ticket;
        until_done(&rx, brighter);
        assert_ne!(bridge.frame(brighter).unwrap().rgba, shown.rgba, "brighter");
        assert!(matches!(
            bridge.engine().apply(
                opened.photo_id,
                &patch(serde_json::json!({ "ag_exposure_wb": { "nope": 1 } }))
            ),
            Err(Error::UnknownSetting { .. })
        ));

        // Latest wins: of two patches sent at once, only the second reports,
        // and the first's frame is never there to fetch.
        let first = bridge
            .engine()
            .apply(
                opened.photo_id,
                &patch(serde_json::json!({ "ag_exposure_wb": { "exposure": 0.2 } })),
            )
            .unwrap()
            .ticket;
        let second = bridge
            .engine()
            .apply(
                opened.photo_id,
                &patch(serde_json::json!({ "ag_exposure_wb": { "exposure": 0.4 } })),
            )
            .unwrap()
            .ticket;
        let events = until_done(&rx, second);
        assert!(
            events.iter().all(|e| !matches!(e,
                Event::Frame { ticket, .. } | Event::Histogram { ticket, .. } if *ticket == first)),
            "{events:?}"
        );
        assert!(bridge.frame(first).is_none());
        assert!(bridge.frame(second).is_some());
        // And once a newer one is out, the older is gone.
        let third = bridge.engine().view(opened.photo_id, view).unwrap();
        until_done(&rx, third);
        assert!(bridge.frame(second).is_none());
    }

    /// An open that waited its turn while a newer one was asked for gives
    /// way without decoding; the newer one goes ahead.
    #[test]
    fn an_open_replaced_while_it_waited_gives_way() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-a-raw.cr3");
        std::fs::write(&path, b"not a raw").unwrap();
        let (bridge, _rx) = with_budget(usize::MAX);
        let bridge = Arc::new(bridge);
        let asked = |n| {
            while bridge.asked.load(Ordering::SeqCst) < n {
                std::thread::yield_now();
            }
        };
        let busy = bridge.opening.lock().unwrap();
        let first = std::thread::spawn({
            let (bridge, path) = (bridge.clone(), path.clone());
            move || bridge.open(&path)
        });
        asked(1);
        let second = std::thread::spawn({
            let (bridge, path) = (bridge.clone(), path.clone());
            move || bridge.open(&path)
        });
        asked(2);
        drop(busy);
        assert!(matches!(first.join().unwrap(), Ok(None)), "gave way");
        // The newer one tried to decode: this file is no photo.
        assert!(matches!(second.join().unwrap(), Err(Error::Decode(_))));
    }

    /// Past the budget the oldest photo is let go; opened again, it has its
    /// edit back. A photo still open comes back at once, with its edit.
    #[test]
    fn photos_past_the_budget_are_let_go_and_keep_their_edits() {
        let Some(path) = raw("photos_past_the_budget_are_let_go_and_keep_their_edits") else {
            return;
        };
        let others: Vec<PathBuf> = shell_photos::folder(&path)
            .unwrap()
            .into_iter()
            .map(PathBuf::from)
            .filter(|p| std::path::absolute(p).ok() != std::path::absolute(&path).ok())
            .take(2)
            .collect();
        if others.len() < 2 {
            eprintln!("photos_past_the_budget_...: SKIPPED - AG_RAW's folder needs two more RAWs");
            return;
        }
        // Room for two of these photos, not three.
        let (bridge, _rx) = with_budget(usize::MAX);
        let first = open(&bridge, &path);
        let one = bridge.engine().decoded_bytes(first.photo_id).unwrap();
        let (bridge, _rx) = with_budget(one * 5 / 2);
        let first = open(&bridge, &path);
        bridge
            .engine()
            .apply(
                first.photo_id,
                &patch(serde_json::json!({ "ag_exposure_wb": { "exposure": 0.5 } })),
            )
            .unwrap();

        let second = open(&bridge, &others[0]);
        let started = Instant::now();
        let first_again = open(&bridge, &path);
        assert!(
            started.elapsed() < Duration::from_millis(50),
            "not decoded again"
        );
        assert_eq!(first_again.photo_id, first.photo_id);
        assert_eq!(
            first_again.edit.tools["ag_exposure_wb"].settings["exposure"],
            0.5
        );

        // Two open: the second is now the oldest, and goes.
        open(&bridge, &others[1]);
        assert!(
            bridge
                .engine()
                .view(second.photo_id, fit(&second, 64, 64))
                .is_err()
        );
        assert_eq!(bridge.photos().open.len(), 2);

        // Then the first goes too, and comes back as a new photo with its edit.
        open(&bridge, &others[0]);
        assert!(
            bridge
                .engine()
                .view(first.photo_id, fit(&first, 64, 64))
                .is_err()
        );
        let reopened = open(&bridge, &path);
        assert_ne!(reopened.photo_id, first.photo_id);
        assert_eq!(reopened.edit, first_again.edit);
        assert_eq!(
            (reopened.width, reopened.height),
            (first.width, first.height)
        );
    }

    /// The process's memory in bytes: (committed now, peak committed, peak
    /// resident).
    fn memory_bytes() -> (usize, usize, usize) {
        let mut v = [0usize; 8];
        let [
            elapsed,
            user,
            system,
            rss,
            peak_rss,
            commit,
            peak_commit,
            faults,
        ] = &mut v;
        // SAFETY: every argument is a valid pointer to a usize mimalloc writes once.
        unsafe {
            libmimalloc_sys::mi_process_info(
                elapsed,
                user,
                system,
                rss,
                peak_rss,
                commit,
                peak_commit,
                faults,
            );
        }
        (v[5], v[6], v[4])
    }

    /// A slider tick through the bridge, timed: the patch in, the frame event
    /// out, the frame's block made for the page. At fit and at 100%, at two
    /// view sizes; and the process's memory after each.
    ///
    /// Run alone, as tests in one process share the thread pool:
    /// `AG_RAW=<file> cargo test --lib mods::engine_bridge::tests::slider_ticks -- --ignored --nocapture --test-threads=1`
    #[test]
    #[ignore = "a measurement: run alone, see above"]
    fn slider_ticks() {
        let Some(path) = raw("slider_ticks") else {
            return;
        };
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let median = |mut v: Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        let gb = |bytes: usize| bytes as f64 / (1u64 << 30) as f64;
        let (bridge, rx) = with_budget(usize::MAX);
        let started = Instant::now();
        let opened = open(&bridge, &path);
        eprintln!(
            "\n  {} x {}: opened in {:.0} ms",
            opened.width,
            opened.height,
            ms(started.elapsed())
        );
        for zoom in ["fit", "100%"] {
            for (w, h) in [(1600, 1067), (2560, 1440)] {
                let mut view = fit(&opened, w, h);
                if zoom == "100%" {
                    view.zoom = 1.0;
                }
                let started = Instant::now();
                until_done(&rx, bridge.engine().view(opened.photo_id, view).unwrap());
                let first = ms(started.elapsed());
                let (mut render, mut block, mut total) = (Vec::new(), Vec::new(), Vec::new());
                let mut block_bytes = 0;
                for i in 1..=12 {
                    let p = patch(
                        serde_json::json!({ "ag_exposure_wb": { "exposure": i as f32 * 0.05 } }),
                    );
                    let started = Instant::now();
                    let t = bridge.engine().apply(opened.photo_id, &p).unwrap().ticket;
                    loop {
                        match rx.recv_timeout(Duration::from_secs(120)).unwrap() {
                            Event::Frame { ticket, .. } if ticket == t => break,
                            Event::Error { message, .. } => panic!("{message}"),
                            _ => {}
                        }
                    }
                    let rendered = started.elapsed();
                    let b = engine_frame::block(&bridge.frame(t).unwrap());
                    let done = started.elapsed();
                    block_bytes = b.len();
                    render.push(ms(rendered));
                    block.push(ms(done - rendered));
                    total.push(ms(done));
                }
                eprintln!(
                    "  view {w} x {h} at {zoom}: first {first:.0} ms; tick median {:.1} ms \
                     (render {:.1}, block {:.1}), {:.1} MB a frame",
                    median(total),
                    median(render),
                    median(block),
                    block_bytes as f64 / 1e6
                );
            }
            let (commit, peak_commit, peak_rss) = memory_bytes();
            eprintln!(
                "  memory after {zoom}: {:.2} GB committed, peak {:.2} GB committed, {:.2} GB resident",
                gb(commit),
                gb(peak_commit),
                gb(peak_rss)
            );
        }
    }
}
