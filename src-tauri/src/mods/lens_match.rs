//! Which Lensfun lens a photo was taken with, decided only when there is no doubt.
//!
//! The matcher Argentum inherited (`lens_correction::loose_lens_match`) scores
//! names with a fuzzy subsequence match and, when the camera's maker has no
//! lens that scores, takes the best of every lens in the database. So a bare
//! name like "Canon 50mm f/1.8" lands on whichever lens happens to score
//! highest, of any maker or mount.
//!
//! This is RAWmakase's matcher (`crates/rawmakase-model/src/lens/lensfun/
//! matching.rs` @ 704ab207, v0.4.12, MIT), on Argentum's database types: the
//! camera is found by the make and model the file writes; a lens can be on it
//! when a mount of the lens is the camera's or one Lensfun says fits it, and
//! when its calibration covers the camera's frame; a lens name matches when
//! every word of it is among the lens's (makers' names aside), with the same
//! generation, focal lengths and aperture, and no other maker's name. The best
//! is the one whose name the photo's explains most of, and only a single best
//! of a name that says more than numbers counts.
//!
//! Argentum: it decides only when it finds exactly one lens ([`confident`]).
//! Anything else (several lenses, none, an unknown camera, no lens name)
//! leaves the answer to the inherited matcher, as before, so a photo it is not
//! sure of is corrected exactly as it was. Not taken (yet): the photo's focal
//! length (RAWmakase refuses a lens whose range does not hold it) and crop
//! modes (an R6 in APS-C mode), which need the focal lengths every caller of
//! `find_best_lens_match` would have to pass; and Lensfun's aspect-ratio
//! variants, which Argentum's database does not read.

use super::lens_names::{THIRD_PARTY, maker_words, same_generation, same_text, stated, words};
use crate::lens_correction::{Camera, Lens, LensDatabase, untranslated};

/// A lens calibrated on a camera of a larger crop factor than the photo's, by
/// more than this, doesn't cover the photo (`optics::lensfun::CROP_TOLERANCE`).
const CROP_TOLERANCE: f32 = 0.96;

/// How a photo matched.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// One lens fits best: its index in the database.
    Unique(usize),
    /// Several fit equally, or the name is too bare to pick: best first.
    Ambiguous(Vec<usize>),
    /// The camera is known but no lens fits its name.
    NoMatch,
    /// The photo records no lens name.
    NoLensName,
    /// Lensfun doesn't know the camera.
    UnknownCamera,
}

/// The camera entry for a file's EXIF make and model. Of several (Lensfun's
/// aspect-ratio variants), the first: Argentum reads no aspect ratio.
pub fn camera<'d>(db: &'d LensDatabase, make: &str, model: &str) -> Option<&'d Camera> {
    db.cameras.iter().find(|c| {
        let (maker, name) = (untranslated(&c.maker), untranslated(&c.model));
        same_text(maker, make)
            && (same_text(name, model) || same_text(name, &format!("{make} {model}")))
    })
}

/// A fixed-lens camera: Lensfun names its mount in lower case first.
fn fixed_lens(camera: &Camera) -> bool {
    camera.mount.chars().next().is_some_and(char::is_lowercase)
}

/// Whether `lens` can be on `camera`: a mount in common with the camera's own
/// and those Lensfun says fit it.
fn fits_mount(db: &LensDatabase, camera: &Camera, lens: &Lens) -> bool {
    let compat: &[String] = db
        .mounts
        .iter()
        .find(|m| untranslated(&m.name).eq_ignore_ascii_case(camera.mount.trim()))
        .map_or(&[], |m| m.compat.as_slice());
    lens.mount.iter().any(|m| {
        let m = m.trim();
        m.eq_ignore_ascii_case(camera.mount.trim())
            || compat.iter().any(|c| c.trim().eq_ignore_ascii_case(m))
    })
}

/// Whether `lens`'s calibration covers a frame of crop factor `crop`.
fn covers(lens: &Lens, crop: f32) -> bool {
    lens.cropfactor
        .is_none_or(|own| crop > 0. && own > 0. && crop / own >= CROP_TOLERANCE)
}

/// The lens a photo was taken with, as far as the database tells.
pub fn find(db: &LensDatabase, make: &str, model: &str, lens_name: &str) -> Outcome {
    let Some(cam) = camera(db, make, model) else {
        return Outcome::UnknownCamera;
    };
    let fitting = db
        .lenses
        .iter()
        .enumerate()
        .filter(|(_, l)| fits_mount(db, cam, l) && covers(l, cam.cropfactor));
    if fixed_lens(cam) {
        let own: Vec<usize> = fitting
            .filter(|(_, l)| {
                l.mount
                    .iter()
                    .any(|m| m.trim().eq_ignore_ascii_case(cam.mount.trim()))
            })
            .map(|(i, _)| i)
            .collect();
        return match own.as_slice() {
            [] => Outcome::NoMatch,
            [one] => Outcome::Unique(*one),
            _ => Outcome::Ambiguous(own),
        };
    }
    if lens_name.trim().is_empty() {
        return Outcome::NoLensName;
    }
    let mut scored: Vec<(usize, f32)> = fitting
        .filter_map(|(i, l)| score(cam, l, lens_name).map(|s| (i, s)))
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    match scored.as_slice() {
        [] => Outcome::NoMatch,
        [(best, s), rest @ ..]
            if !bare(lens_name, cam) && rest.first().is_none_or(|r| r.1 < s - 1e-6) =>
        {
            Outcome::Unique(*best)
        }
        all => Outcome::Ambiguous(all.iter().map(|(i, _)| *i).collect()),
    }
}

/// The lens, when [`find`] finds exactly one. Inputs as `find_best_lens_match`
/// takes them: EXIF Make, the lens name, EXIF Model.
pub fn confident<'d>(
    db: &'d LensDatabase,
    make: &str,
    lens_name: &str,
    model: &str,
) -> Option<&'d Lens> {
    let clean = |s: &str| s.trim().trim_matches('"').trim().to_string();
    match find(db, &clean(make), &clean(model), &clean(lens_name)) {
        Outcome::Unique(i) => db.lenses.get(i),
        _ => None,
    }
}

/// A name of numbers alone ("24-70mm", "17-50mm f/2.8"), makers' names aside
/// ("Canon 50mm f/1.8" says no more than "50mm f/1.8").
fn bare(name: &str, camera: &Camera) -> bool {
    let camera_maker = maker_words(untranslated(&camera.maker));
    words(name)
        .iter()
        .filter(|w| {
            !camera_maker.contains(w)
                && !THIRD_PARTY.contains(&w.as_str())
                && !MAKERS.contains(&w.as_str())
        })
        .all(|w| w == "mm" || w.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

/// Camera makers' names, as lens names may carry them.
const MAKERS: [&str; 16] = [
    "canon",
    "nikon",
    "sony",
    "pentax",
    "olympus",
    "panasonic",
    "lumix",
    "leica",
    "fujifilm",
    "fuji",
    "samsung",
    "minolta",
    "konica",
    "ricoh",
    "hasselblad",
    "om",
];

/// How much of `lens`'s name `lens_name` explains (0-1), or `None` when it
/// can't be that lens.
fn score(camera: &Camera, lens: &Lens, lens_name: &str) -> Option<f32> {
    let lens_maker = maker_words(untranslated(&lens.maker));
    let camera_maker = maker_words(untranslated(&camera.maker));
    let model = untranslated(&lens.model);
    let lens_words: Vec<String> = words(model)
        .into_iter()
        .filter(|w| !lens_maker.contains(w))
        .collect();
    let exif: Vec<String> = words(lens_name)
        .into_iter()
        .filter(|w| !lens_maker.contains(w) && !camera_maker.contains(w))
        .collect();
    if exif.is_empty() {
        return None;
    }
    // A third-party maker the photo names is the lens's.
    if exif.iter().any(|w| THIRD_PARTY.contains(&w.as_str())) {
        return None;
    }
    // Every word of the photo's name, as many times as it has it.
    let mut left = lens_words.clone();
    for w in &exif {
        let i = left.iter().position(|l| l == w)?;
        left.swap_remove(i);
    }
    if !same_generation(&exif, &lens_words) {
        return None;
    }
    if !stated(model).agree(&stated(lens_name)) {
        return None;
    }
    Some(exif.len() as f32 / lens_words.len().max(1) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RAWmakase's test database (`lens/lensfun/tests.rs` @ 704ab207), in the
    /// parts Argentum reads.
    const DB: &str = r#"<lensdatabase version="2">
  <mount><name>Canon EF</name><compat>M42</compat></mount>
  <mount><name>Canon EF-M</name><compat>Canon EF</compat><compat>Canon EF-S</compat></mount>
  <mount><name>Micro 4/3 System</name></mount>
  <camera><maker>Canon</maker><model>Canon EOS 5D Mark III</model><model lang="en">EOS 5D Mark III</model><mount>Canon EF</mount><cropfactor>1</cropfactor></camera>
  <camera><maker>Canon</maker><model>Canon EOS M</model><mount>Canon EF-M</mount><cropfactor>1.6</cropfactor></camera>
  <camera><maker>Nikon Corporation</maker><maker lang="en">Nikon</maker><model>Nikon D6</model><mount>Nikon F AF</mount><cropfactor>1</cropfactor></camera>
  <camera><maker>Panasonic</maker><model>DMC-G6</model><mount>Micro 4/3 System</mount><cropfactor>2</cropfactor></camera>
  <camera><maker>Canon</maker><model>Canon PowerShot G12</model><mount>canonG12</mount><cropfactor>4.6</cropfactor></camera>
  <camera><maker>Sony</maker><model>ILCE-1M2</model><mount>Sony E</mount><cropfactor>1</cropfactor></camera>
  <lens><maker>Canon</maker><model>Canon EF 70-200mm f/2.8L IS II USM</model><mount>Canon EF</mount><cropfactor>1</cropfactor></lens>
  <lens><maker>Canon</maker><model>Canon EF 70-200mm f/2.8L IS USM</model><mount>Canon EF</mount><cropfactor>1</cropfactor></lens>
  <lens><maker>Canon</maker><model>Canon EF 24-70mm f/2.8L USM</model><mount>Canon EF</mount><cropfactor>1</cropfactor></lens>
  <lens><maker>Canon</maker><model>Canon EF-M 15-45mm f/3.5-6.3 IS STM</model><mount>Canon EF-M</mount><cropfactor>1.6</cropfactor></lens>
  <lens><maker>Sigma</maker><model>Sigma 17-50mm f/2.8 EX DC OS HSM</model><mount>Canon EF</mount><cropfactor>1.6</cropfactor></lens>
  <lens><maker>Nikon</maker><model>Nikon AF-S Nikkor 50mm f/1.4G</model><mount>Nikon F AF</mount><cropfactor>1</cropfactor></lens>
  <lens><maker>Sigma</maker><model>Sigma 19mm f/2.8 DN</model><mount>Sony E</mount><mount>Micro 4/3 System</mount><cropfactor>1.534</cropfactor></lens>
  <lens><maker>Sigma</maker><model>Sigma 19mm f/2.8 DN</model><mount>Micro 4/3 System</mount><mount>Sony E</mount><cropfactor>2</cropfactor></lens>
  <lens><maker>Canon</maker><model>PowerShot G12</model><mount>canonG12</mount><cropfactor>4.6</cropfactor></lens>
  <lens><maker>Sony</maker><model>FE 70-200mm f/2.8 GM OSS</model><mount>Sony E</mount><cropfactor>1</cropfactor></lens>
  <lens><maker>Pentacon</maker><model>Pentacon 50mm f/1.8</model><mount>M42</mount><cropfactor>1</cropfactor></lens>
  <lens><maker>Test</maker><model>Test Zoom 24-70mm f/2.8</model><mount>Canon EF</mount><cropfactor>1</cropfactor></lens>
</lensdatabase>"#;

    fn db() -> LensDatabase {
        quick_xml::de::from_str(DB).expect("the test database")
    }
    fn names(db: &LensDatabase, outcome: &Outcome) -> Vec<String> {
        let name = |i: &usize| untranslated(&db.lenses[*i].model).to_string();
        match outcome {
            Outcome::Unique(i) => vec![name(i)],
            Outcome::Ambiguous(all) => all.iter().map(name).collect(),
            _ => vec![],
        }
    }

    #[test]
    fn the_database_reads_its_mounts() {
        let db = db();
        assert_eq!(db.mounts.len(), 3);
        assert_eq!(db.mounts[1].compat, ["Canon EF", "Canon EF-S"]);
        assert_eq!((db.cameras.len(), db.lenses.len()), (6, 12));
    }

    #[test]
    fn cameras_by_their_exif_names() {
        let db = db();
        let found = |make, model| camera(&db, make, model).map(|c| untranslated(&c.model));
        assert_eq!(
            found("Canon", "Canon EOS 5D Mark III"),
            Some("Canon EOS 5D Mark III")
        );
        assert_eq!(found("NIKON CORPORATION", "NIKON D6"), Some("Nikon D6"));
        assert_eq!(
            found("Canon", "EOS 5D Mark III"),
            Some("Canon EOS 5D Mark III")
        );
        assert_eq!(found("Nikon", "D6"), None);
    }

    #[test]
    fn a_lens_matches_by_its_words_mount_and_coverage() {
        let db = db();
        let five_d = |name| find(&db, "Canon", "Canon EOS 5D Mark III", name);
        let m = five_d("EF70-200mm f/2.8L IS II USM");
        assert_eq!(names(&db, &m), ["Canon EF 70-200mm f/2.8L IS II USM"]);
        assert!(matches!(m, Outcome::Unique(_)));
        // The first version is not the second: the generation must agree both ways.
        let m = five_d("EF70-200mm f/2.8L IS USM");
        assert_eq!(names(&db, &m), ["Canon EF 70-200mm f/2.8L IS USM"]);
        let m = find(&db, "SONY", "ILCE-1M2", "FE 70-200mm F2.8 GM OSS II");
        assert_eq!(m, Outcome::NoMatch);
        // A Nikon name without the maker's words.
        let m = find(&db, "NIKON CORPORATION", "NIKON D6", "50mm f/1.4G");
        assert_eq!(names(&db, &m), ["Nikon AF-S Nikkor 50mm f/1.4G"]);
    }

    #[test]
    fn bare_and_tied_names_stay_a_choice() {
        let db = db();
        let m = find(&db, "Canon", "Canon EOS 5D Mark III", "24-70mm");
        assert_eq!(
            names(&db, &m),
            ["Test Zoom 24-70mm f/2.8", "Canon EF 24-70mm f/2.8L USM"]
        );
        // The same lens calibrated on two cameras: both fit a Micro Four Thirds body.
        let m = find(&db, "Panasonic", "DMC-G6", "SIGMA 19mm F2.8 DN");
        assert_eq!(m, Outcome::Ambiguous(vec![6, 7]));
        // Another maker's lens is never this one.
        let m = find(&db, "Canon", "Canon EOS M", "TAMRON 17-50mm F/2.8");
        assert_eq!(m, Outcome::NoMatch);
        assert_eq!(find(&db, "Canon", "Canon EOS M", ""), Outcome::NoLensName);
        assert_eq!(
            find(&db, "Leica", "M10", "Summicron"),
            Outcome::UnknownCamera
        );
    }

    #[test]
    fn mounts_fit_one_way_and_coverage_counts() {
        let db = db();
        let eos_m = |name| find(&db, "Canon", "Canon EOS M", name);
        let five_d = |name| find(&db, "Canon", "Canon EOS 5D Mark III", name);
        let m = eos_m("EF-M15-45mm f/3.5-6.3 IS STM");
        assert_eq!(names(&db, &m), ["Canon EF-M 15-45mm f/3.5-6.3 IS STM"]);
        let m = eos_m("EF70-200mm f/2.8L IS II USM");
        assert_eq!(names(&db, &m), ["Canon EF 70-200mm f/2.8L IS II USM"]);
        assert_eq!(five_d("EF-M15-45mm f/3.5-6.3 IS STM"), Outcome::NoMatch);
        // The Sigma APS-C zoom fits the EOS M by name and mount, not the 5D.
        assert_eq!(five_d("17-50mm F2.8 EX DC OS HSM"), Outcome::NoMatch);
    }

    #[test]
    fn a_fixed_lens_camera_needs_no_lens_name() {
        let db = db();
        let m = find(&db, "Canon", "Canon PowerShot G12", "");
        assert_eq!(names(&db, &m), ["PowerShot G12"]);
    }

    /// A maker's name doesn't make a name of numbers say more: "Canon 50mm
    /// f/1.8" on a Canon fits other makers' 50mm f/1.8 too (the Pentacon, through
    /// M42), and is never one alone.
    #[test]
    fn a_maker_and_numbers_is_still_a_bare_name() {
        let db = db();
        let m = find(&db, "Canon", "Canon EOS 5D Mark III", "Canon 50mm f/1.8");
        assert!(!matches!(m, Outcome::Unique(_)), "{:?}", names(&db, &m));
        assert!(confident(&db, "Canon", "Canon 50mm f/1.8", "Canon EOS 5D Mark III").is_none());
    }

    #[test]
    fn confident_cleans_its_inputs_as_the_callers_send_them() {
        let db = db();
        let lens = confident(
            &db,
            "\"Canon\"",
            " EF70-200mm f/2.8L IS II USM ",
            "Canon EOS 5D Mark III",
        );
        assert_eq!(
            lens.map(|l| untranslated(&l.model)),
            Some("Canon EF 70-200mm f/2.8L IS II USM")
        );
    }

    /// The bundled database, read as the app reads it.
    fn bundled() -> LensDatabase {
        crate::lens_correction::read_lensfun_dir(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/lensfun_db"
        )))
    }

    /// Every file of the bundled database still loads with its mounts read:
    /// the lens and camera counts are those the files hold.
    #[test]
    fn the_bundled_database_loads_whole() {
        let db = bundled();
        let count = |tag: &str| -> usize {
            std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/lensfun_db"))
                .unwrap()
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "xml"))
                .map(|e| {
                    let text = std::fs::read_to_string(e.path()).unwrap();
                    let text = regex::Regex::new(r"(?s)<!--.*?-->")
                        .unwrap()
                        .replace_all(&text, "");
                    text.matches(&format!("<{tag}>")).count()
                })
                .sum()
        };
        assert_eq!(db.lenses.len(), count("lens"));
        assert_eq!(db.cameras.len(), count("camera"));
        assert!(db.mounts.len() >= 50, "{} mounts", db.mounts.len());
    }

    /// What the app answers for a photo, before and after the strict matcher.
    /// A matcher's answer: the lens's maker and display name.
    type Answer = Option<(String, String)>;

    fn answers(db: &LensDatabase, make: &str, lens: &str, model: &str) -> (Answer, Answer) {
        (
            crate::lens_correction::loose_lens_match(db, make, lens, model),
            crate::lens_correction::find_best_lens_match(db, make, lens, model),
        )
    }

    /// The untranslated name of the lens an answer names.
    fn named(db: &LensDatabase, answer: &Option<(String, String)>) -> String {
        answer
            .as_ref()
            .and_then(|(maker, model)| crate::lens_correction::lens_named(db, maker, model))
            .map_or_else(|| "-".to_string(), |l| untranslated(&l.model).to_string())
    }

    /// Old against new on two corpora, every difference written out for review:
    ///
    /// - every lens of the bundled database, named as its own entry names it, on
    ///   a camera of its own mount whose frame it covers: the right answer is
    ///   known, so a regression is a lens the old matcher found and the new one
    ///   does not;
    /// - the lens names of real files (RAWmakase's record of raw.pixls.us's CC0
    ///   files, `tests/corpus/dataset/pixls-decode.json`, AG_LENS_CORPUS), each
    ///   on its camera's Lensfun entry, whose make and model are the strings the
    ///   files write.
    ///
    /// ```text
    /// AG_LENS_CORPUS=.../pixls-decode.json AG_OUT=.../dir cargo test --lib lens_match::tests::old_against_new -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "reads AG_LENS_CORPUS and writes to AG_OUT"]
    fn old_against_new() {
        use std::fmt::Write as _;
        let db = bundled();
        let out = std::path::PathBuf::from(std::env::var("AG_OUT").expect("AG_OUT"));
        std::fs::create_dir_all(&out).unwrap();

        // 1. The database's own names.
        let (mut cases, mut old_right, mut new_right, mut regressions) = (0, 0, 0, 0);
        let mut report =
            String::from("lens\tcamera\tname\told\tnew\tcamera crop\told crop\tnew crop\n");
        let crop_of = |answer: &Option<(String, String)>| {
            answer
                .as_ref()
                .and_then(|(maker, model)| crate::lens_correction::lens_named(&db, maker, model))
                .and_then(|l| l.cropfactor)
                .map_or_else(|| "-".to_string(), |c| c.to_string())
        };
        for lens in &db.lenses {
            let truth = untranslated(&lens.model);
            let Some(cam) = db
                .cameras
                .iter()
                .filter(|c| {
                    lens.mount
                        .iter()
                        .any(|m| m.trim().eq_ignore_ascii_case(c.mount.trim()))
                        && covers(lens, c.cropfactor)
                })
                .min_by(|a, b| {
                    let d =
                        |c: &Camera| (c.cropfactor - lens.cropfactor.unwrap_or(c.cropfactor)).abs();
                    d(a).total_cmp(&d(b))
                })
            else {
                continue;
            };
            let (make, model) = (untranslated(&cam.maker), untranslated(&cam.model));
            let maker = untranslated(&lens.maker);
            let stripped = truth
                .strip_prefix(maker)
                .map(str::trim)
                .filter(|s| !s.is_empty());
            for name in std::iter::once(truth).chain(stripped) {
                cases += 1;
                let (old, new) = answers(&db, make, name, model);
                let (o, n) = (named(&db, &old), named(&db, &new));
                old_right += usize::from(o == truth);
                new_right += usize::from(n == truth);
                if o == truth && n != truth {
                    regressions += 1;
                }
                if old != new {
                    writeln!(
                        report,
                        "{truth}\t{make} {model}\t{name}\t{o}\t{n}\t{}\t{}\t{}",
                        cam.cropfactor,
                        crop_of(&old),
                        crop_of(&new)
                    )
                    .unwrap();
                }
            }
        }
        std::fs::write(out.join("database-names.tsv"), &report).unwrap();
        println!(
            "database names: {cases} cases; right before {old_right}, after {new_right}; regressions {regressions}; {} answers changed",
            report.lines().count() - 1
        );

        // 2. Real files.
        let Ok(path) = std::env::var("AG_LENS_CORPUS") else {
            return;
        };
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut seen = std::collections::BTreeSet::new();
        let (mut files, mut same, mut known) = (0, 0, 0);
        let mut changed = String::from("make\tmodel\tlens\told\tnew\tnew outcome\n");
        for file in json["files"].as_array().unwrap() {
            let md = &file["header"]["metadata"];
            let lens = md["lens_model"]
                .as_str()
                .or_else(|| md["lens"].as_str())
                .unwrap_or("");
            let (nmake, nmodel) = (
                md["make"].as_str().unwrap_or(""),
                md["model"].as_str().unwrap_or(""),
            );
            // The Lensfun entry for this camera gives the strings its files write.
            let entry = db.cameras.iter().find(|c| {
                let makers = [untranslated(&c.maker).to_string(), c.get_maker()];
                let models = [untranslated(&c.model).to_string(), c.get_model()];
                makers
                    .iter()
                    .any(|m| m.to_lowercase().contains(&nmake.to_lowercase()))
                    && models.iter().any(|m| {
                        same_text(m, nmodel)
                            || m.to_lowercase()
                                .ends_with(&format!(" {}", nmodel.to_lowercase()))
                    })
            });
            let (make, model) = entry.map_or((nmake.to_string(), nmodel.to_string()), |c| {
                (
                    untranslated(&c.maker).to_string(),
                    untranslated(&c.model).to_string(),
                )
            });
            if !seen.insert((make.clone(), model.clone(), lens.to_string())) {
                continue;
            }
            files += 1;
            known += usize::from(entry.is_some());
            let (old, new) = answers(&db, &make, lens, &model);
            if old == new {
                same += 1;
                continue;
            }
            let outcome = find(&db, &make, &model, lens);
            writeln!(
                changed,
                "{make}\t{model}\t{lens}\t{}\t{}\t{outcome:?}",
                named(&db, &old),
                named(&db, &new)
            )
            .unwrap();
        }
        std::fs::write(out.join("real-files.tsv"), &changed).unwrap();
        // What a call costs, over the same photos.
        let time = |f: &dyn Fn(&str, &str, &str)| {
            let started = std::time::Instant::now();
            for (make, model, lens) in &seen {
                f(make, lens, model);
            }
            started.elapsed().as_secs_f64() * 1000. / seen.len() as f64
        };
        let loose = time(&|make, lens, model| {
            std::hint::black_box(crate::lens_correction::loose_lens_match(
                &db, make, lens, model,
            ));
        });
        let strict = time(&|make, lens, model| {
            std::hint::black_box(confident(&db, make, lens, model));
        });
        println!("per photo: the old matcher {loose:.3} ms, the strict one {strict:.3} ms");
        println!(
            "real files: {files} distinct camera and lens pairs ({known} cameras known to Lensfun); {same} answer as before; {} changed, listed in real-files.tsv",
            files - same
        );
    }
}
