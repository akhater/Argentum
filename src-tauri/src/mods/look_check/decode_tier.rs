//! The decode tier: what the decode did to a sample, exactly, with no GPU.
//!
//! The exposure offset DNG Standard gives the photo and where it came from,
//! what a highlight-protecting camera mode held back, where each colour stops
//! being data and the lower white the file proves, the photosites recovery
//! and `settle_blown` rewrote, the DNG gain maps applied and the built-in lens
//! correction found. All of it from the app's own code: the decode as
//! `raw_processing::develop_internal` does it, then the decode hook
//! (`decode::on_raw_decoded_watched`), which shows the mosaic between the
//! steps that rewrite it.

use rawler::decoders::RawDecodeParams;
use rawler::rawimage::RawPhotometricInterpretation;

use crate::mods::decode::{self, Watch};
use crate::mods::{dng_standard, highlights};

/// One decode case's lines, in order.
pub type Lines = Vec<(String, String)>;

/// The decode lines that may differ by a rounding, compared within 0.001.
const EV_KEYS: [&str; 2] = ["baseline_ev", "held_back_ev"];

/// What the decode did, as the lines of the `decode` case, and whether the
/// mosaic is X-Trans.
pub fn values(bytes: &[u8], path: &str) -> Result<(Lines, bool), String> {
    let facts = dng_standard::facts(bytes)?;
    let baseline = dng_standard::baseline(&facts);

    let source = rawler::rawsource::RawSource::new_from_slice(bytes);
    let decoder = rawler::get_decoder(&source).map_err(|e| e.to_string())?;
    let mut raw = decoder
        .raw_image(&source, &RawDecodeParams::default(), false)
        .map_err(|e| e.to_string())?;
    let (layout, x_trans) = match &raw.photometric {
        RawPhotometricInterpretation::Cfa(c) => (
            format!("{}x{} {}", c.cfa.width, c.cfa.height, c.cfa.name),
            c.cfa.width == 6,
        ),
        other => (format!("{} a pixel, {other:?}", raw.cpp), false),
    };
    let levels = format!(
        "{} {}",
        raw.blacklevel.levels.first().map_or(0.0, |v| v.as_f32()),
        raw.whitelevel.0.first().copied().unwrap_or(0)
    );

    let mut proven = None;
    let mut before: Vec<f32> = Vec::new();
    let (mut recovered, mut settled) = (0usize, 0usize);
    let changed = |a: &[f32], b: &[f32]| a.iter().zip(b).filter(|(x, y)| x != y).count();
    // The mosaic steps it counts are the ones an edit saved before the pull
    // still renders with; the pull runs after demosaic, where this does not look.
    let report = decode::on_raw_decoded_watched(
        &mut raw,
        bytes,
        Some(path),
        crate::mods::highlight_rule::Rule::Rebuild,
        &mut |at, raw| {
            let now = raw.data.as_f32();
            match at {
                Watch::Judged => proven = highlights::proven_white_of(raw),
                Watch::Recovered => recovered = changed(&before, &now),
                Watch::Settled => settled = changed(&before, &now),
            }
            before = now.into_owned();
        },
    );

    let held = facts.held_back;
    let lens = argentum_engine::rm_lens_builtin::read(bytes);
    let line = |k: &str, v: String| (k.to_string(), v);
    let none = || "-".to_string();
    Ok((
        vec![
            line("camera", format!("{} {}", facts.make, facts.model)),
            line("layout", layout),
            line("levels", levels),
            line("baseline_ev", format!("{:+.3}", baseline.ev)),
            line("baseline_origin", format!("{:?}", baseline.origin)),
            line(
                "held_back_mode",
                held.map_or_else(none, |h| format!("{:?}", h.mode)),
            ),
            line(
                "held_back_ev",
                held.map_or_else(none, |h| format!("{:+.3}", h.ev)),
            ),
            line(
                "ceilings",
                report
                    .ceilings
                    .map_or_else(none, |c| c.map(|v| format!("{v:.2}")).join(" ")),
            ),
            line(
                "proven_white",
                proven.map_or_else(none, |w| format!("{w:.0}")),
            ),
            line(
                "recovery",
                if report.rebuilt {
                    "in charge"
                } else {
                    "not in charge"
                }
                .into(),
            ),
            line("recovered_photosites", recovered.to_string()),
            line("settled_photosites", settled.to_string()),
            line("gain_maps", report.gain_maps.to_string()),
            line(
                "lens",
                lens.map_or_else(none, |c| {
                    let on = if c.default_on { "on" } else { "off" };
                    format!("{} ({on})", c.source)
                }),
            ),
        ],
        x_trans,
    ))
}

/// The lines that differ between `then` and `now`, one line each.
pub fn drift(then: &[(String, String)], now: &[(String, String)]) -> Vec<String> {
    let keys: std::collections::BTreeSet<&str> =
        then.iter().chain(now).map(|(k, _)| k.as_str()).collect();
    let get = |lines: &[(String, String)], key: &str| {
        lines
            .iter()
            .find(|(k, _)| k == key)
            .map_or_else(|| "(none)".to_string(), |(_, v)| v.clone())
    };
    keys.into_iter()
        .filter_map(|key| {
            let (a, b) = (get(then, key), get(now, key));
            let same = match (EV_KEYS.contains(&key), a.parse::<f64>(), b.parse::<f64>()) {
                (true, Ok(x), Ok(y)) => (x - y).abs() <= 0.001 + 1e-9,
                _ => a == b,
            };
            (!same).then(|| format!("{key}: {a} -> {b}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(pairs: &[(&str, &str)]) -> Lines {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn an_offset_may_move_by_a_thousandth_and_nothing_else_may_move() {
        let then = lines(&[("baseline_ev", "+0.250"), ("recovered_photosites", "364")]);
        assert!(
            drift(
                &then,
                &lines(&[("baseline_ev", "+0.251"), ("recovered_photosites", "364")])
            )
            .is_empty()
        );
        assert_eq!(
            drift(
                &then,
                &lines(&[("baseline_ev", "+0.252"), ("recovered_photosites", "364")])
            ),
            ["baseline_ev: +0.250 -> +0.252"]
        );
        assert_eq!(
            drift(
                &then,
                &lines(&[("baseline_ev", "+0.250"), ("recovered_photosites", "365")])
            ),
            ["recovered_photosites: 364 -> 365"]
        );
        assert_eq!(
            drift(&then, &lines(&[("baseline_ev", "+0.250")])),
            ["recovered_photosites: 364 -> (none)"]
        );
    }
}
