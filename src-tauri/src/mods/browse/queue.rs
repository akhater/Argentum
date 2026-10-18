//! Photos waiting for a thumbnail, the most wanted first.
//!
//! The interface says which photos it wants, the visible ones first, each
//! time the grid scrolls or the folder changes, and that list replaces what
//! was waiting: latest wins. A photo already being worked on is not queued
//! again; its thumbnail is on its way.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard};

#[derive(Default)]
pub struct Queue {
    state: Mutex<State>,
    ready: Condvar,
}

#[derive(Default)]
struct State {
    waiting: VecDeque<PathBuf>,
    working: HashSet<PathBuf>,
}

impl Queue {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Wait for these, in this order, instead of whatever was waiting.
    pub fn want(&self, paths: Vec<PathBuf>) {
        let mut state = self.state();
        let mut seen = HashSet::new();
        let waiting = paths
            .into_iter()
            .filter(|p| !state.working.contains(p) && seen.insert(p.clone()))
            .collect();
        state.waiting = waiting;
        drop(state);
        self.ready.notify_all();
    }

    /// The next photo, once there is one. It counts as being worked on until
    /// `done`.
    pub fn take(&self) -> PathBuf {
        let mut state = self.state();
        loop {
            if let Some(path) = state.waiting.pop_front() {
                state.working.insert(path.clone());
                return path;
            }
            state = self.ready.wait(state).unwrap_or_else(|p| p.into_inner());
        }
    }

    pub fn done(&self, path: &Path) {
        self.state().working.remove(path);
    }

    /// Nothing waiting: a worker that finishes now has nothing to take.
    pub fn is_empty(&self) -> bool {
        self.state().waiting.is_empty()
    }

    pub fn waiting(&self) -> Vec<PathBuf> {
        self.state().waiting.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn the_latest_want_replaces_what_was_waiting() {
        let q = Queue::default();
        q.want(paths(&["a", "b", "c"]));
        assert_eq!(q.take(), PathBuf::from("a"));
        // The user scrolls: d and e are on screen now.
        q.want(paths(&["d", "e", "b"]));
        assert_eq!(q.waiting(), paths(&["d", "e", "b"]));
        assert_eq!(q.take(), PathBuf::from("d"));
    }

    #[test]
    fn a_photo_being_worked_on_or_named_twice_is_queued_once() {
        let q = Queue::default();
        q.want(paths(&["a", "b"]));
        let a = q.take();
        q.want(paths(&["a", "b", "b", "c"]));
        assert_eq!(q.waiting(), paths(&["b", "c"]));
        q.done(&a);
        q.want(paths(&["a"]));
        assert_eq!(q.waiting(), paths(&["a"]));
    }

    #[test]
    fn a_worker_waits_for_work() {
        let q = std::sync::Arc::new(Queue::default());
        let worker = {
            let q = q.clone();
            std::thread::spawn(move || q.take())
        };
        std::thread::sleep(std::time::Duration::from_millis(20));
        q.want(paths(&["late"]));
        assert_eq!(worker.join().unwrap(), PathBuf::from("late"));
    }
}
