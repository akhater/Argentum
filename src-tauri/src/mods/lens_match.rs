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
//! generation, focal lengths and aperture, and no other maker's name, and when
//! the photo's focal length is within the lens's range (RAWmakase main @
//! 1b2687c4: `matching.rs::score`, and the range as `lensfun/mod.rs::lens`
//! reads it). The best is the one whose name the photo's explains most of, and
//! only a single best of a name that says more than numbers counts.
//!
//! Argentum: it decides only when it finds exactly one lens ([`confident`]).
//! Anything else (several lenses, none, an unknown camera, no lens name)
//! leaves the answer to the inherited matcher, as before, so a photo it is not
//! sure of is corrected exactly as it was. The frame a lens must cover is the
//! photo's ([`photo_crop`]): the camera's, or a crop mode its maker note or
//! its 35 mm equivalent focal length tells. Not taken (yet): the photo frame's
//! own size, and Lensfun's aspect-ratio variants, which Argentum's database
//! does not read.

use super::lens_names::{
    THIRD_PARTY, maker_words, same_generation, same_text, stated, stated_millimetres, words,
};
use super::lens_shot::MatchShot;
use crate::lens_correction::{CalibrationElement, Camera, Lens, LensDatabase, untranslated};

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
pub(crate) fn fixed_lens(camera: &Camera) -> bool {
    camera.mount.chars().next().is_some_and(char::is_lowercase)
}

/// Whether `lens` can be on `camera`: a mount in common with the camera's own
/// and those Lensfun says fit it.
pub(crate) fn fits_mount(db: &LensDatabase, camera: &Camera, lens: &Lens) -> bool {
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

/// The crop factor of a photo's frame from `camera`: the camera's times the
/// crop mode its maker note records (Canon's APS-C and APS-H modes); else the
/// one its 35 mm equivalent focal length gives where that disagrees with the
/// camera's by more than rounding (10%), a crop mode the camera writes so (Nikon
/// DX, Sony APS-C) or a readout of another aspect ratio (1:1); else the
/// camera's. RAWmakase's `photo_crop` (`matching.rs` @ 1b2687c4) without the
/// frame's size, which Argentum does not read here.
///
/// Argentum: a fixed-lens camera keeps its own. Its lens was calibrated on its
/// own frame, and some write a 35 mm equivalent of the processed picture: Sony's
/// RX0 writes 24 mm at 7.7 mm (3.1x) for a frame Lensfun gives as 2.7x.
pub fn photo_crop(camera: &Camera, shot: &MatchShot) -> f32 {
    if let Some(mode) = shot.crop_mode {
        return camera.cropfactor * mode;
    }
    if fixed_lens(camera) {
        return camera.cropfactor;
    }
    crop_or_35mm_equivalent(camera.cropfactor, shot)
}

/// `crop`, unless the crop factor `shot`'s 35 mm equivalent gives is more than
/// rounding (10%) away from it: then that one.
pub fn crop_or_35mm_equivalent(crop: f32, shot: &MatchShot) -> f32 {
    match shot.exif_crop {
        Some(exif) if (exif / crop - 1.).abs() > 0.1 => exif,
        _ => crop,
    }
}

/// Whether `lens`'s calibration covers a frame of crop factor `crop`.
fn covers(lens: &Lens, crop: f32) -> bool {
    lens.cropfactor
        .is_none_or(|own| crop > 0. && own > 0. && crop / own >= CROP_TOLERANCE)
}

/// The focal lengths (mm) `lens` has, shortest and longest: its `<focal>`, else
/// the range its name states when its calibration lies inside it (a converter's,
/// "+ 1.4x", lies outside the name's), else the focal lengths it was calibrated
/// at. `None` for a lens listed for its name only.
fn focal_range(lens: &Lens) -> Option<[f32; 2]> {
    let (lo, hi) = lens
        .calibration
        .iter()
        .flat_map(|c| &c.elements)
        .map(|e| match e {
            CalibrationElement::Distortion(d) => d.focal,
            CalibrationElement::Tca(t) => t.focal,
            CalibrationElement::Vignetting(v) => v.focal,
        })
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), f| {
            (lo.min(f), hi.max(f))
        });
    let range = match &lens.focal {
        Some(f) if f.value.is_some() => f.value.map(|v| [v, v]),
        Some(f) => {
            let min = f.min.unwrap_or(lo);
            Some([min, f.max.unwrap_or(hi.max(min))])
        }
        None => match stated_millimetres(untranslated(&lens.model)) {
            Some([a, b])
                if a > 0. && b >= a && (!lo.is_finite() || (lo >= a * 0.97 && hi <= b * 1.03)) =>
            {
                Some([a, b])
            }
            _ => Some([lo, hi]),
        },
    };
    range.filter(|[lo, hi]| lo.is_finite() && *lo > 0. && hi >= lo)
}

/// Whether `lens` can have been at `focal_mm`: within its range, give or take
/// rounding (3% and half a millimetre). A lens of no known range, or a photo of no
/// recorded focal length, can.
fn holds_focal(lens: &Lens, focal_mm: Option<f32>) -> bool {
    match (focal_range(lens), focal_mm.filter(|f| *f > 0.)) {
        (Some([lo, hi]), Some(f)) => (lo * 0.97 - 0.5..=hi * 1.03 + 0.5).contains(&f),
        _ => true,
    }
}

/// The lens a photo was taken with, as far as the database tells. `shot` is what
/// the photo records of its focal length.
pub fn find(
    db: &LensDatabase,
    make: &str,
    model: &str,
    lens_name: &str,
    shot: &MatchShot,
) -> Outcome {
    let Some(cam) = camera(db, make, model) else {
        return Outcome::UnknownCamera;
    };
    let crop = photo_crop(cam, shot);
    let fitting = db
        .lenses
        .iter()
        .enumerate()
        .filter(|(_, l)| fits_mount(db, cam, l) && covers(l, crop));
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
        .filter(|(_, l)| holds_focal(l, shot.focal_mm))
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
/// takes them: EXIF Make, the lens name, EXIF Model, what it records of its
/// focal length.
pub fn confident<'d>(
    db: &'d LensDatabase,
    make: &str,
    lens_name: &str,
    model: &str,
    shot: &MatchShot,
) -> Option<&'d Lens> {
    let clean = |s: &str| s.trim().trim_matches('"').trim().to_string();
    match find(db, &clean(make), &clean(model), &clean(lens_name), shot) {
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
  <lens><maker>Canon</maker><model>Canon EF 70-200mm f/2.8L IS II USM</model><mount>Canon EF</mount><cropfactor>1</cropfactor>
    <calibration><distortion model="ptlens" focal="70" a="0" b="0.01" c="0"/><distortion model="ptlens" focal="200" a="0" b="0.02" c="0"/></calibration></lens>
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
  <lens><maker>Test</maker><model>Test Zoom 24-70mm f/2.8</model><mount>Canon EF</mount><cropfactor>1</cropfactor>
    <calibration><distortion model="poly3" focal="35" k1="0.01"/></calibration></lens>
  <lens><maker>Minolta</maker><model>Minolta AF 70-210mm f/4 Macro</model><mount>Canon EF</mount><cropfactor>1</cropfactor>
    <calibration><distortion model="poly3" focal="75" k1="0.01"/><distortion model="poly3" focal="210" k1="0.02"/></calibration></lens>
  <lens><maker>Fujifilm</maker><model>XF70-300mmF4-5.6 R LM OIS WR + 1.4x</model><mount>Canon EF</mount><cropfactor>1</cropfactor>
    <calibration><distortion model="poly3" focal="98" k1="0.01"/><distortion model="poly3" focal="420" k1="0.02"/></calibration></lens>
  <lens><maker>Test</maker><model>Test Prime 35</model><mount>Canon EF</mount><cropfactor>1</cropfactor><focal value="35"/></lens>
</lensdatabase>"#;

    /// A photo that records `focal_mm` and no 35 mm equivalent.
    fn at_focal(focal_mm: Option<f32>) -> MatchShot {
        MatchShot {
            focal_mm,
            ..MatchShot::default()
        }
    }

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
        assert_eq!((db.cameras.len(), db.lenses.len()), (6, 15));
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
        let five_d = |name| {
            find(
                &db,
                "Canon",
                "Canon EOS 5D Mark III",
                name,
                &MatchShot::default(),
            )
        };
        let m = five_d("EF70-200mm f/2.8L IS II USM");
        assert_eq!(names(&db, &m), ["Canon EF 70-200mm f/2.8L IS II USM"]);
        assert!(matches!(m, Outcome::Unique(_)));
        // The first version is not the second: the generation must agree both ways.
        let m = five_d("EF70-200mm f/2.8L IS USM");
        assert_eq!(names(&db, &m), ["Canon EF 70-200mm f/2.8L IS USM"]);
        let m = find(
            &db,
            "SONY",
            "ILCE-1M2",
            "FE 70-200mm F2.8 GM OSS II",
            &MatchShot::default(),
        );
        assert_eq!(m, Outcome::NoMatch);
        // A Nikon name without the maker's words.
        let m = find(
            &db,
            "NIKON CORPORATION",
            "NIKON D6",
            "50mm f/1.4G",
            &MatchShot::default(),
        );
        assert_eq!(names(&db, &m), ["Nikon AF-S Nikkor 50mm f/1.4G"]);
    }

    #[test]
    fn bare_and_tied_names_stay_a_choice() {
        let db = db();
        let m = find(
            &db,
            "Canon",
            "Canon EOS 5D Mark III",
            "24-70mm",
            &MatchShot::default(),
        );
        assert_eq!(
            names(&db, &m),
            ["Test Zoom 24-70mm f/2.8", "Canon EF 24-70mm f/2.8L USM"]
        );
        // The same lens calibrated on two cameras: both fit a Micro Four Thirds body.
        let m = find(
            &db,
            "Panasonic",
            "DMC-G6",
            "SIGMA 19mm F2.8 DN",
            &MatchShot::default(),
        );
        assert_eq!(m, Outcome::Ambiguous(vec![6, 7]));
        // Another maker's lens is never this one.
        let m = find(
            &db,
            "Canon",
            "Canon EOS M",
            "TAMRON 17-50mm F/2.8",
            &MatchShot::default(),
        );
        assert_eq!(m, Outcome::NoMatch);
        assert_eq!(
            find(&db, "Canon", "Canon EOS M", "", &MatchShot::default()),
            Outcome::NoLensName
        );
        assert_eq!(
            find(&db, "Leica", "M10", "Summicron", &MatchShot::default()),
            Outcome::UnknownCamera
        );
    }

    #[test]
    fn mounts_fit_one_way_and_coverage_counts() {
        let db = db();
        let eos_m = |name| find(&db, "Canon", "Canon EOS M", name, &MatchShot::default());
        let five_d = |name| {
            find(
                &db,
                "Canon",
                "Canon EOS 5D Mark III",
                name,
                &MatchShot::default(),
            )
        };
        let m = eos_m("EF-M15-45mm f/3.5-6.3 IS STM");
        assert_eq!(names(&db, &m), ["Canon EF-M 15-45mm f/3.5-6.3 IS STM"]);
        let m = eos_m("EF70-200mm f/2.8L IS II USM");
        assert_eq!(names(&db, &m), ["Canon EF 70-200mm f/2.8L IS II USM"]);
        assert_eq!(five_d("EF-M15-45mm f/3.5-6.3 IS STM"), Outcome::NoMatch);
        // The Sigma APS-C zoom fits the EOS M by name and mount, not the 5D.
        assert_eq!(five_d("17-50mm F2.8 EX DC OS HSM"), Outcome::NoMatch);
    }

    /// RAWmakase's `a_lens_matches_by_its_words_mount_and_coverage` (main @
    /// 1b2687c4): a focal length outside the lens's range is another lens.
    #[test]
    fn a_focal_length_outside_the_range_is_another_lens() {
        let db = db();
        let at = |focal| {
            find(
                &db,
                "Canon",
                "Canon EOS 5D Mark III",
                "EF70-200mm f/2.8L IS II USM",
                &at_focal(focal),
            )
        };
        assert_eq!(at(Some(24.)), Outcome::NoMatch);
        assert!(matches!(at(Some(200.)), Outcome::Unique(_)));
        // Rounding: a 70-200mm records 69 or 206 mm.
        assert!(matches!(at(Some(69.)), Outcome::Unique(_)));
        assert!(matches!(at(Some(206.)), Outcome::Unique(_)));
        // A photo that records no focal length keeps every lens.
        assert!(matches!(at(None), Outcome::Unique(_)));
    }

    /// RAWmakase's `a_lens_s_focal_range_comes_from_its_name_before_its_calibration`
    /// (`lens/lensfun/tests.rs` @ 1b2687c4): a zoom calibrated at one focal
    /// length is still a zoom.
    #[test]
    fn a_lens_s_focal_range_comes_from_its_name_before_its_calibration() {
        let db = db();
        let range = |name: &str| {
            focal_range(
                db.lenses
                    .iter()
                    .find(|l| untranslated(&l.model) == name)
                    .unwrap(),
            )
        };
        assert_eq!(range("Minolta AF 70-210mm f/4 Macro"), Some([70., 210.]));
        assert_eq!(range("Test Zoom 24-70mm f/2.8"), Some([24., 70.]));
        // A converter's calibration lies outside the name's range: the calibration's.
        assert_eq!(
            range("XF70-300mmF4-5.6 R LM OIS WR + 1.4x"),
            Some([98., 420.])
        );
        assert_eq!(range("Test Prime 35"), Some([35., 35.]));
        assert_eq!(range("Pentacon 50mm f/1.8"), Some([50., 50.]));
        assert_eq!(
            range("Canon EF-M 15-45mm f/3.5-6.3 IS STM"),
            Some([15., 45.])
        );
        // No name range, no calibration, no `<focal>`.
        assert_eq!(range("PowerShot G12"), None);
        let m = find(
            &db,
            "Canon",
            "Canon EOS 5D Mark III",
            "AF 70-210mm f/4 Macro",
            &at_focal(Some(70.)),
        );
        assert_eq!(names(&db, &m), ["Minolta AF 70-210mm f/4 Macro"]);
    }

    /// A full-frame Sony in its APS-C mode writes the crop in its 35 mm
    /// equivalent focal length (28.5 mm at 19 mm): a lens calibrated on APS-C
    /// covers that frame, and not the whole sensor's.
    #[test]
    fn a_crop_mode_s_frame_is_covered_by_a_crop_calibration() {
        let db = db();
        let shot = |exif_crop| MatchShot {
            focal_mm: Some(19.),
            exif_crop,
            crop_mode: None,
        };
        let a1 = |exif_crop| find(&db, "SONY", "ILCE-1M2", "19mm F2.8 DN", &shot(exif_crop));
        assert_eq!(a1(None), Outcome::NoMatch);
        assert_eq!(a1(Some(1.5)), Outcome::Unique(6));
        // Within rounding of the body's own crop factor: the body's.
        assert_eq!(crop_or_35mm_equivalent(1., &shot(Some(1.05))), 1.);
        assert_eq!(crop_or_35mm_equivalent(1.534, &shot(Some(1.5))), 1.534);
        assert_eq!(crop_or_35mm_equivalent(1., &shot(Some(1.5))), 1.5);
        // A crop mode the maker note records is the camera's times it, whatever
        // the 35 mm equivalent says.
        let five_d = camera(&db, "Canon", "Canon EOS 5D Mark III").unwrap();
        let in_mode = MatchShot {
            crop_mode: Some(1.6),
            ..shot(None)
        };
        assert_eq!(photo_crop(five_d, &in_mode), 1.6);
        // A fixed-lens camera keeps its own: its lens was calibrated on its frame.
        let g12 = camera(&db, "Canon", "Canon PowerShot G12").unwrap();
        assert_eq!(photo_crop(g12, &shot(Some(5.5))), 4.6);
    }

    #[test]
    fn a_fixed_lens_camera_needs_no_lens_name() {
        let db = db();
        let m = find(
            &db,
            "Canon",
            "Canon PowerShot G12",
            "",
            &MatchShot::default(),
        );
        assert_eq!(names(&db, &m), ["PowerShot G12"]);
    }

    /// A maker's name doesn't make a name of numbers say more: "Canon 50mm
    /// f/1.8" on a Canon fits other makers' 50mm f/1.8 too (the Pentacon, through
    /// M42), and is never one alone.
    #[test]
    fn a_maker_and_numbers_is_still_a_bare_name() {
        let db = db();
        let m = find(
            &db,
            "Canon",
            "Canon EOS 5D Mark III",
            "Canon 50mm f/1.8",
            &MatchShot::default(),
        );
        assert!(!matches!(m, Outcome::Unique(_)), "{:?}", names(&db, &m));
        assert!(
            confident(
                &db,
                "Canon",
                "Canon 50mm f/1.8",
                "Canon EOS 5D Mark III",
                &MatchShot::default()
            )
            .is_none()
        );
    }

    #[test]
    fn confident_cleans_its_inputs_as_the_callers_send_them() {
        let db = db();
        let lens = confident(
            &db,
            "\"Canon\"",
            " EF70-200mm f/2.8L IS II USM ",
            "Canon EOS 5D Mark III",
            &MatchShot::default(),
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

    /// What the app answers for a photo: the lens's maker and display name.
    type Answer = Option<(String, String)>;

    /// The answer `find_best_lens_match` gives a photo of `focal_mm`.
    fn answer(
        db: &LensDatabase,
        make: &str,
        lens: &str,
        model: &str,
        focal_mm: Option<f32>,
    ) -> Answer {
        crate::lens_correction::find_best_lens_match(db, make, lens, model, &at_focal(focal_mm))
    }

    /// The untranslated name of the lens an answer names.
    fn named(db: &LensDatabase, answer: &Answer) -> String {
        answer
            .as_ref()
            .and_then(|(maker, model)| crate::lens_correction::lens_named(db, maker, model))
            .map_or_else(|| "-".to_string(), |l| untranslated(&l.model).to_string())
    }

    /// A camera of `lens`'s own mount whose frame it covers, the nearest its
    /// calibration's crop factor.
    fn home_camera<'d>(db: &'d LensDatabase, lens: &Lens) -> Option<&'d Camera> {
        db.cameras
            .iter()
            .filter(|c| {
                lens.mount
                    .iter()
                    .any(|m| m.trim().eq_ignore_ascii_case(c.mount.trim()))
                    && covers(lens, c.cropfactor)
            })
            .min_by(|a, b| {
                let d = |c: &Camera| (c.cropfactor - lens.cropfactor.unwrap_or(c.cropfactor)).abs();
                d(a).total_cmp(&d(b))
            })
    }

    /// A focal length `lens` can be at: its declared one (the shortest of a
    /// range), else the shortest it was calibrated at.
    fn own_focal(lens: &Lens) -> Option<f32> {
        use crate::lens_correction::CalibrationElement as E;
        let declared = lens.focal.as_ref().and_then(|f| f.value.or(f.min));
        let calibrated = lens.calibration.as_ref().and_then(|c| {
            c.elements
                .iter()
                .map(|e| match e {
                    E::Distortion(d) => d.focal,
                    E::Tca(t) => t.focal,
                    E::Vignetting(v) => v.focal,
                })
                .min_by(f32::total_cmp)
        });
        declared.or(calibrated)
    }

    /// The app's answer on three corpora, every answer written out so that two
    /// runs (before and after a change) can be compared line by line:
    ///
    /// - every lens of the bundled database, named as its own entry names it, on
    ///   a camera of its own mount whose frame it covers, at a focal length it
    ///   has: the right answer is known (`database-names.tsv`, and the count of
    ///   right answers is printed);
    /// - a lens with an extender, named as its base lens, at the extender's focal
    ///   length and at the base lens's (`extenders.tsv`);
    /// - the lens names and focal lengths of real files (RAWmakase's record of
    ///   raw.pixls.us's CC0 files, `tests/corpus/dataset/pixls-decode.json` @
    ///   704ab207, AG_LENS_CORPUS), each on its camera's Lensfun entry, whose make
    ///   and model are the strings the files write (`real-files.tsv`).
    ///
    /// ```text
    /// AG_LENS_CORPUS=.../pixls-decode.json AG_OUT=.../dir cargo test --lib lens_match::tests::answers_on_both_corpora -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "reads AG_LENS_CORPUS and writes to AG_OUT"]
    fn answers_on_both_corpora() {
        use std::fmt::Write as _;
        let db = bundled();
        let out = std::path::PathBuf::from(std::env::var("AG_OUT").expect("AG_OUT"));
        std::fs::create_dir_all(&out).unwrap();

        // 1. The database's own names.
        let (mut cases, mut right) = (0, 0);
        let mut report = String::from("lens\tcamera\tname\tfocal\tanswer\tright\n");
        for lens in &db.lenses {
            let truth = untranslated(&lens.model);
            let Some(cam) = home_camera(&db, lens) else {
                continue;
            };
            let (make, model) = (untranslated(&cam.maker), untranslated(&cam.model));
            let maker = untranslated(&lens.maker);
            let stripped = truth
                .strip_prefix(maker)
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let focal = own_focal(lens);
            for name in std::iter::once(truth).chain(stripped) {
                cases += 1;
                let got = named(&db, &answer(&db, make, name, model, focal));
                right += usize::from(got == truth);
                writeln!(
                    report,
                    "{truth}\t{make} {model}\t{name}\t{}\t{got}\t{}",
                    focal.map_or("-".into(), |f| f.to_string()),
                    got == truth
                )
                .unwrap();
            }
        }
        std::fs::write(out.join("database-names.tsv"), &report).unwrap();
        println!("database names: {cases} cases; right {right}");

        // 2. A lens with an extender, which Lensfun lists as its own entry ("+ 1.4x
        // extender"), writes the lens's own name: told apart by the focal length
        // alone. Each such entry, on a camera of its mount, named as its base lens
        // at the extender's focal length and at the base lens's own. Without a
        // focal length the matcher is as it was before the focal filter.
        let (mut cases, mut before, mut after, mut lost) = (0, 0, 0, 0);
        let mut report = String::from(
            "truth	camera	name	focal	before	after
",
        );
        for extended in &db.lenses {
            let full = untranslated(&extended.model);
            let Some((base_name, _)) = full.split_once('+') else {
                continue;
            };
            let base_name = base_name.trim();
            let Some(base) = db
                .lenses
                .iter()
                .find(|l| untranslated(&l.model) == base_name && l.mount == extended.mount)
            else {
                continue;
            };
            let Some(cam) = home_camera(&db, extended) else {
                continue;
            };
            let (make, model) = (untranslated(&cam.maker), untranslated(&cam.model));
            for (truth, focal) in [
                (full, focal_range(extended).map(|r| r[1])),
                (base_name, own_focal(base)),
            ] {
                cases += 1;
                let old = named(&db, &answer(&db, make, base_name, model, None));
                let new = named(&db, &answer(&db, make, base_name, model, focal));
                before += usize::from(old == truth);
                after += usize::from(new == truth);
                lost += usize::from(old == truth && new != truth);
                writeln!(
                    report,
                    "{truth}	{make} {model}	{base_name}	{}	{old}	{new}",
                    focal.map_or("-".into(), |f| f.to_string())
                )
                .unwrap();
            }
        }
        std::fs::write(out.join("extenders.tsv"), &report).unwrap();
        println!("extenders: {cases} cases; right before {before}, after {after}; lost {lost}");

        // 3. Real files.
        let Ok(path) = std::env::var("AG_LENS_CORPUS") else {
            return;
        };
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut seen = std::collections::BTreeSet::new();
        let mut known = 0;
        let mut report = String::from(
            "make\tmodel\tlens\tfocal\t35mm\tanswer\tanswer with the 35mm equivalent's crop\n",
        );
        let mut crop_changed = 0;
        for file in json["files"].as_array().unwrap() {
            let md = &file["header"]["metadata"];
            let lens = md["lens_model"]
                .as_str()
                .or_else(|| md["lens"].as_str())
                .unwrap_or("");
            let focal = md["focal"].as_f64().map(|f| f as f32).filter(|f| *f > 0.);
            let focal_35mm = md["focal_35mm"]
                .as_f64()
                .map(|f| f as f32)
                .filter(|f| *f > 0.);
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
            let focal_key = focal.map_or("-".to_string(), |f| format!("{f:.1}"));
            let key_35mm = focal_35mm.map_or("-".to_string(), |f| format!("{f:.1}"));
            if !seen.insert((
                make.clone(),
                model.clone(),
                lens.to_string(),
                format!("{focal_key} {key_35mm}"),
            )) {
                continue;
            }
            known += usize::from(entry.is_some());
            let got = named(&db, &answer(&db, &make, lens, &model, focal));
            // The same photo with the crop factor its 35 mm equivalent gives, read
            // as the app reads it from the EXIF map.
            let mut exif = std::collections::HashMap::new();
            for (key, value) in [
                ("FocalLength", focal),
                ("FocalLengthIn35mmFilm", focal_35mm),
            ] {
                if let Some(v) = value {
                    exif.insert(key.to_string(), v.to_string());
                }
            }
            let shot = MatchShot::from_exif(&exif);
            let with_crop = named(
                &db,
                &crate::lens_correction::find_best_lens_match(&db, &make, lens, &model, &shot),
            );
            crop_changed += usize::from(with_crop != got);
            writeln!(
                report,
                "{make}\t{model}\t{lens}\t{focal_key}\t{key_35mm}\t{got}\t{with_crop}"
            )
            .unwrap();
        }
        std::fs::write(out.join("real-files.tsv"), &report).unwrap();
        let started = std::time::Instant::now();
        for (make, model, lens, focal) in &seen {
            let focal = focal.split(' ').next().and_then(|f| f.parse().ok());
            std::hint::black_box(answer(&db, make, lens, model, focal));
        }
        let per = started.elapsed().as_secs_f64() * 1000. / seen.len() as f64;
        println!(
            "real files: {} distinct camera, lens and focal length cases ({known} cameras known to Lensfun); {crop_changed} answer otherwise with the crop factor of their 35 mm equivalent; {per:.3} ms per call",
            seen.len()
        );
    }
}
