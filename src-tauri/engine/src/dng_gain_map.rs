//! DNG gain maps: the lens shading a phone records in its DNG.
//!
//! Phones (Pixel, Galaxy and others) do not correct the darkening and colour
//! cast toward the corners before they write a DNG. They record it instead,
//! as GainMap opcodes in OpcodeList2, one map per colour of the mosaic, and
//! leave it to the renderer: Lightroom applies them. rawler copies the list
//! through for its DNG writer and applies none of it, so without this a
//! phone DNG keeps dark, tinted corners (gains reach about 6 at the edge).
//!
//! FROM RAWMAKASE
//!
//! `crates/rawmakase-raw/src/container/dng/opcodes.rs` @ a8c5d6d7 (v0.4.0,
//! MIT; DNG Specification 1.7.1.0, chapter 7): the list's layout, the
//! GainMap's parameters, which maps are skipped, and the bilinear gain
//! between its points. Only GainMap is taken, and only from OpcodeList2: it
//! is what phones write. The file's other opcodes (bad pixels, tables,
//! polynomials, per-line deltas) and OpcodeList1 wait for a file that needs
//! them; each would be one more arm in `parse` and one more function here.
//!
//! WHAT CHANGED IN THE COPY
//!
//! RAWmakase maps the samples to 16-bit linear reference values and applies
//! the list there. Here the gain goes onto the samples as rawler decoded
//! them, around each one's black: `black + (sample - black) * gain`, clipped
//! at the white level as the specification clips at 1.0. Once the develop
//! subtracts the black and divides by the range, that is the same value.
//! Positions are the active area's, as OpcodeList2's are.

use rawler::rawimage::{RawImage, RawImageData};
use rawler::tags::DngTag;
use rayon::prelude::*;

use crate::dng_opcodes::{self, Params};

/// The latest DNG version whose opcodes are understood (1.7.1.0).
const VERSION: u32 = 0x0107_0100;
/// The GainMap opcode's ID.
const GAIN_MAP: u32 = 9;
/// Most opcodes a list may declare. A phone's list is a short list of its
/// GainMaps and little else; the bound is far above that.
const MAX_OPCODES: u32 = 64;
/// Most GainMaps applied from one list. A phone writes four, one per colour
/// of its mosaic, and each map is a pass over the whole image.
const MAX_GAIN_MAPS: usize = 16;
/// Most points a map has along each axis. A real lens-shading grid is small;
/// the bound is far beyond any that a camera writes.
const MAX_GAIN_POINTS: usize = 256;
/// Most planes a map has: one per colour of a four-colour mosaic.
const MAX_GAIN_PLANES: usize = 4;

/// One GainMap: the part of the image it applies to (bottom and right
/// exclusive, every `pitch` rows and columns), and `points` (vertical,
/// horizontal) gains starting at `origin`, `spacing` apart, in fractions of
/// the image's height and width.
#[derive(Debug, Clone, PartialEq)]
struct GainMap {
    top: usize,
    left: usize,
    bottom: usize,
    right: usize,
    plane: u32,
    pitch: [usize; 2],
    points: [usize; 2],
    spacing: [f64; 2],
    origin: [f64; 2],
    planes: usize,
    gains: Vec<f32>,
}

/// Applies the DNG's GainMap opcodes to its decoded mosaic, and says how
/// many. Nothing for a file that is not a DNG, has no OpcodeList2 or no gain
/// map, or a list that does not parse; nothing for an image of more than one
/// value a pixel (a linear DNG).
pub fn apply(raw: &mut RawImage, file: &[u8]) -> usize {
    if raw.cpp != 1 {
        return 0;
    }
    let Some(maps) = dng_opcodes::list(file, DngTag::OpcodeList2).and_then(|list| parse(&list))
    else {
        return 0;
    };
    if maps.is_empty() {
        return 0;
    }
    let (width, height) = (raw.width, raw.height);
    let active = raw
        .active_area
        .map_or([0, 0, width, height], |a| [a.p.x, a.p.y, a.d.w, a.d.h]);
    let white = raw.whitelevel.0.first().map_or(f32::MAX, |w| *w as f32);
    let pattern = &raw.blacklevel;
    let black: Vec<f32> = pattern.levels.iter().map(|v| v.as_f32()).collect();
    let (bw, bh) = (pattern.width.max(1), pattern.height.max(1));
    let black_at = |row: usize, col: usize| -> f32 {
        black
            .get((row % bh) * bw + col % bw)
            .or(black.first())
            .copied()
            .unwrap_or(0.0)
    };
    let image = Image {
        stride: width,
        active,
        white,
    };
    match &mut raw.data {
        RawImageData::Integer(data) => {
            for map in &maps {
                image.apply(
                    data,
                    map,
                    &black_at,
                    |v| v as f32,
                    |v| v.round().clamp(0.0, u16::MAX as f32) as u16,
                );
            }
        }
        RawImageData::Float(data) => {
            for map in &maps {
                image.apply(data, map, &black_at, |v| v, |v| v);
            }
        }
    }
    maps.len()
}

/// Where a list applies: the stored image's row length, its active area
/// (left, top, width, height) and its white level.
struct Image {
    stride: usize,
    active: [usize; 4],
    white: f32,
}

impl Image {
    fn apply<T: Copy + Send>(
        &self,
        data: &mut [T],
        map: &GainMap,
        black_at: &(impl Fn(usize, usize) -> f32 + Sync),
        read: impl Fn(T) -> f32 + Sync,
        write: impl Fn(f32) -> T + Sync,
    ) {
        // The image has one plane: a map for a later one changes nothing.
        if map.plane != 0 {
            return;
        }
        let [left, top, width, height] = self.active;
        let bottom = map.bottom.min(height);
        let right = map.right.min(width);
        if width == 0 || height == 0 || map.top >= bottom || map.left >= right {
            return;
        }
        let (w, h) = (width as f64, height as f64);
        data.par_chunks_mut(self.stride)
            .enumerate()
            .skip(top + map.top)
            .take(bottom - map.top)
            .filter(|(stored_row, _)| (stored_row - top - map.top).is_multiple_of(map.pitch[0]))
            .for_each(|(stored_row, line)| {
                let row = stored_row - top;
                for col in (map.left..right).step_by(map.pitch[1]) {
                    let Some(sample) = line.get_mut(left + col) else {
                        break;
                    };
                    let gain = map.gain(row as f64 / h, col as f64 / w);
                    let black = black_at(row, col);
                    let value = black + (read(*sample) - black) * gain;
                    *sample = write(value.min(self.white));
                }
            });
    }
}

impl GainMap {
    /// The gain at (`v`, `h`), fractions of the image's height and width:
    /// bilinear between the map's points, the edge points' outside them.
    fn gain(&self, v: f64, h: f64) -> f32 {
        let at = |x: f64, axis: usize| {
            let n = self.points[axis];
            let f = ((x - self.origin[axis]) / self.spacing[axis]).clamp(0.0, (n - 1) as f64);
            let i = (f.floor() as usize).min(n - 1);
            (i, (i + 1).min(n - 1), (f - i as f64) as f32)
        };
        let (r0, r1, tv) = at(v, 0);
        let (c0, c1, th) = at(h, 1);
        let g = |r: usize, c: usize| {
            self.gains
                .get((r * self.points[1] + c) * self.planes)
                .copied()
                .unwrap_or(1.0)
        };
        let top = g(r0, c0) + (g(r0, c1) - g(r0, c0)) * th;
        let bottom = g(r1, c0) + (g(r1, c1) - g(r1, c0)) * th;
        top + (bottom - top) * tv
    }
}

/// The list's gain maps, the opcodes skipped left out; `None` when the list
/// does not parse (it is then not applied at all).
fn parse(list: &[u8]) -> Option<Vec<GainMap>> {
    // The declared count is read first, so a list over the bound is refused
    // before any of its opcodes is read. An unreadable count is left to
    // `opcodes`, which refuses it.
    let declared = Params { data: list, at: 0 }.u32().unwrap_or(0);
    if declared > MAX_OPCODES {
        return None;
    }
    let mut out = Vec::new();
    for op in dng_opcodes::opcodes(list)? {
        if op.id == GAIN_MAP && op.version <= VERSION {
            out.extend(gain_map(op.params)?);
            if out.len() > MAX_GAIN_MAPS {
                return None;
            }
        }
    }
    Some(out)
}

/// A GainMap's parameters: `None` when they do not parse, `Some(None)` for a
/// map that changes nothing (every gain 1, as Samsung's phones write).
fn gain_map(data: &[u8]) -> Option<Option<GainMap>> {
    let mut p = Params { data, at: 0 };
    let (top, left, bottom, right) = (p.usize()?, p.usize()?, p.usize()?, p.usize()?);
    let (plane, _planes) = (p.u32()?, p.u32()?);
    let pitch = [p.usize()?, p.usize()?];
    let points = [p.usize()?, p.usize()?];
    let spacing = [p.f64()?, p.f64()?];
    let origin = [p.f64()?, p.f64()?];
    let planes = p.usize()?;
    // The bounds come before the sizes are multiplied, so no count can make
    // them overflow, and before any gain is read.
    if points.iter().any(|&n| n > MAX_GAIN_POINTS) || planes > MAX_GAIN_PLANES {
        return None;
    }
    let n = points[0]
        .checked_mul(points[1])?
        .checked_mul(planes)
        .filter(|&n| n > 0)?;
    // A count cannot ask for more than the parameters hold.
    if n.checked_mul(4)? > data.len().saturating_sub(p.at) {
        return None;
    }
    let gains = (0..n).map(|_| p.f32()).collect::<Option<Vec<f32>>>()?;
    let usable = |o: f64, s: f64| o.is_finite() && s.is_finite() && s > 0.0;
    if pitch.contains(&0) || !usable(origin[0], spacing[0]) || !usable(origin[1], spacing[1]) {
        return None;
    }
    if gains.iter().all(|&g| g == 1.0) {
        return Some(None);
    }
    Some(Some(GainMap {
        top,
        left,
        bottom,
        right,
        plane,
        pitch,
        points,
        spacing,
        origin,
        planes,
        gains,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An opcode list of `(id, parameters)`, version 1.3.
    fn list(ops: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let mut out = (ops.len() as u32).to_be_bytes().to_vec();
        for (id, params) in ops {
            out.extend(id.to_be_bytes());
            out.extend(0x0103_0000u32.to_be_bytes());
            out.extend(0u32.to_be_bytes());
            out.extend((params.len() as u32).to_be_bytes());
            out.extend(params);
        }
        out
    }
    fn longs(v: &[u32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_be_bytes()).collect()
    }
    fn doubles(v: &[f64]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_be_bytes()).collect()
    }
    fn floats(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_be_bytes()).collect()
    }
    /// A GainMap over `area` (top, left, bottom, right, pitch) with `points`
    /// (vertical, horizontal) gains from the origin to 1.0.
    fn gain_map_params(area: [u32; 5], points: [u32; 2], gains: &[f32]) -> Vec<u8> {
        let [t, l, b, r, pitch] = area;
        let mut p = longs(&[t, l, b, r, 0, 1, pitch, pitch, points[0], points[1]]);
        let spacing = |n: u32| if n > 1 { 1.0 / f64::from(n - 1) } else { 1.0 };
        p.extend(doubles(&[spacing(points[0]), spacing(points[1]), 0.0, 0.0]));
        p.extend(longs(&[1]));
        p.extend(floats(gains));
        p
    }

    fn run(list: &[u8], data: &mut [u16], width: usize, black: f32, white: f32) {
        let maps = parse(list).expect("list parses");
        let height = data.len() / width;
        let image = Image {
            stride: width,
            active: [0, 0, width, height],
            white,
        };
        for map in &maps {
            image.apply(
                data,
                map,
                &|_, _| black,
                |v| v as f32,
                |v| v.round().clamp(0.0, u16::MAX as f32) as u16,
            );
        }
    }

    /// RAWmakase's case: 2 × 2 points over a 3 × 3 image. Row 0 and column 0
    /// sit on the first points, the rest between them.
    #[test]
    fn a_gain_map_interpolates_between_its_points() {
        let l = list(&[(
            9,
            gain_map_params([0, 0, 3, 3, 1], [2, 2], &[1., 2., 3., 4.]),
        )]);
        let mut data = vec![1100u16; 9];
        run(&l, &mut data, 3, 100.0, 60000.0);
        for r in 0..3 {
            for c in 0..3 {
                // Gain = 1 + h + 2 v, around the black of 100.
                let gain = 1.0 + c as f32 / 3.0 + 2.0 * r as f32 / 3.0;
                let want = (100.0 + 1000.0 * gain).round() as u16;
                assert_eq!(data[r * 3 + c], want, "{r} {c}");
            }
        }
    }

    /// The pitch and bounds limit the samples a map touches, and a result
    /// clips at the white level.
    #[test]
    fn a_gain_map_keeps_to_its_area_and_clips_at_white() {
        let l = list(&[(9, gain_map_params([1, 0, 4, 4, 2], [1, 1], &[2.]))]);
        let mut data = vec![3000u16; 16];
        run(&l, &mut data, 4, 0.0, 4095.0);
        for r in 0..4 {
            for c in 0..4 {
                let want = if r % 2 == 1 && c % 2 == 0 { 4095 } else { 3000 };
                assert_eq!(data[r * 4 + c], want, "{r} {c}");
            }
        }
    }

    #[test]
    fn maps_that_change_nothing_and_other_opcodes_are_skipped() {
        let ones = gain_map_params([0, 0, 2, 2, 1], [2, 2], &[1.0; 4]);
        let vignette = doubles(&[0.1, 0.0, 0.0, 0.0, 0.0, 0.5, 0.5]);
        let l = list(&[(9, ones), (3, vignette)]);
        assert_eq!(parse(&l), Some(Vec::new()));
    }

    #[test]
    fn a_list_that_does_not_parse_is_not_applied() {
        let mut short = list(&[(9, gain_map_params([0, 0, 2, 2, 1], [2, 2], &[2.0; 4]))]);
        short.truncate(short.len() - 3);
        assert_eq!(parse(&short), None);
        // A count of gains larger than the parameters hold.
        let mut p = longs(&[0, 0, 2, 2, 0, 1, 1, 1, 1000, 1000]);
        p.extend(doubles(&[1.0, 1.0, 0.0, 0.0]));
        p.extend(longs(&[1]));
        assert_eq!(parse(&list(&[(9, p)])), None);
        // A zero pitch.
        let zero = gain_map_params([0, 0, 2, 2, 0], [1, 1], &[2.0]);
        assert_eq!(parse(&list(&[(9, zero)])), None);
    }

    /// A list that declares more opcodes than the bound is refused before its
    /// opcodes are read; one at the bound is still read.
    #[test]
    fn a_list_declaring_too_many_opcodes_is_not_applied() {
        let skipped = (3, doubles(&[0.1]));
        let too_many = vec![skipped.clone(); MAX_OPCODES as usize + 1];
        assert_eq!(parse(&list(&too_many)), None);
        let at_bound = vec![skipped; MAX_OPCODES as usize];
        assert_eq!(parse(&list(&at_bound)), Some(Vec::new()));
    }

    /// A list with more non-trivial maps than the bound is not applied; one
    /// at the bound is.
    #[test]
    fn a_list_with_too_many_gain_maps_is_not_applied() {
        let map = (9, gain_map_params([0, 0, 2, 2, 1], [2, 2], &[2.0; 4]));
        assert_eq!(parse(&list(&vec![map.clone(); MAX_GAIN_MAPS + 1])), None);
        let parsed = parse(&list(&vec![map; MAX_GAIN_MAPS])).expect("at the bound");
        assert_eq!(parsed.len(), MAX_GAIN_MAPS);
    }

    /// A map whose points or planes exceed their bounds is not applied.
    #[test]
    fn a_gain_map_with_too_many_points_or_planes_is_not_applied() {
        let width = MAX_GAIN_POINTS + 1;
        let wide = gain_map_params([0, 0, 2, 2, 1], [width as u32, 2], &vec![2.0; width * 2]);
        assert_eq!(parse(&list(&[(9, wide)])), None);
        let mut planes = longs(&[0, 0, 2, 2, 0, 1, 1, 1, 1, 1]);
        planes.extend(doubles(&[1.0, 1.0, 0.0, 0.0]));
        planes.extend(longs(&[MAX_GAIN_PLANES as u32 + 1]));
        planes.extend(floats(&[2.0; MAX_GAIN_PLANES + 1]));
        assert_eq!(parse(&list(&[(9, planes)])), None);
    }

    /// Points times planes overflowing is refused, not wrapped around.
    #[test]
    fn a_gain_map_whose_size_overflows_is_not_applied() {
        let mut p = longs(&[0, 0, 2, 2, 0, 1, 1, 1, u32::MAX, u32::MAX]);
        p.extend(doubles(&[1.0, 1.0, 0.0, 0.0]));
        p.extend(longs(&[u32::MAX]));
        assert_eq!(parse(&list(&[(9, p)])), None);
    }

    /// A real phone DNG (AG_RAW): how many maps apply, and the corners'
    /// level against the centre's before and after, per colour of the
    /// mosaic. Prints; asserts only that the corners come up toward the
    /// centre.
    #[test]
    #[ignore = "reads a local DNG named by AG_RAW; run by hand"]
    fn a_phone_dngs_corners_come_up() {
        let path = std::env::var("AG_RAW").expect("AG_RAW");
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
        let ratios = |raw: &RawImage| -> [f32; 4] {
            let data = raw.data.as_f32();
            let (w, h) = (raw.width, raw.height);
            let black = raw.blacklevel.levels.first().map_or(0.0, |v| v.as_f32());
            let box_mean = |x0: usize, y0: usize, phase: usize| {
                let (mut sum, mut n) = (0.0f64, 0usize);
                for y in (y0..y0 + 64).filter(|y| y % 2 == phase / 2) {
                    for x in (x0..x0 + 64).filter(|x| x % 2 == phase % 2) {
                        sum += f64::from(data[y * w + x] - black);
                        n += 1;
                    }
                }
                sum / n as f64
            };
            std::array::from_fn(|phase| {
                let centre = box_mean(w / 2 - 32, h / 2 - 32, phase);
                let corner = box_mean(16, 16, phase);
                (corner / centre) as f32
            })
        };
        let before = ratios(&raw);
        let applied = apply(&mut raw, &bytes);
        let after = ratios(&raw);
        eprintln!();
        eprintln!("{path}");
        eprintln!("  {applied} gain maps; corner / centre by mosaic position");
        eprintln!("  before {before:.3?}");
        eprintln!("  after  {after:.3?}");
        assert!(applied > 0, "no gain map applied");
        assert!(
            after.iter().zip(&before).all(|(a, b)| a > b),
            "corners came up"
        );
    }
}
