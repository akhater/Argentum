//! Downloading an AI model's missing files, one download at a time. Ours.
//!
//! Used by Settings > General > AI Models' Download button (`download`), by
//! the selection masks the first time they are used (`selection_models`), and
//! by the other AI features before they load a model (`ensure`).
//!
//! The steps are Shoebox's (src-tauri/src/models.rs @ 3c82753, `download` and
//! `fetch`, with paths.rs `model_file`), which selection_models.rs already
//! followed from RAWmakase (src/app/subject_mask/models.rs @ 7c3c81a): only the
//! files that are missing or wrong are fetched; each is streamed to
//! `<name>.part` beside its final name while its SHA-256 is worked out, and
//! moved into place only once that matches the pinned one. A part file that
//! fails or is stopped is removed. Progress is reported once a MiB, as
//! Shoebox's `Hashed` does, not once a network chunk.
//!
//! The files, addresses and checksums are the catalogue's, the one list every
//! feature loads its model from, so what lands here is what that feature
//! looks for.
//!
//! ONE AT A TIME
//!
//! Two downloads of the same file would write the same part file. So each
//! download waits for the one before it to finish, and then finds what that one
//! fetched already in place. Clearing leftovers deletes part files, so
//! `model_manager` only clears while it holds `exclusive()`.
//!
//! NEVER THROUGH A LINK
//!
//! A folder that is a link (a junction on Windows) points at files kept
//! somewhere else, such as a copy of the selection models kept outside the app.
//! Nothing is written through one; files already there are used.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::model_catalog::{self, File, Stored};

/// The event a download reports its progress on.
pub const EVENT: &str = "ag-model-download";

/// The error a stopped download ends with.
pub const STOPPED: &str = "Download stopped.";

/// Report at most once per this many bytes.
const REPORT_EVERY: u64 = 1 << 20;

/// How long to wait for a connection to a server. A server that is up answers
/// in well under a second; this only has to outlast a slow mobile line, and
/// after it the file's next address is tried.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// How long a download may go without a single byte arriving (the answer's
/// start, or the next piece of the body) before its address is given up on.
/// Deliberately not a limit on the whole transfer: a large model on a slow line
/// must still finish, so only silence counts, and any bytes start it afresh.
const STALL_TIMEOUT: Duration = Duration::from_secs(30);

/// How often a wait for the network looks at Stop, so Stop acts within about
/// this long whatever the server is doing.
const STOP_POLL: Duration = Duration::from_millis(250);

/// The two waits a download puts up with; the tests use short ones.
#[derive(Clone, Copy)]
struct Limits {
    connect: Duration,
    stall: Duration,
}

const LIMITS: Limits = Limits {
    connect: CONNECT_TIMEOUT,
    stall: STALL_TIMEOUT,
};

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// The catalogue entry being downloaded.
    pub id: String,
    pub done: u64,
    pub total: u64,
}

static RUNNING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static CURRENT: Mutex<Option<Progress>> = Mutex::new(None);
static STOP: AtomicBool = AtomicBool::new(false);

/// The download running now, if any: for a settings page opened after it began.
pub fn current() -> Option<Progress> {
    CURRENT.lock().ok().and_then(|c| c.clone())
}

/// Stop the download running now: it removes its part file and fails with
/// `STOPPED`. Nothing happens when none is running.
pub fn stop() {
    if current().is_some() {
        STOP.store(true, Ordering::Relaxed);
    }
}

/// No download starts while the guard is held. None when one is running.
pub fn exclusive() -> Option<tokio::sync::MutexGuard<'static, ()>> {
    RUNNING.try_lock().ok()
}

/// Download catalogue entry `id`'s missing files into the models folder
/// `models`. Returns the bytes fetched: 0 when everything was there.
pub async fn download(
    models: &Path,
    id: &str,
    progress: impl Fn(&Progress),
) -> Result<u64, String> {
    let model = model_catalog::find(id).ok_or_else(|| format!("No AI model called {id}"))?;
    if let Stored::FoldersStartingWith { .. } = model.stored {
        return Err(format!(
            "{} arrives with the feature that needs it, the first time it runs.",
            model.label
        ));
    }
    let dir = match model.stored.folder() {
        Some(folder) => models.join(folder),
        None => models.to_path_buf(),
    };
    fetch_missing(&dir, id, &model.stored.files(), progress).await
}

/// Fetch whatever of `files` is missing from `dir` or does not match its
/// checksum, reporting progress as catalogue entry `id`. A file saved under
/// an old name (`model_catalog::RENAMED`) is renamed rather than fetched.
/// Returns the bytes fetched.
pub async fn fetch_missing(
    dir: &Path,
    id: &str,
    files: &[File],
    progress: impl Fn(&Progress),
) -> Result<u64, String> {
    fetch_missing_with(dir, id, files, LIMITS, progress).await
}

async fn fetch_missing_with(
    dir: &Path,
    id: &str,
    files: &[File],
    limits: Limits,
    progress: impl Fn(&Progress),
) -> Result<u64, String> {
    let _running = RUNNING.lock().await;
    STOP.store(false, Ordering::Relaxed);
    {
        let (dir, files) = (dir.to_path_buf(), files.to_vec());
        tokio::task::spawn_blocking(move || promote_renamed(&dir, &files, model_catalog::RENAMED))
            .await
            .map_err(|e| e.to_string())??;
    }
    let wanted = not_in_place(dir, files).await?;
    if wanted.is_empty() {
        return Ok(0);
    }
    if is_link(dir) {
        return Err(format!(
            "{} is a link to another folder, so Argentum does not download into it",
            dir.display()
        ));
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;

    let report = Reporter::new(id, wanted.iter().map(|f| f.bytes).sum(), progress);
    let mut done = 0u64;
    for file in &wanted {
        let mut last_error = String::new();
        let mut fetched = false;
        for url in std::iter::once(file.url).chain(file.fallback) {
            match fetch(dir, file, url, done, limits, &report).await {
                Ok(()) => {
                    fetched = true;
                    break;
                }
                Err(e) if e == STOPPED => return Err(e),
                Err(e) => last_error = format!("{url}: {e}"),
            }
        }
        if !fetched {
            return Err(format!("Could not download {}: {last_error}", file.name));
        }
        done += file.bytes;
        report.at(done);
    }
    Ok(done)
}

/// What an AI feature calls before it loads its model: `files` of catalogue
/// entry `id` in `dir`, fetched first when any is missing or wrong. Returns
/// the bytes fetched.
///
/// Files already in place are used as they are, without waiting for a
/// download of something else to finish, the way the features' own checks
/// did before they asked here.
pub async fn ensure(
    dir: &Path,
    id: &str,
    files: &[File],
    progress: impl Fn(&Progress),
) -> Result<u64, String> {
    if not_in_place(dir, files).await?.is_empty() {
        return Ok(0);
    }
    fetch_missing(dir, id, files, progress).await
}

/// `ensure` for catalogue entry `id` in the app's models folder, telling the
/// editor's download notice `notice` while it fetches, and the settings page
/// its progress. Returns the folder its files are in.
pub async fn ensure_in_app(
    app: &tauri::AppHandle,
    id: &str,
    notice: &str,
) -> Result<PathBuf, String> {
    use tauri::Emitter;
    let model = model_catalog::find(id).ok_or_else(|| format!("No AI model called {id}"))?;
    let models = super::model_manager::models_dir(app)?;
    let dir = match model.stored.folder() {
        Some(folder) => models.join(folder),
        None => models,
    };
    let announced = AtomicBool::new(false);
    let fetched = ensure(&dir, id, &model.stored.files(), |progress| {
        if progress.total > 0 && !announced.swap(true, Ordering::Relaxed) {
            let _ = app.emit("ai-model-download-start", notice);
        }
        let _ = app.emit(EVENT, progress);
    })
    .await;
    if announced.load(Ordering::Relaxed) {
        let _ = app.emit("ai-model-download-finish", notice);
    }
    fetched.map(|_| dir)
}

fn is_link(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink())
}

/// Each `(old, current)` of `renamed` whose current name is one of `files`:
/// when the current file is missing and the old one is that model, size and
/// checksum, it takes the current name (RapidRAW's ai_processing.rs at 1.6.6,
/// `promote_legacy_model_filename`). Nothing is renamed through a link.
fn promote_renamed(
    dir: &Path,
    files: &[File],
    renamed: &[(&'static str, &'static str)],
) -> Result<(), String> {
    if is_link(dir) {
        return Ok(());
    }
    for &(old, current) in renamed {
        let Some(file) = files.iter().find(|f| f.name == current) else {
            continue;
        };
        let current_path = dir.join(current);
        if current_path.exists() || !in_place(dir, &File { name: old, ..*file }) {
            continue;
        }
        if let Err(e) = std::fs::rename(dir.join(old), &current_path)
            && !current_path.exists()
        {
            return Err(format!("Could not rename {old} to {current}: {e}"));
        }
    }
    Ok(())
}

/// Those of `files` that are not in place in `dir`.
async fn not_in_place(dir: &Path, files: &[File]) -> Result<Vec<File>, String> {
    let (dir, files) = (dir.to_path_buf(), files.to_vec());
    tokio::task::spawn_blocking(move || files.into_iter().filter(|f| !in_place(&dir, f)).collect())
        .await
        .map_err(|e| e.to_string())
}

/// Here, at its size, with its checksum.
fn in_place(dir: &Path, file: &File) -> bool {
    let path = dir.join(file.name);
    std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() == file.bytes)
        && sha256_of(&path).is_ok_and(|sha| sha == file.sha256)
}

/// Whether a feature's files are all here, and how much is left to fetch.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename = "ModelsStatus"))]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub installed: bool,
    pub missing_bytes: u64,
    pub total_bytes: u64,
}

fn has_size(dir: &Path, file: &File) -> bool {
    std::fs::metadata(dir.join(file.name)).is_ok_and(|m| m.is_file() && m.len() == file.bytes)
}

/// `files` in `dir`, by size alone: cheap enough to ask before every mask.
pub fn status(dir: &Path, files: &[File]) -> Status {
    let missing_bytes = files
        .iter()
        .filter(|f| !has_size(dir, f))
        .map(|f| f.bytes)
        .sum();
    Status {
        installed: missing_bytes == 0,
        missing_bytes,
        total_bytes: files.iter().map(|f| f.bytes).sum(),
    }
}

/// Every one of `files` in `dir`, each with its checksum. `verified` holds the
/// folder last found whole, so each file is read through once per run, not
/// once per mask; set it to None after a download.
pub fn verify(dir: &Path, files: &[File], verified: &Mutex<Option<PathBuf>>) -> Result<(), String> {
    if verified.lock().map_err(|e| e.to_string())?.as_deref() == Some(dir) {
        return Ok(());
    }
    for file in files {
        if !has_size(dir, file) {
            return Err(format!("{} is missing", file.name));
        }
        if sha256_of(&dir.join(file.name))? != file.sha256 {
            return Err(format!("{} does not match its checksum", file.name));
        }
    }
    *verified.lock().map_err(|e| e.to_string())? = Some(dir.to_path_buf());
    Ok(())
}

pub(super) fn sha256_of(path: &Path) -> Result<String, String> {
    let mut reader = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Keeps `CURRENT` and the caller told, once a MiB; `CURRENT` is emptied
/// when the download ends, however it ends.
struct Reporter<F: Fn(&Progress)> {
    id: String,
    total: u64,
    last: AtomicU64,
    progress: F,
}

impl<F: Fn(&Progress)> Reporter<F> {
    fn new(id: &str, total: u64, progress: F) -> Self {
        let reporter = Reporter {
            id: id.to_string(),
            total,
            last: AtomicU64::new(0),
            progress,
        };
        reporter.tell(0);
        reporter
    }

    fn at(&self, done: u64) {
        if done >= self.total || done >= self.last.load(Ordering::Relaxed) + REPORT_EVERY {
            self.tell(done);
        }
    }

    fn tell(&self, done: u64) {
        self.last.store(done, Ordering::Relaxed);
        let now = Progress {
            id: self.id.clone(),
            done,
            total: self.total,
        };
        if let Ok(mut current) = CURRENT.lock() {
            *current = Some(now.clone());
        }
        (self.progress)(&now);
    }
}

impl<F: Fn(&Progress)> Drop for Reporter<F> {
    fn drop(&mut self) {
        if let Ok(mut current) = CURRENT.lock() {
            *current = None;
        }
    }
}

/// A part file beside its final name, removed unless it was moved into
/// place (Shoebox's paths.rs `ModelFile`).
struct Part {
    path: PathBuf,
    kept: bool,
}

impl Drop for Part {
    fn drop(&mut self) {
        if !self.kept {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// `wait`, a request or the next piece of a body, failing when nothing
/// arrives for `stall`, and with `STOPPED` soon after Stop, not when the
/// server next speaks.
async fn waiting<T, E: ToString>(
    wait: impl std::future::Future<Output = Result<T, E>>,
    stall: Duration,
) -> Result<T, String> {
    let mut wait = std::pin::pin!(wait);
    let mut silent = Duration::ZERO;
    loop {
        if STOP.load(Ordering::Relaxed) {
            return Err(STOPPED.to_string());
        }
        let poll = STOP_POLL.min(stall.saturating_sub(silent));
        match tokio::time::timeout(poll, wait.as_mut()).await {
            Ok(done) => return done.map_err(|e| e.to_string()),
            Err(_) => silent += poll,
        }
        if silent >= stall {
            return Err(format!("no data for {} s", stall.as_secs_f32()));
        }
    }
}

/// One file from one address, into its part file, moved into place once its
/// checksum matches.
async fn fetch<F: Fn(&Progress)>(
    dir: &Path,
    file: &File,
    url: &str,
    before: u64,
    limits: Limits,
    report: &Reporter<F>,
) -> Result<(), String> {
    let mut part = Part {
        path: dir.join(format!("{}.part", file.name)),
        kept: false,
    };
    let client = reqwest::Client::builder()
        .connect_timeout(limits.connect)
        .build()
        .map_err(|e| e.to_string())?;
    let mut response = waiting(client.get(url).send(), limits.stall).await?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    // Declared after `part`, so it is closed before the part file is removed.
    let mut out = std::fs::File::create(&part.path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut got = 0u64;
    while let Some(chunk) = waiting(response.chunk(), limits.stall).await? {
        if STOP.load(Ordering::Relaxed) {
            return Err(STOPPED.to_string());
        }
        got += chunk.len() as u64;
        if got > file.bytes {
            return Err("larger than expected".to_string());
        }
        hasher.update(&chunk);
        out.write_all(&chunk).map_err(|e| e.to_string())?;
        report.at(before + got);
    }
    out.sync_all().map_err(|e| e.to_string())?;
    drop(out);
    if got != file.bytes || hex::encode(hasher.finalize()) != file.sha256 {
        return Err("checksum does not match".to_string());
    }
    std::fs::rename(&part.path, dir.join(file.name)).map_err(|e| e.to_string())?;
    part.kept = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    /// Answers `GET /<name>` with that body, and anything else with 404.
    /// Counts the requests it answered.
    fn serve(bodies: Vec<(&'static str, Vec<u8>)>) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let counted = count.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                counted.fetch_add(1, Ordering::SeqCst);
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                if reader.read_line(&mut request).is_err() {
                    continue;
                }
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                    line.clear();
                }
                let path = request
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .trim_start_matches('/');
                let mut stream = stream;
                match bodies.iter().find(|(name, _)| *name == path) {
                    Some((_, body)) => {
                        let head = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(head.as_bytes());
                        let _ = stream.write_all(body);
                    }
                    None => {
                        let _ = stream.write_all(
                            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                    }
                }
            }
        });
        (base, count)
    }

    fn leak(s: String) -> &'static str {
        Box::leak(s.into_boxed_str())
    }

    fn file(name: &'static str, url: String, body: &[u8]) -> File {
        File {
            name,
            url: leak(url),
            fallback: None,
            sha256: leak(hex::encode(Sha256::digest(body))),
            bytes: body.len() as u64,
        }
    }

    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[tokio::test]
    async fn a_file_that_matches_lands_under_its_name() {
        let body = vec![7u8; 3 << 20];
        let (base, _) = serve(vec![("a.onnx", body.clone())]);
        let tmp = tempfile::tempdir().unwrap();
        let heard = Mutex::new(Vec::new());
        let files = [file("a.onnx", format!("{base}/a.onnx"), &body)];

        let fetched = fetch_missing(tmp.path(), "lands", &files, |p| {
            heard.lock().unwrap().push(p.done)
        })
        .await
        .unwrap();

        assert_eq!(fetched, body.len() as u64);
        assert_eq!(std::fs::read(tmp.path().join("a.onnx")).unwrap(), body);
        assert_eq!(names_in(tmp.path()), ["a.onnx"]);
        // From 0 to the end, a few times between: not once a network chunk.
        let heard = heard.into_inner().unwrap();
        assert_eq!(heard.first(), Some(&0));
        assert_eq!(heard.last(), Some(&(body.len() as u64)));
        assert!(heard.len() <= 8, "{heard:?}");
        // Other tests download too, so only this one's is checked.
        assert!(current().is_none_or(|p| p.id != "lands"));
    }

    #[tokio::test]
    async fn a_wrong_file_is_not_kept() {
        let (base, _) = serve(vec![("a.onnx", b"not the model".to_vec())]);
        let tmp = tempfile::tempdir().unwrap();
        let files = [file(
            "a.onnx",
            format!("{base}/a.onnx"),
            b"the model, honest",
        )];

        let err = fetch_missing(tmp.path(), "test", &files, |_| {})
            .await
            .unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        assert!(names_in(tmp.path()).is_empty());
    }

    /// What is already there with the right checksum is not fetched again;
    /// a file of the right size with other bytes is.
    #[tokio::test]
    async fn only_what_is_missing_or_wrong_is_fetched() {
        let (a, b) = (b"model a".to_vec(), b"model b".to_vec());
        let (base, requests) = serve(vec![("a", a.clone()), ("b", b.clone())]);
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a"), &a).unwrap();
        std::fs::write(tmp.path().join("b"), b"model x").unwrap();
        let files = [
            file("a", format!("{base}/a"), &a),
            file("b", format!("{base}/b"), &b),
        ];

        let fetched = fetch_missing(tmp.path(), "test", &files, |_| {})
            .await
            .unwrap();
        assert_eq!(fetched, b.len() as u64);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert_eq!(std::fs::read(tmp.path().join("b")).unwrap(), b);

        assert_eq!(
            fetch_missing(tmp.path(), "test", &files, |_| {}).await,
            Ok(0)
        );
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }

    /// A models folder as an earlier version left it: each file under the name
    /// it was saved as, nothing else. It is used as it is: nothing is asked of
    /// the network (the address cannot be reached), nothing is rewritten.
    #[tokio::test]
    async fn a_folder_laid_out_the_old_way_is_used_as_it_is() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (vec![3u8; 2 << 20], b"model b".to_vec());
        std::fs::write(tmp.path().join("a.onnx"), &a).unwrap();
        std::fs::write(tmp.path().join("b.onnx"), &b).unwrap();
        let modified = |name: &str| {
            std::fs::metadata(tmp.path().join(name))
                .unwrap()
                .modified()
                .unwrap()
        };
        let before = (modified("a.onnx"), modified("b.onnx"));
        let files = [
            file("a.onnx", "http://127.0.0.1:9/a".into(), &a),
            file("b.onnx", "http://127.0.0.1:9/b".into(), &b),
        ];

        let heard = Mutex::new(0);
        let fetched = ensure(tmp.path(), "old-layout", &files, |_| {
            *heard.lock().unwrap() += 1
        })
        .await;
        assert_eq!(fetched, Ok(0));
        assert_eq!(*heard.lock().unwrap(), 0, "nothing to report");
        assert_eq!(names_in(tmp.path()), ["a.onnx", "b.onnx"]);
        assert_eq!((modified("a.onnx"), modified("b.onnx")), before);
        assert_eq!(
            fetch_missing(tmp.path(), "old-layout", &files, |_| {}).await,
            Ok(0)
        );
    }

    /// A feature whose files are all here does not wait for a download of
    /// something else, as its own check never did.
    #[tokio::test]
    async fn a_complete_folder_does_not_wait_for_another_download() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a"), b"a").unwrap();
        let files = [file("a", "http://127.0.0.1:9/a".into(), b"a")];

        let _another = RUNNING.lock().await;
        let fetched = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            ensure(tmp.path(), "test", &files, |_| {}),
        )
        .await;
        assert_eq!(fetched, Ok(Ok(0)));
    }

    /// RapidRAW saved the sky model as `skyseg-u2net.onnx` before 1.6.6. That
    /// file takes today's name instead of being fetched again.
    #[tokio::test]
    async fn the_sky_model_under_its_old_name_is_renamed_not_fetched() {
        let tmp = tempfile::tempdir().unwrap();
        let body = b"the sky model".to_vec();
        std::fs::write(tmp.path().join("skyseg-u2net.onnx"), &body).unwrap();
        let files = [file(
            "skyseg_u2net.onnx",
            "http://127.0.0.1:9/sky".into(),
            &body,
        )];

        assert_eq!(ensure(tmp.path(), "test", &files, |_| {}).await, Ok(0));
        assert_eq!(names_in(tmp.path()), ["skyseg_u2net.onnx"]);
        assert_eq!(
            std::fs::read(tmp.path().join("skyseg_u2net.onnx")).unwrap(),
            body
        );
    }

    /// Only the model itself is renamed, and only when today's name is free.
    #[test]
    fn an_old_name_is_renamed_only_when_it_is_that_model() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let renamed = [("old.onnx", "new.onnx")];
        let files = [file("new.onnx", "http://127.0.0.1:9/n".into(), b"model")];

        std::fs::write(dir.join("old.onnx"), b"other").unwrap();
        promote_renamed(dir, &files, &renamed).unwrap();
        assert_eq!(
            names_in(dir),
            ["old.onnx"],
            "wrong bytes stay where they are"
        );

        std::fs::write(dir.join("old.onnx"), b"model").unwrap();
        std::fs::write(dir.join("new.onnx"), b"newer").unwrap();
        promote_renamed(dir, &files, &renamed).unwrap();
        assert_eq!(names_in(dir), ["new.onnx", "old.onnx"]);
        assert_eq!(std::fs::read(dir.join("new.onnx")).unwrap(), b"newer");

        std::fs::remove_file(dir.join("new.onnx")).unwrap();
        promote_renamed(dir, &files, &renamed).unwrap();
        assert_eq!(names_in(dir), ["new.onnx"]);
    }

    #[tokio::test]
    async fn ensure_fetches_what_is_missing() {
        let body = b"the model".to_vec();
        let (base, _) = serve(vec![("a", body.clone())]);
        let tmp = tempfile::tempdir().unwrap();
        let files = [file("a.onnx", format!("{base}/a"), &body)];

        assert_eq!(
            ensure(tmp.path(), "test", &files, |_| {}).await,
            Ok(body.len() as u64)
        );
        assert_eq!(std::fs::read(tmp.path().join("a.onnx")).unwrap(), body);
    }

    #[tokio::test]
    async fn the_fallback_is_tried_when_the_first_address_fails() {
        let body = b"mirrored".to_vec();
        let (base, _) = serve(vec![("original", body.clone())]);
        let tmp = tempfile::tempdir().unwrap();
        let mut files = [file("m.onnx", format!("{base}/mirror"), &body)];
        files[0].fallback = Some(leak(format!("{base}/original")));

        fetch_missing(tmp.path(), "test", &files, |_| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(tmp.path().join("m.onnx")).unwrap(), body);
    }

    #[tokio::test]
    async fn a_stopped_download_leaves_nothing_behind() {
        let body = vec![1u8; 4 << 20];
        let (base, _) = serve(vec![("big", body.clone())]);
        let tmp = tempfile::tempdir().unwrap();
        let files = [file("big.onnx", format!("{base}/big"), &body)];

        let err = fetch_missing(tmp.path(), "stopped", &files, |_| stop())
            .await
            .unwrap_err();
        assert_eq!(err, STOPPED);
        assert!(names_in(tmp.path()).is_empty());
        assert!(current().is_none_or(|p| p.id != "stopped"));
    }

    /// Accepts connections and holds them open. Each is answered with `sent`
    /// (when not empty) and then nothing more: a server that has gone quiet.
    fn quiet_server(sent: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for mut stream in listener.incoming().flatten() {
                if !sent.is_empty() {
                    // Hear the request out first, as a real server does.
                    let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                        line.clear();
                    }
                    let _ = stream.write_all(sent);
                }
                held.push(stream);
            }
        });
        base
    }

    const HALF_BODY: &[u8] =
        b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\nthe mod";

    fn quick() -> Limits {
        Limits {
            connect: std::time::Duration::from_millis(300),
            stall: std::time::Duration::from_millis(300),
        }
    }

    /// The first address takes the connection and says nothing; the second has
    /// the file. The first is given up on after the stall time, not waited for.
    #[tokio::test]
    async fn a_silent_address_fails_over_to_the_next() {
        let body = b"the model".to_vec();
        let (good, _) = serve(vec![("m", body.clone())]);
        let silent = quiet_server(b"");
        let tmp = tempfile::tempdir().unwrap();
        let mut files = [file("m.onnx", format!("{silent}/m"), &body)];
        files[0].fallback = Some(leak(format!("{good}/m")));

        let fetched = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            fetch_missing_with(tmp.path(), "silent", &files, quick(), |_| {}),
        )
        .await
        .expect("must not wait forever");
        assert_eq!(fetched, Ok(body.len() as u64));
        assert_eq!(std::fs::read(tmp.path().join("m.onnx")).unwrap(), body);
        assert_eq!(names_in(tmp.path()), ["m.onnx"]);
    }

    /// The first address starts the body and then stops sending.
    #[tokio::test]
    async fn an_address_that_stalls_mid_body_fails_over_to_the_next() {
        let body = b"the model, all of it".to_vec();
        let (good, _) = serve(vec![("m", body.clone())]);
        let stalling = quiet_server(HALF_BODY);
        let tmp = tempfile::tempdir().unwrap();
        let mut files = [file("m.onnx", format!("{stalling}/m"), &body)];
        files[0].fallback = Some(leak(format!("{good}/m")));

        let fetched = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            fetch_missing_with(tmp.path(), "mid-body", &files, quick(), |_| {}),
        )
        .await
        .expect("must not wait forever");
        assert_eq!(fetched, Ok(body.len() as u64));
        assert_eq!(std::fs::read(tmp.path().join("m.onnx")).unwrap(), body);
        assert_eq!(names_in(tmp.path()), ["m.onnx"]);
    }

    /// With no address left the download fails, and leaves nothing behind.
    #[tokio::test]
    async fn a_stalled_download_with_no_address_left_fails_cleanly() {
        let body = b"the model, all of it".to_vec();
        let stalling = quiet_server(HALF_BODY);
        let tmp = tempfile::tempdir().unwrap();
        let files = [file("m.onnx", format!("{stalling}/m"), &body)];

        let err = fetch_missing_with(tmp.path(), "stalled", &files, quick(), |_| {})
            .await
            .unwrap_err();
        assert!(err.contains("m.onnx"), "{err}");
        assert!(names_in(tmp.path()).is_empty());
    }

    /// Stop while the server is quiet, before or after the body began, takes
    /// effect within about a second even with a long stall time.
    #[tokio::test]
    async fn stop_is_honoured_while_a_read_is_waiting() {
        for (id, sent) in [("stop-silent", &b""[..]), ("stop-mid-body", HALF_BODY)] {
            let quiet = quiet_server(sent);
            let tmp = tempfile::tempdir().unwrap();
            let files = [file("m.onnx", format!("{quiet}/m"), &[9u8; 20])];
            let limits = Limits {
                connect: std::time::Duration::from_secs(60),
                stall: std::time::Duration::from_secs(60),
            };
            let dir = tmp.path().to_path_buf();
            let mut running =
                tokio::spawn(
                    async move { fetch_missing_with(&dir, id, &files, limits, |_| {}).await },
                );
            let begun = std::time::Instant::now();
            while current().is_none_or(|p| p.id != id) {
                assert!(
                    begun.elapsed() < std::time::Duration::from_secs(10),
                    "{id}: never started; finished={} current={:?} result={:?}",
                    running.is_finished(),
                    current(),
                    if running.is_finished() {
                        Some((&mut running).await)
                    } else {
                        None
                    }
                );
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let asked = std::time::Instant::now();
            stop();
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), running)
                .await
                .expect("Stop must not wait for the server")
                .unwrap();
            assert_eq!(result, Err(STOPPED.to_string()), "{id}");
            assert!(
                asked.elapsed() < std::time::Duration::from_millis(1500),
                "{id}"
            );
            assert!(names_in(tmp.path()).is_empty(), "{id}");
        }
    }

    #[tokio::test]
    async fn the_runtime_is_not_downloaded_from_here() {
        let tmp = tempfile::tempdir().unwrap();
        let err = download(tmp.path(), model_catalog::GPU_RUNTIME, |_| {})
            .await
            .unwrap_err();
        assert!(err.contains("arrives with"), "{err}");
        assert!(download(tmp.path(), "nothing", |_| {}).await.is_err());
        assert!(names_in(tmp.path()).is_empty());
    }

    /// A folder that is a link is left alone: the files it points at are kept
    /// somewhere else.
    #[cfg(windows)]
    #[tokio::test]
    async fn nothing_is_written_through_a_junction() {
        let tmp = tempfile::tempdir().unwrap();
        let kept = tmp.path().join("kept");
        std::fs::create_dir(&kept).unwrap();
        let link = tmp.path().join("linked");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&kept)
            .output()
            .is_ok_and(|o| o.status.success());
        if !made {
            eprintln!("could not make a junction here; skipped");
            return;
        }
        let files = [file("a", "https://127.0.0.1:9/a".into(), b"a")];
        let err = fetch_missing(&link, "test", &files, |_| {})
            .await
            .unwrap_err();
        assert!(err.contains("link"), "{err}");
        assert!(names_in(&kept).is_empty());

        // What is already there through the link is used.
        std::fs::write(kept.join("a"), b"a").unwrap();
        assert_eq!(fetch_missing(&link, "test", &files, |_| {}).await, Ok(0));
        assert_eq!(ensure(&link, "test", &files, |_| {}).await, Ok(0));
        assert_eq!(names_in(&kept), ["a"]);
    }
}
