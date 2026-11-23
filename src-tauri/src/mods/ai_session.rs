//! How every one of RapidRAW's AI model sessions is built: with ONNX Runtime's
//! CPU memory arena off.
//!
//! The arena keeps the largest working memory a model has ever needed for as
//! long as the session lives, so the next run need not ask for it again. Their
//! models live for the whole session, so their working memory did too. Measured
//! on 2026-10-09 with a Canon R6 Mark III raw: the five mask models (SAM's
//! encoder and decoder, U2-Net, the sky model, Depth Anything) took the app from
//! 3.3 GB to 8.5 GB after one selection, of which loading them was 1.4 GB, and
//! unloading them gave back 6.1 GB. Without the arena a run's working memory is
//! returned when the run ends, and a loaded model costs what its weights cost.
//!
//! The price is an allocation per tensor per run instead of reuse from the
//! pool. Against a model run that takes a second or more, that is noise.
//!
//! One import in their ai_processing.rs, on an anchor taken 2026-10-09: each of
//! their eight `Session::builder()?` became `session_builder()?`, one line for
//! one. Unloading models nothing is using stays in mods/memory.rs; this makes a
//! loaded one cheap, that makes an idle one free.

//!
//! Every session also goes through [`builder`], which first makes the process's
//! ONNX Runtime environment with telemetry off (`rm_selection`'s
//! `private_environment`). The environment is process-wide and the first one
//! made is the only one: a session built before it would have made `ort`'s
//! default, which reports to Microsoft. The test below keeps every session and
//! every environment in the app on this path.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, Once};
use std::time::{Duration, Instant};

use ort::execution_providers::CPUExecutionProvider;
use ort::session::Session;
use ort::session::builder::SessionBuilder;

/// Makes the process's ONNX Runtime environment, telemetry off, unless one is
/// already made (`Ok(false)`). Call it after `gpu_runtime` has chosen which
/// runtime loads: this loads it.
pub fn environment() -> ort::Result<bool> {
    rm_selection::runtime::private_environment(ort::init().with_name("Argentum")).commit()
}

/// `Session::builder()` on the private environment.
pub fn builder() -> ort::Result<SessionBuilder> {
    environment()?;
    Session::builder()
}

/// `Session::builder()`, with the CPU arena off.
pub fn session_builder() -> ort::Result<SessionBuilder> {
    builder()?.with_execution_providers([CPUExecutionProvider::default()
        .with_arena_allocator(false)
        .build()])
}

/// What a slot holds: the loaded value, the models folder it came from and
/// when it was last used.
struct Held<T> {
    dir: PathBuf,
    value: T,
    used: Instant,
}

/// One AI model's loaded sessions (`T`), kept between uses and let go once
/// nothing has asked for a while (mods/memory.rs's watcher calls
/// [`unload_idle`]). One `static` per model, made with [`IdleSlot::new`].
///
/// [`with`](Self::with) loads `T` from the models folder when the slot is empty
/// or holds another folder's, and runs the work with `T` locked: the lock is
/// held for the load and for the work, so concurrent first uses load once and
/// the watcher (a `try_lock`) never unloads a model in use.
pub struct IdleSlot<T> {
    /// What the memory log calls the models in this slot; slots with the same
    /// label are reported, and have their memory handed back, together.
    label: &'static str,
    held: Mutex<Option<Held<T>>>,
    registered: Once,
}

impl<T: Send + 'static> IdleSlot<T> {
    pub const fn new(label: &'static str) -> Self {
        Self {
            label,
            held: Mutex::new(None),
            registered: Once::new(),
        }
    }

    /// Run `work` on the value for the models folder `dir`, calling `load`
    /// first when it is not the one held. A value of another folder is
    /// dropped before the new one loads, and a failed load leaves the slot
    /// empty.
    pub fn with<R>(
        &'static self,
        dir: &Path,
        load: impl FnOnce() -> Result<T, String>,
        work: impl FnOnce(&mut T) -> Result<R, String>,
    ) -> Result<R, String> {
        self.registered.call_once(|| {
            registry().push(self);
        });
        self.with_at(Instant::now(), dir, load, work)
    }

    /// [`with`](Self::with) at the time `now`.
    fn with_at<R>(
        &self,
        now: Instant,
        dir: &Path,
        load: impl FnOnce() -> Result<T, String>,
        work: impl FnOnce(&mut T) -> Result<R, String>,
    ) -> Result<R, String> {
        let mut slot = self.held.lock().map_err(|e| e.to_string())?;
        let held = match slot.take() {
            Some(held) if held.dir == dir => held,
            other => {
                drop(other);
                Held {
                    dir: dir.to_path_buf(),
                    value: load()?,
                    used: now,
                }
            }
        };
        let held = slot.insert(held);
        held.used = now;
        work(&mut held.value)
    }

    /// Drop the value if nothing has used it for longer than `after`. Never
    /// waits: a slot in use (or poisoned) is left alone and reports `false`.
    fn unload_if_idle_at(&self, now: Instant, after: Duration) -> bool {
        let Ok(mut slot) = self.held.try_lock() else {
            return false;
        };
        if slot
            .as_ref()
            .is_some_and(|h| now.saturating_duration_since(h.used) > after)
        {
            *slot = None;
            return true;
        }
        false
    }
}

/// A slot as the watcher sees it, whatever its `T`.
trait Idle: Sync {
    fn label(&self) -> &'static str;
    fn unload_if_idle(&self, after: Duration) -> bool;
}

impl<T: Send + 'static> Idle for IdleSlot<T> {
    fn label(&self) -> &'static str {
        self.label
    }
    fn unload_if_idle(&self, after: Duration) -> bool {
        self.unload_if_idle_at(Instant::now(), after)
    }
}

/// Every slot that has been used, added by its first use: a slot never used
/// holds nothing to unload, and a new model's slot needs no edit here.
static SLOTS: Mutex<Vec<&'static dyn Idle>> = Mutex::new(Vec::new());

fn registry() -> MutexGuard<'static, Vec<&'static dyn Idle>> {
    // A list of references cannot be left half-written.
    SLOTS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Unload every slot idle for longer than `after`. Returns the labels of the
/// slots that let go, once each, for the caller to hand the memory back and
/// say so.
pub fn unload_idle(after: Duration) -> Vec<&'static str> {
    let slots: Vec<&'static dyn Idle> = registry().clone();
    let mut unloaded = Vec::new();
    for slot in slots {
        if slot.unload_if_idle(after) && !unloaded.contains(&slot.label()) {
            unloaded.push(slot.label());
        }
    }
    unloaded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A slot for a counter, which counts its own loads.
    fn counted(loads: &AtomicUsize) -> impl FnOnce() -> Result<usize, String> + '_ {
        move || Ok(loads.fetch_add(1, Ordering::SeqCst) + 1)
    }

    fn read(slot: &IdleSlot<usize>, at: Instant, dir: &str, loads: &AtomicUsize) -> usize {
        slot.with_at(at, Path::new(dir), counted(loads), |n| Ok(*n))
            .unwrap()
    }

    #[test]
    fn a_folder_loads_once_and_another_folder_reloads() {
        let slot = IdleSlot::<usize>::new("test");
        let loads = AtomicUsize::new(0);
        let t = Instant::now();
        assert_eq!(read(&slot, t, "a", &loads), 1);
        assert_eq!(read(&slot, t, "a", &loads), 1);
        assert_eq!(loads.load(Ordering::SeqCst), 1);
        assert_eq!(read(&slot, t, "b", &loads), 2);
        assert_eq!(read(&slot, t, "a", &loads), 3);
    }

    #[test]
    fn work_can_change_the_value_and_sees_it_next_time() {
        let slot = IdleSlot::<usize>::new("test");
        let t = Instant::now();
        slot.with_at(
            t,
            Path::new("a"),
            || Ok(10),
            |n| {
                *n += 1;
                Ok(())
            },
        )
        .unwrap();
        let seen = slot
            .with_at(t, Path::new("a"), || Ok(0), |n| Ok(*n))
            .unwrap();
        assert_eq!(seen, 11);
    }

    #[test]
    fn it_unloads_only_after_the_idle_time() {
        let slot = IdleSlot::<usize>::new("test");
        let loads = AtomicUsize::new(0);
        let t = Instant::now();
        read(&slot, t, "a", &loads);
        let after = Duration::from_secs(300);
        assert!(!slot.unload_if_idle_at(t + after, after), "not past it yet");
        assert!(slot.unload_if_idle_at(t + after + Duration::from_secs(1), after));
        assert!(
            !slot.unload_if_idle_at(t + after * 3, after),
            "already empty"
        );
        assert_eq!(read(&slot, t + after * 3, "a", &loads), 2, "loads again");
    }

    #[test]
    fn a_use_restarts_the_idle_time() {
        let slot = IdleSlot::<usize>::new("test");
        let loads = AtomicUsize::new(0);
        let t = Instant::now();
        let after = Duration::from_secs(300);
        read(&slot, t, "a", &loads);
        read(&slot, t + Duration::from_secs(200), "a", &loads);
        assert!(!slot.unload_if_idle_at(t + Duration::from_secs(400), after));
        assert!(slot.unload_if_idle_at(t + Duration::from_secs(501), after));
    }

    #[test]
    fn a_failed_load_leaves_the_slot_empty() {
        let slot = IdleSlot::<usize>::new("test");
        let loads = AtomicUsize::new(0);
        let t = Instant::now();
        read(&slot, t, "a", &loads);
        let failed = slot.with_at(t, Path::new("b"), || Err("no file".into()), |n| Ok(*n));
        assert_eq!(failed, Err("no file".to_string()));
        assert_eq!(read(&slot, t, "a", &loads), 2, "the old value was dropped");
    }

    #[test]
    fn a_slot_in_use_is_not_unloaded() {
        let slot = IdleSlot::<usize>::new("test");
        let t = Instant::now();
        slot.with_at(
            t,
            Path::new("a"),
            || Ok(1),
            |_| {
                assert!(!slot.unload_if_idle_at(t + Duration::from_secs(9), Duration::ZERO));
                Ok(())
            },
        )
        .unwrap();
    }

    #[test]
    fn concurrent_first_use_loads_once() {
        let slot = IdleSlot::<usize>::new("test");
        let loads = AtomicUsize::new(0);
        let t = Instant::now();
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| assert_eq!(read(&slot, t, "a", &loads), 1));
            }
        });
        assert_eq!(loads.load(Ordering::SeqCst), 1);
    }

    static REGISTERED: IdleSlot<usize> = IdleSlot::new("registered test models");

    #[test]
    fn a_used_slot_is_found_by_the_watcher() {
        REGISTERED
            .with(Path::new("a"), || Ok(1), |_| Ok(()))
            .unwrap();
        // Idle for longer than "never idle" is only true of a slot used before now.
        std::thread::sleep(Duration::from_millis(2));
        assert!(unload_idle(Duration::from_millis(1)).contains(&"registered test models"));
        assert!(!unload_idle(Duration::from_millis(1)).contains(&"registered test models"));
    }

    /// Every `.rs` file under `dir`.
    fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                sources(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// No ONNX Runtime environment or session in the app or in `rm_selection`
    /// can be made with telemetry on: environments are committed only through
    /// `private_environment`, and sessions are built only after one is.
    #[test]
    fn every_onnx_runtime_environment_is_made_with_telemetry_off() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        sources(&root.join("src"), &mut files);
        sources(&root.join("rm_selection").join("src"), &mut files);
        let init = concat!("ort::", "init");
        // Each of these makes `ort`'s default environment if none is made yet.
        let sessions = [
            concat!("Session::", "builder("),
            concat!("SessionBuilder::", "new("),
            concat!("get_", "environment("),
        ];
        let mut wrong = Vec::new();
        for file in &files {
            let text = fs::read_to_string(file).unwrap();
            let name = file
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            for (at, _) in text.match_indices(init) {
                let statement = text[at..].split(';').next().unwrap_or_default();
                let commits = statement.contains(".commit()");
                let private = text[..at].ends_with("private_environment(");
                if commits && !private {
                    wrong.push(format!("{name}: an environment committed without it"));
                }
            }
            // Here a session is built after `environment()`; in rm_selection
            // after `load_runtime`, which commits through `private_environment`.
            let allowed = ["src/mods/ai_session.rs", "rm_selection/src/runtime.rs"];
            let builds = sessions.iter().any(|s| text.contains(s));
            if builds && !allowed.contains(&name.as_str()) {
                wrong.push(format!("{name}: a session not built through `builder()`"));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
        let runtime = fs::read_to_string(root.join("rm_selection/src/runtime.rs")).unwrap();
        assert!(runtime.contains("builder.with_telemetry(false)"));
    }
}
