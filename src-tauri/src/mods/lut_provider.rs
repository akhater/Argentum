//! A local program that makes a LUT for one photo. Ours.
//!
//! An optional setting, `lutProvider` in `argentum-processing.json`, names one
//! local program. Argentum never ships or writes that field: only a hand edit
//! adds it, and absent or empty means off, with nothing shown and nothing run.
//! One run per request, one word on the command line, one JSON object on stdin
//! and one on stdout. A run that never starts costs nothing.
//!
//! Only this module runs the program, and nothing here can write the field, so
//! no command, edit or answer can choose what runs. An edit can only name a
//! camera, a look and its options, and those count only if the program itself
//! lists them.
//!
//! One option can have a look follow the photo's white balance: the program
//! flags it in `list`, with what it means left to the program. With it on, the
//! white balance the photo renders to goes to the program as `currentWb`, in the
//! form of `asShotWb` and equal to it at the camera's. Any other goes into the
//! name of the file too, so each has its own file and going back to one needs no
//! run ([`Subject::coefficients_of`]).
//! Whatever comes back is treated as untrusted: its size is capped, its ids must
//! fit a file name, its PNG is checked, and no path is taken from it. The
//! program writes one scratch file named here, and only Argentum writes beside a
//! photo, whole or not at all (`write_atomic`).
//!
//! The [`Queue`] holds the work: photos asking for the same thing share a run, a
//! photo waits for one look at a time, two run at once, and a run nobody waits
//! for any more is killed. Rust only makes files. Putting a look on a photo is
//! the interface's, as for any look, so Undo stays in one place
//! (src/argentum/looks/).
//!
//! Preview all goes through the same queue, when its button asks and never
//! otherwise: every look of one photo, each to a temporary file to compare by
//! hover. Those files have hidden names keyed on the photo's whole path, the
//! camera, the look and the options ([`preview_file_for`]), so one is never a
//! photo, a look file or another setting's preview. They are kept to a budget
//! and go at close and at the next start, whatever ended the last one. Only
//! putting a look on leaves a file beside the photo, reusing the preview.
//!
//! Delete all previews clears one photo when its button asks and never
//! otherwise: its look files go to the Recycle Bin, but for those an edit uses,
//! and its previews and what is on its way to them go too.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, mpsc};
use std::time::{Duration, Instant};

use image::{ImageFormat, ImageReader};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;
use crate::file_management::{parse_virtual_path, read_file_mapped};
use crate::lut_processing::{self, Lut};
use crate::mods::ag_settings::Setting;
use crate::mods::atomic_write::write_atomic;
use crate::mods::file_ops::{LookOptions, LookWb, OwnWb};
use crate::mods::profiles::{self, same_model};
use crate::mods::{dng_standard, file_ops, provider_look};
use crate::white_balance::WhiteBalance;

/// The event that reports each photo's result.
pub const EVENT: &str = "provider-look-progress";

/// The program. Read here and nowhere else; never saved.
const PROGRAM: Setting<String> = Setting::new("lutProvider");

/// The Provider looks switch, saved only when the user flips it.
const SWITCH: Setting<bool> = Setting::new("providerLooksOn");

const PROTOCOL: u32 = 1;
/// Steps of the LUT; a PNG LUT must be square, so 65 cannot be stored as one.
const GRID: u32 = 64;
/// The side of a 64-step LUT's PNG, in pixels.
const SIDE_PX: u32 = 512;
const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
/// What is read of stdout and of stderr (the log's, only).
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_NAME_CHARS: usize = 80;
/// What is kept of the options on a list.
const MAX_OPTIONS: usize = 8;
/// What is kept of the groups on a list. Past this, looks stay, under no heading.
const MAX_GROUPS: usize = 32;
/// For what was asked for by hand: a click, a batch, an export.
const RUNS_AT_ONCE: usize = 2;
/// For everything together, with Preview all: more would hold up a smaller machine.
const RUNS_WITH_PREVIEWS: usize = 4;
/// The folder under the app's cache: scratch files, and previews in `PREVIEW_FOLDER`.
const SCRATCH_FOLDER: &str = "lut_provider";
const PREVIEW_FOLDER: &str = "previews";
/// Past this, a press of Preview all drops the oldest: a look's file is about
/// 170 KB, so about 18 photos' sets of ten looks.
const MAX_PREVIEW_BYTES: u64 = 32 * 1024 * 1024;
/// Of the hash of the photo's path in a preview's name.
const PREVIEW_HASH_CHARS: usize = 32;
/// In a row, before the provider is off until the app closes.
const CRASHES_TO_OFF: u32 = 2;
/// How long a closing app waits for its runs to go.
const STOP_WAIT: Duration = Duration::from_secs(2);
/// How often an export waiting for a look looks at its cancellation token.
const EXPORT_POLL: Duration = Duration::from_millis(100);

/// How long the program gets.
#[derive(Clone, Copy)]
pub struct Limits {
    list: Duration,
    lut: Duration,
}

const LIMITS: Limits = Limits {
    list: Duration::from_secs(10),
    // About ten times the slowest run expected.
    lut: Duration::from_secs(60),
};

/// A camera the program offers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OfferedCamera {
    pub id: String,
    pub name: String,
    pub make: String,
    pub model: String,
}

/// A look the program offers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OfferedLook {
    pub id: String,
    pub name: String,
    /// The heading it shows under, named by the program; with none, it has no heading.
    #[serde(default, deserialize_with = "text_or_none")]
    pub group: Option<String>,
}

/// A text as it came, and anything else as none: one field of the wrong shape
/// must not cost a look its place on the list.
fn text_or_none<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(Value::deserialize(d)?.as_str().map(str::to_string))
}

/// An on/off choice the program offers. What it means is the program's: only
/// its label is shown and only its id and value are sent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct OfferedOption {
    pub id: String,
    pub label: String,
    pub default: bool,
    /// With it on, looks follow the photo's white balance. Programs from before
    /// this flag have none.
    #[serde(default)]
    pub sends_current_wb: bool,
}

/// What `list` answered, after validation.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    pub cameras: Vec<OfferedCamera>,
    pub looks: Vec<OfferedLook>,
    pub options: Vec<OfferedOption>,
}

impl Catalog {
    /// Whether the program lists this look and every one of `options`.
    fn lists(&self, look: &str, options: &LookOptions) -> bool {
        self.looks.iter().any(|l| l.id == look)
            && options
                .keys()
                .all(|id| self.options.iter().any(|option| option.id == *id))
    }

    /// Every option it offers, at its default.
    fn option_defaults(&self) -> LookOptions {
        let defaults = self.options.iter();
        defaults.map(|o| (o.id.clone(), o.default)).collect()
    }

    /// The option that has a look follow the photo's white balance, if `options`
    /// have it on.
    fn wb_option(&self, options: &LookOptions) -> Option<&str> {
        let on =
            |o: &&OfferedOption| o.sends_current_wb && *options.get(&o.id).unwrap_or(&o.default);
        self.options.iter().find(on).map(|o| o.id.as_str())
    }

    /// How a look of `subject` with `options` follows its white balance. Reads
    /// its saved edit, with `changes` over it, only if an option asks for that.
    fn own_wb(
        &self,
        subject: &Subject,
        options: &LookOptions,
        changes: Option<&Value>,
    ) -> Option<OwnWb> {
        let option = self.wb_option(options)?.to_string();
        let wb = subject.look_wb(&edit_of(&subject.photo, changes));
        Some(OwnWb { option, wb })
    }
}

// ------------------------------------------------------------------ the setting

/// The program the setting names; None when it is absent or empty.
fn configured(library: &Path) -> Option<String> {
    PROGRAM
        .load(library)
        .filter(|program| !program.trim().is_empty())
}

/// Whether provider looks are the usable list. Starts on.
fn is_on(library: &Path) -> bool {
    SWITCH.load(library).unwrap_or(true)
}

/// Remembers the switch, only for someone with a provider.
fn set_on(library: &Path, on: bool) -> Result<(), String> {
    if configured(library).is_none() {
        return Err("no provider is set".to_string());
    }
    SWITCH.save(library, &on)
}

// ------------------------------------------------------------------ validation

/// The program's path, if it is one that may run: absolute, local, no `..`, a
/// file, and runnable as it stands. A `.cmd` would go through a shell.
fn check_path(program: &str) -> Result<PathBuf, String> {
    let path = Path::new(program);
    if program.starts_with(r"\\") || program.starts_with("//") {
        return Err("a network path".to_string());
    }
    if !path.is_absolute() {
        return Err("not a full path".to_string());
    }
    if path.components().any(|c| c == Component::ParentDir) {
        return Err("a path with ..".to_string());
    }
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a file".to_string());
    }
    if !is_runnable(path, &meta) {
        return Err("not a program".to_string());
    }
    Ok(path.to_path_buf())
}

#[cfg(windows)]
fn is_runnable(path: &Path, _meta: &std::fs::Metadata) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
}

#[cfg(unix)]
fn is_runnable(_path: &Path, meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

/// Whether `id` can name an option. It goes to the program and into an edit,
/// never into a file name, so it need not fit one: `^[A-Za-z0-9][A-Za-z0-9_-]{0,31}$`.
fn is_option_id(id: &str) -> bool {
    id.len() <= 32
        && id.starts_with(|c: char| c.is_ascii_alphanumeric())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The entries that read as a `T` and pass, the first of each id.
fn kept<T: DeserializeOwned>(
    entries: Vec<Value>,
    id_of: impl Fn(&T) -> &str,
    good: impl Fn(&T) -> bool,
) -> Vec<T> {
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .filter_map(|entry| serde_json::from_value::<T>(entry).ok())
        .filter(|entry| good(entry) && seen.insert(id_of(entry).to_string()))
        .collect()
}

fn good_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= MAX_NAME_CHARS
}

/// Each look's group fit to show, of the first `MAX_GROUPS`, or none: the group
/// is a heading and the key of its fold, and the look is worth more than either.
fn keep_groups(looks: &mut [OfferedLook]) {
    let mut seen = HashSet::new();
    for look in looks {
        look.group = look
            .group
            .take()
            .map(|group| group.trim().to_string())
            .filter(|group| good_name(group) && !group.chars().any(char::is_control))
            .filter(|group| {
                seen.contains(group) || (seen.len() < MAX_GROUPS && seen.insert(group.clone()))
            });
    }
}

/// What `list` answered. A bad entry is dropped and the rest stay; with no
/// camera or no look left there is nothing to offer. Options are extra: anything
/// but a list of them is none.
fn read_list(answer: &[u8]) -> Result<Catalog, String> {
    #[derive(Deserialize)]
    struct List {
        protocol: u32,
        generator: Option<String>,
        #[serde(default)]
        cameras: Vec<Value>,
        #[serde(default)]
        looks: Vec<Value>,
        #[serde(default)]
        options: Value,
    }
    let list: List = serde_json::from_slice(answer).map_err(|e| e.to_string())?;
    if list.protocol != PROTOCOL {
        return Err(format!("protocol {}", list.protocol));
    }
    let cameras = kept(
        list.cameras,
        |c: &OfferedCamera| &c.id,
        |c| {
            file_ops::is_look_name_part(&c.id)
                && good_name(&c.name)
                && !c.make.trim().is_empty()
                && !c.model.trim().is_empty()
        },
    );
    let mut looks = kept(
        list.looks,
        |l: &OfferedLook| &l.id,
        |l| file_ops::is_look_name_part(&l.id) && good_name(&l.name),
    );
    keep_groups(&mut looks);
    let mut options = kept(
        match list.options {
            Value::Array(entries) => entries,
            _ => Vec::new(),
        },
        |o: &OfferedOption| &o.id,
        |o| is_option_id(&o.id) && good_name(&o.label),
    );
    options.truncate(MAX_OPTIONS);
    if cameras.is_empty() || looks.is_empty() {
        return Err("no camera or no look on the list".to_string());
    }
    if let Some(generator) = list.generator {
        log::info!("LUT provider: {generator}");
    }
    Ok(Catalog {
        cameras,
        looks,
        options,
    })
}

/// Checks the program and asks what it offers.
fn validate(program: &str, limits: &Limits) -> Result<Catalog, String> {
    let exe = check_path(program)?;
    let request = json!({ "protocol": PROTOCOL }).to_string();
    let never = AtomicBool::new(false);
    let answer = run(&exe, "list", request.as_bytes(), limits.list, &never)
        .map_err(|failure| format!("{failure:?}"))?;
    read_list(&answer)
}

/// The program in the setting and what it offers. For everyone without the
/// field this ends at the first line, with no process run.
fn offered(library: &Path, limits: &Limits) -> Option<(String, Catalog)> {
    let program = configured(library)?;
    match validate(&program, limits) {
        Ok(catalog) => Some((program, catalog)),
        Err(reason) => {
            log::warn!("LUT provider is off: {reason}");
            None
        }
    }
}

// ------------------------------------------------------------------ one run

/// Why a run gave no LUT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    /// It did not start, exited with an error, or answered nonsense.
    Crash,
    Timeout,
    Cancelled,
    /// Its answer was fine and its file was not.
    BadFile,
    /// It said so, with `ok: false`.
    Refused(Refusal),
}

/// The codes `ok: false` carries; one nobody knows counts as `Internal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    UnsupportedCamera,
    UnknownLook,
    BadRequest,
    DataMissing,
    Internal,
}

/// What became of one photo's request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum LookOutcome {
    /// Its look file is there.
    Done,
    Cancelled,
    /// "Couldn't make this look".
    Failed,
    /// The program does not offer this camera or look for it.
    Unsupported,
    /// Its folder can't be written.
    Folder,
}

impl Failure {
    fn outcome(self) -> LookOutcome {
        match self {
            Self::Cancelled => LookOutcome::Cancelled,
            Self::Refused(Refusal::UnsupportedCamera | Refusal::UnknownLook) => {
                LookOutcome::Unsupported
            }
            _ => LookOutcome::Failed,
        }
    }

    /// Whether the same request would fail the same way this session.
    fn is_final(self) -> bool {
        matches!(
            self,
            Self::Refused(Refusal::UnsupportedCamera | Refusal::UnknownLook | Refusal::DataMissing)
        )
    }
}

/// Reads a pipe up to the cap in the background, then keeps draining it so the
/// program never blocks on a full pipe. The answer is what was read, and
/// whether there was more.
fn capped(pipe: impl Read + Send + 'static) -> mpsc::Receiver<(Vec<u8>, bool)> {
    let (tell, told) = mpsc::channel();
    std::thread::spawn(move || {
        let mut pipe = pipe;
        let mut kept = Vec::new();
        let mut more = false;
        let mut chunk = [0u8; 8192];
        while let Ok(n) = pipe.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let room = (MAX_OUTPUT_BYTES + 1).saturating_sub(kept.len()).min(n);
            kept.extend_from_slice(&chunk[..room]);
            more |= kept.len() > MAX_OUTPUT_BYTES;
        }
        kept.truncate(MAX_OUTPUT_BYTES);
        let _ = tell.send((kept, more));
    });
    told
}

/// The program and all it started, now.
fn kill(child: &mut Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let system = std::env::var_os("SystemRoot").map_or_else(PathBuf::new, PathBuf::from);
        let _ = Command::new(system.join("System32").join("taskkill.exe"))
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .creation_flags(NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// `CREATE_NO_WINDOW`.
#[cfg(windows)]
const NO_WINDOW: u32 = 0x0800_0000;

/// One run of the program: no shell, one fixed word, `input` on stdin, in its
/// own folder. What it wrote to stdout, or why not.
fn run(
    exe: &Path,
    word: &str,
    input: &[u8],
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, Failure> {
    let mut command = Command::new(exe);
    command
        .arg(word)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(folder) = exe.parent() {
        command.current_dir(folder);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|e| {
        log::warn!("LUT provider: could not start: {e}");
        Failure::Crash
    })?;

    let (Some(mut stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        kill(&mut child);
        return Err(Failure::Crash);
    };
    let input = input.to_vec();
    std::thread::spawn(move || {
        // A program that does not read its stdin is its own affair.
        let _ = stdin.write_all(&input);
    });
    let (stdout, stderr) = (capped(stdout), capped(stderr));

    let started = Instant::now();
    let status = loop {
        if cancel.load(Ordering::SeqCst) {
            kill(&mut child);
            return Err(Failure::Cancelled);
        }
        if started.elapsed() >= timeout {
            log::warn!("LUT provider: no answer to {word} in {timeout:?}");
            kill(&mut child);
            return Err(Failure::Timeout);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => {
                log::warn!("LUT provider: {e}");
                kill(&mut child);
                return Err(Failure::Crash);
            }
        }
    };

    // Something it started may still hold the pipes, so this does not wait long.
    let pipe_wait = Duration::from_secs(1);
    if let Ok((text, _)) = stderr.recv_timeout(pipe_wait)
        && !text.is_empty()
    {
        log::info!("LUT provider: {}", String::from_utf8_lossy(&text));
    }
    if !status.success() {
        log::warn!("LUT provider: {word} ended with {status}");
        return Err(Failure::Crash);
    }
    match stdout.recv_timeout(pipe_wait) {
        Ok((text, false)) => Ok(text),
        _ => {
            log::warn!("LUT provider: {word} answered too much or too late");
            Err(Failure::Crash)
        }
    }
}

/// The checks a LUT passes before it goes anywhere near a photo: small, 512 x
/// 512 (read from its header, before a byte is decoded), a 64-step LUT.
fn checked_file(bytes: &[u8]) -> Result<Lut, Failure> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(Failure::BadFile);
    }
    let reader = || ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
    let side = reader().into_dimensions().map_err(|_| Failure::BadFile)?;
    if side != (SIDE_PX, SIDE_PX) {
        return Err(Failure::BadFile);
    }
    let image = reader().decode().map_err(|_| Failure::BadFile)?;
    let lut = lut_processing::parse_hald(image).map_err(|_| Failure::BadFile)?;
    (lut.size == GRID).then_some(lut).ok_or(Failure::BadFile)
}

/// One photo, as the program is told of it.
#[derive(Debug, Clone)]
pub struct Subject {
    /// As the editor names it, a virtual copy's own path for a virtual copy.
    photo: String,
    make: String,
    model: String,
    iso: Option<f32>,
    /// As-shot white balance as the program is sent it: R, G, G, B, at the
    /// decoder's scale, where green is 1 in most formats but not all.
    wb: [f32; 4],
    /// The same white balance as the render holds it.
    white: WhiteBalance,
    /// What `white` was solved with.
    xyz_to_camera: Option<[f32; 9]>,
    /// The exposure offset Linear gives this photo.
    offset_ev: f32,
}

impl Subject {
    /// The white balance `edit` renders to, when it is one a look can be made
    /// for and not the camera's.
    fn look_wb(&self, edit: &Value) -> Option<LookWb> {
        let target = crate::image_processing::target_white_balance(edit, self.white);
        let wb = LookWb::of(target);
        (wb != LookWb::of(self.white) && self.coefficients_of(wb).is_some()).then_some(wb)
    }

    /// `wb` in the form of `asShotWb`, at its scale: what the camera's white
    /// balance would have been for the render's white to be `wb`.
    ///
    /// The decode keeps the as-shot white balance and a slider moves the picture
    /// after it, in the eye's cone space (`white_balance::adaptation_log_gains`).
    /// What ends up neutral is then what the as-shot matrix reads as `wb`, so that
    /// matrix is the one to ask, not one mixed for `wb`. That is the white the
    /// LUT must take back to neutral. Other colours move a little differently
    /// from a real change of the camera's white balance, as for any edit of it.
    fn coefficients_of(&self, wb: LookWb) -> Option<[f32; 4]> {
        let [red, green, blue] = wb.white().camera_neutral(self.xyz_to_camera.as_ref()?)?;
        let scale = f64::from(self.wb[1]) * green;
        let [r, b] = [scale / red, scale / blue].map(|c| c as f32);
        Some([r, self.wb[1], self.wb[1], b])
    }
}

/// The file the run left, or why not.
fn ask_for_lut(
    exe: &Path,
    limits: &Limits,
    subject: &Subject,
    key: &Key,
    options: &LookOptions,
    scratch: &Path,
    cancel: &AtomicBool,
) -> Result<(Vec<u8>, Lut), Failure> {
    let mut request = json!({
        "protocol": PROTOCOL,
        "camera": key.camera,
        "look": key.look,
        "photo": {
            "path": parse_virtual_path(&subject.photo).0,
            "make": subject.make,
            "model": subject.model,
            "iso": subject.iso,
        },
        "asShotWb": subject.wb,
        "rendering": { "mode": "linear", "offsetEv": subject.offset_ev },
        "output": scratch,
        "grid": GRID,
    });
    if !options.is_empty() {
        request["options"] = json!(options);
    }
    if let Some(own) = &key.own_wb {
        let current = own
            .wb
            .map_or(Some(subject.wb), |wb| subject.coefficients_of(wb));
        request["currentWb"] = json!(current.ok_or(Failure::Refused(Refusal::BadRequest))?);
    }
    let answer = run(
        exe,
        "lut",
        request.to_string().as_bytes(),
        limits.lut,
        cancel,
    )?;

    #[derive(Deserialize)]
    struct Answer {
        ok: bool,
        error: Option<String>,
        message: Option<String>,
    }
    let answer: Answer = serde_json::from_slice(&answer).map_err(|e| {
        log::warn!("LUT provider: its answer is not JSON: {e}");
        Failure::Crash
    })?;
    if !answer.ok {
        log::info!(
            "LUT provider: {} {}: {:?} {:?}",
            key.camera,
            key.look,
            answer.error,
            answer.message
        );
        return Err(Failure::Refused(match answer.error.as_deref() {
            Some("unsupported_camera") => Refusal::UnsupportedCamera,
            Some("unknown_look") => Refusal::UnknownLook,
            Some("bad_request") => Refusal::BadRequest,
            Some("data_missing") => Refusal::DataMissing,
            _ => Refusal::Internal,
        }));
    }
    let bytes = std::fs::read(scratch).map_err(|_| Failure::BadFile)?;
    let lut = checked_file(&bytes)?;
    Ok((bytes, lut))
}

/// [`ask_for_lut`], whose scratch file never outlives the run.
fn make_lut(
    exe: &Path,
    limits: &Limits,
    subject: &Subject,
    key: &Key,
    options: &LookOptions,
    scratch: &Path,
    cancel: &AtomicBool,
) -> Result<(Vec<u8>, Lut), Failure> {
    let made = ask_for_lut(exe, limits, subject, key, options, scratch, cancel);
    let _ = std::fs::remove_file(scratch);
    made
}

// ------------------------------------------------------------------ the queue

/// One photo's result, as the interface hears it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LookProgress {
    pub job_id: u64,
    pub path: String,
    pub camera: String,
    pub look: String,
    pub options: LookOptions,
    pub own_wb: Option<OwnWb>,
    pub outcome: LookOutcome,
    /// For Preview all: nothing goes on a photo.
    pub preview: bool,
}

/// Where one request's result goes.
pub type Report = Arc<dyn Fn(LookProgress) + Send + Sync>;

/// One photo asking for a camera's look.
pub struct Request {
    subject: Subject,
    camera: String,
    look: String,
    /// Its options away from their default, as the edit holds them.
    options: LookOptions,
    own_wb: Option<OwnWb>,
    /// Overwrite a file already there.
    remake: bool,
    /// The open photo: ahead of what is waiting.
    front: bool,
    /// For Preview all: the file goes to the temporary folder, not beside the photo.
    preview: bool,
    job: u64,
    report: Report,
}

/// What a LUT depends on: requests with the same key share a run.
#[derive(Clone, PartialEq, Eq)]
struct Key {
    camera: String,
    look: String,
    options: LookOptions,
    own_wb: Option<OwnWb>,
    make: String,
    model: String,
    wb: [u32; 4],
    offset: u32,
}

impl Key {
    fn of(
        subject: &Subject,
        camera: &str,
        look: &str,
        options: &LookOptions,
        own_wb: Option<&OwnWb>,
    ) -> Self {
        Self {
            camera: camera.to_string(),
            look: look.to_string(),
            options: options.clone(),
            own_wb: own_wb.cloned(),
            make: subject.make.clone(),
            model: subject.model.clone(),
            wb: subject.wb.map(f32::to_bits),
            offset: subject.offset_ev.to_bits(),
        }
    }
}

/// A photo waiting for an item.
struct Waiter {
    photo: String,
    job: u64,
    /// Where its file goes.
    file: PathBuf,
    preview: bool,
    report: Report,
}

impl Waiter {
    fn tell(&self, key: &Key, outcome: LookOutcome) {
        (self.report)(LookProgress {
            job_id: self.job,
            path: self.photo.clone(),
            camera: key.camera.clone(),
            look: key.look.clone(),
            options: key.options.clone(),
            own_wb: key.own_wb.clone(),
            outcome,
            preview: self.preview,
        });
    }
}

/// One run to make, and who is waiting for it.
struct Item {
    key: Key,
    subject: Subject,
    waiters: Vec<Waiter>,
    /// Set once nobody waits: the run is killed.
    cancel: Arc<AtomicBool>,
}

impl Item {
    /// Whether only Preview all wants it.
    fn is_preview(&self) -> bool {
        self.waiters.iter().all(|waiter| waiter.preview)
    }
}

/// The camera, look, options and white balance of a refused request.
type RefusalOf = (String, String, LookOptions, Option<OwnWb>);

#[derive(Default)]
struct State {
    waiting: VecDeque<Item>,
    running: Vec<Item>,
    crashes: u32,
    off: bool,
    /// What the program said no to, not asked again.
    refused: HashMap<RefusalOf, LookOutcome>,
    scratch_files: u64,
}

impl State {
    fn items(&mut self) -> impl Iterator<Item = &mut Item> {
        self.waiting.iter_mut().chain(self.running.iter_mut())
    }

    /// Takes the waiters `leaves` picks, and kills or forgets the items that
    /// leaves empty.
    fn take_waiters(&mut self, leaves: impl Fn(&Item, &Waiter) -> bool) -> Vec<(Key, Waiter)> {
        let mut gone = Vec::new();
        for item in self.items() {
            let mut kept = Vec::new();
            for waiter in std::mem::take(&mut item.waiters) {
                if leaves(item, &waiter) {
                    gone.push((item.key.clone(), waiter));
                } else {
                    kept.push(waiter);
                }
            }
            item.waiters = kept;
            if item.waiters.is_empty() {
                item.cancel.store(true, Ordering::SeqCst);
            }
        }
        self.waiting.retain(|item| !item.waiters.is_empty());
        gone
    }
}

/// Told when a photo's look file is written: its LUT replaces any in memory.
type Placed = Box<dyn Fn(&str, &Path, Arc<Lut>) + Send + Sync>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// The work for one program.
pub struct Queue {
    exe: PathBuf,
    scratch: PathBuf,
    limits: Limits,
    /// Every option the program offers, at its default.
    option_defaults: LookOptions,
    placed: Placed,
    state: Mutex<State>,
}

impl Queue {
    /// A queue with an empty scratch folder: what a run left behind goes.
    fn new(
        exe: PathBuf,
        scratch: PathBuf,
        limits: Limits,
        option_defaults: LookOptions,
        placed: Placed,
    ) -> Arc<Self> {
        let _ = std::fs::remove_dir_all(&scratch);
        let _ = std::fs::create_dir_all(&scratch);
        Arc::new(Self {
            exe,
            scratch,
            limits,
            option_defaults,
            placed,
            state: Mutex::new(State::default()),
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// Whether crashes turned the provider off until the app closes.
    fn is_off(&self) -> bool {
        self.state().off
    }

    /// Whether anything is queued or running.
    #[cfg(test)]
    fn is_busy(&self) -> bool {
        let state = self.state();
        !state.waiting.is_empty() || !state.running.is_empty()
    }

    /// Asks for a photo's look file. The answer comes through `report`, at once
    /// if nothing needs a run. A photo waits for one thing: the latest.
    pub fn request(self: &Arc<Self>, request: Request) {
        let Request {
            subject,
            camera,
            look,
            options,
            own_wb,
            remake,
            front,
            preview,
            job,
            report,
        } = request;
        let key = Key::of(&subject, &camera, &look, &options, own_wb.as_ref());
        let source = parse_virtual_path(&subject.photo).0;
        let (Some(beside), Some(previewed)) = (
            file_ops::look_file_for(&source, &camera, &look, &options, own_wb.as_ref()),
            self.preview_file(&source, &camera, &look, &options, own_wb.as_ref()),
        ) else {
            return;
        };
        let waiter = Waiter {
            photo: subject.photo.clone(),
            job,
            file: if preview { &previewed } else { &beside }.clone(),
            preview,
            report,
        };

        // Preview all goes on beside whatever is asked for by hand.
        if !preview {
            let replaced = {
                let photo = subject.photo.clone();
                let same = key.clone();
                self.state()
                    .take_waiters(|item, w| !w.preview && w.photo == photo && item.key != same)
            };
            for (key, old) in replaced {
                old.tell(&key, LookOutcome::Cancelled);
            }
        }

        if !remake && good_file(&beside).is_some() {
            waiter.tell(&key, LookOutcome::Done);
            return;
        }
        if !folder_takes_files(&waiter.file) {
            waiter.tell(&key, LookOutcome::Folder);
            return;
        }
        if !remake && let Some((bytes, lut)) = good_file(&previewed) {
            let outcome = if preview {
                LookOutcome::Done
            } else {
                self.place(&waiter, &bytes, &Arc::new(lut))
            };
            waiter.tell(&key, outcome);
            return;
        }

        let mut state = self.state();
        let refused = state.refused.get(&(camera, look, options, own_wb)).copied();
        match (state.off, refused) {
            (true, _) => {
                drop(state);
                waiter.tell(&key, LookOutcome::Failed);
                return;
            }
            (false, Some(outcome)) if !remake => {
                drop(state);
                waiter.tell(&key, outcome);
                return;
            }
            _ => {}
        }

        let joined = state
            .items()
            .find(|item| item.key == key && !item.cancel.load(Ordering::SeqCst));
        if let Some(item) = joined {
            item.waiters.push(waiter);
        } else {
            let item = Item {
                key: key.clone(),
                subject,
                waiters: vec![waiter],
                cancel: Arc::new(AtomicBool::new(false)),
            };
            state.waiting.push_back(item);
        }
        if front
            && let Some(at) = state.waiting.iter().position(|item| item.key == key)
            && let Some(item) = state.waiting.remove(at)
        {
            state.waiting.push_front(item);
        }
        self.start_more(&mut state);
    }

    /// Where Preview all keeps a look of `photo`.
    fn preview_file(
        &self,
        photo: &Path,
        camera: &str,
        look: &str,
        options: &LookOptions,
        own_wb: Option<&OwnWb>,
    ) -> Option<PathBuf> {
        let folder = self.scratch.join(PREVIEW_FOLDER);
        preview_file_for(&folder, photo, camera, look, options, own_wb)
    }

    /// Asks for every look on the list as a temporary file: what the button
    /// does, and nothing else does. Each look answers once, whatever the photo
    /// is, so the interface can count them.
    fn preview_all(
        self: &Arc<Self>,
        catalog: &Catalog,
        subject: Option<Subject>,
        args: &ProviderLookPreviewArgs,
        job: u64,
        report: Report,
    ) {
        let folder = self.scratch.join(PREVIEW_FOLDER);
        let _ = std::fs::create_dir_all(&folder);
        trim_previews(&folder, MAX_PREVIEW_BYTES);
        let chosen = Some(args.camera.as_str());
        let camera = subject
            .as_ref()
            .and_then(|s| camera_for(catalog, chosen, &s.make, &s.model));
        let own_wb = subject
            .as_ref()
            .and_then(|s| catalog.own_wb(s, &args.options, args.changes.as_ref()));
        for look in &catalog.looks {
            match (&subject, camera) {
                (Some(subject), Some(camera)) if catalog.lists(&look.id, &args.options) => {
                    self.request(Request {
                        subject: subject.clone(),
                        camera: camera.to_string(),
                        look: look.id.clone(),
                        options: args.options.clone(),
                        own_wb: own_wb.clone(),
                        remake: false,
                        front: false,
                        preview: true,
                        job,
                        report: Arc::clone(&report),
                    });
                }
                _ => report(LookProgress {
                    job_id: job,
                    path: args.path.clone(),
                    camera: args.camera.clone(),
                    look: look.id.clone(),
                    options: args.options.clone(),
                    own_wb: None,
                    outcome: LookOutcome::Unsupported,
                    preview: true,
                }),
            }
        }
    }

    /// How many of `photo`'s look files and previews Delete all previews looks at.
    fn clearable(&self, photo: &Path) -> usize {
        let folder = self.scratch.join(PREVIEW_FOLDER);
        file_ops::look_files_of(photo).len() + previews_of(&folder, photo).len()
    }

    /// Clears what can go of one photo: the look files no edit uses, its
    /// previews, and what is on its way to them, first, so nothing lands after.
    /// A look an edit names is left to finish: an export may be waiting for it.
    /// `remove` takes one file, so one that will not go leaves the rest to go.
    fn delete_looks(&self, args: &ProviderLookDeleteArgs, remove: impl Fn(&Path)) -> LooksDeleted {
        let source = parse_virtual_path(&args.path).0;
        let mut used = provider_look::files_in_use(&source);
        used.extend(args.on_photo.as_ref().and_then(|on| on.file(&source)));
        self.cancel_where(|waiter| {
            parse_virtual_path(&waiter.photo).0 == source && !used.contains(&waiter.file)
        });

        let mut result = LooksDeleted::default();
        // One that will not go now goes when the app closes.
        for preview in previews_of(&self.scratch.join(PREVIEW_FOLDER), &source) {
            if std::fs::remove_file(preview).is_ok() {
                result.deleted += 1;
            }
        }
        for file in file_ops::look_files_of(&source) {
            if used.contains(&file) {
                result.kept += 1;
                continue;
            }
            remove(&file);
            if file.exists() {
                result.failed += 1;
            } else {
                result.deleted += 1;
            }
        }
        result
    }

    /// Asks for a photo's look file and waits for the answer, or for `cancel`.
    /// For an export, which has its own thread to wait on.
    fn wait_for(
        self: &Arc<Self>,
        subject: Subject,
        look: provider_look::ProviderLook,
        cancel: &AtomicBool,
    ) -> LookOutcome {
        let job = NEXT_JOB.fetch_add(1, Ordering::SeqCst);
        let (tell, told) = mpsc::channel();
        let tell = Mutex::new(tell);
        self.request(Request {
            subject,
            camera: look.camera,
            look: look.look,
            options: look.options,
            own_wb: look.own_wb,
            remake: false,
            front: false,
            preview: false,
            job,
            report: Arc::new(move |progress| {
                let _ = lock(&tell).send(progress.outcome);
            }),
        });
        loop {
            match told.recv_timeout(EXPORT_POLL) {
                Ok(outcome) => return outcome,
                Err(mpsc::RecvTimeoutError::Disconnected) => return LookOutcome::Failed,
                Err(mpsc::RecvTimeoutError::Timeout) if cancel.load(Ordering::SeqCst) => {
                    self.cancel(job);
                    return LookOutcome::Cancelled;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }

    /// Takes every photo of `job` out of the queue.
    pub fn cancel(&self, job: u64) {
        self.cancel_where(|waiter| waiter.job == job);
    }

    fn cancel_where(&self, leaves: impl Fn(&Waiter) -> bool) {
        let gone = self.state().take_waiters(|_, waiter| leaves(waiter));
        for (key, waiter) in gone {
            waiter.tell(&key, LookOutcome::Cancelled);
        }
    }

    /// The app is closing: nothing stays behind it, so this returns once the
    /// runs are gone, or after a moment if one will not go, and its temporary
    /// files are gone with them.
    fn stop_all(&self) {
        self.cancel_where(|_| true);
        let deadline = Instant::now() + STOP_WAIT;
        while !self.state().running.is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let _ = std::fs::remove_dir_all(&self.scratch);
    }

    /// Runs what fits, first come first: previews up to the limit for all, and
    /// what is asked for by hand up to its own, so with no previews about, that
    /// is just what it would get alone.
    fn start_more(self: &Arc<Self>, state: &mut State) {
        while state.running.len() < RUNS_WITH_PREVIEWS {
            let by_hand = state.running.iter().filter(|i| !i.is_preview()).count();
            let next = state
                .waiting
                .iter()
                .position(|item| item.is_preview() || by_hand < RUNS_AT_ONCE);
            let Some(item) = next.and_then(|at| state.waiting.remove(at)) else {
                return;
            };
            let (subject, key, cancel) = (
                item.subject.clone(),
                item.key.clone(),
                Arc::clone(&item.cancel),
            );
            state.running.push(item);
            state.scratch_files += 1;
            let scratch = self.scratch.join(format!("{}.png", state.scratch_files));
            let queue = Arc::clone(self);
            std::thread::spawn(move || queue.work(&subject, &key, &scratch, &cancel));
        }
    }

    /// Every option the program offers, at the value `options` gives it.
    fn options_to_send(&self, options: &LookOptions) -> LookOptions {
        let mut sent = self.option_defaults.clone();
        for (id, value) in &mut sent {
            *value = options.get(id).copied().unwrap_or(*value);
        }
        sent
    }

    /// Writes a checked LUT where `waiter` wants it. Beside a photo it also
    /// replaces any in memory; a preview is read when it is looked at.
    fn place(&self, waiter: &Waiter, bytes: &[u8], lut: &Arc<Lut>) -> LookOutcome {
        match write_atomic(&waiter.file, bytes) {
            Ok(()) => {
                if !waiter.preview {
                    (self.placed)(&waiter.photo, &waiter.file, Arc::clone(lut));
                }
                LookOutcome::Done
            }
            Err(e) => {
                log::warn!("LUT provider: {}: {e}", waiter.file.display());
                LookOutcome::Folder
            }
        }
    }

    /// One run, then its results.
    fn work(
        self: Arc<Self>,
        subject: &Subject,
        key: &Key,
        scratch: &Path,
        cancel: &Arc<AtomicBool>,
    ) {
        let made = make_lut(
            &self.exe,
            &self.limits,
            subject,
            key,
            &self.options_to_send(&key.options),
            scratch,
            cancel,
        );

        let mut state = self.state();
        let at = state
            .running
            .iter()
            .position(|item| Arc::ptr_eq(&item.cancel, cancel));
        let waiters = at.map_or_else(Vec::new, |at| state.running.remove(at).waiters);
        let mut lost = Vec::new();
        match &made {
            Ok(_) => state.crashes = 0,
            Err(Failure::Crash) => {
                state.crashes += 1;
                if state.crashes >= CRASHES_TO_OFF {
                    log::warn!("LUT provider: off until the app closes, it keeps crashing");
                    state.off = true;
                    lost.extend(state.waiting.drain(..));
                }
            }
            Err(failure) if failure.is_final() => {
                let id = (
                    key.camera.clone(),
                    key.look.clone(),
                    key.options.clone(),
                    key.own_wb.clone(),
                );
                state.refused.insert(id, failure.outcome());
            }
            Err(_) => {}
        }
        self.start_more(&mut state);
        drop(state);

        for item in lost {
            for waiter in &item.waiters {
                waiter.tell(&item.key, LookOutcome::Failed);
            }
        }
        match made {
            Ok((bytes, lut)) => {
                let lut = Arc::new(lut);
                for waiter in waiters {
                    waiter.tell(key, self.place(&waiter, &bytes, &lut));
                }
            }
            Err(failure) => {
                for waiter in waiters {
                    waiter.tell(key, failure.outcome());
                }
            }
        }
    }
}

/// The file's bytes and LUT, if it is a good look file.
fn good_file(file: &Path) -> Option<(Vec<u8>, Lut)> {
    let bytes = std::fs::read(file).ok()?;
    let lut = checked_file(&bytes).ok()?;
    Some((bytes, lut))
}

/// A preview's file in `folder`: [`file_ops::look_file_for`] on a stem made of a
/// dot and a hash of the photo's whole path. So the camera, the look, the options
/// and the white balance are in the name as for any look, and photos with one
/// name in two folders never meet. A name with a dot first is never listed as a
/// photo (`is_supported_image_file`), and that stem names no photo, so it is no
/// photo's look file either: nothing travels, is deleted or is read with it.
fn preview_file_for(
    folder: &Path,
    photo: &Path,
    camera: &str,
    look: &str,
    options: &LookOptions,
    own_wb: Option<&OwnWb>,
) -> Option<PathBuf> {
    let stem = folder.join(preview_stem(photo));
    file_ops::look_file_for(&stem, camera, look, options, own_wb)
}

/// What the name of every preview of `photo` starts with.
fn preview_stem(photo: &Path) -> String {
    let hash = blake3::hash(photo.as_os_str().as_encoded_bytes()).to_hex();
    format!(".{}", &hash[..PREVIEW_HASH_CHARS])
}

/// Every preview of `photo` in `folder`, whichever camera, look and options
/// made it.
fn previews_of(folder: &Path, photo: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let start = format!("{}.", preview_stem(photo));
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&start))
        .map(|entry| entry.path())
        .collect()
}

/// Drops the oldest previews in `folder` until it is within `budget_bytes`.
fn trim_previews(folder: &Path, budget_bytes: u64) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let mut files: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            Some((meta.modified().ok()?, meta.len(), entry.path()))
        })
        .collect();
    files.sort();
    let mut total: u64 = files.iter().map(|(_, bytes, _)| bytes).sum();
    for (_, bytes, file) in files {
        if total <= budget_bytes {
            return;
        }
        if std::fs::remove_file(file).is_ok() {
            total -= bytes;
        }
    }
}

/// What a crash left in `cache`, the app's cache folder: only this module's own
/// folder, gone at start-up whether or not there is a provider now.
pub fn clear_leftovers(cache: &Path) {
    let _ = std::fs::remove_dir_all(cache.join(SCRATCH_FOLDER));
}

/// Whether a file can be created beside `file`: asked before any run.
fn folder_takes_files(file: &Path) -> bool {
    file.parent()
        .is_some_and(|folder| tempfile::NamedTempFile::new_in(folder).is_ok())
}

/// The camera a photo gets: `chosen` while it is on the list, otherwise its own.
/// A photo from no camera on the list gets none, chosen or not.
fn camera_for<'a>(
    catalog: &'a Catalog,
    chosen: Option<&str>,
    make: &str,
    model: &str,
) -> Option<&'a str> {
    let own = catalog
        .cameras
        .iter()
        .find(|c| same_model(&c.make, make) && same_model(&c.model, model))?;
    let chosen = chosen.and_then(|id| catalog.cameras.iter().find(|c| c.id == id));
    Some(&chosen.unwrap_or(own).id)
}

// ------------------------------------------------------------------ the app's

struct Provider {
    catalog: Catalog,
    queue: Arc<Queue>,
}

/// A good provider, kept until the app closes.
static READY: OnceLock<Provider> = OnceLock::new();
/// One validation at a time.
static CHECKING: Mutex<()> = Mutex::new(());
static NEXT_JOB: AtomicU64 = AtomicU64::new(1);

/// The provider, validated the first time this is asked, never at start-up. A
/// bad one is tried again the next time. None for everyone without the field,
/// with no process run.
fn provider(app: &AppHandle) -> Option<&'static Provider> {
    let ready = |provider: &'static Provider| (!provider.queue.is_off()).then_some(provider);
    if let Some(provider) = READY.get() {
        return ready(provider);
    }
    let _one = lock(&CHECKING);
    if let Some(provider) = READY.get() {
        return ready(provider);
    }
    let (program, catalog) = offered(profiles::library()?, &LIMITS)?;
    let scratch = app.path().app_cache_dir().ok()?.join(SCRATCH_FOLDER);
    let handle = app.clone();
    let placed = Box::new(move |photo: &str, file: &Path, lut: Arc<Lut>| {
        let state = handle.state::<AppState>();
        lock(&state.lut_cache).insert(file.to_string_lossy().into_owned(), lut);
        refresh_thumbnail(&handle, photo, file);
    });
    Some(READY.get_or_init(|| Provider {
        queue: Queue::new(
            PathBuf::from(&program),
            scratch,
            LIMITS,
            catalog.option_defaults(),
            placed,
        ),
        catalog,
    }))
}

/// A photo whose edit names this file has a thumbnail made without it, and
/// its edit text has not changed, so the usual refresh would keep that one.
fn refresh_thumbnail(app: &AppHandle, photo: &str, file: &Path) {
    let sidecar = parse_virtual_path(photo).1;
    let edit = crate::exif_processing::load_sidecar(&sidecar).adjustments;
    if provider_look::look_file(&edit, photo).as_deref() != Some(file) {
        return;
    }
    let state = app.state::<AppState>();
    crate::file_management::add_to_thumbnail_queue(&state, 1, app);
    let settings = crate::app_settings::load_settings(app.clone()).unwrap_or_default();
    let photos = std::iter::once(photo.to_string());
    crate::file_management::regenerate_thumbnails(photos, app, &settings, true);
}

/// The white balance as the program is told of it, from the decoder's R, G, B
/// and fourth colour; None for a photo with none.
fn wb_for_program(c: [f32; 4]) -> Option<[f32; 4]> {
    let wb = if c[3].is_nan() {
        [c[0], c[1], c[1], c[2]]
    } else {
        c
    };
    if wb.iter().any(|c| c.is_nan()) {
        return None;
    }
    Some(wb)
}

/// The saved edit of `photo`, with `changes` it may not hold yet over it.
fn edit_of(photo: &str, changes: Option<&Value>) -> Value {
    let saved = crate::exif_processing::load_sidecar(&parse_virtual_path(photo).1).adjustments;
    let mut edit = saved.as_object().cloned().unwrap_or_default();
    if let Some(Value::Object(changes)) = changes {
        edit.extend(changes.clone());
    }
    Value::Object(edit)
}

/// One photo, read for a request; None for anything but a RAW with a white
/// balance.
fn subject_of(photo: &str) -> Option<Subject> {
    let source = parse_virtual_path(photo).0;
    if !crate::formats::is_raw_file(&source) {
        return None;
    }
    let bytes = read_file_mapped(&source).ok()?;
    let facts = dng_standard::facts(&bytes).ok()?;
    let as_shot = crate::as_shot::as_shot(photo);
    let wb = wb_for_program(as_shot.wb_coeffs)?;
    Some(Subject {
        photo: photo.to_string(),
        make: facts.make,
        model: facts.model,
        iso: facts.iso,
        wb,
        white: as_shot.white_balance,
        xyz_to_camera: as_shot.xyz_to_camera,
        offset_ev: dng_standard::offset_for_photo(Some(photo)),
    })
}

/// A look's file that is already there.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LookFile {
    pub camera: String,
    pub look: String,
    pub path: String,
}

/// What the open photo can have.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PhotoLooks {
    /// Its own camera on the list.
    pub camera: String,
    pub files: Vec<LookFile>,
    /// The temporary files of Preview all: to look at, never a look on a photo.
    pub previews: Vec<LookFile>,
    /// Its look files and previews, for any camera, look and options, listed
    /// or not: what its Delete all previews looks at.
    pub clearable: usize,
    /// How the look files reported follow its white balance.
    pub own_wb: Option<OwnWb>,
}

/// What the provider offers.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProviderLooks {
    pub cameras: Vec<OfferedCamera>,
    pub looks: Vec<OfferedLook>,
    pub options: Vec<OfferedOption>,
    /// The switch.
    pub on: bool,
    /// None for no photo, a photo that is not a RAW, or one from no camera on
    /// the list: no provider looks show for it.
    pub photo: Option<PhotoLooks>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProviderLooksArgs {
    pub path: Option<String>,
    /// The options whose look files to report.
    pub options: LookOptions,
    /// What its saved edit does not hold yet.
    #[cfg_attr(test, ts(type = "unknown"))]
    pub changes: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProviderLookMakeArgs {
    pub paths: Vec<String>,
    /// None for each photo's own camera.
    pub camera: Option<String>,
    pub look: String,
    pub options: LookOptions,
    /// What each photo's saved edit does not hold yet.
    #[cfg_attr(test, ts(type = "unknown"))]
    pub changes: Option<Value>,
    pub remake: bool,
    pub front: bool,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProviderLookPreviewArgs {
    pub path: String,
    pub camera: String,
    pub options: LookOptions,
    /// What its saved edit does not hold yet.
    #[cfg_attr(test, ts(type = "unknown"))]
    pub changes: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ProviderLookDeleteArgs {
    pub path: String,
    /// The look on the open edit: its save may still be on its way.
    pub on_photo: Option<provider_look::ProviderLook>,
}

/// What became of a photo's look files and previews.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LooksDeleted {
    /// Look files and previews, so right after Preview all it is the previews.
    pub deleted: usize,
    /// An edit uses these.
    pub kept: usize,
    /// These would not go.
    pub failed: usize,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ProviderLookFileArgs {
    pub path: String,
    pub camera: String,
    pub look: String,
    pub options: LookOptions,
    pub own_wb: Option<OwnWb>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ProviderLookCancelArgs {
    pub job_id: u64,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProviderLooksSwitchArgs {
    pub on: bool,
}

/// The files that are there of those `name_of` gives, for each camera and look.
fn files_there(
    catalog: &Catalog,
    name_of: impl Fn(&str, &str) -> Option<PathBuf>,
) -> Vec<LookFile> {
    catalog
        .cameras
        .iter()
        .flat_map(|c| catalog.looks.iter().map(move |l| (c, l)))
        .filter_map(|(c, l)| {
            let file = name_of(&c.id, &l.id)?;
            file.is_file().then(|| LookFile {
                camera: c.id.clone(),
                look: l.id.clone(),
                path: file.to_string_lossy().into_owned(),
            })
        })
        .collect()
}

/// What the provider offers `path`; None when there is no provider.
pub fn looks(app: &AppHandle, args: &ProviderLooksArgs) -> Option<ProviderLooks> {
    let provider = provider(app)?;
    let catalog = &provider.catalog;
    let options = &args.options;
    let photo = args
        .path
        .as_deref()
        .and_then(subject_of)
        .and_then(|subject| {
            let camera = camera_for(catalog, None, &subject.make, &subject.model)?.to_string();
            let source = parse_virtual_path(&subject.photo).0;
            let own_wb = catalog.own_wb(&subject, options, args.changes.as_ref());
            let own = own_wb.as_ref();
            let files = files_there(catalog, |camera, look| {
                file_ops::look_file_for(&source, camera, look, options, own)
            });
            let previews = files_there(catalog, |camera, look| {
                let queue = &provider.queue;
                queue.preview_file(&source, camera, look, options, own)
            });
            Some(PhotoLooks {
                camera,
                files,
                previews,
                clearable: provider.queue.clearable(&source),
                own_wb,
            })
        });
    Some(ProviderLooks {
        cameras: catalog.cameras.clone(),
        looks: catalog.looks.clone(),
        options: catalog.options.clone(),
        on: is_on(profiles::library()?),
        photo,
    })
}

/// Queues `args`, reading each photo off the caller's thread. The job's id;
/// each photo's result comes as an [`EVENT`].
pub fn make(app: &AppHandle, args: ProviderLookMakeArgs) -> Result<u64, String> {
    let provider = provider(app).ok_or("no provider")?;
    let job = NEXT_JOB.fetch_add(1, Ordering::SeqCst);
    let app = app.clone();
    std::thread::spawn(move || {
        let report: Report = Arc::new(move |progress| {
            let _ = app.emit(EVENT, progress);
        });
        for path in args.paths {
            let request = subject_of(&path).and_then(|subject| {
                let chosen = args.camera.as_deref();
                let camera = camera_for(&provider.catalog, chosen, &subject.make, &subject.model)?;
                let listed = provider.catalog.lists(&args.look, &args.options);
                let changes = args.changes.as_ref();
                listed.then(|| Request {
                    camera: camera.to_string(),
                    look: args.look.clone(),
                    options: args.options.clone(),
                    own_wb: provider.catalog.own_wb(&subject, &args.options, changes),
                    remake: args.remake,
                    front: args.front,
                    preview: false,
                    job,
                    report: Arc::clone(&report),
                    subject,
                })
            });
            match request {
                Some(request) => provider.queue.request(request),
                None => report(LookProgress {
                    job_id: job,
                    path,
                    camera: args.camera.clone().unwrap_or_default(),
                    look: args.look.clone(),
                    options: args.options.clone(),
                    own_wb: None,
                    outcome: LookOutcome::Unsupported,
                    preview: false,
                }),
            }
        }
    });
    Ok(job)
}

/// Queues every look of one photo as a temporary file. The job's id comes back
/// once all are queued, so cancelling it reaches every one of them; each look's
/// result comes as an [`EVENT`]. Reads the photo: call it off the UI thread.
pub fn preview_all(app: &AppHandle, args: ProviderLookPreviewArgs) -> Result<u64, String> {
    let provider = provider(app).ok_or("no provider")?;
    let job = NEXT_JOB.fetch_add(1, Ordering::SeqCst);
    let app = app.clone();
    let report: Report = Arc::new(move |progress| {
        let _ = app.emit(EVENT, progress);
    });
    let subject = subject_of(&args.path);
    provider
        .queue
        .preview_all(&provider.catalog, subject, &args, job, report);
    Ok(job)
}

/// Whether a photo has this look's file, with a provider or without one: the
/// file is all a look needs to show.
pub fn file_is_there(args: &ProviderLookFileArgs) -> bool {
    let source = parse_virtual_path(&args.path).0;
    let own_wb = args.own_wb.as_ref();
    file_ops::look_file_for(&source, &args.camera, &args.look, &args.options, own_wb)
        .is_some_and(|file| file.is_file())
}

/// For an export: whether the look the edit names is there to render, made
/// now if the provider can. False means it renders without its look, and
/// `cancel` stops the wait. An edit with no provider look needs nothing.
pub fn look_file_ready(app: &AppHandle, js: &Value, photo: &str, cancel: &AtomicBool) -> bool {
    if js["lutPath"].as_str() != Some(provider_look::MARKER) {
        return true;
    }
    let Some(file) = provider_look::look_file(js, photo) else {
        return false;
    };
    if file.is_file() {
        return true;
    }
    let Ok(look) = provider_look::ProviderLook::deserialize(&js[provider_look::KEY]) else {
        return false;
    };
    let Some(provider) = provider(app) else {
        return false;
    };
    let listed = provider.catalog.cameras.iter().any(|c| c.id == look.camera)
        && provider.catalog.lists(&look.look, &look.options);
    let Some(subject) = subject_of(photo).filter(|_| listed) else {
        return false;
    };
    provider.queue.wait_for(subject, look, cancel) == LookOutcome::Done
}

/// [`Queue::delete_looks`], for someone with a provider.
fn delete_looks_with(
    queue: Option<&Queue>,
    args: &ProviderLookDeleteArgs,
    remove: impl Fn(&Path),
) -> Result<LooksDeleted, String> {
    Ok(queue.ok_or("no provider")?.delete_looks(args, remove))
}

/// Delete all previews, on one photo: only its button asks. Reads its folder
/// and the Recycle Bin is slow: call it off the UI thread.
pub fn delete_looks(
    app: &AppHandle,
    args: &ProviderLookDeleteArgs,
) -> Result<LooksDeleted, String> {
    let queue = provider(app).map(|provider| provider.queue.as_ref());
    delete_looks_with(queue, args, |file| {
        file_ops::trash_or_delete(&[file.to_path_buf()]);
    })
}

/// Takes a job's photos out of the queue.
pub fn cancel(job: u64) {
    if let Some(provider) = READY.get() {
        provider.queue.cancel(job);
    }
}

/// Keeps the switch. Waiting work belongs to the interface, which cancels it.
pub fn switch(on: bool) -> Result<(), String> {
    set_on(profiles::library_ready()?, on)
}

/// The app is closing.
pub fn stop_all() {
    if let Some(provider) = READY.get() {
        provider.queue.stop_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{Receiver, channel};

    /// The fake provider: a real program, compiled once for this test run. What
    /// it does is its file name, so each behaviour is a copy under another
    /// name. It keeps a line per `lut` call in `calls.log` beside it.
    const FAKE_SOURCE: &str = r#"
use std::io::{Read, Write};
fn output_of(request: &str) -> String {
    let start = request.find("\"output\":\"").expect("output") + 10;
    let rest = &request[start..];
    let end = rest.find('"').expect("end");
    rest[..end].replace("\\\\", "\\")
}
fn main() {
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap().to_path_buf();
    let kind = exe.file_stem().unwrap().to_str().unwrap().to_string();
    let word = std::env::args().nth(1).unwrap_or_default();
    let mut request = String::new();
    std::io::stdin().read_to_string(&mut request).unwrap();
    let mut calls = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("calls.log")).unwrap();
    writeln!(calls, "{request}").unwrap();
    drop(calls);
    if word == "list" {
        if kind == "crash" { std::process::exit(3); }
        if kind == "slowlist" { std::thread::sleep(std::time::Duration::from_secs(30)); }
        if kind == "badlist" { print!("not json"); return; }
        if kind == "followswb" {
            print!("{{\"protocol\":1,\"cameras\":[{{\"id\":\"cam1\",\"name\":\"Camera One\",\"make\":\"Acme\",\"model\":\"One\"}}],\"looks\":[{{\"id\":\"look1\",\"name\":\"Invented Stock\"}}],\"options\":[{{\"id\":\"optionA\",\"label\":\"Invented choice\",\"default\":false}},{{\"id\":\"optionW\",\"label\":\"Invented follow\",\"default\":false,\"sendsCurrentWb\":true}}]}}");
            return;
        }
        if kind == "optioned" {
            print!("{{\"protocol\":1,\"cameras\":[{{\"id\":\"cam1\",\"name\":\"Camera One\",\"make\":\"Acme\",\"model\":\"One\"}}],\"looks\":[{{\"id\":\"look1\",\"name\":\"Invented Stock\"}}],\"options\":[{{\"id\":\"optionA\",\"label\":\"Invented choice\",\"default\":false}}]}}");
            return;
        }
        print!("{{\"protocol\":1,\"generator\":\"fake 1\",\"cameras\":[{{\"id\":\"cam1\",\"name\":\"Camera One\",\"make\":\"Acme\",\"model\":\"One\"}},{{\"id\":\"cam2\",\"name\":\"Camera Two\",\"make\":\"Acme\",\"model\":\"Two\"}}],\"looks\":[{{\"id\":\"look1\",\"name\":\"Invented Stock\",\"group\":\"Group A\"}},{{\"id\":\"look2\",\"name\":\"Other Stock\"}}]}}");
        return;
    }
    let output = output_of(&request);
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match kind.as_str() {
        "onebad" if request.contains("\"look\":\"look2\"") => std::process::exit(3),
        "good" | "optioned" | "followswb" | "onebad" => {
            std::fs::copy(dir.join("good.png"), &output).unwrap();
            write!(out, "{{\"ok\":true,\"seconds\":0.1}}").unwrap();
        }
        "paced" => {
            // A test that failed never says go, and this must not outlive it.
            let started = std::time::Instant::now();
            while !dir.join("go").exists() {
                if started.elapsed().as_secs() > 120 { std::process::exit(3); }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            std::fs::copy(dir.join("good.png"), &output).unwrap();
            write!(out, "{{\"ok\":true,\"seconds\":0.1}}").unwrap();
        }
        "crash" => { eprintln!("boom"); std::process::exit(3); }
        "ticking" => loop {
            let mut ticks = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("ticks.log")).unwrap();
            write!(ticks, ".").unwrap();
            drop(ticks);
            std::thread::sleep(std::time::Duration::from_millis(10));
        },
        "slow" => { std::fs::write(&output, "half").unwrap(); std::thread::sleep(std::time::Duration::from_secs(30)); }
        "badjson" => { std::fs::copy(dir.join("good.png"), &output).unwrap(); write!(out, "{{not json").unwrap(); }
        "noisy" => { std::fs::copy(dir.join("good.png"), &output).unwrap(); write!(out, "{}", "x".repeat(100_000)).unwrap(); }
        "badpng" => { std::fs::write(&output, "not a picture").unwrap(); write!(out, "{{\"ok\":true}}").unwrap(); }
        "smallpng" => { std::fs::copy(dir.join("small.png"), &output).unwrap(); write!(out, "{{\"ok\":true}}").unwrap(); }
        "hugepng" => { std::fs::write(&output, vec![0u8; 3_000_000]).unwrap(); write!(out, "{{\"ok\":true}}").unwrap(); }
        "nofile" => { write!(out, "{{\"ok\":true}}").unwrap(); }
        "unsupported" => write!(out, "{{\"ok\":false,\"error\":\"unsupported_camera\",\"message\":\"no\"}}").unwrap(),
        "unknown" => write!(out, "{{\"ok\":false,\"error\":\"unknown_look\"}}").unwrap(),
        "missing" => write!(out, "{{\"ok\":false,\"error\":\"data_missing\"}}").unwrap(),
        "refuseson" if request.contains("\"optionA\":true") => write!(out, "{{\"ok\":false,\"error\":\"data_missing\"}}").unwrap(),
        "refuseson" => {
            std::fs::copy(dir.join("good.png"), &output).unwrap();
            write!(out, "{{\"ok\":true}}").unwrap();
        }
        "strange" => write!(out, "{{\"ok\":false,\"error\":\"a_code_nobody_knows\"}}").unwrap(),
        other => panic!("{other}"),
    }
}
"#;

    /// Compiled once for this run and kept in its own temporary folder.
    fn fake_program() -> &'static Path {
        static BUILT: OnceLock<(tempfile::TempDir, PathBuf)> = OnceLock::new();
        let (_, exe) = BUILT.get_or_init(|| {
            let dir = tempfile::tempdir().expect("folder");
            let source = dir.path().join("fake.rs");
            std::fs::write(&source, FAKE_SOURCE).expect("source");
            let exe = dir
                .path()
                .join(format!("fake{}", std::env::consts::EXE_SUFFIX));
            let built = Command::new("rustc")
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .args(["--edition", "2024", "-C", "opt-level=0"])
                .args(["-C", "debuginfo=0"])
                .arg("-o")
                .arg(&exe)
                .arg(&source)
                .output()
                .expect("rustc");
            assert!(
                built.status.success(),
                "{}",
                String::from_utf8_lossy(&built.stderr)
            );
            (dir, exe)
        });
        exe
    }

    /// `kind` of the fake provider in a folder of its own, with a good PNG and
    /// a small one beside it. The folder goes with this.
    struct Fake {
        _dir: tempfile::TempDir,
        exe: PathBuf,
    }

    impl std::ops::Deref for Fake {
        type Target = Path;

        fn deref(&self) -> &Path {
            &self.exe
        }
    }

    fn fake(kind: &str) -> Fake {
        let dir = tempfile::tempdir().expect("folder");
        write_png(&dir.path().join("good.png"), 512);
        write_png(&dir.path().join("small.png"), 64);
        let exe = dir
            .path()
            .join(format!("{kind}{}", std::env::consts::EXE_SUFFIX));
        if std::fs::hard_link(fake_program(), &exe).is_err() {
            std::fs::copy(fake_program(), &exe).expect("copy");
        }
        Fake { _dir: dir, exe }
    }

    fn write_png(path: &Path, side: u32) {
        let mut png = image::RgbImage::new(side, side);
        for (i, pixel) in png.pixels_mut().enumerate() {
            *pixel = image::Rgb([(i % 251) as u8, (i % 241) as u8, (i % 239) as u8]);
        }
        png.save(path).expect("png");
    }

    /// For a test that means to run out of time.
    const QUICK: Limits = Limits {
        list: Duration::from_secs(1),
        lut: Duration::from_secs(1),
    };

    /// A new program can take seconds to start on a busy machine.
    const ROOMY: Limits = Limits {
        list: Duration::from_secs(60),
        lut: Duration::from_secs(60),
    };

    /// Under the 30 s the slow fakes sleep.
    const IN_TIME: Duration = Duration::from_secs(25);

    fn wait_for(what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !done() {
            assert!(Instant::now() < deadline, "{what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn calls(exe: &Path) -> usize {
        std::fs::read_to_string(exe.with_file_name("calls.log"))
            .map(|text| text.lines().count())
            .unwrap_or(0)
    }

    fn release(exe: &Path) {
        std::fs::write(exe.with_file_name("go"), "").expect("go");
    }

    // ------------------------------------------------------------ the setting

    fn library(text: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("folder");
        if let Some(text) = text {
            std::fs::write(dir.path().join("argentum-processing.json"), text).expect("file");
        }
        dir
    }

    /// No file, no key, an empty value: every public user. Off, with nothing
    /// to validate and so nothing to run.
    #[test]
    fn absent_or_empty_means_off() {
        for text in [
            None,
            Some(r#"{ "tiffBitDepth": 8 }"#),
            Some(r#"{ "lutProvider": "" }"#),
            Some(r#"{ "lutProvider": "   " }"#),
            Some(r#"{ "lutProvider": 7 }"#),
        ] {
            assert_eq!(configured(library(text).path()), None, "{text:?}");
        }
    }

    #[test]
    fn the_program_is_read_from_the_file() {
        let lib = library(Some(r#"{ "lutProvider": "C:\\tools\\made-up.exe" }"#));
        assert_eq!(
            configured(lib.path()),
            Some(r"C:\tools\made-up.exe".to_string())
        );
    }

    /// Saving any other preference neither adds the field nor drops it.
    #[test]
    fn saving_other_preferences_leaves_the_field_alone() {
        let other: Setting<u8> = Setting::new("tiffBitDepth");

        let absent = library(None);
        other.save(absent.path(), &8).expect("save");
        SWITCH.save(absent.path(), &false).expect("save");
        let file = absent.path().join("argentum-processing.json");
        let text = std::fs::read_to_string(file).expect("file");
        assert!(!text.contains("lutProvider"), "{text}");

        let by_hand = library(Some(r#"{ "lutProvider": "C:\\tools\\made-up.exe" }"#));
        other.save(by_hand.path(), &8).expect("save");
        SWITCH.save(by_hand.path(), &false).expect("save");
        assert_eq!(
            configured(by_hand.path()),
            Some(r"C:\tools\made-up.exe".to_string())
        );
    }

    /// The field is read through one constant and written by nothing.
    #[test]
    fn nothing_here_can_write_the_field() {
        let source = include_str!("lut_provider.rs");
        let code = source.split("#[cfg(test)]").next().expect("code");
        assert!(!code.contains("PROGRAM.save"));
    }

    #[test]
    fn the_switch_starts_on_and_remembers() {
        let lib = library(Some(r#"{ "lutProvider": "C:\\tools\\made-up.exe" }"#));
        assert!(is_on(lib.path()));
        set_on(lib.path(), false).expect("save");
        assert!(!is_on(lib.path()));
    }

    /// So `providerLooksOn` can't appear for anyone without the field.
    #[test]
    fn the_switch_is_not_written_without_a_provider() {
        let lib = library(None);
        assert!(set_on(lib.path(), false).is_err());
        assert!(!lib.path().join("argentum-processing.json").exists());
    }

    // ------------------------------------------------------------- validation

    #[test]
    fn only_a_local_program_with_a_full_path_passes() {
        let exe = fake("good");
        assert_eq!(check_path(&exe.to_string_lossy()), Ok(exe.to_path_buf()));

        let dir = exe.parent().expect("folder");
        let name = exe.file_name().expect("name").to_string_lossy().to_string();
        let up = format!(
            "{}/../{}/{name}",
            dir.display(),
            dir.file_name().unwrap().to_string_lossy()
        );
        for bad in [
            name.as_str(),
            "tools/made-up.exe",
            up.as_str(),
            r"\\server\share\made-up.exe",
            "//server/share/made-up.exe",
        ] {
            assert!(check_path(bad).is_err(), "{bad}");
        }
        assert!(check_path(&dir.join("gone.exe").to_string_lossy()).is_err());
        assert!(check_path(&dir.to_string_lossy()).is_err(), "a folder");
    }

    #[test]
    #[cfg(windows)]
    fn on_windows_it_must_be_an_exe() {
        let exe = fake("good");
        let cmd = exe.with_extension("cmd");
        std::fs::write(&cmd, "@echo off").expect("file");
        assert!(check_path(&cmd.to_string_lossy()).is_err());
    }

    const GOOD_LIST: &str = r#"{ "protocol": 1, "generator": "t",
        "cameras": [{ "id": "cam1", "name": "Camera One", "make": "Acme", "model": "One" }],
        "looks": [{ "id": "look1", "name": "Invented Stock", "group": "Group A" }] }"#;

    #[test]
    fn a_good_list_is_kept() {
        let catalog = read_list(GOOD_LIST.as_bytes()).expect("a list");
        assert_eq!(catalog.cameras[0].id, "cam1");
        assert_eq!(catalog.looks[0].group.as_deref(), Some("Group A"));
    }

    /// The groups of a list with one look per entry of `groups`, each a JSON value.
    fn groups_kept(groups: &[String]) -> Vec<Option<String>> {
        let looks: Vec<String> = (0..groups.len())
            .map(|n| {
                format!(
                    r#"{{ "id": "look{n}", "name": "L", "group": {} }}"#,
                    groups[n]
                )
            })
            .collect();
        let list = format!(
            r#"{{ "protocol": 1, "looks": [{}],
            "cameras": [{{ "id": "cam1", "name": "One", "make": "Acme", "model": "One" }}] }}"#,
            looks.join(",")
        );
        let looks = read_list(list.as_bytes()).expect("a list").looks;
        assert_eq!(looks.len(), groups.len(), "a look went with its group");
        looks.into_iter().map(|look| look.group).collect()
    }

    #[test]
    fn a_look_has_its_group_as_listed() {
        let groups = [r#""Group B""#, r#""Group A""#, r#""Group B""#].map(String::from);
        let kept = groups_kept(&groups);
        let names: Vec<_> = kept.iter().flatten().map(String::as_str).collect();
        assert_eq!(names, ["Group B", "Group A", "Group B"]);
    }

    #[test]
    fn a_look_with_no_group_has_none() {
        let list = GOOD_LIST.replace(r#", "group": "Group A""#, "");
        let catalog = read_list(list.as_bytes()).expect("a list");
        assert_eq!(catalog.looks[0].group, None);
    }

    /// The spaces around a name would make two headings of one.
    #[test]
    fn a_group_is_kept_without_the_spaces_around_it() {
        let kept = groups_kept(&[r#"" Group A ""#.to_string()]);
        assert_eq!(kept, [Some("Group A".to_string())]);
    }

    /// A group is a heading: the look stays without it, however bad it is.
    #[test]
    fn a_bad_group_costs_a_look_its_heading_only() {
        let long = format!(r#""{}""#, "x".repeat(MAX_NAME_CHARS + 1));
        for bad in [
            long.as_str(),
            r#""""#,
            r#""   ""#,
            r#""Group\nA""#,
            r#""Group\u0007A""#,
            "7",
            "true",
            "null",
            "[]",
            r#"{ "name": "Group A" }"#,
        ] {
            assert_eq!(groups_kept(&[bad.to_string()]), [None], "{bad}");
        }
    }

    #[test]
    fn a_group_of_the_longest_name_is_kept() {
        let longest = "x".repeat(MAX_NAME_CHARS);
        let kept = groups_kept(&[format!(r#""{longest}""#)]);
        assert_eq!(kept, [Some(longest)]);
    }

    /// Past the limit the looks stay, under no heading, and a group already
    /// kept takes more of them.
    #[test]
    fn a_list_of_too_many_groups_keeps_the_first() {
        let mut groups: Vec<String> = (0..MAX_GROUPS + 2)
            .map(|n| format!(r#""Group {n}""#))
            .collect();
        groups.push(r#""Group 0""#.to_string());
        let kept = groups_kept(&groups);
        let distinct: HashSet<_> = kept.iter().flatten().collect();
        assert_eq!(distinct.len(), MAX_GROUPS);
        assert_eq!(
            kept[MAX_GROUPS - 1],
            Some(format!("Group {}", MAX_GROUPS - 1))
        );
        assert_eq!(kept[MAX_GROUPS], None);
        assert_eq!(kept[MAX_GROUPS + 1], None);
        assert_eq!(kept[MAX_GROUPS + 2], Some("Group 0".to_string()));
    }

    /// A look that goes for its own faults holds no place among the groups.
    #[test]
    fn a_dropped_look_counts_for_no_group() {
        let mut looks = vec![r#"{ "id": "Bad", "name": "L", "group": "Dropped" }"#.to_string()];
        looks.extend(
            (0..MAX_GROUPS)
                .map(|n| format!(r#"{{ "id": "look{n}", "name": "L", "group": "Group {n}" }}"#)),
        );
        let list = format!(
            r#"{{ "protocol": 1, "looks": [{}],
            "cameras": [{{ "id": "cam1", "name": "One", "make": "Acme", "model": "One" }}] }}"#,
            looks.join(",")
        );
        let looks = read_list(list.as_bytes()).expect("a list").looks;
        assert!(looks.iter().all(|look| look.group.is_some()));
    }

    fn list_with_options(options: &str) -> String {
        GOOD_LIST.replacen(
            r#""protocol": 1,"#,
            &format!(r#""protocol": 1, "options": {options},"#),
            1,
        )
    }

    fn options_kept(options: &str) -> Vec<OfferedOption> {
        read_list(list_with_options(options).as_bytes())
            .expect("a list")
            .options
    }

    #[test]
    fn a_list_offers_its_options_in_order() {
        let kept = options_kept(
            r#"[{ "id": "optionB", "label": "Second", "default": true },
                { "id": "optionA", "label": "First", "default": false }]"#,
        );
        let ids: Vec<&str> = kept.iter().map(|option| option.id.as_str()).collect();
        assert_eq!(ids, ["optionB", "optionA"]);
        assert!(kept[0].default);
        assert_eq!(kept[1].label, "First");
    }

    #[test]
    fn a_list_with_no_options_is_as_it_was() {
        assert_eq!(read_list(GOOD_LIST.as_bytes()).expect("a list").options, []);
        assert_eq!(options_kept("[]"), []);
    }

    /// The whole list must not fall for one field of the wrong shape.
    #[test]
    fn options_of_the_wrong_shape_are_no_options() {
        for bad in [
            "7",
            "true",
            r#""optionA""#,
            "{}",
            "null",
            r#"{ "id": "a" }"#,
        ] {
            assert_eq!(options_kept(bad), [], "{bad}");
        }
    }

    #[test]
    fn a_bad_option_is_dropped_and_the_rest_stay() {
        let long = "x".repeat(81);
        let kept = options_kept(&format!(
            r#"[{{ "id": "optionA", "label": "Kept", "default": false }},
                {{ "id": "optionA", "label": "Same id", "default": true }},
                {{ "id": "../x", "label": "Bad id", "default": false }},
                {{ "id": "a b", "label": "Bad id", "default": false }},
                {{ "id": "", "label": "No id", "default": false }},
                {{ "id": "x234567890123456789012345678901234", "label": "Long id", "default": false }},
                {{ "id": "noLabel", "label": "", "default": false }},
                {{ "id": "blank", "label": "   ", "default": false }},
                {{ "id": "longLabel", "label": "{long}", "default": false }},
                {{ "id": "numberLabel", "label": 7, "default": false }},
                {{ "id": "noDefault", "label": "No default" }},
                {{ "id": "stringDefault", "label": "Text", "default": "false" }},
                {{ "label": "No id", "default": false }},
                7, "optionB", null, [],
                {{ "id": "optionC", "label": "Kept too", "default": true }}]"#
        ));
        let ids: Vec<&str> = kept.iter().map(|option| option.id.as_str()).collect();
        assert_eq!(ids, ["optionA", "optionC"]);
    }

    /// An id goes to the program and to an edit, never into a file name, so
    /// it need not fit one.
    #[test]
    fn an_options_id_may_be_in_mixed_case() {
        let kept = options_kept(r#"[{ "id": "someOption_2", "label": "L", "default": false }]"#);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn a_list_of_too_many_options_keeps_the_first() {
        let many: Vec<String> = (0..MAX_OPTIONS + 5)
            .map(|n| format!(r#"{{ "id": "option{n}", "label": "L", "default": false }}"#))
            .collect();
        let kept = options_kept(&format!("[{}]", many.join(",")));
        assert_eq!(kept.len(), MAX_OPTIONS);
        assert_eq!(kept[0].id, "option0");
    }

    #[test]
    fn a_working_program_lists_its_options() {
        let exe = fake("optioned");
        let catalog = validate(&exe.to_string_lossy(), &ROOMY).expect("a list");
        assert_eq!(catalog.options.len(), 1);
        assert_eq!(catalog.options[0].id, "optionA");
    }

    #[test]
    fn a_list_of_another_protocol_is_refused() {
        let other = GOOD_LIST.replace("\"protocol\": 1", "\"protocol\": 2");
        assert!(read_list(other.as_bytes()).is_err());
        assert!(read_list(b"not json").is_err());
    }

    #[test]
    fn a_bad_entry_is_dropped_and_the_rest_stay() {
        let long = "x".repeat(81);
        let list = format!(
            r#"{{ "protocol": 1,
            "cameras": [
              {{ "id": "cam1", "name": "One", "make": "Acme", "model": "One" }},
              {{ "id": "cam1", "name": "Again", "make": "Acme", "model": "Again" }},
              {{ "id": "../x", "name": "Bad id", "make": "Acme", "model": "X" }},
              {{ "id": "cam3", "name": "", "make": "Acme", "model": "X" }},
              {{ "id": "cam4", "name": "{long}", "make": "Acme", "model": "X" }},
              {{ "id": "cam5", "name": "No make", "make": "", "model": "X" }},
              {{ "id": "cam6", "name": "No model", "make": "Acme", "model": "" }},
              {{ "id": 7 }}
            ],
            "looks": [
              {{ "id": "look1", "name": "Invented Stock" }},
              {{ "id": "look1", "name": "Again" }},
              {{ "id": "Look2", "name": "Upper case" }},
              {{ "id": "look3", "name": "" }}
            ] }}"#
        );
        let catalog = read_list(list.as_bytes()).expect("a list");
        assert_eq!(catalog.cameras.len(), 1);
        assert_eq!(catalog.looks.len(), 1);
    }

    #[test]
    fn with_no_camera_or_no_look_left_the_provider_is_off() {
        let no_cameras =
            r#"{ "protocol": 1, "cameras": [], "looks": [{ "id": "a", "name": "A" }] }"#;
        let no_looks = r#"{ "protocol": 1, "looks": [],
            "cameras": [{ "id": "c", "name": "C", "make": "M", "model": "N" }] }"#;
        assert!(read_list(no_cameras.as_bytes()).is_err());
        assert!(read_list(no_looks.as_bytes()).is_err());
    }

    #[test]
    fn a_working_program_lists_its_cameras_and_looks() {
        let exe = fake("good");
        let catalog = validate(&exe.to_string_lossy(), &ROOMY).expect("a list");
        assert_eq!(catalog.cameras.len(), 2);
        assert_eq!(catalog.looks.len(), 2);
    }

    /// The fake counts its `list` calls, so this would see one that ran.
    #[test]
    fn with_the_field_absent_or_empty_no_process_runs() {
        let exe = fake("good");
        let named = format!(r#"{{ "lutProvider": {:?} }}"#, exe.to_string_lossy());
        assert!(offered(library(Some(&named)).path(), &ROOMY).is_some());
        assert_eq!(calls(&exe), 1, "the fake was not counted");

        for text in [None, Some(r#"{ "lutProvider": "" }"#)] {
            assert_eq!(offered(library(text).path(), &ROOMY), None);
            assert_eq!(calls(&exe), 1, "{text:?}");
        }
    }

    #[test]
    fn a_program_that_crashes_is_slow_or_lies_is_refused() {
        for kind in ["crash", "slowlist", "badlist"] {
            let exe = fake(kind);
            let started = Instant::now();
            assert!(validate(&exe.to_string_lossy(), &QUICK).is_err(), "{kind}");
            assert!(started.elapsed() < IN_TIME, "{kind}");
        }
    }

    // -------------------------------------------------------------- one LUT

    /// A camera's XYZ to camera matrix for daylight, as a decoder gives one.
    const CAMERA_D65: [f32; 9] = [
        0.4716, 0.0603, -0.0830, -0.7798, 1.5474, 0.2480, -0.1496, 0.1937, 0.6651,
    ];

    fn subject(photo: &str) -> Subject {
        Subject {
            photo: photo.to_string(),
            make: "Acme".to_string(),
            model: "One".to_string(),
            iso: Some(400.0),
            wb: [1.8, 1.0, 1.0, 1.6],
            white: WhiteBalance {
                temperature: 5200.0,
                tint: 4.0,
            },
            xyz_to_camera: Some(CAMERA_D65),
            offset_ev: 0.25,
        }
    }

    /// A three-colour photo's white balance arrives as R, G, B, NaN, and that
    /// NaN does not mean the photo has none.
    #[test]
    fn a_three_colour_white_balance_reaches_the_program_with_green_twice() {
        for [r, g, b] in [[1.8, 1.0, 1.6], [1.5, 1.0, 2.25]] {
            assert_eq!(wb_for_program([r, g, b, f32::NAN]), Some([r, g, g, b]));
        }
    }

    #[test]
    fn a_photo_with_no_white_balance_is_refused() {
        assert_eq!(wb_for_program([f32::NAN; 4]), None);
        assert_eq!(wb_for_program([1.8, f32::NAN, 1.6, 1.0]), None);
    }

    /// Four real numbers are not reshaped; the default for an unread file is
    /// four ones.
    #[test]
    fn four_real_numbers_pass_unchanged() {
        assert_eq!(wb_for_program([1.0; 4]), Some([1.0; 4]));
        let four = [1.8, 1.0, 1.0, 1.6];
        assert_eq!(wb_for_program(four), Some(four));
    }

    /// The look `look1` of `cam1`, as an edit holds it.
    fn look_with(options: LookOptions) -> provider_look::ProviderLook {
        provider_look::ProviderLook {
            camera: "cam1".to_string(),
            look: "look1".to_string(),
            options,
            own_wb: None,
            before: Default::default(),
        }
    }

    fn is_there(photo: &str, camera: &str, look: &str, options: &LookOptions) -> bool {
        file_is_there(&ProviderLookFileArgs {
            path: photo.to_string(),
            camera: camera.to_string(),
            look: look.to_string(),
            options: options.clone(),
            own_wb: None,
        })
    }

    fn look_is_there(photo: &str) -> bool {
        is_there(photo, "cam1", "look1", &LookOptions::new())
    }

    fn plain_key(subject: &Subject) -> Key {
        Key::of(subject, "cam1", "look1", &LookOptions::new(), None)
    }

    /// What a run of `kind` gives, and whether it left its scratch file.
    fn make_with(kind: &str) -> (bool, Result<(Vec<u8>, Lut), Failure>) {
        let limits = if kind == "slow" { QUICK } else { ROOMY };
        let exe = fake(kind);
        let scratch = exe.with_file_name("scratch.png");
        let photo = subject("/photos/IMG.CR3");
        let got = make_lut(
            &exe,
            &limits,
            &photo,
            &plain_key(&photo),
            &LookOptions::new(),
            &scratch,
            &AtomicBool::new(false),
        );
        (scratch.exists(), got)
    }

    #[test]
    fn a_good_run_gives_a_checked_lut_and_clears_the_scratch_file() {
        let (left, got) = make_with("good");
        let (bytes, lut) = got.expect("a LUT");
        assert_eq!(lut.size, 64);
        assert!(!bytes.is_empty());
        assert!(!left);
    }

    #[test]
    fn the_request_names_what_the_protocol_says() {
        let exe = fake("good");
        let scratch = exe.with_file_name("scratch.png");
        let photo = subject("/photos/IMG.CR3");
        make_lut(
            &exe,
            &ROOMY,
            &photo,
            &plain_key(&photo),
            &LookOptions::new(),
            &scratch,
            &AtomicBool::new(false),
        )
        .expect("a LUT");
        let sent = last_request(&exe);
        assert_eq!(sent["protocol"], 1);
        assert_eq!(sent["camera"], "cam1");
        assert_eq!(sent["look"], "look1");
        assert_eq!(sent["photo"]["path"], "/photos/IMG.CR3");
        assert_eq!(sent["photo"]["make"], "Acme");
        assert_eq!(sent["photo"]["model"], "One");
        assert_eq!(sent["photo"]["iso"], 400.0);
        assert_eq!(sent["asShotWb"].as_array().expect("WB").len(), 4);
        assert_eq!(sent["rendering"]["mode"], "linear");
        assert_eq!(sent["rendering"]["offsetEv"], 0.25);
        assert_eq!(sent["grid"], 64);
        assert_eq!(sent["output"], scratch.to_string_lossy().as_ref());
        assert!(sent.get("options").is_none(), "{sent}");
    }

    /// The last request the fake was sent.
    fn last_request(exe: &Path) -> Value {
        let calls = std::fs::read_to_string(exe.with_file_name("calls.log")).expect("calls");
        serde_json::from_str(calls.lines().last().expect("a call")).expect("JSON")
    }

    fn options(entries: &[(&str, bool)]) -> LookOptions {
        entries
            .iter()
            .map(|(id, value)| (id.to_string(), *value))
            .collect()
    }

    #[test]
    fn every_offered_option_goes_with_its_value() {
        let exe = fake("good");
        let heard = queue_offering(&exe, &[("optionA", false), ("optionB", true)]);
        heard.ask_with(&photo_beside(&exe, "A.CR3"), &LookOptions::new(), 1);
        assert_eq!(heard.next().outcome, LookOutcome::Done);
        assert_eq!(
            last_request(&exe)["options"],
            json!({ "optionA": false, "optionB": true })
        );
        heard.ask_with(
            &photo_beside(&exe, "B.CR3"),
            &options(&[("optionA", true), ("optionB", false)]),
            2,
        );
        assert_eq!(heard.next().outcome, LookOutcome::Done);
        assert_eq!(
            last_request(&exe)["options"],
            json!({ "optionA": true, "optionB": false })
        );
    }

    /// An edit can name anything; only what the program offers reaches it.
    #[test]
    fn an_option_it_does_not_offer_is_never_sent() {
        let exe = fake("good");
        let heard = queue_offering(&exe, &[("optionA", false)]);
        let asked = options(&[("gone", true), ("optionA", true)]);
        heard.ask_with(&photo_beside(&exe, "A.CR3"), &asked, 1);
        assert_eq!(heard.next().outcome, LookOutcome::Done);
        assert_eq!(last_request(&exe)["options"], json!({ "optionA": true }));
    }

    #[test]
    fn a_crash_is_a_crash() {
        let (left, got) = make_with("crash");
        assert_eq!(got.err(), Some(Failure::Crash));
        assert!(!left);
    }

    #[test]
    fn a_timeout_kills_the_run_and_clears_the_scratch_file() {
        let started = Instant::now();
        let (left, got) = make_with("slow");
        assert_eq!(got.err(), Some(Failure::Timeout));
        assert!(started.elapsed() < IN_TIME);
        assert!(!left);
    }

    #[test]
    fn a_killed_run_stops_working() {
        let exe = fake("ticking");
        let scratch = exe.with_file_name("scratch.png");
        let ticks = exe.with_file_name("ticks.log");
        let cancel = Arc::new(AtomicBool::new(false));
        let run = {
            let (exe, cancel) = (exe.to_path_buf(), Arc::clone(&cancel));
            std::thread::spawn(move || {
                let subject = subject("/photos/IMG.CR3");
                let key = plain_key(&subject);
                let none = LookOptions::new();
                make_lut(&exe, &ROOMY, &subject, &key, &none, &scratch, &cancel)
            })
        };
        wait_for("it never ran", || ticks.exists());
        cancel.store(true, Ordering::SeqCst);
        assert_eq!(run.join().expect("run").err(), Some(Failure::Cancelled));
        let size = || std::fs::metadata(&ticks).expect("ticks").len();
        let at_the_end = size();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(size(), at_the_end);
    }

    #[test]
    fn bad_json_counts_as_a_crash() {
        let (left, got) = make_with("badjson");
        assert_eq!(got.err(), Some(Failure::Crash));
        assert!(!left);
    }

    #[test]
    fn too_much_output_counts_as_a_crash() {
        let (_, got) = make_with("noisy");
        assert_eq!(got.err(), Some(Failure::Crash));
    }

    #[test]
    fn a_bad_png_is_refused() {
        for kind in ["badpng", "smallpng", "hugepng", "nofile"] {
            let (left, got) = make_with(kind);
            assert_eq!(got.err(), Some(Failure::BadFile), "{kind}");
            assert!(!left, "{kind}");
        }
    }

    #[test]
    fn a_reported_failure_keeps_its_code_and_an_unknown_one_is_internal() {
        for (kind, want) in [
            ("unsupported", Refusal::UnsupportedCamera),
            ("unknown", Refusal::UnknownLook),
            ("missing", Refusal::DataMissing),
            ("strange", Refusal::Internal),
        ] {
            let (_, got) = make_with(kind);
            assert_eq!(got.err(), Some(Failure::Refused(want)), "{kind}");
        }
    }

    // ---------------------------------------------------------------- the queue

    /// Everything a queue reports, in order.
    struct Heard {
        queue: Arc<Queue>,
        told: Receiver<LookProgress>,
        report: Report,
    }

    fn queue_of(exe: &Path) -> Heard {
        queue_offering(exe, &[])
    }

    /// A queue for a program offering these options, with these defaults.
    fn queue_offering(exe: &Path, offered: &[(&str, bool)]) -> Heard {
        let (tell, told) = channel();
        let tell = Mutex::new(tell);
        Heard {
            queue: Queue::new(
                exe.to_path_buf(),
                exe.with_file_name("scratch"),
                ROOMY,
                options(offered),
                Box::new(|_, _, _| {}),
            ),
            told,
            report: Arc::new(move |progress| {
                let _ = tell.lock().unwrap().send(progress);
            }),
        }
    }

    impl Heard {
        fn ask(&self, photo: &Path, job: u64) {
            self.ask_for(photo, "look1", job, false, false);
        }

        fn ask_with(&self, photo: &Path, options: &LookOptions, job: u64) {
            self.queue.request(Request {
                subject: subject(&photo.to_string_lossy()),
                camera: "cam1".to_string(),
                look: "look1".to_string(),
                options: options.clone(),
                own_wb: None,
                remake: false,
                front: false,
                preview: false,
                job,
                report: Arc::clone(&self.report),
            });
        }

        fn ask_for(&self, photo: &Path, look: &str, job: u64, remake: bool, front: bool) {
            self.queue.request(Request {
                subject: subject(&photo.to_string_lossy()),
                camera: "cam1".to_string(),
                look: look.to_string(),
                options: LookOptions::new(),
                own_wb: None,
                remake,
                front,
                preview: false,
                job,
                report: Arc::clone(&self.report),
            });
        }

        fn next(&self) -> LookProgress {
            self.told
                .recv_timeout(Duration::from_secs(60))
                .expect("a report")
        }
    }

    /// A photo in the fake's folder; nothing reads it.
    fn photo_beside(exe: &Path, name: &str) -> PathBuf {
        exe.with_file_name(name)
    }

    fn look_file_of_photo(photo: &Path) -> PathBuf {
        look_file_with(photo, &LookOptions::new())
    }

    fn look_file_with(photo: &Path, options: &LookOptions) -> PathBuf {
        file_ops::look_file_for(photo, "cam1", "look1", options, None).expect("a name")
    }

    #[test]
    fn the_file_lands_beside_the_photo_and_is_reported() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.ask(&photo, 1);
        let told = heard.next();
        assert_eq!(told.outcome, LookOutcome::Done);
        assert_eq!(told.job_id, 1);
        assert_eq!(told.path, photo.to_string_lossy());
        assert_eq!(
            (told.camera.as_str(), told.look.as_str()),
            ("cam1", "look1")
        );
        assert!(look_file_of_photo(&photo).is_file());
    }

    #[test]
    fn a_valid_file_already_there_is_used_with_no_call() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        write_png(&look_file_of_photo(&photo), 512);
        heard.ask(&photo, 1);
        assert_eq!(heard.next().outcome, LookOutcome::Done);
        assert_eq!(calls(&exe), 0);
    }

    #[test]
    fn a_broken_file_already_there_is_made_again() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        std::fs::write(look_file_of_photo(&photo), "half").expect("file");
        heard.ask(&photo, 1);
        assert_eq!(heard.next().outcome, LookOutcome::Done);
        assert_eq!(calls(&exe), 1);
    }

    #[test]
    fn remake_overwrites_the_file_and_replaces_the_cached_lut() {
        let exe = fake("good");
        let (tell, placed) = channel();
        let tell = Mutex::new(tell);
        let queue = Queue::new(
            exe.to_path_buf(),
            exe.with_file_name("scratch"),
            ROOMY,
            LookOptions::new(),
            Box::new(move |_, file, lut| {
                let _ = tell.lock().unwrap().send((file.to_path_buf(), lut.size));
            }),
        );
        let (reply, replied) = channel();
        let reply = Mutex::new(reply);
        let photo = photo_beside(&exe, "IMG.CR3");
        write_png(&look_file_of_photo(&photo), 64);
        queue.request(Request {
            subject: subject(&photo.to_string_lossy()),
            camera: "cam1".to_string(),
            look: "look1".to_string(),
            options: LookOptions::new(),
            own_wb: None,
            remake: true,
            front: false,
            preview: false,
            job: 1,
            report: Arc::new(move |progress| {
                let _ = reply.lock().unwrap().send(progress);
            }),
        });
        let told = replied
            .recv_timeout(Duration::from_secs(60))
            .expect("a report");
        assert_eq!(told.outcome, LookOutcome::Done);
        assert_eq!(calls(&exe), 1);
        assert_eq!(
            placed.recv_timeout(Duration::from_secs(1)).expect("placed"),
            (look_file_of_photo(&photo), 64)
        );
        assert!(checked_file(&std::fs::read(look_file_of_photo(&photo)).expect("file")).is_ok());
    }

    #[test]
    fn frames_with_the_same_settings_share_one_run() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        let photos: Vec<PathBuf> = (0..5)
            .map(|n| photo_beside(&exe, &format!("IMG_{n}.CR3")))
            .collect();
        for photo in &photos {
            heard.ask(photo, 1);
        }
        release(&exe);
        for _ in &photos {
            assert_eq!(heard.next().outcome, LookOutcome::Done);
        }
        assert_eq!(calls(&exe), 1);
        for photo in &photos {
            assert!(look_file_of_photo(photo).is_file());
        }
    }

    #[test]
    fn a_double_click_does_no_extra_work() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.ask(&photo, 1);
        heard.ask(&photo, 2);
        release(&exe);
        assert_eq!(heard.next().outcome, LookOutcome::Done);
        assert_eq!(calls(&exe), 1);
    }

    #[test]
    fn two_run_at_a_time() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        for n in 0..4 {
            let mut other = subject(&photo_beside(&exe, &format!("IMG_{n}.CR3")).to_string_lossy());
            other.wb[0] += n as f32;
            heard.queue.request(Request {
                subject: other,
                camera: "cam1".to_string(),
                look: "look1".to_string(),
                options: LookOptions::new(),
                own_wb: None,
                remake: false,
                front: false,
                preview: false,
                job: 1,
                report: Arc::clone(&heard.report),
            });
        }
        wait_for("two runs", || calls(&exe) >= 2);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(calls(&exe), 2);
        release(&exe);
        for _ in 0..4 {
            assert_eq!(heard.next().outcome, LookOutcome::Done);
        }
        assert_eq!(calls(&exe), 4);
    }

    #[test]
    fn the_open_photo_goes_to_the_front() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        // Two fill the runs; of the two waiting, the last asked goes first.
        for n in 0..3 {
            let mut other = subject(&photo_beside(&exe, &format!("IMG_{n}.CR3")).to_string_lossy());
            other.wb[0] += n as f32;
            heard.queue.request(Request {
                subject: other,
                camera: "cam1".to_string(),
                look: "look1".to_string(),
                options: LookOptions::new(),
                own_wb: None,
                remake: false,
                front: false,
                preview: false,
                job: 1,
                report: Arc::clone(&heard.report),
            });
        }
        let open = photo_beside(&exe, "OPEN.CR3");
        heard.ask_for(&open, "look2", 2, false, true);
        let next = {
            let state = heard.queue.state();
            let photos = state.waiting.iter().map(|item| item.subject.photo.clone());
            photos.collect::<Vec<_>>()
        };
        assert_eq!(next.len(), 2);
        assert_eq!(next[0], open.to_string_lossy());
        release(&exe);
        for _ in 0..4 {
            assert_eq!(heard.next().outcome, LookOutcome::Done);
        }
    }

    #[test]
    fn cancelling_while_others_wait_lets_the_run_go_on() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        let (first, second) = (photo_beside(&exe, "A.CR3"), photo_beside(&exe, "B.CR3"));
        heard.ask(&first, 1);
        heard.ask(&second, 2);
        heard.queue.cancel(1);
        assert_eq!(heard.next().outcome, LookOutcome::Cancelled);
        release(&exe);
        let told = heard.next();
        assert_eq!(told.outcome, LookOutcome::Done);
        assert_eq!(told.path, second.to_string_lossy());
        assert!(!look_file_of_photo(&first).exists());
        assert!(look_file_of_photo(&second).exists());
    }

    #[test]
    fn cancelling_the_last_photo_kills_the_run_and_clears_its_scratch_file() {
        let exe = fake("slow");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.ask(&photo, 1);
        let scratch = exe.with_file_name("scratch");
        let files = || std::fs::read_dir(&scratch).expect("folder").count();
        wait_for("the run left its half file", || files() == 1);
        heard.queue.cancel(1);
        assert_eq!(heard.next().outcome, LookOutcome::Cancelled);
        wait_for("the run stopped", || !heard.queue.is_busy());
        assert_eq!(files(), 0);
        assert!(!look_file_of_photo(&photo).exists());
    }

    #[test]
    fn another_look_on_the_same_photo_replaces_the_waiting_one() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.ask_for(&photo, "look1", 1, false, false);
        heard.ask_for(&photo, "look2", 2, false, false);
        assert_eq!(heard.next().outcome, LookOutcome::Cancelled);
        release(&exe);
        let told = heard.next();
        assert_eq!(
            (told.look.as_str(), told.outcome),
            ("look2", LookOutcome::Done)
        );
        assert!(!look_file_of_photo(&photo).exists());
    }

    /// Unchecking while the checked one is still being made, as a click on
    /// another camera does.
    #[test]
    fn other_options_on_the_same_photo_replace_the_waiting_ones() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        let on = options(&[("optionA", true)]);
        heard.ask_with(&photo, &on, 1);
        heard.ask_with(&photo, &LookOptions::new(), 2);
        let first = heard.next();
        assert_eq!(first.outcome, LookOutcome::Cancelled);
        assert_eq!(first.options, on);
        release(&exe);
        let told = heard.next();
        assert_eq!(told.outcome, LookOutcome::Done);
        assert_eq!(told.options, LookOptions::new());
        assert!(look_file_of_photo(&photo).is_file());
        assert!(!look_file_with(&photo, &on).exists());
    }

    #[test]
    fn frames_with_other_options_do_not_share_a_run() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        let (first, second) = (photo_beside(&exe, "A.CR3"), photo_beside(&exe, "B.CR3"));
        let on = options(&[("optionA", true)]);
        heard.ask_with(&first, &LookOptions::new(), 1);
        heard.ask_with(&second, &on, 2);
        release(&exe);
        for _ in 0..2 {
            assert_eq!(heard.next().outcome, LookOutcome::Done);
        }
        assert_eq!(calls(&exe), 2);
        assert!(look_file_of_photo(&first).is_file());
        assert!(look_file_with(&second, &on).is_file());
    }

    #[test]
    fn a_file_for_other_options_is_not_used() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        write_png(&look_file_of_photo(&photo), 512);
        let on = options(&[("optionA", true)]);
        heard.ask_with(&photo, &on, 1);
        let told = heard.next();
        assert_eq!(told.outcome, LookOutcome::Done);
        assert_eq!(told.options, on);
        assert_eq!(calls(&exe), 1);
        assert!(look_file_with(&photo, &on).is_file());
    }

    /// Saying no to the checked one says nothing about the unchecked one, and
    /// it is not asked again.
    #[test]
    fn a_refusal_holds_for_its_own_options_only() {
        let exe = fake("refuseson");
        let heard = queue_offering(&exe, &[("optionA", false)]);
        let on = options(&[("optionA", true)]);
        heard.ask_with(&photo_beside(&exe, "A.CR3"), &on, 1);
        assert_eq!(heard.next().outcome, LookOutcome::Failed);
        heard.ask_with(&photo_beside(&exe, "B.CR3"), &LookOptions::new(), 2);
        assert_eq!(heard.next().outcome, LookOutcome::Done);
        heard.ask_with(&photo_beside(&exe, "C.CR3"), &on, 3);
        assert_eq!(heard.next().outcome, LookOutcome::Failed);
        assert_eq!(calls(&exe), 2);
    }

    #[test]
    fn two_crashes_in_a_row_turn_the_provider_off() {
        let exe = fake("crash");
        let heard = queue_of(&exe);
        heard.ask(&photo_beside(&exe, "A.CR3"), 1);
        assert_eq!(heard.next().outcome, LookOutcome::Failed);
        assert!(!heard.queue.is_off());
        heard.ask_for(&photo_beside(&exe, "B.CR3"), "look2", 2, false, false);
        assert_eq!(heard.next().outcome, LookOutcome::Failed);
        assert!(heard.queue.is_off());
        heard.ask(&photo_beside(&exe, "C.CR3"), 3);
        assert_eq!(heard.next().outcome, LookOutcome::Failed);
        assert_eq!(calls(&exe), 2);
    }

    #[test]
    fn a_failed_item_is_not_tried_again_on_its_own() {
        let exe = fake("badpng");
        let heard = queue_of(&exe);
        heard.ask(&photo_beside(&exe, "A.CR3"), 1);
        assert_eq!(heard.next().outcome, LookOutcome::Failed);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(calls(&exe), 1);
    }

    #[test]
    fn a_refused_camera_is_not_asked_again_this_session() {
        let exe = fake("unsupported");
        let heard = queue_of(&exe);
        heard.ask(&photo_beside(&exe, "A.CR3"), 1);
        assert_eq!(heard.next().outcome, LookOutcome::Unsupported);
        heard.ask(&photo_beside(&exe, "B.CR3"), 2);
        assert_eq!(heard.next().outcome, LookOutcome::Unsupported);
        assert_eq!(calls(&exe), 1);
    }

    #[test]
    fn a_folder_that_cannot_be_written_stops_before_any_call() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = exe.with_file_name("no-such-folder").join("IMG.CR3");
        heard.ask(&photo, 1);
        assert_eq!(heard.next().outcome, LookOutcome::Folder);
        assert_eq!(calls(&exe), 0);
    }

    #[test]
    fn a_closing_app_leaves_no_run_behind() {
        let exe = fake("slow");
        let heard = queue_of(&exe);
        heard.ask(&photo_beside(&exe, "IMG.CR3"), 1);
        wait_for("the run started", || calls(&exe) == 1);
        heard.queue.stop_all();
        assert_eq!(heard.next().outcome, LookOutcome::Cancelled);
        wait_for("the run stopped", || !heard.queue.is_busy());
    }

    #[test]
    fn an_export_waits_for_the_file() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        let never = AtomicBool::new(false);
        let got = heard.queue.wait_for(
            subject(&photo.to_string_lossy()),
            look_with(LookOptions::new()),
            &never,
        );
        assert_eq!(got, LookOutcome::Done);
        assert!(look_file_of_photo(&photo).is_file());
    }

    #[test]
    fn an_export_waits_for_the_file_of_its_options() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        let on = options(&[("optionA", true)]);
        let got = heard.queue.wait_for(
            subject(&photo.to_string_lossy()),
            look_with(on.clone()),
            &AtomicBool::new(false),
        );
        assert_eq!(got, LookOutcome::Done);
        assert!(look_file_with(&photo, &on).is_file());
        assert!(!look_file_of_photo(&photo).exists());
    }

    #[test]
    fn an_export_gets_on_when_the_look_fails() {
        let exe = fake("crash");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        let never = AtomicBool::new(false);
        let got = heard.queue.wait_for(
            subject(&photo.to_string_lossy()),
            look_with(LookOptions::new()),
            &never,
        );
        assert_eq!(got, LookOutcome::Failed);
    }

    #[test]
    fn a_cancelled_export_stops_waiting_and_kills_the_run() {
        let exe = fake("slow");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        let token = Arc::new(AtomicBool::new(false));
        let waiting = {
            let (queue, token) = (Arc::clone(&heard.queue), Arc::clone(&token));
            let subject = subject(&photo.to_string_lossy());
            std::thread::spawn(move || {
                let none = LookOptions::new();
                queue.wait_for(subject, look_with(none), &token)
            })
        };
        wait_for("the run started", || calls(&exe) == 1);
        token.store(true, Ordering::SeqCst);
        assert_eq!(waiting.join().expect("wait"), LookOutcome::Cancelled);
        wait_for("the run stopped", || !heard.queue.is_busy());
    }

    /// The open photo and its export can wait for the same file.
    #[test]
    fn everyone_waiting_for_a_photo_hears() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.ask(&photo, 1);
        let export = {
            let queue = Arc::clone(&heard.queue);
            let subject = subject(&photo.to_string_lossy());
            std::thread::spawn(move || {
                let never = AtomicBool::new(false);
                let none = LookOptions::new();
                queue.wait_for(subject, look_with(none), &never)
            })
        };
        wait_for("both wait", || {
            heard
                .queue
                .state()
                .items()
                .any(|item| item.waiters.len() == 2)
        });
        release(&exe);
        assert_eq!(heard.next().outcome, LookOutcome::Done);
        assert_eq!(export.join().expect("wait"), LookOutcome::Done);
        assert_eq!(calls(&exe), 1);
    }

    #[test]
    fn a_look_file_is_found_for_a_photo_and_its_virtual_copy() {
        let exe = fake("good");
        let photo = photo_beside(&exe, "IMG.CR3");
        let name = photo.to_string_lossy();
        let none = LookOptions::new();
        assert!(!is_there(&name, "cam1", "look1", &none));
        write_png(&look_file_of_photo(&photo), 512);
        assert!(is_there(&name, "cam1", "look1", &none));
        let copy = format!("{name}?vc=a1b2c3");
        assert!(is_there(&copy, "cam1", "look1", &none));
        assert!(!is_there(&name, "cam2", "look1", &none));
        assert!(!is_there(&name, "../cam1", "look1", &none));
    }

    #[test]
    fn a_look_file_is_found_for_its_options_alone() {
        let exe = fake("good");
        let photo = photo_beside(&exe, "IMG.CR3");
        let name = photo.to_string_lossy();
        let on = options(&[("optionA", true)]);
        write_png(&look_file_of_photo(&photo), 512);
        assert!(!is_there(&name, "cam1", "look1", &on));
        write_png(&look_file_with(&photo, &on), 512);
        assert!(is_there(&name, "cam1", "look1", &on));
    }

    #[test]
    fn leftovers_are_cleared_at_first_use() {
        let exe = fake("good");
        let scratch = exe.with_file_name("scratch");
        std::fs::create_dir_all(&scratch).expect("folder");
        std::fs::write(scratch.join("old.png"), "left").expect("file");
        let _heard = queue_of(&exe);
        assert_eq!(std::fs::read_dir(&scratch).expect("folder").count(), 0);
    }

    // ------------------------------------------------------------ Preview all

    fn numbered(count: usize) -> Catalog {
        let look = |n: usize| OfferedLook {
            id: format!("look{n}"),
            name: format!("Invented Stock {n}"),
            group: None,
        };
        Catalog {
            looks: (1..=count).map(look).collect(),
            ..catalog()
        }
    }

    fn preview_args(photo: &Path) -> ProviderLookPreviewArgs {
        ProviderLookPreviewArgs {
            changes: None,
            path: photo.to_string_lossy().into_owned(),
            camera: "cam1".to_string(),
            options: LookOptions::new(),
        }
    }

    impl Heard {
        /// Preview all on `photo`, for a provider with `count` looks.
        fn preview_all(&self, photo: &Path, count: usize, job: u64) {
            self.queue.preview_all(
                &numbered(count),
                Some(subject(&photo.to_string_lossy())),
                &preview_args(photo),
                job,
                Arc::clone(&self.report),
            );
        }

        fn preview_of(&self, photo: &Path, look: &str) -> PathBuf {
            let none = LookOptions::new();
            let file = self.queue.preview_file(photo, "cam1", look, &none, None);
            file.expect("a name")
        }

        /// A good preview of `look1`, as a press of the button left it.
        fn with_preview(&self, photo: &Path) -> PathBuf {
            let file = self.preview_of(photo, "look1");
            std::fs::create_dir_all(file.parent().expect("folder")).expect("folder");
            write_png(&file, 512);
            file
        }

        fn all_done(&self, count: usize) -> Vec<LookProgress> {
            let told: Vec<LookProgress> = (0..count).map(|_| self.next()).collect();
            for progress in &told {
                assert_eq!(progress.outcome, LookOutcome::Done, "{}", progress.look);
            }
            told
        }
    }

    #[test]
    fn a_preview_is_kept_apart_by_photo_camera_look_and_options() {
        let name = |photo: &str, camera: &str, look: &str, set: &LookOptions| {
            let folder = Path::new("/cache/previews");
            preview_file_for(folder, Path::new(photo), camera, look, set, None).expect("a name")
        };
        let none = LookOptions::new();
        let one = name("/a/IMG.CR3", "cam1", "look1", &none);
        assert_eq!(one, name("/a/IMG.CR3", "cam1", "look1", &none));
        assert_eq!(one.parent(), Some(Path::new("/cache/previews")));
        let others = [
            name("/b/IMG.CR3", "cam1", "look1", &none),
            name("/a/IMG.CR2", "cam1", "look1", &none),
            name("/a/IMG.CR3", "cam2", "look1", &none),
            name("/a/IMG.CR3", "cam1", "look2", &none),
            name(
                "/a/IMG.CR3",
                "cam1",
                "look1",
                &options(&[("optionA", true)]),
            ),
            name(
                "/a/IMG.CR3",
                "cam1",
                "look1",
                &options(&[("optionA", false)]),
            ),
        ];
        let mut all: Vec<&PathBuf> = others.iter().chain([&one]).collect();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), others.len() + 1);
    }

    #[test]
    fn an_id_that_could_not_be_a_file_name_has_no_preview() {
        let none = LookOptions::new();
        let photo = Path::new("/a/IMG.CR3");
        assert_eq!(
            preview_file_for(Path::new("/c"), photo, "../x", "look1", &none, None),
            None
        );
    }

    #[test]
    fn a_preview_is_never_listed_as_a_photo() {
        let on = options(&[("optionA", true)]);
        for set in [LookOptions::new(), on] {
            let photo = Path::new("/a/IMG.CR3");
            let file = preview_file_for(Path::new("/c"), photo, "cam1", "look1", &set, None);
            let file = file.expect("a name");
            assert!(!crate::formats::is_supported_image_file(&file), "{file:?}");
        }
    }

    /// Beside the very photo it was made for, it still belongs to nothing:
    /// not listed, not copied, not moved and not deleted with it.
    #[test]
    fn a_preview_is_no_companion_of_a_photo() {
        let dir = tempfile::tempdir().expect("folder");
        let photo = dir.path().join("IMG.CR3");
        std::fs::write(&photo, "photo").expect("photo");
        let none = LookOptions::new();
        let file =
            preview_file_for(dir.path(), &photo, "cam1", "look1", &none, None).expect("a name");
        write_png(&file, 512);

        let name = file.file_name().and_then(|n| n.to_str()).expect("a name");
        assert_eq!(file_ops::photo_of_look_file(name), None);
        assert!(file_ops::look_file_copies(&photo, &dir.path().join("B.CR3")).is_empty());
        let deleted =
            file_ops::files_to_delete_with_associated(&[photo.to_string_lossy().into_owned()]);
        assert_eq!(deleted, [photo]);
    }

    /// For its swatch and for hover, through the same load as any LUT.
    #[test]
    fn a_preview_loads_as_a_lut() {
        let dir = tempfile::tempdir().expect("folder");
        let none = LookOptions::new();
        let photo = Path::new("/a/IMG.CR3");
        let file =
            preview_file_for(dir.path(), photo, "cam1", "look1", &none, None).expect("a name");
        write_png(&file, 512);
        let lut = lut_processing::parse_lut_file(&file.to_string_lossy()).expect("a LUT");
        assert_eq!(lut.size, 64);
    }

    /// Only the file beside the photo counts as its look.
    #[test]
    fn a_preview_is_never_read_as_an_applied_look() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        let preview = heard.with_preview(&photo);
        let name = photo.to_string_lossy();
        assert!(!look_is_there(&name));
        let edit = json!({
            "lutPath": provider_look::MARKER,
            provider_look::KEY: { "camera": "cam1", "look": "look1" },
        });
        assert_ne!(provider_look::look_file(&edit, &name), Some(preview));
    }

    #[test]
    fn a_preview_lands_in_the_temporary_folder_with_nothing_beside_the_photo() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.preview_all(&photo, 1, 1);
        let told = heard.all_done(1).remove(0);
        assert!(told.preview);
        assert_eq!(told.job_id, 1);
        assert_eq!(told.path, photo.to_string_lossy());
        assert!(heard.preview_of(&photo, "look1").is_file());
        assert!(!look_file_of_photo(&photo).exists());
    }

    /// The temporary file is the program's scratch file's whole purpose, and
    /// a run of Preview all is the normal one for that photo.
    #[test]
    fn a_preview_is_asked_for_like_any_look() {
        let exe = fake("good");
        let heard = queue_offering(&exe, &[("optionA", false)]);
        let photo = photo_beside(&exe, "IMG.CR3");
        let mut args = preview_args(&photo);
        args.options = options(&[("optionA", true)]);
        heard.queue.preview_all(
            &numbered(1),
            Some(subject(&photo.to_string_lossy())),
            &args,
            1,
            Arc::clone(&heard.report),
        );
        heard.all_done(1);
        let sent = last_request(&exe);
        assert_eq!(sent["camera"], "cam1");
        assert_eq!(sent["look"], "look1");
        assert_eq!(sent["asShotWb"].as_array().expect("WB").len(), 4);
        assert_eq!(sent["rendering"]["offsetEv"], 0.25);
        assert_eq!(sent["options"], json!({ "optionA": true }));
    }

    // ------------------------------------------------------ Delete all previews

    /// Stands in for the Recycle Bin, which a test must not fill.
    fn gone(file: &Path) {
        std::fs::remove_file(file).expect("delete");
    }

    fn delete_args(photo: &Path) -> ProviderLookDeleteArgs {
        ProviderLookDeleteArgs {
            path: photo.to_string_lossy().into_owned(),
            on_photo: None,
        }
    }

    fn look_on(camera: &str, look: &str) -> Value {
        json!({
            "lutPath": provider_look::MARKER,
            provider_look::KEY: { "camera": camera, "look": look },
        })
    }

    /// Saves `js` as the edit of `photo`, which a virtual copy's path names.
    fn save_edit(photo: &str, js: &Value) {
        let text = json!({ "version": 1, "rating": 0, "adjustments": js }).to_string();
        std::fs::write(parse_virtual_path(photo).1, text).expect("sidecar");
    }

    fn deleted(deleted: usize, kept: usize, failed: usize) -> LooksDeleted {
        LooksDeleted {
            deleted,
            kept,
            failed,
        }
    }

    #[test]
    fn only_the_photos_own_look_files_go() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR2");
        let own = [
            look_file_of_photo(&photo),
            photo.with_file_name("IMG.CR2.cam2.gone.lut.png"),
            look_file_with(&photo, &options(&[("optionA", true)])),
        ];
        let others = [
            "IMG.CR2",
            "IMG.CR2.agexif",
            "my.film.lut.png",
            "IMG.CR2.jpg.cam1.look1.lut.png",
            "IMG.CR3.cam1.look1.lut.png",
            "OTHER.CR2.cam1.look1.lut.png",
            "notes.txt",
        ]
        .map(|name| photo.with_file_name(name));
        for file in own.iter().chain(&others) {
            std::fs::write(file, "file").expect("file");
        }

        let told = heard.queue.delete_looks(&delete_args(&photo), gone);

        assert_eq!(told, deleted(3, 0, 0));
        assert!(own.iter().all(|file| !file.exists()), "{own:?}");
        assert!(others.iter().all(|file| file.exists()), "{others:?}");
    }

    #[test]
    fn a_look_the_edit_or_a_virtual_copy_uses_stays() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR2");
        let name = photo.to_string_lossy();
        save_edit(&name, &look_on("cam1", "look1"));
        save_edit(&format!("{name}?vc=a1b2c3"), &look_on("cam2", "look2"));
        let (main, copy, spare) = (
            photo.with_file_name("IMG.CR2.cam1.look1.lut.png"),
            photo.with_file_name("IMG.CR2.cam2.look2.lut.png"),
            photo.with_file_name("IMG.CR2.cam2.look1.lut.png"),
        );
        for file in [&main, &copy, &spare] {
            std::fs::write(file, "file").expect("file");
        }

        let told = heard.queue.delete_looks(&delete_args(&photo), gone);

        assert_eq!(told, deleted(1, 2, 0));
        assert!(main.exists() && copy.exists());
        assert!(!spare.exists());
    }

    /// A look goes on and its save follows a moment later.
    #[test]
    fn the_look_on_the_open_edit_stays_before_it_is_saved() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR2");
        let file = look_file_of_photo(&photo);
        std::fs::write(&file, "file").expect("file");
        let on_photo = serde_json::from_value(json!({ "camera": "cam1", "look": "look1" }));
        let args = ProviderLookDeleteArgs {
            on_photo: Some(on_photo.expect("a look")),
            ..delete_args(&photo)
        };

        assert_eq!(heard.queue.delete_looks(&args, gone), deleted(0, 1, 0));
        assert!(file.exists());
    }

    /// A virtual copy is open: the files and the jobs are its photo's.
    #[test]
    fn a_virtual_copys_path_reaches_its_photos_files() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR2");
        let file = look_file_of_photo(&photo);
        std::fs::write(&file, "file").expect("file");
        let preview = heard.with_preview(&photo);
        let copy = PathBuf::from(format!("{}?vc=a1b2c3", photo.display()));

        let told = heard.queue.delete_looks(&delete_args(&copy), gone);

        assert_eq!(told, deleted(2, 0, 0));
        assert!(!file.exists() && !preview.exists());
    }

    #[test]
    fn with_no_provider_nothing_happens() {
        let dir = tempfile::tempdir().expect("folder");
        let photo = dir.path().join("IMG.CR2");
        let file = look_file_of_photo(&photo);
        std::fs::write(&file, "file").expect("file");

        let told = delete_looks_with(None, &delete_args(&photo), |_| panic!("removed"));

        assert!(told.is_err());
        assert!(file.exists());
    }

    /// One at a time, and what is left afterwards is what failed, as the trash
    /// says nothing of how a file went.
    #[test]
    fn a_file_that_will_not_go_leaves_the_rest_to_go_and_is_counted() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR2");
        let stuck = photo.with_file_name("IMG.CR2.cam1.look2.lut.png");
        let rest = ["look1", "look3", "look4"]
            .map(|look| photo.with_file_name(format!("IMG.CR2.cam1.{look}.lut.png")));
        for file in rest.iter().chain([&stuck]) {
            std::fs::write(file, "file").expect("file");
        }

        let told = delete_looks_with(Some(&heard.queue), &delete_args(&photo), |file| {
            if file != stuck {
                gone(file);
            }
        });

        assert_eq!(told, Ok(deleted(3, 0, 1)));
        assert!(stuck.exists());
        assert!(rest.iter().all(|file| !file.exists()), "{rest:?}");
    }

    /// Previews of this photo for several cameras, looks and options.
    fn with_previews(heard: &Heard, exe: &Path, photo: &Path) -> [PathBuf; 3] {
        let previews = [
            heard.with_preview(photo),
            heard.preview_of(photo, "look2"),
            preview_file_for(
                &exe.with_file_name("scratch").join(PREVIEW_FOLDER),
                photo,
                "cam2",
                "look1",
                &options(&[("optionA", true)]),
                None,
            )
            .expect("a name"),
        ];
        for file in &previews {
            write_png(file, 512);
        }
        previews
    }

    /// Right after Preview all there are only these, and the result counts them.
    #[test]
    fn every_preview_of_the_photo_goes_and_no_other() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR2");
        let previews = with_previews(&heard, &exe, &photo);
        let kept = heard.with_preview(&photo_beside(&exe, "OTHER.CR2"));

        let told = heard.queue.delete_looks(&delete_args(&photo), gone);

        assert_eq!(told, deleted(3, 0, 0));
        assert!(previews.iter().all(|file| !file.exists()), "{previews:?}");
        assert!(kept.exists());
    }

    #[test]
    fn the_button_has_nothing_to_clear_on_a_photo_with_neither() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR2");
        heard.with_preview(&photo_beside(&exe, "OTHER.CR2"));
        std::fs::write(photo.with_file_name("my.film.lut.png"), "file").expect("file");
        assert_eq!(heard.queue.clearable(&photo), 0);
    }

    /// In any camera or options, so another option set's previews count too.
    #[test]
    fn the_button_counts_previews_alone() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR2");
        with_previews(&heard, &exe, &photo);
        assert_eq!(heard.queue.clearable(&photo), 3);
    }

    #[test]
    fn the_button_counts_look_files_and_previews_together() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR2");
        std::fs::write(look_file_of_photo(&photo), "file").expect("file");
        with_previews(&heard, &exe, &photo);
        assert_eq!(heard.queue.clearable(&photo), 4);
    }

    #[test]
    fn a_look_on_its_way_to_the_photo_is_called_off() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR2");
        heard.ask(&photo, 1);
        wait_for("the run to start", || calls(&exe) == 1);

        heard.queue.delete_looks(&delete_args(&photo), gone);

        let told = heard.next();
        assert_eq!((told.job_id, told.outcome), (1, LookOutcome::Cancelled));
        wait_for("the run to go", || !heard.queue.is_busy());
        assert!(!look_file_of_photo(&photo).exists());
    }

    #[test]
    fn a_running_preview_is_called_off_with_its_photo() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR2");
        heard.preview_all(&photo, 1, 1);
        wait_for("the run to start", || calls(&exe) == 1);

        heard.queue.delete_looks(&delete_args(&photo), gone);

        assert_eq!(heard.next().outcome, LookOutcome::Cancelled);
        release(&exe);
        wait_for("the queue to empty", || !heard.queue.is_busy());
        assert!(!heard.preview_of(&photo, "look1").exists());
    }

    /// Both ask for the same thing, so they share a run, and it goes on for one.
    #[test]
    fn another_photos_look_carries_on() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        let (photo, other) = (
            photo_beside(&exe, "IMG.CR2"),
            photo_beside(&exe, "OTHER.CR2"),
        );
        heard.ask(&photo, 1);
        heard.ask(&other, 2);
        wait_for("the run to start", || calls(&exe) == 1);

        heard.queue.delete_looks(&delete_args(&photo), gone);

        assert_eq!(heard.next().outcome, LookOutcome::Cancelled);
        release(&exe);
        let told = heard.next();
        assert_eq!((told.job_id, told.outcome), (2, LookOutcome::Done));
        assert!(look_file_of_photo(&other).is_file());
        assert!(!look_file_of_photo(&photo).exists());
    }

    /// What an edit names is being made for it, and an export may be waiting.
    #[test]
    fn a_look_an_edit_is_waiting_for_carries_on() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR2");
        save_edit(&photo.to_string_lossy(), &look_on("cam1", "look1"));
        heard.ask(&photo, 1);
        wait_for("the run to start", || calls(&exe) == 1);

        heard.queue.delete_looks(&delete_args(&photo), gone);

        release(&exe);
        assert_eq!(heard.next().outcome, LookOutcome::Done);
        assert!(look_file_of_photo(&photo).is_file());
    }

    #[test]
    fn a_look_with_its_file_beside_the_photo_is_not_remade() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        write_png(&look_file_of_photo(&photo), 512);
        heard.preview_all(&photo, 1, 1);
        assert!(heard.all_done(1)[0].preview);
        assert_eq!(calls(&exe), 0);
        assert!(!heard.preview_of(&photo, "look1").exists());
    }

    #[test]
    fn a_broken_file_beside_the_photo_does_not_stop_a_preview() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        std::fs::write(look_file_of_photo(&photo), "half").expect("file");
        heard.preview_all(&photo, 1, 1);
        heard.all_done(1);
        assert_eq!(calls(&exe), 1);
    }

    #[test]
    fn a_second_press_makes_nothing_again() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.preview_all(&photo, 2, 1);
        heard.all_done(2);
        heard.preview_all(&photo, 2, 2);
        heard.all_done(2);
        assert_eq!(calls(&exe), 2);
    }

    #[test]
    fn another_photo_with_the_same_name_gets_its_own_preview() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let first = photo_beside(&exe, "IMG.CR3");
        heard.preview_all(&first, 1, 1);
        heard.all_done(1);
        let folder = exe.with_file_name("elsewhere");
        std::fs::create_dir_all(&folder).expect("folder");
        heard.preview_all(&folder.join("IMG.CR3"), 1, 2);
        heard.all_done(1);
        assert_eq!(calls(&exe), 2);
    }

    #[test]
    fn four_previews_run_at_a_time() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        heard.preview_all(&photo_beside(&exe, "IMG.CR3"), 6, 1);
        wait_for("four runs", || calls(&exe) >= 4);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(calls(&exe), 4);
        release(&exe);
        heard.all_done(6);
        assert_eq!(calls(&exe), 6);
    }

    /// What a click, a batch or an export asks for never gets more than it
    /// does today, and the four is for everything together.
    #[test]
    fn what_is_asked_by_hand_stays_two_at_a_time_beside_previews() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        for n in 0..3 {
            let mut other = subject(&photo_beside(&exe, &format!("IMG_{n}.CR3")).to_string_lossy());
            other.wb[0] += n as f32;
            heard.queue.request(Request {
                subject: other,
                camera: "cam1".to_string(),
                look: "look1".to_string(),
                options: LookOptions::new(),
                own_wb: None,
                remake: false,
                front: false,
                preview: false,
                job: 1,
                report: Arc::clone(&heard.report),
            });
        }
        wait_for("two runs", || calls(&exe) >= 2);
        heard.preview_all(&photo_beside(&exe, "OPEN.CR3"), 3, 2);
        wait_for("four runs", || calls(&exe) >= 4);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(calls(&exe), 4);
        {
            let state = heard.queue.state();
            let by_hand = state.running.iter().filter(|item| !item.is_preview());
            assert_eq!(by_hand.count(), 2);
        }
        release(&exe);
        heard.all_done(6);
    }

    #[test]
    fn a_click_goes_ahead_of_the_previews_waiting() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.preview_all(&photo, 6, 1);
        wait_for("four runs", || calls(&exe) >= 4);
        heard.ask_for(&photo, "look9", 2, false, true);
        assert!(!heard.queue.state().waiting[0].is_preview());
        release(&exe);
        heard.all_done(7);
    }

    #[test]
    fn a_click_leaves_the_previews_of_its_photo_running() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.preview_all(&photo, 2, 1);
        heard.ask_for(&photo, "look3", 2, false, false);
        release(&exe);
        let told = heard.all_done(3);
        assert_eq!(told.iter().filter(|p| p.preview).count(), 2);
        assert!(heard.preview_of(&photo, "look2").is_file());
    }

    #[test]
    fn previews_leave_a_click_waiting_on_their_photo_alone() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.ask_for(&photo, "look3", 2, false, false);
        heard.preview_all(&photo, 2, 1);
        release(&exe);
        let told = heard.all_done(3);
        assert_eq!(told.iter().filter(|p| !p.preview).count(), 1);
    }

    #[test]
    fn a_click_during_a_preview_of_that_look_shares_its_run() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.preview_all(&photo, 1, 1);
        wait_for("the run started", || calls(&exe) == 1);
        heard.ask(&photo, 2);
        release(&exe);
        let told = heard.all_done(2);
        assert_eq!(told.iter().filter(|p| p.preview).count(), 1);
        assert_eq!(calls(&exe), 1);
        assert!(look_file_of_photo(&photo).is_file());
        assert!(heard.preview_of(&photo, "look1").is_file());
    }

    #[test]
    fn taking_a_click_back_leaves_the_preview_it_joined_running() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.preview_all(&photo, 1, 1);
        wait_for("the run started", || calls(&exe) == 1);
        heard.ask(&photo, 2);
        heard.queue.cancel(2);
        assert_eq!(heard.next().outcome, LookOutcome::Cancelled);
        release(&exe);
        heard.all_done(1);
        assert!(heard.preview_of(&photo, "look1").is_file());
        assert!(!look_file_of_photo(&photo).exists());
    }

    #[test]
    fn applying_a_look_reuses_its_preview_with_no_run() {
        let exe = fake("crash");
        let (tell, placed) = channel();
        let tell = Mutex::new(tell);
        let queue = Queue::new(
            exe.to_path_buf(),
            exe.with_file_name("scratch"),
            ROOMY,
            LookOptions::new(),
            Box::new(move |_, file, _| {
                let _ = tell.lock().unwrap().send(file.to_path_buf());
            }),
        );
        let heard = Heard {
            queue,
            ..queue_of(&exe)
        };
        let photo = photo_beside(&exe, "IMG.CR3");
        let preview = heard.with_preview(&photo);
        heard.ask(&photo, 1);
        assert!(!heard.all_done(1)[0].preview);
        assert_eq!(calls(&exe), 0);
        let file = look_file_of_photo(&photo);
        assert_eq!(
            std::fs::read(&file).expect("the file"),
            std::fs::read(&preview).expect("the preview")
        );
        let told = placed.recv_timeout(Duration::from_secs(1)).expect("placed");
        assert_eq!(told, file);
    }

    #[test]
    fn applying_a_look_whose_preview_has_gone_makes_it() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        let preview = heard.with_preview(&photo);
        std::fs::remove_file(preview).expect("delete");
        heard.ask(&photo, 1);
        heard.all_done(1);
        assert_eq!(calls(&exe), 1);
        assert!(look_file_of_photo(&photo).is_file());
    }

    #[test]
    fn a_broken_preview_is_never_applied() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        let preview = heard.with_preview(&photo);
        std::fs::write(preview, "half").expect("file");
        heard.ask(&photo, 1);
        heard.all_done(1);
        assert_eq!(calls(&exe), 1);
        let bytes = std::fs::read(look_file_of_photo(&photo)).expect("file");
        assert!(checked_file(&bytes).is_ok());
    }

    #[test]
    fn a_preview_for_another_camera_or_option_is_not_applied() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.with_preview(&photo);
        heard.ask_with(&photo, &options(&[("optionA", true)]), 1);
        heard.all_done(1);
        assert_eq!(calls(&exe), 1);
    }

    /// Remake means run the program again.
    #[test]
    fn a_remake_does_not_take_the_preview() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.with_preview(&photo);
        heard.ask_for(&photo, "look1", 1, true, false);
        heard.all_done(1);
        assert_eq!(calls(&exe), 1);
    }

    #[test]
    fn stopping_takes_what_has_not_started_and_stops_what_has() {
        let exe = fake("paced");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.preview_all(&photo, 6, 1);
        wait_for("four runs", || calls(&exe) >= 4);
        heard.queue.cancel(1);
        for _ in 0..6 {
            assert_eq!(heard.next().outcome, LookOutcome::Cancelled);
        }
        wait_for("the runs stopped", || !heard.queue.is_busy());
        assert_eq!(calls(&exe), 4);
        assert!(!heard.preview_of(&photo, "look1").exists());
    }

    #[test]
    fn a_failed_look_stays_blank_and_the_others_carry_on() {
        let exe = fake("onebad");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.preview_all(&photo, 3, 1);
        let told: Vec<LookProgress> = (0..3).map(|_| heard.next()).collect();
        for progress in told {
            let want = match progress.look.as_str() {
                "look2" => LookOutcome::Failed,
                _ => LookOutcome::Done,
            };
            assert_eq!(progress.outcome, want, "{}", progress.look);
        }
        assert!(heard.preview_of(&photo, "look1").is_file());
        assert!(!heard.preview_of(&photo, "look2").exists());
        assert!(heard.preview_of(&photo, "look3").is_file());
    }

    /// The button counts one answer per look, so every look answers.
    #[test]
    fn a_photo_it_cannot_do_answers_for_every_look_with_no_run() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        let mut other = subject(&photo.to_string_lossy());
        other.model = "Three".to_string();
        for from in [None, Some(other)] {
            heard.queue.preview_all(
                &numbered(2),
                from,
                &preview_args(&photo),
                1,
                Arc::clone(&heard.report),
            );
            for _ in 0..2 {
                let told = heard.next();
                assert_eq!(told.outcome, LookOutcome::Unsupported);
                assert!(told.preview);
            }
        }
        assert_eq!(calls(&exe), 0);
    }

    #[test]
    fn an_option_it_does_not_offer_answers_with_no_run() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        let mut args = preview_args(&photo);
        args.options = options(&[("gone", true)]);
        heard.queue.preview_all(
            &numbered(1),
            Some(subject(&photo.to_string_lossy())),
            &args,
            1,
            Arc::clone(&heard.report),
        );
        assert_eq!(heard.next().outcome, LookOutcome::Unsupported);
        assert_eq!(calls(&exe), 0);
    }

    #[test]
    fn previews_are_listed_for_their_own_options_alone() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        let preview = heard.with_preview(&photo);
        let list = |set: &LookOptions| {
            files_there(&numbered(2), |camera, look| {
                heard.queue.preview_file(&photo, camera, look, set, None)
            })
        };
        assert_eq!(
            list(&LookOptions::new()),
            [LookFile {
                camera: "cam1".to_string(),
                look: "look1".to_string(),
                path: preview.to_string_lossy().into_owned(),
            }]
        );
        assert_eq!(list(&options(&[("optionA", true)])), []);
    }

    #[test]
    fn past_the_budget_the_oldest_previews_go() {
        let dir = tempfile::tempdir().expect("folder");
        let now = std::time::SystemTime::now();
        for n in 0..4u64 {
            let path = dir.path().join(format!("{n}.lut.png"));
            std::fs::write(&path, [0u8; 100]).expect("file");
            let file = std::fs::File::options()
                .write(true)
                .open(&path)
                .expect("file");
            file.set_modified(now - Duration::from_secs(3600 * n))
                .expect("date");
        }
        trim_previews(dir.path(), 400);
        assert_eq!(std::fs::read_dir(dir.path()).expect("folder").count(), 4);
        trim_previews(dir.path(), 250);
        assert!(dir.path().join("0.lut.png").exists());
        assert!(dir.path().join("1.lut.png").exists());
        assert!(!dir.path().join("2.lut.png").exists());
        assert!(!dir.path().join("3.lut.png").exists());
    }

    /// North star 5: however many photos are previewed, the folder holds its
    /// budget and one set.
    #[test]
    fn a_press_brings_the_folder_back_within_its_budget() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        let old = heard.with_preview(&photo_beside(&exe, "OLD.CR3"));
        let file = std::fs::File::options()
            .write(true)
            .open(&old)
            .expect("file");
        file.set_len(MAX_PREVIEW_BYTES + 1).expect("size");
        drop(file);
        heard.preview_all(&photo, 1, 1);
        heard.all_done(1);
        assert!(!old.exists());
        assert!(heard.preview_of(&photo, "look1").is_file());
    }

    #[test]
    fn a_closing_app_leaves_no_temporary_file_and_nothing_else_goes() {
        let exe = fake("good");
        let heard = queue_of(&exe);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.preview_all(&photo, 1, 1);
        heard.all_done(1);
        let outside = exe.with_file_name("outside.png");
        write_png(&outside, 64);
        heard.queue.stop_all();
        assert!(!exe.with_file_name("scratch").exists());
        assert!(outside.is_file());
    }

    #[test]
    fn what_a_crash_left_goes_at_the_next_start_and_nothing_else() {
        let cache = tempfile::tempdir().expect("folder");
        let left = cache.path().join(SCRATCH_FOLDER).join("previews");
        std::fs::create_dir_all(&left).expect("folder");
        std::fs::write(left.join(".a.cam1.look1.lut.png"), "left").expect("file");
        let thumbnails = cache.path().join("thumbnails");
        std::fs::create_dir_all(&thumbnails).expect("folder");
        std::fs::write(thumbnails.join("a.jpg"), "kept").expect("file");

        clear_leftovers(cache.path());
        assert!(!cache.path().join(SCRATCH_FOLDER).exists());
        assert!(thumbnails.join("a.jpg").is_file());
        // Everyone without a provider: nothing there, nothing to do.
        clear_leftovers(cache.path());
    }

    // ---------------------------------------------------------- which camera

    fn catalog() -> Catalog {
        let camera = |id: &str, model: &str| OfferedCamera {
            id: id.to_string(),
            name: model.to_string(),
            make: "Acme".to_string(),
            model: model.to_string(),
        };
        Catalog {
            cameras: vec![camera("cam1", "One"), camera("cam2", "Two")],
            looks: vec![OfferedLook {
                id: "look1".to_string(),
                name: "Invented Stock".to_string(),
                group: None,
            }],
            options: vec![OfferedOption {
                id: "optionA".to_string(),
                label: "Invented choice".to_string(),
                default: false,
                sends_current_wb: false,
            }],
        }
    }

    #[test]
    fn a_catalog_gives_each_option_at_its_default() {
        assert_eq!(catalog().option_defaults(), options(&[("optionA", false)]));
    }

    /// Its look stays on the photo through the file; the program is not asked
    /// to make what it no longer offers.
    #[test]
    fn a_look_with_an_option_no_longer_offered_is_not_on_the_list() {
        let catalog = catalog();
        assert!(catalog.lists("look1", &LookOptions::new()));
        assert!(catalog.lists("look1", &options(&[("optionA", true)])));
        assert!(!catalog.lists("look1", &options(&[("gone", true)])));
        assert!(!catalog.lists("other", &LookOptions::new()));
    }

    #[test]
    fn a_photo_finds_its_camera() {
        assert_eq!(camera_for(&catalog(), None, "Acme", "Two"), Some("cam2"));
        assert_eq!(camera_for(&catalog(), None, "ACME", "two"), Some("cam2"));
    }

    #[test]
    fn a_photo_from_no_camera_on_the_list_gets_none() {
        assert_eq!(camera_for(&catalog(), None, "Acme", "Three"), None);
        assert_eq!(camera_for(&catalog(), None, "Other", "One"), None);
        assert_eq!(camera_for(&catalog(), Some("cam1"), "Acme", "Three"), None);
    }

    // ------------------------------------------------- the current white balance

    fn follow() -> OwnWb {
        OwnWb {
            option: "optionW".to_string(),
            wb: None,
        }
    }

    fn follow_at(temperature: i32, tint: i32) -> OwnWb {
        OwnWb {
            wb: Some(LookWb { temperature, tint }),
            ..follow()
        }
    }

    fn slider_at(temperature: i32) -> Value {
        json!({ "temperature": temperature })
    }

    fn own_file(photo: &Path, set: &LookOptions, own: &OwnWb) -> PathBuf {
        file_ops::look_file_for(photo, "cam1", "look1", set, Some(own)).expect("a name")
    }

    /// As `Heard::ask_with`, for a look that follows the white balance.
    fn ask_following(heard: &Heard, photo: &Path, set: &LookOptions, own: &OwnWb, job: u64) {
        heard.queue.request(Request {
            subject: subject(&photo.to_string_lossy()),
            camera: "cam1".to_string(),
            look: "look1".to_string(),
            options: set.clone(),
            own_wb: Some(own.clone()),
            remake: false,
            front: false,
            preview: false,
            job,
            report: Arc::clone(&heard.report),
        });
    }

    #[test]
    fn a_program_may_flag_the_option_that_follows_the_white_balance() {
        let kept = options_kept(
            r#"[{ "id": "optionA", "label": "Plain", "default": false },
                { "id": "optionW", "label": "Follows", "default": false, "sendsCurrentWb": true }]"#,
        );
        assert!(
            !kept[0].sends_current_wb,
            "a list from before the flag has none"
        );
        assert!(kept[1].sends_current_wb);
    }

    #[test]
    fn a_flag_of_the_wrong_shape_drops_its_option_alone() {
        let kept = options_kept(
            r#"[{ "id": "optionA", "label": "Plain", "default": false },
                { "id": "optionW", "label": "Bad", "default": false, "sendsCurrentWb": "yes" }]"#,
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "optionA");
    }

    fn catalog_following() -> Catalog {
        let exe = fake("followswb");
        validate(&exe.to_string_lossy(), &ROOMY).expect("a list")
    }

    #[test]
    fn only_the_flagged_option_on_follows_the_white_balance() {
        let catalog = catalog_following();
        assert_eq!(catalog.wb_option(&LookOptions::new()), None);
        assert_eq!(catalog.wb_option(&options(&[("optionA", true)])), None);
        assert_eq!(catalog.wb_option(&options(&[("optionW", false)])), None);
        assert_eq!(
            catalog.wb_option(&options(&[("optionW", true)])),
            Some("optionW")
        );
    }

    #[test]
    fn a_flagged_option_on_by_default_follows_with_nothing_chosen() {
        let mut catalog = catalog_following();
        catalog.options[1].default = true;
        assert_eq!(catalog.wb_option(&LookOptions::new()), Some("optionW"));
        assert_eq!(catalog.wb_option(&options(&[("optionW", false)])), None);
    }

    /// A program from before the flag never has an edit read for it.
    #[test]
    fn with_no_flagged_option_on_nothing_follows() {
        let exe = fake("good");
        let photo = subject(&photo_beside(&exe, "IMG.CR3").to_string_lossy());
        let moved = slider_at(40);
        let on = options(&[("optionA", true)]);
        assert_eq!(catalog().own_wb(&photo, &on, Some(&moved)), None);
    }

    #[test]
    fn at_the_cameras_white_balance_there_is_nothing_of_its_own() {
        let photo = subject("/photos/IMG.CR3");
        assert_eq!(photo.look_wb(&json!({})), None);
        assert_eq!(photo.look_wb(&slider_at(0)), None);
        let same = json!({ "whiteBalance": { "temperature": 5200.0, "tint": 4.0 } });
        assert_eq!(photo.look_wb(&same), None);
    }

    #[test]
    fn a_moved_slider_gives_a_white_balance_of_its_own() {
        let photo = subject("/photos/IMG.CR3");
        let warmer = photo.look_wb(&slider_at(40)).expect("its own");
        assert!(warmer.temperature > 5200, "{warmer:?}");
        assert_eq!(warmer.tint, 4);
        let kelvin = json!({ "whiteBalance": { "temperature": 4300.0, "tint": -12.0 } });
        assert_eq!(
            photo.look_wb(&kelvin),
            Some(LookWb {
                temperature: 4300,
                tint: -12
            })
        );
    }

    /// Basic's eye shut renders the camera's white balance, so that is the LUT.
    #[test]
    fn a_hidden_white_balance_is_the_cameras() {
        let photo = subject("/photos/IMG.CR3");
        let hidden = json!({ "temperature": 40, "sectionVisibility": { "basic": false } });
        assert_eq!(photo.look_wb(&hidden), None);
    }

    /// A sensor this matrix does not describe can follow nothing.
    #[test]
    fn a_photo_with_no_matrix_has_nothing_of_its_own() {
        let mut photo = subject("/photos/IMG.CR3");
        photo.xyz_to_camera = None;
        assert_eq!(photo.look_wb(&slider_at(40)), None);
    }

    fn own_wb_in_rgb(photo: &Subject, wb: LookWb) -> [f32; 3] {
        let as_shot = photo.white;
        let neutral = as_shot.camera_neutral(&CAMERA_D65).expect("as shot");
        let neutral = neutral.map(|n| n as f32);
        let columns = crate::white_balance::camera_to_srgb(&CAMERA_D65, &neutral).expect("colour");
        let coefficients = photo.coefficients_of(wb).expect("in the form of asShotWb");

        // What the sensor recorded of that white, through the decode...
        let sensor = [
            1.0 / coefficients[0],
            1.0 / coefficients[1],
            1.0 / coefficients[3],
        ];
        let mut rgb = glam::Vec3::ZERO;
        for (column, recorded) in columns.iter().zip(sensor) {
            rgb += glam::Vec3::from_array(*column) * recorded;
        }
        // ...and through the slider, as `apply_white_balance` in shader.wgsl.
        let gains = crate::white_balance::adaptation_log_gains(as_shot, wb.white()).map(f32::exp);
        let rgb_to_lms = crate::white_balance::rgb_to_lms();
        let moved = rgb_to_lms.inverse() * (glam::Vec3::from_array(gains) * (rgb_to_lms * rgb));
        moved.to_array()
    }

    /// What the LUT must take back to neutral is the white the render made
    /// neutral: at any slider, what the camera records of that white comes out
    /// of the decode and the slider with no colour left.
    #[test]
    fn the_render_makes_neutral_the_white_the_program_is_told_of() {
        let photo = subject("/photos/IMG.CR3");
        for (temperature, tint) in [(3200, 0), (4300, -20), (5200, 4), (6500, 30), (9000, -60)] {
            let wb = LookWb { temperature, tint };
            let [r, g, b] = own_wb_in_rgb(&photo, wb);
            for channel in [r, b] {
                assert!((channel / g - 1.0).abs() < 1e-3, "{wb:?}: {r} {g} {b}");
            }
        }
    }

    /// The way back gives the white it started from.
    #[test]
    fn a_white_balance_reads_back_from_what_the_program_is_told() {
        let photo = subject("/photos/IMG.CR3");
        for (temperature, tint) in [(3200, 0), (4300, -20), (6500, 30), (9000, -60)] {
            let wb = LookWb { temperature, tint };
            let [r, g, _, b] = photo.coefficients_of(wb).expect("in the form of asShotWb");
            let neutral = [1.0 / r, 1.0 / g, 1.0 / b];
            let white = WhiteBalance::from_camera_neutral(&CAMERA_D65, &neutral).expect("a white");
            assert!(
                (white.temperature - f64::from(temperature)).abs() < 1.0,
                "{white:?}"
            );
            assert!((white.tint - f64::from(tint)).abs() < 0.2, "{white:?}");
        }
    }

    /// Green as `asShotWb`'s, whatever the decoder's scale: the program divides by it.
    #[test]
    fn the_current_white_balance_is_at_the_scale_of_the_as_shot_one() {
        let mut photo = subject("/photos/IMG.CR3");
        photo.wb = [2.0, 2.0, 2.0, 2.0];
        let wb = LookWb {
            temperature: 4300,
            tint: 0,
        };
        let [r, g1, g2, b] = photo.coefficients_of(wb).expect("in the form of asShotWb");
        assert_eq!((g1, g2), (2.0, 2.0));
        assert!(r > 0.0 && b > 0.0);
        photo.wb = [1.0; 4];
        let halved = photo.coefficients_of(wb).expect("in the form of asShotWb");
        assert!((halved[0] * 2.0 - r).abs() < 1e-5);
    }

    #[test]
    fn the_program_is_sent_the_current_white_balance_and_the_as_shot_one() {
        let exe = fake("good");
        let heard = queue_offering(&exe, &[("optionW", false)]);
        let photo = photo_beside(&exe, "IMG.CR3");
        let on = options(&[("optionW", true)]);
        let own = follow_at(4300, -12);
        ask_following(&heard, &photo, &on, &own, 1);

        let told = heard.next();
        assert_eq!(told.outcome, LookOutcome::Done);
        assert_eq!(told.own_wb, Some(own.clone()));
        let sent = last_request(&exe);
        let photo_told = subject("/photos/IMG.CR3");
        let current = photo_told.coefficients_of(own.wb.unwrap());
        let number = |field: &str, at: usize| sent[field][at].as_f64().expect("a number") as f32;
        for (at, value) in current.expect("a white balance").into_iter().enumerate() {
            assert_eq!(number("currentWb", at), value);
        }
        for (at, value) in photo_told.wb.into_iter().enumerate() {
            assert_eq!(number("asShotWb", at), value);
        }
        assert_eq!(sent["options"], json!({ "optionW": true }));
        assert!(own_file(&photo, &on, &own).is_file());
        assert!(!look_file_of_photo(&photo).exists(), "not the as-shot file");
    }

    /// The tool decides what wins: both go, and Argentum knows neither.
    #[test]
    fn both_options_go_to_the_program_together() {
        let exe = fake("good");
        let heard = queue_offering(&exe, &[("optionA", false), ("optionW", false)]);
        let photo = photo_beside(&exe, "IMG.CR3");
        let on = options(&[("optionA", true), ("optionW", true)]);
        ask_following(&heard, &photo, &on, &follow_at(4300, 0), 1);
        assert_eq!(heard.next().outcome, LookOutcome::Done);
        let sent = last_request(&exe);
        assert_eq!(sent["options"], json!({ "optionA": true, "optionW": true }));
        assert!(sent["currentWb"].is_array(), "{sent}");
    }

    #[test]
    fn at_the_cameras_white_balance_the_program_gets_the_as_shot_one_as_it_stands() {
        let exe = fake("good");
        let heard = queue_offering(&exe, &[("optionW", false)]);
        let photo = photo_beside(&exe, "IMG.CR3");
        ask_following(&heard, &photo, &options(&[("optionW", true)]), &follow(), 1);
        assert_eq!(heard.next().outcome, LookOutcome::Done);
        let sent = last_request(&exe);
        assert_eq!(sent["currentWb"], sent["asShotWb"]);
        assert!(look_file_of_photo(&photo).is_file(), "the as-shot name");
    }

    /// The file of the other option alone, so nothing need know what that does.
    #[test]
    fn beside_another_option_the_cameras_white_balance_needs_no_run() {
        let exe = fake("good");
        let heard = queue_offering(&exe, &[("optionA", false), ("optionW", false)]);
        let photo = photo_beside(&exe, "IMG.CR3");
        let alone = options(&[("optionA", true)]);
        let both = options(&[("optionA", true), ("optionW", true)]);
        heard.ask_with(&photo, &alone, 1);
        assert_eq!(heard.next().outcome, LookOutcome::Done);
        ask_following(&heard, &photo, &both, &follow(), 2);
        assert_eq!(heard.next().outcome, LookOutcome::Done);
        assert_eq!(calls(&exe), 1, "the file was there");
        assert_eq!(
            own_file(&photo, &both, &follow()),
            look_file_with(&photo, &alone)
        );
    }

    /// With no file yet, the request says it all: the program decides from it.
    #[test]
    fn beside_another_option_the_cameras_white_balance_still_goes_to_the_program() {
        let exe = fake("good");
        let heard = queue_offering(&exe, &[("optionA", false), ("optionW", false)]);
        let photo = photo_beside(&exe, "IMG.CR3");
        let both = options(&[("optionA", true), ("optionW", true)]);
        ask_following(&heard, &photo, &both, &follow(), 1);
        assert_eq!(heard.next().outcome, LookOutcome::Done);
        let sent = last_request(&exe);
        assert_eq!(sent["currentWb"], sent["asShotWb"]);
        assert_eq!(sent["options"], json!(both));
        assert!(look_file_with(&photo, &options(&[("optionA", true)])).is_file());
    }

    /// Exactly today's request and today's file, from a program with no flag.
    #[test]
    fn a_program_with_no_flag_is_asked_as_it_always_was() {
        let exe = fake("good");
        let heard = queue_offering(&exe, &[("optionA", false)]);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.ask_with(&photo, &options(&[("optionA", true)]), 1);
        assert_eq!(heard.next().own_wb, None);
        let sent = last_request(&exe);
        assert!(sent.get("currentWb").is_none(), "{sent}");
        assert_eq!(sent["options"], json!({ "optionA": true }));
        assert!(look_file_with(&photo, &options(&[("optionA", true)])).is_file());
    }

    #[test]
    fn the_option_on_at_the_cameras_white_balance_needs_no_run() {
        let exe = fake("good");
        let heard = queue_offering(&exe, &[("optionW", false)]);
        let photo = photo_beside(&exe, "IMG.CR3");
        heard.ask(&photo, 1);
        assert_eq!(heard.next().outcome, LookOutcome::Done);
        assert_eq!(calls(&exe), 1);

        ask_following(&heard, &photo, &options(&[("optionW", true)]), &follow(), 2);
        assert_eq!(heard.next().outcome, LookOutcome::Done);
        assert_eq!(calls(&exe), 1, "the file was there");
    }

    #[test]
    fn each_white_balance_keeps_its_file_and_going_back_needs_no_run() {
        let exe = fake("good");
        let heard = queue_offering(&exe, &[("optionW", false)]);
        let photo = photo_beside(&exe, "IMG.CR3");
        let on = options(&[("optionW", true)]);
        let (warm, cool) = (follow_at(4300, 0), follow_at(6500, 0));
        for (job, own) in [(1, &warm), (2, &cool)] {
            ask_following(&heard, &photo, &on, own, job);
            assert_eq!(heard.next().outcome, LookOutcome::Done);
        }
        assert_eq!(calls(&exe), 2);
        assert_ne!(own_file(&photo, &on, &warm), own_file(&photo, &on, &cool));
        assert!(own_file(&photo, &on, &warm).is_file());
        assert!(own_file(&photo, &on, &cool).is_file());

        ask_following(&heard, &photo, &on, &warm, 3);
        assert_eq!(heard.next().outcome, LookOutcome::Done);
        assert_eq!(calls(&exe), 2, "it was there");
    }

    #[test]
    fn a_newer_white_balance_wins_over_an_older_one_on_its_way() {
        let exe = fake("paced");
        let heard = queue_offering(&exe, &[("optionW", false)]);
        let photo = photo_beside(&exe, "IMG.CR3");
        let on = options(&[("optionW", true)]);
        let (older, newer) = (follow_at(4300, 0), follow_at(6500, 0));

        ask_following(&heard, &photo, &on, &older, 1);
        wait_for("the older run", || calls(&exe) == 1);
        ask_following(&heard, &photo, &on, &newer, 2);
        let told = heard.next();
        assert_eq!((told.job_id, told.outcome), (1, LookOutcome::Cancelled));

        release(&exe);
        let told = heard.next();
        assert_eq!((told.job_id, told.outcome), (2, LookOutcome::Done));
        assert!(own_file(&photo, &on, &newer).is_file());
        assert!(!own_file(&photo, &on, &older).exists());
    }

    /// One white balance the program cannot do says nothing of another.
    #[test]
    fn a_refusal_is_for_that_white_balance_alone() {
        let exe = fake("missing");
        let heard = queue_offering(&exe, &[("optionW", false)]);
        let photo = photo_beside(&exe, "IMG.CR3");
        let on = options(&[("optionW", true)]);
        ask_following(&heard, &photo, &on, &follow_at(4300, 0), 1);
        assert_eq!(heard.next().outcome, LookOutcome::Failed);
        ask_following(&heard, &photo, &on, &follow_at(6500, 0), 2);
        assert_eq!(heard.next().outcome, LookOutcome::Failed);
        assert_eq!(calls(&exe), 2);
        ask_following(&heard, &photo, &on, &follow_at(4300, 0), 3);
        assert_eq!(heard.next().outcome, LookOutcome::Failed);
        assert_eq!(calls(&exe), 2, "this one was refused");
    }

    #[test]
    fn a_white_balance_that_cannot_be_told_is_not_asked_for() {
        let exe = fake("good");
        let heard = queue_offering(&exe, &[("optionW", false)]);
        let mut photo = subject(&photo_beside(&exe, "IMG.CR3").to_string_lossy());
        photo.xyz_to_camera = None;
        heard.queue.request(Request {
            subject: photo,
            camera: "cam1".to_string(),
            look: "look1".to_string(),
            options: options(&[("optionW", true)]),
            own_wb: Some(follow_at(4300, 0)),
            remake: false,
            front: false,
            preview: false,
            job: 1,
            report: Arc::clone(&heard.report),
        });
        assert_eq!(heard.next().outcome, LookOutcome::Failed);
        assert_eq!(calls(&exe), 0);
    }

    #[test]
    fn delete_all_previews_takes_the_files_of_every_white_balance() {
        let exe = fake("good");
        let heard = queue_offering(&exe, &[("optionW", false)]);
        let photo = photo_beside(&exe, "IMG.CR3");
        let on = options(&[("optionW", true)]);
        let (warm, cool) = (follow_at(4300, 0), follow_at(6500, 0));
        for (job, own) in [(1, &warm), (2, &cool)] {
            ask_following(&heard, &photo, &on, own, job);
            assert_eq!(heard.next().outcome, LookOutcome::Done);
        }
        let on_photo = provider_look::ProviderLook {
            options: on.clone(),
            own_wb: Some(cool.clone()),
            ..look_with(LookOptions::new())
        };

        let args = ProviderLookDeleteArgs {
            on_photo: Some(on_photo),
            ..delete_args(&photo)
        };
        assert_eq!(heard.queue.delete_looks(&args, gone), deleted(1, 1, 0));
        assert!(!own_file(&photo, &on, &warm).exists());
        assert!(own_file(&photo, &on, &cool).is_file(), "the edit uses it");

        assert_eq!(
            heard.queue.delete_looks(&delete_args(&photo), gone),
            deleted(1, 0, 0)
        );
        assert!(!own_file(&photo, &on, &cool).exists());
    }

    /// The saved edit counts as well as the open one.
    #[test]
    fn delete_all_previews_keeps_the_white_balance_the_saved_edit_uses() {
        let exe = fake("good");
        let heard = queue_offering(&exe, &[("optionW", false)]);
        let photo = photo_beside(&exe, "IMG.CR3");
        let on = options(&[("optionW", true)]);
        let own = follow_at(4300, 0);
        ask_following(&heard, &photo, &on, &own, 1);
        assert_eq!(heard.next().outcome, LookOutcome::Done);
        let mut edit = look_on("cam1", "look1");
        edit[provider_look::KEY]["options"] = json!(on);
        edit[provider_look::KEY]["ownWb"] = json!(own);
        save_edit(&photo.to_string_lossy(), &edit);

        assert_eq!(
            heard.queue.delete_looks(&delete_args(&photo), gone),
            deleted(0, 1, 0)
        );
        assert!(own_file(&photo, &on, &own).is_file());
    }

    #[test]
    fn preview_all_makes_every_look_for_the_current_white_balance() {
        let exe = fake("followswb");
        let heard = queue_offering(&exe, &[("optionA", false), ("optionW", false)]);
        let photo = photo_beside(&exe, "IMG.CR3");
        let set = options(&[("optionW", true)]);
        let args = ProviderLookPreviewArgs {
            options: set.clone(),
            changes: Some(slider_at(40)),
            ..preview_args(&photo)
        };
        let catalog = catalog_following();
        let subject = subject(&photo.to_string_lossy());
        let wb = subject.look_wb(&slider_at(40)).expect("its own");
        heard
            .queue
            .preview_all(&catalog, Some(subject), &args, 1, Arc::clone(&heard.report));

        let told = heard.next();
        assert_eq!(told.outcome, LookOutcome::Done);
        assert!(last_request(&exe)["currentWb"].is_array());
        let own = OwnWb {
            option: "optionW".to_string(),
            wb: Some(wb),
        };
        let file = heard
            .queue
            .preview_file(&photo, "cam1", "look1", &set, Some(&own));
        assert!(file.expect("a name").is_file());
        assert!(!look_file_of_photo(&photo).exists());
    }

    /// An export of a photo with a look of its own makes that file, not another.
    #[test]
    fn an_export_waits_for_the_file_of_its_white_balance() {
        let exe = fake("good");
        let heard = queue_offering(&exe, &[("optionW", false)]);
        let photo = photo_beside(&exe, "IMG.CR3");
        let on = options(&[("optionW", true)]);
        let own = follow_at(4300, 0);
        let look = provider_look::ProviderLook {
            own_wb: Some(own.clone()),
            ..look_with(on.clone())
        };
        let never = AtomicBool::new(false);
        let got = heard
            .queue
            .wait_for(subject(&photo.to_string_lossy()), look, &never);
        assert_eq!(got, LookOutcome::Done);
        assert!(own_file(&photo, &on, &own).is_file());
    }

    #[test]
    fn what_a_saved_edit_does_not_hold_yet_goes_over_it() {
        let dir = tempfile::tempdir().expect("folder");
        let photo = dir.path().join("IMG.CR3");
        let name = photo.to_string_lossy();
        save_edit(&name, &json!({ "temperature": 10, "tint": 5 }));
        assert_eq!(
            edit_of(&name, Some(&slider_at(40))),
            json!({ "temperature": 40, "tint": 5 })
        );
        assert_eq!(
            edit_of(&name, None),
            json!({ "temperature": 10, "tint": 5 })
        );
    }

    /// A photo never saved has no edit to go over.
    #[test]
    fn changes_stand_alone_for_a_photo_with_no_edit() {
        let dir = tempfile::tempdir().expect("folder");
        let photo = dir.path().join("IMG.CR3");
        let moved = slider_at(40);
        assert_eq!(edit_of(&photo.to_string_lossy(), Some(&moved)), moved);
    }

    #[test]
    fn a_chosen_camera_stays_while_it_is_on_the_list() {
        assert_eq!(
            camera_for(&catalog(), Some("cam1"), "Acme", "Two"),
            Some("cam1")
        );
        assert_eq!(
            camera_for(&catalog(), Some("gone"), "Acme", "Two"),
            Some("cam2")
        );
    }
}
