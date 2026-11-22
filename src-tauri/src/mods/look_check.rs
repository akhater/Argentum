//! The golden photos: real public RAWs through the app's own decode and
//! render, compared with small text references, so a change to how a photo
//! looks either fails or is logged with its reason. Ours.
//!
//! WHY
//!
//! The highlights and the exposure changed eight times in a week, each change
//! tested only for its own piece, and nothing in CI looked at a real RAW: the
//! tests that need one skip there. This renders ten CC0 RAWs from
//! raw.pixls.us, each picked for what it exercises (`tests/look/samples.tsv`),
//! exactly as the app does, and holds every result to a reference blessed by
//! a row of the ledger (`tests/look/LEDGER.tsv`). The samples stay outside
//! git, the references are text in it: the layout of rawler's sample tests
//! (RAWLER_RAWDB, with committed digests).
//!
//! TWO TIERS
//!
//! - Decode (`decode_tier`, no GPU, exact), run first for every sample.
//! - Render (`render_tier`, GPU): four states a sample, and for an X-Trans
//!   sample Exposure -2 with recovery off too. Each is held to
//!   `gate::limits`: robust statistics, never a single pixel.
//!
//! RUNNING IT
//!
//! ```bash
//! # the samples, downloaded and checked against samples.tsv (CI does this)
//! AG_LOOK_SAMPLES=<folder of 7882.CR3, 8964.CR3, ...> \
//!   cargo test --lib mods::look_check::golden_photos -- --ignored --exact --nocapture
//! # bless what a change was meant to move: one ledger row, the reason, the cases
//! AG_LOOK_BLESS="Whites from RapidRAW 1.6.7" AG_LOOK_CASES="*/highlights-whites" AG_LOOK_PR="#171" ...
//! # someone's own RAWs, with references kept outside the repository
//! AG_RAWS="a.CR3;b.NEF" AG_LOOK_REFS=<folder> ...
//! ```
//!
//! The samples need not be on anyone's disk to bless: `golden-bless.yml`
//! (Actions, run by hand with the branch, the reason, the cases and the pull
//! request) blesses on CI and commits the references and the ledger row to
//! the branch.
//!
//! `AG_LOOK_ONLY=8771,7882` limits a run to some samples; `AG_LOOK_SUMMARY`
//! names a file the Markdown summary is appended to (CI: the job summary).
//! Without samples it says SKIPPED and passes.

mod decode_tier;
mod gate;
mod ledger;
mod lens_baseline;
mod outcome;
mod reference;
pub(super) mod render_tier;
mod stats;
mod summary;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use serde_json::json;

use crate::app_settings::AppSettings;
use crate::mods::cache_version::PIPELINE;
use crate::mods::export_precision::{HighPrecisionRenderer, render_high_precision};

use gate::Limits;
use ledger::{Bless, Moved, Row};
use outcome::Outcome;
use reference::{Blessing, Case, SampleRef};
use render_tier::{NO_RECOVERY, RENDER_LONG};

/// How long a run may take, on the adapter it was measured on: twice the
/// first measurement (CLAUDE.md rule 5), 113 s on CI's lavapipe on
/// 2026-10-25 (ten samples, 41 renders, and one more on an export's own
/// processor).
const TIME_BUDGET: (&str, f64) = ("llvmpipe", 226.0);

/// One sample: where it is and what it should be.
struct Sample {
    id: String,
    path: PathBuf,
    /// From the manifest; None for a RAW named by `AG_RAWS`.
    sha256: Option<String>,
    camera: String,
}

/// Where the run reads from and writes to.
struct Setup {
    samples: Vec<Sample>,
    refs: PathBuf,
    summary: Option<PathBuf>,
}

fn repo_refs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/look")
}

fn setup() -> Result<Option<Setup>, String> {
    let refs = std::env::var_os("AG_LOOK_REFS").map_or_else(repo_refs, PathBuf::from);
    let mut samples = if let Ok(raws) = std::env::var("AG_RAWS") {
        if refs == repo_refs() {
            return Err(
                "AG_RAWS needs AG_LOOK_REFS: references of your own RAWs stay outside the repository"
                    .into(),
            );
        }
        raws.split(';')
            .filter(|p| !p.trim().is_empty())
            .map(|p| {
                let path = PathBuf::from(p.trim());
                let id = path.file_stem().map_or_else(String::new, |s| {
                    s.to_string_lossy()
                        .replace(|c: char| !c.is_ascii_alphanumeric(), "-")
                });
                Sample {
                    id,
                    path,
                    sha256: None,
                    camera: String::new(),
                }
            })
            .collect()
    } else if let Some(folder) = std::env::var_os("AG_LOOK_SAMPLES") {
        manifest(&repo_refs().join("samples.tsv"), Path::new(&folder))?
    } else {
        return Ok(None);
    };
    let only = std::env::var("AG_LOOK_ONLY").unwrap_or_default();
    let only: Vec<&str> = only
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if !only.is_empty() {
        samples.retain(|s| only.contains(&s.id.as_str()));
    }
    Ok(Some(Setup {
        samples,
        refs,
        summary: std::env::var_os("AG_LOOK_SUMMARY").map(PathBuf::from),
    }))
}

/// The samples `samples.tsv` lists, each at `<folder>/<id>.<extension>`.
fn manifest(path: &Path, folder: &Path) -> Result<Vec<Sample>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .skip(1)
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let [id, url, sha256, camera, _licence, _covers] = f[..] else {
                return Err(format!("samples.tsv: not six fields: {line:?}"));
            };
            let extension = url.rsplit('.').next().unwrap_or("raw");
            Ok(Sample {
                id: id.to_string(),
                path: folder.join(format!("{id}.{extension}")),
                sha256: Some(sha256.to_string()),
                camera: camera.to_string(),
            })
        })
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(bytes))
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// A sample read, checked against its hash, and through the decode tier.
struct Opened<'a> {
    sample: &'a Sample,
    path: String,
    reference: SampleRef,
    x_trans: bool,
}

/// Everything a run found, for the blessing and the summary.
pub struct Run {
    samples: usize,
    adapter: String,
    limits: Limits,
    renders: usize,
    seconds: f64,
    budget_line: String,
    blessed_row: Option<usize>,
    /// By sample, its cases in order.
    outcomes: BTreeMap<String, Vec<Outcome>>,
    /// By sample, its reference as it will be written if blessed.
    references: BTreeMap<String, SampleRef>,
    failures: Vec<String>,
    notes: Vec<String>,
    /// Defaults renders a one-level highlight shift was put on, and how many
    /// of those the gate caught.
    seen: usize,
    caught: usize,
}

impl Run {
    fn outcome(&mut self, sample: &str, outcome: Outcome) {
        self.outcomes
            .entry(sample.to_string())
            .or_default()
            .push(outcome);
    }

    /// Tier 0: read and check every sample, and what its decode did.
    fn decode_tier<'a>(&mut self, setup: &'a Setup) -> Vec<Opened<'a>> {
        let mut opened = Vec::new();
        for sample in &setup.samples {
            let bytes = match std::fs::read(&sample.path) {
                Ok(b) => b,
                Err(e) => {
                    self.failures
                        .push(format!("{}: {}: {e}", sample.id, sample.path.display()));
                    continue;
                }
            };
            let sha = sha256_hex(&bytes);
            if let Some(want) = &sample.sha256
                && *want != sha
            {
                self.failures.push(format!(
                    "{}: sha256 {sha}, samples.tsv says {want}",
                    sample.id
                ));
                continue;
            }
            // Our readers of the camera's lens correction against RAWmakase's record.
            if let Err(e) = lens_baseline::check(
                &sha,
                argentum_engine::rm_lens_builtin::read(&bytes).as_ref(),
                argentum_engine::rm_lens_builtin::developed_frame(&bytes),
            ) {
                self.failures.push(format!(
                    "{}: built-in lens correction differs from RAWmakase's record: {e}",
                    sample.id
                ));
            }
            let ref_path = setup.refs.join(format!("{}.txt", sample.id));
            let reference = match std::fs::read_to_string(&ref_path) {
                Ok(text) => reference::parse(&text)
                    .unwrap_or_else(|e| panic!("{}: {e}", ref_path.display())),
                Err(_) => SampleRef::default(),
            };
            if !reference.sha256.is_empty() && reference.sha256 != sha {
                self.failures.push(format!(
                    "{}: the reference was made from another file ({})",
                    sample.id, reference.sha256
                ));
            }
            let path = sample.path.to_string_lossy().to_string();
            let (values, x_trans) = match decode_tier::values(&bytes, &path) {
                Ok(v) => v,
                Err(e) => {
                    self.failures.push(format!("{}: decode: {e}", sample.id));
                    continue;
                }
            };
            let mut kept = reference.clone();
            kept.sample = sample.id.clone();
            kept.camera = if sample.camera.is_empty() {
                values
                    .iter()
                    .find(|(k, _)| k == "camera")
                    .map_or_else(String::new, |(_, v)| v.clone())
            } else {
                sample.camera.clone()
            };
            kept.sha256 = sha;
            self.references.insert(sample.id.clone(), kept);
            let case = format!("{}/decode", sample.id);
            let o = outcome::of_decode(case, values, reference.cases.get("decode"));
            self.outcome(&sample.id, o);
            opened.push(Opened {
                sample,
                path,
                reference,
                x_trans,
            });
        }
        opened
    }

    /// Tier 1: every state of every sample, on one kept renderer.
    fn render_tier(
        &mut self,
        opened: &[Opened],
        renderer: &HighPrecisionRenderer,
        ctx: &crate::image_processing::GpuContext,
    ) {
        let settings = AppSettings::default();
        let film_look = render_tier::film_look();
        let lut = Arc::new(
            crate::lut_processing::parse_lut_file(&film_look.to_string_lossy())
                .unwrap_or_else(|e| panic!("{}: {e}", film_look.display())),
        );
        let mut kept_checked = false;
        for o in opened {
            let (id, path) = (o.sample.id.as_str(), o.path.as_str());
            let bytes = match std::fs::read(&o.sample.path) {
                Ok(b) => b,
                Err(e) => {
                    self.failures.push(format!("{id}: {e}"));
                    continue;
                }
            };
            let small = match render_tier::develop(&bytes, path, &settings) {
                Ok(i) => i,
                Err(e) => {
                    self.failures.push(format!("{id}: develop: {e}"));
                    continue;
                }
            };
            let opening = render_tier::opening_edit(&bytes);
            let mut with_recovery = None;
            for (state, keys) in render_tier::states(&film_look) {
                let edit = render_tier::merged(&opening, &keys);
                let asked = || render_tier::request(&edit, &bytes, path, &settings, &lut);
                let t = Instant::now();
                let out = match render_tier::render(renderer, &small, asked()) {
                    Ok(out) => out,
                    Err(e) => {
                        self.failures.push(format!("{id}/{state}: render: {e}"));
                        continue;
                    }
                };
                self.renders += 1;
                eprintln!(
                    "  {id}/{state}: {} x {} in {:.2} s",
                    out.width(),
                    out.height(),
                    t.elapsed().as_secs_f64()
                );
                let look = stats::measure(&out);
                if state == "defaults" {
                    if !kept_checked {
                        // The kept processor renders what an export's own,
                        // built for this photo alone, renders: once a run.
                        kept_checked = true;
                        match render_high_precision(ctx, &small, asked()) {
                            Ok(fresh) if fresh.to_rgba32f().as_raw() == out.as_raw() => {}
                            Ok(_) => self.failures.push(format!(
                                "{id}: the kept renderer renders differently from render_high_precision"
                            )),
                            Err(e) => self.failures.push(format!("{id}: {e}")),
                        }
                    }
                    // The gate has to see a one-level highlight shift here.
                    let shifted = stats::measure(&render_tier::one_level_brighter_highlights(&out));
                    let d = gate::drift(&look, &shifted);
                    self.seen += 1;
                    if d.breaches(&self.limits).is_empty() {
                        self.notes.push(format!(
                            "{id}: a one-level highlight shift stays within the limits ({})",
                            d.line()
                        ));
                    } else {
                        self.caught += 1;
                    }
                }
                if state == "exposure-2" {
                    with_recovery = Some(look.clone());
                }
                let r = o.reference.cases.get(state);
                let case = format!("{id}/{state}");
                let done = outcome::of_render(case, &look, &self.adapter, r, &self.limits);
                self.outcome(id, done);
            }

            // X-Trans: the same Exposure -2 with recovery off. Recovery reads
            // a 2x2 Bayer block (`highlights::recover_mosaic`); this shows
            // what it does on a 6x6 pattern.
            if o.x_trans {
                let edit = render_tier::merged(&opening, &json!({ "exposure": -2.0 }));
                let rendered = render_tier::develop_without_recovery(&bytes, path, &settings)
                    .and_then(|img| {
                        let asked = render_tier::request(&edit, &bytes, path, &settings, &lut);
                        render_tier::render(renderer, &img, asked)
                    });
                match rendered {
                    Ok(out) => {
                        self.renders += 1;
                        let look = stats::measure(&out);
                        if let Some(on) = &with_recovery {
                            self.notes.push(format!(
                                "{id} (X-Trans), Exposure -2, recovery off against on: {}",
                                gate::drift(on, &look).line()
                            ));
                        }
                        let r = o.reference.cases.get(NO_RECOVERY);
                        let case = format!("{id}/{NO_RECOVERY}");
                        let done = outcome::of_render(case, &look, &self.adapter, r, &self.limits);
                        self.outcome(id, done);
                    }
                    Err(e) => self.failures.push(format!("{id}/{NO_RECOVERY}: {e}")),
                }
            }
        }
    }

    /// A blessing: refused unless the run succeeded, every case that moved
    /// is named and the pipeline moved with the pixels; then one ledger row,
    /// and the named cases rewritten, keeping their anchors.
    fn bless(&mut self, bless: &Bless, ledger_path: &Path, refs: &Path) {
        if !self.failures.is_empty() {
            panic!("not blessed: the run failed:\n{}", self.failures.join("\n"));
        }
        for o in self
            .outcomes
            .values()
            .flatten()
            .filter(|o| !o.breaches.is_empty())
        {
            let moved = Moved {
                case: o.case.clone(),
                blessed: o
                    .reference
                    .as_ref()
                    .map(|r| (r.blessed.pipeline, r.blessed.adapter.clone())),
            };
            ledger::may_bless(bless, &[moved], PIPELINE, &o.adapter)
                .unwrap_or_else(|e| panic!("not blessed: {e}"));
        }
        let row = ledger::append(
            ledger_path,
            &Row {
                date: today(),
                pr: bless.pr.clone(),
                cases: bless.cases.clone(),
                reason: bless.reason.clone(),
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));
        self.blessed_row = Some(row);
        for (id, done) in self.outcomes.iter_mut() {
            let sample_ref = self.references.get_mut(id).expect("every sample has one");
            for o in done.iter_mut().filter(|o| bless.names(&o.case)) {
                let state = o.case.split_once('/').map_or("", |(_, s)| s).to_string();
                let (anchor_date, anchor) = o.reference.as_ref().map_or_else(
                    || (today(), o.values.clone()),
                    |r| (r.anchor_date.clone(), r.anchor.clone()),
                );
                let case = Case {
                    blessed: Blessing {
                        row,
                        date: today(),
                        pipeline: PIPELINE,
                        adapter: o.adapter.clone(),
                    },
                    values: o.values.clone(),
                    thumb: o.thumb.clone(),
                    anchor_date,
                    anchor,
                };
                sample_ref.cases.insert(state, case.clone());
                o.reference = Some(case);
                o.breaches.clear();
            }
            let path = refs.join(format!("{id}.txt"));
            std::fs::create_dir_all(refs).unwrap_or_else(|e| panic!("{e}"));
            std::fs::write(&path, reference::write(sample_ref))
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
    }

    /// Every case that moved is a failure, and so is a reference whose
    /// ledger row does not name it: one edited by hand, or a row taken out.
    fn judge(&mut self, rows: &[Row]) {
        let mut failures = Vec::new();
        for o in self.outcomes.values().flatten() {
            if !o.breaches.is_empty() {
                failures.push(format!("{}: {}", o.case, o.breaches.join("; ")));
            }
            if let Some(r) = &o.reference {
                let named = rows
                    .get(r.blessed.row.wrapping_sub(1))
                    .is_some_and(|row| row.names(&o.case));
                if !named {
                    failures.push(format!(
                        "{}: its reference names ledger row {}, which does not name it",
                        o.case, r.blessed.row
                    ));
                }
            }
        }
        self.failures.extend(failures);
    }
}

#[test]
#[ignore = "reads the golden photos and needs a GPU: CI runs it (app.yml), or by hand"]
fn golden_photos() {
    let started = Instant::now();
    let setup = match setup() {
        Ok(Some(s)) => s,
        Ok(None) => {
            eprintln!(
                "golden_photos: SKIPPED - set AG_LOOK_SAMPLES to the downloaded samples, or AG_RAWS with AG_LOOK_REFS"
            );
            return;
        }
        Err(e) => panic!("{e}"),
    };
    let bless = Bless::from_env().unwrap_or_else(|e| panic!("{e}"));
    let ledger_path = setup.refs.join("LEDGER.tsv");
    // A ledger that does not read fails now, not after every render.
    ledger::read(&ledger_path).unwrap_or_else(|e| panic!("{e}"));

    let mut run = Run {
        samples: setup.samples.len(),
        adapter: String::new(),
        limits: gate::limits(),
        renders: 0,
        seconds: 0.0,
        budget_line: String::new(),
        blessed_row: None,
        outcomes: BTreeMap::new(),
        references: BTreeMap::new(),
        failures: Vec::new(),
        notes: Vec::new(),
        seen: 0,
        caught: 0,
    };

    let opened = run.decode_tier(&setup);
    match render_tier::device() {
        Ok((ctx, adapter)) => {
            eprintln!(
                "golden_photos: {} samples on {adapter}",
                setup.samples.len()
            );
            run.adapter = adapter;
            // One processor for every render: building one compiles every
            // pipeline, which on a software adapter takes longer than a render.
            let t = Instant::now();
            let renderer = HighPrecisionRenderer::new(&ctx, RENDER_LONG, RENDER_LONG)
                .unwrap_or_else(|e| panic!("{e}"));
            eprintln!(
                "golden_photos: pipelines built in {:.1} s",
                t.elapsed().as_secs_f64()
            );
            run.render_tier(&opened, &renderer, &ctx);
        }
        Err(e) => run
            .failures
            .push(format!("the render tier did not run: {e}")),
    }
    run.seconds = started.elapsed().as_secs_f64();

    if let Some(bless) = &bless {
        run.bless(bless, &ledger_path, &setup.refs);
    }
    let rows = ledger::read(&ledger_path).unwrap_or_else(|e| panic!("{e}"));
    run.judge(&rows);

    let (budget_adapter, budget) = TIME_BUDGET;
    run.budget_line = if budget > 0.0 {
        format!("budget {budget:.0} s on {budget_adapter}")
    } else {
        "no time budget yet".into()
    };
    if budget > 0.0 && run.adapter.contains(budget_adapter) && run.seconds > budget {
        run.failures.push(format!(
            "the run took {:.0} s, over its budget of {budget:.0} s on {budget_adapter}",
            run.seconds
        ));
    }

    let (md, warnings) = summary::markdown(&run, &rows);
    for w in &warnings {
        println!("::warning::{w}");
    }
    eprintln!("{md}");
    if let Some(path) = &setup.summary {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = f.write_all(md.as_bytes());
        }
    }
    assert!(
        run.failures.is_empty(),
        "golden photos: {} failure(s):\n{}",
        run.failures.len(),
        run.failures.join("\n")
    );
}
