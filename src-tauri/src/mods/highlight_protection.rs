//! The exposure a camera held back to protect highlights, given back in DNG
//! Standard.
//!
//! Some camera modes record the RAW darker by a known amount and brighten their
//! own JPEG to match: Fujifilm DR200 and DR400, Canon Highlight Tone Priority,
//! Nikon Active D-Lighting and HLG, Olympus gradation, Pentax dynamic-range
//! expansion. RapidRAW 1.6.6 reads the amount from the maker notes
//! (`baseline_exposure.rs` @ bc7791a4, after darktable's
//! `_check_highlight_preservation`), and this reads it through theirs.
//!
//! Their total also counts Canon's Auto Lighting Optimizer, which is on by
//! default in Canon bodies, does not darken the RAW, and is not given back by
//! Adobe. DNG Standard renders as Adobe does, so that share comes out here
//! (decided 2026-10-20). The Canon reading is theirs, cut down to that one
//! value.

use rawler::decoders::{Decoder, FormatHint};
use rawler::formats::bmff::Bmff;
use rawler::formats::tiff::reader::TiffReader;
use rawler::formats::tiff::{IFD, ifd::OffsetMode};
use rawler::tags::ExifTag;
use serde::Serialize;
use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Below this a mode is not worth naming, or adding.
const SMALLEST_EV: f32 = 0.05;

/// Canon's LightingOpt maker-note tag; element 2 is the Auto Lighting Optimizer.
const CANON_LIGHTING_OPT: u16 = 0x4018;

/// A camera mode that recorded the photo darker, and by how many stops.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldBack {
    pub mode: Mode,
    pub ev: f32,
}

/// The modes, named as the Profile note names them (src/argentum/locales).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    FujifilmDr200,
    FujifilmDr400,
    CanonHighlightTonePriority,
    NikonHlg,
    NikonActiveDLighting,
    OlympusGradation,
    PentaxDynamicRange,
}

/// What this RAW's camera held back, if anything Adobe would give back.
pub fn held_back(decoder: &dyn Decoder, bytes: &[u8], make: &str) -> Option<HeldBack> {
    let total = crate::baseline_exposure::read(decoder, bytes, false).highlight_preservation;
    let ev = total - canon_auto_lighting_optimizer_ev(decoder.format_hint(), bytes);
    mode_of(make, ev).map(|mode| HeldBack { mode, ev })
}

/// Which mode a maker's stops come from. Their reader returns one number per
/// file, and each maker has one mode that adds stops, or two told apart by
/// how many: Fujifilm's DR200 (1) from DR400 (2), Nikon's HLG (2) from Active
/// D-Lighting (at most 1.33).
fn mode_of(make: &str, ev: f32) -> Option<Mode> {
    if ev < SMALLEST_EV {
        return None;
    }
    let make = make.to_ascii_lowercase();
    let mode = if make.starts_with("fujifilm") {
        if ev >= 1.5 {
            Mode::FujifilmDr400
        } else {
            Mode::FujifilmDr200
        }
    } else if make.starts_with("canon") {
        Mode::CanonHighlightTonePriority
    } else if make.starts_with("nikon") {
        if ev >= 1.9 {
            Mode::NikonHlg
        } else {
            Mode::NikonActiveDLighting
        }
    } else if make.starts_with("olympus") || make.starts_with("om digital") {
        Mode::OlympusGradation
    } else if make.starts_with("pentax") || make.starts_with("ricoh") {
        Mode::PentaxDynamicRange
    } else {
        return None;
    };
    Some(mode)
}

/// The stops RapidRAW's reader counts for Canon's Auto Lighting Optimizer.
fn canon_auto_lighting_optimizer_ev(format: FormatHint, bytes: &[u8]) -> f32 {
    catch_unwind(AssertUnwindSafe(|| match format {
        FormatHint::CR2 => tiff_makernote(bytes).map(|makernote| lighting_optimizer_ev(&makernote)),
        FormatHint::CR3 => Bmff::new_buf(bytes)
            .ok()
            .and_then(|bmff| bmff.filebox.moov.cr3desc)
            .map(|desc| lighting_optimizer_ev(desc.cmt3.tiff.root_ifd())),
        _ => None,
    }))
    .ok()
    .flatten()
    .unwrap_or(0.0)
}

fn lighting_optimizer_ev(makernote: &IFD) -> f32 {
    match integer(makernote, CANON_LIGHTING_OPT, 2) {
        Some(0) => 0.5,
        Some(1) => 0.33,
        Some(2) => 0.66,
        _ => 0.0,
    }
}

fn tiff_makernote(bytes: &[u8]) -> Option<IFD> {
    let mut cursor = Cursor::new(bytes);
    IFD::new_root(&mut cursor, 0)
        .ok()?
        .find_first_ifd_with_tag(ExifTag::MakerNotes)
        .cloned()?
        .parse_makernote(&mut cursor, OffsetMode::Absolute, &[])
        .ok()?
}

/// One value of a tag, as i64, which holds every TIFF integer: rawler's
/// `get_i32` panics on a value above `i32::MAX` instead of failing, and
/// Canon's LightingOpt carries such values beside the one read here.
fn integer(ifd: &IFD, tag: u16, index: usize) -> Option<i64> {
    ifd.get_entry(tag)?.value.get_i64(index).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rawler::formats::tiff::{Entry, Value};

    #[test]
    fn each_maker_names_its_mode() {
        assert_eq!(mode_of("FUJIFILM", 1.0), Some(Mode::FujifilmDr200));
        assert_eq!(mode_of("FUJIFILM", 2.0), Some(Mode::FujifilmDr400));
        assert_eq!(
            mode_of("Canon", 1.0),
            Some(Mode::CanonHighlightTonePriority)
        );
        assert_eq!(mode_of("NIKON CORPORATION", 2.0), Some(Mode::NikonHlg));
        assert_eq!(
            mode_of("NIKON CORPORATION", 0.66),
            Some(Mode::NikonActiveDLighting)
        );
        assert_eq!(
            mode_of("OM Digital Solutions", 0.33),
            Some(Mode::OlympusGradation)
        );
        assert_eq!(
            mode_of("RICOH IMAGING COMPANY, LTD.", 1.0),
            Some(Mode::PentaxDynamicRange)
        );
    }

    #[test]
    fn nothing_held_back_names_nothing() {
        assert_eq!(mode_of("Canon", 0.0), None);
        assert_eq!(mode_of("Sony", 1.0), None);
    }

    #[test]
    fn a_value_too_big_for_i32_does_not_hide_the_lighting_optimizer() {
        let mut makernote = IFD::default();
        makernote.entries.insert(
            CANON_LIGHTING_OPT,
            Entry {
                tag: CANON_LIGHTING_OPT,
                value: Value::Long(vec![u32::MAX, 0, 1, 0]),
                embedded: None,
            },
        );
        assert_eq!(lighting_optimizer_ev(&makernote), 0.33);
    }

    #[test]
    fn a_file_that_is_not_canon_has_no_lighting_optimizer() {
        assert_eq!(
            canon_auto_lighting_optimizer_ev(FormatHint::NEF, b"not a raw"),
            0.0
        );
        assert_eq!(
            canon_auto_lighting_optimizer_ev(FormatHint::CR3, b"not a raw"),
            0.0
        );
    }
}
