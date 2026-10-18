//! Colour maths on the CPU, defined once: the frame histogram's luminance,
//! and the sRGB curve the tests judge pictures by. On the GPU the same
//! formulas are `src/pixels/common.wgsl`.
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

#[cfg(test)]
mod tests {
    use super::*;

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
