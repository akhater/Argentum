//! How many points a tone curve may have, and what happens to one that has more. Ours.
//!
//! The shader takes each curve as a fixed array of `MAX_CURVE_POINTS` points. A curve with more
//! (an edit made before the editor had a limit, a converted preset) is thinned to the limit by
//! dropping, one at a time, the point whose absence changes the curve least - measured against
//! the original curve as the shader draws it. The end points are never dropped, the points that
//! stay are the original ones unchanged, and a curve within the limit is returned untouched.
//! The interface's copy of the limit is `MAX_CURVE_POINTS` in src/argentum/curveLimit.ts, kept
//! equal by a test below (the generated contract carries types, not constants).

use serde_json::Value;

/// The most points one curve may have; the size of the shader's arrays.
pub const MAX_CURVE_POINTS: usize = 16;

/// A curve's value (0..255) at `x` (0..255) as `apply_curve` in shader.wgsl draws it:
/// monotone cubic Hermite between the points, flat beyond the end points.
pub fn curve_at(points: &[(f64, f64)], x: f64) -> f64 {
    let n = points.len();
    if n < 2 {
        return x;
    }
    if x <= points[0].0 {
        return points[0].1;
    }
    if x >= points[n - 1].0 {
        return points[n - 1].1;
    }
    for i in 0..n - 1 {
        let (p1, p2) = (points[i], points[i + 1]);
        if x > p2.0 {
            continue;
        }
        let p0 = if i == 0 { points[0] } else { points[i - 1] };
        let p3 = points[(i + 2).min(n - 1)];
        let delta_before = (p1.1 - p0.1) / (p1.0 - p0.0).max(0.001);
        let delta_current = (p2.1 - p1.1) / (p2.0 - p1.0).max(0.001);
        let delta_after = (p3.1 - p2.1) / (p3.0 - p2.0).max(0.001);
        let mut m1 = if i == 0 {
            delta_current
        } else if delta_before * delta_current <= 0.0 {
            0.0
        } else {
            (delta_before + delta_current) / 2.0
        };
        let mut m2 = if i + 1 == n - 1 {
            delta_current
        } else if delta_current * delta_after <= 0.0 {
            0.0
        } else {
            (delta_current + delta_after) / 2.0
        };
        if delta_current != 0.0 {
            let (alpha, beta) = (m1 / delta_current, m2 / delta_current);
            if alpha * alpha + beta * beta > 9.0 {
                let tau = 3.0 / (alpha * alpha + beta * beta).sqrt();
                m1 *= tau;
                m2 *= tau;
            }
        }
        let dx = p2.0 - p1.0;
        if dx <= 0.0 {
            return p1.1;
        }
        let t = (x - p1.0) / dx;
        let (t2, t3) = (t * t, t * t * t);
        let y = (2.0 * t3 - 3.0 * t2 + 1.0) * p1.1
            + (t3 - 2.0 * t2 + t) * m1 * dx
            + (-2.0 * t3 + 3.0 * t2) * p2.1
            + (t3 - t2) * m2 * dx;
        return y.clamp(0.0, 255.0);
    }
    points[n - 1].1
}

/// Samples per unit of x for the comparison below: a quarter step over 0..=255.
const SAMPLES_PER_UNIT: f64 = 4.0;
const SAMPLE_COUNT: usize = 1021;

fn sample_x(k: usize) -> f64 {
    k as f64 / SAMPLES_PER_UNIT
}

/// The largest difference, over 0..=255 in steps of a quarter, between two curves.
#[cfg(test)]
pub fn max_difference(a: &[(f64, f64)], b: &[(f64, f64)]) -> f64 {
    (0..SAMPLE_COUNT)
        .map(|k| (curve_at(a, sample_x(k)) - curve_at(b, sample_x(k))).abs())
        .fold(0.0, f64::max)
}

/// The most points the thinning looks at. A curve with more is first spread down to this many by
/// even steps along its points (end points kept): such a curve is dense enough that this costs
/// almost nothing, and it keeps the time of the thinning bounded.
pub const MAX_THINNING_INPUT: usize = 64;

/// The indices of the points to keep, in order: all of them when there are `MAX_CURVE_POINTS` or
/// fewer, else that many chosen as the module header says.
pub fn kept_indices(points: &[(f64, f64)]) -> Vec<usize> {
    let n = points.len();
    if n <= MAX_CURVE_POINTS {
        return (0..n).collect();
    }
    // Over the input cap: even steps first, then the thinning below on those.
    let start: Vec<usize> = if n > MAX_THINNING_INPUT {
        (0..MAX_THINNING_INPUT)
            .map(|i| (i * (n - 1) + (MAX_THINNING_INPUT - 1) / 2) / (MAX_THINNING_INPUT - 1))
            .collect()
    } else {
        (0..n).collect()
    };
    let pts: Vec<(f64, f64)> = start.iter().map(|&i| points[i]).collect();
    // The original curve, drawn once.
    let original: Vec<f64> = (0..SAMPLE_COUNT)
        .map(|k| curve_at(&pts, sample_x(k)))
        .collect();
    let mut kept: Vec<usize> = (0..pts.len()).collect();
    while kept.len() > MAX_CURVE_POINTS {
        let mut best = (f64::MAX, 1);
        for at in 1..kept.len() - 1 {
            // Taking a point out changes the curve only between the points two before and two
            // after it (each segment's slopes read one neighbour either side).
            let from = kept[at.saturating_sub(2)];
            let to = kept[(at + 2).min(kept.len() - 1)];
            let lo = ((pts[from].0 * SAMPLES_PER_UNIT).floor().max(0.0)) as usize;
            let hi = (((pts[to].0 * SAMPLES_PER_UNIT).ceil()) as usize).min(SAMPLE_COUNT - 1);
            let without: Vec<(f64, f64)> = kept
                .iter()
                .enumerate()
                .filter(|(k, _)| *k != at)
                .map(|(_, &i)| pts[i])
                .collect();
            let error = (lo..=hi.max(lo))
                .map(|k| (curve_at(&without, sample_x(k)) - original[k]).abs())
                .fold(0.0, f64::max);
            if error < best.0 {
                best = (error, at);
            }
        }
        kept.remove(best.1);
    }
    kept.into_iter().map(|i| start[i]).collect()
}

/// How many thinned curves are remembered: a handful of curves is thinned on every slider tick
/// and the answer only changes when the curve does.
const REMEMBERED: usize = 32;

type Remembered = Vec<(Vec<u64>, Vec<usize>)>;

fn remembered() -> &'static std::sync::Mutex<Remembered> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Remembered>> = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// `kept_indices`, remembered for the last `REMEMBERED` curves (oldest dropped first).
fn kept_indices_remembered(points: &[(f64, f64)]) -> Vec<usize> {
    let key: Vec<u64> = points
        .iter()
        .flat_map(|p| [p.0.to_bits(), p.1.to_bits()])
        .collect();
    if let Ok(mut cache) = remembered().lock() {
        if let Some(at) = cache.iter().position(|(k, _)| *k == key) {
            let entry = cache.remove(at);
            let answer = entry.1.clone();
            cache.push(entry);
            return answer;
        }
        drop(cache);
        let answer = kept_indices(points);
        if let Ok(mut cache) = remembered().lock() {
            if cache.len() >= REMEMBERED {
                cache.remove(0);
            }
            cache.push((key, answer.clone()));
        }
        return answer;
    }
    kept_indices(points)
}

/// A curve as the interface sends it (`{x, y}` objects), within the limit. A point with no
/// readable x and y counts as (0, 0), as the shader's array holds it.
pub fn fit_values(points: Vec<Value>) -> Vec<Value> {
    if points.len() <= MAX_CURVE_POINTS {
        return points;
    }
    let xy: Vec<(f64, f64)> = points
        .iter()
        .map(|p| {
            (
                p["x"].as_f64().unwrap_or(0.0),
                p["y"].as_f64().unwrap_or(0.0),
            )
        })
        .collect();
    kept_indices_remembered(&xy)
        .into_iter()
        .map(|i| points[i].clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn values(points: &[(f64, f64)]) -> Vec<Value> {
        points
            .iter()
            .map(|&(x, y)| json!({"x": x, "y": y}))
            .collect()
    }

    /// `n` points along a gently wavy curve, x from 0 to 255, evenly spaced.
    fn wavy(n: usize) -> Vec<(f64, f64)> {
        (0..n)
            .map(|i| {
                let x = 255.0 * i as f64 / (n - 1) as f64;
                let u = x / 255.0;
                (
                    x,
                    (255.0 * (u + 0.12 * (u * 9.0).sin() * (1.0 - u) * u * 4.0)).clamp(0.0, 255.0),
                )
            })
            .collect()
    }

    fn steep_s(x: f64) -> f64 {
        let raw = |x: f64| 1.0 / (1.0 + (-14.0 * (x / 255.0 - 0.5)).exp());
        255.0 * (raw(x) - raw(0.0)) / (raw(255.0) - raw(0.0))
    }

    /// `n` points along a steep S, evenly spaced.
    fn s_curve(n: usize) -> Vec<(f64, f64)> {
        (0..n)
            .map(|i| {
                let x = 255.0 * i as f64 / (n - 1) as f64;
                (x, steep_s(x))
            })
            .collect()
    }

    /// `n` points along the steep S, crowded into the middle of the range.
    fn clustered(n: usize) -> Vec<(f64, f64)> {
        (0..n)
            .map(|i| {
                let u = i as f64 / (n - 1) as f64 - 0.5;
                let x = 255.0 * (0.5 + 4.0 * u * u * u);
                (x, steep_s(x))
            })
            .collect()
    }

    fn pairs(values: &[Value]) -> Vec<(f64, f64)> {
        values
            .iter()
            .map(|p| (p["x"].as_f64().unwrap(), p["y"].as_f64().unwrap()))
            .collect()
    }

    #[test]
    fn a_curve_within_the_limit_is_untouched() {
        let p = values(&wavy(MAX_CURVE_POINTS));
        assert_eq!(fit_values(p.clone()), p);
    }

    /// The largest difference from the original (of 255) for each shape and length, with a bound
    /// just above what was measured on 2026-11-23, so a worse rule fails.
    #[test]
    fn a_long_curve_keeps_its_end_points_and_its_shape() {
        type Shape = fn(usize) -> Vec<(f64, f64)>;
        let cases: [(&str, Shape, usize, f64); 9] = [
            ("wavy", wavy, 17, 0.15),
            ("wavy", wavy, 24, 0.48),
            ("wavy", wavy, 40, 0.41),
            ("steep S", s_curve, 17, 0.09),
            ("steep S", s_curve, 24, 0.27),
            ("steep S", s_curve, 40, 0.24),
            ("clustered", clustered, 17, 0.01),
            ("clustered", clustered, 24, 0.04),
            ("clustered", clustered, 40, 0.26),
        ];
        for (name, shape, n, bound) in cases {
            let original = shape(n);
            let kept = fit_values(values(&original));
            assert_eq!(kept.len(), MAX_CURVE_POINTS);
            assert_eq!(kept[0]["x"], json!(original[0].0));
            assert_eq!(kept[MAX_CURVE_POINTS - 1]["x"], json!(original[n - 1].0));
            let worst = max_difference(&original, &pairs(&kept));
            eprintln!("{name} {n} -> {MAX_CURVE_POINTS}: max difference {worst:.3} of 255");
            assert!(worst <= bound, "{name} {n}: {worst} > {bound}");
        }
    }

    #[test]
    fn the_first_sixteen_would_be_far_off() {
        for shape in [wavy as fn(usize) -> Vec<(f64, f64)>, s_curve, clustered] {
            let original = shape(40);
            let first = &original[..MAX_CURVE_POINTS];
            let kept = pairs(&fit_values(values(&original)));
            assert!(max_difference(&original, first) > 4.0 * max_difference(&original, &kept));
        }
    }

    /// Budgets for the thinning, which runs on every render of a curve with too many points:
    /// measured on 2026-11-23 in the test profile (plain `cargo test`), each (1 ms and 3 ms
    /// measured) 20 ms, so only a slowdown to the old cubic cost fails them.
    const BUDGET_40_POINTS_MS: u128 = 20;
    const BUDGET_2000_POINTS_MS: u128 = 20;

    #[test]
    fn thinning_is_fast_and_a_long_curve_returns_promptly() {
        let forty = values(&s_curve(40));
        let started = std::time::Instant::now();
        let _ = kept_indices(&pairs(&forty));
        let ms = started.elapsed().as_millis();
        eprintln!("40 points: {ms} ms");
        assert!(ms <= BUDGET_40_POINTS_MS, "{ms} ms");

        let many = values(&clustered(2000));
        let started = std::time::Instant::now();
        let kept = fit_values(many);
        let ms = started.elapsed().as_millis();
        eprintln!("2000 points: {ms} ms");
        assert_eq!(kept.len(), MAX_CURVE_POINTS);
        assert!(ms <= BUDGET_2000_POINTS_MS, "{ms} ms");
    }

    #[test]
    fn the_same_curve_is_thinned_once() {
        let curve = values(&wavy(31));
        let first = fit_values(curve.clone());
        assert_eq!(fit_values(curve), first);
        assert!(remembered().lock().unwrap().len() <= REMEMBERED);
    }

    #[test]
    fn the_shader_has_the_same_size() {
        let source = include_str!("../shaders/shader.wgsl");
        let arrays = source
            .matches(&format!("array<Point, {MAX_CURVE_POINTS}>"))
            .count();
        assert_eq!(arrays, 9);
        assert!(source.contains(&format!("i < {}u; i = i + 1u", MAX_CURVE_POINTS - 1)));
    }

    #[test]
    fn the_limit_equals_the_one_in_the_page() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/argentum/curveLimit.ts");
        let source = std::fs::read_to_string(path).unwrap();
        let line = source
            .lines()
            .find(|l| l.starts_with("export const MAX_CURVE_POINTS"))
            .unwrap();
        let number: usize = line
            .split('=')
            .nth(1)
            .unwrap()
            .trim()
            .trim_end_matches(';')
            .trim()
            .parse()
            .unwrap();
        assert_eq!(number, MAX_CURVE_POINTS);
    }
}
