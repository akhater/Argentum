//! The radius capture sharpening reads from the RAW: how wide a gaussian
//! blurred the sharpest edge on the sensor. Copied from
//! src-tauri/src/mods/sharpen.rs @ 2bcb953d.

use rawler::rawimage::{RawImage, RawImageData};
use rayon::prelude::*;

use super::MAX_RADIUS_PX;
// Copied from src-tauri/src/mods/sharpen.rs @ 2bcb953d (`measure_radius`).
/// The radius a RAW's mosaic gives, as the app measures it at decode:
/// RawTherapee's measurement for whichever mosaic this is. `None` for sRAW,
/// linear DNGs and anything else without a mosaic.
pub fn measure_radius(raw: &RawImage) -> Option<f32> {
    if raw.cpp != 1 || raw.width < 16 || raw.height < 16 {
        // sRAW and linear DNGs: no mosaic, and RawTherapee does not capture
        // sharpen those at all. Manual radius still works.
        return None;
    }
    let black = raw
        .blacklevel
        .levels
        .first()
        .map(|r| r.as_f32())
        .unwrap_or(0.0);
    let white = *raw.whitelevel.0.first()? as f32;
    if white <= black {
        return None;
    }
    let area = picture_area(raw)?;
    let data = scaled_area(raw, &area, black, white)?;
    let (w, h) = (area.w, area.h);
    let cfa = &raw.camera.cfa;
    // The pattern as seen from the corner of the picture area, which is
    // where RawTherapee's rawData starts.
    let color = |row: usize, col: usize| cfa.color_at(row + area.y, col + area.x);
    let upper = 65535.0;

    let radius = if cfa.width == 6 && cfa.height == 6 {
        let (sy, sx) = xtrans_start(&color)?;
        calc_radius_xtrans(&data, w, h, 1000.0, upper, sy, sx)
    } else if cfa.width == 2 && cfa.height == 2 {
        let fc = [color(0, 0), color(1, 0)];
        calc_radius_bayer(&data, w, h, 1000.0, upper, fc)
    } else {
        return None;
    };
    // RawTherapee: std::min(calcRadius*(...), maxSigma), and gives up on NaN.
    if radius.is_nan() {
        return None;
    }
    Some(radius.min(MAX_RADIUS_PX))
}

/// The part of the sensor that holds the picture: the crop rawler recommends
/// if it has one, else the unmasked area. Measuring across the masked strips
/// round the edge would find the steepest "edge" on the sensor.
#[derive(Clone, Copy, Debug)]
struct Area {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

// Copied from src-tauri/src/mods/sharpen.rs @ 2bcb953d.
fn picture_area(raw: &RawImage) -> Option<Area> {
    let full = Area {
        x: 0,
        y: 0,
        w: raw.width,
        h: raw.height,
    };
    let area = raw
        .crop_area
        .or(raw.active_area)
        .map(|r| Area {
            x: r.p.x,
            y: r.p.y,
            w: r.d.w,
            h: r.d.h,
        })
        .unwrap_or(full);
    let fits = area.x + area.w <= raw.width && area.y + area.h <= raw.height;
    (fits && area.w >= 16 && area.h >= 16).then_some(area)
}

// Copied from src-tauri/src/mods/sharpen.rs @ 2bcb953d.
/// The picture area, black subtracted and scaled as RawTherapee's rawData is
/// at this point: green's white at 65535, which is what its limits - 1000
/// below, clipVal above - are in.
fn scaled_area(raw: &RawImage, area: &Area, black: f32, white: f32) -> Option<Vec<f32>> {
    let scale = 65535.0 / (white - black);
    let stride = raw.width;
    let sample = |i: usize| -> Option<f32> {
        let v = match &raw.data {
            RawImageData::Integer(px) => *px.get(i)? as f32,
            RawImageData::Float(px) => *px.get(i)?,
        };
        Some((v - black).max(0.0) * scale)
    };
    let mut out = Vec::with_capacity(area.w * area.h);
    for row in area.y..area.y + area.h {
        for col in area.x..area.x + area.w {
            out.push(sample(row * stride + col)?);
        }
    }
    Some(out)
}

// Copied from src-tauri/src/mods/sharpen.rs @ 2bcb953d.
/// rt_ calcRadiusBayer, RawTherapee capturesharpening.cc
/// @ c6d04960e14fed89e531cbbdf680021d131b4685.
///
/// The largest ratio between two diagonally adjacent green photosites,
/// ignoring clipped neighbourhoods, says how steep the sharpest edge in the
/// photo is - and so how wide a gaussian blurred it: sigma = sqrt(1 / ln(ratio)).
fn calc_radius_bayer(
    data: &[f32],
    w: usize,
    h: usize,
    lower: f32,
    upper: f32,
    fc: [usize; 2],
) -> f32 {
    (1.0 / bayer_peak(data, w, h, lower, upper, fc).ln()).sqrt()
}

// Copied from src-tauri/src/mods/sharpen.rs @ 2bcb953d (`bayer_peak`), less
// the position it also reports for the app's diagnostic test.
fn bayer_peak(data: &[f32], w: usize, h: usize, lower: f32, upper: f32, fc: [usize; 2]) -> f32 {
    let at = |r: usize, c: usize| data[r * w + c];
    (4..h.saturating_sub(4))
        .into_par_iter()
        .map(|row| {
            let mut ratio = 1.0f32;
            // RawTherapee: col = 5 + (fc[row & 1] & 1), stepping by two, which
            // lands on green on every row - its FC codes green as 1 or 3.
            let green_first = fc[row & 1] == 1 || fc[row & 1] == 3;
            let mut col = 5 + usize::from(green_first);
            while col < w - 4 {
                let val00 = at(row, col);
                if val00 > 0.0 {
                    let val1m1 = at(row + 1, col - 1);
                    let val1p1 = at(row + 1, col + 1);
                    let max_val0 = val00.max(val1m1);
                    if val1m1 > 0.0 && max_val0 > lower {
                        let min_val = val00.min(val1m1);
                        if max_val0 > ratio * min_val {
                            let clipped = if max_val0 == val00 {
                                at(row - 1, col - 1).max(at(row - 1, col + 1)).max(val1p1) >= upper
                            } else {
                                at(row, col - 2)
                                    .max(val00)
                                    .max(at(row + 2, col - 2))
                                    .max(at(row + 2, col))
                                    >= upper
                            };
                            if !clipped {
                                ratio = max_val0 / min_val;
                            }
                        }
                    }
                    let max_val1 = val00.max(val1p1);
                    if val1p1 > 0.0 && max_val1 > lower {
                        let min_val = val00.min(val1p1);
                        if max_val1 > ratio * min_val {
                            let clipped = if max_val1 == val00 {
                                at(row - 1, col - 1).max(at(row - 1, col + 1)).max(val1p1) >= upper
                            } else {
                                val00
                                    .max(at(row, col + 2))
                                    .max(at(row + 2, col))
                                    .max(at(row + 2, col + 2))
                                    >= upper
                            };
                            if !clipped {
                                ratio = max_val1 / min_val;
                            }
                        }
                    }
                }
                col += 2;
            }
            ratio
        })
        .reduce(|| 1.0, |a, b| if b > a { b } else { a })
}

// Copied from src-tauri/src/mods/sharpen.rs @ 2bcb953d.
/// Where RawTherapee starts its X-Trans scan: in rows and columns 6..12, a
/// green whose left and right neighbours differ, with no green above it and
/// none to its left. As `getDeconvAutoRadius_capturesharpening_SE` finds it.
fn xtrans_start(color: &impl Fn(usize, usize) -> usize) -> Option<(usize, usize)> {
    for i in 6..12 {
        for j in 6..12 {
            if color(i, j) == 1
                && color(i, j - 1) != color(i, j + 1)
                && color(i - 1, j) != 1
                && color(i, j - 1) != 1
            {
                return Some((i - 6, j - 6));
            }
        }
    }
    None
}

// Copied from src-tauri/src/mods/sharpen.rs @ 2bcb953d.
/// rt_ calcRadiusXtrans, RawTherapee capturesharpening.cc
/// @ c6d04960e14fed89e531cbbdf680021d131b4685. The same measurement as the
/// Bayer one, on the pairs of greens an X-Trans pattern has.
fn calc_radius_xtrans(
    data: &[f32],
    w: usize,
    h: usize,
    lower: f32,
    upper: f32,
    starty: usize,
    startx: usize,
) -> f32 {
    let at = |r: usize, c: usize| data[r * w + c];
    let rows: Vec<usize> = (starty + 2..h.saturating_sub(4)).step_by(3).collect();
    let max_ratio = rows
        .into_par_iter()
        .map(|row| {
            let mut max_ratio = 1.0f32;
            let mut consider = |a: f32, b: f32| {
                let max_val = a.max(b);
                if max_val > lower {
                    let min_val = a.min(b);
                    if max_val > max_ratio * min_val {
                        max_ratio = max_val / min_val;
                    }
                }
            };
            let mut col = startx + 2;
            while col < w.saturating_sub(4) {
                let valp1p1 = at(row + 1, col + 1);
                let square_clipped = valp1p1
                    .max(at(row + 1, col + 2))
                    .max(at(row + 2, col + 1))
                    .max(at(row + 2, col + 2))
                    >= upper;
                let green_solitary = at(row, col);
                if green_solitary > 1.0
                    && green_solitary < upper
                    && at(row - 1, col - 1).max(at(row - 1, col + 1)) < upper
                {
                    let valp1m1 = at(row + 1, col - 1);
                    if valp1m1 > 1.0
                        && at(row + 1, col - 2)
                            .max(valp1m1)
                            .max(at(row + 2, col - 2))
                            .max(at(row + 1, col - 1))
                            < upper
                    {
                        consider(green_solitary, valp1m1);
                    }
                    if valp1p1 > 1.0 && !square_clipped {
                        consider(green_solitary, valp1p1);
                    }
                }
                if !square_clipped {
                    let valp2p2 = at(row + 2, col + 2);
                    if valp2p2 > 1.0 {
                        if valp1p1 > 1.0 {
                            consider(valp1p1, valp2p2);
                        }
                        let right = at(row + 3, col + 3);
                        if right.max(at(row + 4, col + 2)).max(at(row + 4, col + 4)) < upper
                            && right > 1.0
                        {
                            consider(right, valp2p2);
                        }
                    }
                    let valp1p2 = at(row + 1, col + 2);
                    let valp2p1 = at(row + 2, col + 1);
                    if valp2p1 > 1.0 {
                        if valp1p2 > 1.0 {
                            consider(valp1p2, valp2p1);
                        }
                        let left = at(row + 3, col);
                        if left.max(at(row + 4, col - 1)).max(at(row + 4, col + 1)) < upper
                            && left > 1.0
                        {
                            consider(left, valp2p1);
                        }
                    }
                }
                col += 3;
            }
            max_ratio
        })
        .reduce(|| 1.0, f32::max);
    (1.0 / max_ratio.ln()).sqrt()
}

