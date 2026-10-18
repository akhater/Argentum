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
#[serde(rename_all = "camelCase")]
struct PointArgs {
    x: f32,
    y: f32,
    js_adjustments: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutoWbArgs {
    js_adjustments: serde_json::Value,
    mode: DetectMode,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PathArgs {
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PathsArgs {
    paths: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawToneArgs {
    path: String,
    mode: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileArgs {
    file: String,
}

#[derive(Deserialize)]
struct OnArgs {
    on: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SuperResolutionPreviewArgs {
    path: String,
    scale: u32,
    /// Restore's Mask slider, when Restore is on.
    #[serde(default)]
    restore: Option<f32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SuperResolutionSaveArgs {
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RestoreMaskArgs {
    path: String,
    mask: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RestoreReblendArgs {
    mask: f32,
}

#[derive(Deserialize)]
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
struct DepthArgs {
    depth: u8,
}

/// Space under a compact slider row, in pixels.
#[derive(Deserialize)]
struct GapArgs {
    gap: u8,
}

/// A catalogue id from `mods::model_catalog`, never a path.
#[derive(Deserialize)]
struct IdArgs {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelArgs {
    /// Absent for commands that only identify a camera by model.
    #[serde(default)]
    make: String,
    model: String,
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
struct PickArgs {
    #[serde(default)]
    from: Option<String>,
}

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

async fn json(
    name: String,
    args: serde_json::Value,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<serde_json::Value, String> {
    match name.as_str() {
        "detect_auto_white_balance" => {
            let a: AutoWbArgs = args_for(&name, args)?;
            let r = commands::detect_auto_white_balance(a.js_adjustments, a.mode, state).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        }
        "sample_processed_pixel" => {
            let a: PointArgs = args_for(&name, args)?;
            let r = commands::sample_processed_pixel(a.x, a.y, a.js_adjustments, state, app_handle)
                .await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        }
        "refresh_image_metadata" => {
            let a: PathArgs = args_for(&name, args)?;
            let r = commands::refresh_image_metadata(a.path, app_handle)?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        }
        "recover_lens_name" => {
            let a: PathArgs = args_for(&name, args)?;
            let r = crate::mods::lens_name::recover_lens_name(a.path).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        }
        "raw_tone_curve" => {
            let a: RawToneArgs = args_for(&name, args)?;
            serde_json::to_value(commands::raw_tone_curve(a.path, a.mode, app_handle)?)
                .map_err(|e| e.to_string())
        }
        "sharpen_auto" => {
            // What auto picked for this photo, for the Sharpening card to show.
            let a: PathArgs = args_for(&name, args)?;
            Ok(crate::mods::sharpen::auto_values(&a.path))
        }
        "camera_profile_status" => {
            let a: PathArgs = args_for(&name, args)?;
            serde_json::to_value(commands::camera_profile_status(a.path)?)
                .map_err(|e| e.to_string())
        }
        "import_camera_profile" => {
            let a: PathArgs = args_for(&name, args)?;
            serde_json::to_value(commands::import_camera_profile(a.path)?)
                .map_err(|e| e.to_string())
        }
        "list_camera_profiles" => {
            serde_json::to_value(commands::list_camera_profiles()?).map_err(|e| e.to_string())
        }
        "remove_camera_profile" => {
            let a: FileArgs = args_for(&name, args)?;
            commands::remove_camera_profile(a.file)?;
            Ok(serde_json::Value::Null)
        }
        "list_cameras" => {
            serde_json::to_value(commands::list_cameras()?).map_err(|e| e.to_string())
        }
        "forget_camera" => {
            let a: ModelArgs = args_for(&name, args)?;
            commands::forget_camera(a.model)?;
            Ok(serde_json::Value::Null)
        }
        "get_profile_online" => {
            let a: ModelArgs = args_for(&name, args)?;
            serde_json::to_value(commands::get_profile_online(a.make, a.model).await?)
                .map_err(|e| e.to_string())
        }
        "highlight_recovery" => {
            serde_json::to_value(commands::highlight_recovery()?).map_err(|e| e.to_string())
        }
        "set_highlight_recovery" => {
            let a: OnArgs = args_for(&name, args)?;
            commands::set_highlight_recovery(a.on)?;
            Ok(serde_json::Value::Null)
        }
        "redecode_open_photo" => {
            let r = crate::mods::redecode::open_photo(state, app_handle).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        }
        "tiff_bit_depth" => {
            serde_json::to_value(commands::tiff_bit_depth()).map_err(|e| e.to_string())
        }
        "set_tiff_bit_depth" => {
            let a: DepthArgs = args_for(&name, args)?;
            commands::set_tiff_bit_depth(a.depth)?;
            Ok(serde_json::Value::Null)
        }
        "compact_sliders" => {
            serde_json::to_value(commands::compact_sliders()?).map_err(|e| e.to_string())
        }
        "set_compact_sliders" => {
            let a: OnArgs = args_for(&name, args)?;
            commands::set_compact_sliders(a.on)?;
            Ok(serde_json::Value::Null)
        }
        "compact_slider_gap" => {
            serde_json::to_value(commands::compact_slider_gap()?).map_err(|e| e.to_string())
        }
        "set_compact_slider_gap" => {
            let a: GapArgs = args_for(&name, args)?;
            commands::set_compact_slider_gap(a.gap)?;
            Ok(serde_json::Value::Null)
        }
        "super_resolution_preview" => {
            let a: SuperResolutionPreviewArgs = args_for(&name, args)?;
            serde_json::to_value(
                crate::mods::super_resolution::preview(a.path, a.scale, a.restore, app_handle)
                    .await
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())
        }
        "save_super_resolution" => {
            let a: SuperResolutionSaveArgs = args_for(&name, args)?;
            serde_json::to_value(crate::mods::super_resolution::save(a.path, app_handle).await?)
                .map_err(|e| e.to_string())
        }
        "discard_super_resolution" => {
            crate::mods::super_resolution::discard();
            Ok(serde_json::Value::Null)
        }
        "restore_mask" => {
            let a: RestoreMaskArgs = args_for(&name, args)?;
            serde_json::to_value(
                crate::mods::super_resolution::restore_mask(a.path, a.mask, app_handle).await?,
            )
            .map_err(|e| e.to_string())
        }
        "restore_reblend" => {
            let a: RestoreReblendArgs = args_for(&name, args)?;
            serde_json::to_value(crate::mods::super_resolution::restore_reblend(a.mask).await?)
                .map_err(|e| e.to_string())
        }
        "batch_super_resolution" => {
            let a: SuperResolutionBatchArgs = args_for(&name, args)?;
            serde_json::to_value(
                crate::mods::super_resolution::batch(a.paths, a.scale, a.restore, app_handle)
                    .await
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())
        }
        "profiles_for_camera" => {
            let a: ModelArgs = args_for(&name, args)?;
            serde_json::to_value(commands::profiles_for_camera(a.make, a.model)?)
                .map_err(|e| e.to_string())
        }
        "list_ai_models" => {
            let dir = crate::mods::model_manager::models_dir(&app_handle)?;
            serde_json::to_value(crate::mods::model_manager::list(&dir)).map_err(|e| e.to_string())
        }
        "delete_ai_model" => {
            let a: IdArgs = args_for(&name, args)?;
            let r = crate::mods::model_manager::delete_for_app(&app_handle, &a.id)?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        }
        "upgrade_white_balance" => {
            // The photos among these whose saved edit predates RapidRAW 1.6.5,
            // converted. Those converted by this call are saved one at a time
            // in the background; see wb_legacy::save_in_turn.
            let a: PathsArgs = args_for(&name, args)?;
            let (found, fresh) =
                tokio::task::spawn_blocking(move || crate::mods::wb_legacy::find_old(&a.paths))
                    .await
                    .map_err(|e| e.to_string())?;
            crate::mods::wb_legacy::save_in_turn(fresh, app_handle);
            serde_json::to_value(found).map_err(|e| e.to_string())
        }
        // The new interface, inside the app: see shell_photos.rs.
        "shell_ready" => {
            // The window is built hidden and today's interface shows it; the
            // new one has no window buttons of its own, so it gets the system's.
            window.show().map_err(|e| e.to_string())?;
            let _ = window.set_decorations(true);
            let _ = window.set_focus();
            serde_json::to_value(shell_photos::launched_with(std::env::args().skip(1)))
                .map_err(|e| e.to_string())
        }
        "shell_folder" => {
            let a: PathArgs = args_for(&name, args)?;
            let r = tokio::task::spawn_blocking(move || {
                shell_photos::folder(std::path::Path::new(&a.path))
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        }
        "shell_pick" => {
            let a: PickArgs = args_for(&name, args)?;
            serde_json::to_value(shell_photos::pick(&app_handle, a.from).await)
                .map_err(|e| e.to_string())
        }
        other => Err(format!("unknown Argentum command: {other}")),
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

    /// Every call the new interface makes, with the arguments its client sends
    /// (src/shell/engine/wire.fixtures.json, whose own test checks the client
    /// sends exactly these).
    #[test]
    fn the_shells_calls_deserialise_as_its_client_sends_them() {
        let wire: serde_json::Value =
            serde_json::from_str(include_str!("../../../src/shell/engine/wire.fixtures.json"))
                .unwrap();
        let calls = wire["calls"].as_object().unwrap();
        assert!(calls.len() >= 8);
        for (name, args) in calls {
            let args = args.clone();
            match name.as_str() {
                "engine_sections" | "engine_tools" | "shell_ready" => {
                    assert_eq!(args, serde_json::json!({}))
                }
                "engine_open" | "shell_folder" => {
                    args_for::<PathArgs>(name, args).unwrap();
                }
                "engine_apply" => {
                    let a: ApplyArgs = args_for(name, args).unwrap();
                    assert_eq!(a.photo, PhotoId(1));
                    // null puts a setting back to its default.
                    assert!(
                        a.patch
                            .0
                            .values()
                            .flat_map(|t| t.values())
                            .any(|v| v.is_null())
                    );
                }
                "engine_view" => {
                    let a: ViewArgs = args_for(name, args).unwrap();
                    assert!(a.view.zoom > 0.0 && a.view.width > 0);
                }
                "engine_frame" => {
                    args_for::<TicketArgs>(name, args).unwrap();
                }
                "shell_pick" => {
                    assert!(args_for::<PickArgs>(name, args).unwrap().from.is_some());
                    let none: PickArgs = args_for(name, serde_json::json!({})).unwrap();
                    assert!(none.from.is_none());
                }
                other => panic!("{other}: the dispatcher has no arguments for it"),
            }
        }
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
