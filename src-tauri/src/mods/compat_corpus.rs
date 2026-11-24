//! The compatibility corpus: settings files and photo edits as each release
//! wrote them, read by the code of today. Ours, test-only.
//!
//! WHY
//!
//! Inherited code is being deleted: old panels, RapidRAW Cloud, settings
//! branches nothing takes, serde clean-ups. A deletion that changes how a
//! stored file is read, what of it survives being written back, or what the
//! renderer is given for it loses people's settings and edits without one
//! error. This makes any such change a failing test that names what moved.
//!
//! WHAT
//!
//! `src-tauri/fixtures/compat/<release>/` holds a `settings.json` and a
//! `photo.agdata` in the shape that release wrote them; synthetic, never
//! made from a photo. A folder stands for the releases around it whose stored
//! shape is the same: 26.41.6 for 26.41.x (26.41.12 added no key to an edit
//! or a setting, and its masks are still `ai-subject`), 26.43.3 for 26.43.1 to
//! 26.43.3, then one folder per release. For every release folder:
//!
//! - `settings.json` reads as `AppSettings` with a plain `serde_json::from_str`
//!   (what load_settings does before anything else), and written back the way
//!   save_settings writes it, keeps every key and value;
//! - `photo.agdata` reads as `ImageMetadata` and written back keeps every key
//!   and value; each edit in it (the photo's, and each snapshot's) has masks
//!   and AI patches that read as the render reads them; its History still
//!   belongs to its edit;
//! - the GPU input of each of those edits is what `snapshots/<release>.txt`
//!   says, within a tolerance (compat_snapshot.rs says what is in it).
//!
//! Not covered: what load_settings changes after reading (the Copy settings
//! lists, the root folders, the AI provider); for an edit, the Rust round trip
//! keeps every key by construction (`adjustments` is a `Value`), so the guards
//! for edit keys are the snapshot and the interface's test.
//!
//! A stored key that stops being kept on purpose goes in `DROPPED`, with the
//! reason and the PR. The interface's side, `normalizeLoadedAdjustments`, is
//! held to the same files by src/argentum/compatCorpus.test.ts.
//!
//! ADDING THE NEXT RELEASE
//!
//! The newest folder is the version in Cargo.toml, written by the generator
//! (compat_fixture.rs) with every setting and every key of the editor's
//! starting values set to a valid value other than its default:
//!
//!     cargo test --lib mods::compat_fixture -- --ignored
//!
//! then its snapshot is recorded as below. Run it after a version bump, and
//! again whenever a change adds a setting or a key while that version is
//! unreleased; `the_newest_fixture_has_every_key` fails until it is. Once a
//! release has shipped its folder is never written again: the generator only
//! writes the version being worked on. Cutting a release writes it for the
//! version being released (step 2 of docs/RELEASING.md, which runs
//! `the_release_being_cut_has_its_folder`); see "THE RELEASE'S FOLDER" below
//! for why that check is not run on every build.
//!
//! UPDATING THE SNAPSHOTS ON PURPOSE
//!
//!     AG_COMPAT_UPDATE=<release> cargo test --lib mods::compat_corpus
//!     AG_COMPAT_UPDATE=all cargo test --lib mods::compat_corpus
//!
//! rewrites `snapshots/<release>.txt`, or every one, from the code of today;
//! the diff is the review. Recording a new release uses its own version, so an
//! older snapshot cannot be rewritten by mistake and the tests still check it. A change meant to change pixels also bumps `PIPELINE` in
//! cache_version.rs, so cached thumbnails are made again.

use std::path::PathBuf;

use serde_json::Value;

use crate::app_settings::AppSettings;
use crate::image_processing::ImageMetadata;
use crate::mask_generation::{AiPatchDefinition, MaskDefinition};
use crate::mods::compat_snapshot::{differences, render, sections_for};

pub use crate::mods::retired_keys::{DROPPED, Dropped, EDIT, SETTINGS};

/// Lists held in a `HashSet`, written in no fixed order: compared as sets.
pub const UNORDERED: &[(&str, &str)] = &[
    (SETTINGS, "/copyPasteSettings/includedAdjustments"),
    (SETTINGS, "/copyPasteSettings/knownAdjustments"),
];

/// Settings the newest fixture may hold at their default, and why.
pub const SETTINGS_AT_DEFAULT: &[(&str, &str)] = &[(
    "useWgpuRenderer",
    "its default is true on Windows and macOS and false on Linux: no one value differs from both",
)];

/// Starting values the newest fixture's edit may hold at their default, and why.
pub const EDIT_AT_DEFAULT: &[(&str, &str)] = &[(
    "lutData",
    "nothing writes a LUT's data into an edit: it is always null",
)];

const SNAPSHOT_ADVICE: &str = "If the move is meant (a change to how stored edits render), bump \
     PIPELINE in src-tauri/src/mods/cache_version.rs so cached thumbnails are made again, and \
     rewrite the snapshots: AG_COMPAT_UPDATE=all cargo test --lib mods::compat_corpus";

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("compat")
}

/// The release folders, oldest first.
pub fn releases() -> Vec<String> {
    let version = |name: &str| -> Option<Vec<u32>> {
        name.split('.').map(|part| part.parse().ok()).collect()
    };
    let mut found: Vec<(Vec<u32>, String)> = std::fs::read_dir(root())
        .expect("src-tauri/fixtures/compat")
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            version(&name).map(|v| (v, name))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, name)| name).collect()
}

fn read(release: &str, file: &str) -> String {
    let path = root().join(release).join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every edit a sidecar holds: the photo's, then each snapshot's.
fn edits_of(metadata: &ImageMetadata) -> Vec<(String, &Value)> {
    let mut edits = vec![("edit".to_string(), &metadata.adjustments)];
    for (i, snapshot) in metadata.ag_snapshots.iter().flatten().enumerate() {
        edits.push((
            format!("snapshot {i} {:?}", snapshot.name),
            &snapshot.adjustments,
        ));
    }
    edits
}

/// What of `stored` is not in `kept`, as `kept` is the same file written back.
pub fn lost(file: &str, stored: &Value, kept: &Value) -> Vec<String> {
    let mut out = Vec::new();
    walk(file, String::new(), stored, Some(kept), &mut out);
    out
}

fn walk(file: &str, path: String, stored: &Value, kept: Option<&Value>, out: &mut Vec<String>) {
    if DROPPED
        .iter()
        .any(|d| d.file == file && matches(d.path, &path))
    {
        return;
    }
    let shown = if path.is_empty() { "/" } else { &path };
    let Some(kept) = kept else {
        out.push(format!("{shown} is gone"));
        return;
    };
    match (stored, kept) {
        (Value::Object(s), Value::Object(k)) => {
            for (key, value) in s {
                let child = format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
                walk(file, child, value, k.get(key), out);
            }
        }
        (Value::Array(s), Value::Array(k))
            if UNORDERED.iter().any(|&(f, p)| f == file && p == path) =>
        {
            let sorted = |items: &[Value]| {
                let mut texts: Vec<String> = items.iter().map(Value::to_string).collect();
                texts.sort();
                texts
            };
            if sorted(s) != sorted(k) {
                out.push(format!(
                    "{shown} holds other items: was {stored}, now {kept}"
                ));
            }
        }
        (Value::Array(s), Value::Array(k)) if s.len() != k.len() => {
            out.push(format!("{shown} had {} items, now {}", s.len(), k.len()));
        }
        (Value::Array(s), Value::Array(k)) => {
            for (i, (a, b)) in s.iter().zip(k).enumerate() {
                walk(file, format!("{path}/{i}"), a, Some(b), out);
            }
        }
        _ if same_json(stored, kept) => {}
        _ => out.push(format!("{shown} was {stored}, now {kept}")),
    }
}

/// Is `path` one that `pattern` names?
fn matches(pattern: &str, path: &str) -> bool {
    let (p, q): (Vec<&str>, Vec<&str>) = (pattern.split('/').collect(), path.split('/').collect());
    p.len() == q.len() && p.iter().zip(&q).all(|(p, q)| *p == "*" || p == q)
}

/// Equal, numbers by value: a float field gives `1.0` back for a stored `1`,
/// and an f32 its shortest text, which reads back to within a millionth.
pub fn same_json(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (
                x.as_f64().unwrap_or(f64::NAN),
                y.as_f64().unwrap_or(f64::NAN),
            );
            (x - y).abs() <= 1e-6 * x.abs().max(y.abs()).max(1.0)
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same_json(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(key, x)| y.get(key).is_some_and(|y| same_json(x, y)))
        }
        _ => a == b,
    }
}

/// The editor's starting values, as image_processing.rs builds them: RapidRAW's
/// defaultAdjustments.json with Argentum's on top.
pub fn starting_values() -> serde_json::Map<String, Value> {
    let mut values: serde_json::Map<String, Value> =
        serde_json::from_str(include_str!("../../../src/utils/defaultAdjustments.json"))
            .expect("defaultAdjustments.json");
    values.extend(
        serde_json::from_str::<serde_json::Map<String, Value>>(include_str!(
            "../../../src/argentum/defaultAdjustments.json"
        ))
        .expect("argentum/defaultAdjustments.json"),
    );
    values
}

/// Each setting and starting value that `settings` and `edit` leave out or
/// hold at its default, beyond the ones allowed to be.
pub fn uncovered(settings: &Value, edit: &Value) -> Vec<String> {
    let defaults: Value = serde_json::from_str(
        &serde_json::to_string(&AppSettings::default()).expect("default settings"),
    )
    .expect("default settings read back");
    let mut out = Vec::new();
    let mut check = |what: &str,
                     keys: &serde_json::Map<String, Value>,
                     held: &Value,
                     allowed: &[(&str, &str)]| {
        for (key, default) in keys {
            match held.get(key) {
                None => out.push(format!("{what} {key} is missing")),
                Some(value)
                    if same_json(value, default) && !allowed.iter().any(|(k, _)| k == key) =>
                {
                    out.push(format!("{what} {key} is at its default, {default}"))
                }
                Some(_) => {}
            }
        }
    };
    check(
        "setting",
        defaults.as_object().expect("an object"),
        settings,
        SETTINGS_AT_DEFAULT,
    );
    check("edit key", &starting_values(), edit, EDIT_AT_DEFAULT);
    out
}

fn report(problems: &[String]) -> String {
    format!(
        "{} problem(s):\n  {}",
        problems.len(),
        problems.join("\n  ")
    )
}

#[test]
fn every_settings_file_reads_and_keeps_every_key() {
    let mut problems = Vec::new();
    for release in releases() {
        let text = read(&release, SETTINGS);
        let stored: Value = serde_json::from_str(&text).expect("settings.json is JSON");
        match serde_json::from_str::<AppSettings>(&text) {
            Err(e) => problems.push(format!(
                "{release}/{SETTINGS} does not read as AppSettings, so load_settings starts \
                 from defaults: {e}"
            )),
            Ok(settings) => {
                // As save_settings writes it.
                let written = serde_json::to_string_pretty(&settings).expect("writes");
                let kept: Value = serde_json::from_str(&written).expect("reads back");
                problems.extend(
                    lost(SETTINGS, &stored, &kept)
                        .into_iter()
                        .map(|p| format!("{release}/{SETTINGS}: {p}")),
                );
            }
        }
    }
    assert!(problems.is_empty(), "{}", report(&problems));
}

#[test]
fn every_edit_reads_and_keeps_every_key() {
    let mut problems = Vec::new();
    for release in releases() {
        let file = format!("{release}/{EDIT}");
        let text = read(&release, EDIT);
        let stored: Value = serde_json::from_str(&text).expect("photo.agdata is JSON");
        let metadata = match serde_json::from_str::<ImageMetadata>(&text) {
            Ok(metadata) => metadata,
            Err(e) => {
                problems.push(format!(
                    "{file} does not read as ImageMetadata, so the photo opens unedited: {e}"
                ));
                continue;
            }
        };
        // As write_sidecar writes it.
        let written = serde_json::to_string_pretty(&metadata).expect("writes");
        let kept: Value = serde_json::from_str(&written).expect("reads back");
        problems.extend(
            lost(EDIT, &stored, &kept)
                .into_iter()
                .map(|p| format!("{file}: {p}")),
        );

        for (label, edit) in edits_of(&metadata) {
            // The render reads the whole list or none of it.
            if let Some(masks) = edit.get("masks")
                && let Err(e) = serde_json::from_value::<Vec<MaskDefinition>>(masks.clone())
            {
                problems.push(format!(
                    "{file}, {label}: its masks do not read, so the render draws none: {e}"
                ));
            }
            if let Some(patches) = edit.get("aiPatches")
                && let Err(e) = serde_json::from_value::<Vec<AiPatchDefinition>>(patches.clone())
            {
                problems.push(format!("{file}, {label}: its AI patches do not read: {e}"));
            }
        }

        let sidecar = root().join(&release).join(EDIT);
        if stored.get("agHistory").is_some() && crate::mods::history::read(&sidecar).is_none() {
            problems.push(format!(
                "{file}: its History no longer belongs to its edit, so the editor starts a new \
                 one (did the edit's fingerprint change?)"
            ));
        }
        let snapshots = crate::mods::snapshots::read(&sidecar);
        if snapshots.len() != metadata.ag_snapshots.as_ref().map_or(0, Vec::len) {
            problems.push(format!(
                "{file}: Snapshots reads {} of them",
                snapshots.len()
            ));
        }
    }
    assert!(problems.is_empty(), "{}", report(&problems));
}

#[test]
fn the_gpu_input_of_every_edit_is_as_recorded() {
    let update = std::env::var("AG_COMPAT_UPDATE").ok();
    if let Some(which) = &update {
        assert!(
            which == "all" || releases().contains(which),
            "AG_COMPAT_UPDATE is a release folder's name or `all`, not {which:?}"
        );
    }
    let mut problems = Vec::new();
    for release in releases() {
        let Ok(metadata) = serde_json::from_str::<ImageMetadata>(&read(&release, EDIT)) else {
            // every_edit_reads_and_keeps_every_key says why.
            problems.push(format!("{release}/{EDIT} does not read"));
            continue;
        };
        let sections: Vec<_> = edits_of(&metadata)
            .into_iter()
            .flat_map(|(label, edit)| sections_for(&label, edit))
            .collect();
        let header = format!(
            "What the code of today gives the renderer for fixtures/compat/{release}/{EDIT}\n\
             (src-tauri/src/mods/compat_snapshot.rs). Rewritten on purpose, never by hand:\n\
             AG_COMPAT_UPDATE=<release> cargo test --lib mods::compat_corpus"
        );
        let today = render(&header, &sections);
        let path = root().join("snapshots").join(format!("{release}.txt"));
        if update
            .as_deref()
            .is_some_and(|u| u == "all" || u == release)
        {
            std::fs::create_dir_all(root().join("snapshots")).expect("snapshots folder");
            std::fs::write(&path, &today).expect("writes the snapshot");
            continue;
        }
        let Ok(committed) = std::fs::read_to_string(&path) else {
            problems.push(format!(
                "{release} has no snapshot: record it with AG_COMPAT_UPDATE={release} cargo test --lib \
                 mods::compat_corpus"
            ));
            continue;
        };
        let moved = differences(&committed, &today);
        if !moved.is_empty() {
            let shown: Vec<&str> = moved.iter().take(80).map(String::as_str).collect();
            let more = moved.len().saturating_sub(shown.len());
            problems.push(format!(
                "{release}: {} value(s) moved from snapshots/{release}.txt\n    {}{}",
                moved.len(),
                shown.join("\n    "),
                if more > 0 {
                    format!("\n    ... and {more} more")
                } else {
                    String::new()
                }
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "{}\n\n{SNAPSHOT_ADVICE}",
        problems.join("\n\n")
    );
}

#[test]
fn the_newest_fixture_has_every_key() {
    let newest = releases().pop().expect("a release folder");
    let settings: Value = serde_json::from_str(&read(&newest, SETTINGS)).expect("JSON");
    let metadata: Value = serde_json::from_str(&read(&newest, EDIT)).expect("JSON");
    let missing = uncovered(&settings, &metadata["adjustments"]);
    assert!(
        missing.is_empty(),
        "the newest fixture, {newest}, does not cover the code of today:\n  {}\n\
         Write this version's with `cargo test --lib mods::compat_fixture -- --ignored`, \
         after adding a non-default value for each to compat_fixture.rs.",
        missing.join("\n  ")
    );
}

// NEWER FILES (a user who goes back a release, two machines on different
// releases sharing a folder): a key this release does not know is kept, not
// dropped, by every load and save of the two files.

/// Keys of several JSON types a newer release might add at the top level.
fn newer_keys() -> serde_json::Map<String, Value> {
    let Value::Object(keys) = serde_json::json!({
        "agFutureObject": { "x": 1, "nested": { "list": [1, 2.5, "three"] } },
        "agFutureList": [1, "two", null, { "k": false }],
        "agFutureText": "a value",
        "agFutureNumber": 42.5,
        "agFutureFlag": true,
        "agFutureNothing": null,
    }) else {
        unreachable!("an object")
    };
    keys
}

fn with_keys(text: &str, keys: &serde_json::Map<String, Value>) -> String {
    let mut file: Value = serde_json::from_str(text).expect("JSON");
    file.as_object_mut()
        .expect("an object")
        .extend(keys.clone());
    serde_json::to_string_pretty(&file).expect("writes")
}

fn assert_kept(what: &str, written: &str, keys: &serde_json::Map<String, Value>) {
    let kept: Value = serde_json::from_str(written).expect("reads back");
    for (key, value) in keys {
        assert_eq!(kept.get(key), Some(value), "{what}: {key} was not kept");
    }
}

/// What the newest fixtures are written back as: the settings, then the edit.
fn newest_written() -> (String, String) {
    let newest = releases().pop().expect("a release folder");
    let settings: AppSettings = serde_json::from_str(&read(&newest, SETTINGS)).expect("reads");
    let metadata: ImageMetadata = serde_json::from_str(&read(&newest, EDIT)).expect("reads");
    (
        serde_json::to_string_pretty(&settings).expect("writes"),
        serde_json::to_string_pretty(&metadata).expect("writes"),
    )
}

/// A file with no unknown key is written as it was before they were kept:
/// every key it writes is one of the type's own, none else appears.
#[test]
fn a_file_with_no_unknown_key_is_written_with_none() {
    let newest = releases().pop().expect("a release folder");
    let (settings, edit) = newest_written();
    let own = |value: Result<Value, serde_json::Error>| -> Vec<String> {
        value
            .expect("to JSON")
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect()
    };
    for (file, written, own_keys) in [
        (
            SETTINGS,
            settings,
            own(serde_json::to_value(AppSettings::default())),
        ),
        (
            EDIT,
            edit,
            own(serde_json::to_value(ImageMetadata::default())),
        ),
    ] {
        let stored: Value = serde_json::from_str(&read(&newest, file)).expect("JSON");
        let kept: Value = serde_json::from_str(&written).expect("JSON");
        for key in kept.as_object().expect("an object").keys() {
            assert!(
                stored.get(key).is_some() || own_keys.contains(key),
                "{newest}/{file}: {key} appears when it is read and saved"
            );
        }
    }
}

#[test]
fn a_newer_settings_file_keeps_its_unknown_keys_through_load_and_save() {
    let newest = releases().pop().expect("a release folder");
    let keys = newer_keys();
    let dir = tempfile::tempdir().expect("a folder");
    let path = dir.path().join("settings.json");
    std::fs::write(&path, with_keys(&read(&newest, SETTINGS), &keys)).expect("writes");

    // load_settings: read_or_keep on the file's text.
    let content = std::fs::read_to_string(&path).expect("reads");
    let loaded: AppSettings = crate::mods::salvage::read_or_keep(&path, &content, Default::default);
    // To the webview and back as save_settings receives it, then written as it writes.
    let sent = serde_json::to_value(&loaded).expect("to the webview");
    assert_kept("sent to the webview", &sent.to_string(), &keys);
    let returned: AppSettings = serde_json::from_value(sent).expect("from the webview");
    let written = serde_json::to_string_pretty(&returned).expect("writes");
    assert_kept("settings.json", &written, &keys);
}

#[test]
fn a_newer_settings_file_with_a_key_of_a_new_shape_keeps_its_unknown_keys() {
    let newest = releases().pop().expect("a release folder");
    let keys = newer_keys();
    let mut file: Value =
        serde_json::from_str(&with_keys(&read(&newest, SETTINGS), &keys)).expect("JSON");
    file["theme"] = serde_json::json!({ "a": "newer shape" });
    let dir = tempfile::tempdir().expect("a folder");
    let path = dir.path().join("settings.json");
    let content = file.to_string();
    std::fs::write(&path, &content).expect("writes");

    let loaded: AppSettings = crate::mods::salvage::read_or_keep(&path, &content, Default::default);
    let written = serde_json::to_string_pretty(&loaded).expect("writes");
    assert_kept("settings.json", &written, &keys);
}

#[test]
fn a_retired_settings_key_is_not_kept_with_the_unknown_ones() {
    let newest = releases().pop().expect("a release folder");
    let keys = newer_keys();
    let retired = serde_json::json!({ "baselineExposure": 1.5, "lastImportSettings": { "a": 1 } });
    let mut file: Value =
        serde_json::from_str(&with_keys(&read(&newest, SETTINGS), &keys)).expect("JSON");
    file.as_object_mut()
        .expect("an object")
        .extend(retired.as_object().expect("an object").clone());
    let dir = tempfile::tempdir().expect("a folder");
    let path = dir.path().join("settings.json");
    let content = file.to_string();

    // As load_settings reads it, then save_settings writes it.
    let mut loaded: AppSettings =
        crate::mods::salvage::read_or_keep(&path, &content, Default::default);
    assert!(crate::mods::retired_keys::strip(
        &mut loaded.extra,
        SETTINGS
    ));
    let written = serde_json::to_string_pretty(&loaded).expect("writes");
    assert_kept("settings.json", &written, &keys);
    let kept: Value = serde_json::from_str(&written).expect("JSON");
    for key in retired.as_object().expect("an object").keys() {
        assert!(kept.get(key).is_none(), "{key} was kept");
    }
}

#[test]
fn a_retired_key_in_an_edit_file_is_not_kept_with_the_unknown_ones() {
    use crate::exif_processing::try_load_sidecar;
    // The edit file has none retired today: a list entry for it is stripped on load.
    let keys = newer_keys();
    let dir = tempfile::tempdir().expect("a folder");
    let sidecar = dir.path().join("IMG.CR2.agdata");
    let plain = serde_json::to_string(&ImageMetadata::default()).expect("writes");
    std::fs::write(&sidecar, with_keys(&plain, &keys)).expect("writes");
    let metadata = try_load_sidecar(&sidecar).expect("reads");
    assert_eq!(
        metadata.extra, keys,
        "the unknown keys are kept as they are"
    );
}

#[test]
fn a_newer_edit_file_keeps_its_unknown_keys_through_every_save() {
    use crate::exif_processing::{try_load_sidecar, write_sidecar};
    let newest = releases().pop().expect("a release folder");
    let keys = newer_keys();
    let dir = tempfile::tempdir().expect("a folder");
    let photo = dir.path().join("IMG.CR2");
    std::fs::write(&photo, "raw").expect("a photo");
    let sidecar = crate::file_management::sidecar_for(&photo, None);
    std::fs::write(&sidecar, with_keys(&read(&newest, EDIT), &keys)).expect("writes");

    // write_sidecar of what try_load_sidecar read.
    let metadata = try_load_sidecar(&sidecar).expect("reads");
    write_sidecar(&sidecar, &photo, &metadata).expect("saves");
    assert_kept(
        EDIT,
        &std::fs::read_to_string(&sidecar).expect("reads"),
        &keys,
    );

    // A rating and a tag take the same read-change-write path.
    crate::mods::write_locks::update_sidecar(&sidecar, &photo, |m| m.rating = 1).expect("rates");
    let written = std::fs::read_to_string(&sidecar).expect("reads");
    assert_kept(EDIT, &written, &keys);
    assert_eq!(
        serde_json::from_str::<Value>(&written).expect("JSON")["rating"],
        1
    );
}

#[test]
fn an_unedited_edit_file_with_unknown_keys_is_not_deleted() {
    use crate::exif_processing::{try_load_sidecar, write_sidecar};
    let keys = newer_keys();
    let dir = tempfile::tempdir().expect("a folder");
    let photo = dir.path().join("IMG.CR2");
    std::fs::write(&photo, "raw").expect("a photo");
    let sidecar = crate::file_management::sidecar_for(&photo, None);
    let plain = serde_json::to_string(&ImageMetadata::default()).expect("writes");
    std::fs::write(&sidecar, with_keys(&plain, &keys)).expect("writes");

    // Nothing but the newer release's keys is on it: a save must not take it for a default.
    let metadata = try_load_sidecar(&sidecar).expect("reads");
    write_sidecar(&sidecar, &photo, &metadata).expect("saves");
    assert_kept(
        EDIT,
        &std::fs::read_to_string(&sidecar).expect("the file is still there"),
        &keys,
    );
}

#[test]
fn a_dropped_path_names_any_index() {
    let example = Dropped {
        file: EDIT,
        path: "/adjustments/masks/*/adjustments/oldKey",
        reason: "an example",
        pr: "#0",
    };
    assert!(matches(
        example.path,
        "/adjustments/masks/3/adjustments/oldKey"
    ));
    assert!(!matches(
        example.path,
        "/adjustments/masks/3/adjustments/newKey"
    ));
    assert!(!matches(example.path, "/adjustments/masks/3/adjustments"));
    for dropped in DROPPED.iter().chain([&example]) {
        assert!(
            !dropped.reason.is_empty() && !dropped.pr.is_empty(),
            "{} says why and which PR",
            dropped.path
        );
    }
}

#[test]
fn a_key_written_back_without_its_value_is_lost() {
    let stored = serde_json::json!({ "a": 1, "b": { "c": [1, 2], "d/e": true } });
    let kept = serde_json::json!({ "a": 1.0, "b": { "c": [1, 3] } });
    assert_eq!(
        lost(EDIT, &stored, &kept),
        ["/b/c/1 was 2, now 3", "/b/d~1e is gone"]
    );
}

// THE RELEASE'S FOLDER
//
// Which versions need a folder: the one being RELEASED. A folder stands for the
// releases around it whose stored shape is the same, so a release that changed
// no stored key may have none of its own, and the development version moves on
// right after each release: a test that wanted a folder for every version
// Cargo.toml names would be red on every bump. The version being cut is
// different: it is named when the release is made (docs/RELEASING.md), and
// its folder is written and committed with it. That check is an ignored test,
// run by that step.

/// What is missing for `version`'s own folder, if anything.
pub fn release_without_folder(version: &str, releases: &[String]) -> Option<String> {
    let mut missing = Vec::new();
    if !releases.iter().any(|r| r == version) {
        missing.push(format!("fixtures/compat/{version}/"));
    }
    if !root()
        .join("snapshots")
        .join(format!("{version}.txt"))
        .is_file()
    {
        missing.push(format!("fixtures/compat/snapshots/{version}.txt"));
    }
    if missing.is_empty() {
        return None;
    }
    Some(format!(
        "{version} is being released and the compatibility corpus has no {}. Write them: \
         `cargo test --lib mods::compat_fixture -- --ignored`, then \
         `AG_COMPAT_UPDATE={version} cargo test --lib mods::compat_corpus`; check that they hold only \
         synthetic values; commit them with the release (docs/RELEASING.md).",
        missing.join(" and ")
    ))
}

#[test]
fn a_version_with_no_folder_is_named() {
    let message = release_without_folder("99.99.9", &releases()).expect("no such folder");
    assert!(message.contains("fixtures/compat/99.99.9/"), "{message}");
    assert!(message.contains("snapshots/99.99.9.txt"), "{message}");
    assert!(message.contains("compat_fixture"), "{message}");
    let newest = releases().pop().expect("a release folder");
    assert_eq!(release_without_folder(&newest, &releases()), None);
}

/// Run by the release step: the version in Cargo.toml has its folder, and the
/// folder holds every file a folder of today holds.
#[test]
#[ignore = "the release step (docs/RELEASING.md) runs it on the version being released"]
fn the_release_being_cut_has_its_folder() {
    let version = env!("CARGO_PKG_VERSION");
    if let Some(message) = release_without_folder(version, &releases()) {
        panic!("{message}");
    }
    for file in [SETTINGS, EDIT, PROCESSING] {
        assert!(
            root().join(version).join(file).is_file(),
            "fixtures/compat/{version}/{file} is missing: write the folder again with \
             `cargo test --lib mods::compat_fixture -- --ignored`"
        );
    }
}

// THE PROCESSING SETTINGS FILE
//
// `argentum-processing.json` is read and written by ag_settings.rs, one key at
// a time. It joined the corpus at PROCESSING_SINCE: older folders predate it
// and are accepted without it, by that rule and no other.

pub const PROCESSING: &str = "argentum-processing.json";

/// The first release folder that holds the processing settings.
const PROCESSING_SINCE: &str = "26.47.4";

fn version_parts(name: &str) -> Vec<u32> {
    name.split('.').filter_map(|p| p.parse().ok()).collect()
}

fn has_processing_file(release: &str) -> bool {
    root().join(release).join(PROCESSING).is_file()
}

/// Does `key` of the file in `dir` read as the type its owner reads it as?
/// `None` if this table has no entry for it: a new setting, add it here.
fn reads_as_its_type(key: &str, dir: &std::path::Path) -> Option<bool> {
    use crate::mods::ag_settings::Setting;
    use crate::mods::editor_layout::{LayoutPanels, ModernTheme, TextSize};
    use crate::mods::raw_decoder::RawDecoder;
    macro_rules! reads {
        ($t:ty, $key:literal) => {
            Setting::<$t>::new($key).load(dir).is_some()
        };
    }
    Some(match key {
        "editorLayoutPanels" => reads!(LayoutPanels, "editorLayoutPanels"),
        "modernTheme" => reads!(ModernTheme, "modernTheme"),
        "modernTextSize" => reads!(TextSize, "modernTextSize"),
        "rawDecoder" => reads!(RawDecoder, "rawDecoder"),
        "compactSliderGap" => reads!(u64, "compactSliderGap"),
        "tiffBitDepth" => reads!(u8, "tiffBitDepth"),
        "looksFavourites" => reads!(Vec<String>, "looksFavourites"),
        "recentFolders" => reads!(Vec<String>, "recentFolders"),
        "lutProvider" => reads!(String, "lutProvider"),
        "focusPointsShown" => reads!(bool, "focusPointsShown"),
        "highlightRecovery" => reads!(bool, "highlightRecovery"),
        "providerLooksOn" => reads!(bool, "providerLooksOn"),
        "preSharpeningRetired" => reads!(bool, "preSharpeningRetired"),
        "whiteBalancePresetsMigrated" => reads!(bool, "whiteBalancePresetsMigrated"),
        _ => return None,
    })
}

/// Every key the code declares as a processing setting, found in the source of
/// `src/mods` and its subfolders: a `const` or `static` of type `Setting<..>`, made with a quoted key or with
/// its file's own `KEY` constant. Only constants count: a test may use others.
pub fn declared_processing_keys() -> std::collections::BTreeSet<String> {
    let mut keys = std::collections::BTreeSet::new();
    let mut files = Vec::new();
    let mut folders = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/mods")];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(folder).expect("src/mods").flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|e| e == "rs")
                && !path.ends_with("ag_settings.rs")
            {
                files.push(path);
            }
        }
    }
    for path in files {
        let text = std::fs::read_to_string(&path).expect("a source file");
        for (at, _) in text
            .match_indices("const ")
            .chain(text.match_indices("static "))
        {
            let statement = text[at..].split(';').next().unwrap_or_default();
            if !statement.contains("Setting<") {
                continue;
            }
            let Some(args) = statement.split("new(").nth(1) else {
                continue;
            };
            if let Some(key) = args.strip_prefix('"').and_then(|a| a.split('"').next()) {
                keys.insert(key.to_string());
            } else if args.starts_with("KEY)")
                && let Some(key) = text
                    .split("const KEY: &str = \"")
                    .nth(1)
                    .and_then(|a| a.split('"').next())
            {
                keys.insert(key.to_string());
            }
        }
    }
    keys
}

#[test]
fn a_processing_file_may_be_missing_only_before_it_existed() {
    let since = version_parts(PROCESSING_SINCE);
    for release in releases() {
        if version_parts(&release) >= since {
            assert!(
                has_processing_file(&release),
                "{release}/{PROCESSING} is missing; only folders before {PROCESSING_SINCE} \
                 predate it. Write it with `cargo test --lib mods::compat_fixture -- --ignored`"
            );
        }
    }
    assert!(
        has_processing_file(PROCESSING_SINCE),
        "PROCESSING_SINCE names {PROCESSING_SINCE}, which has no {PROCESSING}: if that folder was \n         renamed or removed, set PROCESSING_SINCE to the oldest folder that holds one"
    );
}

#[test]
fn every_processing_file_reads_and_keeps_every_key() {
    use crate::mods::ag_settings::Setting;
    let mut problems = Vec::new();
    for release in releases().into_iter().filter(|r| has_processing_file(r)) {
        let file = format!("{release}/{PROCESSING}");
        let dir = tempfile::tempdir().expect("a folder");
        std::fs::copy(
            root().join(&release).join(PROCESSING),
            dir.path().join(PROCESSING),
        )
        .expect("copies");
        let stored: Value = serde_json::from_str(&read(&release, PROCESSING)).expect("JSON");
        for key in stored.as_object().expect("an object").keys() {
            match reads_as_its_type(key, dir.path()) {
                None => problems.push(format!(
                    "{file}: {key} has no type check in compat_corpus.rs"
                )),
                Some(false) => problems.push(format!(
                    "{file}: {key} no longer reads as its type, so it counts as never set"
                )),
                Some(true) => {}
            }
        }
        // Saving one setting leaves every other one as it was.
        Setting::<Value>::new("compatCorpusProbe")
            .save(dir.path(), &Value::Bool(true))
            .expect("saves");
        let kept: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join(PROCESSING)).expect("reads"),
        )
        .expect("JSON");
        problems.extend(
            lost(PROCESSING, &stored, &kept)
                .into_iter()
                .map(|p| format!("{file}: saving another setting changed {p}")),
        );
    }
    assert!(problems.is_empty(), "{}", report(&problems));
}

#[test]
fn the_newest_processing_file_has_every_key() {
    let newest = releases().pop().expect("a release folder");
    let stored: Value = serde_json::from_str(&read(&newest, PROCESSING))
        .unwrap_or_else(|e| panic!("{newest}/{PROCESSING} does not read: {e}"));
    let declared = declared_processing_keys();
    assert!(
        declared.len() >= 10,
        "the scan of src/mods found only {declared:?}: has the way settings are declared changed?"
    );
    let missing: Vec<&String> = declared
        .iter()
        .filter(|key| stored.get(key.as_str()).is_none_or(Value::is_null))
        .collect();
    assert!(
        missing.is_empty(),
        "the newest fixture, {newest}/{PROCESSING}, does not cover the code of today: {missing:?}\n\
         Add a value for each to `processing` in compat_fixture.rs, and a type check to \
         `reads_as_its_type` in compat_corpus.rs, then write this version's folder with \
         `cargo test --lib mods::compat_fixture -- --ignored`."
    );
    let unknown: Vec<&String> = stored
        .as_object()
        .expect("an object")
        .keys()
        .filter(|key| !declared.contains(*key))
        .collect();
    assert!(
        unknown.is_empty(),
        "{newest}/{PROCESSING} holds {unknown:?}, which no setting declares any more"
    );
}
