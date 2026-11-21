//! Solving a fitted look (tone curve and HSV table) from the pairs. Every
//! loop runs in a fixed order, so a fit is the same on any machine with any
//! number of threads.
//!
//! From RAWmakase `v0.7.1` @ 78069aa3 (MIT),
//! `crates/rawmakase-decode/src/camera_look/solve.rs`, as written, with the
//! photo's DNG Standard tone (`Tone`) in place of the ACR3 curve, and the
//! forward matrix and the unconstrained reference lattice left out (see
//! `rm_camera_look.rs`).

use super::Rgb;
use super::look::{CURVE_SAMPLES, Look, TABLE_DIMS, Tone, at, extrema, hsv, inverse};
use crate::colour::{srgb_decode, srgb_encode};

const LUT: usize = CURVE_SAMPLES - 1;

fn clamp01(p: Rgb) -> Rgb {
    p.map(|v| if v.is_finite() { v.clamp(0., 1.) } else { 0. })
}

/// The colour the look's table must make for the tone curves (the base's, then
/// the look's `curve`, on encoded values) to give `target`.
fn untoned(base: &Tone, target: Rgb, curve_inverse: &[f32]) -> Rgb {
    let e = extrema(clamp01(target).map(srgb_encode), |v| at(curve_inverse, v));
    extrema(e.map(srgb_decode), |v| base.inverse_at(v))
}

/// The look's curve from `knots` of the composite tone (base tone then look
/// curve): linear scene value to linear output, sorted, from (0, 0) to (1, 1).
/// The curve ends at 1 whatever the base: a base that reaches white before
/// the scene's does (a camera's exposure offset) has no single input for it.
fn curve_from_composite(base: &Tone, knots: &[(f32, f32)]) -> Vec<f32> {
    let mut last = 0f32;
    let mut curve: Vec<f32> = (0..=LUT)
        .map(|i| {
            let x = base.inverse_at(srgb_decode(i as f32 / LUT as f32));
            let v = srgb_encode(interpolate(knots, x)).clamp(last, 1.);
            last = v;
            v
        })
        .collect();
    curve[LUT] = 1.;
    curve
}
fn interpolate(knots: &[(f32, f32)], x: f32) -> f32 {
    let i = knots
        .partition_point(|k| k.0 <= x)
        .clamp(1, knots.len() - 1);
    let (a, b) = (knots[i - 1], knots[i]);
    if b.0 > a.0 {
        a.1 + (b.1 - a.1) * ((x - a.0) / (b.0 - a.0)).clamp(0., 1.)
    } else {
        b.1
    }
}
/// Knots from (0, 0) to (1, 1) through `points` (sorted by x), each y at least
/// the one before: monotone, the curve's ends fixed.
fn monotone_knots(points: impl IntoIterator<Item = (f32, f32)>) -> Vec<(f32, f32)> {
    let mut knots = vec![(0f32, 0f32)];
    for (x, y) in points {
        let last = *knots.last().expect("starts at 0");
        if x > last.0 + 1e-5 && x < 1. - 1e-5 {
            knots.push((x, y.clamp(last.1, 1.)));
        }
    }
    knots.push((1., 1.));
    knots
}
/// The `q` quantile of sorted `v`.
fn quantile(v: &[f32], q: f32) -> f32 {
    let i = (q * (v.len() - 1) as f32).round() as usize;
    v[i.min(v.len() - 1)]
}

/// The starting curve: the i-th quantile of the scene's value (the brightest
/// channel, which the tone curves map) goes to the i-th quantile of the
/// preview's, at 32 knots.
fn quantile_curve(base: &Tone, scene: &[Rgb], target: &[Rgb]) -> Vec<f32> {
    let values = |v: &[Rgb]| {
        let mut out: Vec<f32> = v
            .iter()
            .map(|p| {
                let p = clamp01(*p);
                p[0].max(p[1]).max(p[2])
            })
            .collect();
        out.sort_by(f32::total_cmp);
        out
    };
    let (xs, ys) = (values(scene), values(target));
    curve_from_composite(
        base,
        &monotone_knots(
            (1..32).map(|k| (quantile(&xs, k as f32 / 32.), quantile(&ys, k as f32 / 32.))),
        ),
    )
}

/// The widest bin of the refitted curve (encoded), and the least weight it
/// closes with.
const BIN_WIDTH: f32 = 0.06;
const BIN_LEAST: f64 = 8.;

/// The curve again for the table `look` has: the encoded extrema the look's
/// curve receives against the preview's, binned into 32 equal-weight bins,
/// narrower where [`BIN_WIDTH`] says, made monotone (pool adjacent violators).
fn refit_curve(base: &Tone, look: &Look, scene: &[Rgb], target: &[Rgb]) -> Vec<f32> {
    let mut points: Vec<(f32, f32, f32)> = Vec::with_capacity(scene.len() * 2);
    for (s, t) in scene.iter().zip(target) {
        let a = look.table_output(clamp01(*s));
        let toned = base.rgb_tone(a).map(srgb_encode);
        let t = clamp01(*t).map(srgb_encode);
        let lo = |p: Rgb| p.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = |p: Rgb| p.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        points.push((hi(toned), hi(t), 1.));
        points.push((lo(toned), lo(t), 0.5));
    }
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f32 = points.iter().map(|p| p.2).sum();
    let mut bins: Vec<[f64; 3]> = Vec::new();
    let (mut acc, mut next) = ([0f64; 3], total / 32.);
    let mut seen = 0f32;
    let mut first = 0f32;
    for (u, y, w) in points {
        if acc[2] == 0. {
            first = u;
        }
        let w64 = f64::from(w);
        acc = [
            acc[0] + w64 * f64::from(u),
            acc[1] + w64 * f64::from(y),
            acc[2] + w64,
        ];
        seen += w;
        // A bin spans a limited range too: few pixels in the shadows must still set them.
        if seen >= next || (u - first > BIN_WIDTH && acc[2] >= BIN_LEAST) {
            bins.push(acc);
            acc = [0.; 3];
            next = seen + total / 32.;
        }
    }
    if acc[2] > 0. {
        bins.push(acc);
    }
    // Pool adjacent violators over the bins' means.
    let mut pooled: Vec<[f64; 3]> = Vec::new();
    for b in bins {
        pooled.push(b);
        while pooled.len() > 1 {
            let n = pooled.len();
            let (p, q) = (pooled[n - 2], pooled[n - 1]);
            if p[1] / p[2] <= q[1] / q[2] {
                break;
            }
            pooled.truncate(n - 2);
            pooled.push([p[0] + q[0], p[1] + q[1], p[2] + q[2]]);
        }
    }
    let knots = monotone_knots(
        pooled
            .iter()
            .map(|b| ((b[0] / b[2]) as f32, (b[1] / b[2]) as f32)),
    );
    let mut last = 0f32;
    (0..=LUT)
        .map(|i| {
            let v = interpolate(&knots, i as f32 / LUT as f32).clamp(last, 1.);
            last = v;
            v
        })
        .collect()
}

/// The most a node may shift hue (degrees), and scale saturation and value (as a
/// natural logarithm).
const LIMITS: [f32; 3] = [30., 0.7, 0.7];
/// How firmly neighbouring nodes are tied to each other, against a sample's
/// weight of about 1, and how firmly each node is held to no change, as a share
/// of that.
const SMOOTHNESS: f64 = 3.;
const NO_CHANGE: f64 = 0.05;
/// The most a sample weighs, and the least lightness (encoded value over 0.5)
/// it is weighed by, so that a dark photo's colours still count.
const WEIGHT_MOST: f64 = 9.;
const WEIGHT_DARKEST: f32 = 0.2;

/// The look table's nodes as the solve sees them: which lie beside which, and
/// how firmly neighbours are tied. The neutral column is fixed at no change and
/// takes no part.
///
/// The ties are those of a smooth look over the cylinder of hue, saturation and
/// value. Neighbouring hues of little saturation are nearly one colour, and are
/// tied the more firmly: nodes near grey are learned from few pixels, mostly of
/// soft edges, and a zigzag between them shows as bands around such an edge.
/// Along saturation and value the ties weaken towards grey, as the cells there
/// shrink.
struct Nodes {
    dims: [usize; 3],
    /// Each node's 27 neighbours (itself among them), by offset: hue,
    /// saturation and value each one below, at or one above; `NONE` outside the
    /// table and on the neutral column.
    beside: Vec<[u32; 27]>,
}
const NONE: u32 = u32::MAX;
impl Nodes {
    fn new() -> Self {
        let dims = TABLE_DIMS;
        let [nh, ns, nv] = dims;
        let index = |h: usize, s: usize, v: usize| (v * nh + h) * ns + s;
        let beside = (0..nh * ns * nv)
            .map(|node| {
                let (s, h, v) = (node % ns, node / ns % nh, node / ns / nh);
                std::array::from_fn(|k| {
                    let d = [k as i32 % 3 - 1, k as i32 / 3 % 3 - 1, k as i32 / 9 - 1];
                    let (is, iv) = (s as i32 + d[1], v as i32 + d[2]);
                    if is < 1 || is >= ns as i32 || iv < 0 || iv >= nv as i32 {
                        return NONE;
                    }
                    let ih = (h as i32 + d[0]).rem_euclid(nh as i32) as usize;
                    index(ih, is as usize, iv as usize) as u32
                })
            })
            .collect();
        Self { dims, beside }
    }
    /// The offset (see `beside`) from a cell's corner `a` to its corner `b`,
    /// each 0 or 1 along hue, saturation and value.
    fn offset(a: [usize; 3], b: [usize; 3]) -> usize {
        let d = |c: usize| b[c] + 1 - a[c];
        d(2) * 9 + d(1) * 3 + d(0)
    }
    /// The steps between nodes along hue (radians), saturation and value.
    fn steps(&self) -> [f64; 3] {
        let [nh, ns, nv] = self.dims;
        [
            std::f64::consts::TAU / nh as f64,
            1. / (ns - 1) as f64,
            1. / (nv - 1) as f64,
        ]
    }
    /// How firmly `node` is held to no change: its cell's volume.
    fn hold(&self, node: usize) -> f64 {
        let [dh, ds, dv] = self.steps();
        let s = (node % self.dims[1]) as f64;
        NO_CHANGE * SMOOTHNESS * dh * s * ds * ds * dv
    }
    /// The ties of `node` for `channel` (0 hue, 1 saturation, 2 value): each
    /// neighbour and the tie's weight, the cell's face between them over their
    /// distance. No neighbour is the neutral column's no change: saturation and
    /// value end there, no more firmly than they are tied to each other; a hue
    /// shift has nothing to meet.
    fn ties(&self, channel: usize, node: usize) -> [(u32, f64); 6] {
        let [_, ns, nv] = self.dims;
        let [dh, ds, dv] = self.steps();
        let (s, v) = (node % ns, node / ns / self.dims[0]);
        let radius = |s: f64| s * ds;
        let hue = SMOOTHNESS * ds * dv / (dh * radius(s as f64));
        let saturation = |low: usize| SMOOTHNESS * dh * radius(low as f64 + 0.5) * dv / ds;
        let value = SMOOTHNESS * dh * radius(s as f64) * ds / dv;
        let to_grey = if channel == 0 { 0. } else { saturation(0) };
        let beside = |d: [usize; 3]| self.beside[node][Self::offset([1, 1, 1], d)];
        [
            (beside([2, 1, 1]), hue),
            (beside([0, 1, 1]), hue),
            match s > 1 {
                true => (beside([1, 0, 1]), saturation(s - 1)),
                false => (NONE, to_grey),
            },
            (
                beside([1, 2, 1]),
                if s + 1 < ns { saturation(s) } else { 0. },
            ),
            (beside([1, 1, 0]), if v > 0 { value } else { 0. }),
            (beside([1, 1, 2]), if v + 1 < nv { value } else { 0. }),
        ]
    }
    /// What the samples' side `data` (per node, by offset), the ties and the
    /// holds make of `x`.
    fn apply(&self, data: &[[f64; 27]], channel: usize, x: &[f64], out: &mut [f64]) {
        for (node, out) in out.iter_mut().enumerate() {
            if node % self.dims[1] == 0 {
                *out = x[node];
                continue;
            }
            let mut sum = self.hold(node) * x[node];
            for (coefficient, other) in data[node].iter().zip(&self.beside[node]) {
                if *other != NONE {
                    sum += coefficient * x[*other as usize];
                }
            }
            for (other, weight) in self.ties(channel, node) {
                let there = if other == NONE { 0. } else { x[other as usize] };
                sum += weight * (x[node] - there);
            }
            *out = sum;
        }
    }
    /// The table's `channel` the samples ask for: from `now`, the least squares
    /// of their remaining changes (`asked`, already gathered onto the nodes),
    /// the ties and the holds, by conjugate gradients preconditioned with the
    /// diagonal. The same steps in the same order on any machine.
    fn solve(&self, data: &[[f64; 27]], asked: &[f64], now: &[f64], channel: usize) -> Vec<f64> {
        let n = now.len();
        let ns = self.dims[1];
        let mut scratch = vec![0f64; n];
        // The right-hand side: what the samples make of the table as it is, and
        // the change they still ask for.
        let mut b = vec![0f64; n];
        for node in (0..n).filter(|node| node % ns != 0) {
            b[node] = asked[node];
            for (coefficient, other) in data[node].iter().zip(&self.beside[node]) {
                if *other != NONE {
                    b[node] += coefficient * now[*other as usize];
                }
            }
        }
        let diagonal: Vec<f64> = (0..n)
            .map(|node| match node % ns {
                0 => 1.,
                _ => {
                    let ties: f64 = self.ties(channel, node).iter().map(|t| t.1).sum();
                    data[node][13] + self.hold(node) + ties
                }
            })
            .collect();
        let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(a, b)| a * b).sum::<f64>();
        let mut x = now.to_vec();
        self.apply(data, channel, &x, &mut scratch);
        let mut r: Vec<f64> = b.iter().zip(&scratch).map(|(b, m)| b - m).collect();
        let mut z: Vec<f64> = r.iter().zip(&diagonal).map(|(r, d)| r / d).collect();
        let mut p = z.clone();
        let mut rz = dot(&r, &z);
        let small = 1e-18 * dot(&b, &b).max(1e-30);
        for _ in 0..400 {
            if dot(&r, &r) <= small {
                break;
            }
            self.apply(data, channel, &p, &mut scratch);
            let curvature = dot(&p, &scratch);
            if curvature <= 0. {
                break;
            }
            let alpha = rz / curvature;
            for node in 0..n {
                x[node] += alpha * p[node];
                r[node] -= alpha * scratch[node];
                z[node] = r[node] / diagonal[node];
            }
            let next = dot(&r, &z);
            let beta = next / rz;
            rz = next;
            for node in 0..n {
                p[node] = z[node] + beta * p[node];
            }
        }
        x
    }
}

/// One round of the table, by weighted least squares. Each sample asks the
/// eight nodes around where it enters the table, by the table's own
/// interpolation, for the hue, saturation and value change that remains
/// between what the current table makes of it and what the curves need.
/// Neighbouring nodes are tied ([`Nodes`]), so a colour the photo has little
/// of is still learned, and the nodes around it follow instead of staying
/// unchanged beside it: a face among trees is a few hundred samples.
fn refine_table(base: &Tone, look: &mut Look, scene: &[Rgb], target: &[Rgb]) {
    let curve_inverse = inverse(|v| at(&look.curve, v));
    let nodes = Nodes::new();
    let [nh, ns, nv] = nodes.dims;
    let n = nh * ns * nv;
    // The samples' side of the normal equations, for hue and saturation and
    // for value (their weights differ), and the changes asked for, gathered
    // onto the nodes.
    let mut data = [vec![[0f64; 27]; n], vec![[0f64; 27]; n]];
    let mut asked = [vec![0f64; n], vec![0f64; n], vec![0f64; n]];
    for (s, t) in scene.iter().zip(target) {
        let input = clamp01(*s);
        let [h, sat, v] = hsv(input);
        let made = hsv(look.table_output(input));
        let wanted = hsv(untoned(base, *t, &curve_inverse));
        let d_hue = ((wanted[0] - made[0] + 0.5).rem_euclid(1.) - 0.5) * 360.;
        // Scales are asked for as differences, not logarithms: among noisy
        // bright pixels it is their mean the table should make, not their
        // geometric mean.
        let colour = (sat > 0.05 && made[1] > 0.02 && wanted[1] > 0.)
            .then(|| [f64::from(d_hue), f64::from(wanted[1] / made[1] - 1.)]);
        let value = (made[2] > 1e-3 && wanted[2] > 1e-4)
            .then(|| f64::from(srgb_encode(wanted[2]) / srgb_encode(made[2]) - 1.));
        let encoded = srgb_encode(v).clamp(0., 1.);
        // A change of hue or saturation shows as much as the colour is
        // saturated and light, one of value as much as it is light.
        let light = f64::from((encoded / 0.5).max(WEIGHT_DARKEST)).powi(2);
        let weights = [
            (f64::from(sat / 0.3).powi(2) * light).min(WEIGHT_MOST),
            light,
        ];
        // The cell, as `lookup` finds it.
        let place = [
            h.rem_euclid(1.) * nh as f32,
            sat.clamp(0., 1.) * (ns - 1) as f32,
            encoded * (nv - 1) as f32,
        ];
        let cell = place.map(|c| c.floor() as usize);
        let within: [f32; 3] = std::array::from_fn(|c| place[c] - cell[c] as f32);
        let corners: [(usize, [usize; 3], f64); 8] = std::array::from_fn(|i| {
            let o = [i & 1, i >> 1 & 1, i >> 2 & 1];
            let (ih, is, iv) = (
                (cell[0] + o[0]) % nh,
                (cell[1] + o[1]).min(ns - 1),
                (cell[2] + o[2]).min(nv - 1),
            );
            let share: f32 = (0..3)
                .map(|c| if o[c] == 1 { within[c] } else { 1. - within[c] })
                .product();
            // The neutral column is fixed: its share asks nothing.
            let share = if is == 0 { 0. } else { f64::from(share) };
            ((iv * nh + ih) * ns + is, o, share)
        });
        for (node, a, share) in corners {
            if share == 0. {
                continue;
            }
            if let Some([d_hue, d_saturation]) = colour {
                asked[0][node] += weights[0] * share * d_hue;
                asked[1][node] += weights[0] * share * d_saturation;
            }
            if let Some(d_value) = value {
                asked[2][node] += weights[1] * share * d_value;
            }
            for (_, b, other) in corners {
                let k = Nodes::offset(a, b);
                if colour.is_some() {
                    data[0][node][k] += weights[0] * share * other;
                }
                if value.is_some() {
                    data[1][node][k] += weights[1] * share * other;
                }
            }
        }
    }
    let now = |channel: usize| -> Vec<f64> {
        look.table
            .iter()
            .map(|e| match channel {
                0 => f64::from(e[0]),
                c => f64::from(e[c].ln()),
            })
            .collect()
    };
    let solved: [Vec<f64>; 3] = std::array::from_fn(|channel| {
        let data = &data[usize::from(channel == 2)];
        nodes.solve(data, &asked[channel], &now(channel), channel)
    });
    for (node, entry) in look.table.iter_mut().enumerate() {
        if node % ns == 0 {
            // The neutral column stays as it is: grey's tone is the curve's.
            *entry = [0., 1., 1.];
            continue;
        }
        let next: [f32; 3] =
            std::array::from_fn(|c| (solved[c][node] as f32).clamp(-LIMITS[c], LIMITS[c]));
        *entry = [next[0], next[1].exp(), next[2].exp()];
    }
    limit_slopes(&mut look.table);
}
/// The most a node may differ from its neighbour along hue, saturation and
/// value, for the hue shift (degrees) and the logarithms of the saturation and
/// value scales: a bound on what the solve may make of contradictory samples
/// (skin in two lights), which would bend a gradient into contours.
const SLOPES: [[f32; 3]; 3] = [[13.5, 15., 15.], [0.24, 0.36, 0.36], [0.12, 0.36, 0.36]];
/// The most the value scale of the least saturated nodes may differ from
/// grey's 1 (as a logarithm): saturation is the least steady thing about a dark
/// or pale colour, so a value scale that leaves grey steeply turns noise and
/// soft edges there into contours.
const GREY_SLOPE: f32 = 0.12;

/// The closest table, in sweeps along each axis, whose neighbouring nodes
/// respect [`SLOPES`]; the neutral column stays as it is.
fn limit_slopes(table: &mut [[f32; 3]]) {
    let [nh, ns, nv] = TABLE_DIMS;
    let index = |h: usize, s: usize, v: usize| (v * nh + h) * ns + s;
    let log = |e: [f32; 3]| [e[0], e[1].ln(), e[2].ln()];
    let mut logged: Vec<[f32; 3]> = table.iter().map(|e| log(*e)).collect();
    let limit = |to: &mut f32, from: f32, slope: f32| *to = to.clamp(from - slope, from + slope);
    for _ in 0..2 {
        for c in 0..3 {
            for v in 0..nv {
                for s in 1..ns {
                    // Twice around: hue is a circle.
                    for h in 0..2 * nh {
                        let (a, b) = (index(h % nh, s, v), index((h + 1) % nh, s, v));
                        let from = logged[a][c];
                        limit(&mut logged[b][c], from, SLOPES[c][0]);
                    }
                    for h in (0..2 * nh).rev() {
                        let (a, b) = (index((h + 1) % nh, s, v), index(h % nh, s, v));
                        let from = logged[a][c];
                        limit(&mut logged[b][c], from, SLOPES[c][0]);
                    }
                }
            }
            for v in 0..nv {
                for h in 0..nh {
                    for s in 1..ns {
                        let from = logged[index(h, s - 1, v)][c];
                        let slope = if (c, s) == (2, 1) {
                            GREY_SLOPE
                        } else {
                            SLOPES[c][1]
                        };
                        limit(&mut logged[index(h, s, v)][c], from, slope);
                    }
                    for s in (1..ns - 1).rev() {
                        let from = logged[index(h, s + 1, v)][c];
                        limit(&mut logged[index(h, s, v)][c], from, SLOPES[c][1]);
                    }
                }
            }
            for h in 0..nh {
                for s in 1..ns {
                    for v in 1..nv {
                        let from = logged[index(h, s, v - 1)][c];
                        limit(&mut logged[index(h, s, v)][c], from, SLOPES[c][2]);
                    }
                    for v in (0..nv - 1).rev() {
                        let from = logged[index(h, s, v + 1)][c];
                        limit(&mut logged[index(h, s, v)][c], from, SLOPES[c][2]);
                    }
                }
            }
        }
    }
    for (entry, l) in table.iter_mut().zip(&logged) {
        *entry = [l[0], l[1].exp(), l[2].exp()];
    }
}

/// Most and least a channel of grey is moved, and the fewest grey pairs to
/// judge by.
const GREY_MOST: f32 = 0.05;
const GREY_LEAST: f32 = 0.002;
const GREY_PAIRS: usize = 64;

/// The gains (geometric mean 1) on linear ProPhoto that take the greys of
/// `scene` to those of `target`, where both are near neutral and neither dark
/// nor bright; `None` with too few of them or no difference worth a change.
pub(super) fn grey_gains(scene: &[Rgb], target: &[Rgb]) -> Option<[f32; 3]> {
    let tint = |p: &Rgb| {
        let max = p.iter().copied().fold(f32::MIN, f32::max);
        let min = p.iter().copied().fold(f32::MAX, f32::min);
        let mean = (p[0] + p[1] + p[2]) / 3.;
        (min > 0. && (max - min) / max < 0.08 && (0.05..0.9).contains(&mean))
            .then(|| p.map(|v| (v / mean).ln()))
    };
    let (mut sum, mut n) = ([0f64; 3], 0usize);
    for (s, t) in scene.iter().zip(target) {
        if let (Some(s), Some(t)) = (tint(s), tint(t)) {
            (0..3).for_each(|c| sum[c] += f64::from(t[c] - s[c]));
            n += 1;
        }
    }
    if n < GREY_PAIRS {
        return None;
    }
    let mean = sum.map(|v| (v / n as f64) as f32);
    let centre = (mean[0] + mean[1] + mean[2]) / 3.;
    let log = mean.map(|v| v - centre);
    log.iter()
        .any(|v| v.abs() >= GREY_LEAST)
        .then(|| log.map(|v| v.clamp(-GREY_MOST, GREY_MOST).exp()))
}

/// The look over `base` taking `scene` (linear ProPhoto as the stage receives
/// it, the grey gains applied) to `target` (the preview's linear ProPhoto):
/// the quantile curve, then alternately the table and the curve.
pub(super) fn colour_look(base: &Tone, scene: &[Rgb], target: &[Rgb]) -> Look {
    let mut look = Look {
        curve: quantile_curve(base, scene, target),
        ..Look::identity()
    };
    for round in 0..3 {
        for _ in 0..2 {
            refine_table(base, &mut look, scene, target);
        }
        if round < 2 {
            look.curve = refit_curve(base, &look, scene, target);
        }
    }
    look
}
/// A look of the curve alone, the table unchanged.
pub(super) fn tone_look(base: &Tone, scene: &[Rgb], target: &[Rgb]) -> Look {
    let mut look = Look {
        curve: quantile_curve(base, scene, target),
        ..Look::identity()
    };
    look.curve = refit_curve(base, &look, scene, target);
    look
}

#[cfg(test)]
mod tests {
    use super::super::look::tests::{base, known_look};
    use super::*;

    /// Colours spread over the gamut, as a scene's would be, in a fixed order.
    fn scene() -> Vec<Rgb> {
        let mut seed = 12345u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        (0..6000)
            .map(|_| {
                let v = 0.02 + 0.6 * next().powi(2);
                let s = next() * 0.8;
                let h = next();
                [h, (h + 0.33) % 1., (h + 0.66) % 1.]
                    .map(|x| v * (1. - s * (0.5 + 0.5 * (x * std::f32::consts::TAU).cos())))
            })
            .collect()
    }

    fn rms(base: &Tone, look: &Look, scene: &[Rgb], target: &[Rgb]) -> f32 {
        let sse: f32 = scene
            .iter()
            .zip(target)
            .map(|(s, t)| {
                let got = look.render(base, *s);
                (0..3).map(|c| (got[c] - t[c]).powi(2)).sum::<f32>()
            })
            .sum();
        (sse / (3 * scene.len()) as f32).sqrt()
    }

    #[test]
    fn a_known_look_is_recovered() {
        let base = base();
        let truth = known_look();
        let scene = scene();
        let target: Vec<Rgb> = scene.iter().map(|s| truth.render(&base, *s)).collect();
        let before = rms(&base, &Look::identity(), &scene, &target);
        let fit = colour_look(&base, &scene, &target);
        let after = rms(&base, &fit, &scene, &target);
        assert!(after < 0.25 * before && after < 0.01, "{before} -> {after}");
        // The same fit again, to the bit.
        assert_eq!(colour_look(&base, &scene, &target), fit);
        assert!(fit.is_valid());
    }

    /// A few dark pixels among many bright ones: equal-weight bins alone would
    /// draw one line from black to the bright tones and lift the blacks.
    #[test]
    fn a_few_dark_pixels_still_set_the_blacks() {
        let base = base();
        let look = Look::identity();
        let dark = (0..40).map(|i| 0.004 + i as f32 / 40. * 0.004);
        let bright = (0..4000).map(|i| 0.3 + i as f32 / 4000. * 0.3);
        let scene: Vec<Rgb> = dark.chain(bright).map(|v| [v; 3]).collect();
        // A camera deeper in its blacks than DNG Standard, and the same elsewhere.
        let target: Vec<Rgb> = scene
            .iter()
            .map(|s| {
                let made = look.render(&base, *s);
                if s[0] < 0.1 {
                    made.map(|v| v * 0.5)
                } else {
                    made
                }
            })
            .collect();
        let curve = refit_curve(&base, &look, &scene, &target);
        let x = srgb_encode(base.at(scene[20][0]));
        let wanted = srgb_encode(target[20][0]);
        assert!(
            (at(&curve, x) - wanted).abs() < 0.02,
            "{} {wanted}",
            at(&curve, x)
        );
    }

    /// Skin in two lights, nearly the same colour and asked for very different
    /// ones: the table bends only as far as its slopes allow.
    #[test]
    fn a_table_never_steps_between_neighbouring_nodes() {
        let base = base();
        let mut scene = Vec::new();
        let mut target = Vec::new();
        for i in 0..4000 {
            let t = i as f32 / 4000.;
            scene.push([0.3 + 0.1 * t, 0.2 + 0.05 * t, 0.15]);
            target.push(if i % 2 == 0 {
                [0.6, 0.3, 0.1]
            } else {
                [0.2, 0.2, 0.5]
            });
        }
        let look = colour_look(&base, &scene, &target);
        let [nh, ns, nv] = TABLE_DIMS;
        let at = |h: usize, s: usize, v: usize| look.table[(v * nh + h) * ns + s];
        for v in 0..nv {
            for s in 1..ns {
                for h in 0..nh {
                    let (a, b) = (at(h, s, v), at((h + 1) % nh, s, v));
                    assert!((a[0] - b[0]).abs() <= SLOPES[0][0] + 1e-3, "{a:?} {b:?}");
                    assert!((a[1].ln() - b[1].ln()).abs() <= SLOPES[1][0] + 1e-3);
                    assert!((a[2].ln() - b[2].ln()).abs() <= SLOPES[2][0] + 1e-3);
                }
            }
        }
    }

    /// A face among trees: a few hundred samples of skin, with nothing of
    /// their colour around them, are still taken to the camera's skin.
    #[test]
    fn a_small_patch_of_colour_is_learned_beside_a_large_one() {
        let base = base();
        let plain = Look::identity();
        let mut scene: Vec<Rgb> = scene()
            .into_iter()
            .filter(|p| (0.25..0.75).contains(&hsv(*p)[0]))
            .collect();
        let mut target: Vec<Rgb> = scene.iter().map(|s| plain.render(&base, *s)).collect();
        let trees = scene.len();
        let mut camera = Look::identity();
        for entry in camera.table.iter_mut() {
            *entry = [8., 1.25, 1.];
        }
        for i in 0..200 {
            let t = i as f32 / 200.;
            let skin = [0.20 + 0.06 * t, 0.155 + 0.04 * t, 0.125 + 0.03 * t];
            scene.push(skin);
            target.push(camera.render(&base, skin));
        }
        let fit = colour_look(&base, &scene, &target);
        let (before, after) = (
            rms(&base, &plain, &scene[trees..], &target[trees..]),
            rms(&base, &fit, &scene[trees..], &target[trees..]),
        );
        assert!(after < 0.25 * before, "{before} -> {after}");
    }

    #[test]
    fn a_grey_cast_is_measured_on_greys_alone() {
        let scene: Vec<Rgb> = (0..200).map(|i| [0.4 + i as f32 / 1000.; 3]).collect();
        let target: Vec<Rgb> = scene.iter().map(|p| [p[0], p[1] * 1.02, p[2]]).collect();
        let g = grey_gains(&scene, &target).expect("a cast");
        assert!(g[1] / g[0] > 1.019 && g[1] / g[0] < 1.021, "{g:?}");
        assert_eq!(grey_gains(&scene, &scene), None, "none to correct");
        assert_eq!(grey_gains(&scene[..10], &target[..10]), None, "too few");
        let red: Vec<Rgb> = vec![[0.6, 0.2, 0.1]; 200];
        assert_eq!(grey_gains(&red, &target), None);
    }

    #[test]
    fn the_quantile_curve_is_monotone_with_fixed_ends() {
        let base = base();
        let scene = scene();
        let target: Vec<Rgb> = scene.iter().map(|p| p.map(|v| v.sqrt())).collect();
        let curve = quantile_curve(&base, &scene, &target);
        assert_eq!(curve.len(), CURVE_SAMPLES);
        assert_eq!(curve[0], 0.);
        assert!((curve[LUT] - 1.).abs() < 1e-6);
        assert!(curve.windows(2).all(|w| w[0] <= w[1]));
    }
}
