//! Highlight recovery: putting colour back where a channel clipped.
//!
//! WHAT THE PROBLEM ACTUALLY IS
//!
//! A sensor photosite saturates. Past that point it counts no more light and
//! reports its ceiling, and the ceiling is per channel — the three channels do
//! not reach it together, because the scene is not neutral and because the
//! camera's own white balance means green saturates long before red and blue in
//! daylight. So a bright area does not go white. It goes *wrong*: green pins to
//! its ceiling while red and blue keep climbing, white balance then multiplies
//! red and blue by nearly two, and a white dress in sun comes out magenta, or a
//! sunset comes out with a hard cyan edge where the sky stopped being data.
//!
//! Recovery is the observation that a pixel with one clipped channel and two
//! good ones is not lost. The two that survived say what colour it was and
//! roughly how bright; the missing one can be estimated rather than left at its
//! ceiling. A pixel with all three clipped is genuinely gone, and no amount of
//! arithmetic invents it — the honest thing there is a smooth white, not a
//! hallucination.
//!
//! WHY THIS IS NOT THE HIGHLIGHTS SLIDER
//!
//! RapidRAW has one, in `shader.wgsl`. It is a tone adjustment: it moves detail
//! that is still in the file. It cannot help here, because the problem is
//! detail that is *not* in the file — and it runs after demosaic, by which
//! point a clipped green has already been smeared across its neighbours.
//!
//! WHY MEASUREMENT COMES FIRST
//!
//! Camera profiles were built on the reasoning that they would close the gap
//! against darktable. They could not, and one measurement afterwards said so.
//! So this module starts with the measurement and not the fix: how much of a
//! real photo is clipped, and how much of that is *recoverable* — one or two
//! channels gone, not all three. That number is the ceiling on what any
//! recovery can be worth, and it is knowable before writing the recovery.

// The survey below is a measuring instrument, run by hand, and nothing in the
// app calls it. It stays because the numbers it produces are the reason
// highlight recovery is built the way it is, and a measurement you cannot rerun
// is a claim rather than a measurement.
#![allow(dead_code)]

use rawler::rawimage::{RawImage, RawImageData};

/// How close to the ceiling still counts as clipped, as a fraction of the
/// range above black.
///
/// Not 100%: sensors do not saturate at exactly the tabulated white level.
/// Noise, per-channel variation and the manufacturer's own rounding mean the
/// real ceiling sits slightly below, and a pixel one count short of the table
/// is as dead as one that reached it. darktable uses a comparable margin for
/// the same reason.
const CLIP_MARGIN: f32 = 0.99;

/// What one photo's highlights look like.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Clipping {
    /// 2x2 CFA blocks examined.
    pub blocks: u64,
    /// Blocks with at least one channel at its ceiling.
    pub clipped: u64,
    /// Blocks with at least one clipped channel *and* at least one good one.
    /// This is the part recovery can do anything about.
    pub recoverable: u64,
    /// Blocks with every channel at its ceiling. Gone for good.
    pub blown: u64,
}

impl Clipping {
    pub fn percent_clipped(&self) -> f64 {
        self.fraction(self.clipped)
    }

    pub fn percent_recoverable(&self) -> f64 {
        self.fraction(self.recoverable)
    }

    pub fn percent_blown(&self) -> f64 {
        self.fraction(self.blown)
    }

    fn fraction(&self, n: u64) -> f64 {
        if self.blocks == 0 {
            return 0.0;
        }
        n as f64 / self.blocks as f64 * 100.0
    }
}

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

/// Count what is clipped in one decoded RAW.
///
/// Works on 2x2 CFA blocks rather than single photosites, because "this pixel
/// clipped" is only meaningful alongside the neighbours that carry the other
/// colours — one photosite holds one channel and cannot be recoverable or
/// blown on its own.
///
/// sRAW is measured differently and more directly — see `measure_three_colour`.
///
/// `None` when the image is neither: a floating point DNG, or anything else
/// this does not know how to read. Not wrong, just not this measurement.
pub fn measure(raw: &RawImage) -> Option<Clipping> {
    match raw.cpp {
        1 => measure_mosaic(raw),
        3 => measure_three_colour(raw),
        _ => None,
    }
}

/// Canon sRAW and mRAW, where every pixel already carries all three channels.
///
/// No block arithmetic and no guessing which colour a photosite is: "one
/// channel gone, two good" is a fact about the pixel itself. AK shoots a great
/// deal of sRAW, so leaving this out would have measured the wrong half of the
/// library.
fn measure_three_colour(raw: &RawImage) -> Option<Clipping> {
    let RawImageData::Integer(pixels) = &raw.data else {
        return None;
    };
    let ceilings = ceilings(raw)?;

    let mut out = Clipping::default();
    for pixel in pixels.as_chunks::<3>().0 {
        let clipped = (0..3).filter(|&c| pixel[c] as f32 >= ceilings[c]).count();
        out.blocks += 1;
        if clipped > 0 {
            out.clipped += 1;
            if clipped == 3 {
                out.blown += 1;
            } else {
                out.recoverable += 1;
            }
        }
    }
    Some(out)
}

fn measure_mosaic(raw: &RawImage) -> Option<Clipping> {
    let RawImageData::Integer(pixels) = &raw.data else {
        return None;
    };
    let ceilings = ceilings(raw)?;

    let (w, h) = (raw.width, raw.height);
    if w < 2 || h < 2 {
        return None;
    }

    let mut out = Clipping::default();
    for row in (0..h - 1).step_by(2) {
        for col in (0..w - 1).step_by(2) {
            let mut any_clipped = false;
            let mut all_clipped = true;

            for (dr, dc) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                let (r, c) = (row + dr, col + dc);
                let value = pixels[r * w + c] as f32;
                let colour = raw.camera.cfa.color_at(r, c).min(3);
                if value >= ceilings[colour] {
                    any_clipped = true;
                } else {
                    all_clipped = false;
                }
            }

            out.blocks += 1;
            if any_clipped {
                out.clipped += 1;
                if all_clipped {
                    out.blown += 1;
                } else {
                    out.recoverable += 1;
                }
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_photo_with_no_blocks_reports_nothing() {
        let empty = Clipping::default();
        assert_eq!(empty.percent_clipped(), 0.0);
        assert_eq!(empty.percent_recoverable(), 0.0);
        assert_eq!(empty.percent_blown(), 0.0);
    }

    /// The three counts have to partition the clipped blocks, or the numbers
    /// this whole decision rests on are not comparable.
    #[test]
    fn recoverable_and_blown_account_for_every_clipped_block() {
        let c = Clipping {
            blocks: 100,
            clipped: 30,
            recoverable: 18,
            blown: 12,
        };
        assert_eq!(c.recoverable + c.blown, c.clipped);
        assert!((c.percent_clipped() - 30.0).abs() < 1e-9);
        assert!((c.percent_recoverable() - 18.0).abs() < 1e-9);
    }
}

/// What real photos actually look like, before any recovery exists.
///
/// Run by hand against AK's files. The point is a number, not a pass: if the
/// recoverable share of a bright photo is a fraction of a percent then highlight
/// recovery is not the big rock it was put on the roadmap as, and that is worth
/// knowing before it is built rather than after.
#[cfg(test)]
mod survey {
    use super::*;

    fn clipping_of(path: &std::path::Path) -> Option<Clipping> {
        let bytes = std::fs::read(path).ok()?;
        let source = rawler::rawsource::RawSource::new_from_slice(&bytes);
        let decoder = rawler::get_decoder(&source).ok()?;
        let mut raw = decoder
            .raw_image(
                &source,
                &rawler::decoders::RawDecodeParams::default(),
                false,
            )
            .ok()?;
        // The same levels the app decodes with, or sRAW would be measured
        // against a ceiling that is not its own.
        crate::mods::sraw_levels::fix(&mut raw, &bytes);
        measure(&raw)
    }

    #[test]
    #[ignore = "reads AK's photos; run by hand"]
    fn how_much_of_a_real_photo_is_clipped() {
        let dir = std::env::var("AG_DIR").expect("set AG_DIR to a folder of RAWs");
        let mut rows = Vec::new();

        for entry in std::fs::read_dir(&dir).expect("folder").flatten() {
            let path = entry.path();
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("cr2") || e.eq_ignore_ascii_case("dng"))
            {
                continue;
            }
            let Some(c) = clipping_of(&path) else {
                println!("skipped (not a Bayer mosaic): {}", path.display());
                continue;
            };
            rows.push((path.file_stem().unwrap().to_string_lossy().to_string(), c));
        }

        rows.sort_by(|a, b| {
            b.1.percent_recoverable()
                .partial_cmp(&a.1.percent_recoverable())
                .unwrap()
        });

        println!(
            "\n{:<46} {:>10} {:>13} {:>9}",
            "photo", "clipped", "recoverable", "blown"
        );
        for (name, c) in &rows {
            println!(
                "{:<46} {:>9.2}% {:>12.2}% {:>8.2}%",
                name.get(..46).unwrap_or(name),
                c.percent_clipped(),
                c.percent_recoverable(),
                c.percent_blown()
            );
        }

        let n = rows.len().max(1) as f64;
        let mean = |f: fn(&Clipping) -> f64| rows.iter().map(|r| f(&r.1)).sum::<f64>() / n;
        println!(
            "\n{} photos   mean clipped {:.2}%   recoverable {:.2}%   blown {:.2}%\n",
            rows.len(),
            mean(Clipping::percent_clipped),
            mean(Clipping::percent_recoverable),
            mean(Clipping::percent_blown),
        );
    }
}

// ============================================================================
// RECOVERY
// ============================================================================

/// How bright an unclipped pixel has to be before it is allowed to say what
/// colour the highlights of this photo are, as a fraction of the range to the
/// ceiling.
///
/// The ratio between channels is not constant across a photo — shadows are
/// bluer, skin is redder — and the only ratio that matters here is the one just
/// below the clipping point, because that is what the clipped pixels *were*.
/// Too low and the estimate is contaminated by the whole scene; too high and
/// there are not enough pixels left to average.
const BRIGHT_ENOUGH: f32 = 0.5;

/// Fewest bright unclipped samples that will be trusted.
///
/// A ratio measured from a handful of pixels is noise, and reconstructing from
/// noise is worse than leaving the highlight alone.
const ENOUGH_SAMPLES: u64 = 500;

/// The colour of this photo's highlights, as each channel's share relative to
/// green.
///
/// WHY THIS IS MEASURED AND NOT ASSUMED
///
/// The obvious shortcut is the camera's white balance coefficients: a highlight
/// is usually the light source, the light source is what white balance
/// describes, so a clipped channel could be reconstructed from that. It is
/// often right and it fails exactly where it matters — a sunset, a tungsten
/// lamp, a white dress under leaves — because those highlights are not the
/// colour the camera was balanced for.
///
/// darktable's opposed method solves this by reading the ratio out of the
/// pixels ringing each clipped region, which is self-calibrating: whatever the
/// light was, the pixels that nearly clipped were that colour. This does the
/// same thing globally rather than per region — every unclipped pixel bright
/// enough to be near the ceiling, averaged. Cruder in a photo lit by two
/// different lights, and it needs no neighbourhood search, which is what keeps
/// it affordable in the decode path.
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

/// Rebuild one pixel's clipped channels. Returns it unchanged when there is
/// nothing to do.
///
/// The estimate of how bright the pixel really was comes from whichever
/// unclipped channel implies the *most* light. That is not arbitrary: a clipped
/// channel is a lower bound on the truth, so among the channels that can still
/// be read, the one implying the most light is nearest the ceiling and least
/// likely to be understating the pixel.
///
/// A reconstructed channel is only ever raised. Lowering one would invent
/// darkness where the sensor reported light, and the sensor is not wrong about
/// that — it is only incapable of saying how much.
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
    // They are averaged rather than maximised.
    //
    // Taking the brightest reading looks reasonable — a clipped channel is a
    // lower bound, so surely the highest estimate is nearest the truth — and it
    // is wrong. Every reading carries noise, and the maximum of several noisy
    // readings is biased upwards by construction, so barely-clipped pixels get
    // pushed past where they belong. Measured on a frame that is 45% clipped,
    // the maximum made the colour cast 2% *worse*; the mean is unbiased.
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
    let level = level / readings as f32;
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

#[cfg(test)]
mod recovery_tests {
    use super::*;

    const CEILINGS: [f32; 3] = [1000.0, 1000.0, 1000.0];

    fn neutral() -> HighlightColour {
        HighlightColour {
            ratio: [1.0, 1.0, 1.0],
        }
    }

    /// The property every photo without blown highlights depends on.
    #[test]
    fn a_pixel_with_nothing_clipped_is_untouched() {
        let p = [500.0, 600.0, 700.0];
        assert_eq!(rebuild(p, CEILINGS, &neutral()), p);
    }

    /// All three gone means no evidence. Inventing a value here is what makes a
    /// highlight look wrong in a way nobody can quite point at.
    #[test]
    fn a_pixel_with_everything_clipped_is_left_alone() {
        let p = [1000.0, 1000.0, 1000.0];
        assert_eq!(rebuild(p, CEILINGS, &neutral()), p);
    }

    /// The case this exists for: green saturates first, so a bright neutral
    /// area reads magenta until the green is put back.
    #[test]
    fn a_clipped_green_is_rebuilt_from_red_and_blue() {
        let colour = HighlightColour {
            ratio: [0.9, 1.0, 0.8],
        };
        // Red at 990 implies a level of 1100, blue at 800 implies 1000, and the
        // two readings are averaged: 1050. Green comes back there rather than
        // stopping at its ceiling.
        let out = rebuild([990.0, 1000.0, 800.0], CEILINGS, &colour);
        assert!(
            (out[1] - 1050.0).abs() < 1.0,
            "green came back as {}",
            out[1]
        );
        assert_eq!(out[0], 990.0, "red was not clipped and must not move");
        assert_eq!(out[2], 800.0, "blue was not clipped and must not move");
    }

    /// Never darker than the sensor said.
    #[test]
    fn a_rebuilt_channel_is_never_lowered() {
        let colour = HighlightColour {
            ratio: [1.0, 1.0, 1.0],
        };
        // Blue is unclipped and dim, which implies a level below the ceiling.
        // The clipped channels must not follow it down.
        let out = rebuild([1000.0, 1000.0, 200.0], CEILINGS, &colour);
        assert!(out[0] >= 1000.0 && out[1] >= 1000.0, "{out:?}");
    }

    /// With no bright pixels there is no evidence, and the honest answer is to
    /// say so rather than to average the whole photo.
    #[test]
    fn a_dark_photo_teaches_nothing() {
        let dim = std::iter::repeat_n([100.0f32, 100.0, 100.0], 10_000);
        assert!(highlight_colour(dim, CEILINGS).is_none());
    }

    /// And clipped pixels must not reach the average, or the measured colour
    /// drifts towards whatever the ceilings happen to be.
    #[test]
    fn the_colour_is_learnt_from_bright_unclipped_pixels_only() {
        let bright = std::iter::repeat_n([600.0f32, 800.0, 400.0], 2_000);
        let clipped = std::iter::repeat_n([1000.0f32, 1000.0, 1000.0], 50_000);
        let c = highlight_colour(bright.chain(clipped), CEILINGS).expect("enough samples");
        assert!((c.ratio[0] - 0.75).abs() < 1e-3, "{:?}", c.ratio);
        assert!((c.ratio[2] - 0.50).abs() < 1e-3, "{:?}", c.ratio);
    }
}

// ============================================================================
// APPLYING IT TO A DECODED RAW
// ============================================================================

/// Every Nth block is enough to learn the highlight colour.
///
/// The statistics pass exists to average a ratio, and a ratio converges long
/// before twenty million samples. Sampling keeps this off the critical path of
/// every photo opened and every thumbnail built; `ENOUGH_SAMPLES` still has to
/// be met, so a photo with few bright pixels is refused rather than measured
/// from too little.
const SAMPLE_STRIDE: usize = 4;

/// Put back what the sensor could not record, in place, before demosaic.
///
/// Runs from the decode anchor on every RAW. That is deliberate and safe: a
/// pixel with nothing clipped is returned bit-for-bit, so a photo with no blown
/// highlights decodes exactly as it did before this existed. The measurement
/// says most of AK's photos are in that state.
///
/// Silent about everything it cannot do — an unreadable level table, a format
/// that is not a mosaic or a triple, a frame too dark to learn a colour from.
/// None of those is an error, and none is worth failing a decode over.
pub fn recover(raw: &mut RawImage) {
    if !enabled() {
        return;
    }
    // A way to render the same photo without this from a test, so the two can
    // be put side by side. Nothing in the app reads the environment.
    if std::env::var_os("AG_NO_RECOVERY").is_some() {
        return;
    }
    let Some(ceil4) = ceilings(raw) else { return };
    let ceilings = [ceil4[0], ceil4[1], ceil4[2]];

    match raw.cpp {
        3 => recover_three_colour(raw, ceilings),
        1 => recover_mosaic(raw, ceilings),
        _ => {}
    }
}

/// WHY THE RESULT IS WRITTEN AS FLOAT
///
/// Reconstruction puts values *above* the sensor's ceiling — that is what it is
/// for — and the decoded buffer is unsigned 16-bit. On a Canon sRAW the ceiling
/// is 64424 of a possible 65535, which is 1.7% of headroom: a highlight that
/// should come back at twice the ceiling has nowhere to go, and the first two
/// versions of this quietly reconstructed almost nothing because every value
/// hit the top of the integer.
///
/// rawler carries `RawImageData::Float` for exactly this reason, converts
/// everything to `f32` on the next step anyway, and its black-level correction
/// clips negatives only — nothing downstream puts a lid on a bright value. So
/// where reconstruction happens the buffer is handed on as float, and where it
/// does not the original integers are left exactly as they were.
fn recover_three_colour(raw: &mut RawImage, ceilings: [f32; 3]) {
    let RawImageData::Integer(pixels) = &raw.data else {
        return;
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
    let Some(colour) = colour else { return };

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
    // buffer for no reason — and keep the guarantee that a photo without blown
    // highlights is decoded exactly as it was before this existed.
    if touched {
        raw.data = RawImageData::Float(out);
    }
}

/// A Bayer mosaic, one 2x2 block at a time.
///
/// A photosite holds one colour, so "this pixel is magenta" is not a statement
/// about a photosite — it is a statement about the block that will become a
/// pixel. The block is therefore the unit: read the three colours out of it,
/// rebuild the ones that clipped, and write each rebuilt value back only to the
/// photosites it came from.
///
/// Coarser than reconstructing per photosite from a wider neighbourhood, which
/// is what darktable's laplacian methods do. It is also the resolution at which
/// the artefact exists, and blown highlights are smooth: there is no detail
/// left in them to lose.
fn recover_mosaic(raw: &mut RawImage, ceilings: [f32; 3]) {
    let (w, h) = (raw.width, raw.height);
    if w < 2 || h < 2 {
        return;
    }
    // Copied out because reading the pattern borrows `raw` while writing pixels
    // needs it mutably, and the pattern is four numbers.
    let pattern: Vec<usize> = (0..4)
        .map(|i| raw.camera.cfa.color_at(i / 2, i % 2).min(2))
        .collect();

    let RawImageData::Integer(pixels) = &raw.data else {
        return;
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
    let Some(colour) = colour else { return };

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
}

/// Does recovery change what a person sees, and in the right direction?
///
/// Two questions, and the second is the one that matters. A reconstruction that
/// *changes* pixels proves nothing — the swapped camera profile changed pixels
/// too, and it was wrong on purpose. What has to be shown is that clipped
/// highlights get *less* coloured, because the fault being fixed is a colour
/// cast: green saturates first, so a blown neutral reads magenta.
#[cfg(test)]
mod proof {
    use super::*;

    fn decode(path: &str, with_recovery: bool) -> Option<(Vec<u16>, usize, [f32; 3])> {
        let bytes = std::fs::read(path).ok()?;
        let source = rawler::rawsource::RawSource::new_from_slice(&bytes);
        let decoder = rawler::get_decoder(&source).ok()?;
        let mut raw = decoder
            .raw_image(
                &source,
                &rawler::decoders::RawDecodeParams::default(),
                false,
            )
            .ok()?;
        crate::mods::sraw_levels::fix(&mut raw, &bytes);
        let c4 = ceilings(&raw)?;
        let ceil = [c4[0], c4[1], c4[2]];
        if with_recovery {
            recover(&mut raw);
        }
        let RawImageData::Integer(pixels) = raw.data else {
            return None;
        };
        Some((pixels, raw.cpp, ceil))
    }

    /// How far from neutral the clipped pixels are, once white balance has been
    /// undone by dividing each channel by what the photo's own highlights say
    /// it should be.
    ///
    /// A perfectly reconstructed blown highlight scores zero: all three
    /// channels sit at the same level. A magenta one scores high, because green
    /// is stuck at its ceiling while red and blue are not.
    fn cast_of_clipped(
        pixels: &[u16],
        cpp: usize,
        ceilings: [f32; 3],
        ratio: [f32; 3],
    ) -> Option<(f64, u64)> {
        if cpp != 3 {
            return None;
        }
        let mut total = 0.0f64;
        let mut n = 0u64;
        for p in pixels.as_chunks::<3>().0 {
            let v = [p[0] as f32, p[1] as f32, p[2] as f32];
            // Only pixels that were clipped in the original. A pixel is judged
            // clipped by the *ceiling*, which recovery pushes values past — so
            // "at or above" still finds them afterwards.
            if !(0..3).any(|c| v[c] >= ceilings[c]) {
                continue;
            }
            // How far the three levels sit from agreeing, as a fraction of
            // their average.
            //
            // The first version of this took the spread between the highest
            // and the lowest, and reported exactly 0% improvement on a
            // reconstruction that was working: green clips, so green is the
            // *middle* channel once it has been rebuilt, and a measure that
            // only watches the two extremes cannot see the middle one move.
            let level = [v[0] / ratio[0], v[1] / ratio[1], v[2] / ratio[2]];
            let mean = (level[0] + level[1] + level[2]) / 3.0;
            if mean > 1e-6 {
                let spread = (0..3).map(|c| (level[c] - mean).powi(2)).sum::<f32>() / 3.0;
                total += (spread.sqrt() / mean) as f64;
                n += 1;
            }
        }
        (n > 0).then_some((total / n as f64, n))
    }

    #[test]
    #[ignore = "reads AK's photos; run by hand"]
    fn recovery_makes_blown_highlights_less_coloured() {
        let path = std::env::var("AG_RAW").expect("set AG_RAW");

        let (before, cpp, ceilings) = decode(&path, false).expect("decode");
        let (after, _, _) = decode(&path, true).expect("decode with recovery");

        let changed = before
            .iter()
            .zip(after.iter())
            .filter(|(a, b)| a != b)
            .count();
        println!(
            "\nvalues changed: {changed} of {} ({:.2}%)",
            before.len(),
            changed as f64 / before.len() as f64 * 100.0
        );

        // Measured against the photo's own highlight colour, which is what the
        // reconstruction aims at — scoring against neutral would just reward
        // making everything grey.
        let ratio = highlight_colour(
            before
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]),
            ceilings,
        )
        .expect("a bright enough photo")
        .ratio;
        println!(
            "highlight colour: R {:.3}  G 1.000  B {:.3}",
            ratio[0], ratio[2]
        );

        let (cast_before, n) =
            cast_of_clipped(&before, cpp, ceilings, ratio).expect("three colour");
        let (cast_after, _) = cast_of_clipped(&after, cpp, ceilings, ratio).expect("three colour");

        println!("clipped pixels: {n}");
        println!("colour cast across them   before {cast_before:.3}   after {cast_after:.3}");
        println!(
            "improvement: {:.0}%\n",
            (1.0 - cast_after / cast_before.max(1e-9)) * 100.0
        );

        assert!(changed > 0, "recovery did nothing at all");
        assert!(
            cast_after < cast_before,
            "recovery left the highlights more coloured, not less"
        );
    }

    /// And the property the whole always-on decision rests on: a photo with
    /// nothing clipped has to come out bit for bit identical.
    #[test]
    #[ignore = "reads AK's photos; run by hand"]
    fn a_photo_without_clipping_is_untouched() {
        let path =
            std::env::var("AG_CLEAN_RAW").expect("set AG_CLEAN_RAW to a photo with no clipping");
        let (before, _, _) = decode(&path, false).expect("decode");
        let (after, _, _) = decode(&path, true).expect("decode with recovery");
        let changed = before
            .iter()
            .zip(after.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            changed, 0,
            "{changed} values moved in a photo with nothing to recover"
        );
    }
}

/// What the numbers in a clipped photo actually are.
///
/// Written because two plausible reconstructions in a row did nearly nothing,
/// and at that point guessing again is worse than looking.
#[cfg(test)]
mod diagnose {
    use super::*;

    #[test]
    #[ignore = "reads AK's photos; run by hand"]
    fn what_do_the_clipped_pixels_look_like() {
        let path = std::env::var("AG_RAW").expect("set AG_RAW");
        let bytes = std::fs::read(&path).expect("read");
        let source = rawler::rawsource::RawSource::new_from_slice(&bytes);
        let decoder = rawler::get_decoder(&source).expect("decoder");
        let mut raw = decoder
            .raw_image(
                &source,
                &rawler::decoders::RawDecodeParams::default(),
                false,
            )
            .expect("decode");
        crate::mods::sraw_levels::fix(&mut raw, &bytes);

        let c4 = ceilings(&raw).expect("ceilings");
        println!(
            "\ncpp {}   whitelevel {:?}   ceilings {:?}",
            raw.cpp,
            raw.whitelevel.0,
            &c4[..3]
        );
        println!("blacklevel {:?}", raw.blacklevel.levels);
        println!("wb_coeffs {:?}", raw.wb_coeffs);

        let RawImageData::Integer(pixels) = &raw.data else {
            panic!("not integer data");
        };
        let ceilings = [c4[0], c4[1], c4[2]];

        // Where does each channel actually top out?
        let mut maxes = [0u16; 3];
        let mut at_ceiling = [0u64; 3];
        for p in pixels.as_chunks::<3>().0 {
            for c in 0..3 {
                maxes[c] = maxes[c].max(p[c]);
                if p[c] as f32 >= ceilings[c] {
                    at_ceiling[c] += 1;
                }
            }
        }
        let total = (pixels.len() / 3) as f64;
        println!("\nchannel maxima      {maxes:?}");
        println!(
            "at or over ceiling  R {:.2}%   G {:.2}%   B {:.2}%",
            at_ceiling[0] as f64 / total * 100.0,
            at_ceiling[1] as f64 / total * 100.0,
            at_ceiling[2] as f64 / total * 100.0
        );

        let colour = highlight_colour(
            pixels
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]),
            ceilings,
        )
        .expect("bright enough");
        println!(
            "\nhighlight ratio     R {:.3}  G 1.000  B {:.3}",
            colour.ratio[0], colour.ratio[2]
        );

        // For pixels where green has clipped, what do red and blue say the
        // level should have been, against green's ceiling?
        let mut buckets = [0u64; 6];
        let mut n = 0u64;
        for p in pixels.as_chunks::<3>().0 {
            let v = [p[0] as f32, p[1] as f32, p[2] as f32];
            if v[1] < ceilings[1] {
                continue;
            }
            let from_r = v[0] / colour.ratio[0];
            let from_b = v[2] / colour.ratio[2];
            let implied = (from_r + from_b) / 2.0;
            let over = implied / ceilings[1];
            let bucket = if over < 1.0 {
                0
            } else if over < 1.01 {
                1
            } else if over < 1.05 {
                2
            } else if over < 1.10 {
                3
            } else if over < 1.20 {
                4
            } else {
                5
            };
            buckets[bucket] += 1;
            n += 1;
        }
        // Is there any structure left in red and blue where green has blown?
        // If those channels still vary across the blown region there is
        // information to reconstruct from; if they are pinned, there is not,
        // and no algorithm anywhere can invent it.
        {
            let mut r = Vec::new();
            let mut b = Vec::new();
            for p in pixels.as_chunks::<3>().0 {
                if (p[1] as f32) >= ceilings[1] {
                    r.push(p[0] as f64);
                    b.push(p[2] as f64);
                }
            }
            let stats = |v: &[f64]| {
                let n = v.len() as f64;
                let mean = v.iter().sum::<f64>() / n;
                let sd = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt();
                let lo = v.iter().cloned().fold(f64::INFINITY, f64::min);
                let hi = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                (lo, hi, mean, sd)
            };
            let (rlo, rhi, rmean, rsd) = stats(&r);
            let (blo, bhi, bmean, bsd) = stats(&b);
            println!(
                "
inside the blown region:"
            );
            println!(
                "  red   {rlo:.0} to {rhi:.0}   mean {rmean:.0}   sd {rsd:.1}  ({:.2}% of mean)",
                rsd / rmean * 100.0
            );
            println!(
                "  blue  {blo:.0} to {bhi:.0}   mean {bmean:.0}   sd {bsd:.1}  ({:.2}% of mean)",
                bsd / bmean * 100.0
            );
        }

        // Red and blue top out well below a table that says 64424 for all
        // three. Are they saturating too, at a ceiling of their own?
        let mut both_at_max = 0u64;
        let mut green_clipped = 0u64;
        for p in pixels.as_chunks::<3>().0 {
            if (p[1] as f32) < ceilings[1] {
                continue;
            }
            green_clipped += 1;
            if p[0] as f32 >= maxes[0] as f32 * 0.99 && p[2] as f32 >= maxes[2] as f32 * 0.99 {
                both_at_max += 1;
            }
        }
        println!(
            "of green-clipped pixels, red AND blue also within 1% of their own maxima: {:.1}%",
            both_at_max as f64 / green_clipped.max(1) as f64 * 100.0
        );

        println!("\ngreen-clipped pixels: {n}");
        let names = [
            "<1.00",
            "1.00-1.01",
            "1.01-1.05",
            "1.05-1.10",
            "1.10-1.20",
            ">1.20",
        ];
        for (i, name) in names.iter().enumerate() {
            println!(
                "  red+blue imply {name:>8} of green's ceiling   {:>10}  {:>5.1}%",
                buckets[i],
                buckets[i] as f64 / n.max(1) as f64 * 100.0
            );
        }
        println!();
    }
}

// ============================================================================
// THE SETTING
// ============================================================================
//
// WHY IT IS A SETTING AND NOT A SLIDER
//
// Neither Lightroom nor darktable gives highlight recovery an amount. Lightroom
// has no control at all — it happens, always, as part of reading the file.
// darktable and RawTherapee give you a *method* to choose between and a switch
// to turn it off, because there is nothing continuous to dial: a channel is
// either being reconstructed or it is being left at its ceiling.
//
// WHY IT IS NOT PER PHOTO
//
// Because this runs while the RAW is decoded, and a per-photo setting that
// changes the decode has to re-read the file every time it is flipped. Camera
// profiles were built that way first and it took four attempts and a rewrite
// onto the GPU before switching one stopped breaking something. This cannot go
// on the GPU — it has to happen before demosaic, on the mosaic itself — so
// instead it does not pretend to be instant: it is a preference, and it applies
// to the next photo opened.

use std::sync::atomic::{AtomicBool, Ordering};

/// On unless someone turns it off.
///
/// Default on because it is the right default and a free one: a photo with
/// nothing clipped comes out of it bit for bit unchanged.
static ENABLED: AtomicBool = AtomicBool::new(true);

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// The key this preference is stored under.
///
/// The file itself, and the read-modify-write that keeps other preferences
/// alive, are in `mods/ag_settings.rs`. This module used to own both, and wrote
/// the whole file on every save - which was correct while Argentum had exactly
/// one preference and would have quietly erased it the moment there were two.
const KEY: &str = "highlightRecovery";

/// Read the preference at startup. Missing or unreadable means on.
pub fn load(library: &std::path::Path) {
    let on = super::ag_settings::get(library, KEY)
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    set_enabled(on);
}

/// Write it, and apply it now.
pub fn save(library: &std::path::Path, on: bool) -> Result<(), String> {
    set_enabled(on);
    super::ag_settings::set(library, KEY, serde_json::Value::Bool(on))
}

#[cfg(test)]
mod setting_tests {
    use super::*;

    fn scratch(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("argentum-hl-{label}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    #[test]
    fn it_survives_a_restart() {
        let dir = scratch("roundtrip");
        save(&dir, false).expect("save");
        set_enabled(true); // as if the app had restarted with the default
        load(&dir);
        assert!(!enabled());

        save(&dir, true).expect("save");
        set_enabled(false);
        load(&dir);
        assert!(enabled());
        set_enabled(true);
    }

    /// No settings file is the normal case for everyone who never opens the
    /// switch, and it has to mean on.
    #[test]
    fn nothing_written_yet_means_on() {
        let dir = scratch("absent");
        set_enabled(false);
        load(&dir);
        assert!(enabled());
    }

    /// A corrupt file must not decide anything.
    #[test]
    fn rubbish_means_on() {
        let dir = scratch("rubbish");
        std::fs::write(dir.join("argentum-processing.json"), "not json").expect("write");
        set_enabled(false);
        load(&dir);
        assert!(enabled());
    }
}
