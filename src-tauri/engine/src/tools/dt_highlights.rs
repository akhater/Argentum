//! Highlight recovery: putting colour back where a channel clipped, on the
//! sensor data before demosaic.
//!
//! Copied from `src-tauri/src/mods/highlights.rs` @ 2bcb953d - `recover`, which
//! rebuilds a clipped channel from the ones that survived, and `settle_blown`,
//! which makes what no channel survived come out white rather than magenta -
//! with the helpers they call. The method is darktable's "opposed"
//! reconstruction (`src/iop/hlreconstruct/opposed.c`): learn the colour of the
//! highlights from the bright pixels that did not clip. The code is
//! Argentum's, written after it, and learns that colour once per photo rather
//! than per clipped region.
//!
//! What changed in the copy: the switch is this tool's setting rather than a
//! global preference and an environment variable, and the white balance
//! `settle_blown` makes neutral is the one in the decoded file. The app also
//! applies RapidRAW's multi-exposure exception to it, which is not in the
//! engine; for every other photo the two are the same.
//!
//! A photo with nothing clipped comes out bit for bit as it went in, and the
//! tool says so by returning nothing to do.

use std::sync::Arc;

use rawler::rawimage::{RawImage, RawImageData};
use serde::{Deserialize, Serialize};

use crate::{Context, Error, Field, Image, Result, Section, Stage, Tool};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Rebuild clipped channels from the ones that survived. Off is plain
    /// clipping: what clipped is still made white, never magenta.
    pub recover: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { recover: true }
    }
}

impl crate::Settings for Settings {
    const VERSION: u32 = 1;
    const FIELDS: &'static [Field] = &[Field::new("recover")];
}

#[derive(Default)]
pub struct DtHighlights;

impl Tool for DtHighlights {
    const ID: &'static str = "dt_highlights";
    const STAGE: Stage = Stage::Raw;
    const SECTION: Section = Section::Tone;
    type Settings = Settings;

    fn process(&self, settings: &Settings, input: &Image, _cx: &Context) -> Result<Option<Image>> {
        let raw = input.as_raw().ok_or_else(|| Error::Tool {
            tool: Self::ID,
            message: format!("was handed {}", input.describe()),
        })?;
        let mut out = raw.clone();
        Ok(apply(&mut out, settings.recover).then(|| Image::Raw(Arc::new(out))))
    }
}

/// Recovery, then the blown highlights settled, as the app's decode does it.
/// Says whether any value changed.
pub fn apply(raw: &mut RawImage, recover_on: bool) -> bool {
    let before_integer = matches!(raw.data, RawImageData::Integer(_));
    let before_float = match &raw.data {
        RawImageData::Float(v) => Some(v.clone()),
        RawImageData::Integer(_) => None,
    };
    let rebuilt = recover_on && recover(raw);
    let wb = raw.wb_coeffs;
    settle_blown(raw, wb, rebuilt);
    // Both only ever replace an integer buffer with a new float one when they
    // move something, so an integer buffer still integer is untouched.
    match (&raw.data, before_float) {
        (RawImageData::Integer(_), _) => !before_integer,
        (RawImageData::Float(after), Some(before)) => *after != before,
        (RawImageData::Float(_), None) => true,
    }
}

// ============================================================================
// Below: copied from src-tauri/src/mods/highlights.rs @ 2bcb953d.
// ============================================================================

/// How close to the ceiling still counts as clipped, as a fraction of the
/// range above black.
///
/// Not 100%: sensors do not saturate at exactly the tabulated white level.
/// Noise, per-channel variation and the manufacturer's own rounding mean the
/// real ceiling sits slightly below, and a pixel one count short of the table
/// is as dead as one that reached it. darktable uses a comparable margin for
/// the same reason.
const CLIP_MARGIN: f32 = 0.99;

// Copied from src-tauri/src/mods/highlights.rs @ 2bcb953d.
/// The value at which each of the four CFA colours stops being data.
///
/// `whitelevel` may carry one entry for all channels or one per channel; both
/// are normal, and a missing table means we cannot say anything useful, so the
/// caller gets `None` rather than a guess.
pub fn ceilings(raw: &RawImage) -> Option<[f32; 4]> {
    let levels = &raw.whitelevel.0;
    let first = *levels.first()? as f32;
    let mut out = [first; 4];
    for (i, slot) in out.iter_mut().enumerate() {
        if let Some(level) = levels.get(i) {
            *slot = *level as f32;
        }
    }

    let black = raw
        .blacklevel
        .levels
        .first()
        .map(|r| r.as_f32())
        .unwrap_or(0.0);

    for slot in out.iter_mut() {
        // The margin applies to the range above black, which is the part that
        // is signal. Applying it to the raw count would move the threshold by
        // a different amount on every camera.
        *slot = black + (*slot - black) * CLIP_MARGIN;
    }
    Some(out)
}

/// How bright an unclipped pixel has to be before it is allowed to say what
/// colour the highlights of this photo are, as a fraction of the range to the
/// ceiling.
///
/// The ratio between channels is not constant across a photo - shadows are
/// bluer, skin is redder - and the only ratio that matters here is the one
/// just below the clipping point, because that is what the clipped pixels
/// *were*. Too low and the estimate is contaminated by the whole scene; too
/// high and there are not enough pixels left to average.
const BRIGHT_ENOUGH: f32 = 0.5;

/// Fewest bright unclipped samples that will be trusted.
///
/// A ratio measured from a handful of pixels is noise, and reconstructing from
/// noise is worse than leaving the highlight alone.
const ENOUGH_SAMPLES: u64 = 500;

// Copied from src-tauri/src/mods/highlights.rs @ 2bcb953d.
/// The colour of this photo's highlights, as each channel's share relative to
/// green.
///
/// Measured rather than assumed from the camera's white balance: a highlight
/// is often not the colour the camera was balanced for - a sunset, a tungsten
/// lamp, a white dress under leaves. Every unclipped pixel bright enough to be
/// near the ceiling is averaged: whatever the light was, the pixels that
/// nearly clipped were that colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HighlightColour {
    /// Each channel's value relative to green, at the top of the range.
    pub ratio: [f32; 3],
}

impl HighlightColour {
    /// The level a pixel is at, judged by one channel that can be trusted.
    fn level_from(&self, channel: usize, value: f32) -> f32 {
        let r = self.ratio[channel];
        if r > 1e-6 { value / r } else { 0.0 }
    }
}

// Copied from src-tauri/src/mods/highlights.rs @ 2bcb953d.
/// Read the highlight colour out of one photo's own bright pixels.
///
/// `None` when there is nothing bright enough to learn from, which is the
/// correct answer for an underexposed frame: with no evidence, do nothing.
pub fn highlight_colour(
    triples: impl Iterator<Item = [f32; 3]>,
    ceilings: [f32; 3],
) -> Option<HighlightColour> {
    let mut sums = [0.0f64; 3];
    let mut n: u64 = 0;

    for p in triples {
        // Every channel has to be real data, or the average is pulled towards
        // whatever the ceilings happen to be.
        if (0..3).any(|c| p[c] >= ceilings[c]) {
            continue;
        }
        if (0..3).all(|c| p[c] < ceilings[c] * BRIGHT_ENOUGH) {
            continue;
        }
        for c in 0..3 {
            sums[c] += p[c] as f64;
        }
        n += 1;
    }

    if n < ENOUGH_SAMPLES || sums[1] <= 0.0 {
        return None;
    }

    let green = sums[1];
    Some(HighlightColour {
        ratio: [(sums[0] / green) as f32, 1.0, (sums[2] / green) as f32],
    })
}

// Copied from src-tauri/src/mods/highlights.rs @ 2bcb953d.
/// Rebuild one pixel's clipped channels. Returns it unchanged when there is
/// nothing to do.
///
/// A reconstructed channel is only ever raised. Lowering one would invent
/// darkness where the sensor reported light, and the sensor is not wrong about
/// that - it is only incapable of saying how much.
pub fn rebuild(pixel: [f32; 3], ceilings: [f32; 3], colour: &HighlightColour) -> [f32; 3] {
    let clipped = [
        pixel[0] >= ceilings[0],
        pixel[1] >= ceilings[1],
        pixel[2] >= ceilings[2],
    ];
    if !clipped.iter().any(|&c| c) || clipped.iter().all(|&c| c) {
        // Nothing gone, or everything gone. The second has no evidence left to
        // reconstruct from, and guessing there is invention.
        return pixel;
    }

    // Each surviving channel gives its own reading of how bright the pixel is.
    // They are averaged rather than maximised: the maximum of several noisy
    // readings is biased upwards, and pushes barely-clipped pixels past where
    // they belong.
    let mut level = 0.0f32;
    let mut readings = 0u32;
    for c in 0..3 {
        if !clipped[c] {
            level += colour.level_from(c, pixel[c]);
            readings += 1;
        }
    }
    if readings == 0 {
        return pixel;
    }
    // But the clipped channels are not silent: each says the pixel was at
    // least bright enough to reach it. That is a floor the estimate must not
    // fall through, or green is rebuilt below what a clipped red proves and
    // the reconstruction itself is magenta.
    let floor = (0..3)
        .filter(|&c| clipped[c])
        .map(|c| colour.level_from(c, pixel[c]))
        .fold(0.0f32, f32::max);
    let level = (level / readings as f32).max(floor);
    if level <= 0.0 {
        return pixel;
    }

    let mut out = pixel;
    for c in 0..3 {
        if clipped[c] {
            out[c] = pixel[c].max(level * colour.ratio[c]);
        }
    }
    out
}

/// Every Nth block is enough to learn the highlight colour: a ratio converges
/// long before twenty million samples.
const SAMPLE_STRIDE: usize = 4;

// Copied from src-tauri/src/mods/highlights.rs @ 2bcb953d, without its two
// switches: the global preference (`enabled()`) is this tool's `recover`
// setting, and the `AG_NO_RECOVERY` environment variable is not read.
/// Put back what the sensor could not record, in place, before demosaic.
///
/// Silent about everything it cannot do - an unreadable level table, a format
/// that is not a mosaic or a triple, a frame too dark to learn a colour from.
///
/// Returns whether recovery was in charge of this photo's clipped pixels: with
/// a measured highlight colour to rebuild them from. `settle_blown` needs to
/// know, because when it was not, nothing has dealt with them yet.
pub fn recover(raw: &mut RawImage) -> bool {
    let Some(ceil4) = ceilings(raw) else {
        return false;
    };
    let ceilings = [ceil4[0], ceil4[1], ceil4[2]];

    match raw.cpp {
        3 => recover_three_colour(raw, ceilings),
        1 => recover_mosaic(raw, ceilings),
        _ => false,
    }
}

// Copied from src-tauri/src/mods/highlights.rs @ 2bcb953d.
/// Canon sRAW and linear DNGs: three colours per pixel.
///
/// Reconstruction puts values *above* the sensor's ceiling, and the decoded
/// buffer is unsigned 16-bit, so where it happens the buffer is handed on as
/// float; where it does not, the original integers are left exactly as they
/// were.
fn recover_three_colour(raw: &mut RawImage, ceilings: [f32; 3]) -> bool {
    let RawImageData::Integer(pixels) = &raw.data else {
        return false;
    };

    let colour = highlight_colour(
        pixels
            .as_chunks::<3>()
            .0
            .iter()
            .step_by(SAMPLE_STRIDE)
            .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]),
        ceilings,
    );
    let Some(colour) = colour else { return false };

    let mut out: Vec<f32> = Vec::with_capacity(pixels.len());
    let mut touched = false;

    for p in pixels.as_chunks::<3>().0 {
        let before = [p[0] as f32, p[1] as f32, p[2] as f32];
        if (0..3).any(|c| before[c] >= ceilings[c]) {
            let after = rebuild(before, ceilings, &colour);
            touched |= after != before;
            out.extend_from_slice(&after);
        } else {
            out.extend_from_slice(&before);
        }
    }

    // Nothing was rebuilt, so leave the integers alone rather than convert a
    // buffer for no reason.
    if touched {
        raw.data = RawImageData::Float(out);
    }
    true
}

// Copied from src-tauri/src/mods/highlights.rs @ 2bcb953d.
/// A Bayer mosaic, one 2x2 block at a time.
///
/// A photosite holds one colour, so the block that will become a pixel is the
/// unit: read the three colours out of it, rebuild the ones that clipped, and
/// write each rebuilt value back only to the photosites it came from.
fn recover_mosaic(raw: &mut RawImage, ceilings: [f32; 3]) -> bool {
    let (w, h) = (raw.width, raw.height);
    if w < 2 || h < 2 {
        return false;
    }
    // Copied out because reading the pattern borrows `raw` while writing pixels
    // needs it mutably, and the pattern is four numbers.
    let pattern: Vec<usize> = (0..4)
        .map(|i| raw.camera.cfa.color_at(i / 2, i % 2).min(2))
        .collect();

    let RawImageData::Integer(pixels) = &raw.data else {
        return false;
    };

    let read_block = |src: &[f32], row: usize, col: usize| -> [f32; 3] {
        let mut out = [0.0f32; 3];
        for (i, (dr, dc)) in [(0, 0), (0, 1), (1, 0), (1, 1)].into_iter().enumerate() {
            let v = src[(row + dr) * w + (col + dc)];
            // Two greens: the brighter decides, because a block has lost green
            // as soon as either of them is at the ceiling.
            out[pattern[i]] = out[pattern[i]].max(v);
        }
        out
    };

    let mut out: Vec<f32> = pixels.iter().map(|&v| v as f32).collect();

    let colour = {
        let mut samples = Vec::new();
        let mut row = 0;
        while row + 1 < h {
            let mut col = 0;
            while col + 1 < w {
                samples.push(read_block(&out, row, col));
                col += 2 * SAMPLE_STRIDE;
            }
            row += 2 * SAMPLE_STRIDE;
        }
        highlight_colour(samples.into_iter(), ceilings)
    };
    let Some(colour) = colour else { return false };

    let mut touched = false;
    let mut row = 0;
    while row + 1 < h {
        let mut col = 0;
        while col + 1 < w {
            let before = read_block(&out, row, col);
            if (0..3).any(|c| before[c] >= ceilings[c]) {
                let after = rebuild(before, ceilings, &colour);
                if after != before {
                    for (i, (dr, dc)) in [(0, 0), (0, 1), (1, 0), (1, 1)].into_iter().enumerate() {
                        let c = pattern[i];
                        // Only the channels that were actually gone get replaced.
                        if before[c] >= ceilings[c] {
                            out[(row + dr) * w + (col + dc)] = after[c];
                            touched = true;
                        }
                    }
                }
            }
            col += 2;
        }
        row += 2;
    }

    if touched {
        raw.data = RawImageData::Float(out);
    }
    true
}

// Copied from src-tauri/src/mods/highlights.rs @ 2bcb953d.
/// Make sure what the sensor lost entirely comes out white, not magenta.
///
/// Where every channel clipped there is nothing left to rebuild from, and
/// leaving those photosites alone is not neutral: they all sit at their
/// ceiling, white balance multiplies red and blue by two or more, and the
/// camera matrix turns that into flat magenta.
///
/// `rebuilt` says whether `recover` was in charge of this photo.
///
/// - It was: a block with every channel gone is set neutral at the brightest
///   level any of its photosites recorded, and blocks recovery rebuilt fade
///   from their reconstructed colour, just over the clipping point, to
///   neutral half a stop above it.
/// - It was not: plain clipping. In each block with a clipped channel, every
///   photosite is capped where the first channel stops being data.
///
/// Neutral means neutral *after white balance*: `wb` has to be the
/// coefficients rawler will develop with. A block with nothing clipped is
/// never touched, and a photo with nothing clipped keeps its integer buffer
/// exactly as decoded.
pub fn settle_blown(raw: &mut RawImage, wb: [f32; 4], rebuilt: bool) {
    let Some(ceil4) = ceilings(raw) else { return };
    let black = raw
        .blacklevel
        .levels
        .first()
        .map(|r| r.as_f32())
        .unwrap_or(0.0);
    let Some(settle) = Settle::new([ceil4[0], ceil4[1], ceil4[2]], black, wb) else {
        return;
    };

    match raw.cpp {
        3 => {
            let (w, h) = (raw.width, raw.height);
            settle_buffer(raw, w * 3, h, 1, |_, col| col % 3, &settle, rebuilt);
        }
        1 => {
            let cfa = raw.camera.cfa.clone();
            // The block has to hold every colour, or "every channel clipped"
            // cannot be judged inside it. A Bayer 2x2 does; an X-Trans 6x6
            // does in each of its 3x3 quarters. Anything else is left as it is.
            let block = match (cfa.width, cfa.height) {
                (2, 2) if cfa.is_rgb() => 2,
                (6, 6) if cfa.is_rgb() => 3,
                _ => return,
            };
            let (w, h) = (raw.width, raw.height);
            settle_buffer(
                raw,
                w,
                h,
                block,
                |r, c| cfa.color_at(r, c),
                &settle,
                rebuilt,
            );
        }
        _ => {}
    }
}

/// The numbers `settle_blown` needs, worked out once per photo.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Settle {
    black: f32,
    /// Where each colour stops being data, from `ceilings`.
    ceiling: [f32; 3],
    /// White balance as rawler will apply it.
    wb: [f32; 3],
    /// Where each colour is capped when nothing was rebuilt: the raw value at
    /// which it reaches, after white balance, the level of whichever colour
    /// clips first. Never above that colour's own ceiling.
    cap: [f32; 3],
    /// After white balance: where the first colour stops being data, and where
    /// a rebuilt block has finished fading to neutral, half a stop above it.
    first_to_clip: f32,
    fade_end: f32,
}

/// How far above the first clip a rebuilt block keeps any of its colour, as a
/// factor: half a stop. Measured: fades ending at 1.1x, 1.25x and 1.5x all
/// removed a lavender band beside a blown core, and none brought an outline
/// back.
const FADE_SPAN: f32 = std::f32::consts::SQRT_2;

impl Settle {
    // Copied from src-tauri/src/mods/highlights.rs @ 2bcb953d.
    fn new(ceiling: [f32; 3], black: f32, wb: [f32; 4]) -> Option<Self> {
        // rawler's own rule: no coefficients means no white balance at all.
        let wb = if wb[0].is_nan() {
            [1.0, 1.0, 1.0]
        } else {
            [wb[0], wb[1], wb[2]]
        };
        // A zero or broken coefficient would put a division by it below.
        if wb.iter().any(|c| !c.is_finite() || *c <= 0.0) {
            return None;
        }
        if ceiling.iter().any(|c| *c <= black) {
            return None;
        }

        let balanced = [0, 1, 2].map(|c| (ceiling[c] - black) * wb[c]);
        let first_to_clip = balanced.iter().copied().fold(f32::INFINITY, f32::min);
        let cap = [0, 1, 2].map(|c| black + first_to_clip / wb[c]);
        Some(Self {
            black,
            ceiling,
            wb,
            cap,
            first_to_clip,
            fade_end: first_to_clip * FADE_SPAN,
        })
    }

    // Copied from src-tauri/src/mods/highlights.rs @ 2bcb953d.
    /// One block's photosites, with the colour each one holds. Returns how many
    /// values moved.
    fn block(&self, values: &mut [f32], colours: &[usize], rebuilt: bool) -> usize {
        let mut present = [false; 3];
        let mut clipped = [false; 3];
        let mut brightest = 0.0f32;

        for (&v, &c) in values.iter().zip(colours) {
            if c > 2 {
                return 0;
            }
            present[c] = true;
            // A colour is gone as soon as any of its photosites is, the same
            // judgement `recover_mosaic` makes with its two greens.
            clipped[c] |= v >= self.ceiling[c];
            brightest = brightest.max((v - self.black).max(0.0) * self.wb[c]);
        }
        if present.contains(&false) {
            return 0;
        }
        let any_clipped = clipped.contains(&true);
        let all_clipped = !clipped.contains(&false);
        if !any_clipped {
            return 0;
        }

        let mut moved = 0;
        if rebuilt {
            // A blown block is neutral. A block recovery rebuilt keeps the
            // colour it was given just over the clipping point and is neutral
            // by half a stop above it (`FADE_SPAN`): a hard edge either way
            // draws a line along every clipping contour.
            let weight = if all_clipped {
                1.0
            } else {
                smoothstep(self.first_to_clip, self.fade_end, brightest)
            };
            if weight <= 0.0 {
                return 0;
            }
            for (v, &c) in values.iter_mut().zip(colours) {
                let neutral = self.black + brightest / self.wb[c];
                let settled = *v + (neutral - *v) * weight;
                if *v != settled {
                    *v = settled;
                    moved += 1;
                }
            }
        } else {
            for (v, &c) in values.iter_mut().zip(colours) {
                if *v > self.cap[c] {
                    *v = self.cap[c];
                    moved += 1;
                }
            }
        }
        moved
    }
}

// Copied from src-tauri/src/mods/highlights.rs @ 2bcb953d.
/// 0 at or below `edge0`, 1 at or above `edge1`, smooth in between.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge1 <= edge0 {
        return if x >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// Copied from src-tauri/src/mods/highlights.rs @ 2bcb953d.
/// Walk a buffer `block` x `block` at a time and settle every block.
///
/// `w` is in values, not pixels: a three-colour image is walked as a row of
/// `3 * width` values with blocks of one row by three, where `colour_at` gives
/// the channel. A partial block at the right or bottom edge is skipped; it is
/// at most two photosites wide and outside every crop.
fn settle_buffer(
    raw: &mut RawImage,
    w: usize,
    h: usize,
    block: usize,
    colour_at: impl Fn(usize, usize) -> usize + Sync,
    settle: &Settle,
    rebuilt: bool,
) {
    use rayon::prelude::*;

    // Most photos have nothing clipped at all. Find that out without copying
    // anything, and leave the decoded integers exactly as they were.
    let lowest = settle.ceiling.iter().copied().fold(f32::INFINITY, f32::min);
    let anything_clipped = match &raw.data {
        RawImageData::Integer(v) => v.par_iter().any(|&x| x as f32 >= lowest),
        RawImageData::Float(v) => v.par_iter().any(|&x| x >= lowest),
    };
    if !anything_clipped || w == 0 || h < block {
        return;
    }

    let mut out: Vec<f32> = raw.data.as_f32().into_owned();
    if out.len() != w * h {
        return;
    }
    // A three-colour row is one block high and three values wide.
    let (block_h, block_w) = if block == 1 { (1, 3) } else { (block, block) };

    let moved: usize = out
        .par_chunks_mut(w * block_h)
        .enumerate()
        .map(|(band, rows)| {
            if rows.len() < w * block_h {
                return 0;
            }
            let row0 = band * block_h;
            let mut values = [0.0f32; 9];
            let mut colours = [0usize; 9];
            let n = block_h * block_w;
            let mut moved = 0;
            let mut col = 0;
            while col + block_w <= w {
                for i in 0..n {
                    let (dr, dc) = (i / block_w, i % block_w);
                    values[i] = rows[dr * w + col + dc];
                    colours[i] = colour_at(row0 + dr, col + dc);
                }
                let m = settle.block(&mut values[..n], &colours[..n], rebuilt);
                if m > 0 {
                    for (i, &v) in values[..n].iter().enumerate() {
                        let (dr, dc) = (i / block_w, i % block_w);
                        rows[dr * w + col + dc] = v;
                    }
                    moved += m;
                }
                col += block_w;
            }
            moved
        })
        .sum();

    if moved > 0 {
        raw.data = RawImageData::Float(out);
    }
}

// Copied from src-tauri/src/mods/highlights.rs @ 2bcb953d (`recovery_tests`
// and `settle_tests`), so the copy keeps the properties the original is held
// to.
#[cfg(test)]
mod tests {
    use super::*;

    mod recovery {
        use super::*;

        const CEILINGS: [f32; 3] = [1000.0, 1000.0, 1000.0];

        fn neutral() -> HighlightColour {
            HighlightColour {
                ratio: [1.0, 1.0, 1.0],
            }
        }

        #[test]
        fn a_pixel_with_nothing_clipped_is_untouched() {
            let p = [500.0, 600.0, 700.0];
            assert_eq!(rebuild(p, CEILINGS, &neutral()), p);
        }

        #[test]
        fn a_pixel_with_everything_clipped_is_left_alone() {
            let p = [1000.0, 1000.0, 1000.0];
            assert_eq!(rebuild(p, CEILINGS, &neutral()), p);
        }

        #[test]
        fn a_clipped_green_is_rebuilt_from_red_and_blue() {
            let colour = HighlightColour {
                ratio: [0.9, 1.0, 0.8],
            };
            // Red at 990 implies 1100, blue at 800 implies 1000: averaged, 1050.
            let out = rebuild([990.0, 1000.0, 800.0], CEILINGS, &colour);
            assert!((out[1] - 1050.0).abs() < 1.0, "green came back as {}", out[1]);
            assert_eq!(out[0], 990.0, "red was not clipped and must not move");
            assert_eq!(out[2], 800.0, "blue was not clipped and must not move");
        }

        #[test]
        fn a_rebuilt_channel_is_never_lowered() {
            let out = rebuild([1000.0, 1000.0, 200.0], CEILINGS, &neutral());
            assert!(out[0] >= 1000.0 && out[1] >= 1000.0, "{out:?}");
        }

        #[test]
        fn a_rebuild_never_contradicts_a_clipped_channel() {
            let colour = HighlightColour {
                ratio: [0.65, 1.0, 0.6],
            };
            let out = rebuild([1000.0, 1000.0, 480.0], CEILINGS, &colour);
            let level_r = out[0] / colour.ratio[0];
            let level_g = out[1] / colour.ratio[1];
            assert!(level_g >= level_r - 1.0, "green rebuilt below what red proves: {out:?}");
            assert_eq!(out[2], 480.0, "blue was not clipped and must not move");
        }

        #[test]
        fn a_dark_photo_teaches_nothing() {
            let dim = std::iter::repeat_n([100.0f32, 100.0, 100.0], 10_000);
            assert!(highlight_colour(dim, CEILINGS).is_none());
        }

        #[test]
        fn the_colour_is_learnt_from_bright_unclipped_pixels_only() {
            let bright = std::iter::repeat_n([600.0f32, 800.0, 400.0], 2_000);
            let clipped = std::iter::repeat_n([1000.0f32, 1000.0, 1000.0], 50_000);
            let c = highlight_colour(bright.chain(clipped), CEILINGS).expect("enough samples");
            assert!((c.ratio[0] - 0.75).abs() < 1e-3, "{:?}", c.ratio);
            assert!((c.ratio[2] - 0.50).abs() < 1e-3, "{:?}", c.ratio);
        }
    }

    mod settle {
        use super::*;

        const BLACK: f32 = 100.0;
        const CEILINGS: [f32; 3] = [1000.0, 1000.0, 1000.0];
        const WB: [f32; 4] = [2.0, 1.0, 1.5, f32::NAN];
        const RGGB: [usize; 4] = [0, 1, 1, 2];

        fn settle() -> Settle {
            Settle::new(CEILINGS, BLACK, WB).expect("valid levels")
        }

        fn balanced(values: &[f32], colours: &[usize]) -> Vec<f32> {
            let wb = [WB[0], WB[1], WB[2]];
            values
                .iter()
                .zip(colours)
                .map(|(v, &c)| (v - BLACK) * wb[c])
                .collect()
        }

        fn spread(b: &[f32]) -> f32 {
            let (lo, hi) = b
                .iter()
                .fold((f32::INFINITY, 0.0f32), |(lo, hi), &x| (lo.min(x), hi.max(x)));
            (hi - lo) / hi
        }

        #[test]
        fn a_blown_block_comes_out_neutral_either_way() {
            for rebuilt in [true, false] {
                let mut block = [1000.0, 1000.0, 1000.0, 1000.0];
                assert!(settle().block(&mut block, &RGGB, rebuilt) > 0);
                assert!(spread(&balanced(&block, &RGGB)) < 1e-5);
            }
        }

        #[test]
        fn one_green_just_short_of_the_ceiling_still_counts_as_blown() {
            let mut block = [1000.0, 1000.0, 995.0, 1000.0];
            assert!(settle().block(&mut block, &RGGB, true) > 0);
            assert!(spread(&balanced(&block, &RGGB)) < 1e-5);
        }

        #[test]
        fn with_recovery_a_blown_core_is_as_bright_as_its_brightest_channel() {
            let mut block = [1000.0, 1000.0, 1000.0, 1000.0];
            settle().block(&mut block, &RGGB, true);
            let level = balanced(&block, &RGGB)[0];
            assert!((level - 900.0 * 2.0).abs() < 1e-3, "level {level}");
        }

        #[test]
        fn without_recovery_clipping_lands_on_the_first_channel_to_stop() {
            let mut block = [1000.0, 1000.0, 1000.0, 1000.0];
            settle().block(&mut block, &RGGB, false);
            let level = balanced(&block, &RGGB)[0];
            assert!((level - 900.0).abs() < 1e-3, "level {level}");
        }

        #[test]
        fn a_block_with_nothing_clipped_is_untouched() {
            for rebuilt in [true, false] {
                let before = [900.0, 600.0, 650.0, 400.0];
                let mut block = before;
                assert_eq!(settle().block(&mut block, &RGGB, rebuilt), 0);
                assert_eq!(block, before);
            }
        }

        #[test]
        fn with_recovery_half_a_stop_past_the_clip_is_neutral() {
            let mut block = [710.0, 1400.0, 1400.0, 998.0];
            settle().block(&mut block, &RGGB, true);
            assert!(spread(&balanced(&block, &RGGB)) < 1e-4);
        }

        #[test]
        fn clipping_never_raises_a_value() {
            let before = [1000.0, 1000.0, 1000.0, 120.0];
            let mut block = before;
            settle().block(&mut block, &RGGB, false);
            for (a, b) in block.iter().zip(before) {
                assert!(*a <= b, "{block:?}");
            }
        }

        #[test]
        fn missing_white_balance_means_unity() {
            let s = Settle::new(CEILINGS, BLACK, [f32::NAN; 4]).expect("valid");
            assert_eq!(s.wb, [1.0, 1.0, 1.0]);
            assert!(Settle::new(CEILINGS, BLACK, [2.0, 0.0, 1.5, 1.0]).is_none());
        }

        #[test]
        fn smoothstep_has_flat_ends() {
            assert_eq!(smoothstep(1.0, 2.0, 0.5), 0.0);
            assert_eq!(smoothstep(1.0, 2.0, 2.5), 1.0);
            assert!((smoothstep(1.0, 2.0, 1.5) - 0.5).abs() < 1e-6);
        }
    }
}
