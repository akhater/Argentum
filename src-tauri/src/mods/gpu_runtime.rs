//! Runs Argentum's AI enlargement on the graphics card where Windows can.
//!
//! RapidRAW ships the CPU build of ONNX Runtime 1.22. Microsoft publishes the
//! same release built with DirectML, which runs on any DirectX 12 GPU, Intel,
//! AMD or NVIDIA, and still carries the CPU path every other AI feature uses.
//! On an Intel Arc 140V it enlarged a 512px tile 18x faster at 2x and 13x
//! faster at 4x, with output identical to the CPU's.
//!
//! It is not bundled. Like the models, it is downloaded the first time
//! super resolution runs, from our copy of Microsoft's own NuGet package, or
//! from NuGet when ours cannot be reached (`mirror`), and pinned by hash: an
//! 18 MB download, of which the 16 MB runtime for this machine is kept.
//!
//! ONE RUNTIME PER PROCESS
//!
//! ONNX Runtime is loaded once, on first use, from whatever path `ort` was
//! given, and stays loaded. `lib.rs` points `ORT_DYLIB_PATH` at the CPU build
//! during setup; `ort::init_from` takes precedence over that variable. So
//! `pin_if_installed`, called from `startup::init` before anything runs a model,
//! makes the DirectML build the one that loads. When it is downloaded during a
//! run in which another AI feature already loaded the CPU build, enlargement
//! stays on the CPU until the next launch.
//!
//! DirectML.dll itself is not downloaded. The runtime delay-loads it, so it is
//! only touched when a GPU session is made, and Windows 11 ships a recent enough
//! copy in System32 (1.15.5 on the machine this was measured on, where ONNX
//! Runtime 1.22 asks for 1.15.4). Where it is missing or too old, making the GPU
//! session fails and enlargement falls back to the CPU.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use ort::session::Session;

use super::mirror::mirror;
use super::model_catalog::File;

/// The runtime's folder inside the models folder, named for its version
/// (`model_catalog::GPU_RUNTIME` finds it, and older ones, by its prefix).
#[cfg_attr(not(windows), allow(dead_code))]
pub const FOLDER: &str = "onnxruntime-directml-1.22.0";

/// Microsoft's ONNX Runtime 1.22.0 DirectML package, as published on NuGet.
/// `model_download` fetches it into `FOLDER`, where the runtime is taken out
/// of it and the package removed.
#[cfg_attr(not(windows), allow(dead_code))]
pub const PACKAGE: File = File {
    name: "microsoft.ml.onnxruntime.directml.1.22.0.nupkg",
    url: mirror!(
        "onnxruntime-1.22.0",
        "microsoft.ml.onnxruntime.directml.1.22.0.nupkg"
    ),
    fallback: Some(
        "https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime.directml/1.22.0/microsoft.ml.onnxruntime.directml.1.22.0.nupkg",
    ),
    sha256: "29f9872d786236b79aa83f94482f3a17c14297e4833768d6d0ed4883ee732e60",
    bytes: 17_898_472,
};

/// Set the moment this process points ONNX Runtime at the DirectML build.
///
/// `ort` loads the library lazily, on first use, from the path it was given,
/// and panics if nothing is there. So once pointed, the runtime has to stay on
/// disk until the app exits even if no model has run yet — deleting it would
/// take down the next AI feature used, masks and all. The AI models page in
/// Settings asks this before removing it, and removes it at the next start
/// instead. Set conservatively: also when the call turned out to be a no-op
/// because another build had already loaded.
static POINTED_AT: AtomicBool = AtomicBool::new(false);

/// Whether the DirectML runtime must stay on disk until this run ends.
pub fn held_until_exit() -> bool {
    POINTED_AT.load(Ordering::SeqCst)
}

#[cfg_attr(not(windows), allow(dead_code))]
fn point_at(path: &Path) {
    POINTED_AT.store(true, Ordering::SeqCst);
    let _ = ort::init_from(path.to_string_lossy());
}

/// Remembers that a file was fully verified, so the next start need not read it.
///
/// What the full SHA-256 protects against: a download that stopped half way, a
/// file left by another version, a file replaced by something else. None of
/// those keep the verified file's size AND modification time. So beside the
/// runtime sits a one-line record of the pinned hash, the size and the
/// modification time it had when its whole content last matched that hash. A
/// start that finds the file as the record describes it trusts it; any other
/// state (no record, an unreadable one, a different size or time, a different
/// pinned hash) reads and hashes the whole file again and rewrites the record
/// only if it matches. A file that does not match is never accepted.
///
/// What it does not stop: someone who can write to this folder can also write
/// the record, or keep the file's time, or replace the app. The hash at every
/// start did not stop them either (it is checked, then loaded later), and it
/// never claimed to. Nor is damage that keeps both size and time (bit rot)
/// noticed any more; the runtime would fail to load and the CPU build is used.
#[cfg_attr(not(windows), allow(dead_code))]
mod stamp {
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::time::UNIX_EPOCH;

    fn record_path(file: &Path) -> PathBuf {
        file.with_extension("dll.verified")
    }

    /// Size and modification time (nanoseconds since 1970) of `file` now.
    fn describe(file: &Path) -> Option<(u64, u128)> {
        let meta = fs::metadata(file).ok()?;
        let time = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        Some((meta.len(), time.as_nanos()))
    }

    fn line(sha256: &str, size: u64, nanos: u128) -> String {
        format!("{sha256} {size} {nanos}\n")
    }

    /// Whether `file`'s whole content hashes to `sha256`. `hash` reads the file
    /// and is only called when the record does not vouch for it.
    ///
    /// Size and time are taken BEFORE the hash and again after it; the record
    /// is written, and the file accepted, only if the two readings agree. A
    /// file written while it was being read would otherwise be vouched for by
    /// a record made from content that was never hashed.
    pub fn verified(
        file: &Path,
        sha256: &str,
        hash: impl FnOnce(&Path) -> io::Result<String>,
    ) -> bool {
        let Some((size, nanos)) = describe(file) else {
            return false;
        };
        let record = record_path(file);
        if fs::read_to_string(&record).is_ok_and(|text| text == line(sha256, size, nanos)) {
            return true;
        }
        if hash(file).is_ok_and(|found| found == sha256) {
            if describe(file) == Some((size, nanos)) {
                write_record(file, sha256, size, nanos);
                return true;
            }
            let _ = fs::remove_file(record);
            return false;
        }
        let _ = fs::remove_file(record);
        false
    }

    /// Size and modification time of `file` now, to hand to `remember_as`.
    pub fn snapshot(file: &Path) -> Option<(u64, u128)> {
        describe(file)
    }

    /// Writes the record for `file` as it was when `stamp` was taken, for a
    /// file whose content the caller has just verified (taken from a checked
    /// download). Best effort: without it the next start hashes again.
    pub fn remember_as(file: &Path, sha256: &str, stamp: Option<(u64, u128)>) {
        if let Some((size, nanos)) = stamp {
            write_record(file, sha256, size, nanos);
        }
    }

    /// `remember_as` for a file read in full just now.
    #[cfg(test)]
    pub fn remember(file: &Path, sha256: &str) {
        remember_as(file, sha256, describe(file));
    }

    fn write_record(file: &Path, sha256: &str, size: u64, nanos: u128) {
        let record = record_path(file);
        let temporary = record.with_extension("tmp");
        if fs::write(&temporary, line(sha256, size, nanos)).is_ok()
            && fs::rename(&temporary, &record).is_err()
        {
            let _ = fs::remove_file(&temporary);
        }
    }
}

/// Where a session runs. Only Windows ever has a GPU here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    #[cfg_attr(not(windows), allow(dead_code))]
    Gpu,
    Cpu,
}

impl Device {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Gpu => "GPU",
            Self::Cpu => "CPU",
        }
    }
}

/// Makes the DirectML runtime the one this process loads, if it is installed.
///
/// Called once from `startup::init`, before any model runs. A runtime that is
/// missing, damaged or built for another ONNX Runtime release is left alone and
/// the CPU build loads as it always has. Never fails: a broken runtime here
/// would take every AI feature with it.
pub fn pin_if_installed(app: &tauri::AppHandle) {
    #[cfg(windows)]
    windows_runtime::pin_if_installed(app);
    #[cfg(not(windows))]
    let _ = app;
}

/// The device enlargement will use, downloading the GPU runtime the first time.
///
/// Any failure — no network, an old ONNX Runtime already loaded this run, no
/// DirectML — answers `Cpu`. Enlargement still works; it is only slower.
pub async fn prepare(app: &tauri::AppHandle) -> Device {
    #[cfg(windows)]
    {
        windows_runtime::prepare(app).await
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Device::Cpu
    }
}

/// A session for `model` on `device`, or on the CPU if the GPU will not take it.
pub fn session(model: &Path, device: Device) -> Result<(Session, Device)> {
    #[cfg(windows)]
    {
        if device == Device::Gpu {
            match windows_runtime::gpu_session(model) {
                Ok(session) => return Ok((session, Device::Gpu)),
                Err(error) => log::warn!("GPU session failed, using the CPU: {error:#}"),
            }
        }
    }
    #[cfg(not(windows))]
    let _ = device;

    let session = crate::mods::ai_session::builder()?
        .commit_from_file(model)
        .with_context(|| format!("Could not load {}", model.display()))?;
    Ok((session, Device::Cpu))
}

#[cfg(windows)]
mod windows_runtime {
    use std::fs;
    use std::io::{Read, Write};
    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result, anyhow, bail};
    use ort::execution_providers::{DirectMLExecutionProvider, ExecutionProvider};
    use ort::session::Session;
    use sha2::{Digest, Sha256};
    use tauri::Emitter;
    use tokio::sync::Mutex as TokioMutex;

    use super::Device;
    use crate::mods::model_catalog::GPU_RUNTIME;
    use crate::mods::model_download;

    /// The ONNX Runtime release the DirectML package below was built from.
    /// `ort` asks the loaded library for this API version and panics if it is
    /// older, so a runtime is only ever pinned when the two agree.
    const RUNTIME_API_VERSION: u32 = 22;

    struct Build {
        entry: &'static str,
        sha256: &'static str,
    }

    #[cfg(target_arch = "x86_64")]
    const BUILD: Option<Build> = Some(Build {
        entry: "runtimes/win-x64/native/onnxruntime.dll",
        sha256: "95366724919f4e95ecc60010912ed538ad9804b6683fbd0aad389749102834b9",
    });
    #[cfg(target_arch = "aarch64")]
    const BUILD: Option<Build> = Some(Build {
        entry: "runtimes/win-arm64/native/onnxruntime.dll",
        sha256: "c544001fbb76c7217fce76e9c24dada4a35d263b7ae3c0024474d9d7323a888f",
    });
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    const BUILD: Option<Build> = None;

    static INSTALL: TokioMutex<()> = TokioMutex::const_new(());

    fn runtime_path(app: &tauri::AppHandle) -> Result<PathBuf> {
        Ok(crate::mods::model_manager::models_dir(app)
            .map_err(anyhow::Error::msg)?
            .join(super::FOLDER)
            .join("onnxruntime.dll"))
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    /// SHA-256 of a whole file, read in chunks.
    fn sha256_file(path: &Path) -> std::io::Result<String> {
        let mut file = fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 1 << 20];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                return Ok(hex::encode(hasher.finalize()));
            }
            hasher.update(&buffer[..read]);
        }
    }

    /// The runtime on disk is the pinned build: by its record when the file is
    /// as last verified, otherwise by reading all of it (see `stamp`).
    fn installed(path: &Path, build: &Build) -> bool {
        super::stamp::verified(path, build.sha256, sha256_file)
    }

    fn api_matches() -> bool {
        ort::sys::ORT_API_VERSION == RUNTIME_API_VERSION
    }

    pub fn pin_if_installed(app: &tauri::AppHandle) {
        let Some(build) = BUILD else { return };
        if !api_matches() {
            return;
        }
        let Ok(path) = runtime_path(app) else { return };
        if installed(&path, &build) {
            super::point_at(&path);
        }
    }

    pub async fn prepare(app: &tauri::AppHandle) -> Device {
        match install_and_activate(app).await {
            Ok(true) => Device::Gpu,
            Ok(false) => Device::Cpu,
            Err(error) => {
                log::warn!("GPU runtime unavailable, using the CPU: {error:#}");
                Device::Cpu
            }
        }
    }

    async fn install_and_activate(app: &tauri::AppHandle) -> Result<bool> {
        let Some(build) = BUILD else { return Ok(false) };
        if !api_matches() {
            return Ok(false);
        }
        let path = runtime_path(app)?;
        {
            let _guard = INSTALL.lock().await;
            if !installed(&path, &build) {
                install(app, &path, &build).await?;
            }
        }
        // A no-op when ONNX Runtime is already loaded; then the check below
        // reports whichever build that was.
        super::point_at(&path);
        Ok(DirectMLExecutionProvider::default().is_available()?)
    }

    async fn install(app: &tauri::AppHandle, path: &Path, build: &Build) -> Result<()> {
        const LABEL: &str = "GPU runtime (ONNX Runtime DirectML)";
        let _ = app.emit(crate::mods::events::AI_MODEL_DOWNLOAD_START, LABEL);
        let result = async {
            let dir = path.parent().context("GPU runtime path has no parent")?;
            // Our copy, then NuGet; size and SHA-256 checked before it is kept.
            model_download::fetch_missing(dir, GPU_RUNTIME, &[super::PACKAGE], |_| {})
                .await
                .map_err(anyhow::Error::msg)
                .context("Could not download the GPU runtime")?;
            let package_path = dir.join(super::PACKAGE.name);
            let package = fs::read(&package_path)?;
            let _ = fs::remove_file(&package_path);
            let dll = zip_entry(&package, build.entry)?;
            if sha256_hex(&dll) != build.sha256 {
                bail!("The GPU runtime failed integrity verification");
            }
            let temporary = path.with_extension("dll.download");
            {
                let mut file = fs::File::create(&temporary)?;
                file.write_all(&dll)?;
                file.sync_all()?;
            }
            // Taken from the file we wrote and hashed, before it takes its name:
            // a rename keeps both.
            let written = super::stamp::snapshot(&temporary);
            fs::rename(&temporary, path)?;
            super::stamp::remember_as(path, build.sha256, written);
            Ok(())
        }
        .await;
        let _ = app.emit(crate::mods::events::AI_MODEL_DOWNLOAD_FINISH, LABEL);
        result
    }

    pub fn gpu_session(model: &Path) -> Result<Session> {
        // DirectML cannot run with memory patterns or parallel execution.
        Ok(crate::mods::ai_session::builder()?
            .with_parallel_execution(false)?
            .with_memory_pattern(false)?
            .with_execution_providers([DirectMLExecutionProvider::default()
                .build()
                .error_on_failure()])?
            .commit_from_file(model)?)
    }

    /// One file out of a zip archive: a NuGet package is one.
    ///
    /// Only what the package uses — stored or deflated entries, no ZIP64 —
    /// rather than a zip crate for the sake of one file.
    pub(super) fn zip_entry(archive: &[u8], name: &str) -> Result<Vec<u8>> {
        let u16_at = |at: usize| -> Result<usize> {
            archive
                .get(at..at + 2)
                .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
                .ok_or_else(|| anyhow!("Truncated zip archive"))
        };
        let u32_at = |at: usize| -> Result<usize> {
            archive
                .get(at..at + 4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
                .ok_or_else(|| anyhow!("Truncated zip archive"))
        };

        let search_from = archive.len().saturating_sub(22 + usize::from(u16::MAX));
        let end = (search_from..archive.len().saturating_sub(21))
            .rev()
            .find(|&at| archive[at..at + 4] == [0x50, 0x4b, 0x05, 0x06])
            .ok_or_else(|| anyhow!("Not a zip archive"))?;
        let count = u16_at(end + 10)?;
        let mut at = u32_at(end + 16)?;

        for _ in 0..count {
            if archive.get(at..at + 4) != Some(&[0x50, 0x4b, 0x01, 0x02]) {
                bail!("Damaged zip directory");
            }
            let method = u16_at(at + 10)?;
            let compressed = u32_at(at + 20)?;
            let size = u32_at(at + 24)?;
            let name_len = u16_at(at + 28)?;
            let extra_len = u16_at(at + 30)?;
            let comment_len = u16_at(at + 32)?;
            let local = u32_at(at + 42)?;
            let entry_name = archive
                .get(at + 46..at + 46 + name_len)
                .ok_or_else(|| anyhow!("Truncated zip archive"))?;

            if entry_name == name.as_bytes() {
                if archive.get(local..local + 4) != Some(&[0x50, 0x4b, 0x03, 0x04]) {
                    bail!("Damaged zip entry");
                }
                let start = local + 30 + u16_at(local + 26)? + u16_at(local + 28)?;
                let data = archive
                    .get(start..start + compressed)
                    .ok_or_else(|| anyhow!("Truncated zip entry"))?;
                let mut out = Vec::with_capacity(size);
                match method {
                    0 => out.extend_from_slice(data),
                    8 => {
                        flate2::read::DeflateDecoder::new(data).read_to_end(&mut out)?;
                    }
                    other => bail!("Unsupported zip compression method {other}"),
                }
                if out.len() != size {
                    bail!("Zip entry has the wrong size");
                }
                return Ok(out);
            }
            at += 46 + name_len + extra_len + comment_len;
        }
        bail!("{name} is not in the archive")
    }
}

#[cfg(all(test, windows))]
mod tests {
    use std::io::Write;

    use super::windows_runtime::zip_entry;

    /// A minimal zip, written the way the format says, with one entry per
    /// (name, data, deflate) triple.
    fn zip(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut directory = Vec::new();
        for (name, data, deflate) in entries {
            let stored = if *deflate {
                let mut encoder =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                encoder.write_all(data).unwrap();
                encoder.finish().unwrap()
            } else {
                data.to_vec()
            };
            let method: u16 = if *deflate { 8 } else { 0 };
            let offset = out.len() as u32;
            out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0]);
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0; 8]); // time, date, crc
            out.extend_from_slice(&(stored.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&stored);

            directory.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02, 20, 0, 20, 0, 0, 0]);
            directory.extend_from_slice(&method.to_le_bytes());
            directory.extend_from_slice(&[0; 8]);
            directory.extend_from_slice(&(stored.len() as u32).to_le_bytes());
            directory.extend_from_slice(&(data.len() as u32).to_le_bytes());
            directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
            directory.extend_from_slice(&[0; 12]); // extra, comment, disk, attributes
            directory.extend_from_slice(&offset.to_le_bytes());
            directory.extend_from_slice(name.as_bytes());
        }
        let directory_offset = out.len() as u32;
        out.extend_from_slice(&directory);
        out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
        out.extend_from_slice(&directory_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn reads_the_named_entry_stored_or_deflated() {
        let dll = b"MZ pretend runtime ".repeat(500);
        let archive = zip(&[
            ("LICENSE", b"MIT", false),
            ("runtimes/win-arm64/native/onnxruntime.dll", b"arm", true),
            ("runtimes/win-x64/native/onnxruntime.dll", &dll, true),
        ]);
        assert_eq!(
            zip_entry(&archive, "runtimes/win-x64/native/onnxruntime.dll").unwrap(),
            dll
        );
        assert_eq!(zip_entry(&archive, "LICENSE").unwrap(), b"MIT");
    }

    #[test]
    fn says_so_when_the_entry_is_missing_or_the_archive_is_not_one() {
        let archive = zip(&[("LICENSE", b"MIT", false)]);
        assert!(zip_entry(&archive, "onnxruntime.dll").is_err());
        assert!(zip_entry(b"not a zip at all", "LICENSE").is_err());
        assert!(zip_entry(&archive[..archive.len() - 30], "LICENSE").is_err());
    }

    /// The package pinned in `windows_runtime` is ONNX Runtime 1.22. When the
    /// `ort` crate moves to another release this fails, because pinning would
    /// then be skipped and the GPU silently lost: update the package with it.
    #[test]
    fn the_pinned_runtime_matches_the_ort_crate() {
        assert_eq!(ort::sys::ORT_API_VERSION, 22);
    }
}

#[cfg(test)]
mod stamp_tests {
    use std::cell::Cell;
    use std::fs;
    use std::io;
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    use super::stamp::{remember, verified};

    const GOOD: &str = "good-hash";

    /// A hasher that says `GOOD` for content "right" and counts its reads.
    fn judge<'a>(reads: &'a Cell<u32>) -> impl FnOnce(&Path) -> io::Result<String> + 'a {
        move |path| {
            reads.set(reads.get() + 1);
            Ok(if fs::read(path)? == b"right" {
                GOOD.into()
            } else {
                "other".into()
            })
        }
    }

    fn file_with(content: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("onnxruntime.dll");
        fs::write(&path, content).unwrap();
        (dir, path)
    }

    fn touch(path: &Path, seconds: u64) {
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() + Duration::from_secs(seconds))
            .unwrap();
    }

    #[test]
    fn a_file_with_the_right_content_is_accepted_after_one_full_read() {
        let (_dir, path) = file_with(b"right");
        let reads = Cell::new(0);
        assert!(verified(&path, GOOD, judge(&reads)));
        assert_eq!(reads.get(), 1);
    }

    #[test]
    fn a_file_with_the_wrong_content_is_refused_every_time() {
        let (_dir, path) = file_with(b"wrong");
        let reads = Cell::new(0);
        assert!(!verified(&path, GOOD, judge(&reads)));
        assert!(!verified(&path, GOOD, judge(&reads)));
        assert_eq!(reads.get(), 2, "a refusal is never remembered");
    }

    #[test]
    fn a_missing_file_is_refused_without_reading() {
        let dir = tempfile::tempdir().unwrap();
        let reads = Cell::new(0);
        assert!(!verified(&dir.path().join("none.dll"), GOOD, judge(&reads)));
        assert_eq!(reads.get(), 0);
    }

    #[test]
    fn a_verified_file_is_not_read_again() {
        let (_dir, path) = file_with(b"right");
        let reads = Cell::new(0);
        assert!(verified(&path, GOOD, judge(&reads)));
        assert!(verified(&path, GOOD, judge(&reads)));
        assert!(verified(&path, GOOD, judge(&reads)));
        assert_eq!(reads.get(), 1, "only the first start reads the file");
    }

    #[test]
    fn a_remembered_download_is_not_read_at_the_next_start() {
        let (_dir, path) = file_with(b"right");
        remember(&path, GOOD);
        let reads = Cell::new(0);
        assert!(verified(&path, GOOD, judge(&reads)));
        assert_eq!(reads.get(), 0);
    }

    #[test]
    fn a_file_of_another_size_is_verified_again_and_refused() {
        let (_dir, path) = file_with(b"right");
        remember(&path, GOOD);
        fs::write(&path, b"wrong!").unwrap();
        let reads = Cell::new(0);
        assert!(!verified(&path, GOOD, judge(&reads)));
        assert_eq!(reads.get(), 1);
    }

    #[test]
    fn a_file_of_the_same_size_and_a_new_time_is_verified_again_and_refused() {
        let (_dir, path) = file_with(b"right");
        remember(&path, GOOD);
        fs::write(&path, b"wrong").unwrap();
        touch(&path, 60);
        let reads = Cell::new(0);
        assert!(!verified(&path, GOOD, judge(&reads)));
        assert_eq!(reads.get(), 1);
        // And the refusal took the record with it: it never vouches again.
        let again = Cell::new(0);
        assert!(!verified(&path, GOOD, judge(&again)));
        assert_eq!(again.get(), 1);
    }

    #[test]
    fn a_file_written_while_it_is_hashed_is_not_accepted_or_remembered() {
        let (_dir, path) = file_with(b"right");
        let reads = Cell::new(0);
        let changed = |p: &Path| {
            reads.set(reads.get() + 1);
            // Right content when read, then replaced before the hash returns.
            let content = fs::read(p)?;
            fs::write(p, b"wrong")?;
            touch(p, 60);
            Ok(if content == b"right" {
                GOOD.into()
            } else {
                "other".into()
            })
        };
        assert!(!verified(&path, GOOD, changed));
        assert!(!path.with_extension("dll.verified").exists());
        // Nothing vouches for it on the next start either.
        let next = Cell::new(0);
        assert!(!verified(&path, GOOD, judge(&next)));
        assert_eq!(next.get(), 1);
    }

    #[test]
    fn a_new_time_on_the_right_content_is_verified_again_and_accepted() {
        let (_dir, path) = file_with(b"right");
        remember(&path, GOOD);
        touch(&path, 60);
        let reads = Cell::new(0);
        assert!(verified(&path, GOOD, judge(&reads)));
        assert_eq!(reads.get(), 1);
        assert!(verified(&path, GOOD, judge(&reads)), "and remembered anew");
        assert_eq!(reads.get(), 1);
    }

    #[test]
    fn a_missing_stale_or_unreadable_record_forces_a_full_check() {
        let (_dir, path) = file_with(b"right");
        let record = path.with_extension("dll.verified");
        for text in [None, Some("garbage"), Some(""), Some("good-hash 5\n")] {
            let _ = fs::remove_file(&record);
            if let Some(text) = text {
                fs::write(&record, text).unwrap();
            }
            let reads = Cell::new(0);
            assert!(verified(&path, GOOD, judge(&reads)));
            assert_eq!(reads.get(), 1, "record {text:?}");
        }
    }

    #[test]
    fn a_record_for_another_pinned_hash_does_not_vouch() {
        let (_dir, path) = file_with(b"right");
        remember(&path, "an-older-pin");
        let reads = Cell::new(0);
        assert!(verified(&path, GOOD, judge(&reads)));
        assert_eq!(reads.get(), 1);
    }
}
