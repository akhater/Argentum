//! The engine as the interface uses it: list the sections and tools, open a
//! photo, change its edit, look at part of it. Step 2 makes each of these a Tauri command
//! and forwards the events.
//!
//! `apply` and `view` return a ticket at once and the render happens on a
//! thread of its own. Renders are latest-wins: a newer ticket replaces any
//! render still waiting, and one already running gives up at the next step
//! boundary. A finished render is reported by a `frame` event, then a
//! `histogram` event, both with its ticket; a failed one by an `error` event.
//! A render that was overtaken reports nothing.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;

use crate::contract::{
    Applied, EditDocument, EditPatch, Event, Opened, PhotoId, RenderTicket, SectionDescriptor,
    ToolDescriptor, View,
};
use crate::frame::{self, Frame, fitted};
use crate::{Error, GpuSlot, Pipeline, Registry, Result, Source};

/// The view a photo has until `view` is called: the whole photo, fitted in
/// this many pixels.
const FIRST_VIEW_PX: u32 = 1600;

pub struct Engine {
    /// Reads patches here; the render thread's pipeline runs the same tools.
    registry: Arc<Registry>,
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    /// The newest ticket handed out. A render for any other gives up.
    latest: AtomicU32,
    frame: Mutex<Option<Arc<Frame>>>,
    sink: Box<dyn Fn(Event) + Send + Sync>,
}

struct State {
    photos: HashMap<PhotoId, Photo>,
    next_photo: u32,
    next_ticket: u32,
    job: Option<Job>,
    stop: bool,
}

struct Photo {
    source: Arc<Source>,
    size_px: (u32, u32),
    edit: EditDocument,
    view: View,
}

struct Job {
    ticket: RenderTicket,
    photo: PhotoId,
    source: Arc<Source>,
    size_px: (u32, u32),
    edit: EditDocument,
    view: View,
}

impl Engine {
    /// Argentum's tools. Events go to `sink`, from the render thread.
    pub fn new(sink: impl Fn(Event) + Send + Sync + 'static) -> Result<Engine> {
        Self::with_registry(crate::registry::standard(), sink)
    }

    /// `registry`'s tools.
    pub fn with_registry(
        registry: Registry,
        sink: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<Engine> {
        let registry = Arc::new(registry);
        let render = Pipeline::with_gpu(registry.clone(), GpuSlot::new())?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                photos: HashMap::new(),
                next_photo: 1,
                next_ticket: 1,
                job: None,
                stop: false,
            }),
            wake: Condvar::new(),
            latest: AtomicU32::new(0),
            frame: Mutex::new(None),
            sink: Box::new(sink),
        });
        let worker = {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("argentum-engine render".into())
                .spawn(move || work(&shared, render))
                .map_err(|e| Error::Tool {
                    tool: "engine",
                    message: format!("starting the render thread: {e}"),
                })?
        };
        Ok(Engine {
            registry,
            shared,
            worker: Some(worker),
        })
    }

    /// Every section, in the order the interface shows them.
    pub fn sections(&self) -> Vec<SectionDescriptor> {
        crate::contract::sections()
    }

    /// Every tool, as the interface builds its panels, in pipeline order.
    pub fn tools(&self) -> Vec<ToolDescriptor> {
        self.registry.descriptors()
    }

    /// Decode a RAW file. Blocks while it decodes; renders nothing.
    pub fn open(&self, path: impl AsRef<Path>) -> Result<Opened> {
        Ok(self.insert(Source::open(path)?))
    }

    /// A photo decoded elsewhere, opened with every tool at its defaults.
    pub fn insert(&self, source: Source) -> Opened {
        let (w_px, h_px) = source.size_px();
        let size_px = (w_px as u32, h_px as u32);
        let edit = self.registry.default_edit();
        let mut state = self.state();
        let id = PhotoId(state.next_photo);
        state.next_photo += 1;
        state.photos.insert(
            id,
            Photo {
                source: Arc::new(source),
                size_px,
                edit: edit.clone(),
                view: fitted(size_px, FIRST_VIEW_PX, FIRST_VIEW_PX),
            },
        );
        Opened {
            photo_id: id,
            width: size_px.0,
            height: size_px.1,
            edit,
        }
    }

    /// Change a photo's edit and render it again. Returns the edit as the
    /// engine took it. An unknown tool or setting, or a value of the wrong
    /// type, refuses the whole patch and nothing renders.
    pub fn apply(&self, photo: PhotoId, patch: &EditPatch) -> Result<Applied> {
        let mut state = self.state();
        let edit = self.registry.merge(&state.photo(photo)?.edit, patch)?;
        state.photo_mut(photo)?.edit = edit.clone();
        let ticket = self.queue(&mut state, photo)?;
        Ok(Applied { ticket, edit })
    }

    /// Look at another part of a photo, or at another zoom.
    pub fn view(&self, photo: PhotoId, view: View) -> Result<RenderTicket> {
        let usable = view.zoom.is_finite()
            && view.zoom > 0.0
            && view.center_x.is_finite()
            && view.center_y.is_finite()
            && view.width > 0
            && view.height > 0;
        if !usable {
            return Err(Error::BadView(format!("{view:?}")));
        }
        let mut state = self.state();
        state.photo_mut(photo)?.view = view;
        self.queue(&mut state, photo)
    }

    /// The frame a `frame` event announced, while it is the latest one.
    pub fn frame(&self, ticket: RenderTicket) -> Option<Arc<Frame>> {
        let frame = self.shared.frame.lock().unwrap_or_else(|e| e.into_inner());
        frame.as_ref().filter(|f| f.ticket == ticket).cloned()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.shared.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn queue(&self, state: &mut State, photo: PhotoId) -> Result<RenderTicket> {
        let ticket = RenderTicket(state.next_ticket);
        state.next_ticket += 1;
        let p = state.photo(photo)?;
        let job = Job {
            ticket,
            photo,
            source: p.source.clone(),
            size_px: p.size_px,
            edit: p.edit.clone(),
            view: p.view,
        };
        state.job = Some(job);
        self.shared.latest.store(ticket.0, Ordering::SeqCst);
        self.shared.wake.notify_one();
        Ok(ticket)
    }
}

impl State {
    fn photo(&self, id: PhotoId) -> Result<&Photo> {
        self.photos.get(&id).ok_or(Error::UnknownPhoto(id))
    }

    fn photo_mut(&mut self, id: PhotoId) -> Result<&mut Photo> {
        self.photos.get_mut(&id).ok_or(Error::UnknownPhoto(id))
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.state().stop = true;
        self.shared.wake.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
/// The render thread: the newest job, rendered, reported - unless a newer
/// one arrives first.
fn work(shared: &Shared, mut pipeline: Pipeline) {
    loop {
        let job = {
            let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if state.stop {
                    return;
                }
                if let Some(job) = state.job.take() {
                    break job;
                }
                state = shared.wake.wait(state).unwrap_or_else(|e| e.into_inner());
            }
        };
        let overtaken = || shared.latest.load(Ordering::SeqCst) != job.ticket.0;
        match render(&mut pipeline, &job, &overtaken) {
            Ok(_) if overtaken() => {}
            Ok((frame, bins)) => {
                let placement = frame.placement;
                *shared.frame.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(frame));
                (shared.sink)(Event::Frame {
                    ticket: job.ticket,
                    photo_id: job.photo,
                    placement,
                });
                (shared.sink)(Event::Histogram {
                    ticket: job.ticket,
                    photo_id: job.photo,
                    bins,
                });
            }
            Err(Error::Cancelled) => {}
            Err(e) => (shared.sink)(Event::Error {
                ticket: Some(job.ticket),
                kind: e.kind(),
                message: e.to_string(),
            }),
        }
    }
}

fn render(
    pipeline: &mut Pipeline,
    job: &Job,
    overtaken: &dyn Fn() -> bool,
) -> Result<(Frame, Vec<u32>)> {
    pipeline.set_edit(&job.edit)?;
    let layout = frame::layout(&job.view, job.size_px);
    let image = pipeline.render_unless(&job.source, layout.size, overtaken)?;
    let rgb = image.as_rgb().ok_or_else(|| Error::Tool {
        tool: "engine",
        message: format!("the pipeline ended with {}", image.describe()),
    })?;
    let frame = frame::cut(rgb, &layout, job.ticket);
    let bins = frame::histogram(&frame);
    Ok((frame, bins))
}
