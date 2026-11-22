//! RapidRAW 1.7.0's demosaic, on the CPU: RCD green with a green-checkerboard
//! debias and residual interpolation (RI + MLRI) for red and blue on Bayer
//! sensors, Markesteijn's 3-pass on X-Trans.
//!
//! FROM RAPIDRAW
//!
//! RapidRAW `v1.7.0` @ 00379017 (AGPL-3.0), first shipped in 041a4f43, with
//! 80f6e635 (residual interpolation) and dbb84900 (the green debias):
//! - `src-tauri/src/shaders/demosaic.wgsl`: every stage, in the order
//!   `decode/demosaic.rs` dispatches them, each over the whole tile and the
//!   output stage over its inner part, as the shader runs them. One GPU
//!   dispatch is one loop here; a stage that reads what another wrote reads
//!   it after the whole of that stage, as on the GPU.
//! - `src-tauri/src/decode/demosaic.rs`: `Mosaic::new` (what the mosaic must
//!   be), `markesteijn_hexagons` (dcraw's), the unpack's levels and the
//!   tiles' margins.
//!
//! RCD is Luis Sanz Rodríguez's (RawTherapee `rcd_demosaic.cc`); residual
//! interpolation and its minimised-Laplacian form are Kiku, Monno, Tanaka
//! and Okutomi's; Markesteijn's X-Trans demosaic is Frank Markesteijn's, as
//! written for dcraw (`xtrans_interpolate`).
//!
//! WHAT CHANGED IN THE COPY
//!
//! - It runs on the CPU, tiles in parallel (rayon), not on the GPU: the same
//!   picture on every machine, with or without a graphics adapter, and in the
//!   tests. The tile is smaller (`TILE_INNER`), the margins theirs.
//! - The output is the whole domain (rawler's active area), as rawler's own
//!   demosaic gives it; the caller crops it as it crops rawler's.
//! - The mosaic may hold float samples too (`RawImageData::Float`).
//! - The white level is the caller's: the app has replaced the photo's by the
//!   time it develops.

use std::ops::{Add, AddAssign, Div, Mul, Sub};
use std::sync::Mutex;

use rawler::RawImageData;
use rawler::imgop::{Point, Rect};
use rawler::rawimage::{RawImage, RawPhotometricInterpretation};
use rayon::prelude::*;

/// The output side of a tile, each way. Theirs is as large as the GPU allows
/// (1376 px); on the CPU a tile is one thread's work, and its scratch
/// (`Pattern::texel_bytes`) is held once per thread: 20 MB a Bayer tile.
const TILE_INNER: usize = 256;

/// The sensor's colour pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    Bayer,
    XTrans,
}

impl Pattern {
    fn period(self) -> i32 {
        match self {
            Pattern::Bayer => 2,
            Pattern::XTrans => 6,
        }
    }

    /// What a tile computes around the pixels it outputs: how far the
    /// stages read, added up along each axis. RapidRAW 00379017:
    /// decode/demosaic.rs, where Bayer's is 40, all the stages' reach added
    /// whichever axis it is along; along one axis it is 25 (`BAYER_MARGIN`).
    fn margin(self) -> usize {
        match self {
            Pattern::Bayer => BAYER_MARGIN,
            Pattern::XTrans => XTRANS_MARGIN,
        }
    }

    /// How many four-float planes the stages use (`b1`), and how many
    /// one-float ones (`b2`).
    fn planes(self) -> (usize, usize) {
        match self {
            Pattern::Bayer => (6, 1),
            Pattern::XTrans => (8, 8),
        }
    }
}

type Hexagons = [[[[i32; 2]; 8]; 3]; 3];

enum Samples<'a> {
    Integer(&'a [u16]),
    Float(&'a [f32]),
}

impl Samples<'_> {
    fn at(&self, i: usize) -> f32 {
        match self {
            Samples::Integer(data) => data[i] as f32,
            Samples::Float(data) => data[i],
        }
    }
}

/// A photo's mosaic, ready for demosaicing: its samples, colour pattern,
/// levels and white balance.
pub struct Mosaic<'a> {
    samples: Samples<'a>,
    stride: usize,
    domain: Rect,
    pattern: Pattern,
    colors: [[u8; 6]; 6],
    hex: Hexagons,
    solitary: [usize; 2],
    black: [f32; 4],
    scale: f32,
    wb: [f32; 3],
}

// RapidRAW 00379017: src-tauri/src/decode/demosaic.rs (dcraw's `xtrans_interpolate` tables).
fn markesteijn_hexagons(colors: &[[u8; 6]; 6]) -> Option<(Hexagons, [usize; 2])> {
    const ORTH: [i32; 12] = [1, 0, 0, 1, -1, 0, 0, -1, 1, 0, 0, 1];
    const PATT: [[i32; 16]; 2] = [
        [0, 1, 0, -1, 2, 0, -1, 0, 1, 1, 1, -1, 0, 0, 0, 0],
        [0, 1, 0, -2, 1, 0, -2, 0, 1, 1, -2, -2, 1, -1, -1, 1],
    ];
    let color = |row: i32, col: i32| colors[row.rem_euclid(6) as usize][col.rem_euclid(6) as usize];
    let mut hex: Hexagons = Default::default();
    let mut filled = [[false; 3]; 3];
    let mut solitary = None;
    for row in 0..3 {
        for col in 0..3 {
            let g = (color(row, col) == 1) as usize;
            let mut ng = 0;
            for d in (0..10).step_by(2) {
                if color(row + ORTH[d], col + ORTH[d + 2]) == 1 {
                    ng = 0;
                } else {
                    ng += 1;
                }
                if ng == 4 {
                    solitary = Some([row as usize, col as usize]);
                }
                if ng == g + 1 {
                    for c in 0..8 {
                        let v = ORTH[d] * PATT[g][c * 2] + ORTH[d + 1] * PATT[g][c * 2 + 1];
                        let h = ORTH[d + 2] * PATT[g][c * 2] + ORTH[d + 3] * PATT[g][c * 2 + 1];
                        hex[row as usize][col as usize][c ^ ((g * 2) & d)] = [h, v];
                    }
                    filled[row as usize][col as usize] = true;
                }
            }
        }
    }
    if !filled.iter().flatten().all(|&f| f) {
        return None;
    }
    Some((hex, solitary?))
}

impl<'a> Mosaic<'a> {
    /// The mosaic of `raw_image`, balanced by `wb` (red, green, blue, as
    /// rawler gives them) and scaled so that `white` is 1. `None` when this
    /// demosaic cannot take it: not a three-colour 2x2 or X-Trans pattern,
    /// an active area under 64 px, or a
    /// white balance that is not a positive number.
    // RapidRAW 00379017: src-tauri/src/decode/demosaic.rs (`Mosaic::new`).
    pub fn new(raw_image: &'a RawImage, wb: [f32; 3], white: f32) -> Option<Self> {
        let RawPhotometricInterpretation::Cfa(config) = &raw_image.photometric else {
            return None;
        };
        let samples = match &raw_image.data {
            RawImageData::Integer(data) if data.len() >= raw_image.width * raw_image.height => {
                Samples::Integer(data)
            }
            RawImageData::Float(data) if data.len() >= raw_image.width * raw_image.height => {
                Samples::Float(data)
            }
            _ => return None,
        };
        if raw_image.cpp != 1 || wb.iter().any(|c| !c.is_finite() || *c <= 0.0) {
            return None;
        }
        let pattern = match (config.cfa.width, config.cfa.height) {
            (2, 2) => Pattern::Bayer,
            (6, 6) => Pattern::XTrans,
            _ => return None,
        };
        let colors: [[u8; 6]; 6] =
            std::array::from_fn(|y| std::array::from_fn(|x| config.cfa.color_at(y, x) as u8));
        let counts = [0u8, 1, 2].map(|c| colors.iter().flatten().filter(|&&v| v == c).count());
        if colors.iter().flatten().any(|&c| c > 2) || counts.contains(&0) {
            return None;
        }
        let (hex, solitary) = match pattern {
            Pattern::Bayer => Default::default(),
            Pattern::XTrans if counts == [8, 20, 8] => markesteijn_hexagons(&colors)?,
            Pattern::XTrans => return None,
        };

        let full = Rect::new(Point::zero(), raw_image.dim());
        let domain = raw_image.active_area.unwrap_or(full);
        if domain.p.x + domain.d.w > raw_image.width
            || domain.p.y + domain.d.h > raw_image.height
            || domain.d.w < 64
            || domain.d.h < 64
        {
            return None;
        }

        let black = raw_image.blacklevel.as_bayer_array();
        Some(Self {
            samples,
            stride: raw_image.width,
            domain,
            pattern,
            colors,
            hex,
            solitary,
            black,
            scale: 1.0 / (white - black[0]).max(1.0),
            wb,
        })
    }

    pub fn pattern(&self) -> Pattern {
        self.pattern
    }

    /// What `develop` covers: the active area, in the sensor's pixels.
    pub fn domain(&self) -> Rect {
        self.domain
    }

    /// Demosaic the whole domain: camera RGB, row by row, white balanced,
    /// 1 at the white level, nothing below 0 and nothing clipped above.
    pub fn develop(&self) -> Vec<[f32; 3]> {
        let (width, height) = (self.domain.d.w, self.domain.d.h);
        let mut out = vec![[0.0f32; 3]; width * height];
        let columns: Vec<usize> = (0..width).step_by(TILE_INNER).collect();
        // One scratch per worker thread, made on its first tile and dropped
        // with this call.
        let scratches: Vec<Mutex<Option<Scratch>>> = (0..rayon::current_num_threads())
            .map(|_| Mutex::new(None))
            .collect();
        out.par_chunks_mut(TILE_INNER * width)
            .enumerate()
            .for_each(|(band, rows_out)| {
                let y0 = band * TILE_INNER;
                let rows = rows_out.len() / width;
                let blocks: Vec<(usize, Vec<[f32; 3]>)> = columns
                    .par_iter()
                    .map(|&x0| {
                        let worker = rayon::current_thread_index().unwrap_or(0) % scratches.len();
                        let mut held = scratches[worker].lock().unwrap_or_else(|e| e.into_inner());
                        let scratch = held.get_or_insert_with(|| Scratch::new(self.pattern));
                        let cols = TILE_INNER.min(width - x0);
                        (x0, self.tile(scratch, x0, y0, cols, rows))
                    })
                    .collect();
                for (x0, block) in blocks {
                    let cols = block.len() / rows;
                    for (row, line) in block.chunks_exact(cols).enumerate() {
                        rows_out[row * width + x0..row * width + x0 + cols].copy_from_slice(line);
                    }
                }
            });
        out
    }

    /// One tile: `cols` x `rows` output pixels at (`x0`, `y0`) in the
    /// domain, computed with the pattern's margin around them.
    fn tile(
        &self,
        scratch: &mut Scratch,
        x0: usize,
        y0: usize,
        cols: usize,
        rows: usize,
    ) -> Vec<[f32; 3]> {
        let margin = self.pattern.margin();
        let tile = Tile {
            m: self,
            ox: (self.domain.p.x + x0) as i32 - margin as i32,
            oy: (self.domain.p.y + y0) as i32 - margin as i32,
            w: (cols + 2 * margin) as i32,
            h: (rows + 2 * margin) as i32,
            margin: margin as i32,
            cols: cols as i32,
            rows: rows as i32,
        };
        scratch.fit(self.pattern, tile.texels());
        unpack(&tile, scratch);
        match self.pattern {
            Pattern::Bayer => bayer(&tile, scratch),
            Pattern::XTrans => markesteijn(&tile, scratch),
        }
    }
}

/// A tile's working buffers, kept from tile to tile on a thread. As
/// `decode/demosaic.rs` binds them (with `color` added): `cfa` the balanced mosaic, `b0` two
/// floats a texel, `b1` four-float planes, `b2` one-float planes, `words`
/// the X-Trans homogeneity counts.
struct Scratch {
    cfa: Vec<f32>,
    /// Each texel's colour: 0 red, 1 green, 2 blue.
    color: Vec<u8>,
    b0: Vec<f32>,
    b1: Vec<V4>,
    b2: Vec<f32>,
    words: Vec<u32>,
}

impl Scratch {
    fn new(pattern: Pattern) -> Self {
        let mut scratch = Self {
            cfa: Vec::new(),
            color: Vec::new(),
            b0: Vec::new(),
            b1: Vec::new(),
            b2: Vec::new(),
            words: Vec::new(),
        };
        let side = TILE_INNER + 2 * pattern.margin();
        scratch.fit(pattern, side * side);
        scratch
    }

    /// Room for a tile of `texels`. Every stage writes before it is read,
    /// so what a tile before left is never seen.
    fn fit(&mut self, pattern: Pattern, texels: usize) {
        let (planes4, planes1) = pattern.planes();
        self.cfa.resize(texels, 0.0);
        self.color.resize(texels, 0);
        self.b0.resize(2 * texels, 0.0);
        self.b1.resize(planes4 * texels, V4::ZERO);
        self.b2.resize(planes1 * texels, 0.0);
        if pattern == Pattern::XTrans {
            self.words.resize(texels, 0);
        }
    }
}

/// Where a tile is: its first texel in the sensor (`ox`, `oy`), its size,
/// and the output part inside it.
struct Tile<'m> {
    m: &'m Mosaic<'m>,
    ox: i32,
    oy: i32,
    w: i32,
    h: i32,
    margin: i32,
    cols: i32,
    rows: i32,
}

// RapidRAW 00379017: src-tauri/src/shaders/demosaic.wgsl (`color`, `row_phase`, `col_phase`).
impl Tile<'_> {
    fn texels(&self) -> usize {
        (self.w * self.h) as usize
    }

    #[inline]
    fn color(&self, x: i32, y: i32) -> u8 {
        self.m.colors[(self.oy + y).rem_euclid(6) as usize][(self.ox + x).rem_euclid(6) as usize]
    }

    #[inline]
    fn row_phase(&self, y: i32) -> i32 {
        (self.oy + y - self.m.solitary[0] as i32).rem_euclid(3)
    }

    #[inline]
    fn col_phase(&self, x: i32) -> i32 {
        (self.ox + x - self.m.solitary[1] as i32).rem_euclid(3)
    }
}

/// Four floats, as the shader's `vec4<f32>`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct V4([f32; 4]);

impl V4 {
    const ZERO: V4 = V4([0.0; 4]);
}

impl Add for V4 {
    type Output = V4;
    fn add(self, o: V4) -> V4 {
        V4(std::array::from_fn(|i| self.0[i] + o.0[i]))
    }
}

impl AddAssign for V4 {
    fn add_assign(&mut self, o: V4) {
        *self = *self + o;
    }
}

impl Sub for V4 {
    type Output = V4;
    fn sub(self, o: V4) -> V4 {
        V4(std::array::from_fn(|i| self.0[i] - o.0[i]))
    }
}

impl Mul<f32> for V4 {
    type Output = V4;
    fn mul(self, k: f32) -> V4 {
        V4(self.0.map(|v| v * k))
    }
}

impl Mul for V4 {
    type Output = V4;
    fn mul(self, o: V4) -> V4 {
        V4(std::array::from_fn(|i| self.0[i] * o.0[i]))
    }
}

impl Div<f32> for V4 {
    type Output = V4;
    fn div(self, k: f32) -> V4 {
        V4(self.0.map(|v| v / k))
    }
}

/// WGSL's `clamp`: `min(max(v, lo), hi)`, which never panics.
#[inline]
fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    v.max(lo).min(hi)
}

#[inline]
fn sq(v: f32) -> f32 {
    v * v
}

// RapidRAW 00379017: src-tauri/src/shaders/demosaic.wgsl (`fold`).
/// `v` mirrored back into `lo..hi` at the borders, keeping its place in the
/// colour pattern (`period`).
fn fold(v: i32, lo: i32, hi: i32, period: i32) -> i32 {
    let mut r = v;
    if v < lo {
        r = 2 * lo - v;
        r -= (r - v).rem_euclid(period);
        if r < lo {
            r += period;
        }
    } else if v >= hi {
        r = 2 * (hi - 1) - v;
        r += (v - r).rem_euclid(period);
        if r >= hi {
            r -= period;
        }
    }
    r.clamp(lo, hi - 1)
}

// RapidRAW 00379017: src-tauri/src/shaders/demosaic.wgsl (`unpack`).
/// The tile's samples: black taken off and clipped at 0, scaled to 1 at
/// white, white balanced.
fn unpack(t: &Tile, s: &mut Scratch) {
    let m = t.m;
    let d = m.domain;
    let (lo_x, hi_x) = (d.p.x as i32, (d.p.x + d.d.w) as i32);
    let (lo_y, hi_y) = (d.p.y as i32, (d.p.y + d.d.h) as i32);
    let period = m.pattern.period();
    each_xy(t, 0, |x, y, i| {
        let (x, y) = (x as i32, y as i32);
        let ax = fold(t.ox + x, lo_x, hi_x, period);
        let ay = fold(t.oy + y, lo_y, hi_y, period);
        let value = m.samples.at(ay as usize * m.stride + ax as usize);
        let black = m.black[((ay & 1) * 2 + (ax & 1)) as usize];
        let color = t.color(x, y);
        s.color[i] = color;
        s.cfa[i] = (value - black).max(0.0) * m.scale * m.wb[color as usize];
    });
}

// ---------------------------------------------------------------------------
// Bayer: RCD green, debias, RI + MLRI red and blue.
// RapidRAW 00379017: src-tauri/src/shaders/demosaic.wgsl (`rcd_*`, `rb_*`).
// ---------------------------------------------------------------------------

const EPS: f32 = 1e-5;
const EPSSQ: f32 = 1e-10;
const DEBIAS_RADIUS: i32 = 3;
const DEBIAS_NORM: f32 = 1.0 / 64.0;
const DEBIAS_LIMIT: f32 = 0.04;
const RB_EPS: f32 = 1e-5;
const RB_SHOT: f32 = 1e-4;
const RB_SIGMA: f32 = 1.5;
const RB_TINY: f32 = 1e-20;
const RB_RADIUS: i32 = 4;
const RB_AREA: f32 = 81.0;

// The four-float planes, as the shader names them; its STATS planes are
// not needed here, the box sums keep their columns themselves.
const STATE: usize = 0;
const COEF: usize = 2;
const CRIT: usize = 5;
const SMOOTH: usize = COEF;
const TENTATIVE: usize = STATE;
const REFINED: usize = 1;

fn gauss(s: i32, sigma: f32) -> f32 {
    let f = s as f32 / sigma;
    (-0.5 * f * f).exp()
}

/// The binomial weights of the debias, (1, 6, 15, 20, 15, 6, 1).
fn binomial() -> [f32; (2 * DEBIAS_RADIUS + 1) as usize] {
    let mut weights = [0.0; (2 * DEBIAS_RADIUS + 1) as usize];
    let mut weight = 1.0;
    for (k, w) in weights.iter_mut().enumerate() {
        *w = weight;
        weight *= (2 * DEBIAS_RADIUS - k as i32) as f32 / (k as f32 + 1.0);
    }
    weights
}

/// Run `f(i)` for every texel at least `ix` from the tile's left and right
/// edges and `iy` from its top and bottom, row by row: `i` is its index.
#[inline(always)]
fn each(t: &Tile, (ix, iy): (usize, usize), mut f: impl FnMut(usize)) {
    let (w, h) = (t.w as usize, t.h as usize);
    for y in iy..h - iy {
        let row = y * w;
        for x in ix..w - ix {
            f(row + x);
        }
    }
}

/// The sums over the (2 `RB_RADIUS` + 1)² box around every texel at
/// `inset` (as `each`), handed to `put` row by row; `add(sum, j, sign)`
/// adds texel `j`'s values to `sum`, times `sign`. The shader's two
/// passes of nine taps, as one: sliding sums down the columns and along the
/// rows, in f64, so no rounding builds up as they slide.
#[inline(always)]
fn box_sums<const CH: usize>(
    t: &Tile,
    (ix, iy): (usize, usize),
    add: impl Fn(&mut [f64; CH], usize, f64),
    mut put: impl FnMut(usize, [f64; CH]),
) {
    let (w, h) = (t.w as usize, t.h as usize);
    let r = RB_RADIUS as usize;
    let (x0, x1) = (ix - r, w - ix + r);
    let mut columns = vec![[0.0f64; CH]; w];
    for (x, column) in columns.iter_mut().enumerate().take(x1).skip(x0) {
        for y in iy - r..=iy + r {
            add(column, y * w + x, 1.0);
        }
    }
    for y in iy..h - iy {
        if y > iy {
            let (enter, leave) = ((y + r) * w, (y - r - 1) * w);
            for (x, column) in columns.iter_mut().enumerate().take(x1).skip(x0) {
                add(column, enter + x, 1.0);
                add(column, leave + x, -1.0);
            }
        }
        let mut sum = [0.0f64; CH];
        for column in &columns[ix - r..=ix + r] {
            for c in 0..CH {
                sum[c] += column[c];
            }
        }
        put(y * w + ix, sum);
        for x in ix + 1..w - ix {
            for c in 0..CH {
                sum[c] += columns[x + r][c] - columns[x - r - 1][c];
            }
            put(y * w + x, sum);
        }
    }
}

// Each stage runs on the texels the stages after it read, no further, along
// each axis: (left and right, top and bottom) from the tile's edge. The
// output is `Pattern::margin` in; the first stage reads the tile's edge and
// nothing beyond it, so nothing is clamped and a tile's pixels are the
// untiled picture's.
const HIGHPASS_AT: (usize, usize) = (3, 3);
const LOWPASS_AT: (usize, usize) = (3, 3);
const VH_AT: (usize, usize) = (4, 4);
const GREEN_AT: (usize, usize) = (5, 5);
const DEBIAS_H_AT: (usize, usize) = (8, 5);
const DEBIAS_V_AT: (usize, usize) = (8, 8);
const PREPARE_AT: (usize, usize) = (10, 10);
const COEF_AT: (usize, usize) = (14, 14);
const TENTATIVE_AT: (usize, usize) = (18, 18);
const RESIDUAL_AT: (usize, usize) = (19, 19);
const CRIT_AT: (usize, usize) = (20, 20);
const SMOOTH_AT: (usize, usize) = (25, 20);
/// The output: `rb_out` reads five rows of `SMOOTH` each way.
const BAYER_MARGIN: usize = 25;

fn bayer(t: &Tile, s: &mut Scratch) -> Vec<[f32; 3]> {
    let Scratch {
        cfa,
        b0,
        b1,
        b2,
        color,
        ..
    } = s;
    let w = t.w as usize;
    let tex = t.texels();
    let plane = |p: usize| p * tex;

    // rcd_vh. The squared high-pass each way, once a texel (plane 0, free
    // until rb_prepare), then summed over three.
    let hp = plane(0);
    each(t, HIGHPASS_AT, |i| {
        let c = |k: isize| cfa[(i as isize + k) as usize];
        let v = sq(
            c(-3 * w as isize) - c(-(w as isize)) - c(w as isize) + c(3 * w as isize)
                - 3.0 * (c(-2 * w as isize) + c(2 * w as isize))
                + 6.0 * c(0),
        );
        let hz = sq(c(-3) - c(-1) - c(1) + c(3) - 3.0 * (c(-2) + c(2)) + 6.0 * c(0));
        b1[hp + i].0[0] = v;
        b1[hp + i].0[1] = hz;
    });
    each(t, LOWPASS_AT, |i| {
        b0[i * 2 + 1] = cfa[i]
            + 0.5 * (cfa[i - w] + cfa[i + w] + cfa[i - 1] + cfa[i + 1])
            + 0.25 * (cfa[i - w - 1] + cfa[i - w + 1] + cfa[i + w - 1] + cfa[i + w + 1]);
    });
    each(t, VH_AT, |i| {
        let v = EPSSQ.max(b1[hp + i - w].0[0] + b1[hp + i].0[0] + b1[hp + i + w].0[0]);
        let hz = EPSSQ.max(b1[hp + i - 1].0[1] + b1[hp + i].0[1] + b1[hp + i + 1].0[1]);
        b0[i * 2] = v / (v + hz);
    });

    // rcd_green
    let refine = |center: f32, around: f32| {
        clamp(
            if (0.5 - center).abs() < (0.5 - around).abs() {
                around
            } else {
                center
            },
            0.0,
            1.0,
        )
    };
    each(t, GREEN_AT, |i| {
        let c0 = cfa[i];
        if color[i] == 1 {
            b2[i] = c0;
            return;
        }
        let (w1, w2, w3, w4) = (w, 2 * w, 3 * w, 4 * w);
        let n_grad = EPS
            + (cfa[i - w1] - cfa[i + w1]).abs()
            + (c0 - cfa[i - w2]).abs()
            + (cfa[i - w1] - cfa[i - w3]).abs()
            + (cfa[i - w2] - cfa[i - w4]).abs();
        let s_grad = EPS
            + (cfa[i + w1] - cfa[i - w1]).abs()
            + (c0 - cfa[i + w2]).abs()
            + (cfa[i + w1] - cfa[i + w3]).abs()
            + (cfa[i + w2] - cfa[i + w4]).abs();
        let w_grad = EPS
            + (cfa[i - 1] - cfa[i + 1]).abs()
            + (c0 - cfa[i - 2]).abs()
            + (cfa[i - 1] - cfa[i - 3]).abs()
            + (cfa[i - 2] - cfa[i - 4]).abs();
        let e_grad = EPS
            + (cfa[i + 1] - cfa[i - 1]).abs()
            + (c0 - cfa[i + 2]).abs()
            + (cfa[i + 1] - cfa[i + 3]).abs()
            + (cfa[i + 2] - cfa[i + 4]).abs();

        let lowpass = |j: usize| b0[j * 2 + 1];
        let l = lowpass(i);
        let n_est = cfa[i - w1] * (l + l) / (EPS + l + lowpass(i - w2));
        let s_est = cfa[i + w1] * (l + l) / (EPS + l + lowpass(i + w2));
        let w_est = cfa[i - 1] * (l + l) / (EPS + l + lowpass(i - 2));
        let e_est = cfa[i + 1] * (l + l) / (EPS + l + lowpass(i + 2));

        let v_est = (s_grad * n_est + n_grad * s_est) / (n_grad + s_grad);
        let h_est = (w_grad * e_est + e_grad * w_est) / (e_grad + w_grad);
        let vh = |j: usize| b0[j * 2];
        let disc = refine(
            vh(i),
            0.25 * (vh(i - w - 1) + vh(i - w + 1) + vh(i + w - 1) + vh(i + w + 1)),
        );
        b2[i] = v_est * (1.0 - disc) + h_est * disc;
    });

    // rcd_debias_h
    let weights = binomial();
    let radius = DEBIAS_RADIUS as usize;
    each(t, DEBIAS_H_AT, |i| {
        let mut modulated = 0.0;
        let mut level = 0.0;
        for (k, weight) in weights.iter().enumerate() {
            let j = i + k - radius;
            let g = b2[j];
            modulated += weight * if color[j] == 1 { -g } else { g };
            level += weight * g;
        }
        b0[i * 2] = modulated * DEBIAS_NORM;
        b0[i * 2 + 1] = level * DEBIAS_NORM;
    });

    // rcd_debias_v
    each(t, DEBIAS_V_AT, |i| {
        if color[i] == 1 {
            return;
        }
        let mut modulated = 0.0;
        let mut level = 0.0;
        for (k, weight) in weights.iter().enumerate() {
            let j = i + k * w - radius * w;
            modulated += weight * b0[j * 2];
            level += weight * b0[j * 2 + 1];
        }
        let limit = DEBIAS_LIMIT * level * DEBIAS_NORM;
        b2[i] -= clamp(2.0 * modulated * DEBIAS_NORM, -limit, limit);
    });

    // rb_prepare
    let (state0, state1) = (plane(STATE), plane(STATE + 1));
    each(t, PREPARE_AT, |i| {
        if color[i] == 1 {
            b1[state0 + i] = V4::ZERO;
            b1[state1 + i] = V4::ZERO;
            return;
        }
        let g = b2[i];
        let c = cfa[i];
        let lg = 4.0 * g - b2[i - 2] - b2[i + 2] - b2[i - 2 * w] - b2[i + 2 * w];
        let lx = 4.0 * c - cfa[i - 2] - cfa[i + 2] - cfa[i - 2 * w] - cfa[i + 2 * w];
        b1[state0 + i] = V4([1.0, g, c, g * g]);
        b1[state1 + i] = V4([c * c, g * c, lg * lx, lg * lg]);
    });

    // rb_stats + rb_coef: each colour's statistics over the 9 x 9 box, red
    // sites apart from blue ones, and its two fits.
    let coef = plane(COEF);
    {
        // The statistics' planes come before the fits'.
        let (state, fits) = b1.split_at_mut(coef);
        let color = &*color;
        box_sums(
            t,
            COEF_AT,
            // Red sites into the first eight sums, blue into the last;
            // green sites hold nothing.
            |sum: &mut [f64; 16], j, sign| {
                let half = match color[j] {
                    0 => 0,
                    2 => 8,
                    _ => return,
                };
                let (a, b) = (state[state0 + j].0, state[state1 + j].0);
                for c in 0..4 {
                    sum[half + c] += sign * a[c] as f64;
                    sum[half + 4 + c] += sign * b[c] as f64;
                }
            },
            |i, m| {
                let mean = |k: usize| {
                    V4(std::array::from_fn(|c| {
                        (m[4 * k + c] / RB_AREA as f64) as f32
                    }))
                };
                let (r_ri, r_ml) = color_coef(mean(0), mean(1));
                let (b_ri, b_ml) = color_coef(mean(2), mean(3));
                fits[i] = V4([r_ri[0], r_ri[1], b_ri[0], b_ri[1]]);
                fits[tex + i] = V4([r_ml[0], r_ml[1], r_ml[2], b_ml[0]]);
                fits[2 * tex + i] = V4([b_ml[1], b_ml[2], 0.0, 0.0]);
            },
        );
    }

    // rb_average + rb_tentative: the fits summed over the 9 x 9 box, then
    // each colour's two candidates.
    let tentative = plane(TENTATIVE);
    {
        // The candidates' plane comes before the fits'.
        let (candidates, fits) = b1.split_at_mut(coef);
        let green = &*b2;
        box_sums(
            t,
            TENTATIVE_AT,
            |sum: &mut [f64; 10], j, sign| {
                let (a, b, c) = (fits[j].0, fits[tex + j].0, fits[2 * tex + j].0);
                let values = [a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3], c[0], c[1]];
                for (s, v) in sum.iter_mut().zip(values) {
                    *s += sign * v as f64;
                }
            },
            |i, s| {
                let s = s.map(|v| v as f32);
                let g = green[i];
                candidates[tentative + i] = V4([
                    (s[0] * g + s[1]) / RB_AREA,
                    (s[4] * g + s[5]) / s[6],
                    (s[2] * g + s[3]) / RB_AREA,
                    (s[7] * g + s[8]) / s[9],
                ]);
            },
        );
    }

    // rb_residual
    let (refined, crit) = (plane(REFINED), plane(CRIT));
    each(t, RESIDUAL_AT, |i| {
        let mut resid = V4::ZERO;
        let mut base = [0.0f32; 2];
        for dy in [-1isize, 0, 1] {
            for dx in [-1isize, 0, 1] {
                let j = (i as isize + dy * w as isize + dx) as usize;
                let col = color[j];
                if col == 1 {
                    continue;
                }
                let k = if dx == 0 && dy == 0 {
                    1.0
                } else if dx == 0 || dy == 0 {
                    0.5
                } else {
                    0.25
                };
                let c = cfa[j];
                let tq = b1[tentative + j].0;
                if col == 0 {
                    resid += V4([c - tq[0], c - tq[1], 0.0, 0.0]) * k;
                    base[0] += k * (c - b2[j]);
                } else {
                    resid += V4([0.0, 0.0, c - tq[2], c - tq[3]]) * k;
                    base[1] += k * (c - b2[j]);
                }
            }
        }
        let tp = b1[tentative + i];
        let g = b2[i];
        b1[refined + i] = tp + resid;
        b1[crit + i] = tp - V4([g + base[0], g + base[0], g + base[1], g + base[1]]);
    });

    // rb_smooth. The criterion once a texel (plane COEF + 1, free since the
    // candidates), then its horizontal Gaussian.
    let weighted = plane(COEF + 1);
    each(t, CRIT_AT, |i| {
        let d = b1[crit + i];
        let gx = abs4(b1[crit + i + 1] - b1[crit + i - 1]);
        let gy = abs4(b1[crit + i + w] - b1[crit + i - w]);
        b1[weighted + i] = d * d * (gx + gy) * 0.5;
    });
    let kernel: [f32; 11] = std::array::from_fn(|k| gauss(k as i32 - 5, RB_SIGMA));
    let total: f32 = kernel.iter().sum();
    let smooth = plane(SMOOTH);
    each(t, SMOOTH_AT, |i| {
        let mut acc = V4::ZERO;
        for (k, g) in kernel.iter().enumerate() {
            acc += b1[weighted + i + k - 5] * *g;
        }
        b1[smooth + i] = acc / total;
    });

    // rb_out
    let mut out = vec![[0.0f32; 3]; (t.cols * t.rows) as usize];
    let m = t.margin as usize;
    for iy in 0..t.rows as usize {
        for ix in 0..t.cols as usize {
            let i = (m + iy) * w + m + ix;
            let mut acc = V4::ZERO;
            for (k, g) in kernel.iter().enumerate() {
                acc += b1[smooth + i + k * w - 5 * w] * *g;
            }
            let wt = V4((acc / total).0.map(|v| 1.0 / (v + RB_TINY)));
            let v = (b1[refined + i] * wt).0;
            let wt = wt.0;
            let mut r = (v[0] + v[1]) / (wt[0] + wt[1]);
            let mut b = (v[2] + v[3]) / (wt[2] + wt[3]);
            let col = color[i];
            if col == 0 {
                r = cfa[i];
            }
            if col == 2 {
                b = cfa[i];
            }
            out[iy * t.cols as usize + ix] = [r, b2[i], b].map(|v| v.max(0.0));
        }
    }
    out
}

fn abs4(v: V4) -> V4 {
    V4(v.0.map(f32::abs))
}

// RapidRAW 00379017: src-tauri/src/shaders/demosaic.wgsl (`color_coef`).
/// One colour's two fits over a window's means `s0` = (n, g, x, g²) and
/// `s1` = (x², g·x, Lg·Lx, Lg²): residual interpolation's (a, b), and the
/// minimised-Laplacian one, weighted by its fit, (wt·a, wt·b, wt).
fn color_coef(s0: V4, s1: V4) -> ([f32; 2], [f32; 3]) {
    let [s0x, s0y, s0z, s0w] = s0.0;
    let [s1x, s1y, s1z, s1w] = s1.0;
    let n = s0x.max(1e-12);
    let mg = s0y / n;
    let mx = s0z / n;
    let a = (s1y / n - mg * mx) / (s0w / n - mg * mg + RB_EPS + RB_SHOT * mg);
    let b = mx - a * mg;
    let a2 = s1z / (s1w + RB_EPS + RB_SHOT * 20.0 * s0y);
    let b2 = mx - a2 * mg;
    let e = (s1x - 2.0 * a2 * s1y - 2.0 * b2 * s0z
        + a2 * a2 * s0w
        + 2.0 * a2 * b2 * s0y
        + b2 * b2 * s0x)
        / n;
    let wt = 1.0 / e.max(1e-10);
    ([a, b], [wt * a2, wt * b2, wt])
}

// ---------------------------------------------------------------------------
// X-Trans: Markesteijn 3-pass.
// RapidRAW 00379017: src-tauri/src/shaders/demosaic.wgsl (`xt_*`).
// ---------------------------------------------------------------------------

/// `each`, with the texel's place in the tile: `f(x, y, i)`.
#[inline(always)]
fn each_xy(t: &Tile, inset: usize, mut f: impl FnMut(usize, usize, usize)) {
    let (w, h) = (t.w as usize, t.h as usize);
    for y in inset..h - inset {
        let row = y * w;
        for x in inset..w - inset {
            f(x, y, row + x);
        }
    }
}

// Each stage runs as far from the tile's edge as the stages after it read
// (the hexagons reach 3, 2 x a neighbour 2, `redblue` 3), as for Bayer. Along
// one axis or both, the reach adds up to RapidRAW's margin of 32.
const XT_GREEN_AT: usize = 3;
const XT_PASS_1_AT: [usize; 4] = [5, 8, 10, 12];
const XT_PASS_2_AT: [usize; 4] = [14, 17, 19, 21];
const XT_PASS_3_AT: [usize; 3] = [23, 26, 28];
const XT_DRV_AT: usize = 29;
const XT_HOMO_AT: usize = 30;
const XTRANS_MARGIN: usize = 32;
/// The directions Markesteijn's passes interpolate along, each texel's
/// side by side in `b1` and `b2`.
const DIRECTIONS: usize = 8;

fn markesteijn(t: &Tile, s: &mut Scratch) -> Vec<[f32; 3]> {
    let Scratch {
        cfa,
        b0,
        b1,
        b2,
        words,
        color,
    } = s;
    let (w, h) = (t.w as usize, t.h as usize);
    // Each texel's eight directions side by side (`DIRECTIONS` four-float
    // values, then as many floats): every stage reads a texel's directions
    // together.
    let at = |i: usize, d: usize| i * DIRECTIONS + d;
    // The pattern's phases by row and by column, and the hexagons as index
    // steps, so no texel works out its place in the pattern.
    let row3: Vec<usize> = (0..h)
        .map(|y| (t.oy + y as i32).rem_euclid(3) as usize)
        .collect();
    let col3: Vec<usize> = (0..w)
        .map(|x| (t.ox + x as i32).rem_euclid(3) as usize)
        .collect();
    let row_phase: Vec<bool> = (0..h).map(|y| t.row_phase(y as i32) == 0).collect();
    let col_phase: Vec<bool> = (0..w).map(|x| t.col_phase(x as i32) == 0).collect();
    let hex: [[[isize; 8]; 3]; 3] = std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            std::array::from_fn(|k| {
                let [dx, dy] = t.m.hex[r][c][k];
                dy as isize * w as isize + dx as isize
            })
        })
    });
    let step = |i: usize, d: isize, k: isize| (i as isize + k * d) as usize;

    // xt_minmax
    each_xy(t, XT_GREEN_AT, |x, y, i| {
        if color[i] == 1 {
            return;
        }
        let hx = &hex[row3[y]][col3[x]];
        let mut lo = f32::MAX;
        let mut hi = 0.0f32;
        for d in &hx[..6] {
            let v = cfa[step(i, *d, 1)];
            lo = lo.min(v);
            hi = hi.max(v);
        }
        b0[i * 2] = lo;
        b0[i * 2 + 1] = hi;
    });

    // xt_green
    each_xy(t, XT_GREEN_AT, |x, y, i| {
        let f = color[i] as usize;
        let v = cfa[i];
        let mut init = V4::ZERO;
        init.0[f] = v;
        for d in 0..4 {
            b1[at(i, d)] = init;
        }
        if f == 1 {
            return;
        }
        let hx = &hex[row3[y]][col3[x]];
        let p = |k: usize, m: isize| cfa[step(i, hx[k], m)];
        let est = [
            (174.0 * (p(1, 1) + p(0, 1)) - 46.0 * (p(1, 2) + p(0, 2))) / 256.0,
            (223.0 * p(3, 1) + 33.0 * p(2, 1) + 92.0 * (v - p(2, -1))) / 256.0,
            (164.0 * p(4, 1) + 92.0 * p(4, -2) + 33.0 * (2.0 * v - p(4, 3) - p(4, -3))) / 256.0,
            (164.0 * p(5, 1) + 92.0 * p(5, -2) + 33.0 * (2.0 * v - p(5, 3) - p(5, -3))) / 256.0,
        ];
        let (lo, hi) = (b0[i * 2], b0[i * 2 + 1]);
        let swap = row_phase[y] as usize;
        for (k, e) in est.into_iter().enumerate() {
            b1[at(i, k ^ swap)].0[1] = clamp(e, lo, hi);
        }
    });

    let solitary_axis = |b1: &[V4], i: usize, d: usize, unit: usize| {
        let mut out = [0.0f32; 3];
        let g0 = b1[at(i, d)].0[1];
        for dist in 1..=2 {
            let (fore, back) = (at(i + unit * dist, d), at(i - unit * dist, d));
            let c = color[i + unit * dist] as usize;
            let (gp, gm) = (b1[fore].0[1], b1[back].0[1]);
            let (cp, cm) = (b1[fore].0[c], b1[back].0[c]);
            let g = 2.0 * g0 - gp - gm;
            out[c >> 1] = (g + cp + cm) * 0.5;
            out[2] += sq(gp - gm - cp + cm) + sq(g);
        }
        out
    };

    let solitary = |b1: &mut [V4], base: usize, inset: usize| {
        each_xy(t, inset, |x, y, i| {
            if !row_phase[y] || !col_phase[x] {
                return;
            }
            for k in 0..4 {
                let d = base + k;
                let rb = match k {
                    0 => solitary_axis(b1, i, d, 1),
                    1 => solitary_axis(b1, i, d, w),
                    _ => {
                        let h = solitary_axis(b1, i, d, 1);
                        let v = solitary_axis(b1, i, d, w);
                        if h[2] < v[2] { h } else { v }
                    }
                };
                let s = at(i, d);
                b1[s].0[0] = rb[0];
                b1[s].0[2] = rb[1];
            }
        });
    };

    let redblue = |b1: &mut [V4], base: usize, inset: usize| {
        each_xy(t, inset, |_, y, i| {
            let own = color[i] as usize;
            if own == 1 {
                return;
            }
            let f = 2 - own;
            let vertical = !row_phase[y];
            let (cu, hu) = if vertical { (w, 3) } else { (1, 3 * w) };
            for k in 0..4usize {
                let d = base + k;
                let g0 = b1[at(i, d)].0[1];
                let green = |j: usize| b1[at(j, d)].0[1];
                let mut use_c = k > 1 || ((if vertical { k } else { k ^ 1 }) & 1) != 0;
                if !use_c {
                    use_c = (g0 - green(i + cu)).abs() + (g0 - green(i - cu)).abs()
                        < 2.0 * ((g0 - green(i + hu)).abs() + (g0 - green(i - hu)).abs());
                }
                let u = if use_c { cu } else { hu };
                let value = (b1[at(i + u, d)].0[f] + b1[at(i - u, d)].0[f] + 2.0 * g0
                    - green(i + u)
                    - green(i - u))
                    * 0.5;
                b1[at(i, d)].0[f] = value;
            }
        });
    };

    let greens = |b1: &mut [V4], base: usize, inset: usize| {
        each_xy(t, inset, |x, y, i| {
            if color[i] != 1 || row_phase[y] || col_phase[x] {
                return;
            }
            let hx = &hex[row3[y]][col3[x]];
            for k in 0..4 {
                let d = base + k;
                let (a, b) = (hx[2 * k], hx[2 * k + 1]);
                let g0 = b1[at(i, d)].0[1];
                let ca = b1[at(step(i, a, 1), d)].0;
                let cb = b1[at(step(i, b, 1), d)].0;
                let (r, bl) = if a + b != 0 {
                    let g = 3.0 * g0 - 2.0 * ca[1] - cb[1];
                    (
                        (g + 2.0 * ca[0] + cb[0]) / 3.0,
                        (g + 2.0 * ca[2] + cb[2]) / 3.0,
                    )
                } else {
                    let g = 2.0 * g0 - ca[1] - cb[1];
                    ((g + ca[0] + cb[0]) * 0.5, (g + ca[2] + cb[2]) * 0.5)
                };
                b1[at(i, d)].0[0] = r;
                b1[at(i, d)].0[2] = bl;
            }
        });
    };

    let recalc = |b1: &[V4], b2: &mut [f32], src: usize, inset: usize| {
        each_xy(t, inset, |x, y, i| {
            let own = color[i] as usize;
            if own == 1 {
                return;
            }
            for k in 0..4 {
                b2[at(i, k)] = b1[at(i, src + k)].0[1];
            }
            let (lo, hi) = (b0[i * 2], b0[i * 2 + 1]);
            let swap = row_phase[y] as usize;
            let hx = &hex[row3[y]][col3[x]];
            for (e, a) in hx.iter().enumerate().take(6).skip(3) {
                let tt = (e - 2) ^ swap;
                let d = src + tt;
                let (back, fore) = (at(step(i, *a, -2), d), at(step(i, *a, 1), d));
                let value =
                    b1[back].0[1] + 2.0 * b1[fore].0[1] - b1[back].0[own] - 2.0 * b1[fore].0[own]
                        + 3.0 * b1[at(i, d)].0[own];
                b2[at(i, tt)] = clamp(value / 3.0, lo, hi);
            }
        });
    };

    let commit = |b1: &mut [V4], b2: &[f32], src: usize, inset: usize| {
        each_xy(t, inset, |_, _, i| {
            let refined = color[i] != 1;
            for k in 0..4 {
                let mut value = b1[at(i, src + k)];
                if refined {
                    value.0[1] = b2[at(i, k)];
                }
                b1[at(i, 4 + k)] = value;
            }
        });
    };

    let [a, b, c, d] = XT_PASS_1_AT;
    solitary(b1, 0, a);
    redblue(b1, 0, b);
    greens(b1, 0, c);
    recalc(b1, b2, 0, d);
    commit(b1, b2, 0, d);
    let [a, b, c, d] = XT_PASS_2_AT;
    solitary(b1, 4, a);
    redblue(b1, 4, b);
    greens(b1, 4, c);
    recalc(b1, b2, 4, d);
    commit(b1, b2, 4, d);
    let [a, b, c] = XT_PASS_3_AT;
    solitary(b1, 4, a);
    redblue(b1, 4, b);
    greens(b1, 4, c);

    // xt_drv
    let yuv = |v: V4| {
        let [r, g, b, _] = v.0;
        let y = 0.2627 * r + 0.6780 * g + 0.0593 * b;
        [y, (b - y) * 0.56433, (r - y) * 0.67815]
    };
    let steps = [1, w, w + 1, w - 1];
    each_xy(t, XT_DRV_AT, |_, _, i| {
        for d in 0..DIRECTIONS {
            let o = steps[d & 3];
            let c = yuv(b1[at(i, d)]);
            let a = yuv(b1[at(i + o, d)]);
            let b = yuv(b1[at(i - o, d)]);
            let t3: [f32; 3] = std::array::from_fn(|k| 2.0 * c[k] - a[k] - b[k]);
            b2[at(i, d)] = t3.iter().map(|v| v * v).sum();
        }
    });

    // xt_homo
    each_xy(t, XT_HOMO_AT, |_, _, i| {
        let own = &b2[at(i, 0)..at(i, DIRECTIONS)];
        let tr = 8.0 * own.iter().copied().fold(f32::MAX, f32::min);
        let mut n = [0u32; DIRECTIONS];
        for row in [i - w, i, i + w] {
            for j in row - 1..=row + 1 {
                for (n, v) in n.iter_mut().zip(&b2[at(j, 0)..at(j, DIRECTIONS)]) {
                    *n += (*v <= tr) as u32;
                }
            }
        }
        words[i] = n
            .iter()
            .enumerate()
            .fold(0, |packed, (d, n)| packed | n << (4 * d));
    });

    // xt_out. The 5 x 5 counts in two passes, along the rows then down:
    // every direction's count is a byte (at most 25 x 9), so the sums are
    // the shader's, exactly.
    let mut out = vec![[0.0f32; 3]; (t.cols * t.rows) as usize];
    let m = t.margin as usize;
    let (cols, rows) = (t.cols as usize, t.rows as usize);
    let mut across = vec![(0u32, 0u32); (rows + 4) * cols];
    for (r, line) in across.chunks_exact_mut(cols).enumerate() {
        let row = (m + r - 2) * w + m;
        for (ix, sums) in line.iter_mut().enumerate() {
            for word in &words[row + ix - 2..=row + ix + 2] {
                sums.0 += word & 0x0f0f_0f0f;
                sums.1 += (word >> 4) & 0x0f0f_0f0f;
            }
        }
    }
    for iy in 0..rows {
        for ix in 0..cols {
            let i = (m + iy) * w + m + ix;
            let mut even = 0u32;
            let mut odd = 0u32;
            for dy in 0..5 {
                let (e, o) = across[(iy + dy) * cols + ix];
                even += e;
                odd += o;
            }
            let mut hm = [0u32; 8];
            let mut top = 0u32;
            for (d, h) in hm.iter_mut().enumerate() {
                *h = ((if d & 1 == 1 { odd } else { even }) >> (8 * (d >> 1))) & 0xff;
                top = top.max(*h);
            }
            top -= top >> 3;
            for d in 0..4 {
                if hm[d] < hm[d + 4] {
                    hm[d] = 0;
                } else if hm[d] > hm[d + 4] {
                    hm[d + 4] = 0;
                }
            }
            let mut sum = [0.0f32; 3];
            let mut n = 0.0f32;
            for (d, h) in hm.iter().enumerate() {
                if *h >= top {
                    let c = b1[at(i, d)].0;
                    for k in 0..3 {
                        sum[k] += c[k];
                    }
                    n += 1.0;
                }
            }
            let n = n.max(1.0);
            out[iy * t.cols as usize + ix] = sum.map(|v| (v / n).max(0.0));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RGGB: [[u8; 2]; 2] = [[0, 1], [1, 2]];
    // Fujifilm X-Trans (X-T1 and after), as dcraw lists it.
    const XTRANS: [[u8; 6]; 6] = [
        [1, 1, 0, 1, 1, 2],
        [1, 1, 2, 1, 1, 0],
        [2, 0, 1, 0, 2, 1],
        [1, 1, 2, 1, 1, 0],
        [1, 1, 0, 1, 1, 2],
        [0, 2, 1, 2, 0, 1],
    ];

    fn mosaic_of<'a>(
        data: &'a [f32],
        w: usize,
        h: usize,
        colors: [[u8; 6]; 6],
        pattern: Pattern,
    ) -> Mosaic<'a> {
        let (hex, solitary) = match pattern {
            Pattern::Bayer => Default::default(),
            Pattern::XTrans => markesteijn_hexagons(&colors).expect("X-Trans hexagons"),
        };
        Mosaic {
            samples: Samples::Float(data),
            stride: w,
            domain: Rect::new(Point::zero(), rawler::imgop::Dim2::new(w, h)),
            pattern,
            colors,
            hex,
            solitary,
            black: [0.0; 4],
            scale: 1.0,
            wb: [1.0; 3],
        }
    }

    fn bayer_colors() -> [[u8; 6]; 6] {
        std::array::from_fn(|y| std::array::from_fn(|x| RGGB[y % 2][x % 2]))
    }

    /// A smooth, coloured scene: what a demosaic must get nearly right.
    fn scene(x: usize, y: usize) -> [f32; 3] {
        let (fx, fy) = (x as f32 / 97.0, y as f32 / 71.0);
        [
            0.30 + 0.20 * fx.sin(),
            0.40 + 0.15 * (fx + fy).cos(),
            0.25 + 0.10 * fy.sin(),
        ]
    }

    fn sampled(
        w: usize,
        h: usize,
        colors: &[[u8; 6]; 6],
        f: impl Fn(usize, usize) -> [f32; 3],
    ) -> Vec<f32> {
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| f(x, y)[colors[y % 6][x % 6] as usize])
            .collect()
    }

    fn worst_error(
        out: &[[f32; 3]],
        w: usize,
        h: usize,
        f: impl Fn(usize, usize) -> [f32; 3],
    ) -> f32 {
        let mut worst = 0.0f32;
        for y in 8..h - 8 {
            for x in 8..w - 8 {
                let truth = f(x, y);
                for c in 0..3 {
                    worst = worst.max((out[y * w + x][c] - truth[c]).abs());
                }
            }
        }
        worst
    }

    #[test]
    fn a_flat_grey_stays_grey() {
        for (colors, pattern) in [(bayer_colors(), Pattern::Bayer), (XTRANS, Pattern::XTrans)] {
            // Wider and taller than one tile, so the tiles' seams are in it.
            let (w, h) = (TILE_INNER + 70, TILE_INNER + 30);
            let data = vec![0.37f32; w * h];
            let out = mosaic_of(&data, w, h, colors, pattern).develop();
            for (i, px) in out.iter().enumerate() {
                for c in px {
                    assert!((c - 0.37).abs() < 1e-4, "{pattern:?} pixel {i}: {px:?}");
                }
            }
        }
    }

    #[test]
    fn a_smooth_scene_comes_back() {
        for (colors, pattern, tolerance) in [
            (bayer_colors(), Pattern::Bayer, 0.01),
            (XTRANS, Pattern::XTrans, 0.01),
        ] {
            let (w, h) = (TILE_INNER + 90, TILE_INNER + 50);
            let data = sampled(w, h, &colors, scene);
            let out = mosaic_of(&data, w, h, colors, pattern).develop();
            let worst = worst_error(&out, w, h, scene);
            assert!(worst < tolerance, "{pattern:?}: worst error {worst}");
        }
    }

    #[test]
    fn tiles_join_without_a_seam() {
        // The same scene in one tile and in several: the pixels match.
        let colors = bayer_colors();
        let (w, h) = (TILE_INNER + 40, 120);
        let data = sampled(w, h, &colors, scene);
        let tiled = mosaic_of(&data, w, h, colors, Pattern::Bayer).develop();
        let m = mosaic_of(&data, w, h, colors, Pattern::Bayer);
        let mut scratch = Scratch::new(Pattern::Bayer);
        let whole = m.tile(&mut scratch, 0, 0, w, h);
        for (i, (a, b)) in tiled.iter().zip(&whole).enumerate() {
            for c in 0..3 {
                assert!((a[c] - b[c]).abs() < 1e-5, "pixel {i}: {a:?} against {b:?}");
            }
        }
    }

    #[test]
    fn folding_keeps_the_colour_phase() {
        for period in [2, 6] {
            for v in -40..140 {
                let r = fold(v, 10, 100, period);
                assert!((10..100).contains(&r), "{v} -> {r}");
                assert_eq!((r - v).rem_euclid(period), 0, "{v} -> {r}, period {period}");
            }
        }
    }

    #[test]
    fn x_trans_has_its_hexagons() {
        let (_, solitary) = markesteijn_hexagons(&XTRANS).expect("hexagons");
        let [row, col] = solitary;
        assert_eq!(XTRANS[row][col], 1, "the solitary site is a green");
        for (dy, dx) in [(0i32, 1i32), (0, -1), (1, 0), (-1, 0)] {
            let c = XTRANS[(row as i32 + dy).rem_euclid(6) as usize]
                [(col as i32 + dx).rem_euclid(6) as usize];
            assert_ne!(c, 1, "its four neighbours are not green");
        }
    }
}
