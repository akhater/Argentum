//! Colour maths on the CPU, defined once for the engine and the app:
//! luminance, the sRGB curve, the sRGB-to-XYZ and Bradford matrices, and the
//! colour difference the tests measure with (`lab`, `delta_e2000`). On the
//! GPU the first two are `src/pixels/common.wgsl`.
//!
//! RapidRAW's, from `src-tauri/src/shaders/shader.wgsl` @ c786c146
//! (`LUMA_COEFF`, `get_luma`, `linear_to_srgb`, `srgb_to_linear`), written
//! for one value at a time.

/// Rec. 709 luminance weights for linear sRGB: `LUMA_COEFF`.
pub const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// Luminance: `get_luma`.
pub fn luma(c: [f32; 3]) -> f32 {
    c[0] * LUMA[0] + c[1] * LUMA[1] + c[2] * LUMA[2]
}

/// Linear light to the sRGB curve, clipped to 0..1 first: `linear_to_srgb`.
pub fn srgb_encode(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// The sRGB curve back to linear light: `srgb_to_linear`.
pub fn srgb_decode(e: f32) -> f32 {
    if e <= 0.040_45 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear sRGB (D65) to CIE XYZ, row by row: the constant rawler holds.
pub const SRGB_TO_XYZ_D65: [[f32; 3]; 3] = [
    [0.412_456_4, 0.357_576_1, 0.180_437_5],
    [0.212_672_9, 0.715_152_2, 0.072_175_0],
    [0.019_333_9, 0.119_192, 0.950_304_1],
];

/// Bradford chromatic adaptation, XYZ D65 to XYZ D50, row by row: the
/// standard published values. ICC colourants and DNG forward matrices are
/// stored against D50.
pub const BRADFORD_D65_TO_D50: [[f32; 3]; 3] = [
    [1.047_811_2, 0.022_886_6, -0.050_127_0],
    [0.029_542_4, 0.990_484_4, -0.017_049_1],
    [-0.009_234_5, 0.015_043_6, 0.752_131_6],
];

/// The Bradford cone response, XYZ to LMS, that white balance adapts in.
pub const BRADFORD: glam::DMat3 = glam::DMat3::from_cols_array(&[
    0.8951, -0.7502, 0.0389, 0.2664, 1.7135, -0.0685, -0.1614, 0.0367, 1.0296,
]);

/// Linear sRGB to linear ProPhoto, Bradford-adapted; modules.wgsl's
/// AG_SRGB_TO_PROPHOTO, row by row.
pub const SRGB_TO_PROPHOTO: [[f32; 3]; 3] = [
    [0.5293458, 0.3300728, 0.1405813],
    [0.0983744, 0.873461, 0.0281647],
    [0.0168832, 0.1176725, 0.8654443],
];

/// Linear ProPhoto back to linear sRGB: modules.wgsl's AG_PROPHOTO_TO_SRGB,
/// row by row.
pub const PROPHOTO_TO_SRGB: [[f32; 3]; 3] = [
    [2.0340764, -0.7273343, -0.3067417],
    [-0.2288134, 1.2317302, -0.0029169],
    [-0.0085698, -0.1532866, 1.1618564],
];

/// `m` times `v`, `m` row by row.
pub fn mul(m: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    m.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
}

// Copied from RAWmakase `crates/rawmakase-decode/src/half_check.rs`
// @ 183e09fd (pch/rawmakase#526, MIT): `lab` and `delta_e2000`, on f32
// pixels here. Used by the engine's tests and the app's look check
// (src-tauri/src/mods/look_check.rs).

/// CIELAB (D65) of linear sRGB.
pub fn lab([r, g, b]: [f32; 3]) -> [f64; 3] {
    let [r, g, b] = [r as f64, g as f64, b as f64];
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f64| {
        if t > 216. / 24389. {
            t.cbrt()
        } else {
            (24389. / 27. * t + 16.) / 116.
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116. * fy - 16., 500. * (fx - fy), 200. * (fy - fz)]
}

/// CIEDE2000 colour difference (Sharma, Wu and Dalal 2005).
pub fn delta_e2000(lab1: [f64; 3], lab2: [f64; 3]) -> f64 {
    let [l1, a1, b1] = lab1;
    let [l2, a2, b2] = lab2;
    let c_bar = (a1.hypot(b1) + a2.hypot(b2)) / 2.;
    let g = 0.5 * (1. - (c_bar.powi(7) / (c_bar.powi(7) + 25f64.powi(7))).sqrt());
    let (a1p, a2p) = ((1. + g) * a1, (1. + g) * a2);
    let (c1p, c2p) = (a1p.hypot(b1), a2p.hypot(b2));
    let hue = |b: f64, a: f64| {
        if a == 0. && b == 0. {
            0.
        } else {
            b.atan2(a).to_degrees().rem_euclid(360.)
        }
    };
    let (h1p, h2p) = (hue(b1, a1p), hue(b2, a2p));
    let dl = l2 - l1;
    let dc = c2p - c1p;
    let dh = if c1p * c2p == 0. {
        0.
    } else if (h2p - h1p).abs() <= 180. {
        h2p - h1p
    } else if h2p - h1p > 180. {
        h2p - h1p - 360.
    } else {
        h2p - h1p + 360.
    };
    let dhh = 2. * (c1p * c2p).sqrt() * (dh / 2.).to_radians().sin();
    let l_bar = (l1 + l2) / 2.;
    let cp_bar = (c1p + c2p) / 2.;
    let hp_bar = if c1p * c2p == 0. {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180. {
        (h1p + h2p) / 2.
    } else if h1p + h2p < 360. {
        (h1p + h2p + 360.) / 2.
    } else {
        (h1p + h2p - 360.) / 2.
    };
    let t = 1. - 0.17 * (hp_bar - 30.).to_radians().cos()
        + 0.24 * (2. * hp_bar).to_radians().cos()
        + 0.32 * (3. * hp_bar + 6.).to_radians().cos()
        - 0.20 * (4. * hp_bar - 63.).to_radians().cos();
    let d_theta = 30. * (-((hp_bar - 275.) / 25.).powi(2)).exp();
    let rc = 2. * (cp_bar.powi(7) / (cp_bar.powi(7) + 25f64.powi(7))).sqrt();
    let sl = 1. + 0.015 * (l_bar - 50.).powi(2) / (20. + (l_bar - 50.).powi(2)).sqrt();
    let sc = 1. + 0.045 * cp_bar;
    let sh = 1. + 0.015 * cp_bar * t;
    let rt = -(2. * d_theta).to_radians().sin() * rc;
    ((dl / sl).powi(2) + (dc / sc).powi(2) + (dhh / sh).powi(2) + rt * (dc / sc) * (dhh / sh))
        .sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ciede2000_matches_published_pairs() {
        // Sharma, Wu and Dalal's test data, pairs 1, 7 and 17 (RAWmakase's test).
        for (a, b, want) in [
            ([50., 2.6772, -79.7751], [50., 0., -82.7485], 2.0425),
            ([50., 0., 0.], [50., -1., 2.], 2.3669),
            ([50., 2.5, 0.], [73., 25., -18.], 27.1492),
        ] {
            let got = delta_e2000(a, b);
            assert!((got - want).abs() < 1e-4, "{got} for {want}");
        }
    }

    #[test]
    fn the_srgb_curve_goes_there_and_back() {
        for i in 0..=1000 {
            let v = i as f32 / 1000.0;
            assert!((srgb_decode(srgb_encode(v)) - v).abs() < 2e-6, "{v}");
        }
        assert_eq!(srgb_encode(2.0), srgb_encode(1.0), "clipped at white");
        assert_eq!(srgb_encode(-1.0), 0.0);
    }

    #[test]
    fn white_has_a_luminance_of_one() {
        assert!((luma([1.0; 3]) - 1.0).abs() < 1e-6);
    }
}
