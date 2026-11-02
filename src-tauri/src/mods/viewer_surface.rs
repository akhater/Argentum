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
use std::sync::{Mutex, TryLockError};
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
    let Some(ctx) = context_now(&state.gpu_context, || {
        fit_once_free(window.app_handle(), restored)
    }) else {
        return;
    };

    // Never wait here: this is the event thread, and a render may hold the lock.
    let presented = match ctx.display.try_lock() {
        Ok(mut guard) => guard.as_mut().map(|d| fit_and_render(d, &ctx, size)),
        Err(TryLockError::Poisoned(poisoned)) => poisoned
            .into_inner()
            .as_mut()
            .map(|d| fit_and_render(d, &ctx, size)),
        Err(TryLockError::WouldBlock) => None,
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

/// The context, or None with `busy` called if something holds its lock.
///
/// Never wait for this lock on the event thread: a thread making a new context
/// holds it while it asks the event thread for the window, see mods/ui_thread.rs.
fn context_now<C: Clone>(lock: &Mutex<Option<C>>, busy: impl FnOnce()) -> Option<C> {
    match lock.try_lock() {
        Ok(guard) => guard.clone(),
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner().clone(),
        Err(TryLockError::WouldBlock) => {
            busy();
            None
        }
    }
}

/// Fits the surface once the context lock is free, off the event thread.
fn fit_once_free(app: &tauri::AppHandle, restored: bool) {
    static WAITING: WaitingFit = WaitingFit::new();
    if !WAITING.ask(restored) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        WAITING.run(&state.gpu_context, latest_size, |ctx, size, restored| {
            if is_minimised() {
                return;
            }
            if let Some(display) = ctx
                .display
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_mut()
            {
                fit_and_render(display, &ctx, size);
            }
            if restored {
                redraw_after_restore(ctx);
            }
        });
    });
}

/// A fit that could not run because the context lock was busy. However many
/// resizes arrive meanwhile, one thread waits, and it fits to the latest size.
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

    /// Waits for the lock, then fits. The next `ask` starts a thread of its own,
    /// and is taken before the size is read, so no resize falls between the two.
    fn run<C: Clone>(
        &self,
        lock: &Mutex<Option<C>>,
        size: impl Fn() -> PhysicalSize<u32>,
        fit: impl FnOnce(C, PhysicalSize<u32>, bool),
    ) {
        let context = lock.lock().unwrap_or_else(|e| e.into_inner()).clone();
        self.waiting.store(false, Ordering::SeqCst);
        let restored = self.restored.swap(false, Ordering::SeqCst);
        if let Some(context) = context {
            fit(context, size(), restored);
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
        let context = context_now(&lock, || busy = true);
        let _ = answer.send(());

        holder.join().expect("the holder gave up waiting");
        assert_eq!(context, None);
        assert!(busy);
    }

    #[test]
    fn a_free_lock_gives_the_context_at_once() {
        let lock = Mutex::new(Some(7));
        assert_eq!(context_now(&lock, || panic!("not busy")), Some(7));
        assert_eq!(
            context_now(&Mutex::new(None::<u8>), || panic!("not busy")),
            None
        );
    }

    /// A drag ends on one last size, and it is the one that has to land.
    #[test]
    fn the_last_size_of_a_drag_is_the_one_fitted() {
        let lock = Arc::new(Mutex::new(Some(1)));
        let newest = Arc::new(AtomicU64::new(0));
        let waiting = Arc::new(WaitingFit::new());
        let (fitted, fits) = channel();
        let guard = lock.lock().expect("lock");

        let mut threads = 0;
        for width in 1..=50 {
            newest.store(width, Ordering::SeqCst);
            if waiting.ask(false) {
                threads += 1;
                let (lock, newest, waiting, fitted) = (
                    Arc::clone(&lock),
                    Arc::clone(&newest),
                    Arc::clone(&waiting),
                    fitted.clone(),
                );
                std::thread::spawn(move || {
                    let size = || PhysicalSize::new(newest.load(Ordering::SeqCst) as u32, 1);
                    waiting.run(&lock, size, |_, size, _| {
                        fitted.send(size.width).expect("fit")
                    });
                });
            }
        }
        drop(guard);
        drop(fitted);

        assert_eq!(threads, 1);
        assert_eq!(fits.iter().collect::<Vec<_>>(), [50]);
    }

    #[test]
    fn a_resize_after_the_fit_starts_another() {
        let lock = Mutex::new(Some(1));
        let waiting = WaitingFit::new();
        assert!(waiting.ask(false));
        assert!(!waiting.ask(false));
        waiting.run(&lock, || PhysicalSize::new(1, 1), |_, _, _| {});
        assert!(waiting.ask(false));
    }

    /// A restore among the resizes still owes its redraws, once.
    #[test]
    fn a_restore_while_waiting_is_carried_to_the_fit() {
        let lock = Mutex::new(Some(1));
        let waiting = WaitingFit::new();
        let size = || PhysicalSize::new(1, 1);
        waiting.ask(false);
        waiting.ask(true);
        waiting.run(&lock, size, |_, _, restored| assert!(restored));
        waiting.ask(false);
        waiting.run(&lock, size, |_, _, restored| assert!(!restored));
    }

    #[test]
    fn no_context_means_nothing_to_fit() {
        let lock = Mutex::new(None::<u8>);
        let waiting = WaitingFit::new();
        waiting.ask(false);
        waiting.run(
            &lock,
            || PhysicalSize::new(1, 1),
            |_, _, _| panic!("no context"),
        );
        assert!(waiting.ask(false));
    }
}
