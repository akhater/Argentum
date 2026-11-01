//! One Tauri command for all of Argentum's, forever.
//!
//! WHY
//!
//! `tauri::generate_handler!` takes a literal list, and it lives in *their*
//! `lib.rs`. Registering commands one by one means one line of upstream code per
//! feature — the cost that decides whether a fork survives. Four features is
//! four lines and looks free. A hundred is a hundred lines in the file every
//! upstream release also edits, and the merge becomes an argument every time.
//!
//! So the frontend calls a single command, `ag`, with a name and a bag of
//! arguments, and the match below routes it. Adding a command from here on costs
//! one arm in this file and nothing at all in theirs.
//!
//! WHAT IT COSTS
//!
//! Tauri's per-command argument typing happens here instead, in a struct per
//! command, deserialised explicitly. That is the same checking, in our file
//! rather than generated in theirs — and it is why each arm names its arguments
//! rather than passing `serde_json::Value` around.
//!
//! An unknown name is an error rather than a silent no-op: a typo in a command
//! name should fail loudly and immediately, not look like a feature that quietly
//! does nothing.
//!
//! THE OTHER HALF
//!
//! `src/argentum/ag.ts` is the matching wrapper, so callers write
//! `ag('detect_auto_white_balance', { ... })` and never see the envelope.
//!
//! BYTES
//!
//! Every answer is JSON except `engine_frame`'s, which carries a frame's
//! pixels (`engine_frame.rs`): as raw bytes, which Tauri hands to the page as
//! an ArrayBuffer, or beside the answer in a WebView2 shared buffer.

use crate::app_state::AppState;
use crate::mods::auto_wb::DetectMode;
use crate::mods::commands;
use crate::mods::engine_bridge::bridge;
use crate::mods::{engine_frame, shell_photos};
use argentum_engine::contract::{EditPatch, EngineError, ErrorKind, PhotoId, RenderTicket, View};
use serde::{Deserialize, Serialize};
use tauri::ipc::Response;

/// Pull one command's arguments out of the bag, or say which command failed.
fn args_for<T: for<'de> Deserialize<'de>>(
    name: &str,
    args: serde_json::Value,
) -> Result<T, String> {
    serde_json::from_value(args).map_err(|e| format!("{name}: bad arguments: {e}"))
}

/// Arguments are sent as the frontend writes them, so camelCase throughout.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct PointArgs {
    x: f32,
    y: f32,
    /// The edit, as the interface holds it: untyped on both sides.
    #[cfg_attr(test, ts(type = "unknown"))]
    js_adjustments: serde_json::Value,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct AutoWbArgs {
    /// The edit, as the interface holds it: untyped on both sides.
    #[cfg_attr(test, ts(type = "unknown"))]
    js_adjustments: serde_json::Value,
    mode: DetectMode,
}

/// A photo to measure Auto white balance on, and the photo a pasted Auto came from.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct MeasureWbArgs {
    path: String,
    from: Option<String>,
}

/// A one-click selection: which part, the edit's geometry, and the frame it is stored in.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct SelectionArgs {
    kind: crate::mods::selection::Kind,
    /// The edit, as the interface holds it: untyped on both sides.
    #[cfg_attr(test, ts(type = "unknown"))]
    js_adjustments: serde_json::Value,
    #[serde(flatten)]
    frame: crate::mods::selection::Frame,
}

/// Object: the clicks and the box on the photo, the edit's geometry, and the frame.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct ObjectArgs {
    /// The edit, as the interface holds it: untyped on both sides.
    #[cfg_attr(test, ts(type = "unknown"))]
    js_adjustments: serde_json::Value,
    #[serde(default)]
    clicks: Vec<crate::mods::selection::Click>,
    #[serde(default)]
    bounds: Option<crate::mods::selection::Bounds>,
    #[serde(flatten)]
    frame: crate::mods::selection::Frame,
}

/// A Text mask: the words, the edit's geometry, and the frame.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct TextMaskArgs {
    /// The edit, as the interface holds it: untyped on both sides.
    #[cfg_attr(test, ts(type = "unknown"))]
    js_adjustments: serde_json::Value,
    words: String,
    #[serde(flatten)]
    frame: crate::mods::selection::Frame,
}

/// A Face mask: the parts to draw, the edit's geometry, and the frame.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct FaceMaskArgs {
    /// The edit, as the interface holds it: untyped on both sides.
    #[cfg_attr(test, ts(type = "unknown"))]
    js_adjustments: serde_json::Value,
    #[cfg_attr(test, ts(type = "ReadonlyArray<string>"))]
    parts: Vec<String>,
    #[serde(flatten)]
    frame: crate::mods::selection::Frame,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct PathArgs {
    path: String,
}

/// A folder's path before and after a rename.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct FolderRenameArgs {
    old_path: String,
    new_path: String,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct PathsArgs {
    paths: Vec<String>,
}

/// The open photo, as the editor names it, and its edit.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct OpenPhotoArgs {
    path: String,
    /// The edit, as the interface holds it: untyped on both sides.
    #[cfg_attr(test, ts(type = "unknown"))]
    js_adjustments: serde_json::Value,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct RawToneArgs {
    path: String,
    mode: String,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct FileArgs {
    file: String,
}

/// The app's RAW decoder.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct DecoderArgs {
    decoder: crate::mods::raw_decoder::RawDecoder,
}

/// A photo's RAW decoder; none is the app's.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct PhotoDecoderArgs {
    path: String,
    decoder: Option<crate::mods::raw_decoder::RawDecoder>,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct OnArgs {
    on: bool,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct SuperResolutionPreviewArgs {
    path: String,
    scale: u32,
    /// Restore's Mask slider, when Restore is on.
    #[serde(default)]
    restore: Option<f32>,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct SuperResolutionSaveArgs {
    path: String,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct RestoreMaskArgs {
    path: String,
    mask: f32,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct RestoreReblendArgs {
    mask: f32,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct SuperResolutionBatchArgs {
    paths: Vec<String>,
    scale: u32,
    #[serde(default)]
    restore: Option<f32>,
}

/// A TIFF bit depth as the interface names it: 8 or 16.
///
/// A plain `u8` rather than an enum over the wire, because the value crossing
/// this boundary comes from a browser and any number is possible. It is narrowed
/// to a depth we support by `TiffDepth::from_u8`, which treats anything
/// unrecognised as 16 - what an export did before the setting existed.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct DepthArgs {
    depth: u8,
}

/// Space under a compact slider row, in pixels.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct GapArgs {
    gap: u8,
}

/// Looks' ids: a built-in preset's, a preset's own, a LUT's path.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct IdsArgs {
    ids: Vec<String>,
}

/// The open photo and the LUT just put on it (its path).
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct LookPickArgs {
    path: String,
    lut: String,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct LearningArgs {
    on: bool,
}

/// The photo the editor holds, if it is the one asked about: the editor may
/// ask while another photo is still being decoded.
fn the_open_photo(state: &AppState, path: &str) -> Result<crate::app_state::LoadedImage, String> {
    let loaded = state
        .original_image
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or("No image loaded")?;
    let source = |path: &str| crate::file_management::parse_virtual_path(path).0;
    if source(path) != source(&loaded.path) {
        return Err(format!("'{path}' is not the loaded image"));
    }
    Ok(loaded)
}

/// A photo and the snapshots to keep for it.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct SnapshotsArgs {
    path: String,
    snapshots: Vec<crate::mods::snapshots::Snapshot>,
}

/// A photo and the History to keep for it; none takes it out.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct HistoryArgs {
    path: String,
    record: Option<crate::mods::history::HistoryRecord>,
}

/// A text size's name, narrowed by `TextSize::from_name`, Normal if unknown.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct TextSizeArgs {
    size: String,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct LayoutPanelsArgs {
    panels: crate::mods::editor_layout::LayoutPanels,
}

/// A Modern palette's name, narrowed by `ModernTheme::from_name`.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct ThemeArgs {
    theme: String,
}

/// The library's saved folder state, for `modern_start`.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct ModernStartArgs {
    last_folder: Option<String>,
    #[serde(default)]
    expanded: Vec<String>,
}

/// A catalogue id from `mods::model_catalog`, never a path.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct IdArgs {
    id: String,
}

/// Leftovers in the models folder, as `list_ai_models` named them.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct NamesArgs {
    names: Vec<String>,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct ModelArgs {
    /// Absent for commands that only identify a camera by model.
    #[serde(default)]
    #[cfg_attr(test, ts(as = "Option<String>", optional))]
    make: String,
    model: String,
}

/// A camera and the profile its new photos open with (none: built-in).
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct DefaultProfileArgs {
    model: String,
    file: Option<String>,
}

#[derive(Deserialize)]
struct ApplyArgs {
    photo: PhotoId,
    patch: EditPatch,
}

#[derive(Deserialize)]
struct ViewArgs {
    photo: PhotoId,
    view: View,
}

#[derive(Deserialize)]
struct TicketArgs {
    ticket: RenderTicket,
}

/// The folder a file dialog opens in: the open photo's, when there is one.
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct PickArgs {
    #[serde(default)]
    from: Option<String>,
}

/// `black_white_auto`'s answer: the B&W mix's bands as the edit names them,
/// -100..100. `unknown` to the interface: it is a piece of the edit, whose band
/// names the interface defines (`BlackWhite.tsx`), and the edit is untyped on
/// both sides.
#[derive(Serialize)]
#[serde(transparent)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct AutoMix(#[cfg_attr(test, ts(type = "unknown"))] serde_json::Map<String, serde_json::Value>);

/// What a refused call answers: for the engine's calls, the engine's own
/// error as the contract writes it (`{ kind, message }`); for the rest, a
/// message, as before.
#[derive(Serialize)]
#[serde(untagged)]
pub enum Refusal {
    Engine(EngineError),
    Message(String),
}

impl From<String> for Refusal {
    fn from(message: String) -> Self {
        Refusal::Message(message)
    }
}

impl From<argentum_engine::Error> for Refusal {
    fn from(e: argentum_engine::Error) -> Self {
        Refusal::Engine(e.to_contract())
    }
}

/// A fault on this side of an engine call - arguments that do not parse, a
/// task that did not finish - in the engine's own terms.
fn internal(message: impl ToString) -> Refusal {
    Refusal::Engine(EngineError {
        kind: ErrorKind::Internal,
        message: message.to_string(),
    })
}

/// Every Argentum command, behind one registration.
///
/// Answers JSON rather than a typed value because the arms return different
/// shapes; the wrapper on the frontend restores the type at the call site.
/// A frame's pixels are the one answer that is not JSON.
#[tauri::command]
pub async fn ag(
    name: String,
    args: serde_json::Value,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Response, Refusal> {
    let value = match name.as_str() {
        "engine_frame" => {
            let a: TicketArgs = args_for(&name, args).map_err(internal)?;
            let frame = bridge(&app_handle)?.frame(a.ticket);
            return Ok(engine_frame::deliver(&window, frame).await.into_response());
        }
        engine if engine.starts_with("engine_") => engine_call(engine, args, &app_handle).await?,
        _ => json(name, args, state, app_handle, window).await?,
    };
    Ok(Response::new(value.to_string()))
}

/// The new interface's engine (engine_bridge.rs). Each answer is the engine's
/// own and each refusal its EngineError, passed through as the contract
/// writes them.
async fn engine_call(
    name: &str,
    args: serde_json::Value,
    app_handle: &tauri::AppHandle,
) -> Result<serde_json::Value, Refusal> {
    let photos = bridge(app_handle)?;
    let engine = photos.engine();
    let value = match name {
        "engine_sections" => serde_json::to_value(engine.sections()),
        "engine_tools" => serde_json::to_value(engine.tools()),
        "engine_open" => {
            let a: PathArgs = args_for(name, args).map_err(internal)?;
            let path = std::path::PathBuf::from(a.path);
            let opened = tokio::task::spawn_blocking(move || photos.open(&path))
                .await
                .map_err(internal)??;
            serde_json::to_value(opened)
        }
        "engine_apply" => {
            let a: ApplyArgs = args_for(name, args).map_err(internal)?;
            serde_json::to_value(engine.apply(a.photo, &a.patch)?)
        }
        "engine_view" => {
            let a: ViewArgs = args_for(name, args).map_err(internal)?;
            serde_json::to_value(engine.view(a.photo, a.view)?)
        }
        other => return Err(internal(format!("unknown Argentum command: {other}"))),
    };
    value.map_err(internal)
}

/// Every JSON command, named once: its name, what it takes, what it answers
/// and what it does. The match in `json` and the interface's types
/// (`src/argentum/ag.generated.ts`, written by the test in
/// `mods/ag_contract.rs`) are both made from this list, so neither can name a
/// command the other does not.
///
/// An entry is `"name" => (a: Args) -> Answer { ... }`, or `()` for a command
/// that takes nothing. The block is the answer; `?` refuses with a message.
/// `state`, `app_handle` and `window` are in scope.
macro_rules! commands {
    (@visit $v:ident $name:literal () $Out:ty) => {
        $v.command_without_args::<$Out>($name)
    };
    (@visit $v:ident $name:literal ($Args:ty) $Out:ty) => {
        $v.command::<$Args, $Out>($name)
    };
    (
        $state:ident, $app:ident, $window:ident;
        $( $name:literal => ( $($a:ident : $Args:ty)? ) -> $Out:ty $body:block )*
    ) => {
        // A command that answers nothing runs its block for what it does:
        // `()` is the answer, sent as null.
        #[allow(clippy::unit_arg)]
        async fn json(
            name: String,
            args: serde_json::Value,
            $state: tauri::State<'_, AppState>,
            $app: tauri::AppHandle,
            $window: tauri::WebviewWindow,
        ) -> Result<serde_json::Value, String> {
            match name.as_str() {
                $( $name => {
                    $( let $a: $Args = args_for(&name, args)?; )?
                    serde_json::to_value::<$Out>($body).map_err(|e| e.to_string())
                } )*
                other => Err(format!("unknown Argentum command: {other}")),
            }
        }

        /// Each command's name, arguments and answer, for the interface's types.
        #[cfg(test)]
        pub(crate) fn visit_commands(v: &mut crate::mods::ag_contract::Commands) {
            $( commands!(@visit v $name ($($Args)?) $Out); )*
        }
    };
}

commands! {
    state, app_handle, window;

    "selection_models_status" => () -> crate::mods::model_download::Status {
        let dir = crate::mods::selection_models::dir(&app_handle)?;
        crate::mods::selection_models::status(&dir)
    }
    "download_selection_models" => () -> () {
        use tauri::Emitter;
        let dir = crate::mods::selection_models::dir(&app_handle)?;
        let app = app_handle.clone();
        crate::mods::selection_models::download(&dir, move |progress| {
            let _ = app.emit(crate::mods::model_download::EVENT, progress);
        })
        .await?;
    }
    "selection_mask" => (a: SelectionArgs) -> crate::mods::selection::Selection {
        let dir = crate::mods::selection_models::dir(&app_handle)?;
        let picture = crate::get_cached_full_warped_image(&state, &a.js_adjustments)?;
        let path = state
            .original_image
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
            .map(|loaded| loaded.path.clone())
            .ok_or("No image loaded")?;
        let key = crate::mods::selection::picture_key(&path, &a.js_adjustments);
        tokio::task::spawn_blocking(move || {
            crate::mods::selection::select(&dir, &picture, key, a.kind, a.frame)
        })
        .await
        .map_err(|e| e.to_string())??
    }
    "selection_object" => (a: ObjectArgs) -> crate::mods::selection::Selection {
        let dir = crate::mods::selection_models::dir(&app_handle)?;
        let picture = crate::get_cached_full_warped_image(&state, &a.js_adjustments)?;
        let path = state
            .original_image
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
            .map(|loaded| loaded.path.clone())
            .ok_or("No image loaded")?;
        let key = crate::mods::selection::picture_key(&path, &a.js_adjustments);
        tokio::task::spawn_blocking(move || {
            crate::mods::selection::select_object(
                &dir, &picture, key, &a.clicks, a.bounds, a.frame,
            )
        })
        .await
        .map_err(|e| e.to_string())??
    }
    "text_mask_models_status" => () -> crate::mods::model_download::Status {
        let dir = crate::mods::model_manager::models_dir(&app_handle)?;
        crate::mods::text_mask::status(&dir)
    }
    "download_text_mask_models" => () -> () {
        use tauri::Emitter;
        let dir = crate::mods::model_manager::models_dir(&app_handle)?;
        let app = app_handle.clone();
        crate::mods::text_mask::download(&dir, move |progress| {
            let _ = app.emit(crate::mods::model_download::EVENT, progress);
        })
        .await?;
    }
    "text_mask" => (a: TextMaskArgs) -> crate::mods::selection::Selection {
        let dir = crate::mods::model_manager::models_dir(&app_handle)?;
        let picture = crate::get_cached_full_warped_image(&state, &a.js_adjustments)?;
        let path = state
            .original_image
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
            .map(|loaded| loaded.path.clone())
            .ok_or("No image loaded")?;
        let key = crate::mods::selection::picture_key(&path, &a.js_adjustments);
        tokio::task::spawn_blocking(move || {
            crate::mods::text_mask::select(&dir, &picture, key, &a.words, a.frame)
        })
        .await
        .map_err(|e| e.to_string())??
    }
    "face_mask_models_status" => () -> crate::mods::model_download::Status {
        let dir = crate::mods::model_manager::models_dir(&app_handle)?;
        crate::mods::face_mask::status(&dir)
    }
    "download_face_mask_models" => () -> () {
        use tauri::Emitter;
        let dir = crate::mods::model_manager::models_dir(&app_handle)?;
        let app = app_handle.clone();
        crate::mods::face_mask::download(&dir, move |progress| {
            let _ = app.emit(crate::mods::model_download::EVENT, progress);
        })
        .await?;
    }
    "face_mask" => (a: FaceMaskArgs) -> crate::mods::selection::Selection {
        let dir = crate::mods::model_manager::models_dir(&app_handle)?;
        let picture = crate::get_cached_full_warped_image(&state, &a.js_adjustments)?;
        let path = state
            .original_image
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
            .map(|loaded| loaded.path.clone())
            .ok_or("No image loaded")?;
        let key = crate::mods::selection::picture_key(&path, &a.js_adjustments);
        tokio::task::spawn_blocking(move || {
            crate::mods::face_mask::select(&dir, &picture, key, &a.parts, a.frame)
        })
        .await
        .map_err(|e| e.to_string())??
    }
    "detect_auto_white_balance" => (a: AutoWbArgs) -> crate::mods::auto_wb::AutoWhiteBalance {
        commands::detect_auto_white_balance(a.js_adjustments, a.mode, state).await?
    }
    "black_white_auto" => (a: OpenPhotoArgs) -> AutoMix {
        let loaded = the_open_photo(&state, &a.path)?;
        let mix = tokio::task::spawn_blocking(move || {
            crate::mods::black_white::auto(&loaded, &a.js_adjustments)
        })
        .await
        .map_err(|e| e.to_string())?;
        AutoMix(mix)
    }
    "measure_auto_white_balance" => (a: MeasureWbArgs) -> crate::mods::auto_wb_photo::Measured {
        let handle = app_handle.clone();
        tokio::task::spawn_blocking(move || {
            let settings = crate::app_settings::load_settings(handle).unwrap_or_default();
            crate::mods::auto_wb_photo::measure(
                &a.path,
                a.from.as_deref(),
                &settings,
                crate::mods::auto_wb_photo::PASTE_WAIT,
            )
        })
        .await
        .map_err(|e| e.to_string())??
    }
    "sample_processed_pixel" => (a: PointArgs) -> [u8; 3] {
        commands::sample_processed_pixel(a.x, a.y, a.js_adjustments, state, app_handle)
            .await?
    }
    "refresh_image_metadata" => (a: PathArgs) -> std::collections::HashMap<String, String> {
        commands::refresh_image_metadata(a.path, app_handle)?
    }
    "recover_lens_name" => (a: PathArgs) -> Option<String> {
        crate::mods::lens_name::recover_lens_name(a.path).await?
    }
    "lens_builtin" => (a: PathArgs) -> crate::mods::lens_builtin::Found {
        // The camera's own lens correction, when the photo opens.
        tokio::task::spawn_blocking(move || {
            crate::mods::lens_builtin::lens_builtin(&a.path)
        })
        .await
        .map_err(|e| e.to_string())??
    }
    "raw_tone_curve" => (a: RawToneArgs) -> Vec<crate::mods::raw_tone::ToneCurvePoint> {
        commands::raw_tone_curve(a.path, a.mode, app_handle)?
    }
    "dng_standard_held_back" => (a: PathArgs) -> Option<crate::mods::highlight_protection::HeldBack> {
        // What a highlight-protecting camera mode held back, for the note
        // under Rendering.
        crate::mods::dng_standard::held_back_of(&a.path)?
    }
    "sharpen_auto" => (a: PathArgs) -> crate::mods::sharpen::AutoValues {
        // What auto picked for this photo, for the Sharpening card to show.
        crate::mods::sharpen::auto_values(&a.path)
    }
    "camera_profile_status" => (a: PathArgs) -> crate::mods::profiles::Status {
        crate::mods::profiles::status_for(crate::mods::profiles::library_ready()?, std::path::Path::new(&a.path))
    }
    "import_camera_profile" => (a: PathArgs) -> crate::mods::profiles::Installed {
        commands::import_camera_profile(a.path)?
    }
    "remove_camera_profile" => (a: FileArgs) -> () {
        crate::mods::profiles::remove(crate::mods::profiles::library_ready()?, &a.file)?;
    }
    "list_cameras" => () -> Vec<crate::mods::profiles::Camera> {
        crate::mods::profiles::cameras(crate::mods::profiles::library_ready()?)
    }
    "forget_camera" => (a: ModelArgs) -> () {
        crate::mods::profiles::forget_camera(crate::mods::profiles::library_ready()?, &a.model);
    }
    "set_camera_default_profile" => (a: DefaultProfileArgs) -> () {
        crate::mods::profiles::set_default_profile(crate::mods::profiles::library_ready()?, &a.model, a.file);
    }
    "camera_default_profile" => (a: ModelArgs) -> Option<String> {
        crate::mods::profiles::default_profile(crate::mods::profiles::library_ready()?, &a.model)
    }
    "get_profile_online" => (a: ModelArgs) -> Option<String> {
        commands::get_profile_online(a.make, a.model).await?
    }
    "highlight_recovery" => () -> bool {
        crate::mods::highlights::enabled()
    }
    "set_highlight_recovery" => (a: OnArgs) -> () {
        crate::mods::highlights::save(crate::mods::profiles::library_ready()?, a.on)?;
    }
    "raw_decoder" => () -> crate::mods::raw_decoder::RawDecoder {
        crate::mods::raw_decoder::default_decoder()
    }
    "set_raw_decoder" => (a: DecoderArgs) -> () {
        crate::mods::raw_decoder::save(crate::mods::profiles::library_ready()?, a.decoder)?;
    }
    "set_photo_raw_decoder" => (a: PhotoDecoderArgs) -> () {
        crate::mods::raw_decoder::remember(&a.path, a.decoder);
    }
    "flashraw_handles" => (a: PathArgs) -> bool {
        tokio::task::spawn_blocking(move || {
            crate::mods::raw_decoder::flashraw_handles(&a.path)
        })
        .await
        .map_err(|e| e.to_string())??
    }
    "redecode_open_photo" => () -> bool {
        crate::mods::redecode::open_photo(state, app_handle).await?
    }
    "tiff_bit_depth" => () -> u8 {
        crate::mods::export_precision::tiff_depth().as_u8()
    }
    "set_tiff_bit_depth" => (a: DepthArgs) -> () {
        crate::mods::export_precision::save(
            crate::mods::profiles::library_ready()?,
            crate::mods::export_precision::TiffDepth::from_u8(a.depth),
        )?;
    }
    "compact_slider_gap" => () -> u8 {
        crate::mods::editor_layout::compact_slider_gap(crate::mods::profiles::library_ready()?)
    }
    "set_compact_slider_gap" => (a: GapArgs) -> () {
        crate::mods::editor_layout::save_compact_slider_gap(crate::mods::profiles::library_ready()?, a.gap)?;
    }
    "editor_layout_panels" => () -> Option<crate::mods::editor_layout::LayoutPanels> {
        crate::mods::editor_layout::panels(crate::mods::profiles::library_ready()?)
    }
    "set_editor_layout_panels" => (a: LayoutPanelsArgs) -> () {
        crate::mods::editor_layout::save_panels(crate::mods::profiles::library_ready()?, &a.panels)?;
    }
    "modern_theme" => () -> crate::mods::editor_layout::ModernTheme {
        crate::mods::editor_layout::theme(crate::mods::profiles::library_ready()?)
    }
    "set_modern_theme" => (a: ThemeArgs) -> () {
        crate::mods::editor_layout::save_theme(
            crate::mods::profiles::library_ready()?,
            crate::mods::editor_layout::ModernTheme::from_name(&a.theme),
        )?;
    }
    "text_size" => () -> crate::mods::editor_layout::TextSize {
        crate::mods::editor_layout::text_size(crate::mods::profiles::library_ready()?)
    }
    "builtin_presets" => () -> Vec<crate::mods::looks::PresetGroup> {
        let handle = app_handle.clone();
        tokio::task::spawn_blocking(move || commands::builtin_presets(&handle))
            .await
            .map_err(|e| e.to_string())?
    }
    "snapshots" => (a: PathArgs) -> Vec<crate::mods::snapshots::Snapshot> {
        tokio::task::spawn_blocking(move || {
            let (_, sidecar) = crate::file_management::parse_virtual_path(&a.path);
            crate::mods::snapshots::read(&sidecar)
        })
            .await
            .map_err(|e| e.to_string())?
    }
    "set_snapshots" => (a: SnapshotsArgs) -> () {
        tokio::task::spawn_blocking(move || {
            let (_, sidecar) = crate::file_management::parse_virtual_path(&a.path);
            crate::mods::snapshots::write(&sidecar, a.snapshots)
        })
            .await
            .map_err(|e| e.to_string())??;
    }
    "history" => (a: PathArgs) -> Option<crate::mods::history::HistoryRecord> {
        tokio::task::spawn_blocking(move || {
            let (_, sidecar) = crate::file_management::parse_virtual_path(&a.path);
            crate::mods::history::read(&sidecar)
        })
            .await
            .map_err(|e| e.to_string())?
    }
    "set_history" => (a: HistoryArgs) -> () {
        tokio::task::spawn_blocking(move || {
            let (source, sidecar) = crate::file_management::parse_virtual_path(&a.path);
            crate::mods::history::write(&source, &sidecar, a.record)
        })
            .await
            .map_err(|e| e.to_string())??;
    }
    "looks_favourites" => () -> Vec<String> {
        crate::mods::looks::favourites(crate::mods::profiles::library_ready()?)
    }
    "set_looks_favourites" => (a: IdsArgs) -> () {
        crate::mods::looks::save_favourites(crate::mods::profiles::library_ready()?, &a.ids)?;
    }
    "provider_looks" => (a: crate::mods::lut_provider::ProviderLooksArgs) -> Option<crate::mods::lut_provider::ProviderLooks> {
        let app = app_handle.clone();
        tauri::async_runtime::spawn_blocking(move || {
            crate::mods::lut_provider::looks(&app, &a)
        })
        .await
        .map_err(|e| e.to_string())?
    }
    "provider_look_make" => (a: crate::mods::lut_provider::ProviderLookMakeArgs) -> u64 {
        let app = app_handle.clone();
        tauri::async_runtime::spawn_blocking(move || crate::mods::lut_provider::make(&app, a))
            .await
            .map_err(|e| e.to_string())??
    }
    "provider_look_preview" => (a: crate::mods::lut_provider::ProviderLookPreviewArgs) -> u64 {
        let app = app_handle.clone();
        tauri::async_runtime::spawn_blocking(move || crate::mods::lut_provider::preview_all(&app, a))
            .await
            .map_err(|e| e.to_string())??
    }
    "provider_look_delete" => (a: crate::mods::lut_provider::ProviderLookDeleteArgs) -> crate::mods::lut_provider::LooksDeleted {
        let app = app_handle.clone();
        tauri::async_runtime::spawn_blocking(move || crate::mods::lut_provider::delete_looks(&app, &a))
            .await
            .map_err(|e| e.to_string())??
    }
    "provider_look_file" => (a: crate::mods::lut_provider::ProviderLookFileArgs) -> bool {
        crate::mods::lut_provider::file_is_there(&a)
    }
    "provider_look_cancel" => (a: crate::mods::lut_provider::ProviderLookCancelArgs) -> () {
        crate::mods::lut_provider::cancel(a.job_id);
    }
    "provider_looks_switch" => (a: crate::mods::lut_provider::ProviderLooksSwitchArgs) -> () {
        crate::mods::lut_provider::switch(a.on)?;
    }
    "look_suggestions" => (a: PathArgs) -> crate::mods::look_picks::Suggested {
        let loaded = the_open_photo(&state, &a.path)?;
        commands::look_suggestions(&a.path, &loaded)?
    }
    "look_pick" => (a: LookPickArgs) -> crate::mods::look_picks::Suggested {
        let loaded = the_open_photo(&state, &a.path)?;
        commands::look_pick(&a.path, &a.lut, &loaded)?
    }
    "look_unpick" => (a: PathArgs) -> crate::mods::look_picks::Suggested {
        let loaded = the_open_photo(&state, &a.path)?;
        commands::look_unpick(&a.path, &loaded)?
    }
    "set_look_learning" => (a: LearningArgs) -> () {
        crate::mods::look_picks::change(crate::mods::profiles::library_ready()?, |book| book.learning = a.on)?;
    }
    "forget_look_picks" => () -> () {
        crate::mods::look_picks::change(crate::mods::profiles::library_ready()?, |book| book.picks.clear())?;
    }
    "set_text_size" => (a: TextSizeArgs) -> () {
        crate::mods::editor_layout::save_text_size(
            crate::mods::profiles::library_ready()?,
            crate::mods::editor_layout::TextSize::from_name(&a.size),
        )?;
    }
    "modern_start" => (a: ModernStartArgs) -> crate::mods::modern_start::ModernStart {
        tokio::task::spawn_blocking(move || {
            commands::modern_start(a.last_folder.as_deref(), &a.expanded)
        })
        .await
        .map_err(|e| e.to_string())??
    }
    "folder_photo_count" => (a: PathArgs) -> u32 {
        tokio::task::spawn_blocking(move || {
            crate::mods::folder_counts::photo_count(std::path::Path::new(&a.path))
        })
        .await
        .map_err(|e| e.to_string())?
    }
    "add_recent_folder" => (a: PathArgs) -> Vec<String> {
        tokio::task::spawn_blocking(move || commands::add_recent_folder(&a.path))
            .await
            .map_err(|e| e.to_string())??
    }
    "rename_recent_folder" => (a: FolderRenameArgs) -> Vec<String> {
        tokio::task::spawn_blocking(move || {
            commands::rename_recent_folder(&a.old_path, &a.new_path)
        })
        .await
        .map_err(|e| e.to_string())??
    }
    "super_resolution_preview" => (a: SuperResolutionPreviewArgs) -> crate::mods::super_resolution::PreviewPayload {
        crate::mods::super_resolution::preview(a.path, a.scale, a.restore, app_handle)
            .await
            .map_err(|e| e.to_string())?
    }
    "save_super_resolution" => (a: SuperResolutionSaveArgs) -> String {
        crate::mods::super_resolution::save(a.path, app_handle).await?
    }
    "discard_super_resolution" => () -> () {
        crate::mods::super_resolution::discard();
    }
    "restore_mask" => (a: RestoreMaskArgs) -> String {
        crate::mods::super_resolution::restore_mask(a.path, a.mask, app_handle).await?
    }
    "restore_reblend" => (a: RestoreReblendArgs) -> String {
        crate::mods::super_resolution::restore_reblend(a.mask).await?
    }
    "batch_super_resolution" => (a: SuperResolutionBatchArgs) -> Vec<String> {
        crate::mods::super_resolution::batch(a.paths, a.scale, a.restore, app_handle)
            .await
            .map_err(|e| e.to_string())?
    }
    "profiles_for_camera" => (a: ModelArgs) -> commands::CameraProfiles {
        commands::profiles_for_camera(a.make, a.model)?
    }
    "list_ai_models" => () -> crate::mods::model_manager::Listing {
        crate::mods::model_manager::list_for_app(&app_handle)?
    }
    "download_ai_model" => (a: IdArgs) -> u64 {
        use tauri::Emitter;
        let dir = crate::mods::model_manager::models_dir(&app_handle)?;
        let app = app_handle.clone();
        crate::mods::model_download::download(&dir, &a.id, move |progress| {
            let _ = app.emit(crate::mods::model_download::EVENT, progress);
        })
        .await?
    }
    "stop_ai_model_download" => () -> () {
        crate::mods::model_download::stop();
    }
    "clear_ai_model_leftovers" => (a: NamesArgs) -> crate::mods::model_manager::Cleared {
        crate::mods::model_manager::clear_for_app(&app_handle, &a.names)?
    }
    "delete_ai_model" => (a: IdArgs) -> crate::mods::model_manager::Outcome {
        crate::mods::model_manager::delete_for_app(&app_handle, &a.id)?
    }
    "upgrade_white_balance" => (a: PathsArgs) -> Vec<crate::mods::wb_legacy::Found> {
        // The photos among these whose saved edit predates RapidRAW 1.6.5,
        // converted. Those converted by this call are saved one at a time
        // in the background; see wb_legacy::save_in_turn.
        let (found, fresh) =
            tokio::task::spawn_blocking(move || crate::mods::wb_legacy::find_old(&a.paths))
                .await
                .map_err(|e| e.to_string())?;
        crate::mods::wb_legacy::save_in_turn(fresh, app_handle);
        found
    }
    // The new interface, inside the app: see shell_photos.rs.
    "shell_show_window" => () -> () {
        // The window is built hidden and today's interface shows it; the
        // new one has no window buttons of its own, so it gets the system's.
        window.show().map_err(|e| e.to_string())?;
        window.set_decorations(true).map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    "shell_launch_photo" => () -> Option<String> {
        shell_photos::launched_with(std::env::args().skip(1))
    }
    "shell_folder" => (a: PathArgs) -> Vec<String> {
        tokio::task::spawn_blocking(move || {
            shell_photos::folder(std::path::Path::new(&a.path))
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?
    }
    "shell_pick" => (a: PickArgs) -> Option<String> {
        shell_photos::pick(&app_handle, a.from).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frontend sends camelCase; the structs must accept it unchanged.
    #[test]
    fn point_arguments_deserialise_as_the_frontend_sends_them() {
        let v = serde_json::json!({ "x": 0.25, "y": 0.75, "jsAdjustments": { "exposure": 1 } });
        let a: PointArgs = args_for("test", v).expect("should parse");
        assert_eq!(a.x, 0.25);
        assert_eq!(a.y, 0.75);
        assert_eq!(a.js_adjustments["exposure"], 1);
    }

    #[test]
    fn auto_wb_arguments_carry_the_mode() {
        let v = serde_json::json!({ "jsAdjustments": {}, "mode": "surfaces" });
        let a: AutoWbArgs = args_for("test", v).expect("should parse");
        assert_eq!(a.mode, DetectMode::Surfaces);
    }

    /// The engine's calls with their arguments as the new interface's client
    /// sends them (src/shell/engine/tauri.ts, whose test checks the same
    /// shapes), its values from the contract fixtures.
    #[test]
    fn the_shells_calls_deserialise_as_its_client_sends_them() {
        let f: serde_json::Value =
            serde_json::from_str(include_str!("../../../src/shell/contract.fixtures.json"))
                .unwrap();
        let photo = f["opened"]["photoId"].clone();
        let path = serde_json::json!({ "path": "C:/Photos/IMG_0001.CR3" });

        let a: ApplyArgs = args_for(
            "engine_apply",
            serde_json::json!({ "photo": photo, "patch": f["patch"] }),
        )
        .unwrap();
        assert_eq!(serde_json::to_value(a.photo).unwrap(), photo);
        assert_eq!(serde_json::to_value(&a.patch).unwrap(), f["patch"]);

        let v: ViewArgs = args_for(
            "engine_view",
            serde_json::json!({ "photo": photo, "view": f["view"] }),
        )
        .unwrap();
        assert_eq!(serde_json::to_value(v.view).unwrap(), f["view"]);

        let t: TicketArgs = args_for(
            "engine_frame",
            serde_json::json!({ "ticket": f["events"][0]["ticket"] }),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(t.ticket).unwrap(),
            f["events"][0]["ticket"]
        );

        args_for::<PathArgs>("engine_open", path.clone()).unwrap();
        args_for::<PathArgs>("shell_folder", path).unwrap();
        let from: PickArgs = args_for(
            "shell_pick",
            serde_json::json!({ "from": "C:/Photos/IMG_0001.CR3" }),
        )
        .unwrap();
        assert!(from.from.is_some());
        let none: PickArgs = args_for("shell_pick", serde_json::json!({})).unwrap();
        assert!(none.from.is_none());
    }

    /// Today's commands still answer a refusal with a bare message.
    #[test]
    fn a_message_refusal_is_a_plain_string() {
        let r = Refusal::from("no such photo".to_string());
        assert_eq!(
            serde_json::to_value(r).unwrap(),
            serde_json::json!("no such photo")
        );
        let r = internal("engine_view: bad arguments");
        assert_eq!(
            serde_json::to_value(r).unwrap(),
            serde_json::json!({ "kind": "internal", "message": "engine_view: bad arguments" })
        );
    }

    /// The engine's refusals reach the interface exactly as the contract
    /// fixtures write them: what the fake refuses, the app refuses alike.
    #[test]
    fn engine_refusals_answer_as_the_contract_fixtures() {
        let Some(path) = std::env::var("AG_RAW")
            .ok()
            .filter(|p| !p.trim().is_empty())
        else {
            eprintln!("engine_refusals_answer_as_the_contract_fixtures: SKIPPED - set AG_RAW");
            return;
        };
        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../../../src/shell/contract.fixtures.json"))
                .unwrap();
        let engine = argentum_engine::Engine::new(|_| {}).unwrap();
        let photo = engine.open(&path).unwrap().photo_id;
        let refused = fixtures["refused"].as_array().unwrap();
        assert!(!refused.is_empty());
        for case in refused {
            let patch: EditPatch = serde_json::from_value(case["patch"].clone()).unwrap();
            let refusal = Refusal::from(engine.apply(photo, &patch).unwrap_err());
            assert_eq!(serde_json::to_value(refusal).unwrap(), case["error"]);
        }
    }

    /// A malformed bag names the command, so the error says which call broke.
    #[test]
    fn bad_arguments_name_the_command() {
        let v = serde_json::json!({ "x": "not a number" });
        let e = args_for::<PointArgs>("sample_processed_pixel", v)
            .err()
            .expect("bad arguments must not parse");
        assert!(
            e.starts_with("sample_processed_pixel:"),
            "unhelpful error: {e}"
        );
    }
}
