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
//!
//! Vignetting is calibrated over focal length, aperture and distance, and
//! Lensfun weights all its rows by their distance from the photo instead
//! (`vignetting_at`).

use crate::lens_correction::Vignetting;

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

/// The inverse distance weighting's power (`InterpolateVignetting`, "p = 3.5").
const IDW_POWER: f64 = 3.5;

/// `rows`' vignetting terms (k₁, k₂, k₃) at a photo's focal length, f-number
/// and subject distance (m), as Lensfun interpolates them
/// (`libs/lensfun/lens.cpp`: `lfLens::InterpolateVignetting`,
/// `__vignetting_dist` and `__parameter_scales` @ 52cf1a7b), in single
/// precision as Lensfun computes them:
///
/// - only the rows of the first model met, in the file's order;
/// - each row's distance from the photo over the focal length, normalised by
///   the lens's range `focal_range` (`mods/lens_focal_span.rs`), 4/f-number
///   and 0.1/distance; a row at the photo's point (closer than 1e-4) is taken
///   as it is;
/// - otherwise every row weighted by 1/distance^3.5; pa terms do not scale
///   with the focal length;
/// - `None`, no correction, when no row is within 1 of the photo.
///
/// Argentum took the nearest aperture, then the nearest distance, at the two
/// calibrated focal lengths around the photo's and drew a straight line.
pub fn vignetting_at(
    rows: &[&Vignetting],
    focal_range: [f32; 2],
    focal_mm: f32,
    f_number: f32,
    distance_m: f32,
) -> Option<[f64; 3]> {
    let [min_focal, max_focal] = focal_range;
    let span = max_focal - min_focal;
    let normalised = |focal: f32| {
        let f = focal - min_focal;
        if span != 0. { f / span } else { f }
    };
    // `4.0 / aperture` and `0.1 / distance` are double literals over floats,
    // stored as floats.
    let reciprocal = |scale: f64, x: f32| (scale / f64::from(x)) as f32;
    let (f1, a1, d1) = (
        normalised(focal_mm),
        reciprocal(4.0, f_number),
        reciprocal(0.1, distance_m),
    );

    let mut model: Option<&str> = None;
    let mut terms = [0f32; 3];
    let mut total_weighting = 0f32;
    let mut smallest = f32::MAX;
    for row in rows {
        // A "none" row leaves the model unset, as LF_VIGNETTING_MODEL_NONE does.
        match model {
            None => model = Some(row.model.as_str()).filter(|m| *m != "none"),
            Some(m) if m != row.model => continue,
            Some(_) => {}
        }
        let row_terms = [row.k1, row.k2, row.k3].map(|k| k.unwrap_or(0.));
        let f2 = normalised(row.focal);
        let a2 = reciprocal(4.0, row.aperture);
        // A row with no distance reads 0 (Lensfun's `memset`): infinitely far.
        let d2 = reciprocal(0.1, row.distance.unwrap_or(0.));
        let square = |x: f32| x * x;
        let distance = (square(f2 - f1) + square(a2 - a1) + square(d2 - d1)).sqrt();
        if f64::from(distance) < 0.0001 {
            return Some(row_terms.map(f64::from));
        }
        smallest = smallest.min(distance);
        let weighting = (1.0 / f64::from(distance).powf(IDW_POWER)).abs() as f32;
        for (sum, k) in terms.iter_mut().zip(row_terms) {
            *sum += weighting * k;
        }
        total_weighting += weighting;
    }
    if smallest > 1. || !(total_weighting > 0. && smallest < f32::MAX) {
        return None;
    }
    Some(terms.map(|sum| f64::from(sum / total_weighting)))
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

    /// A lens of `rows` (focal, f-number, distance, k₁) and a focal range.
    fn vignetting_lens(
        range: [f32; 2],
        rows: &[(f32, f32, f32, f32)],
    ) -> crate::lens_correction::Lens {
        let rows: String = rows
            .iter()
            .map(|(f, a, d, k1)| {
                format!(r#"<vignetting model="pa" focal="{f}" aperture="{a}" distance="{d}" k1="{k1}" k2="0" k3="0"/>"#)
            })
            .collect();
        quick_xml::de::from_str(&format!(
            r#"<lens><model>Test</model><focal min="{}" max="{}"/><calibration>{rows}</calibration></lens>"#,
            range[0], range[1]
        ))
        .expect("a lens")
    }

    /// Lensfun's weighting over (focal over the range, 4/f-number, 0.1/distance)
    /// in double precision, from `lens.cpp` as written, for the tests to hold
    /// `vignetting_at` to.
    fn lensfun_idw(
        range: [f32; 2],
        rows: &[(f32, f32, f32, [f64; 3])],
        focal: f32,
        f_number: f32,
        distance: f32,
    ) -> Option<[f64; 3]> {
        let span = f64::from(range[1] - range[0]);
        let at = |f: f32, a: f32, d: f32| {
            [
                f64::from(f - range[0]) / if span != 0. { span } else { 1. },
                4. / f64::from(a),
                0.1 / f64::from(d),
            ]
        };
        let photo = at(focal, f_number, distance);
        let (mut sum, mut total, mut smallest) = ([0.; 3], 0., f64::INFINITY);
        for (f, a, d, k) in rows {
            let row = at(*f, *a, *d);
            let x = (0..3)
                .map(|i| (row[i] - photo[i]).powi(2))
                .sum::<f64>()
                .sqrt();
            if x < 1e-4 {
                return Some(*k);
            }
            smallest = smallest.min(x);
            let w = 1. / x.powf(3.5);
            (0..3).for_each(|i| sum[i] += w * k[i]);
            total += w;
        }
        (smallest <= 1.).then(|| sum.map(|s| s / total))
    }

    /// A photo at f/4 on a lens calibrated at f/2.8 and f/5.6: Lensfun weights
    /// both rows by their distance over 4/f-number, where Argentum took f/2.8's
    /// row alone.
    #[test]
    fn an_aperture_between_two_calibrations_weights_both() {
        let lens = vignetting_lens(
            [24., 70.],
            &[(50., 2.8, 1000., -0.5), (50., 5.6, 1000., -0.1)],
        );
        let p = lens.get_distortion_params(50., Some(4.), None).unwrap();
        let want = lensfun_idw(
            [24., 70.],
            &[
                (50., 2.8, 1000., [-0.5, 0., 0.]),
                (50., 5.6, 1000., [-0.1, 0., 0.]),
            ],
            50.,
            4.,
            1000.,
        )
        .unwrap()[0];
        assert!(
            (p.vig_k1 - want).abs() < 1e-6,
            "{} against Lensfun's {want}",
            p.vig_k1
        );
        // Between the two, nearer f/5.6 on Lensfun's 4/f-number axis.
        assert!(want > -0.3 && want < -0.1, "{want}");
    }

    /// Focal length, aperture and distance together: every row counts, the
    /// focal length over the lens's range, the distance as 0.1/distance; a
    /// photo with no distance is at 1000 m, as darktable assumes.
    #[test]
    fn every_row_counts_by_its_distance_from_the_photo() {
        let rows = [
            (24., 2.8, 1., -0.6),
            (24., 8., 1000., -0.2),
            (70., 4., 10., -0.4),
            (70., 11., 1000., -0.05),
        ];
        let lens = vignetting_lens([24., 70.], &rows);
        let oracle: Vec<_> = rows
            .iter()
            .map(|(f, a, d, k)| (*f, *a, *d, [f64::from(*k), 0., 0.]))
            .collect();
        for (focal, f_number, distance) in [
            (35., 5.6, None),
            (50., 3.5, Some(2.)),
            (24., 2.8, Some(1000.)),
        ] {
            let p = lens
                .get_distortion_params(focal, Some(f_number), distance)
                .unwrap();
            let want = lensfun_idw(
                [24., 70.],
                &oracle,
                focal,
                f_number,
                distance.unwrap_or(1000.),
            )
            .unwrap()[0];
            assert!(
                (p.vig_k1 - want).abs() < 1e-6,
                "{focal} mm f/{f_number} {distance:?}: {} against Lensfun's {want}",
                p.vig_k1
            );
        }
    }

    /// A row at the photo's point is taken as it is; a photo further than 1
    /// from every row gets no correction, as Lensfun gives none.
    #[test]
    fn a_row_at_the_photo_or_none_within_reach() {
        let lens = vignetting_lens(
            [24., 70.],
            &[(24., 1.4, 1000., -0.7), (70., 1.4, 1000., -0.6)],
        );
        let p = lens.get_distortion_params(24., Some(1.4), None).unwrap();
        assert_eq!(p.vig_k1, f64::from(-0.7f32));
        // 4/1.4 - 4/16 = 2.6 from both rows.
        let p = lens.get_distortion_params(24., Some(16.), None).unwrap();
        assert_eq!([p.vig_k1, p.vig_k2, p.vig_k3], [0.; 3]);
    }

    /// Lensfun's interpolation against Argentum's over the bundled database, at
    /// each calibrated focal length and f-number and halfway between (focal
    /// lengths linearly, f-numbers on 4/f-number), at 1000 m: the corner gain
    /// `1/(1 + k₁ + k₂ + k₃)` off Lensfun's, in EV, and how many points move
    /// further from it than `AG_BEFORE` (a file this writes when absent).
    ///
    /// ```text
    /// AG_BEFORE=<file> cargo test --lib lens_interp::tests::vignetting_interpolation_against_lensfun -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "measurement over the bundled database"]
    fn vignetting_interpolation_against_lensfun() {
        use crate::lens_correction::CalibrationElement as E;
        let db = crate::lens_correction::read_lensfun_dir(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/lensfun_db"
        )));
        let gain = |k: [f64; 3]| 1. / (1. + k[0] + k[1] + k[2]);
        let (mut errors, mut lenses, mut no_row, mut corrected_anyway) = (Vec::new(), 0, 0, 0);
        let mut points = Vec::new();
        for lens in &db.lenses {
            let rows: Vec<(f32, f32, f32, [f64; 3])> = lens
                .calibration
                .iter()
                .flat_map(|c| &c.elements)
                .filter_map(|e| match e {
                    E::Vignetting(v) if v.model == "pa" => Some((
                        v.focal,
                        v.aperture,
                        v.distance.unwrap_or(0.),
                        [v.k1, v.k2, v.k3].map(|k| f64::from(k.unwrap_or(0.))),
                    )),
                    _ => None,
                })
                .collect();
            if rows.len() < 2 {
                continue;
            }
            lenses += 1;
            let range = crate::mods::lens_focal_span::lensfun_focal_range(lens);
            let sorted = |mut v: Vec<f32>| {
                v.sort_by(f32::total_cmp);
                v.dedup();
                v
            };
            let focals = sorted(rows.iter().map(|r| r.0).collect());
            let apertures = sorted(rows.iter().map(|r| r.1).collect());
            let mut focal_points = focals.clone();
            focal_points.extend(focals.windows(2).map(|w| (w[0] + w[1]) / 2.));
            let mut aperture_points = apertures.clone();
            aperture_points.extend(
                apertures
                    .windows(2)
                    .map(|w| 4. / ((4. / w[0] + 4. / w[1]) / 2.)),
            );
            for &f in &focal_points {
                for &a in &aperture_points {
                    let ours = lens.get_distortion_params(f, Some(a), None).unwrap();
                    let ours = [ours.vig_k1, ours.vig_k2, ours.vig_k3];
                    let Some(lensfun) = lensfun_idw(range, &rows, f, a, 1000.) else {
                        no_row += 1;
                        corrected_anyway += usize::from(ours != [0.; 3]);
                        continue;
                    };
                    let (gl, ga) = (gain(lensfun), gain(ours));
                    // NaN where a factor reaches 0: both sides' gains are unbounded.
                    errors.push(if gl > 0. && ga > 0. {
                        (gl / ga).log2().abs()
                    } else {
                        f64::NAN
                    });
                    points.push(format!(
                        "{} at {f} mm f/{a}: Lensfun 1 + k = {:.6}, ours {:.6}",
                        crate::lens_correction::untranslated(&lens.model),
                        1. / gl,
                        1. / ga
                    ));
                }
            }
        }
        let path = std::env::var("AG_BEFORE").ok();
        let before: Option<Vec<f64>> = path
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| t.lines().map(|l| l.parse().unwrap()).collect());
        let further = before.as_ref().map(|b| {
            assert_eq!(b.len(), errors.len(), "the same points");
            let further: Vec<usize> = (0..errors.len())
                .filter(|&i| match (b[i].is_nan(), errors[i].is_nan()) {
                    (false, false) => errors[i] > b[i] + 1e-6,
                    (true, _) => false,
                    (false, true) => true,
                })
                .collect();
            for &i in &further {
                println!("further: {} ({} -> {} EV)", points[i], b[i], errors[i]);
            }
            further.len()
        });
        if before.is_none()
            && let Some(p) = &path
        {
            let text: Vec<String> = errors.iter().map(f64::to_string).collect();
            std::fs::write(p, text.join("\n")).unwrap();
        }
        if let Some(worst) = (0..errors.len())
            .filter(|&i| !errors[i].is_nan())
            .max_by(|&i, &j| errors[i].total_cmp(&errors[j]))
        {
            println!("furthest: {} ({} EV)", points[worst], errors[worst]);
        }
        let mut finite: Vec<f64> = errors.iter().copied().filter(|e| !e.is_nan()).collect();
        finite.sort_by(f64::total_cmp);
        let n = finite.len();
        println!(
            "lenses {lenses}; points {n} ({} with a factor at or below 0; {no_row} where Lensfun has no row within reach, {corrected_anyway} of them corrected anyway); corner gain off Lensfun's, EV: median {:.4}, p95 {:.4}, max {:.4}; over 0.1 EV {}, over 3 EV {}; further than before: {further:?}",
            errors.len() - n,
            finite[n / 2],
            finite[n * 95 / 100],
            finite[n - 1],
            finite.iter().filter(|e| **e > 0.1).count(),
            finite.iter().filter(|e| **e > 3.).count(),
        );
    }
}
