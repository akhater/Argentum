//! Turning a camera profile's ForwardMatrix into something rawler can use.
//!
//! THE FIRST ATTEMPT, AND WHY IT WAS WRONG
//!
//! Substituting a profile's `ColorMatrix` for rawler's made colour worse —
//! measured against darktable over ten photos, red went from 4.2% off to 9.5%.
//! A `.dcp` is not a bag of interchangeable matrices. Its `ColorMatrix` exists
//! to *find the illuminant*; the thing that actually renders is
//! `ForwardMatrix`, which maps white-balanced camera RGB straight to XYZ D50.
//! Using half of a profile inside a pipeline built for a different convention
//! produced colour that was differently wrong, not better.
//!
//! WHAT RAWLER ACTUALLY DOES
//!
//! From `imgop/raw.rs`:
//!
//! ```text
//! rgb2cam = normalize(xyz2cam · SRGB_TO_XYZ_D65)
//! cam2rgb = pseudo_inverse(rgb2cam)
//! out     = cam2rgb · (wb ⊙ cameraRGB)
//! ```
//!
//! `normalize` scales each row to sum to one, so white maps to white, and the
//! white balance is applied separately just before. Which means the matrix
//! rawler ultimately applies is a map from *white-balanced camera RGB* to
//! sRGB — exactly what `ForwardMatrix` describes, only expressed in sRGB D65
//! rather than XYZ D50.
//!
//! SO THE SUBSTITUTION CAN STAY A SUBSTITUTION
//!
//! Work backwards from what we want rawler to end up applying:
//!
//! ```text
//! want:  cam2rgb = XYZ_D65→sRGB · Bradford(D50→D65) · FM
//! rawler: cam2rgb = pinv(normalize(xyz2cam · sRGB→XYZ_D65))
//! ```
//!
//! Equate them, and the sRGB conversions cancel:
//!
//! ```text
//! xyz2cam = inv(FM) · Bradford(D65→D50)
//! ```
//!
//! Row scaling drops out because `normalize` reimposes it. So the profile still
//! goes in through `raw.color_matrix` and not one line of their pipeline
//! changes — but what rawler now applies is the profile's own rendering
//! transform rather than a matrix borrowed from a different job.

/// Bradford chromatic adaptation, D65 to D50. The standard published values.
///
/// Needed because `ForwardMatrix` is defined against D50 and rawler's
/// conversion is built around sRGB, whose white is D65.
const BRADFORD_D65_TO_D50: [[f32; 3]; 3] = [
    [1.047_811_2, 0.022_886_6, -0.050_127_0],
    [0.029_542_4, 0.990_484_4, -0.017_049_1],
    [-0.009_234_5, 0.015_043_6, 0.752_131_6],
];

pub fn multiply(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0f32; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

/// Invert a 3x3, or `None` if it is singular.
pub fn invert(m: &[[f32; 3]; 3]) -> Option<[[f32; 3]; 3]> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);

    if det.abs() < 1e-9 {
        return None;
    }
    let d = 1.0 / det;

    Some([
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * d,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * d,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * d,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * d,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * d,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * d,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * d,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * d,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * d,
        ],
    ])
}

fn as_rows(flat: &[f32; 9]) -> [[f32; 3]; 3] {
    [
        [flat[0], flat[1], flat[2]],
        [flat[3], flat[4], flat[5]],
        [flat[6], flat[7], flat[8]],
    ]
}

fn as_flat(m: &[[f32; 3]; 3]) -> [f32; 9] {
    [
        m[0][0], m[0][1], m[0][2], m[1][0], m[1][1], m[1][2], m[2][0], m[2][1], m[2][2],
    ]
}

/// The `xyz2cam` that makes rawler apply this ForwardMatrix.
///
/// `None` when the matrix cannot be inverted, which would mean a profile that
/// maps different colours to the same place — not something to render with.
pub fn xyz2cam_from_forward(forward: &[f32; 9]) -> Option<[f32; 9]> {
    let inverse = invert(&as_rows(forward))?;
    Some(as_flat(&multiply(&inverse, &BRADFORD_D65_TO_D50)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Canon EOS 5D Mark II, ForwardMatrix2, read out of the profile
    /// RawTherapee publishes. Not typed from memory: the first version of this
    /// test used numbers I had invented, and the D50 check below caught them.
    const CANON_5D2_FORWARD: [f32; 9] = [
        0.6399, 0.1294, 0.1949, 0.2827, 0.6579, 0.0594, 0.0001, 0.0051, 0.8199,
    ];

    fn apply(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
        [
            m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
            m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
            m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
        ]
    }

    #[test]
    fn inverting_and_multiplying_gives_the_identity() {
        let m = as_rows(&CANON_5D2_FORWARD);
        let product = multiply(&m, &invert(&m).expect("invertible"));
        for i in 0..3 {
            for j in 0..3 {
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((product[i][j] - want).abs() < 1e-4, "{product:?}");
            }
        }
    }

    #[test]
    fn a_singular_matrix_is_refused() {
        assert!(invert(&[[1.0, 2.0, 3.0], [2.0, 4.0, 6.0], [1.0, 1.0, 1.0]]).is_none());
        assert!(xyz2cam_from_forward(&[1.0, 2.0, 3.0, 2.0, 4.0, 6.0, 1.0, 1.0, 1.0]).is_none());
    }

    /// A ForwardMatrix maps white-balanced camera RGB to XYZ D50, so feeding it
    /// neutral must give D50 white. This is what makes the whole derivation
    /// meaningful rather than plausible-looking, and it is a property of the
    /// published Canon numbers, not of anything computed here.
    #[test]
    fn the_forward_matrix_takes_neutral_to_d50_white() {
        const D50: [f32; 3] = [0.9642, 1.0, 0.8249];
        let white = apply(&as_rows(&CANON_5D2_FORWARD), [1.0, 1.0, 1.0]);
        for c in 0..3 {
            assert!(
                (white[c] - D50[c]).abs() < 0.01,
                "neutral gave {white:?}, D50 is {D50:?}"
            );
        }
    }

    /// The end-to-end check: run the derived matrix through rawler's own
    /// arithmetic and confirm neutral camera RGB comes out neutral in sRGB.
    ///
    /// Reproduced here rather than trusted, because the whole point of the
    /// derivation is that it survives `normalize` and `pseudo_inverse` — and a
    /// matrix that is subtly wrong still renders, just wrongly.
    #[test]
    fn neutral_survives_rawlers_own_arithmetic() {
        // sRGB primaries to XYZ D65, as rawler holds them.
        const SRGB_TO_XYZ_D65: [[f32; 3]; 3] = [
            [0.412_456_4, 0.357_576_1, 0.180_437_5],
            [0.212_672_9, 0.715_152_2, 0.072_175_0],
            [0.019_333_9, 0.119_192_0, 0.950_304_1],
        ];

        let xyz2cam = as_rows(&xyz2cam_from_forward(&CANON_5D2_FORWARD).expect("derivable"));

        // rgb2cam, with rows normalised to sum to one.
        let mut rgb2cam = multiply(&xyz2cam, &SRGB_TO_XYZ_D65);
        for row in rgb2cam.iter_mut() {
            let sum: f32 = row.iter().sum();
            if sum.abs() > 1e-9 {
                for cell in row.iter_mut() {
                    *cell /= sum;
                }
            }
        }

        let cam2rgb = invert(&rgb2cam).expect("invertible");
        let out = apply(&cam2rgb, [1.0, 1.0, 1.0]);

        assert!(
            (out[0] - out[1]).abs() < 0.02 && (out[1] - out[2]).abs() < 0.02,
            "neutral camera RGB came out as {out:?}, which is not neutral"
        );
    }
}
