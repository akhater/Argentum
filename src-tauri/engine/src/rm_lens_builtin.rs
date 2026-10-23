//! A camera's built-in lens correction: the per-shot tables it stores in the
//! RAW for the lens on the camera, at the focal length and aperture used.
//!
//! Fujifilm writes distortion, lateral chromatic aberration and vignetting
//! into every RAF (FujiIFD tags 0xF00B, 0xF00F and 0xF010), and Lightroom
//! applies them by default as the "built-in lens profile". rawler reads the
//! FujiIFD for its own decode and none of these, so without this a Fujifilm
//! photo keeps the darker corners and the colour fringes the camera's JPEG
//! has already corrected.
//!
//! Sony writes the same three into an ARW's raw SubIFD (0x7032 vignetting,
//! 0x7035 CA, 0x7037 distortion). Lightroom has not been shown to apply
//! them, so they start off. Sony's tables carry the camera's own framing
//! (a ZV-1's distortion is 1.077 at the centre), so they are filled only as
//! far as the photo's edge needs, which keeps the camera's crop.
//!
//! A DNG records the corrections Lightroom applies as opcodes:
//! FixVignetteRadial (a gain over radius), WarpRectilinear (a radial remap
//! per colour plane) and, in OpcodeList3, GainMaps (a gain per colour on a
//! grid over the demosaiced image, which DJI and Hasselblad drones write
//! for their lens shading: up to 2.8 in the corners, different per colour).
//! Cameras that write DNGs (Leica, DJI's drones, phones) and Lightroom's own
//! DNGs carry them. On, as Lightroom applies them.
//!
//! FROM RAWMAKASE
//!
//! RAWmakase `v0.4.2` @ f0899e0 (PR #561, MIT), OpcodeList3's GainMaps:
//! - `crates/rawmakase-model/src/optics/mod.rs`: `GainGrid`, as written, and
//!   `LensCorrection::gain_map`.
//! - `crates/rawmakase-model/src/dng.rs`: `gain_grid` (which maps are read,
//!   each plane's gains, whether the list warps before them) and its test.
//! - `crates/rawmakase-engine/src/develop/pipeline/fields.rs`: `GainField`
//!   and `LensWarp::sample`: the map at the Vignetting amount, with the
//!   vignetting, each channel reading it where it samples, or at the
//!   corrected position for a map after the warp.
//!
//! RAWmakase `v0.4.0` @ a8c5d6d7 (MIT):
//! - `crates/rawmakase-model/src/optics/mod.rs`: `Radial`, `LensCorrection`,
//!   `radial_scale_with`, `fill_scale_with` and `fill_scale_within`, as
//!   written.
//! - `crates/rawmakase-model/src/lens/embedded.rs`: the Fujifilm and Sony
//!   readers, their units, the 0.85 Fujifilm vignetting strength, and their
//!   tests.
//! - `crates/rawmakase-model/src/dng.rs`: the FixVignetteRadial and
//!   WarpRectilinear readers (radial terms only, centred within 0.02,
//!   sampled at 33 radii), and their test.
//! - `crates/rawmakase-engine/src/develop/image_space.rs`: `LensMap`'s
//!   position map, its Newton inverse, and which fill each source takes
//!   (RAWmakase #533).
//! - `docs/lens-corrections.md`: the order (vignetting gain in linear light
//!   at the source pixel, then distortion and CA as one radial remap while
//!   sampling, scaled so corrected corners stay inside the sensor).
//!
//! WHAT CHANGED IN THE COPY
//!
//! - The tables are read with rawler's TIFF reader, as `raf.rs` reads the
//!   FujiIFD, in place of `rawmakase-tiff`, and from the file's bytes rather
//!   than its path.
//! - The amounts are the app's three Lens Correction sliders: distortion and
//!   vignetting as RAWmakase applies Lightroom's (the radial scale moved by
//!   the amount, the gain raised to it), and lateral CA moved the same way as
//!   distortion, where RAWmakase has no amount for it.
//! - `LensMap` works on offsets from the image centre in pixels, as the app's
//!   warp does, rather than on RAWmakase's image positions.
//! - Sony's fill keeps the developed image's edge inside it. rawler already
//!   crops an ARW to its DefaultCrop, which is the camera's aspect-ratio crop
//!   on a 16:9 ZV-1 shot, so the developed image is the frame RAWmakase's
//!   `camera_crop` describes; the sensor beyond it is not there to sample.
//! - FixVignetteRadial is read from OpcodeList2 and, when that has none,
//!   from OpcodeList3: DJI's drones write it there, where RAWmakase does not
//!   look. Both lists apply before the warp, and a gain is the same in
//!   either. The lists come through `dng_opcodes.rs`, shared with the gain
//!   maps.
//! - A DNG's `source` is "DNG", the word the switch shows, where RAWmakase
//!   writes "DNG opcodes".
//! - A DNG's radius is over its raw image; the developed image here is its
//!   DefaultCrop, a few pixels smaller on the files seen. The same holds for
//!   a gain map's positions, which are over the active area.
//! - A GainMap's parameters are read by `dng_gain_map.rs`, which reads
//!   OpcodeList2's, and `gain_grid` checks them as RAWmakase's does. A map
//!   of gains 1 is skipped there, before the checks RAWmakase makes of the
//!   maps together; it changes nothing either way. A list that does not
//!   parse is not read at all, where RAWmakase reads it up to that opcode.
//! - The map is read at offsets from the image centre, as the rest of
//!   `LensMap`, where RAWmakase reads it at pixel centres; in the app's warp
//!   the two are half a pixel apart.
//! - RAWmakase also measures the photo for its tone stage with the map
//!   applied (`pipeline::measurement_source`). Not taken: today's app's Auto
//!   measures the photo as decoded, before any lens correction, and this
//!   leaves it so.

use std::io::Cursor;

use rawler::formats::tiff::IFD;
use rawler::tags::DngTag;
use serde::{Deserialize, Serialize};

/// FujiIFD, in the TIFF a RAF's header points to.
const FUJI_IFD: u16 = 0xf000;
const FUJI_DISTORTION: u16 = 0xf00b;
const FUJI_CHROMATIC: u16 = 0xf00f;
const FUJI_VIGNETTING: u16 = 0xf010;
/// Where a RAF's header holds the offset of that TIFF, big-endian.
const RAF_TIFF_POINTER: usize = 100;
/// SubIFDs: an ARW's raw image directory is the first.
const SUB_IFDS: u16 = 0x14a;
const SONY_VIGNETTING: u16 = 0x7032;
const SONY_CHROMATIC: u16 = 0x7035;
const SONY_DISTORTION: u16 = 0x7037;

/// Lightroom renders Fujifilm's vignetting table about 15% weaker in log
/// gain: corners of three X100F photos match Camera Raw at this exponent
/// (+0.08 EV at the corner at full strength). RAWmakase's measurement.
const FUJI_VIGNETTE_STRENGTH: f32 = 0.85;

/// DNG opcode IDs (DNG Specification 1.7.1.0, chapter 7).
const WARP_RECTILINEAR: u32 = 1;
const FIX_VIGNETTE_RADIAL: u32 = 3;

/// A radial function sampled at increasing radii. Radius 0 is the image
/// centre and 1 is half of the image diagonal. Values are linearly
/// interpolated and held constant beyond the outermost knots.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Radial {
    pub knots: Vec<f32>,
    pub values: Vec<f32>,
}

impl Radial {
    fn new(knots: Vec<f32>, values: Vec<f32>) -> Option<Self> {
        let r = Self { knots, values };
        r.valid().then_some(r)
    }

    fn valid(&self) -> bool {
        self.knots.len() >= 2
            && self.knots.len() == self.values.len()
            && self.knots.len() <= 64
            && self.knots.iter().chain(&self.values).all(|v| v.is_finite())
            && self.knots.windows(2).all(|k| k[0] < k[1])
            && self.knots[0] >= 0.
            && self.values.iter().all(|v| (0.2..=5.).contains(v))
    }

    pub fn eval(&self, r: f32) -> f32 {
        let k = &self.knots;
        let i = k.partition_point(|x| *x <= r);
        if i == 0 {
            return self.values[0];
        }
        if i == k.len() {
            return self.values[k.len() - 1];
        }
        let t = (r - k[i - 1]) / (k[i] - k[i - 1]);
        self.values[i - 1] + (self.values[i] - self.values[i - 1]) * t
    }
}

/// A gain per colour channel sampled on a grid over part of the image: a
/// DNG GainMap opcode of OpcodeList3, which applies to the demosaiced image
/// (DJI and Hasselblad drones' and the Sigma fp's lens shading). It covers
/// `area` (top, left, bottom, right in fractions of the image's height and
/// width; no gain outside it). Points are `points` = [rows, columns],
/// starting at `origin` and `spacing` apart, in fractions of the image's
/// height and width, as the DNG SDK reads them; each holds `planes` gains,
/// one per channel (red, green, blue) or one for all three. Between points
/// the gain is bilinear, beyond the outermost ones that of the nearest.
/// `after_warp`: the opcode list warps the image before it, so it applies at
/// the corrected position rather than the source's.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GainGrid {
    #[serde(default = "GainGrid::whole")]
    pub area: [f32; 4],
    #[serde(default)]
    pub after_warp: bool,
    pub points: [usize; 2],
    pub origin: [f32; 2],
    pub spacing: [f32; 2],
    pub planes: usize,
    pub gains: Vec<f32>,
}
impl Default for GainGrid {
    fn default() -> Self {
        Self {
            area: Self::whole(),
            after_warp: false,
            points: [1, 1],
            origin: [0.; 2],
            spacing: [1.; 2],
            planes: 1,
            gains: vec![1.],
        }
    }
}
impl GainGrid {
    fn whole() -> [f32; 4] {
        [0., 0., 1., 1.]
    }
    fn valid(&self) -> bool {
        let [rows, columns] = self.points;
        let [top, left, bottom, right] = self.area;
        (0. ..1.).contains(&top)
            && (0. ..1.).contains(&left)
            && top < bottom
            && left < right
            && bottom <= 1.
            && right <= 1.
            && (1..=256).contains(&rows)
            && (1..=256).contains(&columns)
            && matches!(self.planes, 1 | 3)
            && self.gains.len() == rows * columns * self.planes
            && self.origin.iter().all(|v| v.is_finite())
            && self.spacing.iter().all(|v| v.is_finite() && *v > 0.)
            && self.gains.iter().all(|g| (0.2..=8.).contains(g))
    }
    /// The gain of each channel at `v`, `h`: fractions of the image's height and width.
    pub fn gain(&self, v: f32, h: f32) -> [f32; 3] {
        let [top, left, bottom, right] = self.area;
        if !(top..bottom).contains(&v) || !(left..right).contains(&h) {
            return [1.; 3];
        }
        let at = |x: f32, axis: usize| {
            let n = self.points[axis];
            let f = ((x - self.origin[axis]) / self.spacing[axis]).clamp(0., (n - 1) as f32);
            let i = (f as usize).min(n.saturating_sub(2));
            (i, (i + 1).min(n - 1), f - i as f32)
        };
        let (r0, r1, tv) = at(v, 0);
        let (c0, c1, th) = at(h, 1);
        let g =
            |r: usize, c: usize, p: usize| self.gains[(r * self.points[1] + c) * self.planes + p];
        std::array::from_fn(|c| {
            let p = c.min(self.planes - 1);
            let top = g(r0, c0, p) + (g(r0, c1, p) - g(r0, c0, p)) * th;
            let bottom = g(r1, c0, p) + (g(r1, c1, p) - g(r1, c0, p)) * th;
            top + (bottom - top) * tv
        })
    }
}

/// Built-in lens correction for one photo.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LensCorrection {
    /// Where the data came from, for display, e.g. "Fujifilm built-in".
    pub source: String,
    /// Lightroom applies this correction by default. True where verified
    /// against Adobe's own rendering.
    #[serde(default)]
    pub default_on: bool,
    /// Gain that restores illumination at each radius (1 = none).
    pub vignetting: Option<Radial>,
    /// Source radius divided by output radius for the green channel (1 = none).
    pub distortion: Option<Radial>,
    /// Additional source radius scale for red and blue relative to green (1 = none).
    pub chromatic: Option<[Radial; 2]>,
    /// Gain per channel over the image, before distortion, as a DNG's OpcodeList3
    /// GainMap applies it (1 = none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain_map: Option<GainGrid>,
}

/// How much of each part to apply, 1 = all of it: the app's Lens Correction
/// amounts (0 to 200%) over 100. An amount of 0 turns that part off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Amounts {
    pub distortion: f32,
    pub chromatic: f32,
    pub vignetting: f32,
}

impl Amounts {
    pub const FULL: Amounts = Amounts {
        distortion: 1.,
        chromatic: 1.,
        vignetting: 1.,
    };
}

impl LensCorrection {
    /// The `source` of the correction read from a Sony file's own tables.
    pub const SONY_BUILTIN: &'static str = "Sony built-in";

    pub fn is_empty(&self) -> bool {
        self.vignetting.is_none()
            && self.distortion.is_none()
            && self.chromatic.is_none()
            && self.gain_map.is_none()
    }

    pub fn validate(&self) -> bool {
        self.vignetting.iter().all(Radial::valid)
            && self.distortion.iter().all(Radial::valid)
            && self.chromatic.iter().flatten().all(Radial::valid)
            && self.gain_map.iter().all(GainGrid::valid)
    }

    /// Source radius scale for red, green and blue at output radius `r`.
    pub fn radial_scale_with(&self, r: f32, amounts: Amounts) -> [f32; 3] {
        let g = self
            .distortion
            .as_ref()
            .map_or(1., |d| 1. + (d.eval(r) - 1.) * amounts.distortion);
        let ca = |c: &Radial| 1. + (c.eval(r) - 1.) * amounts.chromatic;
        match &self.chromatic {
            Some([red, blue]) => [g * ca(red), g, g * ca(blue)],
            None => [g; 3],
        }
    }

    /// Output radii are scaled by this factor so every corrected corner
    /// samples inside the sensor, as Lightroom crops the undefined border
    /// after distortion correction.
    pub fn fill_scale_with(&self, amounts: Amounts) -> f32 {
        if self.distortion.is_none() && self.chromatic.is_none() {
            return 1.;
        }
        let widest = (0..=64)
            .flat_map(|i| self.radial_scale_with(i as f32 / 64., amounts))
            .fold(1f32, f32::max);
        1. / widest
    }

    /// As `fill_scale_with`, for a photo that is only the rectangle `crop`
    /// (`[left, top, right, bottom]`, in pixels) of the decoded image of
    /// `size`: the scale keeps every corrected sample of the crop's edge inside
    /// the decoded image, which can reach beyond the crop (a camera's
    /// aspect-ratio crop of a wider sensor). Never below the scale for the
    /// whole image.
    pub fn fill_scale_within(&self, amounts: Amounts, size: [f32; 2], crop: [f32; 4]) -> f32 {
        let whole = self.fill_scale_with(amounts);
        if whole >= 1. {
            return whole;
        }
        let [w, h] = size;
        let half = w.hypot(h) * 0.5;
        let [left, top, right, bottom] = crop.map(|v| v.max(0.));
        let (x0, x1) = (left - w * 0.5, right.min(w) - w * 0.5);
        let (y0, y1) = (top - h * 0.5, bottom.min(h) - h * 0.5);
        let inside = |dx: f32, dy: f32, fill: f32| {
            let s = self
                .radial_scale_with(dx.hypot(dy) * fill / half, amounts)
                .into_iter()
                .fold(0f32, f32::max);
            dx.abs() * fill * s <= w * 0.5 && dy.abs() * fill * s <= h * 0.5
        };
        // About one sample per pixel along the edge, so no extremum between
        // samples leaves the image by more than a fraction of a pixel.
        let steps = (w.max(h).ceil() as usize).clamp(64, 8192);
        let mut fill = 1f32;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let (x, y) = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
            for (dx, dy) in [(x, y0), (x, y1), (x0, y), (x1, y)] {
                if inside(dx, dy, fill) {
                    continue;
                }
                let (mut lo, mut hi) = (whole, fill);
                for _ in 0..24 {
                    let mid = (lo + hi) * 0.5;
                    if inside(dx, dy, mid) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                fill = lo;
            }
        }
        fill
    }

    /// The gain that restores the light at radius `r` of the decoded image.
    pub fn vignetting_gain(&self, r: f32, amount: f32) -> f32 {
        match &self.vignetting {
            None => 1.,
            Some(v) if amount == 1. => v.eval(r),
            Some(v) => v.eval(r).powf(amount),
        }
    }
}

/// The correction as a map of positions over one image: from an output
/// position to where each channel samples the image, and back. Positions are
/// offsets from the image centre in pixels.
#[derive(Clone, Copy, Debug)]
pub struct LensMap<'a> {
    pub lens: &'a LensCorrection,
    pub amounts: Amounts,
    /// The image's width and height, in pixels.
    pub size_px: [f32; 2],
    /// Half the image diagonal, in pixels: radius 1.
    pub half: f32,
    /// The scale that keeps corrected samples inside the image.
    pub fill: f32,
}

impl<'a> LensMap<'a> {
    pub fn new(lens: &'a LensCorrection, width: f32, height: f32, amounts: Amounts) -> Self {
        Self {
            lens,
            amounts,
            size_px: [width, height],
            half: width.hypot(height) * 0.5,
            fill: Self::fill(lens, width, height, amounts),
        }
    }

    /// The scale that keeps corrected samples inside the image. Sony's
    /// built-in correction is the camera's own, which zooms by no more than
    /// its table does, so only the photo's edge has to stay inside. Any other
    /// correction keeps the whole frame's corners inside, as Lightroom's
    /// profiles do.
    fn fill(lens: &LensCorrection, width: f32, height: f32, amounts: Amounts) -> f32 {
        if lens.source == LensCorrection::SONY_BUILTIN {
            lens.fill_scale_within(amounts, [width, height], [0., 0., width, height])
        } else {
            lens.fill_scale_with(amounts)
        }
    }

    /// Output offset (`dx`, `dy`) scaled by the fill, and the per-channel
    /// radial scale there: red, green and blue sample at the offset times
    /// their scale.
    pub fn scales(&self, dx: f32, dy: f32) -> ([f32; 2], [f32; 3]) {
        let (dx, dy) = (dx * self.fill, dy * self.fill);
        let r = dx.hypot(dy) / self.half;
        ([dx, dy], self.lens.radial_scale_with(r, self.amounts))
    }

    /// Where green samples for output offset (`dx`, `dy`).
    pub fn forward(&self, dx: f32, dy: f32) -> [f32; 2] {
        let ([dx, dy], scale) = self.scales(dx, dy);
        [dx * scale[1], dy * scale[1]]
    }

    /// The gain of each channel for the image pixel green samples at offset
    /// `green`, red and blue sampling at `ca` (their scale against green's)
    /// times it, for output offset `out` at the fill's scale (`scales`' first
    /// value). The vignetting's gain is green's pixel's. A DNG's gain map
    /// goes on at the Vignetting amount: before the warp each channel reads
    /// it where that channel samples, after it at `out`, the corrected
    /// position (RAWmakase's `LensWarp::sample`).
    pub fn gains(&self, out: [f32; 2], green: [f32; 2], ca: [f32; 2]) -> [f32; 3] {
        let [dx, dy] = green;
        let amount = self.amounts.vignetting;
        let vignetting = self.lens.vignetting_gain(dx.hypot(dy) / self.half, amount);
        let Some(grid) = self.lens.gain_map.as_ref().filter(|_| amount != 0.) else {
            return [vignetting; 3];
        };
        let [w, h] = self.size_px;
        let at = |[x, y]: [f32; 2]| grid.gain(0.5 + y / h, 0.5 + x / w);
        let map = if grid.after_warp {
            at(out)
        } else if ca == [1., 1.] {
            at(green)
        } else {
            let [r, b] = ca;
            [
                at([dx * r, dy * r])[0],
                at(green)[1],
                at([dx * b, dy * b])[2],
            ]
        };
        map.map(|g| {
            let g = if amount == 1. {
                g
            } else {
                g.max(0.).powf(amount)
            };
            vignetting * g
        })
    }

    /// The output offset whose green samples image offset (`dx`, `dy`): the
    /// radius is found with a few Newton steps, the direction is kept.
    pub fn inverse(&self, dx: f32, dy: f32) -> [f32; 2] {
        let target = dx.hypot(dy);
        if target < 1e-6 {
            return [dx, dy];
        }
        let f = |rho: f32| rho * self.lens.radial_scale_with(rho / self.half, self.amounts)[1];
        let mut rho = target;
        for _ in 0..8 {
            let h = self.half * 1e-3;
            let slope = (f(rho + h) - f(rho - h)) / (2. * h);
            if slope.abs() < 1e-6 {
                break;
            }
            let step = (f(rho) - target) / slope;
            rho = (rho - step).max(0.);
            if step.abs() < 1e-4 {
                break;
            }
        }
        let k = rho / target / self.fill;
        [dx * k, dy * k]
    }
}

/// The built-in correction a RAW carries. `None` for a file of another
/// format, one that stored no correction, or tables that do not parse.
pub fn read(file: &[u8]) -> Option<LensCorrection> {
    let c = if file.starts_with(b"FUJIFILMCCD-RAW") {
        fuji(file)
    } else if file.starts_with(b"II*\0") || file.starts_with(b"MM\0*") {
        dng(file).or_else(|| sony(file))
    } else {
        None
    }?;
    (!c.is_empty() && c.validate()).then_some(c)
}

/// Fujifilm's tables are signed rationals. The first value is a pixel scale;
/// knots are fractions of the half diagonal, distortion is in percent, CA a
/// radius fraction, vignetting the remaining illumination in percent.
fn fuji(file: &[u8]) -> Option<LensCorrection> {
    let pointer = file.get(RAF_TIFF_POINTER..RAF_TIFF_POINTER + 4)?;
    let base = u32::from_be_bytes(pointer.try_into().ok()?);
    let mut reader = Cursor::new(file);
    let root = IFD::new_root_with_correction(&mut reader, 0, base, 0, 1, &[]).ok()?;
    // Fujifilm writes the FujiIFD pointer as TIFF type 13 (IFD), which rawler
    // keeps as a single byte. The offset is the entry's four bytes.
    let at = base as usize + root.get_entry(FUJI_IFD)?.offset()?;
    file.get(at..at + 4)?;
    let offset = root.endian.read_u32(file, at);
    let fuji = IFD::new(&mut reader, offset, base, 0, root.endian, &[]).ok()?;
    let numbers = |tag: u16| -> Option<Vec<f32>> {
        let v = &fuji.get_entry(tag)?.value;
        (0..v.count())
            .map(|i| v.get_f32(i).ok().flatten())
            .collect()
    };
    // [scale, n knots, n values]
    let pairs = |v: &[f32], f: &dyn Fn(f32) -> f32| {
        let n = v.len().checked_sub(1)? / 2;
        (n >= 2 && v.len() == 2 * n + 1).then(|| {
            Radial::new(
                v[1..=n].to_vec(),
                v[n + 1..].iter().map(|x| f(*x)).collect(),
            )
        })?
    };
    let vignetting = numbers(FUJI_VIGNETTING)
        .and_then(|v| pairs(&v, &|p| (100. / p).powf(FUJI_VIGNETTE_STRENGTH)))
        .filter(|r| r.values.iter().any(|v| (v - 1.).abs() > 1e-4));
    let distortion = numbers(FUJI_DISTORTION)
        .and_then(|v| pairs(&v, &|p| 1. + p / 100.))
        .filter(|r| r.values.iter().any(|v| (v - 1.).abs() > 1e-6));
    // [scale, n knots, n red, n blue]
    let chromatic = numbers(FUJI_CHROMATIC).and_then(|v| {
        let n = v.len().checked_sub(1)? / 3;
        if n < 2 || v.len() != 3 * n + 1 {
            return None;
        }
        let knots = v[1..=n].to_vec();
        let red = Radial::new(
            knots.clone(),
            v[n + 1..=2 * n].iter().map(|x| 1. + x).collect(),
        )?;
        let blue = Radial::new(knots, v[2 * n + 1..].iter().map(|x| 1. + x).collect())?;
        [&red, &blue]
            .iter()
            .any(|r| r.values.iter().any(|v| (v - 1.).abs() > 1e-7))
            .then_some([red, blue])
    });
    Some(LensCorrection {
        source: "Fujifilm built-in".into(),
        default_on: true,
        vignetting,
        distortion,
        chromatic,
        gain_map: None,
    })
}

/// Sony's tables are signed shorts led by the number of values and padded
/// with zeros to a fixed length (ZV-1: 11 distortion knots in 16 slots).
/// Knots are spread evenly from the centre to the corner. Distortion is in
/// 2^-14, CA in 2^-21 units; vignetting v restores 2^(2^(v/8192 - 1) - 0.5).
fn sony(file: &[u8]) -> Option<LensCorrection> {
    let root =
        IFD::new_root_with_correction(&mut Cursor::new(file), 0, 0, 0, 1, &[SUB_IFDS]).ok()?;
    let sub = root.get_sub_ifd_all(SUB_IFDS)?.first()?;
    let numbers = |tag: u16| -> Option<Vec<f32>> {
        let v = &sub.get_entry(tag)?.value;
        (0..v.count())
            .map(|i| v.get_f32(i).ok().flatten())
            .collect()
    };
    let knots = |n: usize| {
        (0..n)
            .map(|i| i as f32 / (n - 1) as f32)
            .collect::<Vec<_>>()
    };
    // The count's values, from storage that may be longer: a ZV-1 stores 11
    // distortion knots in 16 slots. Anything stored past the count must be
    // zero padding.
    let counted = |v: Option<Vec<f32>>, per_count: usize| {
        let v = v?;
        let count = *v.first()? as usize;
        let values = v.get(1..=count)?;
        (count >= 2 * per_count
            && count.is_multiple_of(per_count)
            && v[count + 1..].iter().all(|x| *x == 0.))
        .then(|| values.to_vec())
    };
    let single = |v: Option<Vec<f32>>, f: &dyn Fn(f32) -> f32| {
        let v = counted(v, 1)?;
        Radial::new(knots(v.len()), v.iter().map(|x| f(*x)).collect())
    };
    let vignetting = single(numbers(SONY_VIGNETTING), &|v| {
        (2f32.powf(v / 8192. - 1.) - 0.5).exp2()
    })
    .filter(|r| r.values.iter().any(|v| (v - 1.).abs() > 1e-4));
    let distortion = single(numbers(SONY_DISTORTION), &|d| 1. + d / 16384.)
        .filter(|r| r.values.iter().any(|v| (v - 1.).abs() > 1e-6));
    let chromatic = counted(numbers(SONY_CHROMATIC), 2).and_then(|v| {
        let n = v.len() / 2;
        let scale = |s: &[f32]| s.iter().map(|x| 1. + x / 2_097_152.).collect();
        Some([
            Radial::new(knots(n), scale(&v[..n]))?,
            Radial::new(knots(n), scale(&v[n..]))?,
        ])
    });
    Some(LensCorrection {
        source: LensCorrection::SONY_BUILTIN.into(),
        // Not yet compared with Lightroom, which may apply its own profile instead.
        default_on: false,
        vignetting,
        distortion,
        chromatic,
        gain_map: None,
    })
}

/// A DNG's FixVignetteRadial, WarpRectilinear and OpcodeList3 GainMaps;
/// `None` for a file that is not a DNG or has none of them.
fn dng(file: &[u8]) -> Option<LensCorrection> {
    let first = |tag: DngTag, id: u32| -> Option<Vec<u8>> {
        let list = crate::dng_opcodes::list(file, tag)?;
        let ops = crate::dng_opcodes::opcodes(&list)?;
        ops.into_iter()
            .find(|op| op.id == id)
            .map(|op| op.params.to_vec())
    };
    let vignetting = [DngTag::OpcodeList2, DngTag::OpcodeList3]
        .into_iter()
        .find_map(|tag| first(tag, FIX_VIGNETTE_RADIAL))
        .and_then(|p| vignette(&p));
    let (distortion, chromatic) = first(DngTag::OpcodeList3, WARP_RECTILINEAR)
        .and_then(|p| warp(&p))
        .unwrap_or((None, None));
    // OpcodeList3 applies to the demosaiced image: the active area (or the
    // whole stored image), whose size its GainMaps must cover.
    let gain_map = crate::dng_opcodes::list(file, DngTag::OpcodeList3)
        .zip(crate::dng_opcodes::image_size(file, DngTag::OpcodeList3))
        .and_then(|(list, size)| gain_grid(&list, size));
    let lens = LensCorrection {
        source: "DNG".into(),
        default_on: true,
        vignetting,
        distortion,
        chromatic,
        gain_map,
    };
    (!lens.is_empty()).then_some(lens)
}

/// OpcodeList3's GainMaps as one gain per channel, on an image of `size`
/// [height, width]. Each must cover a rectangle inside the image, every row
/// and column, with planes among the three channels, each plane at most
/// once; maps for separate planes must share one grid and area, and a warp
/// may come before them or after them, not between. Anything else is not
/// read, and the photo renders as before.
fn gain_grid(list: &[u8], [height, width]: [u32; 2]) -> Option<GainGrid> {
    let ops = crate::dng_opcodes::opcodes(list)?;
    let ids: Vec<u32> = ops.iter().map(|op| op.id).collect();
    let gain_map = crate::dng_opcodes::GAIN_MAP;
    let after_warp = ids
        .iter()
        .position(|id| *id == WARP_RECTILINEAR)
        .zip(ids.iter().position(|id| *id == gain_map))
        .is_some_and(|(warp, gain)| warp < gain);
    if ids
        .iter()
        .skip_while(|id| **id != gain_map)
        .skip_while(|id| **id == gain_map)
        .any(|id| *id == gain_map)
    {
        return None;
    }
    let mut grid: Option<GainGrid> = None;
    let mut per_plane: [Option<Vec<f32>>; 3] = Default::default();
    for op in ops.iter().filter(|op| op.id == gain_map) {
        // A map of gains 1 changes nothing.
        let Some(map) = crate::dng_gain_map::gain_map(op.params)? else {
            continue;
        };
        let (plane, planes) = (map.plane, map.planes);
        let [rows, columns] = map.points;
        if map.top >= map.bottom
            || map.left >= map.right
            || map.bottom > height as usize
            || map.right > width as usize
            || map.pitch != [1, 1]
            || planes == 0
            || plane >= 3
            || planes > 3 - plane
            || map.map_planes > planes as usize
            || !(1..=256).contains(&rows)
            || !(1..=256).contains(&columns)
        {
            return None;
        }
        let shape = GainGrid {
            area: [
                map.top as f32 / height as f32,
                map.left as f32 / width as f32,
                map.bottom as f32 / height as f32,
                map.right as f32 / width as f32,
            ],
            after_warp,
            points: map.points,
            origin: map.origin.map(|v| v as f32),
            spacing: map.spacing.map(|v| v as f32),
            planes: 1,
            gains: Vec::new(),
        };
        if grid.as_ref().is_some_and(|g| *g != shape) {
            return None;
        }
        grid = Some(shape);
        for c in plane..plane + planes {
            // The DNG SDK reads the map's plane of the image plane itself, the last
            // one for planes beyond the map's.
            let m = (c as usize).min(map.map_planes - 1);
            // Two maps on one plane multiply sample by sample, which a single grid
            // holds only where their points coincide: not read.
            if per_plane[c as usize].is_some() {
                return None;
            }
            per_plane[c as usize] = Some(map.gains.chunks(map.map_planes).map(|g| g[m]).collect());
        }
    }
    let mut grid = grid?;
    let points = grid.points[0] * grid.points[1];
    let one = vec![1.; points];
    let planes: [&Vec<f32>; 3] = std::array::from_fn(|c| per_plane[c].as_ref().unwrap_or(&one));
    grid.planes = 3;
    grid.gains = (0..points).flat_map(|i| planes.map(|p| p[i])).collect();
    // A map the lens correction does not hold is left out on its own: the file's
    // warp and vignetting still apply.
    (grid.valid() && grid.gains.iter().any(|g| (g - 1.).abs() > 1e-4)).then_some(grid)
}

fn doubles(p: &[u8]) -> Vec<f64> {
    p.as_chunks::<8>()
        .0
        .iter()
        .map(|c| f64::from_be_bytes(*c))
        .collect()
}

/// Radius samples; the opcode radius, like ours, is 1 at the farthest corner
/// from the optical centre, which must be near the image centre.
fn dng_knots() -> Vec<f32> {
    (0..=32).map(|i| i as f32 / 32.).collect()
}

fn centred(cx: f64, cy: f64) -> bool {
    (cx - 0.5).abs() < 0.02 && (cy - 0.5).abs() < 0.02
}

/// FixVignetteRadial: gain 1 + k0 r^2 + k1 r^4 + k2 r^6 + k3 r^8 + k4 r^10.
fn vignette(p: &[u8]) -> Option<Radial> {
    let v = doubles(p);
    if v.len() != 7 || !centred(v[5], v[6]) {
        return None;
    }
    let values = dng_knots()
        .iter()
        .map(|r| {
            let r2 = f64::from(*r * *r);
            (1. + (0..5).map(|i| v[i] * r2.powi(i as i32 + 1)).sum::<f64>()) as f32
        })
        .collect();
    let r = Radial {
        knots: dng_knots(),
        values,
    };
    r.values.iter().any(|g| (g - 1.).abs() > 1e-4).then_some(r)
}

/// WarpRectilinear: source radius ratio kr0 + kr1 r^2 + kr2 r^4 + kr3 r^6 per
/// plane. Tangential terms are ignored.
fn warp(p: &[u8]) -> Option<(Option<Radial>, Option<[Radial; 2]>)> {
    let planes = u32::from_be_bytes(p.get(..4)?.try_into().ok()?) as usize;
    let v = doubles(p.get(4..)?);
    if !matches!(planes, 1 | 3)
        || v.len() != planes * 6 + 2
        || !centred(v[planes * 6], v[planes * 6 + 1])
    {
        return None;
    }
    let ratio = |plane: usize, r: f32| {
        let k = &v[plane * 6..plane * 6 + 4];
        let r2 = f64::from(r * r);
        (k[0] + k[1] * r2 + k[2] * r2 * r2 + k[3] * r2 * r2 * r2) as f32
    };
    let green = if planes == 3 { 1 } else { 0 };
    let curve = |f: &dyn Fn(f32) -> f32| Radial {
        knots: dng_knots(),
        values: dng_knots().into_iter().map(f).collect(),
    };
    let distortion = curve(&|r| ratio(green, r));
    let chromatic = (planes == 3).then(|| {
        [
            curve(&|r| ratio(0, r) / ratio(1, r)),
            curve(&|r| ratio(2, r) / ratio(1, r)),
        ]
    });
    let changes = |r: &Radial| r.values.iter().any(|v| (v - 1.).abs() > 1e-6);
    Some((
        Some(distortion).filter(changes),
        chromatic.filter(|[a, b]| changes(a) || changes(b)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (tag, type, count, data)
    type Entry = (u16, u16, u32, Vec<u8>);

    /// Little-endian TIFF with the given directories, each pointed to by the
    /// previous one's first entry. RAWmakase's builder, as written.
    fn tiff(dirs: &[Vec<Entry>]) -> Vec<u8> {
        let mut out = b"II*\0\x08\0\0\0".to_vec();
        for (d, entries) in dirs.iter().enumerate() {
            let start = out.len();
            let data_start = start + 2 + entries.len() * 12 + 4;
            let mut data: Vec<u8> = Vec::new();
            out.extend((entries.len() as u16).to_le_bytes());
            for (tag, kind, count, bytes) in entries {
                out.extend(tag.to_le_bytes());
                out.extend(kind.to_le_bytes());
                out.extend(count.to_le_bytes());
                if bytes.len() <= 4 {
                    let mut v = bytes.clone();
                    v.resize(4, 0);
                    out.extend(v);
                } else {
                    out.extend(((data_start + data.len()) as u32).to_le_bytes());
                    data.extend(bytes);
                }
            }
            out.extend(0u32.to_le_bytes());
            out.extend(data);
            // A pointer entry value of 0 is patched to the next directory.
            if d + 1 < dirs.len() {
                let next = out.len() as u32;
                let value = start + 2 + 8;
                out[value..value + 4].copy_from_slice(&next.to_le_bytes());
            }
        }
        out
    }

    fn rationals(v: &[f64]) -> Vec<u8> {
        v.iter()
            .flat_map(|x| {
                let n = (*x * 1_000_000.).round() as i32;
                [n.to_le_bytes(), 1_000_000i32.to_le_bytes()].concat()
            })
            .collect()
    }

    /// A RAF whose FujiIFD holds `entries`, its pointer of type 13 (IFD) as
    /// Fujifilm writes it.
    fn raf(entries: Vec<Entry>) -> Vec<u8> {
        let fuji = tiff(&[vec![(FUJI_IFD, 13, 1, vec![0; 4])], entries]);
        let mut raf = b"FUJIFILMCCD-RAW 0201FF383501".to_vec();
        raf.resize(128, 0);
        raf[RAF_TIFF_POINTER..RAF_TIFF_POINTER + 4].copy_from_slice(&128u32.to_be_bytes());
        raf.extend(fuji);
        raf
    }

    #[test]
    fn radial_interpolates_and_clamps() {
        let r = Radial::new(vec![0., 0.5, 1.], vec![1., 1.2, 1.6]).unwrap();
        assert_eq!(r.eval(-1.), 1.);
        assert!((r.eval(0.25) - 1.1).abs() < 1e-6);
        assert!((r.eval(0.75) - 1.4).abs() < 1e-6);
        assert_eq!(r.eval(2.), 1.6);
        assert!(Radial::new(vec![0., 0.], vec![1., 1.]).is_none());
        assert!(Radial::new(vec![0., 1.], vec![1., f32::NAN]).is_none());
    }

    #[test]
    fn fill_scale_keeps_corners_inside() {
        let c = LensCorrection {
            distortion: Radial::new(vec![0., 1.], vec![1., 1.02]),
            ..Default::default()
        };
        let s = c.fill_scale_with(Amounts::FULL);
        assert!(s < 1. && s > 0.97);
        assert!(c.radial_scale_with(s, Amounts::FULL)[1] * s <= 1.0001);
        assert_eq!(LensCorrection::default().fill_scale_with(Amounts::FULL), 1.);
    }

    #[test]
    fn amounts_scale_each_part() {
        let c = LensCorrection {
            distortion: Radial::new(vec![0., 1.], vec![1., 1.1]),
            chromatic: Some([
                Radial::new(vec![0., 1.], vec![1., 1.01]).unwrap(),
                Radial::new(vec![0., 1.], vec![1., 0.99]).unwrap(),
            ]),
            vignetting: Radial::new(vec![0., 1.], vec![1., 2.]),
            ..Default::default()
        };
        let half = Amounts {
            distortion: 0.5,
            chromatic: 0.,
            vignetting: 2.,
        };
        let [r, g, b] = c.radial_scale_with(1., half);
        assert!((g - 1.05).abs() < 1e-6);
        assert!(r == g && b == g, "no CA at amount 0");
        assert!((c.vignetting_gain(1., 2.) - 4.).abs() < 1e-5);
        assert_eq!(c.vignetting_gain(1., 0.), 1.);
    }

    #[test]
    fn lens_inverse_undoes_forward() {
        let lens = LensCorrection {
            distortion: Radial::new(vec![0., 0.5, 1.], vec![1., 1.02, 1.08]),
            ..Default::default()
        };
        let map = LensMap::new(&lens, 300., 200., Amounts::FULL);
        assert!(map.fill < 1.);
        for (x, y) in [(-140., -80.), (0., 0.), (140., 90.), (-110., 70.)] {
            let [gx, gy] = map.forward(x, y);
            let [bx, by] = map.inverse(gx, gy);
            assert!((bx - x).abs() < 1e-2 && (by - y).abs() < 1e-2, "{x} {y}");
        }
    }

    #[test]
    fn reads_fujifilm_raf_tables() {
        let knots: Vec<f64> = (0..=10).map(|i| i as f64 / 10.).collect();
        let mut vig = vec![327.7];
        vig.extend(&knots);
        vig.extend([
            100., 99.5, 98.8, 97.5, 95.4, 92.2, 87.7, 82.4, 75.8, 68.3, 60.4,
        ]);
        let mut dist = vec![327.7];
        dist.extend(&knots);
        dist.extend(std::iter::repeat_n(0., 11));
        let mut ca = vec![327.7];
        ca.extend(&knots);
        ca.extend((0..=10).map(|i| i as f64 * 1e-4));
        ca.extend(std::iter::repeat_n(0., 11));
        let file = raf(vec![
            (FUJI_DISTORTION, 10, 23, rationals(&dist)),
            (FUJI_CHROMATIC, 10, 34, rationals(&ca)),
            (FUJI_VIGNETTING, 10, 23, rationals(&vig)),
        ]);
        let c = read(&file).unwrap();
        assert!(c.default_on);
        assert!(c.distortion.is_none(), "identity distortion is omitted");
        let v = c.vignetting.as_ref().unwrap();
        let gain = |p: f32| (100. / p).powf(FUJI_VIGNETTE_STRENGTH);
        assert!((v.eval(1.) - gain(60.4)).abs() < 1e-4);
        assert!((v.eval(0.85) - (gain(75.8) + gain(68.3)) / 2.).abs() < 1e-4);
        let [red, blue] = c.chromatic.as_ref().unwrap();
        assert!((red.eval(1.) - 1.001).abs() < 1e-6);
        assert_eq!(blue.eval(1.), 1.);
    }

    #[test]
    fn unsupported_or_truncated_files_have_no_correction() {
        let mut header_only = b"FUJIFILMCCD-RAW 0201FF383501".to_vec();
        header_only.resize(104, 0xff);
        for (name, bytes) in [
            ("empty", &b""[..]),
            ("text", &[b'x'; 200][..]),
            ("tiff", b"II*\0\x08\0\0\0\0\0"),
            ("raf header pointing nowhere", &header_only[..]),
        ] {
            assert!(read(bytes).is_none(), "{name}");
        }
        // A RAF whose FujiIFD has no tables.
        assert!(read(&raf(vec![(0xf001, 4, 1, vec![1, 0, 0, 0])])).is_none());
    }

    /// A real RAF (AG_RAW): its tables at a few radii, and the zoom that
    /// keeps the corners inside. Prints; asserts only that one was read.
    /// An opcode list holding `ops` (id, parameters), big-endian.
    fn opcode_list(ops: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let mut b = (ops.len() as u32).to_be_bytes().to_vec();
        for (id, params) in ops {
            for v in [*id, 0x0103_0000, 0, params.len() as u32] {
                b.extend(v.to_be_bytes());
            }
            b.extend(params);
        }
        b
    }

    fn be(v: &[f64]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_be_bytes()).collect()
    }

    #[test]
    fn decodes_vignette_and_warp_opcodes() {
        // DSCF7853's FixVignetteRadial from Lightroom's DNG (RAWmakase's test).
        let p = be(&[0.2599, 0.3424, 0.0630, 0.0346, -0.0300, 0.5, 0.5]);
        let v = vignette(&p).unwrap();
        assert!((v.eval(1.) - 1.67).abs() < 0.01);
        assert!((v.eval(0.5) - 1.087).abs() < 0.002);
        let mut w = 3u32.to_be_bytes().to_vec();
        w.extend(be(&[1.0002, 0.0005, 0., 0., 0., 0.]));
        w.extend(be(&[1., 0., 0., 0., 0., 0.]));
        w.extend(be(&[1., 0., 0., 0., 0., 0.]));
        w.extend(be(&[0.5, 0.5]));
        let (distortion, chromatic) = warp(&w).unwrap();
        assert!(distortion.is_none());
        let [red, blue] = chromatic.unwrap();
        assert!((red.eval(1.) - 1.0007).abs() < 1e-5);
        assert_eq!(blue.eval(1.), 1.);
        // Off-centre corrections are not representable and are skipped.
        assert!(vignette(&be(&[0.2, 0., 0., 0., 0., 0.3, 0.5])).is_none());
    }

    /// A GainMap opcode's parameters: `area` (top, left, bottom, right), its planes,
    /// a `rows` × `columns` map over the whole area and `map_planes` gains per point.
    /// RAWmakase's, as written.
    fn gain_map(
        area: [u32; 4],
        plane: u32,
        planes: u32,
        map_planes: u32,
        gains: &[f32],
    ) -> Vec<u8> {
        let (rows, columns) = (2u32, 3u32);
        let mut p: Vec<u8> = area
            .into_iter()
            .chain([plane, planes, 1, 1, rows, columns])
            .flat_map(u32::to_be_bytes)
            .collect();
        p.extend(be(&[1., 0.5, 0., 0.]));
        p.extend(map_planes.to_be_bytes());
        p.extend(gains.iter().flat_map(|g| g.to_be_bytes()));
        p
    }

    /// RAWmakase's test (`dng.rs` @ f0899e0), its lists built by `opcode_list`.
    #[test]
    fn reads_opcode_list3_gain_maps_per_channel() {
        let lists = opcode_list;
        let area = [0, 0, 300, 400];
        // One map for all three planes, as DJI and Hasselblad drones write it, before
        // the warp.
        let rgb: Vec<f32> = (0..6)
            .flat_map(|i| [1. + i as f32 * 0.1, 1., 1.5])
            .collect();
        let warp = {
            let mut w = 1u32.to_be_bytes().to_vec();
            w.extend(be(&[1., 0.01, 0., 0., 0., 0., 0.5, 0.5]));
            w
        };
        let list = lists(&[(9, gain_map(area, 0, 3, 3, &rgb)), (1, warp.clone())]);
        let grid = gain_grid(&list, [300, 400]).unwrap();
        assert_eq!((grid.points, grid.planes), ([2, 3], 3));
        assert_eq!(grid.gain(0., 0.), [1., 1., 1.5]);
        // Towards the last point, and halfway along the first row: bilinear.
        let [r, ..] = grid.gain(0.9999, 0.9999);
        assert!((r - 1.5).abs() < 1e-3);
        let [r, ..] = grid.gain(0., 0.25);
        assert!((r - 1.05).abs() < 1e-6);
        // Outside the image, nothing.
        assert_eq!(grid.gain(1., 0.5), [1.; 3]);
        // A map per plane, and a plane without one keeps its level.
        let list = lists(&[
            (9, gain_map(area, 0, 1, 1, &[2.; 6])),
            (9, gain_map(area, 2, 1, 1, &[3.; 6])),
        ]);
        assert_eq!(
            gain_grid(&list, [300, 400]).unwrap().gain(0.5, 0.5),
            [2., 1., 3.]
        );
        // A map of one plane for all three.
        let list = lists(&[(9, gain_map(area, 0, 3, 1, &[2.; 6]))]);
        assert_eq!(
            gain_grid(&list, [300, 400]).unwrap().gain(0.5, 0.5),
            [2.; 3]
        );
        // Planes from 1: each reads the map's plane of its own index, as the DNG SDK
        // does, the map's last for planes beyond it.
        let two: Vec<f32> = (0..6).flat_map(|_| [2., 3.]).collect();
        let list = lists(&[(9, gain_map(area, 1, 2, 2, &two))]);
        assert_eq!(
            gain_grid(&list, [300, 400]).unwrap().gain(0.5, 0.5),
            [1., 3., 3.]
        );
        // A map of part of the image, after the warp (the Sigma fp's): no gain outside
        // its area, and its points over the area.
        let list = lists(&[
            (1, warp.clone()),
            (9, gain_map([30, 40, 270, 360], 0, 3, 3, &rgb)),
        ]);
        let grid = gain_grid(&list, [300, 400]).unwrap();
        assert!(grid.after_warp);
        assert_eq!(grid.gain(0.05, 0.5), [1.; 3]);
        // Inside it, the points are over the whole image, as without an area.
        assert_eq!(grid.gain(0.5, 0.5), [1.25, 1., 1.5]);
        assert!(
            !gain_grid(&lists(&[(9, gain_map(area, 0, 3, 3, &rgb))]), [300, 400])
                .unwrap()
                .after_warp
        );
        // Grids beyond 256 points a side, however large, are not read.
        let mut huge = gain_map(area, 0, 3, 3, &[2.; 18]);
        huge[32..40].copy_from_slice(&[u32::MAX.to_be_bytes(), u32::MAX.to_be_bytes()].concat());
        assert!(gain_grid(&lists(&[(9, huge)]), [300, 400]).is_none());
        // Planes beyond the image's three, however large, are not read.
        assert!(
            gain_grid(
                &lists(&[(9, gain_map(area, u32::MAX, 2, 1, &[2.; 6]))]),
                [300, 400]
            )
            .is_none()
        );
        // Maps beyond the image, with a warp between them, or of gains 1, are not read.
        for (list, size) in [
            (lists(&[(9, gain_map(area, 0, 3, 3, &rgb))]), [290, 400]),
            (
                lists(&[
                    (9, gain_map(area, 0, 1, 1, &[2.; 6])),
                    (1, warp.clone()),
                    (9, gain_map(area, 1, 1, 1, &[2.; 6])),
                ]),
                [300, 400],
            ),
            (lists(&[(9, gain_map(area, 0, 3, 1, &[1.; 6]))]), [300, 400]),
            // Two maps on one plane, and gains a lens correction does not hold.
            (
                lists(&[
                    (9, gain_map(area, 0, 1, 1, &[2.; 6])),
                    (9, gain_map(area, 0, 1, 1, &[3.; 6])),
                ]),
                [300, 400],
            ),
            (lists(&[(9, gain_map(area, 0, 3, 1, &[9.; 6]))]), [300, 400]),
        ] {
            assert!(gain_grid(&list, size).is_none());
        }
    }

    /// A 2 × 3 grid over the whole image whose red gain grows to the right.
    fn red_to_the_right(after_warp: bool) -> LensCorrection {
        LensCorrection {
            vignetting: Radial::new(vec![0., 1.], vec![1., 2.]),
            gain_map: Some(GainGrid {
                after_warp,
                points: [2, 3],
                spacing: [1., 0.5],
                planes: 3,
                gains: [1., 1.5, 2., 1., 1.5, 2.]
                    .into_iter()
                    .flat_map(|r| [r, 1.25, 1.])
                    .collect(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn each_channel_reads_the_map_where_it_samples_with_the_vignetting() {
        let lens = red_to_the_right(false);
        let map = LensMap::new(&lens, 400., 300., Amounts::FULL);
        // At the centre: the map's middle column, no vignetting.
        assert_eq!(map.gains([0.; 2], [0.; 2], [1., 1.]), [1.5, 1.25, 1.]);
        // Halfway to the right edge, red sampling 10% further out: its gain is
        // read there, and the vignetting at green's radius goes on all three.
        let v = lens.vignetting_gain(100. / 250., 1.);
        let [r, g, b] = map.gains([0.; 2], [100., 0.], [1.1, 1.]);
        assert!((r - v * 1.775).abs() < 1e-5, "{r}");
        assert!((g - v * 1.25).abs() < 1e-5 && (b - v).abs() < 1e-5);
        // At half the Vignetting amount, each gain is its square root.
        let half = Amounts {
            vignetting: 0.5,
            ..Amounts::FULL
        };
        let [r, ..] = LensMap::new(&lens, 400., 300., half).gains([0.; 2], [0.; 2], [1., 1.]);
        assert!((r - 1.5f32.sqrt()).abs() < 1e-6);
        // Off, none of it.
        let off = Amounts {
            vignetting: 0.,
            ..Amounts::FULL
        };
        let none = LensMap::new(&lens, 400., 300., off);
        assert_eq!(none.gains([0.; 2], [100., 0.], [1.1, 1.]), [1.; 3]);
        // A map after the warp reads the corrected position, not the sample's.
        let after = red_to_the_right(true);
        let map = LensMap::new(&after, 400., 300., Amounts::FULL);
        let [r, ..] = map.gains([0.; 2], [100., 0.], [1.1, 1.]);
        assert!(
            (r - lens.vignetting_gain(0.4, 1.) * 1.5).abs() < 1e-5,
            "{r}"
        );
    }

    #[test]
    fn a_correction_without_a_gain_map_reads_and_writes_as_before() {
        let before = r#"{"source":"DNG","default_on":true,"vignetting":null,"distortion":{"knots":[0.0,1.0],"values":[1.0,0.98]},"chromatic":null}"#;
        let c: LensCorrection = serde_json::from_str(before).unwrap();
        assert!(c.gain_map.is_none());
        assert_eq!(serde_json::to_string(&c).unwrap(), before);
        let with = red_to_the_right(false);
        let back: LensCorrection =
            serde_json::from_str(&serde_json::to_string(&with).unwrap()).unwrap();
        assert_eq!(back, with);
    }

    #[test]
    fn reads_a_dngs_opcodes_with_the_vignetting_in_either_list() {
        let vig = be(&[0.2599, 0.3424, 0.0630, 0.0346, -0.0300, 0.5, 0.5]);
        let mut w = 1u32.to_be_bytes().to_vec();
        w.extend(be(&[1., -0.06, 0., 0., 0., 0., 0.5, 0.5]));
        // As DJI writes it: both in OpcodeList3, a gain map after them.
        let list3 = opcode_list(&[(3, vig.clone()), (1, w.clone()), (9, vec![0; 8])]);
        let dng = tiff(&[
            vec![
                (SUB_IFDS, 4, 1, vec![0; 4]),
                (50706, 1, 4, vec![1, 4, 0, 0]),
            ],
            vec![
                (254, 4, 1, vec![0; 4]),
                (51022, 7, list3.len() as u32, list3),
            ],
        ]);
        let c = read(&dng).unwrap();
        assert_eq!(c.source, "DNG");
        assert!(c.default_on);
        assert!((c.vignetting.as_ref().unwrap().eval(1.) - 1.67).abs() < 0.01);
        assert!((c.distortion.as_ref().unwrap().eval(1.) - 0.94).abs() < 1e-5);
        assert!(c.chromatic.is_none(), "one plane has no CA");
        // The whole frame's corners, not Sony's camera frame.
        assert_eq!(
            LensMap::new(&c, 300., 200., Amounts::FULL).fill,
            c.fill_scale_with(Amounts::FULL)
        );
        // Without its DNGVersion it is not a DNG, and has no Sony tables either.
        let not_dng = tiff(&[
            vec![(SUB_IFDS, 4, 1, vec![0; 4])],
            vec![(51022, 7, 4, vec![0; 4])],
        ]);
        assert!(read(&not_dng).is_none());
    }

    #[test]
    fn reads_a_drone_dngs_gain_map_over_its_active_area() {
        let rgb: Vec<f32> = (0..6).flat_map(|_| [2., 1.5, 1.8]).collect();
        let list3 = opcode_list(&[(9, gain_map([0, 0, 300, 400], 0, 3, 3, &rgb))]);
        let longs = |v: &[u32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        // ImageWidth, ImageLength and, when given, ActiveArea.
        let dng = |width: u32, height: u32, active: Option<[u32; 4]>| {
            let mut raw = vec![
                (254, 4, 1, vec![0; 4]),
                (256, 4, 1, longs(&[width])),
                (257, 4, 1, longs(&[height])),
            ];
            raw.extend(active.map(|a| (50829, 4, 4, longs(&a))));
            raw.push((51022, 7, list3.len() as u32, list3.clone()));
            tiff(&[
                vec![
                    (SUB_IFDS, 4, 1, vec![0; 4]),
                    (50706, 1, 4, vec![1, 4, 0, 0]),
                ],
                raw,
            ])
        };
        for (name, file) in [
            ("active area", dng(440, 320, Some([10, 20, 310, 420]))),
            ("whole image", dng(400, 300, None)),
        ] {
            let c = read(&file).expect(name);
            assert_eq!(c.source, "DNG");
            assert!(c.vignetting.is_none() && c.distortion.is_none());
            let grid = c.gain_map.as_ref().expect(name);
            assert_eq!(grid.gain(0.5, 0.5), [2., 1.5, 1.8], "{name}");
        }
        // A map larger than the image is not read, and leaves nothing.
        assert!(read(&dng(400, 290, None)).is_none());
    }

    fn shorts(v: &[i16]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    /// An ARW-like TIFF whose first SubIFD holds Sony's three tables.
    fn sony_file(vig: &[i16], ca: &[i16], dist: &[i16]) -> Vec<u8> {
        let entry = |tag, v: &[i16]| (tag, 8, v.len() as u32, shorts(v));
        tiff(&[
            vec![(SUB_IFDS, 4, 1, vec![0; 4])],
            vec![
                entry(SONY_VIGNETTING, vig),
                entry(SONY_CHROMATIC, ca),
                entry(SONY_DISTORTION, dist),
            ],
        ])
    }

    /// A ZV-1 stores fewer knots than its fixed 16 (CA: 32) slots, padded
    /// with zeros (RAWmakase #403).
    const ZV1_VIG: [i16; 17] = [
        16, 0, 0, 0, 15, 63, 193, 413, 715, 1107, 1576, 2126, 2749, 3443, 4523, 5509, 6639,
    ];
    const ZV1_CA: [i16; 33] = [
        22, 0, 256, 384, 128, 128, 256, 384, 640, 896, 1152, 1408, 0, 1280, 1664, 1664, 1280, 896,
        512, 256, 128, 0, 256, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    const ZV1_DIST: [i16; 17] = [
        11, 1265, 1218, 1083, 860, 562, 209, -179, -580, -972, -1353, -1725, 0, 0, 0, 0, 0,
    ];

    #[test]
    fn reads_sony_subifd_tables() {
        let mut vig = vec![16i16];
        vig.extend((0..16).map(|i| i * 800));
        let mut dist = vec![16i16];
        dist.extend((0..16).map(|i| i * 10));
        let mut ca = vec![32i16];
        ca.extend(std::iter::repeat_n(-384, 16));
        ca.extend(std::iter::repeat_n(0, 16));
        let c = read(&sony_file(&vig, &ca, &dist)).unwrap();
        assert!(!c.default_on);
        assert_eq!(c.source, LensCorrection::SONY_BUILTIN);
        let v = c.vignetting.as_ref().unwrap();
        assert!((v.eval(0.) - 1.).abs() < 1e-6);
        let corner = 2f32.powf(2f32.powf(12000. / 8192. - 1.) - 0.5);
        assert!((v.eval(1.) - corner).abs() < 1e-5);
        assert!((c.distortion.as_ref().unwrap().eval(1.) - (1. + 150. / 16384.)).abs() < 1e-6);
        let [red, blue] = c.chromatic.as_ref().unwrap();
        assert!((red.eval(0.5) - (1. - 384. / 2_097_152.)).abs() < 1e-7);
        assert_eq!(blue.eval(0.5), 1.);
    }

    #[test]
    fn reads_sony_tables_padded_to_fixed_slots() {
        let c = read(&sony_file(&ZV1_VIG, &ZV1_CA, &ZV1_DIST)).unwrap();
        let dist = c.distortion.as_ref().unwrap();
        assert_eq!(dist.knots.len(), 11);
        assert_eq!(dist.knots[1], 0.1);
        assert!((dist.eval(0.) - (1. + 1265. / 16384.)).abs() < 1e-6);
        assert!((dist.eval(1.) - (1. - 1725. / 16384.)).abs() < 1e-6);
        let [red, blue] = c.chromatic.as_ref().unwrap();
        assert_eq!(red.knots.len(), 11);
        assert!((red.eval(1.) - (1. + 1408. / 2_097_152.)).abs() < 1e-7);
        assert!((blue.eval(0.1) - (1. + 1280. / 2_097_152.)).abs() < 1e-7);
        assert!((blue.eval(1.) - (1. + 256. / 2_097_152.)).abs() < 1e-7);
        assert_eq!(c.vignetting.as_ref().unwrap().knots.len(), 16);
    }

    #[test]
    fn rejects_malformed_sony_tables() {
        fn with<const N: usize>(mut v: [i16; N], i: usize, x: i16) -> [i16; N] {
            v[i] = x;
            v
        }
        for (name, dist, ca) in [
            (
                "data past the count",
                with(ZV1_DIST, 16, 1),
                with(ZV1_CA, 32, 1),
            ),
            (
                "count past the stored values",
                with(ZV1_DIST, 0, 17),
                with(ZV1_CA, 0, 34),
            ),
            ("a single knot", with(ZV1_DIST, 0, 1), with(ZV1_CA, 0, 2)),
        ] {
            let c = read(&sony_file(&ZV1_VIG, &ca, &dist)).unwrap();
            assert!(c.distortion.is_none() && c.chromatic.is_none(), "{name}");
        }
        let odd = with(ZV1_CA, 0, 21);
        let c = read(&sony_file(&ZV1_VIG, &odd, &ZV1_DIST)).unwrap();
        assert!(c.chromatic.is_none(), "odd CA count");
    }

    #[test]
    fn fill_within_a_crop_of_a_wider_sensor_is_looser_and_still_inside() {
        let c = LensCorrection {
            distortion: Radial::new(vec![0., 1.], vec![1., 1.08]),
            ..Default::default()
        };
        let size = [6000., 4000.];
        // A 16:9 crop of the 3:2 image.
        let crop = [0., 437.5, 6000., 3812.5];
        let whole = c.fill_scale_with(Amounts::FULL);
        let within = c.fill_scale_within(Amounts::FULL, size, crop);
        assert!(within > whole + 0.005 && within <= 1., "{whole} {within}");
        let half = 6000f32.hypot(4000.) * 0.5;
        for (x, y) in [
            (3000f32, 1562.5f32),
            (3000., 0.),
            (0., 1562.5),
            (1500., 1562.5),
        ] {
            let s = c.radial_scale_with(x.hypot(y) * within / half, Amounts::FULL)[1];
            assert!(
                x * within * s <= 3000.01 && y * within * s <= 2000.01,
                "{x} {y}"
            );
        }
        // The whole image is never tighter than its own scale; no distortion needs none.
        let all = c.fill_scale_within(Amounts::FULL, size, [0., 0., 6000., 4000.]);
        assert!(all >= whole && all < whole + 0.01, "{whole} {all}");
        assert_eq!(
            LensCorrection::default().fill_scale_within(Amounts::FULL, size, crop),
            1.
        );
    }

    #[test]
    fn fill_within_leaves_no_edge_pixel_outside_between_samples() {
        // A table with bumps, the kind Sony's signed values give.
        let c = LensCorrection {
            distortion: Radial::new(
                (0..11).map(|i| i as f32 / 10.).collect(),
                vec![
                    1.08, 1.06, 1.04, 1.02, 1.02, 1.02, 1.02, 1.02, 1.02, 1.03, 0.99,
                ],
            ),
            ..Default::default()
        };
        let (w, h) = (6000f32, 4000f32);
        let fill = c.fill_scale_within(Amounts::FULL, [w, h], [0., 0., w, h]);
        let half = w.hypot(h) * 0.5;
        let mut worst = 0f32;
        for i in 0..6000 {
            for (px, py) in [(i as f32 + 0.5, 0.5), (i as f32 + 0.5, h - 0.5)] {
                let (dx, dy) = (px - w * 0.5, py - h * 0.5);
                let s = c.radial_scale_with(dx.hypot(dy) * fill / half, Amounts::FULL)[1];
                worst = worst.max((dx * fill * s).abs() - w * 0.5);
                worst = worst.max((dy * fill * s).abs() - h * 0.5);
            }
        }
        assert!(worst < 0.5, "{worst}");
    }

    #[test]
    fn sonys_tables_keep_the_photos_frame_others_its_corners() {
        // The ZV-1's distortion: 1.077 at the centre, 0.895 at the corner.
        let sony = read(&sony_file(&ZV1_VIG, &ZV1_CA, &ZV1_DIST)).unwrap();
        let (w, h) = (5472f32, 3648f32);
        let map = LensMap::new(&sony, w, h, Amounts::FULL);
        let within = sony.fill_scale_within(Amounts::FULL, [w, h], [0., 0., w, h]);
        assert_eq!(map.fill, within);
        assert!(
            map.fill > sony.fill_scale_with(Amounts::FULL) + 0.01,
            "{}",
            map.fill
        );
        // The same tables from anywhere else keep the whole frame's corners inside.
        let other = LensCorrection {
            source: "DNG".into(),
            ..sony.clone()
        };
        assert_eq!(
            LensMap::new(&other, w, h, Amounts::FULL).fill,
            other.fill_scale_with(Amounts::FULL)
        );
    }

    /// A real RAF or ARW (AG_RAW): its tables at a few radii, and the zoom
    /// that keeps the corners inside on a 3:2 frame. Prints; asserts only
    /// that one was read.
    #[test]
    #[ignore = "reads a local RAF or ARW named by AG_RAW; run by hand"]
    fn a_real_files_tables() {
        let path = std::env::var("AG_RAW").expect("AG_RAW");
        let c = read(&std::fs::read(&path).expect("read")).expect("a correction");
        let at = |r: &Option<Radial>| -> Vec<f32> {
            r.as_ref()
                .map_or(vec![], |r| [0., 0.5, 0.8, 1.].map(|x| r.eval(x)).to_vec())
        };
        eprintln!();
        eprintln!("{path}: {}", c.source);
        eprintln!(
            "  vignetting gain at r 0/0.5/0.8/1: {:.3?}",
            at(&c.vignetting)
        );
        eprintln!("  distortion at r 0/0.5/0.8/1: {:.4?}", at(&c.distortion));
        if let Some([red, blue]) = &c.chromatic {
            eprintln!(
                "  CA red/blue at r 1: {:.5} {:.5}",
                red.eval(1.),
                blue.eval(1.)
            );
        }
        if let Some(g) = &c.gain_map {
            eprintln!(
                "  gain map {:?} points over {:?}, after the warp {}",
                g.points, g.area, g.after_warp
            );
            eprintln!(
                "  its gains at the centre {:.3?}, top left {:.3?}, bottom right {:.3?}",
                g.gain(0.5, 0.5),
                g.gain(0.005, 0.005),
                g.gain(0.995, 0.995)
            );
        }
        let map = LensMap::new(&c, 6000., 4000., Amounts::FULL);
        eprintln!(
            "  fill on a 3:2 frame {:.4} (whole-frame corners {:.4})",
            map.fill,
            c.fill_scale_with(Amounts::FULL)
        );
    }
}
