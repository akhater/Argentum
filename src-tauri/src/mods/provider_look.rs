//! A provider look on a photo: a LUT made for that photo alone. Ours.
//!
//! The file sits beside its photo as `<photo>.<camera>.<look>.lut.png`
//! (`file_ops::look_file_for`). The edit holds no path, so a photo that moves,
//! or an edit pasted onto another photo, finds that photo's own file:
//!
//! - `lutPath` is exactly [`MARKER`]. The marker is the switch: anything that
//!   writes a real `lutPath` turns the look off, with no code of its own.
//! - `agProviderLook` ([`KEY`]) names the camera, the look, any options away
//!   from their default and the white balance its file was made for, if not the
//!   camera's, and keeps what the look replaced, so taking it off restores the
//!   rendering (src/argentum/looks/providerLook.ts).
//!
//! Every load of an edit's LUT goes through [`lut_of_edit`]. A path from an edit
//! is refused if it could point anywhere (`parse_lut_file`); this one is built
//! from the photo, two checked ids and a hash, so a photo on a network share or
//! in a folder with `..` in its name still finds its file. It needs no provider,
//! so a look stays on after its provider drops an option.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::{Value, json};
use tauri::State;

use crate::AppState;
use crate::exif_processing::peek_sidecar;
use crate::file_management::parse_virtual_path;
use crate::lut_processing::{self, Lut, get_or_load_lut};
use crate::mods::file_ops::{self, LookOptions, OwnWb};
use crate::mods::lut_cache::LutCache;
use crate::mods::raw_tone::ToneCurvePoint;

/// The `lutPath` of an edit with a provider look on.
pub const MARKER: &str = "provider-look";

/// The key in an edit that says which provider look is on.
pub const KEY: &str = "agProviderLook";

/// A provider look as an edit holds it, under `agProviderLook`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ProviderLook {
    pub camera: String,
    pub look: String,
    /// Its options away from their default. Edits saved before options have none.
    #[serde(default)]
    #[cfg_attr(test, ts(as = "Option<LookOptions>", optional))]
    pub options: LookOptions,
    /// Set while an option has it follow the photo's white balance. The photo
    /// keeps the file this names until a new one is ready, however the sliders
    /// have moved since.
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub own_wb: Option<OwnWb>,
    #[serde(default)]
    pub before: RenderingBefore,
}

impl ProviderLook {
    /// Its file beside `photo`, the photo's own path and not a virtual copy's.
    pub fn file(&self, photo: &Path) -> Option<PathBuf> {
        file_ops::look_file_for(
            photo,
            &self.camera,
            &self.look,
            &self.options,
            self.own_wb.as_ref(),
        )
    }
}

/// The rendering a provider look replaced on its photo.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", default)]
pub struct RenderingBefore {
    pub raw_tone_rendering: Option<String>,
    pub raw_tone_curve: Option<Vec<ToneCurvePoint>>,
    pub raw_tone_curve_mode: Option<String>,
}

/// The provider look `js` has on, or None: `lookOf` in providerLook.ts.
fn look_of(js: &Value) -> Option<ProviderLook> {
    if js["lutPath"].as_str() != Some(MARKER) {
        return None;
    }
    ProviderLook::deserialize(&js[KEY]).ok()
}

/// Whether `js` has a provider look on.
pub fn has_look(js: &Value) -> bool {
    look_of(js).is_some()
}

/// The file of the provider look `js` has on `photo`; None for an edit with
/// none, or one whose ids could not be part of a file name.
pub fn look_file(js: &Value, photo: &str) -> Option<PathBuf> {
    look_of(js)?.file(&parse_virtual_path(photo).0)
}

/// The look files the edits of `photo` use: its own and every virtual copy's,
/// which shares its files. An edit that does not read could use any of them.
pub fn files_in_use(photo: &Path) -> HashSet<PathBuf> {
    let (Some(folder), Some(photo_name)) = (
        photo.parent(),
        photo.file_name().and_then(|name| name.to_str()),
    ) else {
        return HashSet::new();
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        return HashSet::new();
    };
    let mut used = HashSet::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name
            .to_str()
            .is_some_and(|name| file_ops::is_sidecar_of(name, photo_name))
        {
            continue;
        }
        let Some(meta) = peek_sidecar(&entry.path()) else {
            return file_ops::look_files_of(photo).into_iter().collect();
        };
        used.extend(look_file(&meta.adjustments, &photo.to_string_lossy()));
    }
    used
}

/// The LUT an edit asks for: a provider look's file beside the photo, else
/// `lutPath`.
pub fn lut_of_edit(state: &State<AppState>, js: &Value, photo: &str) -> Option<Arc<Lut>> {
    let named = js["lutPath"].as_str()?;
    if named != MARKER {
        return get_or_load_lut(state, named).ok();
    }
    load_look_file(&state.lut_cache, &look_file(js, photo)?)
}

/// An enlargement of `photo` is another file, so the look stays on it with a
/// copy of the look file under its name. It holds `rawToneRendering` as it opens
/// (super_resolution.rs `carry_raw_look`), so that is what taking the look off
/// gives it.
pub fn carry_to_enlargement(adjustments: &mut Value, photo: &Path, enlarged: &Path) {
    let name_for = |path: &Path| look_file(adjustments, &path.to_string_lossy());
    let (Some(from), Some(to)) = (name_for(photo), name_for(enlarged)) else {
        return;
    };
    if let Err(e) = std::fs::copy(&from, &to) {
        log::warn!("Super resolution: the look file was not carried over: {e}");
    }
    adjustments[KEY]["before"] = json!({
        "rawToneRendering": adjustments["rawToneRendering"],
        "rawToneCurve": null,
        "rawToneCurveMode": null,
    });
}

/// Settings pasted onto a photo, which may be a photo the interface does not
/// have open: a LUT in them replaces the look on the photo, which gives back
/// the rendering it replaced. Call before the pasted keys go in, so a rendering
/// among them still wins.
pub fn give_way_to_paste(edit: &mut Value, pasted: &Value) {
    let brings_a_lut = pasted
        .get("lutPath")
        .is_some_and(|path| path.as_str() != Some(MARKER));
    if !brings_a_lut || edit["lutPath"].as_str() != Some(MARKER) {
        return;
    }
    let before = edit[KEY]["before"].take();
    for key in ["rawToneRendering", "rawToneCurve", "rawToneCurveMode"] {
        edit[key] = before[key].clone();
    }
    edit[KEY] = Value::Null;
}

/// The LUT in `file`, through the cache; None once the file is gone, whatever
/// the cache still holds.
fn load_look_file(cache: &Mutex<LutCache>, file: &Path) -> Option<Arc<Lut>> {
    if !file.is_file() {
        return None;
    }
    let key = file.to_string_lossy();
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(lut) = cache.get(&key) {
        return Some(lut);
    }
    let lut = Arc::new(lut_processing::parse_hald(image::open(file).ok()?).ok()?);
    cache.insert(key.into_owned(), Arc::clone(&lut));
    Some(lut)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::dng_standard::gpu::{centre, device, image_of, render_with_lut};
    use crate::white_balance::WhiteBalance;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("argentum-provider-look-{label}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    fn edit(camera: &str, look: &str) -> Value {
        json!({ "lutPath": MARKER, KEY: { "camera": camera, "look": look } })
    }

    /// A 16-step LUT that visibly changes a picture: its channels turned round.
    fn write_look_file(path: &Path) {
        let mut png = image::RgbImage::new(64, 64);
        for (i, pixel) in png.pixels_mut().enumerate() {
            let step = |n: usize| ((n % 16) * 17) as u8;
            let (r, g, b) = (step(i), step(i / 16), step(i / 256));
            *pixel = image::Rgb([g, b, r]);
        }
        png.save(path).expect("look file");
    }

    #[test]
    fn a_marked_edit_names_the_file_beside_its_photo() {
        assert_eq!(
            look_file(&edit("cam-1", "look2"), "/photos/IMG.CR3"),
            Some(PathBuf::from("/photos/IMG.CR3.cam-1.look2.lut.png"))
        );
    }

    /// Same photo, same white balance: a virtual copy shares the photo's file.
    #[test]
    fn a_virtual_copy_names_its_photos_file() {
        assert_eq!(
            look_file(&edit("cam1", "abc"), "/photos/IMG.CR3?vc=a1b2c3"),
            Some(PathBuf::from("/photos/IMG.CR3.cam1.abc.lut.png"))
        );
    }

    /// The name that does the look decides it: the file is the photo's, and
    /// an edit file from elsewhere cannot aim at any other.
    #[test]
    fn ids_that_could_not_be_a_look_files_name_name_nothing() {
        for (camera, look) in [
            ("..", "abc"),
            ("../..", "abc"),
            ("cam1", "a/b"),
            ("cam1", r"a\b"),
            ("Cam1", "abc"),
            ("", "abc"),
            ("cam1", ""),
            ("cam1", "-abc"),
            ("cam1", "a.b"),
            ("cam1", "abcdefghijklmnopq"),
        ] {
            assert_eq!(
                look_file(&edit(camera, look), "/photos/IMG.CR3"),
                None,
                "{camera} {look}"
            );
        }
    }

    fn edit_with(option: &str, value: bool) -> Value {
        let mut js = edit("cam1", "abc");
        js[KEY]["options"] = json!({ option: value });
        js
    }

    #[test]
    fn an_edit_with_an_option_names_another_file_than_one_without() {
        let plain = look_file(&edit("cam1", "abc"), "/photos/IMG.CR3").expect("a file");
        let with = look_file(&edit_with("optionA", true), "/photos/IMG.CR3").expect("a file");
        assert_ne!(plain, with);
        let name = with.file_name().and_then(|n| n.to_str()).expect("a name");
        assert!(file_ops::is_look_file_of(name, "IMG.CR3"), "{name}");
    }

    /// Saved before options existed: exactly today's file.
    #[test]
    fn an_edit_with_no_options_names_todays_file() {
        let mut js = edit("cam1", "abc");
        assert!(js[KEY].get("options").is_none());
        let today = Some(PathBuf::from("/photos/IMG.CR3.cam1.abc.lut.png"));
        assert_eq!(look_file(&js, "/photos/IMG.CR3"), today);
        js[KEY]["options"] = json!({});
        assert_eq!(look_file(&js, "/photos/IMG.CR3"), today);
    }

    #[test]
    fn a_virtual_copy_with_an_option_shares_its_photos_file() {
        let js = edit_with("optionA", true);
        assert_eq!(
            look_file(&js, "/photos/IMG.CR3?vc=a1b2c3"),
            look_file(&js, "/photos/IMG.CR3")
        );
    }

    /// An edit file from elsewhere: nothing to load, and nothing breaks.
    #[test]
    fn options_that_do_not_read_name_nothing() {
        for bad in [json!("optionA"), json!(7), json!({ "optionA": "yes" })] {
            let mut js = edit("cam1", "abc");
            js[KEY]["options"] = bad;
            assert_eq!(look_file(&js, "/photos/IMG.CR3"), None);
        }
    }

    fn with_own_wb() -> Value {
        let mut js = edit_with("optionW", true);
        js[KEY]["ownWb"] = json!({
            "option": "optionW",
            "wb": { "temperature": 4300, "tint": -12 },
        });
        js
    }

    #[test]
    fn an_edit_with_a_white_balance_of_its_own_names_its_own_file() {
        let plain = look_file(&edit("cam1", "abc"), "/photos/IMG.CR3");
        let own = look_file(&with_own_wb(), "/photos/IMG.CR3").expect("a file");
        assert_ne!(plain.as_deref(), Some(own.as_path()));
        let name = own.file_name().and_then(|n| n.to_str()).expect("a name");
        assert!(file_ops::is_look_file_of(name, "IMG.CR3"), "{name}");
    }

    /// The option on with no white balance of its own is today's file.
    #[test]
    fn an_edit_at_the_cameras_white_balance_names_todays_file() {
        let mut js = with_own_wb();
        js[KEY]["ownWb"]["wb"] = Value::Null;
        assert_eq!(
            look_file(&js, "/photos/IMG.CR3"),
            look_file(&edit("cam1", "abc"), "/photos/IMG.CR3")
        );
    }

    /// An edit file from elsewhere: nothing to load, and nothing breaks.
    #[test]
    fn a_white_balance_that_does_not_read_names_nothing() {
        for bad in [
            json!("warm"),
            json!({ "option": "optionW", "wb": { "temperature": "warm", "tint": 0 } }),
            json!({ "option": "optionW", "wb": { "temperature": 1.5, "tint": 0 } }),
            json!({ "wb": null }),
        ] {
            let mut js = edit("cam1", "abc");
            js[KEY]["ownWb"] = bad;
            assert_eq!(look_file(&js, "/photos/IMG.CR3"), None);
        }
    }

    #[test]
    fn a_virtual_copy_with_a_white_balance_of_its_own_keeps_it_in_use() {
        let dir = scratch("in-use-own-wb");
        let photo = dir.join("IMG.CR3");
        let name = photo.to_string_lossy();
        save_edit(&name, &edit("cam1", "abc"));
        save_edit(&format!("{name}?vc=a1b2c3"), &with_own_wb());
        assert_eq!(
            files_in_use(&photo),
            HashSet::from([
                dir.join("IMG.CR3.cam1.abc.lut.png"),
                look_file(&with_own_wb(), &name).expect("a file"),
            ])
        );
    }

    /// Through the app's own file operations, not its rules for them: each of
    /// these finds the file by its name on disk, and the edit that travels with
    /// the photo must then find it again.
    #[test]
    fn a_look_file_of_its_own_white_balance_travels_with_its_photo() {
        use crate::file_management::{
            duplicate_file_on_disk, move_files_on_disk, rename_files_on_disk,
        };
        let text = |path: &Path| path.to_string_lossy().into_owned();
        let found = |photo: &Path| {
            look_file(&with_own_wb(), &text(photo))
                .expect("a file")
                .is_file()
        };

        let dir = tempfile::tempdir().expect("folder");
        let (from, to) = (dir.path().join("from"), dir.path().join("to"));
        std::fs::create_dir_all(&from).expect("folder");
        std::fs::create_dir_all(&to).expect("folder");
        let photo = from.join("IMG.CR3");
        std::fs::write(&photo, "photo").expect("photo");
        save_edit(&text(&photo), &with_own_wb());
        write_look_file(&look_file(&with_own_wb(), &text(&photo)).expect("a file"));
        assert!(found(&photo));

        let copy = duplicate_file_on_disk(&text(&photo)).expect("a copy");
        assert!(found(Path::new(&copy)), "duplicated");
        assert!(found(&photo), "the duplicated one stays");

        move_files_on_disk(&[text(&photo)], &text(&to)).expect("moved");
        let moved = to.join("IMG.CR3");
        assert!(found(&moved), "moved");
        assert!(!found(&photo), "the moved one left");

        let (paths, _) = rename_files_on_disk(&[text(&moved)], "trip").expect("renamed");
        let renamed = Path::new(&paths[0]);
        assert!(found(renamed), "renamed");
        assert!(!found(&moved), "the old name left");
        assert_eq!(files_in_use(renamed).len(), 1);

        let trashed = file_ops::files_to_delete_with_associated(&[text(renamed)]);
        let file = look_file(&with_own_wb(), &text(renamed)).expect("a file");
        assert!(trashed.contains(&file), "{trashed:?}");
    }

    /// Saves `js` as the edit of `photo`, which a virtual copy's path names.
    fn save_edit(photo: &str, js: &Value) -> PathBuf {
        let sidecar = parse_virtual_path(photo).1;
        let text = json!({ "version": 1, "rating": 0, "adjustments": js }).to_string();
        std::fs::write(&sidecar, text).expect("sidecar");
        sidecar
    }

    #[test]
    fn the_photos_edit_uses_its_look_file() {
        let dir = scratch("in-use-own");
        let photo = dir.join("IMG.CR3");
        save_edit(&photo.to_string_lossy(), &edit("cam1", "abc"));
        assert_eq!(
            files_in_use(&photo),
            HashSet::from([dir.join("IMG.CR3.cam1.abc.lut.png")])
        );
    }

    /// A copy shares the photo's files, so its look is the photo's to keep,
    /// options and all.
    #[test]
    fn every_virtual_copys_edit_uses_its_look_file_too() {
        let dir = scratch("in-use-copies");
        let photo = dir.join("IMG.CR3");
        let name = photo.to_string_lossy();
        save_edit(&name, &edit("cam1", "abc"));
        save_edit(&format!("{name}?vc=a1b2c3"), &edit("cam2", "other"));
        let with_option = edit_with("optionA", true);
        save_edit(&format!("{name}?vc=d4e5f6"), &with_option);
        assert_eq!(
            files_in_use(&photo),
            HashSet::from([
                dir.join("IMG.CR3.cam1.abc.lut.png"),
                dir.join("IMG.CR3.cam2.other.lut.png"),
                look_file(&with_option, &name).expect("a file"),
            ])
        );
    }

    #[test]
    fn an_edit_with_no_provider_look_uses_no_look_file() {
        let dir = scratch("in-use-none");
        let photo = dir.join("IMG.CR3");
        save_edit(
            &photo.to_string_lossy(),
            &json!({ "lutPath": "film.cube", "exposure": 1.0 }),
        );
        assert!(files_in_use(&photo).is_empty());
    }

    #[test]
    fn another_photos_edits_use_nothing_of_this_ones() {
        let dir = scratch("in-use-other");
        let photo = dir.join("IMG.CR3");
        let jpeg = dir.join("IMG.CR3.jpg");
        save_edit(&jpeg.to_string_lossy(), &edit("cam1", "abc"));
        save_edit(
            &dir.join("OTHER.CR3").to_string_lossy(),
            &edit("cam1", "abc"),
        );
        assert!(files_in_use(&photo).is_empty());
    }

    #[test]
    fn a_photo_with_no_edit_uses_no_look_file() {
        let dir = scratch("in-use-no-edit");
        assert!(files_in_use(&dir.join("IMG.CR3")).is_empty());
    }

    /// There is no telling what it uses, so nothing of the photo's is spare.
    #[test]
    fn an_edit_that_does_not_read_could_use_any_look_file() {
        let dir = scratch("in-use-broken");
        let photo = dir.join("IMG.CR3");
        let file = dir.join("IMG.CR3.cam1.abc.lut.png");
        write_look_file(&file);
        let sidecar = parse_virtual_path(&photo.to_string_lossy()).1;
        std::fs::write(&sidecar, "half an edit").expect("sidecar");
        assert_eq!(files_in_use(&photo), HashSet::from([file]));
        assert_eq!(
            std::fs::read_dir(&dir).expect("folder").count(),
            2,
            "reads only"
        );
    }

    /// The file is all a look needs, so the render never asks what a provider
    /// offers: with no provider, or one that dropped the option, it still loads.
    #[test]
    fn a_look_with_an_option_loads_with_no_provider_at_all() {
        let dir = scratch("gone-option");
        let photo = dir.join("IMG.CR3");
        let js = edit_with("optionA", true);
        let file = look_file(&js, &photo.to_string_lossy()).expect("a file");
        assert_ne!(file, dir.join("IMG.CR3.cam1.abc.lut.png"));
        write_look_file(&file);
        let cache = Mutex::new(LutCache::default());
        assert!(load_look_file(&cache, &file).is_some());
    }

    #[test]
    fn only_the_exact_marker_counts() {
        let mut js = edit("cam1", "abc");
        for other in [json!("film.cube"), json!("Provider-Look"), Value::Null] {
            js["lutPath"] = other;
            assert_eq!(look_file(&js, "/photos/IMG.CR3"), None);
        }
        assert_eq!(
            look_file(&json!({ "lutPath": MARKER }), "/photos/IMG.CR3"),
            None
        );
    }

    /// The renderer takes either path to the same pixels: a marked edit gets
    /// the LUT a path to the file gets.
    #[test]
    fn a_marked_edit_gets_the_lut_its_file_holds() {
        let dir = scratch("same");
        let photo = dir.join("IMG.CR3");
        let file = dir.join("IMG.CR3.cam1.abc.lut.png");
        write_look_file(&file);

        let cache = Mutex::new(LutCache::default());
        let found = look_file(&edit("cam1", "abc"), &photo.to_string_lossy()).expect("a file");
        let marked = load_look_file(&cache, &found).expect("a LUT");
        let named = lut_processing::parse_lut_file(&file.to_string_lossy()).expect("a LUT");
        assert_eq!(marked.size, named.size);
        assert_eq!(marked.data, named.data);
    }

    /// `parse_lut_file`'s refusals are for paths taken from an edit, and this
    /// one is not.
    #[test]
    fn a_look_file_loads_where_an_edits_path_would_be_refused() {
        let dir = scratch("dots").join("trip..2026");
        std::fs::create_dir_all(&dir).expect("folder");
        let file = dir.join("IMG.CR3.cam1.abc.lut.png");
        write_look_file(&file);
        assert!(lut_processing::parse_lut_file(&file.to_string_lossy()).is_err());
        let cache = Mutex::new(LutCache::default());
        assert!(load_look_file(&cache, &file).is_some());
    }

    /// The strip says the file is missing, so the render must not still show it.
    #[test]
    fn a_look_file_gone_gives_no_lut_even_once_cached() {
        let dir = scratch("gone");
        let file = dir.join("IMG.CR3.cam1.abc.lut.png");
        write_look_file(&file);
        let cache = Mutex::new(LutCache::default());
        assert!(load_look_file(&cache, &file).is_some());
        std::fs::remove_file(&file).expect("delete");
        assert!(load_look_file(&cache, &file).is_none());
    }

    #[test]
    fn a_broken_look_file_gives_no_lut() {
        let dir = scratch("broken");
        let file = dir.join("IMG.CR3.cam1.abc.lut.png");
        std::fs::write(&file, "not a picture").expect("file");
        let cache = Mutex::new(LutCache::default());
        assert!(load_look_file(&cache, &file).is_none());
    }

    fn with_look_on() -> Value {
        let mut js = edit("cam1", "abc");
        js[KEY]["before"] = json!({
            "rawToneRendering": "baseCurve",
            "rawToneCurve": [{ "x": 0.0, "y": 0.0 }, { "x": 255.0, "y": 255.0 }],
            "rawToneCurveMode": "baseCurve",
        });
        js["rawToneRendering"] = json!("linear");
        js
    }

    #[test]
    fn a_pasted_lut_gives_the_photo_its_rendering_back() {
        let mut js = with_look_on();
        give_way_to_paste(&mut js, &json!({ "lutPath": "film.cube" }));
        assert_eq!(js["rawToneRendering"], "baseCurve");
        assert_eq!(js["rawToneCurveMode"], "baseCurve");
        assert_eq!(js["rawToneCurve"].as_array().expect("curve").len(), 2);
        assert_eq!(js[KEY], Value::Null);
    }

    #[test]
    fn a_paste_with_no_lut_leaves_the_look_on() {
        let mut js = with_look_on();
        let before = js.clone();
        give_way_to_paste(&mut js, &json!({ "exposure": 1.0 }));
        give_way_to_paste(&mut js, &json!({ "lutPath": MARKER }));
        assert_eq!(js, before);
    }

    #[test]
    fn a_paste_onto_a_photo_with_no_look_changes_nothing() {
        let mut js = json!({ "lutPath": "old.cube", "rawToneRendering": "default" });
        let before = js.clone();
        give_way_to_paste(&mut js, &json!({ "lutPath": "film.cube" }));
        assert_eq!(js, before);
    }

    #[test]
    fn an_enlargement_keeps_the_look_under_its_own_name() {
        let dir = scratch("enlarge");
        let (photo, enlarged) = (dir.join("IMG.CR3"), dir.join("IMG_2x.tif"));
        write_look_file(&dir.join("IMG.CR3.cam1.abc.lut.png"));

        let mut js = edit("cam1", "abc");
        js[KEY]["before"] = json!({ "rawToneRendering": "dngStandard" });
        js["rawToneRendering"] = json!("default");
        carry_to_enlargement(&mut js, &photo, &enlarged);

        let found = look_file(&js, &enlarged.to_string_lossy()).expect("a file");
        assert!(found.is_file());
        assert_eq!(js[KEY]["before"]["rawToneRendering"], "default");
    }

    #[test]
    fn an_enlargement_keeps_the_look_of_its_options() {
        let dir = scratch("enlarge-option");
        let (photo, enlarged) = (dir.join("IMG.CR3"), dir.join("IMG_2x.tif"));
        let mut js = edit_with("optionA", true);
        let file = look_file(&js, &photo.to_string_lossy()).expect("a file");
        assert_ne!(file, dir.join("IMG.CR3.cam1.abc.lut.png"));
        write_look_file(&file);
        carry_to_enlargement(&mut js, &photo, &enlarged);
        let found = look_file(&js, &enlarged.to_string_lossy()).expect("a file");
        assert!(found.is_file());
        assert_eq!(js[KEY]["options"], json!({ "optionA": true }));
    }

    #[test]
    fn an_enlargement_of_a_photo_with_no_look_gets_no_file() {
        let dir = scratch("enlarge-none");
        let mut js = json!({ "lutPath": "film.cube", "rawToneRendering": "default" });
        let before = js.clone();
        carry_to_enlargement(&mut js, &dir.join("IMG.CR3"), &dir.join("IMG_2x.tif"));
        assert_eq!(js, before);
        assert_eq!(std::fs::read_dir(&dir).expect("folder").count(), 0);
    }

    /// A load that skips `lut_of_edit` would hand the marker to the file
    /// parser, which finds no such file, and the photo would show no look in
    /// that one place. These are every file that renders an edit.
    #[test]
    fn every_render_finds_its_lut_through_the_edit() {
        for (name, source) in [
            ("lib.rs", include_str!("../lib.rs")),
            (
                "export_processing.rs",
                include_str!("../export_processing.rs"),
            ),
            ("file_management.rs", include_str!("../file_management.rs")),
            ("mods/commands.rs", include_str!("commands.rs")),
        ] {
            assert!(!source.contains("get_or_load_lut"), "{name}");
        }
    }

    /// Through the real shader: `cargo test --lib provider_look -- --ignored`.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn a_marked_edit_renders_as_one_naming_the_same_file() {
        const PATCHES: [[f32; 3]; 4] = [
            [0.18, 0.18, 0.18],
            [0.9, 0.9, 0.9],
            [0.6, 0.05, 0.03],
            [0.04, 0.12, 0.7],
        ];
        let dir = scratch("render");
        let photo = dir.join("IMG.CR3");
        let file = dir.join("IMG.CR3.cam1.abc.lut.png");
        write_look_file(&file);

        let ctx = device().expect("no wgpu adapter");
        let render = |js: Value, lut: Arc<Lut>| {
            let out = render_with_lut(
                &ctx,
                &image_of(&PATCHES),
                js,
                WhiteBalance::reference(),
                Some(lut),
            );
            (0..PATCHES.len())
                .map(|i| centre(&out, i))
                .collect::<Vec<_>>()
        };
        let settings = json!({
            "rawToneRendering": "linear",
            "agSharpen": { "capture": false, "amount": 0 },
            "lutIntensity": 100,
            "lutIsSceneReferred": false,
            "agLutDeclaredInput": true,
        });

        let mut named_edit = settings.clone();
        named_edit["lutPath"] = json!(file.to_string_lossy());
        let named = lut_processing::parse_lut_file(&file.to_string_lossy()).expect("a LUT");

        let mut marked_edit = settings.clone();
        marked_edit["lutPath"] = json!(MARKER);
        marked_edit[KEY] = json!({ "camera": "cam1", "look": "abc" });
        let cache = Mutex::new(LutCache::default());
        let found = look_file(&marked_edit, &photo.to_string_lossy()).expect("a file");
        let marked = load_look_file(&cache, &found).expect("a LUT");

        let without = render(settings, Arc::new(named.clone()));
        let want = render(named_edit, Arc::new(named));
        assert_ne!(want, without, "the LUT did nothing, so this proves nothing");
        assert_eq!(render(marked_edit, marked), want);
    }
}
