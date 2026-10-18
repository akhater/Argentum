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
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::thread::JoinHandle;

use rayon::prelude::*;

use crate::contract::{
    Applied, EditDocument, EditPatch, Event, HISTOGRAM_BINS, Opened, PhotoId, RenderTicket,
    SectionDescriptor, ToolDescriptor, View,
};
use crate::{Error, GpuSlot, Pipeline, Registry, Result, Rgb, Size, Source};

/// The view a photo has until `view` is called: the whole photo, fitted in
/// this many pixels.
const FIRST_VIEW: u32 = 1600;

/// One rendered view: sRGB, 8 bits per channel, RGBA, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub ticket: RenderTicket,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

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
    size: (u32, u32),
    edit: EditDocument,
    view: View,
}

struct Job {
    ticket: RenderTicket,
    photo: PhotoId,
    source: Arc<Source>,
    size: (u32, u32),
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
        let (w, h) = source.size();
        let size = (w as u32, h as u32);
        let edit = self.registry.default_edit();
        let mut state = self.state();
        let id = PhotoId(state.next_photo);
        state.next_photo += 1;
        state.photos.insert(
            id,
            Photo {
                source: Arc::new(source),
                size,
                edit: edit.clone(),
                view: fitted(size, FIRST_VIEW, FIRST_VIEW),
            },
        );
        Opened {
            photo_id: id,
            width: size.0,
            height: size.1,
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

    /// Decode a RAW file and give it an edit it had before, read as a saved
    /// edit is: migrated, clamped, all or nothing. Renders nothing.
    pub fn open_with(&self, path: impl AsRef<Path>, edit: &EditDocument) -> Result<Opened> {
        self.insert_with(Source::open(path)?, edit)
    }

    /// A photo decoded elsewhere, opened with `edit`.
    pub fn insert_with(&self, source: Source, edit: &EditDocument) -> Result<Opened> {
        let edit = self.registry.merge(edit, &EditPatch::default())?;
        let mut opened = self.insert(source);
        self.state().photo_mut(opened.photo_id)?.edit = edit.clone();
        opened.edit = edit;
        Ok(opened)
    }

    /// A photo's edit, with every patch it has accepted.
    pub fn edit(&self, photo: PhotoId) -> Result<EditDocument> {
        Ok(self.state().photo(photo)?.edit.clone())
    }

    /// Let a photo go, handing back its edit. Its decoded data is freed once
    /// no render is using it; a render already asked for still reports.
    pub fn close(&self, photo: PhotoId) -> Result<EditDocument> {
        let closed = self.state().photos.remove(&photo);
        closed.map(|p| p.edit).ok_or(Error::UnknownPhoto(photo))
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
            size: p.size,
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
                let (width, height) = (frame.width, frame.height);
                *shared.frame.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(frame));
                (shared.sink)(Event::Frame {
                    ticket: job.ticket,
                    photo_id: job.photo,
                    width,
                    height,
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
    let (w, h) = job.size;
    let zoom = job.view.zoom as f64;
    // At or past 100% the pipeline renders at full size and the view is cut
    // out of that; below it, the pipeline renders the whole photo at the
    // zoom, and a pan is a different cut of the same cached render.
    let size = if zoom >= 1.0 {
        Size::Full
    } else {
        Size::Fit(
            ((w as f64 * zoom).ceil() as u32).max(1),
            ((h as f64 * zoom).ceil() as u32).max(1),
        )
    };
    let image = pipeline.render_unless(&job.source, size, overtaken)?;
    let rgb = image.as_rgb().ok_or_else(|| Error::Tool {
        tool: "engine",
        message: format!("the pipeline ended with {}", image.describe()),
    })?;
    let frame = cut(rgb, &job.view, job.ticket);
    let bins = histogram(&frame);
    Ok((frame, bins))
}

/// The view's part of `rgb`, encoded for the screen.
fn cut(rgb: &Rgb, view: &View, ticket: RenderTicket) -> Frame {
    // Render pixels per screen pixel: 1 below 100%, where the render is made
    // at the zoom, and less past it, where pixels are repeated.
    let per_screen = if view.zoom > 1.0 {
        1.0 / view.zoom as f64
    } else {
        1.0
    };
    let (rw, rh) = (rgb.width as f64, rgb.height as f64);
    let span_w = (view.width as f64 * per_screen).min(rw);
    let span_h = (view.height as f64 * per_screen).min(rh);
    let x0 = (view.center_x.clamp(0.0, 1.0) as f64 * rw - span_w / 2.0).clamp(0.0, rw - span_w);
    let y0 = (view.center_y.clamp(0.0, 1.0) as f64 * rh - span_h / 2.0).clamp(0.0, rh - span_h);
    let width = ((span_w / per_screen).round() as u32).clamp(1, view.width);
    let height = ((span_h / per_screen).round() as u32).clamp(1, view.height);

    let sample = |o: u32, origin: f64, size: usize| {
        ((origin + (o as f64 + 0.5) * per_screen) as usize).min(size - 1)
    };
    let rgba: Vec<u8> = (0..height)
        .into_par_iter()
        .flat_map_iter(|oy| {
            let sy = sample(oy, y0, rgb.height);
            (0..width).flat_map(move |ox| {
                let p = rgb.pixels[sy * rgb.width + sample(ox, x0, rgb.width)];
                [encode(p[0]), encode(p[1]), encode(p[2]), 255]
            })
        })
        .collect();
    Frame {
        ticket,
        width,
        height,
        rgba,
    }
}

/// Linear light to an 8-bit sRGB value, clipped at white. A plain display
/// encode until display tools take over.
///
/// From a table of 65536 steps rather than a `powf` per channel, which was
/// most of a slider tick: a step is at most a twentieth of an output level,
/// in the shadows where the curve is steepest.
fn encode(v: f32) -> u8 {
    static TABLE: OnceLock<Vec<u8>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        (0..=u16::MAX)
            .map(|i| {
                let v = i as f32 / u16::MAX as f32;
                let s = if v <= 0.003_130_8 {
                    v * 12.92
                } else {
                    1.055 * v.powf(1.0 / 2.4) - 0.055
                };
                (s * 255.0).round() as u8
            })
            .collect()
    });
    table[(v.clamp(0.0, 1.0) * u16::MAX as f32).round() as usize]
}

/// `HISTOGRAM_BINS` bins of the frame's luminance (Rec. 709 weights on the
/// encoded 8-bit values, one bin per level).
fn histogram(frame: &Frame) -> Vec<u32> {
    frame
        .rgba
        .par_chunks_exact(4)
        .fold(
            || vec![0u32; HISTOGRAM_BINS],
            |mut bins, p| {
                let y = 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
                bins[(y.round() as usize).min(HISTOGRAM_BINS - 1)] += 1;
                bins
            },
        )
        .reduce(
            || vec![0u32; HISTOGRAM_BINS],
            |mut a, b| {
                a.iter_mut().zip(b).for_each(|(x, y)| *x += y);
                a
            },
        )
}

/// The whole photo, fitted in `max_w` x `max_h`, never enlarged.
fn fitted(size: (u32, u32), max_w: u32, max_h: u32) -> View {
    let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
    let zoom = (max_w as f32 / w).min(max_h as f32 / h).min(1.0);
    View {
        zoom,
        center_x: 0.5,
        center_y: 0.5,
        width: ((w * zoom).round() as u32).max(1),
        height: ((h * zoom).round() as u32).max(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: usize, h: usize) -> Rgb {
        Rgb::new(
            w,
            h,
            (0..w * h)
                .map(|i| {
                    let v = (i % w) as f32 / w as f32;
                    [v, v, v]
                })
                .collect(),
        )
    }

    #[test]
    fn below_100_percent_the_view_is_a_cut_of_the_render() {
        let rgb = picture(400, 300);
        let view = View {
            zoom: 0.5,
            center_x: 0.5,
            center_y: 0.5,
            width: 100,
            height: 50,
        };
        let f = cut(&rgb, &view, RenderTicket(1));
        assert_eq!((f.width, f.height), (100, 50));
        // The middle of the render, pixel for pixel.
        assert_eq!(f.rgba[0], encode(150.0 / 400.0));
    }

    #[test]
    fn past_100_percent_pixels_repeat() {
        let rgb = picture(400, 300);
        let view = View {
            zoom: 4.0,
            center_x: 0.0,
            center_y: 0.0,
            width: 40,
            height: 40,
        };
        let f = cut(&rgb, &view, RenderTicket(1));
        assert_eq!((f.width, f.height), (40, 40));
        // Held at the corner, ten render pixels across, each four wide.
        assert_eq!(f.rgba[0], f.rgba[3 * 4]);
        assert_ne!(f.rgba[0], f.rgba[4 * 4]);
    }

    #[test]
    fn a_view_larger_than_the_photo_gets_the_photo() {
        let f = cut(
            &picture(40, 30),
            &View {
                zoom: 1.0,
                center_x: 0.5,
                center_y: 0.5,
                width: 800,
                height: 600,
            },
            RenderTicket(1),
        );
        assert_eq!((f.width, f.height), (40, 30));
        assert_eq!(histogram(&f).iter().sum::<u32>(), 1200);
    }

    #[test]
    fn the_encode_is_srgb() {
        assert_eq!(encode(0.0), 0);
        assert_eq!(encode(1.0), 255);
        assert_eq!(encode(4.0), 255);
        assert_eq!(encode(0.18), 118);
        // The table against the formula, across the range.
        for i in 0..=200_000 {
            let v = i as f32 / 200_000.0;
            let s = if v <= 0.003_130_8 {
                v * 12.92
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            };
            let exact = s * 255.0;
            assert!(
                (encode(v) as f32 - exact).abs() <= 0.55,
                "{v}: {} vs {exact}",
                encode(v)
            );
        }
    }
}
