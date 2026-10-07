//! White balance saved before RapidRAW 1.6.5, read in 1.6.5's units.
//!
//! WHAT CHANGED
//!
//! From 26.37.2 until the 1.6.5 merge, Temperature and Tint ran through our own
//! `dt_white_balance` in `modules.wgsl`. Each slider named an illuminant: the
//! temperature, divided by 25, went through `6500 * exp(t * 0.28)` onto the
//! daylight locus, the tint (divided by 100) moved its `y` by `n * 0.05`, and
//! the picture was adapted from that illuminant to D65 in Bradford cone space.
//!
//! RapidRAW 1.6.5 shipped its own engine and we took it (see `modules.wgsl`).
//! Its sliders mean something else: 1.5 mired and 1.5 tint units per step away
//! from the camera's as-shot white balance, with an optional absolute
//! `whiteBalance` in kelvin beside them. The same number now asks for a
//! different correction. Cooling by 50 was 115 mired and is now 75, and a tint
//! step is close to three times as strong. Left alone, every photo whose white
//! balance was touched would change colour on the first open after the update.
//!
//! WHICH EDITS ARE OLD
//!
//! Every adjustments object the app writes since 1.6.5 carries a `whiteBalance`
//! key, null when the sliders are relative (their `INITIAL_ADJUSTMENTS` has it).
//! An edit without the key was saved before the merge. That is the whole test,
//! and once a converted edit is saved with the key it is never converted again.
//!
//! WHERE, WITHOUT A LINE OF THEIRS
//!
//! The obvious places are a line at the top of their
//! `get_all_adjustments_from_json` and one in their `load_metadata`. The anchor
//! check refused both: those files have no allowance left, and an allowance
//! does not go up for a feature. So everything happens from our side:
//!
//! - **The editor.** `src/argentum/wbLegacy.ts` sees every `load_metadata`
//!   answer before their code does and, when the edit is old, swaps in
//!   `upgraded` and saves it through their `save_metadata_and_update_thumbnail`.
//!   Their editor never holds an old edit, so it can never save one back with
//!   the new key and the old numbers, which would fix the wrong colour for good.
//! - **Everything else.** When the library lists a folder, `find_old` reports
//!   the old edits in it, converted, and the frontend saves each the same way,
//!   which also redraws its thumbnail. Batch exports read the saved file.
//! - **Presets**, once, at start-up.
//!
//! Not covered: a headless export from the command line of a photo whose folder
//! has never been listed since the update.
//!
//! HOW
//!
//! Exactly, not by curve fitting. The old adaptation took the colour of its
//! illuminant to white, which is what auto white balance asks of their engine
//! too: `auto_wb::removing_illuminant` hands the illuminant to their
//! `pick_white_balance` and gets back the white balance that makes the same
//! correction, in their units.
//! The tests below check the gains agree.
//!
//! Relative when it fits on their sliders, absolute when it does not: a
//! relative edit keeps meaning "this much warmer than the camera" when it is
//! copied to another photo, which is what it meant before.
//!
//! WHAT THIS DOES NOT REPRODUCE
//!
//! The old shader decoded the RAW a second time before adapting and clipped at
//! white on the way back, so its correction was weaker than its own maths said
//! (about three quarters of it in the midtones) and every highlight above white
//! was lost whenever the sliders were off zero. That was a bug. This converts
//! what the sliders asked for, so an old edit comes back slightly stronger, and
//! with its highlights.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::white_balance::{self, MIRED_PER_RELATIVE_UNIT, TINT_PER_RELATIVE_UNIT, WhiteBalance};

/// What the old sliders were divided by before reaching the shader.
const OLD_SCALE_TEMPERATURE: f64 = 25.0;
const OLD_SCALE_TINT: f64 = 100.0;

/// Their sliders run -100..100 in relative mode.
const RELATIVE_RANGE: f64 = 100.0;

/// One photo's edit in 1.6.5's units: converted if it is old, as it was if not.
/// `path` is the photo, which the as-shot white balance is read from, and only
/// when there is something to convert.
pub fn upgraded(path: &str, mut adjustments: Value) -> Value {
    if is_old(&adjustments) {
        upgrade(&mut adjustments, white_balance::as_shot_white_balance(path));
    }
    adjustments
}

/// An old edit found on disk, and what it becomes.
#[derive(Debug, Clone, Serialize)]
pub struct Found {
    pub path: String,
    /// The whole edit, converted.
    pub adjustments: Value,
    /// The old white balance numbers, globally and per mask in order: how the
    /// frontend recognises the edit still open in the editor unconverted.
    pub was: Value,
}

/// Every edit converted this session, by path. The editor asks by path, and the
/// answer must not depend on whether the file has been saved yet: a photo
/// opened while its folder was being converted may have been loaded before.
fn converted() -> &'static Mutex<HashMap<String, Found>> {
    static CONVERTED: OnceLock<Mutex<HashMap<String, Found>>> = OnceLock::new();
    CONVERTED.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The photos among `paths` whose edit is old or was converted earlier this
/// session, each converted, and separately the ones converted by this call,
/// which still need saving. Writes nothing; `save_in_turn` does that.
pub fn find_old(paths: &[String]) -> (Vec<Found>, Vec<Found>) {
    let mut all = Vec::new();
    let mut new = Vec::new();
    for path in paths {
        if let Some(found) = converted().lock().unwrap().get(path) {
            all.push(found.clone());
            continue;
        }
        let (_, sidecar) = crate::file_management::parse_virtual_path(path);
        let saved = crate::exif_processing::load_sidecar(&sidecar).adjustments;
        if !is_old(&saved) {
            continue;
        }
        let found = Found {
            path: path.clone(),
            was: white_balance_numbers(&saved),
            adjustments: upgraded(path, saved),
        };
        converted()
            .lock()
            .unwrap()
            .insert(path.clone(), found.clone());
        new.push(found.clone());
        all.push(found);
    }
    (all, new)
}

/// Save converted edits one at a time, through their own save command, which
/// keeps the rest of the sidecar and redraws the thumbnail.
///
/// One at a time is the point. Their command returns at once and redraws the
/// thumbnail on a thread of its own, with nothing limiting how many run, and
/// each one decodes the whole RAW. The first version of this sweep saved every
/// old edit in a folder at once; on AK's first start that was dozens of 24 MP
/// decodes together, and the preview worker died. So each save waits for its
/// thumbnail before the next starts.
///
/// Skipped: the photo open in the editor, which the editor saves itself once
/// it holds the converted values, and any file that is no longer old on disk,
/// which means the editor has saved it since and what it saved must stand.
pub fn save_in_turn(found: Vec<Found>, app: tauri::AppHandle) {
    if found.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        use tauri::{Listener, Manager};
        let (done_tx, done_rx) = std::sync::mpsc::channel::<String>();
        let listener = app.listen("thumbnail-generated", move |event| {
            if let Ok(payload) = serde_json::from_str::<Value>(event.payload())
                && let Some(path) = payload.get("path").and_then(Value::as_str)
            {
                let _ = done_tx.send(path.to_string());
            }
        });
        let state = app.state::<crate::app_state::AppState>();
        for item in found {
            let open = state
                .original_image
                .lock()
                .unwrap()
                .as_ref()
                .map(|loaded| loaded.path.clone());
            if open.as_deref() == Some(item.path.as_str()) {
                continue;
            }
            let (_, sidecar) = crate::file_management::parse_virtual_path(&item.path);
            if !is_old(&crate::exif_processing::load_sidecar(&sidecar).adjustments) {
                continue;
            }
            if let Err(e) = crate::file_management::save_metadata_and_update_thumbnail(
                item.path.clone(),
                item.adjustments,
                app.clone(),
                state.clone(),
            ) {
                log::warn!("[wb_legacy] could not save {}: {e}", item.path);
                continue;
            }
            // Wait for its thumbnail, or give up: a failed redraw sends nothing.
            let deadline = Instant::now() + Duration::from_secs(30);
            while let Some(left) = deadline.checked_duration_since(Instant::now()) {
                match done_rx.recv_timeout(left) {
                    Ok(path) if path == item.path => break,
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
        }
        app.unlisten(listener);
    });
}

/// Convert presets saved before 1.6.5. They describe a correction, not a photo,
/// so they are converted against D65 and stay relative.
fn upgrade_preset(adjustments: &mut Value) -> bool {
    if !is_old(adjustments) {
        return false;
    }
    upgrade(adjustments, WhiteBalance::reference());
    true
}

/// Recorded in our own preferences once the presets have been converted.
const PRESETS_MIGRATED: &str = "whiteBalancePresetsMigrated";

/// Convert the saved presets once, on the first start after the update.
///
/// Not on every load, the way a photo's edit is: a preset cannot be told apart
/// by its keys forever. Their Lightroom preset import (`preset_converter.rs`)
/// writes 1.6.5's units with no `whiteBalance` key, so a preset imported after
/// the update would look old and be converted a second time. Once, recorded
/// beside our other preferences, with the original kept beside the file.
pub fn migrate_presets_once(app_data: &Path, library: &Path) {
    if super::ag_settings::get(library, PRESETS_MIGRATED) == Some(Value::Bool(true)) {
        return;
    }
    let presets = app_data.join("presets").join("presets.json");
    match migrate_presets_file(&presets) {
        Ok(converted) => {
            if converted > 0 {
                log::info!("[wb_legacy] converted white balance in {converted} presets");
            }
            if let Err(e) = super::ag_settings::set(library, PRESETS_MIGRATED, Value::Bool(true)) {
                log::warn!("[wb_legacy] could not record the preset conversion: {e}");
            }
        }
        // Left unrecorded, so the next start tries again.
        Err(e) => log::warn!("[wb_legacy] presets not converted: {e}"),
    }
}

/// Convert every preset in the file and write it back, keeping the original
/// as `presets.pre-1.6.5.json`. Returns how many presets changed.
fn migrate_presets_file(presets: &Path) -> Result<usize, String> {
    if !presets.exists() {
        return Ok(0);
    }
    let text = std::fs::read_to_string(presets).map_err(|e| e.to_string())?;
    let mut value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let converted = upgrade_presets_in(&mut value);
    if converted == 0 {
        return Ok(0);
    }
    let backup = presets.with_file_name("presets.pre-1.6.5.json");
    if !backup.exists() {
        std::fs::copy(presets, &backup).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    std::fs::write(presets, json).map_err(|e| e.to_string())?;
    Ok(converted)
}

/// Every preset in a presets file, folders included. A preset is whatever
/// carries `adjustments`; the walk does not go inside one, because a mask's
/// own `adjustments` belongs to the preset around it and is converted with it.
fn upgrade_presets_in(value: &mut Value) -> usize {
    match value {
        Value::Array(items) => items.iter_mut().map(upgrade_presets_in).sum(),
        Value::Object(object) => {
            let mut converted = 0;
            for (key, child) in object.iter_mut() {
                if key == "adjustments" {
                    converted += usize::from(upgrade_preset(child));
                } else {
                    converted += upgrade_presets_in(child);
                }
            }
            converted
        }
        _ => 0,
    }
}

/// An edit saved before 1.6.5 that moved a white balance slider, globally or in
/// a mask. One that moved none needs nothing and is left exactly as it is.
pub fn is_old(adjustments: &Value) -> bool {
    let Some(object) = adjustments.as_object() else {
        return false;
    };
    if object.contains_key("whiteBalance") {
        return false;
    }
    sliders(object) != (0.0, 0.0) || masks(object).any(|mask| sliders(mask) != (0.0, 0.0))
}

/// Convert in place. Does nothing to an edit that is not old.
pub fn upgrade(adjustments: &mut Value, as_shot: WhiteBalance) -> bool {
    if !is_old(adjustments) {
        return false;
    }
    let Some(object) = adjustments.as_object_mut() else {
        return false;
    };

    let (temperature, tint) = sliders(object);
    let global = write_global(object, as_shot, legacy_target(as_shot, temperature, tint));

    // A mask's old sliders were added to the global ones before the curve, so
    // inside it the picture got the correction for the sum. Their masks add
    // log gains on top of the global ones instead, so the mask is given
    // whatever takes the global result to that sum.
    if let Some(Value::Array(list)) = object.get_mut("masks") {
        for mask in list.iter_mut() {
            let Some(mask) = mask.get_mut("adjustments").and_then(Value::as_object_mut) else {
                continue;
            };
            let (mask_temperature, mask_tint) = sliders(mask);
            if (mask_temperature, mask_tint) == (0.0, 0.0) {
                continue;
            }
            let inside = legacy_target(as_shot, temperature + mask_temperature, tint + mask_tint);
            let (t, n) = relative(global, inside);
            mask.insert(
                "temperature".into(),
                json!(t.clamp(-RELATIVE_RANGE, RELATIVE_RANGE)),
            );
            mask.insert(
                "tint".into(),
                json!(n.clamp(-RELATIVE_RANGE, RELATIVE_RANGE)),
            );
        }
    }
    true
}

/// Temperature and tint, globally and for each mask in order.
fn white_balance_numbers(adjustments: &Value) -> Value {
    let pair = |object: &Map<String, Value>| {
        let (temperature, tint) = sliders(object);
        json!({ "temperature": temperature, "tint": tint })
    };
    let Some(object) = adjustments.as_object() else {
        return Value::Null;
    };
    // One entry per mask, with or without adjustments, so the indices line up
    // with the masks the editor holds.
    let none = Map::new();
    let per_mask = object
        .get("masks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|mask| {
            pair(
                mask.get("adjustments")
                    .and_then(Value::as_object)
                    .unwrap_or(&none),
            )
        })
        .collect();
    let mut numbers = pair(object);
    numbers["masks"] = Value::Array(per_mask);
    numbers
}

fn sliders(object: &Map<String, Value>) -> (f64, f64) {
    let read = |key: &str| object.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    (read("temperature"), read("tint"))
}

fn masks(object: &Map<String, Value>) -> impl Iterator<Item = &Map<String, Value>> {
    object
        .get("masks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|mask| mask.get("adjustments").and_then(Value::as_object))
}

/// Write the global white balance, relative if it fits on the sliders, and
/// return the white balance their renderer will read back from what was written.
fn write_global(
    object: &mut Map<String, Value>,
    as_shot: WhiteBalance,
    target: WhiteBalance,
) -> WhiteBalance {
    let (t, n) = relative(as_shot, target);
    if t.abs() <= RELATIVE_RANGE && n.abs() <= RELATIVE_RANGE {
        object.insert("temperature".into(), json!(t));
        object.insert("tint".into(), json!(n));
        object.insert("whiteBalance".into(), Value::Null);
        as_shot.shifted(t, n)
    } else {
        object.insert("temperature".into(), json!(0));
        object.insert("tint".into(), json!(0));
        object.insert(
            "whiteBalance".into(),
            json!({ "temperature": target.temperature, "tint": target.tint }),
        );
        target
    }
}

/// Their relative slider values that take `from` to `to`.
///
/// To a tenth of a step, not a whole one. The slider shows whole steps and the
/// photo keeps the rest: rounding to the step, as their picker does, moved a
/// converted edit by up to 1% in blue at a tungsten as-shot, and a tenth keeps
/// it under 0.15%.
fn relative(from: WhiteBalance, to: WhiteBalance) -> (f64, f64) {
    let mired = |wb: WhiteBalance| 1.0e6 / wb.temperature;
    let tenth = |value: f64| (value * 10.0).round() / 10.0 + 0.0;
    (
        tenth((mired(from) - mired(to)) / MIRED_PER_RELATIVE_UNIT),
        tenth((to.tint - from.tint) / TINT_PER_RELATIVE_UNIT),
    )
}

/// The white balance that makes the correction the old sliders made.
fn legacy_target(as_shot: WhiteBalance, temperature: f64, tint: f64) -> WhiteBalance {
    if (temperature, tint) == (0.0, 0.0) {
        return as_shot;
    }
    let (x, y) = legacy_illuminant(temperature, tint);
    crate::mods::auto_wb::removing_illuminant(x, y, as_shot).unwrap_or(as_shot)
}

/// The illuminant the old sliders named, in CIE xy: `ag_slider_to_kelvin`,
/// `ag_kelvin_to_xy` and `ag_apply_tint` from the retired `modules.wgsl`.
fn legacy_illuminant(temperature: f64, tint: f64) -> (f64, f64) {
    let t = temperature / OLD_SCALE_TEMPERATURE;
    let n = tint / OLD_SCALE_TINT;
    let kelvin = (6500.0 * (t * 0.28).exp()).clamp(1800.0, 20000.0);
    let (x, y) = kelvin_to_xy(kelvin);
    (x, y + n * 0.05)
}

/// CIE daylight locus above 4000K, Planckian below it, as the shader had it.
fn kelvin_to_xy(kelvin: f64) -> (f64, f64) {
    let inv = 1000.0 / kelvin.clamp(1800.0, 20000.0);
    let x = if kelvin < 4000.0 {
        -0.266_123_9 * inv.powi(3) - 0.234_358_9 * inv.powi(2) + 0.877_695_6 * inv + 0.179_910
    } else if kelvin <= 7000.0 {
        0.244_063 + 0.099_11 * inv + 2.967_8 * inv.powi(2) - 4.607_0 * inv.powi(3)
    } else {
        0.237_040 + 0.247_48 * inv + 1.901_8 * inv.powi(2) - 2.006_4 * inv.powi(3)
    };
    (x, -3.000 * x * x + 2.870 * x - 0.275)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{DMat3, DVec3};

    const BRADFORD: [f64; 9] = [
        0.8951, -0.7502, 0.0389, 0.2664, 1.7135, -0.0685, -0.1614, 0.0367, 1.0296,
    ];

    fn lms(x: f64, y: f64) -> DVec3 {
        DMat3::from_cols_array(&BRADFORD) * DVec3::new(x / y, 1.0, (1.0 - x - y) / y)
    }

    /// The log cone gains the retired shader applied for these sliders,
    /// computed the way it computed them: from the illuminant to D65.
    fn old_gains(temperature: f64, tint: f64) -> DVec3 {
        let (x, y) = legacy_illuminant(temperature, tint);
        let (d65_x, d65_y) = (0.312_726_9, 0.329_023_2);
        let ratio = lms(d65_x, d65_y) / lms(x, y);
        DVec3::new(ratio.x.ln(), ratio.y.ln(), ratio.z.ln())
    }

    /// The log cone gains their renderer applies for these adjustments.
    fn new_gains(adjustments: &Value, as_shot: WhiteBalance) -> DVec3 {
        let target = white_balance::from_adjustments(adjustments, as_shot);
        DVec3::from_array(white_balance::adaptation_log_gains(as_shot, target).map(f64::from))
    }

    fn as_shots() -> [WhiteBalance; 3] {
        [
            WhiteBalance::reference(),
            WhiteBalance {
                temperature: 3200.0,
                tint: 4.0,
            },
            WhiteBalance {
                temperature: 5600.0,
                tint: -6.0,
            },
        ]
    }

    /// How far apart two sets of gains are in colour. Their shader divides by
    /// the brightness of the adapted white, so a gain all three cones share
    /// never shows; only the ratios between them do.
    fn colour_difference(a: DVec3, b: DVec3) -> f64 {
        let d = a - b;
        ((d.x - d.y).abs()).max((d.z - d.y).abs())
    }

    /// Computed exactly, the conversion lands within 0.0002 of the old gains.
    /// What is left is the tenth of a slider step it is rounded to.
    const AGREE: f64 = 0.003;

    #[test]
    fn a_converted_edit_makes_the_correction_the_old_sliders_made() {
        for as_shot in as_shots() {
            for (temperature, tint) in [
                (12.0, 0.0),
                (-25.0, 0.0),
                (40.0, 15.0),
                (-50.0, -20.0),
                (0.0, 30.0),
            ] {
                let mut adjustments =
                    json!({ "temperature": temperature, "tint": tint, "exposure": 0.3 });
                assert!(upgrade(&mut adjustments, as_shot));
                let difference = colour_difference(
                    new_gains(&adjustments, as_shot),
                    old_gains(temperature, tint),
                );
                assert!(
                    difference < AGREE,
                    "({temperature}, {tint}) at {as_shot:?}: gains differ by {difference:?}",
                );
                assert_eq!(
                    adjustments["exposure"], 0.3,
                    "the rest of the edit is untouched"
                );
            }
        }
    }

    #[test]
    fn what_fits_on_the_sliders_stays_relative() {
        let mut adjustments = json!({ "temperature": 20.0, "tint": 0.0 });
        upgrade(&mut adjustments, WhiteBalance::reference());
        assert_eq!(adjustments["whiteBalance"], Value::Null);
        assert_ne!(adjustments["temperature"], 0.0);
    }

    #[test]
    fn what_does_not_fit_becomes_kelvin() {
        // -100 was 318 mired; their sliders reach 150.
        let as_shot = WhiteBalance::reference();
        let mut adjustments = json!({ "temperature": -100.0, "tint": 0.0 });
        upgrade(&mut adjustments, as_shot);
        assert!(adjustments["whiteBalance"].is_object());
        assert_eq!(adjustments["temperature"], 0);
        let difference =
            colour_difference(new_gains(&adjustments, as_shot), old_gains(-100.0, 0.0));
        assert!(difference < AGREE, "{difference}");
    }

    #[test]
    fn inside_a_mask_the_picture_gets_the_old_sum() {
        let as_shot = WhiteBalance {
            temperature: 4300.0,
            tint: 2.0,
        };
        let mut adjustments = json!({
            "temperature": 15.0,
            "tint": 5.0,
            "masks": [{ "adjustments": { "temperature": -30.0, "tint": 10.0 } }],
        });
        upgrade(&mut adjustments, as_shot);

        let global = white_balance::from_adjustments(&adjustments, as_shot);
        let mask = global.shifted(
            adjustments["masks"][0]["adjustments"]["temperature"]
                .as_f64()
                .unwrap(),
            adjustments["masks"][0]["adjustments"]["tint"]
                .as_f64()
                .unwrap(),
        );
        let inside =
            DVec3::from_array(white_balance::adaptation_log_gains(as_shot, mask).map(f64::from));
        let difference = colour_difference(inside, old_gains(15.0 - 30.0, 5.0 + 10.0));
        assert!(difference < 2.0 * AGREE, "{difference}");
    }

    #[test]
    fn an_edit_from_after_the_merge_is_left_alone() {
        let adjustments = json!({ "temperature": 30.0, "tint": 0.0, "whiteBalance": null });
        assert!(!is_old(&adjustments));
        assert_eq!(
            upgraded("not-a-photo.jpg", adjustments.clone()),
            adjustments
        );
    }

    #[test]
    fn an_edit_that_never_touched_white_balance_is_left_alone() {
        let adjustments =
            json!({ "exposure": 1.0, "masks": [{ "adjustments": { "exposure": 0.5 } }] });
        assert!(!is_old(&adjustments));
        assert_eq!(
            upgraded("not-a-photo.jpg", adjustments.clone()),
            adjustments
        );
    }

    #[test]
    fn the_old_edits_in_a_folder_are_found_and_the_rest_left() {
        let dir = tempfile::tempdir().unwrap();
        let photo = |name: &str, adjustments: Value| {
            let path = dir.path().join(name);
            std::fs::write(&path, b"not really a jpeg").unwrap();
            let sidecar = crate::file_management::parse_virtual_path(&path.to_string_lossy()).1;
            let metadata = json!({ "version": 1, "rating": 3, "adjustments": adjustments });
            std::fs::write(sidecar, metadata.to_string()).unwrap();
            path.to_string_lossy().to_string()
        };
        let old = photo("old.jpg", json!({ "temperature": -30.0, "tint": 0.0 }));
        let new = photo(
            "new.jpg",
            json!({ "temperature": -30.0, "whiteBalance": null }),
        );
        let untouched = photo("untouched.jpg", json!({ "exposure": 1.0 }));
        let unedited = dir
            .path()
            .join("unedited.jpg")
            .to_string_lossy()
            .to_string();

        let (found, fresh) = find_old(&[old.clone(), new, untouched, unedited]);
        assert_eq!(found.len(), 1);
        assert_eq!(
            fresh.len(),
            1,
            "converted by this call, so still to be saved"
        );
        assert_eq!(found[0].path, old);
        assert!(
            found[0]
                .adjustments
                .as_object()
                .unwrap()
                .contains_key("whiteBalance")
        );
        assert_eq!(
            found[0].was,
            json!({ "temperature": -30.0, "tint": 0.0, "masks": [] })
        );

        // Asked again before anything was saved: the same answer, from memory,
        // and nothing new to save.
        let (again, fresh) = find_old(std::slice::from_ref(&old));
        assert_eq!(again[0].adjustments, found[0].adjustments);
        assert!(fresh.is_empty());
    }

    #[test]
    fn converting_twice_changes_nothing_the_second_time() {
        let mut adjustments = json!({ "temperature": -40.0, "tint": 12.0 });
        assert!(upgrade(&mut adjustments, WhiteBalance::reference()));
        let once = adjustments.clone();
        assert!(!upgrade(&mut adjustments, WhiteBalance::reference()));
        assert_eq!(adjustments, once);
    }

    #[test]
    fn warmer_is_still_warmer() {
        // Direction, not just size: positive temperature warmed the picture
        // before and must still warm it, so red gains on blue.
        let mut adjustments = json!({ "temperature": 30.0, "tint": 0.0 });
        upgrade(&mut adjustments, WhiteBalance::reference());
        let gains = new_gains(&adjustments, WhiteBalance::reference());
        assert!(gains.x > gains.z, "{gains:?}");
    }

    fn presets() -> Value {
        json!([
            { "preset": { "id": "a", "name": "Warm", "adjustments": { "temperature": 30.0, "tint": 0.0 } } },
            { "folder": { "id": "f", "name": "Folder", "children": [
                { "id": "b", "name": "Masked", "adjustments": {
                    "temperature": 0.0,
                    "tint": 0.0,
                    "masks": [{ "adjustments": { "temperature": -20.0 } }],
                } },
                { "id": "c", "name": "Since", "adjustments": { "temperature": 10.0, "whiteBalance": null } },
                { "id": "d", "name": "Exposure", "adjustments": { "exposure": 0.7 } },
            ] } },
        ])
    }

    #[test]
    fn presets_in_folders_are_converted_and_a_mask_only_with_its_preset() {
        let mut value = presets();
        assert_eq!(upgrade_presets_in(&mut value), 2);

        // -20 was 39 mired warmer than D65; 1.5 mired a step is -26.
        let mask = value[1]["folder"]["children"][0]["adjustments"]["masks"][0]["adjustments"]["temperature"]
            .as_f64()
            .unwrap();
        assert!((-28.0..=-24.0).contains(&mask), "mask converted to {mask}");

        let since = &value[1]["folder"]["children"][1]["adjustments"];
        assert_eq!(
            since["temperature"], 10.0,
            "a preset saved since 1.6.5 is not touched"
        );
        assert!(
            !value[1]["folder"]["children"][2]["adjustments"]
                .as_object()
                .unwrap()
                .contains_key("whiteBalance")
        );
    }

    #[test]
    fn the_presets_file_is_converted_once_and_the_original_kept() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("presets.json");
        std::fs::write(&file, serde_json::to_string_pretty(&presets()).unwrap()).unwrap();

        assert_eq!(migrate_presets_file(&file).unwrap(), 2);
        let backup: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("presets.pre-1.6.5.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(backup, presets());

        assert_eq!(
            migrate_presets_file(&file).unwrap(),
            0,
            "a second pass finds nothing old"
        );
    }

    #[test]
    fn no_presets_file_is_nothing_to_do() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            migrate_presets_file(&dir.path().join("presets.json")).unwrap(),
            0
        );
    }
}
