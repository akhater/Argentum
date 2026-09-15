//! As-shot white balance for the Canon bodies that predate ColorData.
//!
//! THE BUG
//!
//! The EOS-1D and EOS-1Ds are older than Canon's ColorData block (MakerNote
//! 0x4001). They keep their white balance in MakerNote 0x00a4 instead — a table
//! of presets, the shot's own first, each a triple normalised so green is 512.
//!
//! rawler knows about that tag. `Cr2Decoder::get_wb` falls back to it by name
//! (`TiffCommonTag::Cr2OldWB`, 0x00a4) when there is no ColorData. But it looks
//! for it with `self.tiff.get_entry`, which walks the root IFD chain — and
//! 0x00a4 is a *MakerNote* tag. Every other makernote read in that file goes
//! through `self.makernote`; this one does not. So the entry is never found,
//! `get_wb` returns `[NaN; 4]`, and `develop_intermediate` substitutes
//! `[1, 1, 1, 1]` for a NaN first coefficient.
//!
//! A Bayer frame developed with no white balance at all is green, because green
//! has two of the four sites and the most sensitivity. That is the whole of it:
//! not a colour-science problem, no white balance applied.
//!
//! Measured on the 1Ds file from RapidRAW issue #1678, `91BX8040.TIF`:
//!
//! ```text
//!   rawler wb_coeffs   [NaN, NaN, NaN, NaN]
//!   MakerNote 0x00a4   [842, 512, 633, 134, ...]   576 shorts
//! ```
//!
//! It also explains why the reporter could not dial it out with the white
//! balance slider: those are relative adjustments on top of a frame that never
//! received the camera's multipliers, and the camera matrix amplifies whatever
//! is left.
//!
//! WHY THE SCALE IS DIVIDED OUT
//!
//! rawler's ColorData path ends in `normalize_wb`, which divides by green.
//! The 0x00a4 branch does not — it hands back the stored integers. Those are
//! normalised to green = 512, so passing them through unchanged would be a
//! uniform 512x gain. Everything downstream expects green = 1, so that is what
//! this writes.
//!
//! GENERAL, NOT PER CAMERA
//!
//! The trigger is the condition, not the model: a Canon whose white balance
//! came back as NaN, with a plausible 0x00a4 table in the file. Nothing here
//! names a body. On anything that already has coefficients it does not run.
//!
//! WHY HERE AND NOT IN RAWLER
//!
//! Same reason as `sraw_levels`: rawler is a git dependency, and patching it
//! means carrying a fork of a fork. The upstream fix is one word — reading
//! 0x00a4 off the makernote rather than the root IFD — and worth sending, but
//! this repairs it today without one.

use rawler::rawimage::RawImage;

/// Canon MakerNote tag 0x00a4: the old white balance table.
const CANON_OLD_WB: u16 = 0x00a4;

/// The table is presets of twelve shorts each; the shot's own is the first.
/// Anything shorter than one triple is not it, and the bodies that carry this
/// write a few hundred entries.
const PLAUSIBLE_LENGTH: std::ops::RangeInclusive<usize> = 3..=4096;

/// Give a pre-ColorData Canon the white balance it shipped with.
///
/// A no-op unless rawler came back with nothing, which is the only case this
/// can improve on.
pub fn fix(raw: &mut RawImage, file_bytes: &[u8]) {
    if !raw.wb_coeffs[0].is_nan() || !raw.clean_make.eq_ignore_ascii_case("canon") {
        return;
    }

    if let Some(coeffs) = as_shot(file_bytes) {
        log::info!("Canon pre-ColorData white balance from 0x00a4: {coeffs:?}");
        raw.wb_coeffs = coeffs;
    }
}

/// The as-shot triple, green-normalised, or nothing.
fn as_shot(file_bytes: &[u8]) -> Option<[f32; 4]> {
    let table = super::canon_makernote::shorts(file_bytes, CANON_OLD_WB, PLAUSIBLE_LENGTH)?;
    normalise(
        table.first().copied()?,
        table.get(1).copied()?,
        table.get(2).copied()?,
    )
}

/// `[r, g, b]` as stored becomes `[r/g, 1, b/g, NaN]`.
///
/// A zero or absurd ratio means the table was not what we thought it was, and a
/// made-up white balance is worse than none: rawler's own fallback at least
/// leaves the picture obviously wrong rather than plausibly wrong.
pub fn normalise(r: u16, g: u16, b: u16) -> Option<[f32; 4]> {
    if r == 0 || g == 0 || b == 0 {
        return None;
    }
    let (r, g, b) = (r as f32 / g as f32, 1.0, b as f32 / g as f32);
    // No camera needs a channel eight times another to reach neutral. Beyond
    // that the numbers are not white balance.
    let sane = (0.125..=8.0).contains(&r) && (0.125..=8.0).contains(&b);
    sane.then_some([r, g, b, f32::NAN])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The numbers out of AK's copy of the 1Ds file from issue #1678.
    #[test]
    fn the_1ds_table_normalises_to_green() {
        let wb = normalise(842, 512, 633).expect("a white balance");
        assert!((wb[0] - 1.6445).abs() < 1e-3, "red was {}", wb[0]);
        assert_eq!(wb[1], 1.0);
        assert!((wb[2] - 1.2363).abs() < 1e-3, "blue was {}", wb[2]);
        assert!(wb[3].is_nan(), "the fourth channel is not a colour here");
    }

    /// Nonsense in the table is declined rather than turned into a cast.
    #[test]
    fn declines_a_table_that_is_not_a_white_balance() {
        assert!(normalise(0, 512, 633).is_none(), "zero red");
        assert!(
            normalise(842, 0, 633).is_none(),
            "zero green, a divide by zero"
        );
        assert!(normalise(842, 512, 0).is_none(), "zero blue");
        assert!(normalise(60000, 512, 633).is_none(), "red far out of range");
        assert!(normalise(842, 512, 1).is_none(), "blue far out of range");
    }

    /// A neutral table is a legitimate answer, not a suspicious one.
    #[test]
    fn a_neutral_table_is_accepted() {
        let wb = normalise(512, 512, 512).expect("a white balance");
        assert_eq!([wb[0], wb[1], wb[2]], [1.0, 1.0, 1.0]);
    }
}

/// What this module finds in a file, printed so it can be checked against an
/// independent parse of the same bytes.
#[cfg(test)]
mod facts {
    #[test]
    #[ignore = "reads a file named by AG_RAW; run by hand"]
    fn what_we_read_from_the_makernote() {
        let path = std::env::var("AG_RAW").expect("set AG_RAW");
        let bytes = std::fs::read(&path).expect("read raw");
        println!("\nfile      {path}");
        println!("as_shot   {:?}", super::as_shot(&bytes));
        println!(
            "table     {:?}",
            crate::mods::canon_makernote::shorts(&bytes, super::CANON_OLD_WB, 3..=4096)
                .map(|t| t[..12.min(t.len())].to_vec())
        );
    }
}
