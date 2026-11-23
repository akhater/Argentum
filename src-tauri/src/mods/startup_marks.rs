//! How long start-up takes, step by step. One line in the log.
//!
//! `lib.rs` calls `begin` at the top of `run_app` and `mark` after each step
//! of its setup, up to the window being built; `report` writes the one line.
//! A step's time is what passed since the step before it. Nothing is stored
//! or sent anywhere; it is there so the number exists (CLAUDE.md, rule 5).

use std::sync::Mutex;
use std::time::Instant;

/// Steps in the order they were marked, each with the milliseconds it took.
pub struct Marks {
    last: Instant,
    steps: Vec<(&'static str, f64)>,
}

impl Marks {
    pub fn new() -> Self {
        Self {
            last: Instant::now(),
            steps: Vec::new(),
        }
    }

    /// Ends the step called `name`: everything since the previous mark.
    pub fn mark(&mut self, name: &'static str) {
        let now = Instant::now();
        let ms = now.duration_since(self.last).as_secs_f64() * 1000.0;
        self.last = now;
        self.steps.push((name, ms));
    }

    /// `startup: total 123 ms (args 1, init 20, ...)`.
    pub fn summary(&self) -> String {
        let total: f64 = self.steps.iter().map(|(_, ms)| ms).sum();
        let steps: Vec<String> = self
            .steps
            .iter()
            .map(|(name, ms)| format!("{name} {ms:.0}"))
            .collect();
        format!("startup: total {total:.0} ms ({})", steps.join(", "))
    }
}

static MARKS: Mutex<Option<Marks>> = Mutex::new(None);

/// Starts the clock. Called first thing in `run_app`.
pub fn begin() {
    if let Ok(mut marks) = MARKS.lock() {
        *marks = Some(Marks::new());
    }
}

/// Ends the step `name`. Does nothing before `begin`.
pub fn mark(name: &'static str) {
    if let Ok(mut marks) = MARKS.lock()
        && let Some(marks) = marks.as_mut()
    {
        marks.mark(name);
    }
}

/// Writes the one line, once. Called when the window is built, after logging
/// is set up, so the line is not lost.
pub fn report() {
    if let Ok(mut marks) = MARKS.lock()
        && let Some(done) = marks.take()
    {
        log::info!("{}", done.summary());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_come_out_in_order_and_the_summary_names_every_step() {
        let mut marks = Marks::new();
        for name in ["args", "init", "lens database", "window built"] {
            std::thread::sleep(std::time::Duration::from_millis(2));
            marks.mark(name);
        }
        let summary = marks.summary();
        let at: Vec<usize> = ["args", "init", "lens database", "window built"]
            .iter()
            .map(|name| summary.find(name).expect("names every step"))
            .collect();
        assert!(at.windows(2).all(|pair| pair[0] < pair[1]), "{summary}");
        assert!(summary.starts_with("startup: total "), "{summary}");
        assert!(marks.steps.iter().all(|(_, ms)| *ms >= 2.0));
    }

    #[test]
    fn marking_before_begin_or_reporting_twice_does_nothing() {
        mark("early");
        report();
        report();
    }
}
