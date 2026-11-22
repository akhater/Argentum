//! RapidRAW 1.7.0's demosaic (`argentum_engine::rr_demosaic`) in the app's
//! decode. Ours.
//!
//! Edits made since the demosaic rule (`highlight_rule::Rule::
//! PullShadowsDemosaic` and the rules after it) are demosaiced
//! by it: RCD with residual interpolation on Bayer sensors, Markesteijn's
//! 3-pass on X-Trans. Edits saved before keep rawler's.
//!
//! It runs where rawler's would, on the mosaic after the decode's own fixes
//! (`decode.rs`), and hands rawler a three-colour picture in the scale and
//! space its own demosaic hands to calibration, so the highlight pull, the
//! matrix and the crop run as before.

use rawler::RawImageData;
use rawler::imgop::raw::{clip_at_black_balanced, clip_negative};
use rawler::rawimage::RawImage;
use rayon::prelude::*;

/// Demosaic `raw` with RapidRAW's, in place: its samples become camera RGB
/// over its active area, three floats a pixel, black at 0 and `white` at
/// 1 / (`u32::MAX` - black), not white balanced: what rawler's demosaic gives
/// calibration. The caller develops it without rawler's rescale, which this
/// has done. False, with `raw` as it was, when the demosaic cannot take the
/// mosaic (`Mosaic::new`): rawler's then demosaics it.
///
/// `white` is the photo's own white level: the app has put `u32::MAX` in
/// `raw` by now (`raw_processing::develop_decoded`). A pixel below black
/// keeps its colour as rawler's develop would keep it after its own
/// demosaic when `keep_colour_below_black` (the shadow rule), and is clipped
/// channel by channel otherwise.
pub fn demosaic_in_place(raw: &mut RawImage, white: f32, keep_colour_below_black: bool) -> bool {
    // As calibration will balance it (rawler: `calibration_white_balance`).
    let calibration = if raw.wb_coeffs[0].is_nan() {
        [1.0; 4]
    } else {
        raw.wb_coeffs
    };
    // White balanced before demosaic, as RapidRAW does (residual
    // interpolation fits red and blue to green), green at 1, the scale its
    // constants are for. Taken off again after, for rawler's calibration.
    let wb = [
        calibration[0] / calibration[1],
        1.0,
        calibration[2] / calibration[1],
    ];
    let Some(mosaic) = argentum_engine::rr_demosaic::Mosaic::new(raw, wb, white) else {
        return false;
    };
    let domain = mosaic.domain();
    let started = std::time::Instant::now();
    let mut data = mosaic.develop().into_flattened();
    log::info!(
        "RapidRAW {:?} demosaic of {}x{} took {:?}",
        mosaic.pattern(),
        domain.d.w,
        domain.d.h,
        started.elapsed()
    );
    // rawler's scale: (sample - black) / (u32::MAX - black).
    let black = raw.blacklevel.as_bayer_array()[0];
    let to_rawler = (white - black).max(1.0) / (u32::MAX as f32 - black);
    let gains = wb.map(|g| to_rawler / g);
    data.par_chunks_exact_mut(3).for_each(|px| {
        let unbalanced = [px[0] * gains[0], px[1] * gains[1], px[2] * gains[2]];
        // rawler keeps the colour of a pixel below black only after its own
        // full-size Bayer demosaic: this is that step, for this one.
        let kept = if keep_colour_below_black {
            clip_at_black_balanced(&unbalanced, &calibration)
        } else {
            clip_negative(&unbalanced)
        };
        px.copy_from_slice(&kept);
    });
    raw.data = RawImageData::Float(data);
    raw.cpp = 3;
    raw.width = domain.d.w;
    raw.height = domain.d.h;
    // `active_area` stays: rawler crops the default crop against it, and the
    // picture is now exactly that area, as its own demosaic leaves it.
    true
}
