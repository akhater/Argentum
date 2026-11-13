//! A Lensfun lens's focal range as Lensfun itself sets it, which its vignetting
//! interpolation normalises the focal length by (`mods/lens_interp.rs`). Ours,
//! on Lensfun's method.
//!
//! Lensfun (`libs/lensfun/database.cpp`, the `<focal>` element, and
//! `libs/lensfun/lens.cpp`, `lfLens::GuessParameters`, which loading runs
//! through `lfLens::Check`; @ 52cf1a7b):
//!
//! - takes `<focal min max>` or `<focal value>`;
//! - when either the lens's shortest focal length or its widest aperture is
//!   missing, reads the range its name states, by the first of three patterns
//!   that matches the whole name (none for an adapter, converter, extender and
//!   the like), then widens it to every focal length it was calibrated at;
//! - fills only what `<focal>` left out, and a missing longest focal length
//!   with the shortest.
//!
//! `mods/lens_match.rs` has its own range for choosing a lens (RAWmakase's);
//! this one is Lensfun's, for correcting with it.

use std::sync::LazyLock;

use regex::Regex;

use crate::lens_correction::{CalibrationElement, Lens, untranslated};

/// `lens_name_regexes` with, for each, the groups holding the shortest and the
/// longest focal length (`lens_name_matches`, less the aperture's).
static NAME_PATTERNS: LazyLock<[(Regex, [usize; 2]); 3]> = LazyLock::new(|| {
    let whole = |pattern: &str| Regex::new(&format!("^(?:{pattern})$")).expect("a valid pattern");
    [
        // [min focal]-[max focal]mm f/[min aperture]-[max aperture]
        (
            whole(
                r"[^:]*?([0-9]+[0-9.]*)[-]?([0-9]+[0-9.]*)?(mm)[[:space:]]+(f/|f|1/|1:)?([0-9.]+)(-[0-9.]+)?.*",
            ),
            [1, 2],
        ),
        // 1:[min aperture]-[max aperture] [min focal]-[max focal]mm
        (
            whole(r".*?1:([0-9.]+)[-]?([0-9.]+)?[[:space:]]+([0-9.]+)[-]?([0-9.]+)?(mm)?.*"),
            [3, 4],
        ),
        // [min aperture]-[max aperture]/[min focal]-[max focal]
        (
            whole(r".*?([0-9.]+)[-]?([0-9.]+)?[\s]*/[\s]*([0-9.]+)[-]?([0-9.]+)?.*"),
            [3, 4],
        ),
    ]
});

/// `extender_magnification_regex`, `[0.9]` and all.
static EXTENDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:.*?[0-9](\.[0.9]+)?x.*)$").expect("a valid pattern"));

/// Words in a name that keep Lensfun from reading a range from it.
const NOT_A_LENS: [&str; 6] = [
    "adapter",
    "reducer",
    "booster",
    "extender",
    "converter",
    "magnifier",
];

/// `lens`'s shortest and longest focal lengths (mm) as Lensfun sets them; 0
/// where Lensfun has none.
pub fn lensfun_focal_range(lens: &Lens) -> [f32; 2] {
    let (mut min_focal, mut max_focal) = match &lens.focal {
        Some(f) => match f.value {
            Some(v) => (v, v),
            None => (f.min.unwrap_or(0.), f.max.unwrap_or(0.)),
        },
        None => (0., 0.),
    };
    let min_aperture = lens.aperture.as_ref().and_then(|a| a.min).unwrap_or(0.);
    if min_aperture == 0. || min_focal == 0. {
        let (mut lo, mut hi) = (f32::MAX, f32::MIN_POSITIVE);
        let model = untranslated(&lens.model);
        if !model.is_empty()
            && !NOT_A_LENS.iter().any(|w| model.contains(w))
            && !EXTENDER.is_match(model)
            && let Some((found, [min_group, max_group])) = NAME_PATTERNS
                .iter()
                .find_map(|(re, groups)| re.captures(model).map(|c| (c, *groups)))
        {
            if let Some(m) = found.get(min_group) {
                lo = atof(m.as_str());
            }
            if let Some(m) = found.get(max_group) {
                hi = atof(m.as_str());
            }
        }
        for focal in lens
            .calibration
            .iter()
            .flat_map(|c| &c.elements)
            .map(|e| match e {
                CalibrationElement::Distortion(d) => d.focal,
                CalibrationElement::Tca(t) => t.focal,
                CalibrationElement::Vignetting(v) => v.focal,
            })
        {
            lo = lo.min(focal);
            hi = hi.max(focal);
        }
        if lo != f32::MAX && min_focal == 0. {
            min_focal = lo;
        }
        if hi != f32::MIN_POSITIVE && max_focal == 0. {
            max_focal = hi;
        }
    }
    if max_focal == 0. {
        max_focal = min_focal;
    }
    [min_focal, max_focal]
}

/// C's `atof` on the digits and dots a pattern captured: the longest number at
/// the start, 0 for none.
fn atof(text: &str) -> f32 {
    let mut seen_dot = false;
    let end = text
        .char_indices()
        .find(|&(_, c)| {
            let stop = !(c.is_ascii_digit() || (c == '.' && !seen_dot));
            seen_dot |= c == '.';
            stop
        })
        .map_or(text.len(), |(i, _)| i);
    text[..end].parse().unwrap_or(0.)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lens(xml: &str) -> Lens {
        quick_xml::de::from_str(xml).expect("a lens")
    }

    const CALIBRATION: &str = r#"<calibration>
        <vignetting model="pa" focal="72" aperture="4" distance="10" k1="-0.1" k2="0" k3="0"/>
        <distortion model="ptlens" focal="190" a="0" b="0" c="0"/>
    </calibration>"#;

    #[test]
    fn the_focal_element_wins() {
        let l = lens(&format!(
            "<lens><model>Canon EF 70-200mm f/2.8L</model><focal min=\"60\" max=\"210\"/>{CALIBRATION}</lens>"
        ));
        assert_eq!(lensfun_focal_range(&l), [60., 210.]);
        let l = lens(&format!(
            "<lens><model>Canon EF 70-200mm f/2.8L</model><focal value=\"85\"/>{CALIBRATION}</lens>"
        ));
        assert_eq!(lensfun_focal_range(&l), [85., 85.]);
    }

    /// The name's range, widened to the calibration's; the untranslated name.
    #[test]
    fn the_name_states_the_range_then_the_calibration_widens_it() {
        let l = lens(&format!(
            "<lens><model>Canon EF 70-200mm f/2.8L</model><model lang=\"de\">Kanon 10-20mm f/4</model>{CALIBRATION}</lens>"
        ));
        assert_eq!(lensfun_focal_range(&l), [70., 200.]);
        let l = lens(&format!(
            "<lens><model>Canon EF 100-180mm f/2.8L</model>{CALIBRATION}</lens>"
        ));
        assert_eq!(lensfun_focal_range(&l), [72., 190.]);
        // "1:" before the focal lengths, as Pentax and Zeiss write it.
        let l = lens(&format!(
            "<lens><model>smc PENTAX-DA 1:3.5-5.6 75-180mm</model>{CALIBRATION}</lens>"
        ));
        assert_eq!(lensfun_focal_range(&l), [72., 190.]);
        let l = lens(&format!(
            "<lens><model>Lumix G Vario 1:4-5.6/80-150</model>{CALIBRATION}</lens>"
        ));
        assert_eq!(lensfun_focal_range(&l), [72., 190.]);
    }

    /// A converter's name, or one that states no range, leaves the calibration's.
    #[test]
    fn no_range_from_a_converter_or_a_bare_name() {
        for model in [
            "Canon EF 100-150mm f/2.8L + 1.4x",
            "Tele converter 100-150mm f/4",
            "PowerShot G7 X",
        ] {
            let l = lens(&format!("<lens><model>{model}</model>{CALIBRATION}</lens>"));
            assert_eq!(lensfun_focal_range(&l), [72., 190.], "{model}");
        }
    }

    /// A known widest aperture and shortest focal length stop the guessing: a
    /// `<focal min>` alone then leaves the longest at the shortest.
    #[test]
    fn a_lens_with_its_focal_and_aperture_guesses_nothing() {
        let l = lens(&format!(
            "<lens><model>Canon EF 70-200mm f/2.8L</model><focal min=\"70\"/><aperture min=\"2.8\"/>{CALIBRATION}</lens>"
        ));
        assert_eq!(lensfun_focal_range(&l), [70., 70.]);
    }

    #[test]
    fn atof_reads_the_leading_number() {
        assert_eq!(atof("2.8"), 2.8);
        assert_eq!(atof("1.4.5"), 1.4);
        assert_eq!(atof("."), 0.);
        assert_eq!(atof("70"), 70.);
    }
}
