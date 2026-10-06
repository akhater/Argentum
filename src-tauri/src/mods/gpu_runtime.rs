//! Runs Argentum's AI enlargement on the graphics card where Windows can.
//!
//! RapidRAW ships the CPU build of ONNX Runtime 1.22. Microsoft publishes the
//! same release built with DirectML, which runs on any DirectX 12 GPU, Intel,
//! AMD or NVIDIA, and still carries the CPU path every other AI feature uses.
//! On an Intel Arc 140V it enlarged a 512px tile 18x faster at 2x and 13x
//! faster at 4x, with output identical to the CPU's.
//!
//! It is not bundled. Like the models, it is downloaded the first time
//! super resolution runs, from Microsoft's own NuGet package, and pinned by
//! hash: an 18 MB download, of which the 16 MB runtime for this machine is kept.
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

    let session = Session::builder()?
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
    use tauri::{Emitter, Manager};
    use tokio::sync::Mutex as TokioMutex;

    use super::Device;

    /// The ONNX Runtime release the DirectML package below was built from.
    /// `ort` asks the loaded library for this API version and panics if it is
    /// older, so a runtime is only ever pinned when the two agree.
    const RUNTIME_API_VERSION: u32 = 22;

    const PACKAGE_URL: &str = "https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime.directml/1.22.0/microsoft.ml.onnxruntime.directml.1.22.0.nupkg";
    const PACKAGE_SHA256: &str = "29f9872d786236b79aa83f94482f3a17c14297e4833768d6d0ed4883ee732e60";

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
        Ok(app
            .path()
            .app_data_dir()?
            .join("models")
            .join("onnxruntime-directml-1.22.0")
            .join("onnxruntime.dll"))
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    fn installed(path: &Path, build: &Build) -> bool {
        fs::read(path).is_ok_and(|bytes| sha256_hex(&bytes) == build.sha256)
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
        let _ = app.emit("ai-model-download-start", LABEL);
        let result = async {
            let package = reqwest::get(PACKAGE_URL)
                .await
                .context("Could not download the GPU runtime")?
                .error_for_status()
                .context("The GPU runtime server returned an error")?
                .bytes()
                .await
                .context("Could not read the downloaded GPU runtime")?;
            if sha256_hex(&package) != PACKAGE_SHA256 {
                bail!("The GPU runtime package failed integrity verification");
            }
            let dll = zip_entry(&package, build.entry)?;
            if sha256_hex(&dll) != build.sha256 {
                bail!("The GPU runtime failed integrity verification");
            }
            let dir = path.parent().context("GPU runtime path has no parent")?;
            fs::create_dir_all(dir)?;
            let temporary = path.with_extension("dll.download");
            {
                let mut file = fs::File::create(&temporary)?;
                file.write_all(&dll)?;
                file.sync_all()?;
            }
            fs::rename(&temporary, path)?;
            Ok(())
        }
        .await;
        let _ = app.emit("ai-model-download-finish", LABEL);
        result
    }

    pub fn gpu_session(model: &Path) -> Result<Session> {
        // DirectML cannot run with memory patterns or parallel execution.
        Ok(Session::builder()?
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
