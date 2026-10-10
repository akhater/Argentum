//! Sharpening: RawTherapee's capture sharpening and darktable's sharpen, with
//! RawTherapee's contrast mask deciding where either lands. Ours.
//!
//! WHAT IT REPLACES
//!
//! RapidRAW's Sharpness slider sizes its blur from the photo's short edge
//! (`min(w, h) / 1080`), so the radius grows with the sensor: about 2 px on a
//! 24 MP file, 3 px at 45 MP. Lens and sensor blur is under a pixel whatever
//! the megapixels, so it behaved like local contrast with halos. It also cut
//! the dark side of every edge to a tenth, gated out texture under about 2%
//! contrast, smoothed diagonal edges inside the sharpener, and read its fine
//! taps from the image before noise reduction. It stays in the shader, unused
//! unless an old edit still has a value, and its slider is hidden from Details
//! - see `src/argentum/Sharpening.tsx`.
//!
//! WHAT IT IS
//!
//! Three harvested pieces, run on the GPU before their shader - see
//! `shaders/sharpen.wgsl` for the maths and `sharpen_gpu.rs` for the passes:
//!
//! - **Capture sharpening**, RawTherapee `rtengine/capturesharpening.cc`
//!   @ c6d04960e14fed89e531cbbdf680021d131b4685. Richardson-Lucy deconvolution
//!   of the luminance with a gaussian PSF: it undoes blur rather than adding
//!   edge contrast. RAW only, as in RawTherapee. Its radius can be read from
//!   the RAW itself - `calc_radius_bayer` and `calc_radius_xtrans` below.
//! - **Sharpen**, darktable `src/iop/sharpen.c` @ 39df8424e67b99c870fc346ea8f5f241a4620575.
//!   The classic unsharp mask on L, with darktable's threshold, for output and
//!   creative sharpening.
//! - **The contrast mask**, RawTherapee `rtengine/rt_algo.cc`
//!   @ 1c2b3f33772eaf491b7bd4d66323ac36d13aa344. A sigmoid on local contrast,
//!   blurred, so flat areas - sky, skin, noise - are left alone. Its threshold
//!   can be found automatically from the flattest part of the photo -
//!   `auto_contrast` below. The mask view shows it.
//!
//! RADII ARE IN FULL-RESOLUTION PIXELS
//!
//! The user's radius means the same thing at every zoom: the GPU stage scales
//! it by how many render pixels there are per full-resolution pixel, as
//! darktable scales by `roi_in->scale` and RawTherapee divides by its preview
//! scale. So it is exact at 100%, and an approximation at fit-to-screen - where
//! capture sharpening's effect is genuinely below one screen pixel and is
//! skipped. Lightroom asks you to judge sharpening at 1:1 for the same reason.

use std::collections::HashMap;
use std::sync::Mutex;

use bytemuck::{Pod, Zeroable};
use image::DynamicImage;
use rawler::rawimage::{RawImage, RawImageData};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// RawTherapee's defaults, from `rtgui/tools/pdsharpening.cc`
/// @ c777f374b4db9ec5ecec3dcfea2e857382208ea9: contrast 10 (auto on),
/// radius 0.75 (auto on), corner boost 0, 20 iterations.
pub const DEFAULT_CONTRAST: f32 = 10.0;
pub const DEFAULT_RADIUS: f32 = 0.75;
pub const DEFAULT_ITERATIONS: u32 = 20;
/// darktable's defaults, from its `sharpen` preset: radius 2, amount 0.5,
/// threshold 0.5. The amount here defaults to off - capture sharpening is the
/// one that is on for every RAW.
pub const DEFAULT_USM_RADIUS: f32 = 2.0;
pub const DEFAULT_USM_THRESHOLD: f32 = 0.5;

/// RawTherapee's ceiling for an automatic radius (`maxSigma`).
const MAX_AUTO_RADIUS: f32 = 2.0;

pub const FLAG_AUTO_CONTRAST: u32 = 1;
pub const FLAG_ITER_CHECK: u32 = 2;
/// The manual sharpen has a mask of its own rather than capture's.
pub const FLAG_USM_OWN_MASK: u32 = 4;
pub const FLAG_USM_AUTO_CONTRAST: u32 = 8;

/// What one photo asks for, resolved, in the layout the GPU struct carries.
///
/// It rides inside RapidRAW's `GlobalAdjustments` because that is what every
/// render is handed - preview, export and the 16-bit export alike - and it is
/// mirrored field for field by `AgSharpen` in `modules.wgsl`, which keeps
/// their uniform layout lined up. Their shader never reads it; the stage in
/// `input_stage.rs` does.
///
/// 176 bytes, a multiple of 16, so it does not move anything after it.
#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Pod, Zeroable, Default, PartialEq)]
pub struct Params {
    /// 0 is off. 0..1 blends the deconvolved luminance in, on top of the mask.
    pub capture_amount: f32,
    /// Gaussian sigma of the PSF, full-resolution pixels. Already resolved
    /// from the RAW when the photo asked for auto.
    pub capture_radius: f32,
    /// RawTherapee's corner boost: sigma added at the corners, -0.5..0.5.
    pub capture_corner: f32,
    pub capture_iterations: u32,
    /// darktable's amount, 0..2. 0 is off.
    pub usm_amount: f32,
    /// darktable's radius: gaussian sigma, full-resolution pixels.
    pub usm_radius: f32,
    /// darktable's threshold, in L* units.
    pub usm_threshold: f32,
    /// RawTherapee's contrast threshold, 0..200. 0 sharpens everywhere.
    pub contrast: f32,
    pub flags: u32,
    /// The same, for the manual sharpen's own mask when it has one.
    pub usm_contrast: f32,
    pub _pad: [u32; 2],
    /// Each mask's own Sharpen, -1..1, indexed as their shader indexes masks.
    /// Positive sharpens with darktable's unsharp mask at Sharpen's radius and
    /// mask; negative softens towards that blur.
    pub local: [f32; MAX_LOCAL],
}

/// RapidRAW's MAX_MASKS.
pub const MAX_LOCAL: usize = 32;

impl Params {
    pub fn capture_on(&self) -> bool {
        self.capture_amount > 0.0 && self.capture_iterations > 0
    }

    pub fn usm_on(&self) -> bool {
        self.usm_amount > 0.0 && self.usm_radius > 0.0
    }

    pub fn auto_contrast(&self) -> bool {
        self.flags & FLAG_AUTO_CONTRAST != 0
    }

    pub fn usm_own_mask(&self) -> bool {
        self.flags & FLAG_USM_OWN_MASK != 0
    }

    pub fn usm_auto_contrast(&self) -> bool {
        self.flags & FLAG_USM_AUTO_CONTRAST != 0
    }

    pub fn local_on(&self) -> bool {
        self.local.iter().any(|a| *a != 0.0)
    }
}

fn number(section: &Value, key: &str, default: f32) -> f32 {
    section
        .get(key)
        .and_then(Value::as_f64)
        .map(|v| v as f32)
        .filter(|v| v.is_finite())
        .unwrap_or(default)
}

fn flag(section: &Value, key: &str, default: bool) -> bool {
    section.get(key).and_then(Value::as_bool).unwrap_or(default)
}

/// What a photo's adjustments ask for.
///
/// The settings live under `agSharpen` in the photo's adjustments, and only
/// once somebody has touched them: every key there is part of the thumbnail
/// cache hash, so writing defaults into every photo would rebuild the library
/// for nothing. Absent means the defaults below, which the frontend shows.
///
/// The card's own switch turns all of it off, and so does turning off
/// RapidRAW's Details section (their eye on the panel), as it does their own
/// sharpening.
pub fn from_json(js: &Value, is_raw: bool, photo: Option<&str>) -> Params {
    let details_visible = js
        .get("sectionVisibility")
        .and_then(|v| v.get("details"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let s = js.get("agSharpen").cloned().unwrap_or(Value::Null);
    if !details_visible || !flag(&s, "enabled", true) {
        return Params::default();
    }

    // RAW only, as in RawTherapee: a JPEG has already been sharpened by the
    // camera, and there is no RAW to read the radius from.
    let capture = is_raw && flag(&s, "capture", true);
    let capture_radius = if flag(&s, "autoRadius", true) {
        photo.and_then(auto_radius).unwrap_or(DEFAULT_RADIUS)
    } else {
        number(&s, "radius", DEFAULT_RADIUS).clamp(0.4, 2.0)
    };

    let mut flags = 0;
    if flag(&s, "autoContrast", true) {
        flags |= FLAG_AUTO_CONTRAST;
    }
    if flag(&s, "iterCheck", true) {
        flags |= FLAG_ITER_CHECK;
    }
    // The manual sharpen follows capture's mask unless it was given its own.
    // A JPEG has no capture sharpening, so its mask is always its own.
    if !is_raw || flag(&s, "usmOwnMask", false) {
        flags |= FLAG_USM_OWN_MASK;
        if flag(&s, "usmAutoContrast", true) {
            flags |= FLAG_USM_AUTO_CONTRAST;
        }
    }

    Params {
        capture_amount: if capture {
            (number(&s, "captureAmount", 100.0) / 100.0).clamp(0.0, 1.0)
        } else {
            0.0
        },
        capture_radius,
        capture_corner: number(&s, "cornerBoost", 0.0).clamp(-0.5, 0.5),
        capture_iterations: number(&s, "iterations", DEFAULT_ITERATIONS as f32).clamp(1.0, 100.0)
            as u32,
        usm_amount: (number(&s, "amount", 0.0) / 100.0).clamp(0.0, 2.0),
        usm_radius: number(&s, "usmRadius", DEFAULT_USM_RADIUS).clamp(0.1, 8.0),
        usm_threshold: number(&s, "threshold", DEFAULT_USM_THRESHOLD).clamp(0.0, 100.0),
        contrast: number(&s, "contrast", DEFAULT_CONTRAST).clamp(0.0, 200.0),
        flags,
        usm_contrast: number(&s, "usmContrast", DEFAULT_CONTRAST).clamp(0.0, 200.0),
        _pad: [0; 2],
        local: local_amounts(js),
    }
}

/// Each mask's Sharpen, in the order RapidRAW's shader indexes masks.
///
/// Their `get_all_adjustments_from_json` keeps the masks that are visible and
/// have at least one sub-mask, in order, up to MAX_MASKS, and the mask
/// bitmaps a render carries are built with the same rule. Read straight from
/// the JSON rather than through their MaskDefinition, which would mean cloning
/// every sub-mask's painted data for one number per mask.
fn local_amounts(js: &Value) -> [f32; MAX_LOCAL] {
    let mut out = [0.0; MAX_LOCAL];
    let Some(masks) = js.get("masks").and_then(Value::as_array) else {
        return out;
    };
    let kept = masks.iter().filter(|m| {
        m.get("visible").and_then(Value::as_bool).unwrap_or(false)
            && m.get("subMasks")
                .and_then(Value::as_array)
                .is_some_and(|s| !s.is_empty())
    });
    for (slot, mask) in out.iter_mut().zip(kept) {
        let adjustments = mask.get("adjustments").cloned().unwrap_or(Value::Null);
        let details_visible = adjustments
            .get("sectionVisibility")
            .and_then(|v| v.get("details"))
            .and_then(Value::as_bool)
            .unwrap_or(true);
        if details_visible {
            let own = adjustments.get("agSharpen").cloned().unwrap_or(Value::Null);
            *slot = (number(&own, "amount", 0.0) / 100.0).clamp(-1.0, 1.0);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Auto radius: read from the RAW, at decode.
// ---------------------------------------------------------------------------

/// The radius each decoded RAW's sharpest edge implies, per photo.
///
/// Keyed by path for the reason the camera matrix is (`profile_correction.rs`):
/// export runs several workers decoding different photos at once. Recorded at
/// decode because only the decode has the mosaic, and RawTherapee measures it
/// there - on green photosites, before demosaicing has blurred anything.
static AUTO_RADIUS: Mutex<Option<HashMap<String, f32>>> = Mutex::new(None);

/// More than a session's worth of photos. A forgotten one falls back to
/// RawTherapee's default radius until it is decoded again.
const AUTO_RADIUS_KEEP: usize = 4096;

/// Measure and remember the radius for one decoded RAW. Called from
/// `decode.rs`, before highlight recovery rewrites anything.
pub fn record_raw(raw: &RawImage, photo: Option<&str>) {
    let Some(path) = photo else { return };
    let Some(radius) = measure_radius(raw) else {
        return;
    };
    if let Ok(mut slot) = AUTO_RADIUS.lock() {
        let map = slot.get_or_insert_with(HashMap::new);
        if map.len() >= AUTO_RADIUS_KEEP && !map.contains_key(path) {
            map.clear();
        }
        map.insert(path.to_string(), radius);
    }
}

/// The measured radius for a photo, if it has been decoded.
pub fn auto_radius(photo: &str) -> Option<f32> {
    AUTO_RADIUS.lock().ok()?.as_ref()?.get(photo).copied()
}

/// RawTherapee's measurement for whichever mosaic this is.
fn measure_radius(raw: &RawImage) -> Option<f32> {
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
    Some(radius.min(MAX_AUTO_RADIUS))
}

/// The part of the sensor that holds the picture.
///
/// The decoded mosaic is the whole sensor, including the masked strips round
/// the edge that read black. RawTherapee's rawData is the picture alone, and
/// it matters: the step from a masked row to the first lit one is the
/// steepest "edge" on the sensor, a ratio in the thousands, and measuring
/// across it gave every R6 Mark III photo a radius of 0.35 px. The crop
/// rawler recommends if it has one, else the unmasked area.
#[derive(Clone, Copy, Debug)]
struct Area {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

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
    (1.0 / bayer_peak(data, w, h, lower, upper, fc).ratio.ln()).sqrt()
}

/// The pair calcRadiusBayer settles on, and where it is - kept so a radius
/// that looks wrong can be traced to the two photosites that produced it.
// Where and which values are read by the diagnostic test only.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
struct Peak {
    ratio: f32,
    row: usize,
    col: usize,
    high: f32,
    low: f32,
}

fn bayer_peak(data: &[f32], w: usize, h: usize, lower: f32, upper: f32, fc: [usize; 2]) -> Peak {
    let at = |r: usize, c: usize| data[r * w + c];
    let none = Peak {
        ratio: 1.0,
        row: 0,
        col: 0,
        high: 0.0,
        low: 0.0,
    };
    (4..h.saturating_sub(4))
        .into_par_iter()
        .map(|row| {
            let mut peak = none;
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
                        if max_val0 > peak.ratio * min_val {
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
                                peak = Peak {
                                    ratio: max_val0 / min_val,
                                    row,
                                    col,
                                    high: max_val0,
                                    low: min_val,
                                };
                            }
                        }
                    }
                    let max_val1 = val00.max(val1p1);
                    if val1p1 > 0.0 && max_val1 > lower {
                        let min_val = val00.min(val1p1);
                        if max_val1 > peak.ratio * min_val {
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
                                peak = Peak {
                                    ratio: max_val1 / min_val,
                                    row,
                                    col,
                                    high: max_val1,
                                    low: min_val,
                                };
                            }
                        }
                    }
                }
                col += 2;
            }
            peak
        })
        .reduce(|| none, |a, b| if b.ratio > a.ratio { b } else { a })
}

/// Where RawTherapee starts its X-Trans scan.
///
/// It looks in rows and columns 6..12 for a green whose left and right
/// neighbours differ, with no green above it and none to its left.
///
/// Taken from `getDeconvAutoRadius_capturesharpening_SE`, not from
/// `captureSharpening`. Both run the same search, but the loop's `++i` after
/// the inner `break` leaves `i` one row past the hit. The selective-editing
/// version takes it back (`i -= 7; j -= 6`); the main one passes `i` on as is,
/// so the two start their scans a row apart. On the standard layout only the
/// selective-editing one lands on a solitary green - the test below checks
/// that - so that is the one ported.
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

// ---------------------------------------------------------------------------
// Auto contrast: from the flattest part of the photo.
// ---------------------------------------------------------------------------

/// RawTherapee's L, which is L* scaled to 0..32768. Its constants below are in
/// these units, so the luminance is too.
fn rt_l(r: f32, g: f32, b: f32) -> f32 {
    let y = (0.212671 * r + 0.715160 * g + 0.072169 * b).max(0.0);
    let e = 216.0 / 24389.0;
    let k = 24389.0 / 27.0;
    let f = if y > e {
        y.cbrt()
    } else {
        (k * y + 16.0) / 116.0
    };
    (116.0 * f - 16.0) * 327.68
}

/// L for every pixel, read straight out of the float buffer the pipeline
/// already holds. A full-resolution `to_rgb32f` copy would be another 288 MB
/// at 24 MP for a measurement that needs one channel.
fn luminance_plane(image: &DynamicImage, is_raw: bool) -> (Vec<f32>, usize, usize) {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let lin = |v: f32| {
        if is_raw {
            v
        } else if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let from = |samples: &[f32], channels: usize| -> Vec<f32> {
        samples
            .par_chunks(channels)
            .map(|p| rt_l(lin(p[0]), lin(p[1]), lin(p[2])))
            .collect()
    };
    let l = match image {
        DynamicImage::ImageRgb32F(buf) => from(buf.as_raw(), 3),
        DynamicImage::ImageRgba32F(buf) => from(buf.as_raw(), 4),
        other => from(other.to_rgb32f().as_raw(), 3),
    };
    (l, w, h)
}

/// rt_ calcBlendFactor, RawTherapee rt_algo.cc.
fn blend_factor(val: f32, threshold: f32) -> f32 {
    let x = -16.0 + (16.0 / threshold) * val;
    0.5 * (1.0 + x / (1.0 + x * x).sqrt())
}

fn tile_average(l: &[f32], w: usize, y0: usize, x0: usize, size: usize) -> f32 {
    let mut sum = 0.0f32;
    for y in y0..y0 + size {
        sum += l[y * w + x0..y * w + x0 + size].iter().sum::<f32>();
    }
    sum / (size * size) as f32
}

fn tile_variance(l: &[f32], w: usize, y0: usize, x0: usize, size: usize, avg: f32) -> f32 {
    let mut var = 0.0f32;
    for y in y0..y0 + size {
        for &v in &l[y * w + x0..y * w + x0 + size] {
            var += (v - avg) * (v - avg);
        }
    }
    var / ((size * size) as f32 * avg)
}

/// rt_ calcContrastThreshold: the lowest threshold at which no more than 1%
/// of this flat tile would be sharpened.
fn calc_contrast_threshold(l: &[f32], w: usize, tile_y: usize, tile_x: usize, size: usize) -> f32 {
    const SCALE: f32 = 0.0625 / 327.68;
    let at = |y: usize, x: usize| l[y * w + x];
    let mut contrast = Vec::with_capacity((size - 4) * (size - 4));
    for j in tile_y + 2..tile_y + size - 2 {
        for i in tile_x + 2..tile_x + size - 2 {
            let c = ((at(j, i + 1) - at(j, i - 1)).powi(2)
                + (at(j + 1, i) - at(j - 1, i)).powi(2)
                + (at(j, i + 2) - at(j, i - 2)).powi(2)
                + (at(j + 2, i) - at(j - 2, i)).powi(2))
            .sqrt()
                * SCALE;
            contrast.push(c);
        }
    }
    let limit = ((size - 4) * (size - 4)) as f32 / 100.0;
    let mut c = 1;
    while c < 100 {
        let threshold = c as f32 / 100.0;
        let sum: f32 = contrast.iter().map(|&v| blend_factor(v, threshold)).sum();
        if sum <= limit {
            break;
        }
        c += 1;
    }
    (c + 1) as f32 / 100.0
}

/// rt_ the automatic half of buildBlendMask, RawTherapee rt_algo.cc
/// @ 1c2b3f33772eaf491b7bd4d66323ac36d13aa344.
///
/// Finds the flattest tile of mid-tone luminance - an 80 px pass, then a
/// finer 40 px pass round the best of it - and sets the threshold so that
/// tile is left almost entirely alone. Returns the threshold in 0..1, and 0
/// when nothing flat enough was found, which RawTherapee reads as "sharpen
/// everywhere".
fn auto_contrast_threshold(l: &[f32], w: usize, h: usize) -> f32 {
    const MIN_LUMINANCE: f32 = 2000.0;
    const MAX_LUMINANCE: f32 = 20000.0;
    const MIN_TILE_VARIANCE: f32 = 0.5;

    let variance_or_inf = |y: usize, x: usize, size: usize| {
        let avg = tile_average(l, w, y, x, size);
        if !(MIN_LUMINANCE..=MAX_LUMINANCE).contains(&avg) {
            return f32::INFINITY;
        }
        let v = tile_variance(l, w, y, x, size, avg);
        if v < MIN_TILE_VARIANCE {
            f32::INFINITY
        } else {
            v
        }
    };

    for pass in 0..2usize {
        let tilesize = 80 / (pass + 1);
        let skip = if pass == 0 { tilesize } else { tilesize / 4 };
        // RawTherapee's counts, which keep the last tile inside the image:
        // pass 0 steps a whole tile, pass 1 a quarter, less the three
        // quarter-steps a 40 px tile overhangs.
        let tiles_w = (w / skip).saturating_sub(3 * pass);
        let tiles_h = (h / skip).saturating_sub(3 * pass);
        if tiles_w == 0 || tiles_h == 0 {
            return 0.0;
        }
        let best = (0..tiles_h)
            .into_par_iter()
            .flat_map_iter(|i| (0..tiles_w).map(move |j| (i, j)))
            .map(|(i, j)| (variance_or_inf(i * skip, j * skip, tilesize), i, j))
            .reduce(
                || (f32::INFINITY, 0, 0),
                |a, b| if b.0 < a.0 { b } else { a },
            );
        let (minvar, min_i, min_j) = best;

        if minvar <= 1.0 || pass == 1 {
            let min_y = skip * min_i;
            let min_x = skip * min_j;
            if pass == 0 {
                return calc_contrast_threshold(l, w, min_y, min_x, tilesize);
            }
            // Second pass: every position within +-skip of the best tile.
            let y_start = min_y.saturating_sub(skip);
            let x_start = min_x.saturating_sub(skip);
            let y_end = (min_y + skip).min(h - tilesize);
            let x_end = (min_x + skip).min(w - tilesize);
            let mut best = (f32::INFINITY, 0, 0);
            for y in y_start..=y_end {
                for x in x_start..=x_end {
                    let v = variance_or_inf(y, x, tilesize);
                    if v < best.0 {
                        best = (v, y, x);
                    }
                }
            }
            return if best.0 <= 8.0 {
                calc_contrast_threshold(l, w, best.1, best.2, tilesize)
            } else {
                0.0
            };
        }
    }
    0.0
}

/// The last few automatic thresholds, by the image they were measured on.
///
/// A measurement is a full pass over the photo, so it is made once per photo
/// and crop and not on every frame. Keyed by what identifies the pixels to a
/// render - their transform hash and size - and the image buffer itself, so a
/// re-decode of the same photo at the same size is measured again.
type ContrastKey = (u64, u32, u32, usize);
static AUTO_CONTRAST: Mutex<Vec<(ContrastKey, f32)>> = Mutex::new(Vec::new());

/// The two masks' thresholds (0..1) for a render, and the measured one when
/// either asked for auto.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thresholds {
    pub capture: f32,
    pub usm: f32,
    pub measured: Option<f32>,
}

/// Resolve both masks' thresholds, measuring on `image` only if one of them
/// is on auto. `image` should be the full-resolution picture when there is
/// one: RawTherapee measures on the full RAW, and the flattest tile of a
/// downscaled preview is smoother than the real one.
pub fn thresholds(
    params: &Params,
    image: &DynamicImage,
    transform_hash: u64,
    is_raw: bool,
) -> Thresholds {
    let wants_auto =
        params.auto_contrast() || (params.usm_own_mask() && params.usm_auto_contrast());
    let measured = wants_auto.then(|| measured_threshold(image, transform_hash, is_raw));
    let capture = match measured {
        Some(m) if params.auto_contrast() => m,
        _ => params.contrast / 100.0,
    };
    let usm = if !params.usm_own_mask() {
        capture
    } else {
        match measured {
            Some(m) if params.usm_auto_contrast() => m,
            _ => params.usm_contrast / 100.0,
        }
    };
    Thresholds {
        capture,
        usm,
        measured,
    }
}

fn measured_threshold(image: &DynamicImage, transform_hash: u64, is_raw: bool) -> f32 {
    let key = (
        transform_hash,
        image.width(),
        image.height(),
        image.as_bytes().as_ptr() as usize,
    );
    if let Ok(known) = AUTO_CONTRAST.lock()
        && let Some((_, value)) = known.iter().find(|(k, _)| *k == key)
    {
        return *value;
    }
    let (l, w, h) = luminance_plane(image, is_raw);
    let value = auto_contrast_threshold(&l, w, h);
    if let Ok(mut known) = AUTO_CONTRAST.lock() {
        known.retain(|(k, _)| *k != key);
        known.push((key, value));
        if known.len() > 8 {
            known.remove(0);
        }
    }
    value
}

/// What auto found for each photo the editor has shown, so the slider can
/// say it - as RawTherapee writes its automatic values back into its
/// sliders. Recorded by the editor's render only; an export measuring a
/// photo nobody is looking at has no slider to tell.
static SHOWN_CONTRAST: Mutex<Option<HashMap<String, f32>>> = Mutex::new(None);

pub fn remember_shown_contrast(photo: &str, threshold: f32) {
    if let Ok(mut slot) = SHOWN_CONTRAST.lock() {
        let map = slot.get_or_insert_with(HashMap::new);
        if map.len() >= AUTO_RADIUS_KEEP && !map.contains_key(photo) {
            map.clear();
        }
        map.insert(photo.to_string(), threshold * 100.0);
    }
}

/// The automatic values in effect for a photo: the radius its RAW gave, and
/// the contrast threshold its last measurement found. Either is null until
/// there is one - the radius until the photo is decoded, the contrast until
/// a render at 100% (or the mask view) has needed it.
pub fn auto_values(photo: &str) -> serde_json::Value {
    let contrast = SHOWN_CONTRAST
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref()?.get(photo).copied());
    serde_json::json!({
        "radius": auto_radius(photo),
        "contrast": contrast,
    })
}

// ---------------------------------------------------------------------------
// Retiring RapidRAW's base pre-sharpening.
// ---------------------------------------------------------------------------

/// Set RapidRAW's "Base Pre-Sharpening" to 0, once.
///
/// It sharpens every RAW as it loads, with a 5x5 box blur, at 0.35 unless
/// changed - and capture sharpening, which is on for every RAW, would stack on
/// top of it. AK's decision, 2026-10-10: zero it once, on the first start of
/// the version that brings capture sharpening, and leave it a user's setting
/// from then on. The marker is ours, so a user who turns it back up keeps it.
pub fn retire_presharpening_once(app: &tauri::AppHandle, library: &std::path::Path) {
    const DONE: &str = "preSharpeningRetired";
    if super::ag_settings::get(library, DONE).is_some() {
        return;
    }
    let Ok(mut settings) = crate::app_settings::load_settings(app.clone()) else {
        return;
    };
    settings.raw_preprocessing_sharpening = Some(0.0);
    if crate::app_settings::save_settings(settings, app.clone()).is_ok() {
        let _ = super::ag_settings::set(library, DONE, Value::Bool(true));
        log::info!("Base pre-sharpening set to 0: capture sharpening replaces it");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_gpu_struct_is_176_bytes() {
        // modules.wgsl's AgSharpen mirrors this; a size change shifts every
        // field of their adjustments that comes after it.
        assert_eq!(std::mem::size_of::<Params>(), 176);
    }

    #[test]
    fn a_raw_with_no_settings_gets_capture_sharpening_only() {
        let p = from_json(&json!({}), true, None);
        assert!(p.capture_on());
        assert!(!p.usm_on());
        assert_eq!(p.capture_radius, DEFAULT_RADIUS);
        assert_eq!(p.capture_iterations, DEFAULT_ITERATIONS);
        assert!(p.auto_contrast());
        assert_eq!(p.flags & FLAG_ITER_CHECK, FLAG_ITER_CHECK);
    }

    #[test]
    fn a_jpeg_is_never_capture_sharpened() {
        let p = from_json(&json!({ "agSharpen": { "capture": true } }), false, None);
        assert!(!p.capture_on());
    }

    #[test]
    fn hiding_the_details_section_turns_it_all_off() {
        let p = from_json(
            &json!({ "sectionVisibility": { "details": false }, "agSharpen": { "amount": 80 } }),
            true,
            None,
        );
        assert_eq!(p, Params::default());
    }

    #[test]
    fn the_switch_turns_everything_off() {
        let p = from_json(
            &json!({ "agSharpen": { "enabled": false, "amount": 80 } }),
            true,
            None,
        );
        assert_eq!(p, Params::default());
    }

    #[test]
    fn the_manual_sharpen_follows_captures_mask_until_given_its_own() {
        let linked = from_json(&json!({}), true, None);
        assert!(!linked.usm_own_mask());
        let own = from_json(
            &json!({ "agSharpen": { "usmOwnMask": true, "usmAutoContrast": false, "usmContrast": 40 } }),
            true,
            None,
        );
        assert!(own.usm_own_mask() && !own.usm_auto_contrast());
        assert_eq!(own.usm_contrast, 40.0);
        // A JPEG has no capture mask to follow.
        assert!(from_json(&json!({}), false, None).usm_own_mask());
    }

    #[test]
    fn each_masks_sharpen_lands_where_their_shader_indexes_it() {
        let mask = |visible: bool, subs: usize, amount: f64| {
            json!({
                "visible": visible,
                "subMasks": (0..subs).map(|i| json!({ "id": i })).collect::<Vec<_>>(),
                "adjustments": { "agSharpen": { "amount": amount } },
            })
        };
        let js = json!({ "masks": [
            mask(true, 1, 50.0),
            mask(false, 1, 80.0),   // hidden: not indexed
            mask(true, 0, 90.0),    // no sub-masks: not indexed
            mask(true, 2, -40.0),
            mask(true, 1, 500.0),   // clamped
        ] });
        let p = from_json(&js, true, None);
        assert_eq!(&p.local[..4], &[0.5, -0.4, 1.0, 0.0]);
        assert!(p.local_on());
        // The card's switch turns the local ones off with the rest.
        let off = json!({ "agSharpen": { "enabled": false }, "masks": js["masks"].clone() });
        assert!(!from_json(&off, true, None).local_on());
    }

    #[test]
    fn manual_values_are_read_and_clamped() {
        let p = from_json(
            &json!({ "agSharpen": {
                "autoRadius": false, "radius": 9.0, "cornerBoost": -2.0,
                "iterations": 500, "amount": 150, "usmRadius": 1.5,
                "threshold": 2.0, "autoContrast": false, "contrast": 35
            } }),
            true,
            None,
        );
        assert_eq!(p.capture_radius, 2.0);
        assert_eq!(p.capture_corner, -0.5);
        assert_eq!(p.capture_iterations, 100);
        assert_eq!(p.usm_amount, 1.5);
        assert_eq!(p.usm_radius, 1.5);
        assert_eq!(p.usm_threshold, 2.0);
        assert!(!p.auto_contrast());
        assert_eq!(p.contrast, 35.0);
    }

    #[test]
    fn a_recorded_radius_is_used_when_auto() {
        let path = "test://sharpen/recorded.cr2";
        AUTO_RADIUS
            .lock()
            .unwrap()
            .get_or_insert_with(HashMap::new)
            .insert(path.into(), 1.1);
        let p = from_json(&json!({}), true, Some(path));
        assert_eq!(p.capture_radius, 1.1);
        let manual = from_json(
            &json!({ "agSharpen": { "autoRadius": false, "radius": 0.6 } }),
            true,
            Some(path),
        );
        assert_eq!(manual.capture_radius, 0.6);
    }

    /// A hard edge blurred by a gaussian, laid out as an RGGB mosaic - every
    /// photosite gets the same value, which is all the green-to-green
    /// measurement reads.
    fn blurred_edge(sigma: f32) -> (Vec<f32>, usize, usize) {
        let (w, h) = (64usize, 64usize);
        let edge = move |x: f32| {
            let t = (x - 32.0) / (sigma * std::f32::consts::SQRT_2);
            let a = 0.147;
            let erf = (1.0
                - (-t * t * (4.0 / std::f32::consts::PI + a * t * t) / (1.0 + a * t * t)).exp())
            .sqrt()
                * t.signum();
            2000.0 + (30000.0 - 2000.0) * 0.5 * (1.0 + erf)
        };
        let data = (0..h)
            .flat_map(|_| (0..w).map(move |x| edge(x as f32)))
            .collect();
        (data, w, h)
    }

    /// A softer edge has to read as a wider radius. RGGB: the colour at
    /// column 0 is red on even rows and green on odd ones.
    #[test]
    fn the_bayer_radius_grows_with_the_blur() {
        let measure = |sigma| {
            let (data, w, h) = blurred_edge(sigma);
            calc_radius_bayer(&data, w, h, 1000.0, 65535.0, [0, 1])
        };
        let (sharp, soft) = (measure(0.6), measure(1.4));
        assert!(sharp.is_finite() && soft.is_finite());
        assert!(
            sharp < soft,
            "a 0.6 blur measured {sharp}, a 1.4 blur {soft}"
        );
    }

    #[test]
    fn a_clipped_edge_is_not_measured() {
        // Everything at the ceiling: no ratio survives the clip checks, and
        // RawTherapee's formula then gives infinity, capped at 2 by the caller.
        let data = vec![65535.0f32; 64 * 64];
        let r = calc_radius_bayer(&data, 64, 64, 1000.0, 65535.0, [0, 1]);
        assert!(r.is_infinite());
    }

    /// What auto radius a real RAW gives, and which two photosites decided it.
    /// `AG_SHARPEN_RAWS` holds paths separated by `;`.
    #[test]
    #[ignore = "reads AK's photos; run by hand"]
    fn auto_radius_on_real_raws() {
        let paths = std::env::var("AG_SHARPEN_RAWS").expect("set AG_SHARPEN_RAWS");
        for path in paths.split(';').filter(|p| !p.is_empty()) {
            let bytes = std::fs::read(path).expect("read");
            let source = rawler::rawsource::RawSource::new_from_slice(&bytes);
            let decoder = rawler::get_decoder(&source).expect("decoder");
            let raw = decoder
                .raw_image(
                    &source,
                    &rawler::decoders::RawDecodeParams::default(),
                    false,
                )
                .expect("raw");
            let black = raw
                .blacklevel
                .levels
                .first()
                .map(|r| r.as_f32())
                .unwrap_or(0.0);
            let white = *raw.whitelevel.0.first().unwrap() as f32;
            let area = picture_area(&raw).expect("a picture area");
            let data = scaled_area(&raw, &area, black, white).expect("its pixels");
            let cfa = &raw.camera.cfa;
            let fc = [
                cfa.color_at(area.y, area.x),
                cfa.color_at(area.y + 1, area.x),
            ];
            let peak = bayer_peak(&data, area.w, area.h, 1000.0, 65535.0, fc);
            let at = |r: usize, c: usize| data[r * area.w + c];
            let around: Vec<String> = (peak.row.saturating_sub(2)..=(peak.row + 3).min(area.h - 1))
                .map(|r| {
                    (peak.col.saturating_sub(3)..=(peak.col + 3).min(area.w - 1))
                        .map(|c| format!("{:6.0}", at(r, c)))
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect();
            eprintln!("{path}");
            eprintln!(
                "  {}x{}, picture {area:?}, black {black} white {white}, {}",
                raw.width, raw.height, cfa.name
            );
            eprintln!(
                "  measure_radius: {:?}; peak ratio {:.1}: {:.0} vs {:.1} at row {} col {}",
                measure_radius(&raw),
                peak.ratio,
                peak.high,
                peak.low,
                peak.row,
                peak.col
            );
            for line in around {
                eprintln!("    {line}");
            }
        }
    }

    #[test]
    fn a_flat_photo_finds_a_threshold_and_a_busy_one_does_not_need_one() {
        // Flat mid-grey with mild noise: there is a flat tile, so a threshold.
        let (w, h) = (400usize, 300usize);
        let mut seed = 1u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
        };
        let flat: Vec<f32> = (0..w * h).map(|_| 12000.0 + noise() * 400.0).collect();
        let t = auto_contrast_threshold(&flat, w, h);
        assert!(t > 0.0 && t < 1.0, "threshold {t}");

        // All black: nothing in the mid-tones, so RawTherapee's "everywhere".
        let black = vec![0.0f32; w * h];
        assert_eq!(auto_contrast_threshold(&black, w, h), 0.0);
    }

    #[test]
    fn the_blend_factor_is_one_half_at_the_threshold() {
        assert!((blend_factor(0.1, 0.1) - 0.5).abs() < 1e-6);
        assert!(blend_factor(0.0, 0.1) < 0.01);
        assert!(blend_factor(0.3, 0.1) > 0.99);
    }

    #[test]
    fn xtrans_start_finds_a_green_in_phase() {
        // The standard X-Trans layout, in one of its phases.
        const P: [[usize; 6]; 6] = [
            [1, 1, 0, 1, 1, 2],
            [1, 1, 2, 1, 1, 0],
            [2, 0, 1, 0, 2, 1],
            [1, 1, 2, 1, 1, 0],
            [1, 1, 0, 1, 1, 2],
            [0, 2, 1, 2, 0, 1],
        ];
        let color = |r: usize, c: usize| P[r % 6][c % 6];
        let (sy, sx) = xtrans_start(&color).expect("a start");
        // calcRadiusXtrans reads its solitary green at (start + 2, start + 2);
        // it has to be a green with no green beside it.
        let (gy, gx) = (sy + 2 + 6, sx + 2 + 6);
        assert_eq!(color(gy, gx), 1, "the scan must start on a green");
        assert!(
            color(gy, gx - 1) != 1 && color(gy, gx + 1) != 1,
            "and a solitary one"
        );
    }
}
