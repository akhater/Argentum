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
    // But the clipped channels are not silent: each says the pixel was at
    // least bright enough to reach it. That is not a noisy reading to average
    // in — it is a floor, and the estimate must not fall through it.
    //
    // Without the floor, a pixel with red and green clipped and blue surviving
    // takes its level from blue alone. If that implies less light than the
    // clipped red already proves, green is rebuilt to the low level while red
    // stays at its ceiling — never lowered — and the reconstruction itself is
    // magenta, which is the cast this module exists to remove. It showed as a
    // pink fringe along every blown edge on an R6 III window.
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

    /// Red and green clipped, blue surviving low. Blue alone implies a level
    /// below what the clipped red proves, and rebuilding green to that level
    /// leaves red above it: a magenta reconstruction. The red ceiling is a
    /// floor on the level.
    #[test]
    fn a_rebuild_never_contradicts_a_clipped_channel() {
        let colour = HighlightColour {
            ratio: [0.65, 1.0, 0.6],
        };
        // Blue at 480 implies a level of 800; red at its ceiling implies at
        // least 1000 / 0.65 = 1538.
        let out = rebuild([1000.0, 1000.0, 480.0], CEILINGS, &colour);
        let level_r = out[0] / colour.ratio[0];
        let level_g = out[1] / colour.ratio[1];
        assert!(
            level_g >= level_r - 1.0,
            "green rebuilt below what red proves: {out:?}"
        );
        assert_eq!(out[2], 480.0, "blue was not clipped and must not move");
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
///
/// Returns whether recovery was in charge of this photo's clipped pixels: on,
/// and with a measured highlight colour to rebuild them from. `settle_blown`
/// needs to know, because when it was not, nothing has dealt with them yet.
pub fn recover(raw: &mut RawImage) -> bool {
    if !enabled() {
        return false;
    }
    // A way to render the same photo without this from a test, so the two can
    // be put side by side. Nothing in the app reads the environment.
    if std::env::var_os("AG_NO_RECOVERY").is_some() {
        return false;
    }
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
    // buffer for no reason — and keep the guarantee that a photo without blown
    // highlights is decoded exactly as it was before this existed.
    if touched {
        raw.data = RawImageData::Float(out);
    }
    true
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

// ============================================================================
// WHAT IS GONE COMES OUT WHITE
// ============================================================================

/// Make sure what the sensor lost entirely comes out white, not magenta.
///
/// WHY THIS IS NOT PART OF RECOVERY
///
/// Recovery rebuilds a channel from the ones that survived. Where every
/// channel clipped there is nothing left to rebuild from, and `rebuild` rightly
/// refuses to invent one. But leaving those photosites alone is not neutral:
/// they all sit at their ceiling, white balance multiplies red and blue by two
/// or more, and the camera matrix turns that into flat magenta. A blown window
/// behind a portrait came out pink.
///
/// Until 2026-09-23 that was hidden by a compression pass in
/// `raw_processing.rs` that desaturated everything above white. RapidRAW
/// removed it (85bf424a) and covered the gap with a post-demosaic colour
/// correction (40cfa3df). Argentum took the removal and declined the
/// correction, because it would stack on this module's recovery — and from
/// then on nothing did the job. This is the job, done where the evidence is:
/// in the CFA before demosaic, where "every photosite of this block is at its
/// ceiling" is a measurement rather than a guess made from a colour.
///
/// TWO CASES, AND NEITHER LEAVES A CAST
///
/// `rebuilt` says whether `recover` was in charge of this photo.
///
/// - It was: a block with every channel gone is set neutral at the brightest
///   level any of its photosites recorded, so a blown core is never darker
///   than the reconstructed edge around it. Blocks recovery rebuilt keep their
///   brightness and fade from their reconstructed colour, just over the
///   clipping point, to neutral half a stop above it. Recovery recovers light;
///   past the clip, the colour it would give that light is a guess.
/// - It was not — switched off, or no highlight colour to learn from: this is
///   plain clipping, which is what every RAW developer does without
///   reconstruction. In each block with a clipped channel, every photosite is
///   capped where the first channel stops being data. The clipped area goes
///   flat white instead of coloured, and nothing outside it changes.
///
/// Neutral means neutral *after white balance*, the only place it matters.
/// rawler multiplies each channel by its coefficient and then applies a matrix
/// whose rows sum to one, so equal white-balanced values come out as equal RGB.
/// `wb` has to be the coefficients rawler will actually develop with.
///
/// A block with nothing clipped is never touched, and a photo with nothing
/// clipped keeps its integer buffer exactly as decoded.
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
/// factor: half a stop.
///
/// It was the level where the *last* colour clips, more than a stop higher on
/// daylight white balance. That kept a measured colour in everything rebuilt
/// below there, and on a window behind a portrait — red and blue real, only
/// green rebuilt — that colour was the window's cool light. Next to a neutral
/// blown core it read as a lavender band as soon as the photo was darkened.
///
/// The fade is there only to meet unclipped neighbours without drawing a line.
/// Measured on that frame, fades ending at 1.1x, 1.25x and 1.5x all removed the
/// lavender, and none brought the outline back; half a stop sits between the
/// two longer ones.
const FADE_SPAN: f32 = std::f32::consts::SQRT_2;

impl Settle {
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

    /// One block's photosites, with the colour each one holds. Returns how many
    /// values moved.
    ///
    /// Blocks are passed in whole because a photosite carries one colour, and
    /// "this is blown" is only a statement about a block that carries all three.
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
            // judgement `recover_mosaic` makes with its two greens. Asking for
            // all of them would leave the blocks recovery gave up on — red,
            // blue and one green at the ceiling — magenta at the edge of
            // every blown area.
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
            // colour it was given just over the clipping point, where it meets
            // unclipped neighbours that really are that colour, and is neutral
            // by half a stop above it (`FADE_SPAN`). A hard edge either way
            // draws a line along every clipping contour: recovery's colour
            // beside a white core was a cyan or pink outline round anything
            // seen through a blown window.
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

/// 0 at or below `edge0`, 1 at or above `edge1`, smooth in between.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge1 <= edge0 {
        return if x >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

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

#[cfg(test)]
mod settle_tests {
    use super::*;

    const BLACK: f32 = 100.0;
    const CEILINGS: [f32; 3] = [1000.0, 1000.0, 1000.0];
    /// Daylight-ish: red and blue need two and one and a half times green.
    const WB: [f32; 4] = [2.0, 1.0, 1.5, f32::NAN];
    /// R G / G B, as (colour of each photosite in a 2x2 block).
    const RGGB: [usize; 4] = [0, 1, 1, 2];

    fn settle() -> Settle {
        Settle::new(CEILINGS, BLACK, WB).expect("valid levels")
    }

    /// What the block becomes after rawler's white balance, one value per
    /// photosite.
    fn balanced(values: &[f32], colours: &[usize]) -> Vec<f32> {
        let wb = [WB[0], WB[1], WB[2]];
        values
            .iter()
            .zip(colours)
            .map(|(v, &c)| (v - BLACK) * wb[c])
            .collect()
    }

    fn assert_neutral(values: &[f32], colours: &[usize]) {
        let b = balanced(values, colours);
        let (lo, hi) = b.iter().fold((f32::INFINITY, 0.0f32), |(lo, hi), &x| {
            (lo.min(x), hi.max(x))
        });
        assert!(
            (hi - lo) / hi < 1e-5,
            "not neutral after white balance: {b:?}"
        );
    }

    /// The bug: every photosite at its ceiling, white balance makes it pink.
    /// Both with and without recovery it has to come out neutral.
    #[test]
    fn a_blown_block_comes_out_neutral_either_way() {
        for rebuilt in [true, false] {
            let mut block = [1000.0, 1000.0, 1000.0, 1000.0];
            assert!(settle().block(&mut block, &RGGB, rebuilt) > 0);
            assert_neutral(&block, &RGGB);
        }
    }

    /// Red, blue and one green at the ceiling, the other green just short of
    /// it. Recovery counts the green as gone and gives up on the block, so it
    /// has to count as blown here too, or it stays magenta.
    #[test]
    fn one_green_just_short_of_the_ceiling_still_counts_as_blown() {
        let mut block = [1000.0, 1000.0, 995.0, 1000.0];
        assert!(settle().block(&mut block, &RGGB, true) > 0);
        assert_neutral(&block, &RGGB);
    }

    /// Recovery's reconstructed edge can sit well above the ceiling. The blown
    /// core next to it must not be the darker of the two, or a highlight gets a
    /// grey hole in the middle.
    #[test]
    fn with_recovery_a_blown_core_is_as_bright_as_its_brightest_channel() {
        let mut block = [1000.0, 1000.0, 1000.0, 1000.0];
        settle().block(&mut block, &RGGB, true);
        let level = balanced(&block, &RGGB)[0];
        assert!((level - 900.0 * 2.0).abs() < 1e-3, "level {level}");
    }

    /// Without recovery it is plain clipping: capped where green, the first
    /// channel to stop, stops.
    #[test]
    fn without_recovery_clipping_lands_on_the_first_channel_to_stop() {
        let mut block = [1000.0, 1000.0, 1000.0, 1000.0];
        settle().block(&mut block, &RGGB, false);
        let level = balanced(&block, &RGGB)[0];
        assert!((level - 900.0).abs() < 1e-3, "level {level}");
    }

    /// The guarantee every photo without blown highlights rests on.
    #[test]
    fn a_block_with_nothing_clipped_is_untouched() {
        for rebuilt in [true, false] {
            let before = [900.0, 600.0, 650.0, 400.0];
            let mut block = before;
            assert_eq!(settle().block(&mut block, &RGGB, rebuilt), 0);
            assert_eq!(block, before);
        }
    }

    /// With recovery in charge, a block only just over the clipping point keeps
    /// the colour recovery gave it: it sits beside unclipped pixels that really
    /// are that colour.
    #[test]
    fn with_recovery_a_block_just_over_the_clip_keeps_its_colour() {
        // Green rebuilt a little past its ceiling: balanced 910, where the
        // first colour clips at 900 and the last at 1800.
        let before = [500.0, 1010.0, 1010.0, 650.0];
        let mut block = before;
        settle().block(&mut block, &RGGB, true);
        for (a, b) in block.iter().zip(before) {
            assert!((a - b).abs() < 1.0, "moved: {block:?}");
        }
    }

    /// And one nearly as bright as a blown block looks nearly like one, so the
    /// two do not meet in a line.
    #[test]
    fn with_recovery_a_block_near_full_clipping_is_nearly_neutral() {
        let mut block = [990.0, 1400.0, 1400.0, 900.0];
        settle().block(&mut block, &RGGB, true);
        let b = balanced(&block, &RGGB);
        let (lo, hi) = b.iter().fold((f32::INFINITY, 0.0f32), |(lo, hi), &x| {
            (lo.min(x), hi.max(x))
        });
        assert!((hi - lo) / hi < 0.01, "still coloured: {b:?}");
    }

    /// The lavender window: green rebuilt, red and blue real, blue the stronger
    /// of the two because the light was cool. Half a stop past the first clip
    /// it is neutral, so a darkened window is grey-white rather than tinted
    /// beside its white core.
    #[test]
    fn with_recovery_half_a_stop_past_the_clip_is_neutral() {
        // Balanced: red 1220, green 1300 (rebuilt), blue 1347. The first clip
        // is at 900, so the brightest is half a stop past it and more.
        let mut block = [710.0, 1400.0, 1400.0, 998.0];
        settle().block(&mut block, &RGGB, true);
        let b = balanced(&block, &RGGB);
        let (lo, hi) = b.iter().fold((f32::INFINITY, 0.0f32), |(lo, hi), &x| {
            (lo.min(x), hi.max(x))
        });
        assert!((hi - lo) / hi < 1e-4, "still tinted: {b:?}");
    }

    #[test]
    fn smoothstep_has_flat_ends() {
        assert_eq!(smoothstep(1.0, 2.0, 0.5), 0.0);
        assert_eq!(smoothstep(1.0, 2.0, 2.5), 1.0);
        assert!((smoothstep(1.0, 2.0, 1.5) - 0.5).abs() < 1e-6);
    }

    /// Without recovery, a green that clipped first while red and blue did not
    /// is exactly the magenta case. Capping brings red down to where green
    /// stopped, and a dim blue that is real data stays where it is.
    #[test]
    fn without_recovery_a_partly_clipped_block_is_capped_not_coloured() {
        let mut block = [990.0, 1000.0, 1000.0, 300.0];
        settle().block(&mut block, &RGGB, false);
        let b = balanced(&block, &RGGB);
        assert!((b[0] - 900.0).abs() < 1e-3, "red {b:?}");
        assert!((b[1] - 900.0).abs() < 1e-3, "green {b:?}");
        assert_eq!(block[3], 300.0, "an unclipped dim channel must not move");
    }

    /// Capping only ever lowers; it never invents light.
    #[test]
    fn clipping_never_raises_a_value() {
        let before = [1000.0, 1000.0, 1000.0, 120.0];
        let mut block = before;
        settle().block(&mut block, &RGGB, false);
        for (a, b) in block.iter().zip(before) {
            assert!(*a <= b, "{block:?}");
        }
    }

    /// No coefficients is rawler's "no white balance", not a reason to fail.
    #[test]
    fn missing_white_balance_means_unity() {
        let s = Settle::new(CEILINGS, BLACK, [f32::NAN; 4]).expect("valid");
        assert_eq!(s.wb, [1.0, 1.0, 1.0]);
        assert!(Settle::new(CEILINGS, BLACK, [2.0, 0.0, 1.5, 1.0]).is_none());
    }

    /// A block without all three colours cannot be judged blown.
    #[test]
    fn a_block_missing_a_colour_is_left_alone() {
        let before = [1000.0, 1000.0, 1000.0, 1000.0];
        let mut block = before;
        assert_eq!(settle().block(&mut block, &[0, 1, 1, 1], false), 0);
        assert_eq!(block, before);
    }
}

/// The pink-highlight bug, checked the way the app meets it.
///
/// Through `image_loader::load_base_image_from_bytes`, the entry point the
/// editor and the thumbnails use, in both quality paths, and with recovery
/// both on and off. The first attempt at this fix was proven on a harness that
/// always had recovery on, while the photographer had it switched off, and in
/// the app it changed nothing.
#[cfg(test)]
mod end_to_end {
    /// Share of bright pixels that are magenta: red and blue both clearly
    /// above green. A blown window behind a portrait was nearly all of them.
    fn pink_share(rgb: &image::Rgb32FImage) -> f64 {
        let (mut bright, mut pink) = (0u64, 0u64);
        for p in rgb.pixels() {
            let [r, g, b] = p.0;
            let max = r.max(g).max(b);
            if max < 0.9 {
                continue;
            }
            bright += 1;
            if r.min(b) - g > 0.15 * max {
                pink += 1;
            }
        }
        pink as f64 / bright.max(1) as f64
    }

    /// What the screen shows: the app's own encode, `stops` of exposure off.
    fn on_screen(scene: &image::Rgb32FImage, stops: f32) -> image::DynamicImage {
        let gain = 2f32.powf(stops);
        let mut shown = scene.clone();
        shown
            .pixels_mut()
            .for_each(|p| p.0.iter_mut().for_each(|v| *v *= gain));
        let mut shown = image::DynamicImage::ImageRgb32F(shown);
        crate::mods::preview_encode::apply(&mut shown);
        shown
    }

    /// Checked three ways: in the scene-linear data, which is what an exposure
    /// or highlights slider later works on; on screen as decoded; and on screen
    /// one stop down, which is the first thing anyone does to a blown window.
    #[test]
    #[ignore = "reads AK's photos; run by hand"]
    fn blown_highlights_are_not_pink_where_the_app_decodes() {
        let path = std::env::var("AG_RAW").expect("set AG_RAW");
        let out_dir = std::env::var("AG_OUT_DIR").ok();
        let bytes = std::fs::read(&path).expect("read");
        let settings = crate::app_settings::AppSettings::default();
        let stem = std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        let mut failures = Vec::new();
        for recovery in [false, true] {
            super::set_enabled(recovery);
            for fast in [false, true] {
                let image = crate::image_loader::load_base_image_from_bytes(
                    &bytes, &path, fast, &settings, None,
                )
                .expect("decode");
                let scene = image.to_rgb32f();
                let in_data = pink_share(&scene);
                let shown = on_screen(&scene, 0.0);
                let seen = pink_share(&shown.to_rgb32f());
                let seen_down = pink_share(&on_screen(&scene, -1.0).to_rgb32f());
                println!(
                    "\nrecovery {:<5} fast {:<5}  pink in data {:.3}%  on screen {:.3}%  one stop down {:.3}%",
                    recovery,
                    fast,
                    in_data * 100.0,
                    seen * 100.0,
                    seen_down * 100.0
                );

                let worst = in_data.max(seen).max(seen_down);
                if worst >= 0.01 {
                    failures.push(format!(
                        "recovery {recovery} fast {fast}: {:.2}% magenta",
                        worst * 100.0
                    ));
                }

                if let Some(dir) = &out_dir {
                    let width = 1000;
                    let height = shown.height() * width / shown.width();
                    let name = format!(
                        "{stem}__recovery-{}__{}.jpg",
                        if recovery { "on" } else { "off" },
                        if fast { "fast" } else { "full" }
                    );
                    shown
                        .resize_exact(width, height, image::imageops::FilterType::Triangle)
                        .to_rgb8()
                        .save(std::path::Path::new(dir).join(name))
                        .expect("save");
                }
            }
        }
        super::set_enabled(true);

        assert!(failures.is_empty(), "still magenta: {failures:?}");
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
