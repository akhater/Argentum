//! A Lensfun calibration between the focal lengths it was measured at, as
//! Lensfun interpolates it. Ours, on Lensfun's method.
//!
//! Lensfun (`libs/lensfun/lens.cpp`: `InterpolateDistortion`, `InterpolateTCA`,
//! `__insert_spline` and `__parameter_scales`; `auxfun.cpp`: `_lf_interpolate`;
//! @ 52cf1a7b):
//!
//! - takes only the rows of the first model it meets, in the file's order;
//! - takes a row at the photo's focal length exactly as it is;
//! - otherwise takes the two rows nearest on either side and runs a cubic
//!   Hermite spline through the inner two, its tangents from the outer ones
//!   (half the difference across) or, at an end, from the inner pair, with `t`
//!   the photo's place between the inner two;
//! - multiplies each term by its row's focal length before and divides by the
//!   photo's after: distortion's terms and lateral CA's `c` and `b`, not lateral
//!   CA's `v`;
//! - beyond the calibrated focal lengths, takes the nearest row as it is.
//!
//! Argentum interpolated linearly between the two nearest rows, terms as they
//! are, which bends a zoom's correction away from Lensfun's between its
//! calibrated focal lengths.

/// `rows`' terms at `focal_mm`: `rows` are one model's (focal length, terms) in
/// the file's order, `by_focal` says which terms scale with the focal length.
/// `None` when there are no rows.
pub fn at_focal<const N: usize>(
    rows: &[(f32, [f64; N])],
    focal_mm: f32,
    by_focal: [bool; N],
) -> Option<[f64; N]> {
    // The rows nearest above (0, 1, the nearest at 1) and below (2, 3, the
    // nearest at 2), by the photo's focal length less the row's, as
    // `__insert_spline` keeps them.
    let mut spline: [Option<usize>; 4] = [None; 4];
    let mut dist = [f32::MIN, f32::MIN, f32::MAX, f32::MAX];
    for (i, (focal, terms)) in rows.iter().enumerate() {
        let d = focal_mm - focal;
        if d == 0. {
            return Some(*terms);
        }
        if d < 0. {
            if d > dist[1] {
                (dist[0], dist[1]) = (dist[1], d);
                (spline[0], spline[1]) = (spline[1], Some(i));
            } else if d > dist[0] {
                dist[0] = d;
                spline[0] = Some(i);
            }
        } else if d < dist[2] {
            (dist[3], dist[2]) = (dist[2], d);
            (spline[3], spline[2]) = (spline[2], Some(i));
        } else if d < dist[3] {
            dist[3] = d;
            spline[3] = Some(i);
        }
    }
    let (s1, s2) = match (spline[1], spline[2]) {
        (Some(a), Some(b)) => (a, b),
        (Some(one), None) | (None, Some(one)) => return Some(rows[one].1),
        (None, None) => return None,
    };
    let (f1, f2) = (rows[s1].0, rows[s2].0);
    let t = f64::from((focal_mm - f1) / (f2 - f1));
    Some(std::array::from_fn(|k| {
        let scale = |focal: f32| if by_focal[k] { f64::from(focal) } else { 1. };
        let y = |row: Option<usize>| row.map(|i| rows[i].1[k] * scale(rows[i].0));
        hermite([y(spline[0]), y(Some(s1)), y(Some(s2)), y(spline[3])], t) / scale(focal_mm)
    }))
}

/// `_lf_interpolate`: the cubic Hermite spline from `y[1]` (t = 0) to `y[2]`
/// (t = 1), tangents half the difference across the neighbours, or the inner
/// pair's difference where a neighbour is missing.
fn hermite(y: [Option<f64>; 4], t: f64) -> f64 {
    let (y2, y3) = (y[1].unwrap_or(0.), y[2].unwrap_or(0.));
    let tg2 = y[0].map_or(y3 - y2, |y1| (y3 - y1) * 0.5);
    let tg3 = y[3].map_or(y3 - y2, |y4| (y4 - y2) * 0.5);
    let (t2, t3) = (t * t, t * t * t);
    (2. * t3 - 3. * t2 + 1.) * y2
        + (t3 - 2. * t2 + t) * tg2
        + (-2. * t3 + 3. * t2) * y3
        + (t3 - t2) * tg3
}

#[cfg(test)]
mod tests {
    use super::*;

    const BY_FOCAL: [bool; 1] = [true];

    /// A row at the photo's focal length is taken as it is; beyond the ends, the
    /// nearest row; with nothing, nothing.
    #[test]
    fn rows_at_and_beyond_the_photo_s_focal_length() {
        let rows = [(24., [0.01]), (35., [0.02]), (70., [-0.01])];
        assert_eq!(at_focal(&rows, 35., BY_FOCAL), Some([0.02]));
        assert_eq!(at_focal(&rows, 18., BY_FOCAL), Some([0.01]));
        assert_eq!(at_focal(&rows, 105., BY_FOCAL), Some([-0.01]));
        assert_eq!(at_focal::<1>(&[], 50., BY_FOCAL), None);
    }

    /// Between two rows only: the spline through the terms times their focal
    /// lengths, tangent the pair's difference at both ends, divided by the
    /// photo's focal length. Halfway it is their mean, not the terms' mean.
    #[test]
    fn two_rows_meet_halfway_on_the_terms_times_focal_length() {
        let rows = [(20., [0.02]), (40., [0.01])];
        let mid = at_focal(&rows, 30., BY_FOCAL).unwrap()[0];
        assert!(
            (mid - (0.02 * 20. + 0.01 * 40.) / 2. / 30.).abs() < 1e-12,
            "{mid}"
        );
        // Lateral CA's v does not scale with the focal length.
        let v = at_focal(&[(20., [1.0002]), (40., [1.0004])], 30., [false]).unwrap()[0];
        assert!((v - 1.0003).abs() < 1e-12);
    }

    /// Four rows: Catmull-Rom tangents from the outer rows, as `_lf_interpolate`
    /// takes them, in any order the file lists them.
    #[test]
    fn the_outer_rows_give_the_tangents() {
        let rows = [(70., [0.04]), (24., [0.01]), (50., [0.03]), (35., [0.02])];
        let f = 40.;
        let t = (40. - 35.) / (50. - 35.);
        let y = [24. * 0.01, 35. * 0.02, 50. * 0.03, 70. * 0.04];
        let (tg2, tg3) = ((y[2] - y[0]) * 0.5, (y[3] - y[1]) * 0.5);
        let want = ((2. * t * t * t - 3. * t * t + 1.) * y[1]
            + (t * t * t - 2. * t * t + t) * tg2
            + (-2. * t * t * t + 3. * t * t) * y[2]
            + (t * t * t - t * t) * tg3)
            / f;
        // `t` is single precision, as Lensfun computes it.
        let got = at_focal(&rows, 40., BY_FOCAL).unwrap()[0];
        assert!((got - want).abs() < 1e-8, "{got} against {want}");
    }
}
