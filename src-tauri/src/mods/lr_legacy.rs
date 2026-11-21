//! Lightroom settings from before process version 2012 (Lightroom 3 and 4,
//! process versions 2003 and 2010), converted into the 2012 controls the
//! preset import reads, as Lightroom 5's "Update to Current Process (2012)"
//! writes them. Ours, adapted from RAWmakase (MIT).
//!
//! RAWmakase eb8a3356, 72eec1bf: crates/rawmakase-interop/src/lr_develop.rs
//! (`legacy_tone`, the auto-tone rule, zeroed 2012 keys ignored).
//! RAWmakase c83741ea: crates/rawmakase-interop/src/xmp/mod.rs
//! (`before_process_2012`, `Preset::point_curve_keys`).
//!
//! Without this, an old preset kept only its colour controls: Exposure,
//! Contrast, Brightness, Fill Light, Recovery, Shadows, Clarity and the tone
//! curve were dropped because the import reads only the 2012 keys.

use std::collections::{BTreeMap, HashMap};

/// The process version 2003/2010 basic controls `legacy_tone` reads.
const LEGACY_KEYS: [&str; 7] = [
    "Exposure",
    "Contrast",
    "Brightness",
    "FillLight",
    "Shadows",
    "HighlightRecovery",
    "Clarity",
];

/// The 2012 basic controls. Lightroom 5 stores them at 0 in older records and
/// ignores them there (RAWmakase 72eec1bf).
const KEYS_2012: [&str; 7] = [
    "Exposure2012",
    "Contrast2012",
    "Highlights2012",
    "Shadows2012",
    "Whites2012",
    "Blacks2012",
    "Clarity2012",
];

/// Lightroom's placeholder for an Auto Tone value not yet measured.
const AUTO_PENDING: f64 = -999999.;

/// Whether a `ProcessVersion` value is from before process version 2012 (6.7).
fn before_process_2012(version: &str) -> bool {
    version.parse::<f32>().is_ok_and(|v| v < 6.7)
}

fn process_version(attrs: &HashMap<String, String>) -> Option<&str> {
    attrs.get("ProcessVersion").map(|v| v.trim())
}

fn is_legacy(attrs: &HashMap<String, String>) -> bool {
    process_version(attrs).is_some_and(before_process_2012)
}

/// The point-curve set these settings render: before process version 2012
/// Lightroom renders the `ToneCurve` set and ignores the `ToneCurvePV2012`
/// set Lightroom 5 stores beside it (RAWmakase c83741ea).
pub fn point_curve_prefix(attrs: &HashMap<String, String>) -> &'static str {
    if is_legacy(attrs) {
        "ToneCurve"
    } else {
        "ToneCurvePV2012"
    }
}

/// For settings from before process version 2012, replaces the 2012 basic
/// controls with the old controls converted. Settings from 2012 on, or with no
/// process version, are left exactly as they are.
pub fn apply_legacy_tone(attrs: &mut HashMap<String, String>) {
    let Some(version) = process_version(attrs) else {
        return;
    };
    if !before_process_2012(version) {
        return;
    }
    // Only real 2003 records (below 5.7); the Clarity scale was fitted on those.
    let pv2003 = version.parse::<f32>().is_ok_and(|v| v < 5.7);
    let old: BTreeMap<&str, f64> = LEGACY_KEYS
        .iter()
        .filter_map(|&key| {
            let value = attrs
                .get(key)?
                .trim()
                .trim_start_matches('+')
                .parse()
                .ok()?;
            Some((key, value))
        })
        .collect();
    // An explicit Exposure, even 0, means Auto Tone already ran; without one
    // an Auto Tone record gets none.
    let auto = attrs
        .get("AutoTone")
        .is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1"));
    let converted = legacy_tone(&old, pv2003);
    for key in KEYS_2012 {
        attrs.remove(key);
    }
    for (key, value) in converted {
        // RAWmakase writes the Auto Tone placeholder back for its own Auto
        // Tone to measure; this import measures nothing, so it is left out.
        let write = if key == "Exposure2012" {
            value != AUTO_PENDING && (old.contains_key("Exposure") || (!auto && value != 0.))
        } else {
            value != 0.
        };
        if write {
            attrs.insert(key.to_string(), value.to_string());
        }
    }
}

/// Process version 2003/2010 basic controls in process version 2012, as
/// Lightroom 5's "Update to Current Process (2012)" writes them. Every rule
/// reproduces at least 99% of 341 updated catalog records (RAWmakase
/// eb8a3356). Fill Light also adds exposure, 0.02 EV a step up to 25, 0.01 up
/// to 50 and 0.005 above.
fn legacy_tone(old: &BTreeMap<&str, f64>, pv2003: bool) -> [(&'static str, f64); 7] {
    // The placeholder on Exposure leaves Auto pending; on another control it
    // counts as that control's default.
    let auto = old.get("Exposure") == Some(&AUTO_PENDING);
    let get = |key: &str, default: f64| {
        old.get(key)
            .copied()
            .filter(|&v| v != AUTO_PENDING)
            .unwrap_or(default)
    };
    let fill = get("FillLight", 0.);
    let fill_ev =
        0.02 * fill.min(25.) + 0.01 * (fill.clamp(25., 50.) - 25.) + 0.005 * (fill.max(50.) - 50.);
    let exposure = get("Exposure", 0.) + (get("Brightness", 50.) - 50.) / 50. + fill_ev;
    // The old Shadows slider set the black point; 5 was its default.
    let black = get("Shadows", 5.);
    let clarity = get("Clarity", 0.);
    [
        (
            "Exposure2012",
            if auto {
                AUTO_PENDING
            } else {
                ((exposure * 100.).round() / 100.).clamp(-5., 5.)
            },
        ),
        (
            "Contrast2012",
            ((get("Contrast", 25.) - 25.) * 4. / 3.)
                .round()
                .clamp(-100., 100.),
        ),
        ("Highlights2012", -(fill * 0.5).trunc()),
        ("Shadows2012", (fill * 0.5).clamp(0., 100.).trunc()),
        ("Whites2012", -get("HighlightRecovery", 0.)),
        (
            "Blacks2012",
            if black <= 5. {
                5. * (5. - black)
            } else {
                ((5. - black) / 4.).round()
            },
        ),
        // Lightroom 5 writes half of positive Clarity. On process version 2003
        // records a fifth comes closer to Camera Raw 19's own rendering.
        (
            "Clarity2012",
            if clarity > 0. {
                (clarity * if pv2003 { 0.2 } else { 0.5 }).trunc()
            } else {
                clarity
            },
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn get(attrs: &HashMap<String, String>, key: &str) -> Option<f64> {
        attrs.get(key).map(|v| v.parse().expect("a number"))
    }

    /// RAWmakase eb8a3356's case: a process version 2003 record with the
    /// 2012 keys stored at 0, which Lightroom ignores.
    #[test]
    fn process_2003_controls_convert_over_zeroed_2012_keys() {
        let mut a = attrs(&[
            ("ProcessVersion", "5.0"),
            ("Exposure", "+1.70"),
            ("Exposure2012", "0.00"),
            ("Contrast", "+17"),
            ("Contrast2012", "0"),
            ("Brightness", "+3"),
            ("FillLight", "100"),
            ("Shadows", "0"),
            ("HighlightRecovery", "20"),
            ("Clarity", "+100"),
            ("Clarity2012", "0"),
        ]);
        apply_legacy_tone(&mut a);
        let got = KEYS_2012.map(|k| get(&a, k));
        let want = [-11., -50., 50., -20., 25., 20.].map(Some);
        assert_eq!(get(&a, "Exposure2012"), Some(1.76));
        assert_eq!(got[1..], want, "{a:?}");
    }

    #[test]
    fn process_2010_halves_positive_clarity() {
        let mut a = attrs(&[("ProcessVersion", "5.7"), ("Clarity", "100")]);
        apply_legacy_tone(&mut a);
        assert_eq!(get(&a, "Clarity2012"), Some(50.));
        let mut a = attrs(&[("ProcessVersion", "5.7"), ("Clarity", "-40")]);
        apply_legacy_tone(&mut a);
        assert_eq!(get(&a, "Clarity2012"), Some(-40.));
    }

    #[test]
    fn brightness_and_quoted_numbers_add_exposure() {
        let mut a = attrs(&[
            ("ProcessVersion", "5.7"),
            ("Exposure", "0.75"),
            ("Brightness", "100"),
        ]);
        apply_legacy_tone(&mut a);
        assert_eq!(get(&a, "Exposure2012"), Some(1.75));
    }

    /// An explicit Exposure, even 0, means Auto Tone already ran. Lightroom's
    /// placeholder, or no Exposure at all, leaves Exposure unset: the import
    /// does not measure the photo.
    #[test]
    fn auto_tone_records_keep_or_leave_exposure() {
        let mut a = attrs(&[
            ("ProcessVersion", "5.0"),
            ("AutoTone", "True"),
            ("Exposure", "0"),
            ("Contrast", "40"),
        ]);
        apply_legacy_tone(&mut a);
        assert_eq!(get(&a, "Exposure2012"), Some(0.));
        assert_eq!(get(&a, "Contrast2012"), Some(20.));
        for pending in [
            &[("Exposure", "-999999"), ("Contrast", "-999999")][..],
            &[("Brightness", "100"), ("FillLight", "25")][..],
        ] {
            let mut a = attrs(&[("ProcessVersion", "5.0"), ("AutoTone", "True")]);
            a.extend(attrs(pending));
            apply_legacy_tone(&mut a);
            assert_eq!(get(&a, "Exposure2012"), None, "{pending:?}");
            assert_eq!(get(&a, "Contrast2012"), None, "{pending:?}");
        }
    }

    #[test]
    fn process_2012_and_unversioned_settings_are_untouched() {
        for version in [Some("6.7"), Some("11.0"), None] {
            let mut a = attrs(&[
                ("Exposure", "1"),
                ("FillLight", "50"),
                ("Exposure2012", "0.30"),
                ("Shadows2012", "-20"),
            ]);
            if let Some(v) = version {
                a.insert("ProcessVersion".into(), v.into());
            }
            let before = a.clone();
            apply_legacy_tone(&mut a);
            assert_eq!(a, before, "{version:?}");
        }
    }

    #[test]
    fn the_curve_set_follows_the_process_version() {
        let legacy = attrs(&[("ProcessVersion", "5.7")]);
        let current = attrs(&[("ProcessVersion", "6.7")]);
        assert_eq!(point_curve_prefix(&legacy), "ToneCurve");
        assert_eq!(point_curve_prefix(&current), "ToneCurvePV2012");
        assert_eq!(point_curve_prefix(&HashMap::new()), "ToneCurvePV2012");
    }
}
