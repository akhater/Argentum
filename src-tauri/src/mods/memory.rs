//! What Argentum holds in memory, and handing back what nothing is using.
//!
//! RapidRAW loads an AI model the first time a tool needs it and keeps it until
//! the app quits. Measured on 2026-10-09 with a Canon R6 Mark III (6960x4640):
//! one AI erase took the process from 2.6 GB to 12 GB, and it settled at 8.8 GB,
//! where it stayed after switching to another photo, which empties every
//! per-photo cache. What survives a photo change is not a photo.
//!
//! So one thread, started from `startup::init`, does three things:
//!
//! - **Unloads idle AI models.** A model is in use while anything other than
//!   `AiState` holds it: every caller clones the `Arc` out of their
//!   `get_or_init_*` and keeps it for the length of the job. Unused for long
//!   enough, it is taken out of `AiState`, and their `get_or_init_*` loads it
//!   again on the next click. That costs a few seconds, which is why the mask
//!   models, clicked again and again while working, are given longer.
//! - **Hands freed memory back to Windows.** mimalloc keeps freed pages for
//!   reuse; `mi_collect(true)` returns them.
//! - **Says where the memory is**, in the log, whenever the total moves: the
//!   photo copies, the masks, which models are loaded, and what is left over.
//!   The leftover is the part nothing here can name, and it is the number to
//!   watch.
//!
//! Every lock is a `try_lock`. A busy cache is skipped for a second, never
//! waited on: this thread must not be the reason an edit stutters.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

use image::DynamicImage;
use tauri::{AppHandle, Manager};

use crate::app_state::AppState;

const TICK: Duration = Duration::from_secs(1);

/// The eraser, denoise and tagging run once per photo. Unload them soon.
const IDLE_ONE_SHOT: Duration = Duration::from_secs(60);
/// The mask models are clicked again and again while working on one photo,
/// and reloading all five takes a few seconds. Unload them only after a pause.
const IDLE_MASKS: Duration = Duration::from_secs(300);

/// Ask the allocator to hand freed memory back this often.
const COLLECT_EVERY: Duration = Duration::from_secs(20);
/// Below this, handing back is not worth a log line.
const COLLECT_WORTH_SAYING: usize = 64 << 20;
/// Report where the memory is when the total has moved this far.
const REPORT_STEP: usize = 256 << 20;

pub fn watch(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("ag-memory".into())
        .spawn(move || run(app));
    if let Err(e) = spawned {
        log::warn!("[memory] watcher did not start: {e}");
    }
}

fn run(app: AppHandle) {
    let mut models = Models::default();
    let mut last_collect = Instant::now();
    let mut last_report = 0usize;

    loop {
        std::thread::sleep(TICK);
        let state = app.state::<AppState>();
        let now = Instant::now();

        let unloaded = models.unload_idle(&state, now);
        if !unloaded.is_empty() {
            let before = committed();
            collect();
            log::info!(
                "[memory] unloaded the idle AI {}: {} -> {}",
                unloaded.join(", "),
                gb(before),
                gb(committed())
            );
            last_collect = now;
        } else if now.duration_since(last_collect) >= COLLECT_EVERY {
            let before = committed();
            collect();
            let after = committed();
            if before.saturating_sub(after) >= COLLECT_WORTH_SAYING {
                log::info!(
                    "[memory] handed {} of freed memory back: {} -> {}",
                    gb(before - after),
                    gb(before),
                    gb(after)
                );
            }
            last_collect = now;
        }

        let total = committed();
        if total.abs_diff(last_report) >= REPORT_STEP {
            log::info!("[memory] {}", Held::read(&state).describe(total));
            last_report = total;
        }
    }
}

/// The process's committed memory, the number Task Manager calls "Memory".
fn committed() -> usize {
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
    v[5]
}

fn collect() {
    // SAFETY: takes no pointers; force-collects this thread's heap and purges
    // every arena's freed pages.
    unsafe { libmimalloc_sys::mi_collect(true) }
}

fn gb(bytes: usize) -> String {
    format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
}

/// Unused for this long or more: unload.
#[derive(Default)]
struct Idle {
    busy_at: Option<Instant>,
}

impl Idle {
    /// `loaded`: the model is in `AiState`. `in_use`: someone besides
    /// `AiState` holds it. True when it has been loaded and unused for `limit`.
    ///
    /// A model seen for the first time counts as just used, so it is never
    /// unloaded the moment it appears, between its load and its first job.
    fn due(&mut self, loaded: bool, in_use: bool, now: Instant, limit: Duration) -> bool {
        if !loaded {
            self.busy_at = None;
            return false;
        }
        match self.busy_at {
            Some(at) if !in_use => now.duration_since(at) >= limit,
            _ => {
                self.busy_at = Some(now);
                false
            }
        }
    }
}

#[derive(Default)]
struct Models {
    eraser: Idle,
    masks: Idle,
    denoise: Idle,
    tagging: Idle,
}

fn in_use<T: ?Sized>(slot: &Option<Arc<T>>) -> bool {
    slot.as_ref().is_some_and(|a| Arc::strong_count(a) > 1)
}

impl Models {
    /// Takes every model that is due out of `AiState`, and drops them after
    /// the lock is released: freeing a model's memory takes a moment, and their
    /// threads wait on this lock.
    fn unload_idle(&mut self, state: &AppState, now: Instant) -> Vec<&'static str> {
        let mut taken: Vec<Box<dyn Send>> = Vec::new();
        let mut names = Vec::new();
        {
            let mut guard = match state.ai_state.try_lock() {
                Ok(g) => g,
                Err(TryLockError::WouldBlock) => return names,
                Err(TryLockError::Poisoned(_)) => return names,
            };
            let Some(ai) = guard.as_mut() else {
                *self = Models::default();
                return names;
            };

            if self.eraser.due(
                ai.lama_model.is_some(),
                in_use(&ai.lama_model),
                now,
                IDLE_ONE_SHOT,
            ) && let Some(m) = ai.lama_model.take()
            {
                taken.push(Box::new(m));
                names.push("eraser");
            }
            if self.denoise.due(
                ai.denoise_model.is_some(),
                in_use(&ai.denoise_model),
                now,
                IDLE_ONE_SHOT,
            ) && let Some(m) = ai.denoise_model.take()
            {
                taken.push(Box::new(m));
                names.push("denoise");
            }
            if self.tagging.due(
                ai.clip_models.is_some(),
                in_use(&ai.clip_models),
                now,
                IDLE_ONE_SHOT,
            ) && let Some(m) = ai.clip_models.take()
            {
                taken.push(Box::new(m));
                names.push("tagging");
            }
            if self
                .masks
                .due(ai.models.is_some(), in_use(&ai.models), now, IDLE_MASKS)
                && let Some(m) = ai.models.take()
            {
                taken.push(Box::new(m));
                names.push("mask models");
            }
        }
        drop(taken);
        names
    }

    fn loaded(state: &AppState) -> Option<Vec<&'static str>> {
        let guard = state.ai_state.try_lock().ok()?;
        let mut names = Vec::new();
        if let Some(ai) = guard.as_ref() {
            if ai.lama_model.is_some() {
                names.push("eraser");
            }
            if ai.models.is_some() {
                names.push("masks");
            }
            if ai.denoise_model.is_some() {
                names.push("denoise");
            }
            if ai.clip_models.is_some() {
                names.push("tagging");
            }
        }
        Some(names)
    }
}

/// What the caches we can see are holding, in bytes.
#[derive(Default)]
struct Held {
    photo: usize,
    photo_copies: usize,
    masks: usize,
    mask_count: usize,
    results: usize,
    models: Option<Vec<&'static str>>,
}

impl Held {
    fn read(state: &AppState) -> Self {
        let mut held = Held::default();
        let mut seen = HashSet::new();
        let mut add = |held: &mut Held, img: &Arc<DynamicImage>| {
            if seen.insert(Arc::as_ptr(img) as usize) {
                held.photo += img.as_bytes().len();
                held.photo_copies += 1;
            }
        };

        if let Some(g) = peek(&state.original_image)
            && let Some(loaded) = g.as_ref()
        {
            add(&mut held, &loaded.image);
        }
        if let Some(g) = peek(&state.cached_preview)
            && let Some(p) = g.as_ref()
        {
            add(&mut held, &p.image);
            add(&mut held, &p.small_image);
        }
        if let Some(g) = peek(&state.full_warped_cache)
            && let Some((_, img)) = g.as_ref()
        {
            add(&mut held, img);
        }
        if let Some(g) = peek(&state.patched_warped_cache)
            && let Some((_, img)) = g.as_ref()
        {
            add(&mut held, img);
        }
        if let Some(g) = peek(&state.full_transformed_cache)
            && let Some((_, img, _)) = g.as_ref()
        {
            add(&mut held, img);
        }
        if let Some(g) = peek(&state.thumbnail_geometry_cache) {
            for (_, img, _) in g.values() {
                add(&mut held, img);
            }
        }
        if let Some(g) = peek(&state.geometry_cache) {
            for img in g.values() {
                held.photo += img.as_bytes().len();
                held.photo_copies += 1;
            }
        }
        if let Some(g) = peek(&state.mask_cache) {
            held.mask_count = g.len();
            held.masks = g.values().map(|m| m.as_raw().len()).sum();
        }
        for slot in [
            &state.hdr_result,
            &state.panorama_result,
            &state.focus_stack_result,
            &state.denoise_result,
        ] {
            if let Some(g) = peek(slot)
                && let Some(img) = g.as_ref()
            {
                held.results += img.as_bytes().len();
            }
        }
        held.results += super::super_resolution::result_bytes();
        held.models = Models::loaded(state);
        held
    }

    fn describe(&self, total: usize) -> String {
        let named = self.photo + self.masks + self.results;
        let models = match &self.models {
            Some(m) if m.is_empty() => "none".to_string(),
            Some(m) => m.join(", "),
            None => "busy".to_string(),
        };
        format!(
            "{} in use: open photo {} in {} copies, masks {} in {}, results {}, AI loaded: {}; \
             the rest {} (AI, the image cache's other photos, freed memory)",
            gb(total),
            gb(self.photo),
            self.photo_copies,
            gb(self.masks),
            self.mask_count,
            gb(self.results),
            models,
            gb(total.saturating_sub(named)),
        )
    }
}

fn peek<T>(m: &Mutex<T>) -> Option<std::sync::MutexGuard<'_, T>> {
    m.try_lock().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: Duration = Duration::from_secs(60);

    #[test]
    fn a_model_is_never_unloaded_the_moment_it_appears() {
        let t0 = Instant::now();
        let mut idle = Idle::default();
        assert!(!idle.due(true, false, t0 + LIMIT * 5, LIMIT));
    }

    #[test]
    fn unused_for_the_limit_is_unloaded() {
        let t0 = Instant::now();
        let mut idle = Idle::default();
        assert!(!idle.due(true, false, t0, LIMIT));
        assert!(!idle.due(true, false, t0 + LIMIT / 2, LIMIT));
        assert!(idle.due(true, false, t0 + LIMIT, LIMIT));
    }

    #[test]
    fn use_restarts_the_clock() {
        let t0 = Instant::now();
        let mut idle = Idle::default();
        idle.due(true, false, t0, LIMIT);
        assert!(!idle.due(true, true, t0 + LIMIT, LIMIT));
        assert!(!idle.due(true, false, t0 + LIMIT + LIMIT / 2, LIMIT));
        assert!(idle.due(true, false, t0 + LIMIT * 2, LIMIT));
    }

    #[test]
    fn a_model_in_use_is_never_unloaded() {
        let t0 = Instant::now();
        let mut idle = Idle::default();
        for s in 0..600 {
            assert!(!idle.due(true, true, t0 + Duration::from_secs(s), LIMIT));
        }
    }

    #[test]
    fn unloading_forgets_the_model() {
        let t0 = Instant::now();
        let mut idle = Idle::default();
        idle.due(true, false, t0, LIMIT);
        assert!(idle.due(true, false, t0 + LIMIT, LIMIT));
        // Gone, then loaded again: counts as just used.
        assert!(!idle.due(false, false, t0 + LIMIT * 2, LIMIT));
        assert!(!idle.due(true, false, t0 + LIMIT * 3, LIMIT));
    }

    #[test]
    fn in_use_means_held_by_someone_besides_ai_state() {
        let slot = Some(Arc::new(0u8));
        assert!(!in_use(&slot));
        let job = slot.clone();
        assert!(in_use(&slot));
        drop(job);
        assert!(!in_use(&slot));
        assert!(!in_use::<u8>(&None));
    }

    #[test]
    fn committed_memory_is_read() {
        assert!(committed() > 0);
    }
}
