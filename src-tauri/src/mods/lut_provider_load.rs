//! The queue of mods/lut_provider.rs under the load of a real session: Preview
//! all, a click or two on a look, then on to the next photo, which calls off
//! the previews still on their way. The provider is the tests' fake, whose run
//! takes 0.1 to 2 s by its look's number and is timed in its `timed.log`. Ours.
//!
//! The numbers: `cargo test --lib lut_provider::load::numbers -- --ignored --nocapture`,
//! with `LOAD_DETAIL=1` for the log's line per run and each slow click's runs.

use std::sync::mpsc::{Receiver, channel};

use super::tests::{ROOMY, fake, numbered, preview_args, subject};
use super::*;

const LOOKS: u64 = 12;
const PHOTOS: usize = 8;
/// What a user waits for a look before calling it a hang.
const PATIENCE: Duration = Duration::from_secs(30);

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock")
        .as_millis()
}

/// A seeded die, so a session can be played again.
struct Dice(u64);

impl Dice {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

/// One run of the fake, from its log.
struct Run {
    photo: String,
    look: String,
    start: u128,
    end: u128,
}

fn runs(exe: &Path) -> Vec<Run> {
    let log = std::fs::read_to_string(exe.with_file_name("timed.log")).unwrap_or_default();
    log.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            Some(Run {
                photo: parts.next()?.to_string(),
                look: parts.next()?.to_string(),
                start: parts.next()?.parse().ok()?,
                end: parts.next()?.parse().ok()?,
            })
        })
        .collect()
}

/// A click on a look, and what came of it.
struct Click {
    photo: String,
    look: String,
    job: u64,
    asked: u128,
    /// Another click on its photo came after it.
    replaced: bool,
}

/// What a session measured.
#[derive(Default)]
pub(super) struct Numbers {
    /// Of each click that was still wanted: from the click to its run's start,
    /// or nothing when it shared a run or found its file.
    pub waits_ms: Vec<u128>,
    /// From the click to its answer.
    pub answers_ms: Vec<u128>,
    /// From its run's end to its answer, for a click that started one.
    pub after_run_ms: Vec<u128>,
    /// Answers that were not a look file, for a click or a preview.
    pub errors: usize,
    /// Requests never answered.
    pub lost: usize,
    /// From calling off Preview all to the last of its runs giving its place up.
    pub called_off_ms: Vec<u128>,
}

/// Everything heard so far, with when.
struct Ears {
    told: Receiver<(LookProgress, u128)>,
    heard: Vec<(LookProgress, u128)>,
}

impl Ears {
    fn listen(&mut self, wait: Duration) {
        if let Ok(one) = self.told.recv_timeout(wait) {
            self.heard.push(one);
        }
        while let Ok(one) = self.told.try_recv() {
            self.heard.push(one);
        }
    }

    fn pause(&mut self, ms: u64) {
        let until = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < until {
            self.listen(until.saturating_duration_since(Instant::now()));
        }
    }

    fn answer(&self, job: u64) -> Option<&(LookProgress, u128)> {
        self.heard
            .iter()
            .find(|(p, _)| p.job_id == job && !p.preview)
    }

    fn wait_for(&mut self, job: u64) {
        let until = Instant::now() + PATIENCE;
        while self.answer(job).is_none() && Instant::now() < until {
            self.listen(Duration::from_millis(20));
        }
    }
}

/// How long the runs called off keep their places.
fn called_off(queue: &Queue) -> u128 {
    let from = Instant::now();
    while queue
        .state()
        .running
        .iter()
        .any(|item| item.cancel.load(Ordering::SeqCst))
    {
        std::thread::sleep(Duration::from_millis(1));
    }
    from.elapsed().as_millis()
}

/// Windows scans a program the first time it runs, for a second or more: a
/// session measures the queue, not that.
fn warmed(kind: &str) -> super::tests::Fake {
    let exe = fake(kind);
    let never = AtomicBool::new(false);
    run(&exe, "list", b"{}", ROOMY.list, &never).expect("a list");
    exe
}

/// A queue for the timed fake, and a report that keeps when it heard.
fn timed_queue(exe: &Path) -> (Arc<Queue>, Report, Receiver<(LookProgress, u128)>) {
    let queue = Queue::new(
        exe.to_path_buf(),
        exe.with_file_name("scratch"),
        ROOMY,
        LookOptions::new(),
        Box::new(|_, _, _| {}),
    );
    let (tell, told) = channel();
    let tell = Mutex::new(tell);
    let report: Report = Arc::new(move |progress| {
        let _ = lock(&tell).send((progress, now_ms()));
    });
    (queue, report, told)
}

/// Preview all of `LOOKS` looks on `photo`.
fn preview(queue: &Arc<Queue>, photo: &Path, job: u64, report: &Report) {
    queue.preview_all(
        &numbered(LOOKS as usize),
        Some(subject(&photo.to_string_lossy())),
        &preview_args(photo),
        job,
        Arc::clone(report),
    );
}

/// Going to the next photo calls off the previews still running, and what
/// is asked for there must not wait for them to go.
#[test]
fn called_off_previews_give_their_places_up_at_once() {
    // Killing through `taskkill` took 0.5 s or more, idle or not; a starved
    // test machine can slow any one try, so the best of three counts.
    let quick = |ms: &u128| *ms < 300;
    let mut tries = Vec::new();
    while tries.len() < 3 && !tries.iter().any(quick) {
        tries.push(called_off_once());
    }
    assert!(
        tries.iter().any(quick),
        "the runs called off kept their places {tries:?} ms"
    );
}

/// How long four running previews, called off, keep their places.
fn called_off_once() -> u128 {
    let exe = warmed("timed");
    let (queue, report, _told) = timed_queue(&exe);
    preview(&queue, &exe.with_file_name("A.CR3"), 1, &report);
    // Looks 1 to 4 take 0.4 to 1.4 s: once all four programs have read their
    // request (the fake counts each call, the warming one too), all are running.
    super::tests::wait_for("four runs", || super::tests::calls(&exe) >= 5);
    queue.cancel(1);
    called_off(&queue)
}

/// A session of `PHOTOS` photos, played by `seed`.
pub(super) fn session(seed: u64) -> Numbers {
    let exe = warmed("timed");
    let (queue, report, told) = timed_queue(&exe);
    let mut ears = Ears {
        told,
        heard: Vec::new(),
    };
    let mut dice = Dice(seed);
    let mut numbers = Numbers::default();
    let mut clicks: Vec<Click> = Vec::new();
    let mut previews = Vec::new();
    let mut job = 0;

    for n in 0..PHOTOS {
        let name = format!("P{seed}_{n}.CR3");
        let photo = exe.with_file_name(&name);
        job += 1;
        let previewing = job;
        previews.push(previewing);
        preview(&queue, &photo, previewing, &report);
        ears.pause(dice.below(1500));
        for again in 0..=dice.below(2) {
            if again > 0 {
                ears.pause(dice.below(400));
                if let Some(last) = clicks.last_mut() {
                    last.replaced = true;
                }
            }
            job += 1;
            let look = format!("look{}", 1 + dice.below(LOOKS));
            clicks.push(Click {
                photo: name.clone(),
                look: look.clone(),
                job,
                asked: now_ms(),
                replaced: false,
            });
            queue.request(Request {
                subject: subject(&photo.to_string_lossy()),
                camera: "cam1".to_string(),
                look,
                options: LookOptions::new(),
                own_wb: None,
                remake: false,
                front: true,
                preview: false,
                job,
                report: Arc::clone(&report),
            });
        }
        ears.wait_for(job);
        ears.pause(dice.below(800));
        // On to the next photo: its Preview all is called off.
        queue.cancel(previewing);
        numbers.called_off_ms.push(called_off(&queue));
    }
    let until = Instant::now() + PATIENCE;
    while queue.is_busy() && Instant::now() < until {
        ears.listen(Duration::from_millis(20));
    }
    ears.pause(100);

    let runs = runs(&exe);
    for click in clicks.iter().filter(|click| !click.replaced) {
        let Some((progress, at)) = ears.answer(click.job) else {
            numbers.lost += 1;
            continue;
        };
        numbers.answers_ms.push(at - click.asked);
        // Its own run, if it had to start one.
        let own = runs
            .iter()
            .filter(|r| r.photo == click.photo && r.look == click.look && r.start >= click.asked)
            .min_by_key(|r| r.start);
        numbers
            .waits_ms
            .push(own.map_or(0, |run| run.start - click.asked));
        if let Some(run) = own {
            numbers.after_run_ms.push(at.saturating_sub(run.end));
            if run.start - click.asked > 1000 && std::env::var_os("LOAD_DETAIL").is_some() {
                println!(
                    "slow: {} {} waited {} ms",
                    click.photo,
                    click.look,
                    run.start - click.asked
                );
                for r in runs
                    .iter()
                    .filter(|r| r.end + 3000 > click.asked && r.start < run.end)
                {
                    let from = |t: u128| t as i128 - click.asked as i128;
                    println!(
                        "  {} {}: {}..{}",
                        r.photo,
                        r.look,
                        from(r.start),
                        from(r.end)
                    );
                }
            }
        }
        if progress.outcome != LookOutcome::Done {
            numbers.errors += 1;
        }
    }
    for click in clicks.iter().filter(|click| click.replaced) {
        if ears.answer(click.job).is_none() {
            numbers.lost += 1;
        }
    }
    for preview in previews {
        let answers: Vec<_> = ears
            .heard
            .iter()
            .filter(|(p, _)| p.job_id == preview && p.preview)
            .collect();
        numbers.lost += LOOKS as usize - answers.len().min(LOOKS as usize);
        numbers.errors += answers
            .iter()
            .filter(|(p, _)| !matches!(p.outcome, LookOutcome::Done | LookOutcome::Cancelled))
            .count();
    }
    numbers
}

fn percentile(values: &[u128], p: f64) -> u128 {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let at = ((p * sorted.len() as f64).ceil() as usize).max(1) - 1;
    sorted.get(at).copied().unwrap_or(0)
}

/// Three sessions, one after another, and what they add up to.
#[test]
#[ignore = "about two minutes; run by hand for the numbers"]
fn numbers() {
    let mut all = Numbers::default();
    for seed in [1, 2, 3] {
        let one = session(seed);
        all.waits_ms.extend(one.waits_ms);
        all.answers_ms.extend(one.answers_ms);
        all.after_run_ms.extend(one.after_run_ms);
        all.errors += one.errors;
        all.lost += one.lost;
        all.called_off_ms.extend(one.called_off_ms);
    }
    println!(
        "clicks {}: wait p50 {} ms, p95 {} ms, max {} ms; answer p50 {} ms, p95 {} ms",
        all.waits_ms.len(),
        percentile(&all.waits_ms, 0.5),
        percentile(&all.waits_ms, 0.95),
        percentile(&all.waits_ms, 1.0),
        percentile(&all.answers_ms, 0.5),
        percentile(&all.answers_ms, 0.95),
    );
    println!(
        "after its run: p50 {} ms, p95 {} ms",
        percentile(&all.after_run_ms, 0.5),
        percentile(&all.after_run_ms, 0.95),
    );
    println!(
        "called off {}: places held p50 {} ms, p95 {} ms",
        all.called_off_ms.len(),
        percentile(&all.called_off_ms, 0.5),
        percentile(&all.called_off_ms, 0.95),
    );
    println!("errors {}, lost {}", all.errors, all.lost);
}
