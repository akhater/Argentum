//! The viewer surface across minimise and restore. Ours.
//!
//! The photo is drawn by wgpu onto the main window, under a transparent
//! webview. Wherever the window holds no drawn frame it is see-through, and the
//! viewer shows whatever is behind Argentum. After a minimise and restore it did
//! exactly that, until the next edit drew the photo again.
//!
//! WHAT MINIMISING DOES
//!
//! The window's pixels are thrown away (seen with the OpenGL backend, which
//! draws into the window itself through `SwapBuffers`). The resize handler did
//! draw on restore, but a frame drawn while the window is still coming back is
//! thrown away too.
//!
//! Windows reports 0x0 on minimise, but Tauri passes on its own reading of the
//! window's size instead, which is not zero. So the window is asked whether it
//! is minimised, rather than the size being trusted.
//!
//! So nothing is drawn while minimised, and a restore draws at once and again
//! at each of `REDRAW_AFTER_RESTORE_MS`.
//!
//! The `[viewer]` log lines say which of those steps happened. They go once the
//! fix is confirmed.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::Duration;

use tauri::Manager;
use tauri::PhysicalSize;

use crate::AppState;
use crate::gpu_processing::WgpuDisplay;
use crate::image_processing::GpuContext;

/// When the viewer is drawn again after a restore, counted from the restore.
const REDRAW_AFTER_RESTORE_MS: [u64; 3] = [50, 200, 500];

static MINIMISED: AtomicBool = AtomicBool::new(false);

/// The window's latest size, width in the high half, for the redraws that run
/// after it may have changed again.
static LATEST_SIZE: AtomicU64 = AtomicU64::new(0);

/// Whether the main window is minimised. Nothing may be drawn to it then.
pub fn is_minimised() -> bool {
    MINIMISED.load(Ordering::Relaxed)
}

/// The main window's `Resized` event: fit the surface to the window and redraw.
pub fn apply_window_resize(window: &tauri::Window, size: PhysicalSize<u32>) {
    if window.is_minimized().unwrap_or(false) || size.width == 0 || size.height == 0 {
        if !MINIMISED.swap(true, Ordering::Relaxed) {
            log::info!("[viewer] minimised");
        }
        return;
    }
    LATEST_SIZE.store(
        (u64::from(size.width) << 32) | u64::from(size.height),
        Ordering::Relaxed,
    );
    let restored = MINIMISED.swap(false, Ordering::Relaxed);

    let state = window.state::<AppState>();
    let busy = || fit_once_free(window.app_handle(), restored);
    let Some((ctx, presented)) = fit_now(&state.gpu_context, display_of, busy, |ctx, display| {
        display.as_mut().map(|d| fit_and_render(d, ctx, size))
    }) else {
        return;
    };

    if restored {
        log::info!(
            "[viewer] restored {}x{}, frame presented: {presented:?}",
            size.width,
            size.height
        );
        redraw_after_restore(ctx);
    }
}

fn latest_size() -> PhysicalSize<u32> {
    let packed = LATEST_SIZE.load(Ordering::Relaxed);
    PhysicalSize::new((packed >> 32) as u32, packed as u32)
}

/// Fits the surface to `size` and draws; returns whether a frame was presented.
fn fit_and_render(display: &mut WgpuDisplay, ctx: &GpuContext, size: PhysicalSize<u32>) -> bool {
    if display.config.width != size.width || display.config.height != size.height {
        display.config.width = size.width;
        display.config.height = size.height;
        display.surface.configure(&ctx.device, &display.config);
    }
    display.render(&ctx.device, &ctx.queue)
}

fn display_of(ctx: &GpuContext) -> &Mutex<Option<WgpuDisplay>> {
    &ctx.display
}

/// The lock, or None with `busy` called if something holds it.
fn lock_now<T>(lock: &Mutex<T>, busy: impl FnOnce()) -> Option<MutexGuard<'_, T>> {
    match lock.try_lock() {
        Ok(guard) => Some(guard),
        Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(TryLockError::WouldBlock) => {
            busy();
            None
        }
    }
}

/// The context and what `fit` made of its display. None if there is no context,
/// or with `busy` called if something holds either lock.
///
/// Never wait for these on the event thread: a render may hold the display, and
/// a thread making a new context holds the context's lock while it asks the event
/// thread for the window, see mods/ui_thread.rs.
fn fit_now<C: Clone, D, R>(
    contexts: &Mutex<Option<C>>,
    display: fn(&C) -> &Mutex<D>,
    busy: impl Fn(),
    fit: impl FnOnce(&C, &mut D) -> R,
) -> Option<(C, R)> {
    let context = lock_now(contexts, &busy)?.clone()?;
    let fitted = fit(&context, &mut *lock_now(display(&context), &busy)?);
    Some((context, fitted))
}

/// Fits the surface once its locks are free, off the event thread.
fn fit_once_free(app: &tauri::AppHandle, restored: bool) {
    static WAITING: WaitingFit = WaitingFit::new();
    if !WAITING.ask(restored) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        WAITING.run(
            &state.gpu_context,
            display_of,
            latest_size,
            |ctx, display, size, restored| {
                if is_minimised() {
                    return;
                }
                if let Some(display) = display.as_mut() {
                    fit_and_render(display, ctx, size);
                }
                if restored {
                    redraw_after_restore(ctx.clone());
                }
            },
        );
    });
}

/// A fit that could not run because a lock was busy. However many resizes arrive
/// meanwhile, one thread waits, and it fits to the latest size.
struct WaitingFit {
    waiting: AtomicBool,
    restored: AtomicBool,
}

impl WaitingFit {
    const fn new() -> Self {
        Self {
            waiting: AtomicBool::new(false),
            restored: AtomicBool::new(false),
        }
    }

    /// Says a fit is owed; true for the caller that must start the thread.
    fn ask(&self, restored: bool) -> bool {
        self.restored.fetch_or(restored, Ordering::SeqCst);
        !self.waiting.swap(true, Ordering::SeqCst)
    }

    /// Waits for both locks, then fits. Only then does the next `ask` start a
    /// thread of its own, and it is taken before the size is read, so no resize
    /// falls between the two and no older size lands after a newer one.
    fn run<C: Clone, D>(
        &self,
        contexts: &Mutex<Option<C>>,
        display: fn(&C) -> &Mutex<D>,
        size: impl Fn() -> PhysicalSize<u32>,
        fit: impl FnOnce(&C, &mut D, PhysicalSize<u32>, bool),
    ) {
        let context = contexts.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut held = context
            .as_ref()
            .map(|context| display(context).lock().unwrap_or_else(|e| e.into_inner()));
        self.waiting.store(false, Ordering::SeqCst);
        let restored = self.restored.swap(false, Ordering::SeqCst);
        if let (Some(context), Some(held)) = (&context, &mut held) {
            fit(context, held, size(), restored);
        }
    }
}

/// Draws the viewer again at each of `REDRAW_AFTER_RESTORE_MS`, off the event
/// thread, so one of the frames lands once the window is back on screen.
fn redraw_after_restore(ctx: GpuContext) {
    std::thread::spawn(move || {
        let mut slept_ms = 0;
        for at_ms in REDRAW_AFTER_RESTORE_MS {
            std::thread::sleep(Duration::from_millis(at_ms - slept_ms));
            slept_ms = at_ms;
            if is_minimised() {
                return;
            }
            let mut guard = ctx.display.lock().unwrap_or_else(|e| e.into_inner());
            let Some(display) = guard.as_mut() else {
                return;
            };
            let presented = fit_and_render(display, &ctx, latest_size());
            log::info!("[viewer] redraw {at_ms} ms after restore, frame presented: {presented}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::mpsc::channel;
    use std::thread::JoinHandle;

    /// Stands in for `GpuContext`: all that matters here is the lock around its
    /// display, and the display is the width it was last fitted to.
    type Context = Arc<Mutex<u32>>;

    fn display_lock(context: &Context) -> &Mutex<u32> {
        context
    }

    /// The viewer's locks, and a resize taking the path the real one takes.
    struct Viewer {
        contexts: Mutex<Option<Context>>,
        display: Context,
        newest: AtomicU64,
        waiting: WaitingFit,
        threads: Mutex<Vec<JoinHandle<()>>>,
    }

    impl Viewer {
        fn new() -> Arc<Self> {
            let display = Arc::new(Mutex::new(0));
            Arc::new(Self {
                contexts: Mutex::new(Some(Arc::clone(&display))),
                display,
                newest: AtomicU64::new(0),
                waiting: WaitingFit::new(),
                threads: Mutex::new(Vec::new()),
            })
        }

        fn resize(self: &Arc<Self>, width: u32) {
            self.newest.store(u64::from(width), Ordering::SeqCst);
            fit_now(
                &self.contexts,
                display_lock,
                || self.fit_once_free(),
                |_, display| *display = width,
            );
        }

        fn fit_once_free(self: &Arc<Self>) {
            if !self.waiting.ask(false) {
                return;
            }
            let viewer = Arc::clone(self);
            let thread = std::thread::spawn(move || {
                let newest = || PhysicalSize::new(viewer.newest.load(Ordering::SeqCst) as u32, 1);
                viewer.waiting.run(
                    &viewer.contexts,
                    display_lock,
                    newest,
                    |_, display, size, _| *display = size.width,
                );
            });
            self.threads.lock().expect("threads").push(thread);
        }

        /// How many threads had to wait, once they are all done.
        fn join(&self) -> usize {
            let threads = std::mem::take(&mut *self.threads.lock().expect("threads"));
            let count = threads.len();
            for thread in threads {
                thread.join().expect("the fit");
            }
            count
        }

        fn fitted_width(&self) -> u32 {
            *self.display.lock().expect("display")
        }
    }

    fn any_size() -> PhysicalSize<u32> {
        PhysicalSize::new(1, 1)
    }

    /// The freeze, in miniature: a thread making the context holds the lock until
    /// the event thread answers it, and the event thread is handling a resize.
    #[test]
    fn a_resize_comes_back_to_answer_whoever_holds_the_context() {
        let lock = Arc::new(Mutex::new(Some(1)));
        let (held, is_held) = channel();
        let (answer, answered) = channel::<()>();
        let holder = {
            let lock = Arc::clone(&lock);
            std::thread::spawn(move || {
                let guard = lock.lock().expect("lock");
                held.send(()).expect("held");
                answered
                    .recv_timeout(Duration::from_secs(3))
                    .expect("the event thread never came back");
                drop(guard);
            })
        };
        is_held.recv().expect("held");

        let mut busy = false;
        let got_the_lock = lock_now(&lock, || busy = true).is_some();
        let _ = answer.send(());

        holder.join().expect("the holder gave up waiting");
        assert!(!got_the_lock);
        assert!(busy);
    }

    #[test]
    fn free_locks_fit_at_once() {
        let viewer = Viewer::new();
        let fitted = fit_now(
            &viewer.contexts,
            display_lock,
            || panic!("not busy"),
            |_, display| *display = 7,
        );
        assert!(fitted.is_some());
        assert_eq!(viewer.fitted_width(), 7);
    }

    #[test]
    fn no_context_needs_no_fit_now() {
        let contexts = Mutex::new(None::<Context>);
        let fitted = fit_now(
            &contexts,
            display_lock,
            || panic!("not busy"),
            |_, _| panic!("no context"),
        );
        assert!(fitted.is_none());
    }

    /// A drag ends on one last size, and it is the one that has to land.
    #[test]
    fn the_last_size_of_a_drag_is_the_one_fitted() {
        let viewer = Viewer::new();
        let making_a_context = viewer.contexts.lock().expect("contexts");
        for width in 1..=50 {
            viewer.resize(width);
        }
        drop(making_a_context);

        assert_eq!(viewer.join(), 1);
        assert_eq!(viewer.fitted_width(), 50);
    }

    /// The same drag with a render holding the display instead. The first thread
    /// is left time to reach that lock, as the later sizes must not start another
    /// or be missed by it, and its size must be read once it holds the lock.
    #[test]
    fn the_last_size_of_a_drag_lands_after_a_render() {
        let viewer = Viewer::new();
        let render = viewer.display.lock().expect("display");
        viewer.resize(1);
        std::thread::sleep(Duration::from_millis(50));
        for width in 2..=50 {
            viewer.resize(width);
        }
        drop(render);

        let threads = viewer.join();
        assert_eq!(viewer.fitted_width(), 50);
        assert_eq!(threads, 1);
    }

    /// The event thread must leave a restore to the waiting thread: if it took
    /// the context back as well, both would schedule the redraws.
    #[test]
    fn a_restore_on_a_busy_display_is_left_to_the_waiting_thread() {
        let viewer = Viewer::new();
        let render = viewer.display.lock().expect("display");
        let fitted = fit_now(
            &viewer.contexts,
            display_lock,
            || {
                viewer.waiting.ask(true);
            },
            |_, _| (),
        );
        drop(render);
        assert!(fitted.is_none());

        let mut fits_owing_redraws = 0;
        for _ in 0..2 {
            viewer.waiting.run(
                &viewer.contexts,
                display_lock,
                any_size,
                |_, _, _, restored| fits_owing_redraws += u32::from(restored),
            );
        }
        assert_eq!(fits_owing_redraws, 1);
    }

    #[test]
    fn a_resize_after_the_fit_starts_another() {
        let viewer = Viewer::new();
        let waiting = &viewer.waiting;
        assert!(waiting.ask(false));
        assert!(!waiting.ask(false));
        waiting.run(&viewer.contexts, display_lock, any_size, |_, _, _, _| {});
        assert!(waiting.ask(false));
    }

    /// A restore among the resizes still owes its redraws, once.
    #[test]
    fn a_restore_while_waiting_is_carried_to_the_fit() {
        let viewer = Viewer::new();
        let waiting = &viewer.waiting;
        waiting.ask(false);
        waiting.ask(true);
        waiting.run(
            &viewer.contexts,
            display_lock,
            any_size,
            |_, _, _, restored| assert!(restored),
        );
        waiting.ask(false);
        waiting.run(
            &viewer.contexts,
            display_lock,
            any_size,
            |_, _, _, restored| assert!(!restored),
        );
    }

    #[test]
    fn no_context_means_nothing_to_fit() {
        let contexts = Mutex::new(None::<Context>);
        let waiting = WaitingFit::new();
        waiting.ask(false);
        waiting.run(&contexts, display_lock, any_size, |_, _, _, _| {
            panic!("no context")
        });
        assert!(waiting.ask(false));
    }
}
