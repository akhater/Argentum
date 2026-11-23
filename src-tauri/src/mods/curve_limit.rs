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

/// The largest difference, over 0..=255 in steps of a quarter, between two curves.
pub fn max_difference(a: &[(f64, f64)], b: &[(f64, f64)]) -> f64 {
    (0..=1020)
        .map(|i| {
            let x = f64::from(i) / 4.0;
            (curve_at(a, x) - curve_at(b, x)).abs()
        })
        .fold(0.0, f64::max)
}

/// The indices of the points to keep, in order: all of them when there are `MAX_CURVE_POINTS` or
/// fewer, else that many chosen as the module header says.
pub fn kept_indices(points: &[(f64, f64)]) -> Vec<usize> {
    let mut kept: Vec<usize> = (0..points.len()).collect();
    while kept.len() > MAX_CURVE_POINTS {
        let mut best = (f64::MAX, 1);
        for at in 1..kept.len() - 1 {
            let without: Vec<(f64, f64)> = kept
                .iter()
                .enumerate()
                .filter(|(k, _)| *k != at)
                .map(|(_, &i)| points[i])
                .collect();
            let error = max_difference(points, &without);
            if error < best.0 {
                best = (error, at);
            }
        }
        kept.remove(best.1);
    }
    kept
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
    kept_indices(&xy)
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

    /// `n` points along a wavy curve, x from 0 to 255.
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

    #[test]
    fn a_curve_within_the_limit_is_untouched() {
        let p = values(&wavy(MAX_CURVE_POINTS));
        assert_eq!(fit_values(p.clone()), p);
    }

    #[test]
    fn a_long_curve_keeps_its_end_points_and_its_shape() {
        for n in [17, 24, 40] {
            let original = wavy(n);
            let kept = fit_values(values(&original));
            assert_eq!(kept.len(), MAX_CURVE_POINTS);
            assert_eq!(kept[0]["x"], json!(original[0].0));
            assert_eq!(kept[MAX_CURVE_POINTS - 1]["x"], json!(original[n - 1].0));
            let kept_xy: Vec<(f64, f64)> = kept
                .iter()
                .map(|p| (p["x"].as_f64().unwrap(), p["y"].as_f64().unwrap()))
                .collect();
            let worst = max_difference(&original, &kept_xy);
            eprintln!("{n} points -> {MAX_CURVE_POINTS}: max difference {worst:.3} of 255");
            assert!(worst < 2.0, "{n} points: {worst}");
        }
    }

    #[test]
    fn the_first_sixteen_would_be_far_off() {
        let original = wavy(40);
        let first = &original[..MAX_CURVE_POINTS];
        assert!(max_difference(&original, first) > 20.0);
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
