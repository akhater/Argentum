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

use crate::app_state::AppState;
use crate::mods::auto_wb::DetectMode;
use crate::mods::commands;
use serde::Deserialize;

/// Pull one command's arguments out of the bag, or say which command failed.
fn args_for<T: for<'de> Deserialize<'de>>(name: &str, args: serde_json::Value) -> Result<T, String> {
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
struct FileArgs {
    file: String,
}

#[derive(Deserialize)]
struct OnArgs {
    on: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelArgs {
    /// Absent for commands that only identify a camera by model.
    #[serde(default)]
    make: String,
    model: String,
}

/// Every Argentum command, behind one registration.
///
/// Returns JSON rather than a typed value because the arms return different
/// shapes; the wrapper on the frontend restores the type at the call site.
#[tauri::command]
pub async fn ag(
    name: String,
    args: serde_json::Value,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    match name.as_str() {
        "solve_white_balance_at_point" => {
            let a: PointArgs = args_for(&name, args)?;
            let r = commands::solve_white_balance_at_point(a.x, a.y, a.js_adjustments, state).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        }
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
        "profiles_for_camera" => {
            let a: ModelArgs = args_for(&name, args)?;
            serde_json::to_value(commands::profiles_for_camera(a.make, a.model)?)
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

    /// A malformed bag names the command, so the error says which call broke.
    #[test]
    fn bad_arguments_name_the_command() {
        let v = serde_json::json!({ "x": "not a number" });
        let e = args_for::<PointArgs>("sample_processed_pixel", v)
            .err()
            .expect("bad arguments must not parse");
        assert!(e.starts_with("sample_processed_pixel:"), "unhelpful error: {e}");
    }
}
