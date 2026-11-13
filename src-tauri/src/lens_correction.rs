use crate::AppState;
use crate::mods::lens_shot::LensShot;
use fuzzy_matcher::FuzzyMatcher;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use tauri::{Manager, State};
use walkdir::WalkDir;

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Distortion {
    #[serde(rename = "@model")]
    pub model: String,
    #[serde(rename = "@focal")]
    pub focal: f32,
    #[serde(rename = "@real-focal")]
    pub real_focal: Option<f32>,
    #[serde(rename = "@k1")]
    pub k1: Option<f32>,
    #[serde(rename = "@k2")]
    pub k2: Option<f32>,
    #[serde(rename = "@k3")]
    pub k3: Option<f32>,
    #[serde(rename = "@a")]
    pub a: Option<f32>,
    #[serde(rename = "@b")]
    pub b: Option<f32>,
    #[serde(rename = "@c")]
    pub c: Option<f32>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Tca {
    #[serde(rename = "@model")]
    pub model: String,
    #[serde(rename = "@focal")]
    pub focal: f32,
    #[serde(rename = "@vr")]
    pub vr: Option<f32>,
    #[serde(rename = "@vb")]
    pub vb: Option<f32>,
    #[serde(rename = "@cr")]
    pub cr: Option<f32>,
    #[serde(rename = "@cb")]
    pub cb: Option<f32>,
    #[serde(rename = "@br")]
    pub br: Option<f32>,
    #[serde(rename = "@bb")]
    pub bb: Option<f32>,
    // Argentum: the linear model's two terms (`<tca model="linear" kr kb>`).
    #[serde(rename = "@kr")]
    pub kr: Option<f32>,
    #[serde(rename = "@kb")]
    pub kb: Option<f32>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Vignetting {
    #[serde(rename = "@model")]
    pub model: String,
    #[serde(rename = "@focal")]
    pub focal: f32,
    #[serde(rename = "@aperture")]
    pub aperture: f32,
    #[serde(rename = "@distance")]
    pub distance: Option<f32>,
    #[serde(rename = "@k1")]
    pub k1: Option<f32>,
    #[serde(rename = "@k2")]
    pub k2: Option<f32>,
    #[serde(rename = "@k3")]
    pub k3: Option<f32>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum CalibrationElement {
    Distortion(Distortion),
    Tca(Tca),
    Vignetting(Vignetting),
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Calibration {
    #[serde(rename = "$value", default)]
    pub elements: Vec<CalibrationElement>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Focal {
    #[serde(rename = "@value")]
    pub value: Option<f32>,
    #[serde(rename = "@min")]
    pub min: Option<f32>,
    #[serde(rename = "@max")]
    pub max: Option<f32>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Aperture {
    #[serde(rename = "@min")]
    pub min: Option<f32>,
    #[serde(rename = "@max")]
    pub max: Option<f32>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Lens {
    #[serde(default)]
    pub maker: Vec<MultiName>,
    #[serde(default)]
    pub model: Vec<MultiName>,
    #[serde(default)]
    pub mount: Vec<String>,
    pub cropfactor: Option<f32>,
    pub calibration: Option<Calibration>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub focal: Option<Focal>,
    pub aspect_ratio: Option<String>,
    pub center: Option<String>,
    pub compat: Option<String>,
    pub notes: Option<String>,
    pub aperture: Option<Aperture>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Camera {
    pub maker: Vec<MultiName>,
    pub model: Vec<MultiName>,
    pub mount: String,
    pub cropfactor: f32,
}

/// Argentum: a Lensfun `<mount>`, with the mounts whose lenses fit a camera of
/// it (`<compat>`, one way), for `mods/lens_match.rs`.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Mount {
    // Some mounts carry translated names too (slr-pentax.xml).
    pub name: Vec<MultiName>,
    #[serde(default)]
    pub compat: Vec<String>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct LensDatabase {
    // Argentum: read for the strict matcher (mods/lens_match.rs).
    #[serde(rename = "mount", default)]
    pub mounts: Vec<Mount>,
    #[serde(rename = "camera", default)]
    pub cameras: Vec<Camera>,
    #[serde(rename = "lens", default)]
    pub lenses: Vec<Lens>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct MultiName {
    #[serde(rename = "@lang")]
    lang: Option<String>,
    #[serde(rename = "$value")]
    value: String,
}

/// Argentum: the untranslated name of `names`, Lensfun's own (the one without a
/// `lang`), which is the name a file's EXIF writes.
pub fn untranslated(names: &[MultiName]) -> &str {
    names
        .iter()
        .find(|n| n.lang.is_none())
        .or_else(|| names.first())
        .map_or("", |n| n.value.trim())
}

#[derive(Serialize)]
pub struct LensDistortionParams {
    // Argentum: crate-wide for mods/lens_units.rs and mods/lens_frame.rs, which put
    // them in the renderer's units and on the photo's frame.
    pub(crate) k1: f64,
    pub(crate) k2: f64,
    pub(crate) k3: f64,
    pub(crate) model: u32,
    pub(crate) tca_vr: f64,
    pub(crate) tca_vb: f64,
    // Argentum: Lensfun's poly3 lateral CA terms of the radius (red and blue
    // scale by v + c·r + b·r²), in the renderer's units (mods/lens_units.rs).
    pub(crate) tca_cr: f64,
    pub(crate) tca_cb: f64,
    pub(crate) tca_br: f64,
    pub(crate) tca_bb: f64,
    pub(crate) vig_k1: f64,
    pub(crate) vig_k2: f64,
    pub(crate) vig_k3: f64,
}

fn strip_maker_prefix(name: &str, maker: &str) -> String {
    if name.to_lowercase().starts_with(&maker.to_lowercase())
        && let Some(rest) = name.get(maker.len()..)
    {
        let trimmed = rest.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    name.to_string()
}

fn pick_name(names: &[MultiName], fallback: &str) -> String {
    names
        .iter()
        .find(|n| n.lang.as_deref() == Some("en"))
        .or_else(|| names.first())
        .map(|n| n.value.clone())
        .unwrap_or_else(|| fallback.to_string())
}

fn any_name_matches(names: &[MultiName], needle: &str) -> bool {
    names
        .iter()
        .any(|n| n.value.trim().eq_ignore_ascii_case(needle))
}

impl Camera {
    pub fn get_maker(&self) -> String {
        pick_name(&self.maker, "Misc")
    }

    pub fn get_model(&self) -> String {
        pick_name(&self.model, "Unknown Model")
    }
}

impl Lens {
    pub fn get_full_model_name(&self) -> String {
        self.model
            .iter()
            .find(|m| m.lang.as_deref() == Some("en"))
            .or_else(|| self.model.first())
            .map(|m| m.value.clone())
            .unwrap_or_else(|| "Unknown Model".to_string())
    }

    pub fn get_canonical_model_name(&self) -> String {
        self.model
            .iter()
            .find(|m| m.lang.is_none())
            .or_else(|| self.model.first())
            .map(|m| m.value.clone())
            .unwrap_or_else(|| "Unknown Model".to_string())
    }

    pub fn get_name(&self) -> String {
        let raw_name = self.get_full_model_name();
        let maker = self.get_maker();

        if raw_name.to_lowercase().starts_with(&maker.to_lowercase())
            && let Some(rest) = raw_name.get(maker.len()..)
        {
            let stripped = rest.trim();
            if !stripped.is_empty() {
                return stripped.to_string();
            }
        }

        raw_name
    }

    pub fn get_maker(&self) -> String {
        self.maker
            .iter()
            .find(|m| m.lang.as_deref() == Some("en"))
            .or_else(|| self.maker.first())
            .map(|m| m.value.clone())
            .unwrap_or_else(|| "Misc".to_string())
    }

    pub fn get_display_name(&self, all_maker_lenses: &[&Lens]) -> String {
        let my_short = self.get_name();
        let short_count = all_maker_lenses
            .iter()
            .filter(|l| l.get_name() == my_short)
            .count();

        if short_count <= 1 {
            return my_short;
        }

        let maker = self.get_maker();
        let my_canonical_short = strip_maker_prefix(&self.get_canonical_model_name(), &maker);

        let canonical_short_count = all_maker_lenses
            .iter()
            .filter(|l| {
                strip_maker_prefix(&l.get_canonical_model_name(), &l.get_maker())
                    == my_canonical_short
            })
            .count();

        if canonical_short_count <= 1 {
            return my_canonical_short;
        }

        let my_canonical = self.get_canonical_model_name();
        let canonical_count = all_maker_lenses
            .iter()
            .filter(|l| l.get_canonical_model_name() == my_canonical)
            .count();

        if canonical_count <= 1 {
            return my_canonical;
        }

        if let Some(cf) = self.cropfactor {
            format!("{} (crop {:.1}x)", my_canonical_short, cf)
        } else {
            my_canonical_short
        }
    }

    pub fn get_distortion_params(
        &self,
        focal_length: f32,
        aperture: Option<f32>,
        distance: Option<f32>,
    ) -> Option<LensDistortionParams> {
        let cal = self.calibration.as_ref()?;

        let distortions: Vec<&Distortion> = cal
            .elements
            .iter()
            .filter_map(|e| {
                if let CalibrationElement::Distortion(d) = e {
                    Some(d)
                } else {
                    None
                }
            })
            .collect();

        let tcas: Vec<&Tca> = cal
            .elements
            .iter()
            .filter_map(|e| {
                if let CalibrationElement::Tca(t) = e {
                    Some(t)
                } else {
                    None
                }
            })
            .collect();

        let mut vignettings: Vec<&Vignetting> = cal
            .elements
            .iter()
            .filter_map(|e| {
                if let CalibrationElement::Vignetting(v) = e {
                    Some(v)
                } else {
                    None
                }
            })
            .collect();

        // Argentum: between focal lengths as Lensfun interpolates (mods/lens_interp.rs):
        // the rows of the first model met, in the file's order.
        let first_model = |models: Vec<&str>| models.first().map(|m| m.to_string());
        let dist_model = first_model(
            distortions
                .iter()
                .map(|d| d.model.as_str())
                .filter(|m| *m != "none")
                .collect(),
        );
        let dist_rows: Vec<(f32, [f64; 3])> = distortions
            .iter()
            .filter(|d| Some(d.model.as_str()) == dist_model.as_deref())
            .map(|d| {
                let (k1, k2, k3, _) = extract_dist_params(d);
                (d.focal, [k1, k2, k3])
            })
            .collect();
        let model = distortions
            .iter()
            .find(|d| Some(d.model.as_str()) == dist_model.as_deref())
            .map_or(0, |d| extract_dist_params(d).3);
        let [k1, k2, k3] = crate::mods::lens_interp::at_focal(&dist_rows, focal_length, [true; 3])
            .unwrap_or([0.0; 3]);

        let tca_model = first_model(tcas.iter().map(|t| t.model.as_str()).collect());
        let tca_rows: Vec<(f32, [f64; 6])> = tcas
            .iter()
            .filter(|t| Some(t.model.as_str()) == tca_model.as_deref())
            .map(|t| (t.focal, extract_tca_params(t)))
            .collect();
        // v does not scale with the focal length; c and b do (`__parameter_scales`).
        let [tca_vr, tca_vb, tca_cr, tca_cb, tca_br, tca_bb] = crate::mods::lens_interp::at_focal(
            &tca_rows,
            focal_length,
            [false, false, true, true, true, true],
        )
        .unwrap_or(NO_TCA);

        let (vig_k1, vig_k2, vig_k3) = if vignettings.is_empty() {
            (0.0, 0.0, 0.0)
        } else {
            let target_aperture = aperture.unwrap_or(3.5);
            let target_distance = distance.unwrap_or(1000.0);

            vignettings.sort_by(|a, b| a.focal.partial_cmp(&b.focal).unwrap_or(Ordering::Equal));

            let find_best_vig = |items: &[&Vignetting]| -> (f64, f64, f64) {
                let best_aperture_item = items.iter().min_by(|a, b| {
                    (a.aperture - target_aperture)
                        .abs()
                        .partial_cmp(&(b.aperture - target_aperture).abs())
                        .unwrap_or(Ordering::Equal)
                });
                if let Some(best_ap) = best_aperture_item {
                    let candidates: Vec<&&Vignetting> = items
                        .iter()
                        .filter(|x| (x.aperture - best_ap.aperture).abs() < 0.01)
                        .collect();
                    let best_dist = candidates.into_iter().min_by(|a, b| {
                        let da = a.distance.unwrap_or(1000.0);
                        let db = b.distance.unwrap_or(1000.0);
                        (da - target_distance)
                            .abs()
                            .partial_cmp(&(db - target_distance).abs())
                            .unwrap_or(Ordering::Equal)
                    });
                    extract_vig_params(best_dist.unwrap_or(best_ap))
                } else {
                    (0.0, 0.0, 0.0)
                }
            };

            if focal_length <= vignettings[0].focal + 0.01 {
                let group: Vec<&Vignetting> = vignettings
                    .iter()
                    .filter(|x| (x.focal - vignettings[0].focal).abs() < 0.01)
                    .copied()
                    .collect();
                find_best_vig(&group)
            } else if focal_length >= vignettings.last().unwrap().focal - 0.01 {
                let last_focal = vignettings.last().unwrap().focal;
                let group: Vec<&Vignetting> = vignettings
                    .iter()
                    .filter(|x| (x.focal - last_focal).abs() < 0.01)
                    .copied()
                    .collect();
                find_best_vig(&group)
            } else {
                let mut res = (0.0, 0.0, 0.0);
                let unique_focals: Vec<f32> = {
                    let mut f: Vec<f32> = vignettings.iter().map(|v| v.focal).collect();
                    f.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
                    f.dedup_by(|a, b| (*a - *b).abs() < 0.01);
                    f
                };
                for pair in unique_focals.windows(2) {
                    let (f1, f2) = (pair[0], pair[1]);
                    if focal_length >= f1 && focal_length <= f2 {
                        let group1: Vec<&Vignetting> = vignettings
                            .iter()
                            .filter(|x| (x.focal - f1).abs() < 0.01)
                            .copied()
                            .collect();
                        let group2: Vec<&Vignetting> = vignettings
                            .iter()
                            .filter(|x| (x.focal - f2).abs() < 0.01)
                            .copied()
                            .collect();

                        let p1 = find_best_vig(&group1);
                        let p2 = find_best_vig(&group2);

                        let range = f2 - f1;
                        if range.abs() > 0.01 {
                            let t = (focal_length - f1) / range;
                            res = (
                                p1.0 + t as f64 * (p2.0 - p1.0),
                                p1.1 + t as f64 * (p2.1 - p1.1),
                                p1.2 + t as f64 * (p2.2 - p1.2),
                            );
                        } else {
                            res = p1;
                        }
                        break;
                    }
                }
                res
            }
        };

        Some(LensDistortionParams {
            k1,
            k2,
            k3,
            model,
            tca_vr,
            tca_vb,
            tca_cr,
            tca_cb,
            tca_br,
            tca_bb,
            vig_k1,
            vig_k2,
            vig_k3,
        })
    }
}

fn extract_dist_params(dist: &Distortion) -> (f64, f64, f64, u32) {
    match dist.model.as_str() {
        "poly3" | "poly5" => (
            dist.k1.unwrap_or(0.0) as f64,
            dist.k2.unwrap_or(0.0) as f64,
            dist.k3.unwrap_or(0.0) as f64,
            0,
        ),
        "ptlens" => {
            let a = dist.a.unwrap_or(0.0) as f64;
            let b = dist.b.unwrap_or(0.0) as f64;
            let c = dist.c.unwrap_or(0.0) as f64;
            (a, b, c, 1)
        }
        _ => (0.0, 0.0, 0.0, 0),
    }
}

/// A lens with no lateral CA calibration: red and blue at green's scale.
const NO_TCA: [f64; 6] = [1.0, 1.0, 0.0, 0.0, 0.0, 0.0];

/// A TCA row's terms: vr, vb (or the linear model's kr, kb), cr, cb, br, bb.
fn extract_tca_params(tca: &Tca) -> [f64; 6] {
    let term = |v: Option<f32>, none: f32| v.unwrap_or(none) as f64;
    [
        term(tca.vr.or(tca.kr), 1.0),
        term(tca.vb.or(tca.kb), 1.0),
        term(tca.cr, 0.0),
        term(tca.cb, 0.0),
        term(tca.br, 0.0),
        term(tca.bb, 0.0),
    ]
}

fn extract_vig_params(vig: &Vignetting) -> (f64, f64, f64) {
    (
        vig.k1.unwrap_or(0.0) as f64,
        vig.k2.unwrap_or(0.0) as f64,
        vig.k3.unwrap_or(0.0) as f64,
    )
}

fn lenses_for_maker<'a>(db: &'a LensDatabase, maker: &str) -> Vec<&'a Lens> {
    db.lenses
        .iter()
        .filter(|l| l.get_maker() == maker)
        .collect()
}

pub fn load_lensfun_db(app_handle: &tauri::AppHandle) -> LensDatabase {
    let resource_path = app_handle
        .path()
        .resolve("lensfun_db", tauri::path::BaseDirectory::Resource)
        .expect("failed to resolve lensfun_db directory");

    if !resource_path.exists() {
        log::error!("Lensfun DB directory not found at: {:?}", resource_path);
        return LensDatabase {
            mounts: Vec::new(),
            cameras: Vec::new(),
            lenses: Vec::new(),
        };
    }

    let combined_db = read_lensfun_dir(&resource_path);

    log::info!(
        "Loaded {} lenses and {} cameras from Lensfun database.",
        combined_db.lenses.len(),
        combined_db.cameras.len()
    );
    combined_db
}

/// Every Lensfun XML file under `dir`, read into one database. A file that fails
/// to read or parse is logged and left out.
pub fn read_lensfun_dir(dir: &std::path::Path) -> LensDatabase {
    let mut combined_db = LensDatabase {
        mounts: Vec::new(),
        cameras: Vec::new(),
        lenses: Vec::new(),
    };
    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "xml"))
    {
        let path = entry.path();
        log::info!("Processing file: {:?}", path);
        match fs::read_to_string(path) {
            Ok(xml_content) => match quick_xml::de::from_str::<LensDatabase>(&xml_content) {
                Ok(mut db) => {
                    combined_db.mounts.append(&mut db.mounts);
                    combined_db.cameras.append(&mut db.cameras);
                    combined_db.lenses.append(&mut db.lenses);
                }
                Err(e) => {
                    log::error!("Failed to parse Lensfun XML file {:?}: {}", path, e);
                }
            },
            Err(e) => log::error!("Failed to read Lensfun XML file {:?}: {}", path, e),
        }
    }
    combined_db
}

#[tauri::command]
pub fn get_lensfun_makers(state: State<AppState>) -> Result<Vec<String>, String> {
    let db_guard = state
        .lens_db
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?;
    if let Some(db) = &*db_guard {
        let mut makers: Vec<String> = db.lenses.iter().map(|lens| lens.get_maker()).collect();
        makers.sort_unstable();
        makers.dedup();
        Ok(makers)
    } else {
        Err("Lens database not loaded".to_string())
    }
}

#[tauri::command]
pub fn get_lensfun_lenses_for_maker(
    maker: String,
    state: State<AppState>,
) -> Result<Vec<String>, String> {
    let db_guard = state
        .lens_db
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?;
    if let Some(db) = &*db_guard {
        let maker_lenses = lenses_for_maker(db, &maker);

        let mut models: Vec<String> = maker_lenses
            .iter()
            .map(|lens| lens.get_display_name(&maker_lenses))
            .collect();
        models.sort_unstable();
        models.dedup();
        Ok(models)
    } else {
        Err("Lens database not loaded".to_string())
    }
}

pub fn find_lens_by_camera_mount<'a>(
    db: &'a LensDatabase,
    camera_maker: &str,
    camera_model: &str,
) -> Option<&'a Lens> {
    let clean_maker = camera_maker.trim().trim_matches('"');
    let clean_model = camera_model.trim().trim_matches('"');

    if clean_maker.is_empty() || clean_model.is_empty() {
        return None;
    }

    let mounts: Vec<&str> = db
        .cameras
        .iter()
        .filter(|c| {
            any_name_matches(&c.maker, clean_maker) && any_name_matches(&c.model, clean_model)
        })
        .map(|c| c.mount.trim())
        .filter(|m| !m.is_empty())
        .collect();

    if mounts.is_empty() {
        return None;
    }

    let mut hits: Vec<&Lens> = Vec::new();
    for lens in &db.lenses {
        if lens
            .mount
            .iter()
            .any(|m| mounts.iter().any(|c| m.trim().eq_ignore_ascii_case(c)))
        {
            hits.push(lens);
            if hits.len() > 1 {
                log::info!(
                    "Mount fallback for {} {}: more than one lens on mount, giving up.",
                    clean_maker,
                    clean_model
                );
                return None;
            }
        }
    }

    let lens = hits.into_iter().next()?;
    log::info!(
        "Mount fallback for {} {}: using fixed lens {} {}",
        clean_maker,
        clean_model,
        lens.get_maker(),
        lens.get_full_model_name()
    );
    Some(lens)
}

pub(crate) fn lens_result(db: &LensDatabase, lens: &Lens) -> (String, String) {
    let lens_maker = lens.get_maker();
    let maker_lenses = lenses_for_maker(db, &lens_maker);
    (lens_maker, lens.get_display_name(&maker_lenses))
}

/// The lens a photo was taken with: its maker and display name. `shot` is what
/// the photo records of its focal length (`lens_shot::MatchShot`).
pub fn find_best_lens_match(
    db: &LensDatabase,
    maker: &str,
    model: &str,
    camera_model: &str,
    shot: &crate::mods::lens_shot::MatchShot,
) -> Option<(String, String)> {
    // Argentum: when the strict matcher finds exactly one lens, it decides;
    // otherwise the matcher below answers as it always has (mods/lens_match.rs).
    if let Some(lens) = crate::mods::lens_match::confident(db, maker, model, camera_model, shot)
    {
        return Some(lens_result(db, lens));
    }
    // Argentum: never a lens that cannot be on the camera (mods/lens_match.rs).
    loose_lens_match(db, maker, model, camera_model).filter(|(lens_maker, display)| {
        lens_named(db, lens_maker, display).is_none_or(|lens| {
            crate::mods::lens_match::can_be_on(db, maker, camera_model, lens)
        })
    })
}

/// The matcher as it was before the strict one (`mods/lens_match.rs`): kept as
/// the answer wherever that one is not sure.
pub fn loose_lens_match(
    db: &LensDatabase,
    maker: &str,
    model: &str,
    camera_model: &str,
) -> Option<(String, String)> {
    let clean_model = model.trim().trim_matches('"');

    if let Some(hit) = crate::mods::lens_crop::match_for_camera(db, maker, clean_model, camera_model)
    {
        return Some(hit);
    }

    if !clean_model.is_empty()
        && let Some(hit) = find_lens_by_fuzzy_model(db, maker, clean_model)
    {
        return Some(hit);
    }

    find_lens_by_camera_mount(db, maker, camera_model).map(|lens| lens_result(db, lens))
}

fn find_lens_by_fuzzy_model(
    db: &LensDatabase,
    maker: &str,
    model: &str,
) -> Option<(String, String)> {
    let clean_maker = maker.trim().trim_matches('"').to_string();
    let clean_model = model.trim().trim_matches('"').to_string();
    let matcher = fuzzy_matcher::skim::SkimMatcherV2::default().ignore_case();

    let lenses_from_maker: Vec<&Lens> = db
        .lenses
        .iter()
        .filter(|lens| lens.get_maker().eq_ignore_ascii_case(&clean_maker))
        .collect();

    if !lenses_from_maker.is_empty() {
        let best_match = lenses_from_maker
            .iter()
            .filter_map(|lens| {
                let english_name = lens.get_full_model_name();
                let canonical_name = lens.get_canonical_model_name();

                let score_english = matcher
                    .fuzzy_match(&english_name, &clean_model)
                    .unwrap_or(0);
                let score_canonical = matcher
                    .fuzzy_match(&canonical_name, &clean_model)
                    .unwrap_or(0);
                let score = score_english.max(score_canonical);

                if score > 0 {
                    let best_name = if score_canonical > score_english {
                        &canonical_name
                    } else {
                        &english_name
                    };
                    let length_penalty =
                        (best_name.len() as i64 - clean_model.len() as i64).max(0) / 2;
                    let adjusted_score = score - length_penalty;
                    Some((adjusted_score, *lens))
                } else {
                    None
                }
            })
            .max_by_key(|(score, _)| *score);

        if let Some((_, best_lens)) = best_match {
            return Some((
                best_lens.get_maker(),
                best_lens.get_display_name(&lenses_from_maker),
            ));
        }
    }

    let best_match_fallback = db
        .lenses
        .iter()
        .filter_map(|lens| {
            let english_name = lens.get_full_model_name();
            let canonical_name = lens.get_canonical_model_name();

            let score_english = matcher
                .fuzzy_match(&english_name, &clean_model)
                .unwrap_or(0);
            let score_canonical = matcher
                .fuzzy_match(&canonical_name, &clean_model)
                .unwrap_or(0);
            let score = score_english.max(score_canonical);

            if score > 0 { Some((score, lens)) } else { None }
        })
        .max_by_key(|(score, _): &(i64, _)| *score);

    if let Some((_, best_lens)) = best_match_fallback {
        let lens_maker = best_lens.get_maker();
        let maker_lenses = lenses_for_maker(db, &lens_maker);
        return Some((lens_maker, best_lens.get_display_name(&maker_lenses)));
    }

    None
}

#[tauri::command]
pub fn autodetect_lens(
    maker: String,
    model: String,
    camera_model: Option<String>,
    exif: Option<HashMap<String, String>>,
    state: tauri::State<AppState>,
) -> Result<Option<(String, String)>, String> {
    let db_guard = state
        .lens_db
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?;
    if let Some(db) = &*db_guard {
        Ok(find_best_lens_match(
            db,
            &maker,
            &model,
            camera_model.as_deref().unwrap_or(""),
            &exif
                .as_ref()
                .map(crate::mods::lens_shot::MatchShot::from_exif)
                .unwrap_or_default(),
        ))
    } else {
        Ok(None)
    }
}

#[tauri::command]
pub fn get_lens_distortion_params(
    maker: String,
    model: String,
    exif: HashMap<String, String>,
    state: State<AppState>,
) -> Result<Option<LensDistortionParams>, String> {
    let db_guard = state
        .lens_db
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?;
    Ok(db_guard
        .as_ref()
        .and_then(|db| resolve_lens_params(db, &maker, &model, &exif)))
}

pub fn resolve_lens_params(
    db: &LensDatabase,
    maker: &str,
    model: &str,
    exif: &HashMap<String, String>,
) -> Option<LensDistortionParams> {
    let lens = lens_named(db, maker, model)?;
    let shot = LensShot::from_exif(exif);
    let params = lens.get_distortion_params(shot.focal_mm, shot.f_number, shot.distance_m)?;
    // Argentum: on the photo's frame, not the calibration's (mods/lens_frame.rs),
    // and in the renderer's units, not Lensfun's (mods/lens_units.rs).
    let frame = crate::mods::lens_frame::frame_scale(db, lens, exif);
    let params = crate::mods::lens_frame::vignetting_on_photo_frame(params, frame);
    Some(crate::mods::lens_units::in_renderer_units(
        params, lens, frame,
    ))
}

/// The lens an edit names: its maker and the display name `lens_result` gave it.
pub(crate) fn lens_named<'a>(db: &'a LensDatabase, maker: &str, model: &str) -> Option<&'a Lens> {
    let maker_lenses = lenses_for_maker(db, maker);
    maker_lenses
        .iter()
        .find(|l| l.get_display_name(&maker_lenses) == model)
        .copied()
}
