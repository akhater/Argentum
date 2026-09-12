//! What colour the screen actually shows. Ours.
//!
//! THE BUG THIS EXISTS FOR
//!
//! Argentum's pipeline produces sRGB. The native preview handed those numbers
//! to the panel untouched — `gpu_processing.rs` asks for a non-sRGB swapchain
//! format and `display.wgsl` returns what it sampled — which is correct only if
//! the panel *is* sRGB. On a wide-gamut display it is not: the same numbers
//! drive more saturated primaries, so everything came out over-saturated, and
//! no amount of looking at it would say so, because there was nothing beside it
//! to compare against.
//!
//! There was, though. The crop view goes out as a JPEG in an HTML `<img>`,
//! which WebView2 colour-manages properly, so the two views of the same photo
//! disagreed. An audit measured the difference and found it was exactly this
//! monitor's gamut conversion — mean error 0.5 of 255 against a matrix derived
//! from the installed profile, against 3.5 without it.
//!
//! WHAT THIS DOES
//!
//! Reads the ICC profile the operating system has for the display and works out
//! the matrix from sRGB to that display's primaries, so the shader can convert
//! before presenting.
//!
//! It must follow the display and not the machine: a laptop panel and an
//! external monitor have different profiles, and a window dragged between them
//! changes which one applies. So the profile is resolved per window, from the
//! monitor that window is on, and re-resolved when it moves.
//!
//! WHAT IT DELIBERATELY IS NOT
//!
//! Not a full ICC transform. The profile's tone curves are read and ignored:
//! the audit found that applying them was a *worse* match than the primaries
//! with a plain sRGB curve, which is what Windows itself appears to do for this
//! class of profile. Matching the rest of the system matters more here than
//! matching the specification, and claiming more than was measured would be
//! worse than doing less.
//!
//! Not a way to make the old picture come back either. The old picture was
//! wrong. Anything that restored it — saturation, a different matrix — would be
//! inventing colour to match a bug.

use crate::mods::profile_matrix::{invert, multiply};

/// sRGB primaries to XYZ, D65. The same constant rawler and the correction use.
const SRGB_TO_XYZ_D65: [[f32; 3]; 3] = [
    [0.412_456_4, 0.357_576_1, 0.180_437_5],
    [0.212_672_9, 0.715_152_2, 0.072_175_0],
    [0.019_333_9, 0.119_192_0, 0.950_304_1],
];

/// Bradford, D65 to D50.
///
/// An ICC profile's colourants are stored against D50 — that is the connection
/// space the format is defined in — so sRGB has to be adapted to D50 before the
/// two can be compared. Getting this wrong tilts the whole conversion towards
/// blue or yellow, which is exactly the kind of error that looks like a
/// white-balance problem and is not one.
const BRADFORD_D65_TO_D50: [[f32; 3]; 3] = [
    [1.047_811_2, 0.022_886_6, -0.050_127_0],
    [0.029_542_4, 0.990_484_4, -0.017_049_1],
    [-0.009_234_5, 0.015_043_6, 0.752_131_6],
];

/// No conversion. What a genuine sRGB display needs, and the safe answer
/// whenever the profile cannot be read.
pub const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// The red, green and blue a display actually produces, in XYZ D50.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colourants {
    pub red: [f32; 3],
    pub green: [f32; 3],
    pub blue: [f32; 3],
}

/// Read the three colourant tags out of an ICC profile.
///
/// Only the three tags that matter are read, and nothing is trusted: a profile
/// file is data from outside the program, it can be truncated or malformed, and
/// the answer to any of that is `None` rather than a panic in the render path.
pub fn colourants(icc: &[u8]) -> Option<Colourants> {
    // Header is 128 bytes, then a four-byte tag count, then 12 bytes per tag.
    if icc.len() < 132 {
        return None;
    }
    let count = be_u32(icc, 128)? as usize;
    // A profile with thousands of tags is not one; refuse rather than walk it.
    if count > 256 {
        return None;
    }

    let find = |wanted: &[u8; 4]| -> Option<[f32; 3]> {
        for i in 0..count {
            let entry = 132 + i * 12;
            let signature = icc.get(entry..entry + 4)?;
            if signature != wanted {
                continue;
            }
            let offset = be_u32(icc, entry + 4)? as usize;
            let size = be_u32(icc, entry + 8)? as usize;
            return xyz_tag(icc.get(offset..offset.checked_add(size)?)?);
        }
        None
    };

    Some(Colourants {
        red: find(b"rXYZ")?,
        green: find(b"gXYZ")?,
        blue: find(b"bXYZ")?,
    })
}

/// An `XYZType` tag: four bytes of signature, four reserved, then three
/// s15Fixed16 numbers.
fn xyz_tag(tag: &[u8]) -> Option<[f32; 3]> {
    if tag.len() < 20 || &tag[0..4] != b"XYZ " {
        return None;
    }
    Some([
        s15_fixed16(tag, 8)?,
        s15_fixed16(tag, 12)?,
        s15_fixed16(tag, 16)?,
    ])
}

fn be_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at + 4)?;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// ICC's fixed-point number: signed, sixteen bits each side of the point.
fn s15_fixed16(bytes: &[u8], at: usize) -> Option<f32> {
    let slice = bytes.get(at..at + 4)?;
    let raw = i32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]);
    Some(raw as f32 / 65536.0)
}

/// The matrix from sRGB to this display's own RGB.
///
/// sRGB is adapted to D50 to meet the profile's connection space, then taken
/// out of XYZ through the inverse of what the display's own primaries produce.
/// A display whose primaries *are* sRGB's gives back the identity, which is the
/// arithmetic saying "nothing to do" rather than a special case.
///
/// `None` when the colourants describe something that cannot be inverted —
/// three primaries that do not span a space — which is a broken profile, not a
/// display to render for.
pub fn srgb_to_display(c: &Colourants) -> Option<[[f32; 3]; 3]> {
    // Columns are where each of the display's primaries lands in XYZ.
    let display_to_xyz = [
        [c.red[0], c.green[0], c.blue[0]],
        [c.red[1], c.green[1], c.blue[1]],
        [c.red[2], c.green[2], c.blue[2]],
    ];
    let xyz_to_display = invert(&display_to_xyz)?;
    let srgb_to_xyz_d50 = multiply(&BRADFORD_D65_TO_D50, &SRGB_TO_XYZ_D65);
    Some(multiply(&xyz_to_display, &srgb_to_xyz_d50))
}

/// Read a profile from a file and work out the conversion in one step.
pub fn from_file(path: &std::path::Path) -> Option<[[f32; 3]; 3]> {
    let bytes = std::fs::read(path).ok()?;
    srgb_to_display(&colourants(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
        [
            m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
            m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
            m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
        ]
    }

    /// sRGB's own primaries, adapted to D50 — what an sRGB display's profile
    /// holds. Published values, not computed here, so this is a real check.
    const SRGB_COLOURANTS: Colourants = Colourants {
        red: [0.436_065, 0.222_488, 0.013_916],
        green: [0.385_147, 0.716_873, 0.097_076],
        blue: [0.143_066, 0.060_623, 0.714_096],
    };

    /// A display that is sRGB needs nothing done to it, and the arithmetic has
    /// to say so on its own.
    #[test]
    fn an_srgb_display_gets_the_identity() {
        let m = srgb_to_display(&SRGB_COLOURANTS).expect("invertible");
        for i in 0..3 {
            for j in 0..3 {
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((m[i][j] - want).abs() < 2e-3, "not the identity: {m:?}");
            }
        }
    }

    /// White has to stay white on any display, or the conversion would be a
    /// white balance change — which is not what this is for and would look like
    /// one of our own bugs.
    #[test]
    fn white_stays_white_on_a_wide_gamut_display() {
        // Rec. 2020-ish primaries: much wider than sRGB, adapted to D50.
        let wide = Colourants {
            red: [0.6734, 0.2790, -0.0019],
            green: [0.1656, 0.6757, 0.0299],
            blue: [0.1250, 0.0453, 0.7969],
        };
        let m = srgb_to_display(&wide).expect("invertible");
        let white = apply(&m, [1.0, 1.0, 1.0]);
        assert!(
            (white[0] - white[1]).abs() < 0.01 && (white[1] - white[2]).abs() < 0.01,
            "white came out as {white:?}"
        );
    }

    /// And a wide-gamut display must be told to use *less* of each primary for
    /// a saturated colour, which is the whole point: the same number means more
    /// colour there, so the number has to come down.
    #[test]
    fn a_wide_gamut_display_is_given_less_saturation() {
        let wide = Colourants {
            red: [0.6734, 0.2790, -0.0019],
            green: [0.1656, 0.6757, 0.0299],
            blue: [0.1250, 0.0453, 0.7969],
        };
        let m = srgb_to_display(&wide).expect("invertible");
        let red = apply(&m, [1.0, 0.0, 0.0]);
        assert!(red[0] < 1.0, "full red stayed full: {red:?}");
        assert!(
            red[1] > 0.0 || red[2] > 0.0,
            "red lost its other channels: {red:?}"
        );
    }

    #[test]
    fn rubbish_is_refused_rather_than_guessed_at() {
        assert!(colourants(&[]).is_none());
        assert!(colourants(&[0u8; 200]).is_none());
        // Three identical primaries span nothing and cannot be inverted.
        let flat = Colourants {
            red: [1.0, 1.0, 1.0],
            green: [1.0, 1.0, 1.0],
            blue: [1.0, 1.0, 1.0],
        };
        assert!(srgb_to_display(&flat).is_none());
    }

    /// Truncation must not panic. These files come from the operating system,
    /// but they are still files.
    #[test]
    fn truncation_never_panics() {
        let mut icc = vec![0u8; 400];
        icc[128..132].copy_from_slice(&3u32.to_be_bytes());
        for cut in 0..icc.len() {
            let _ = colourants(&icc[..cut]);
        }
    }
}

/// Against the profile on this machine, which is the one the audit measured.
#[cfg(test)]
mod against_a_real_profile {
    use super::*;

    #[test]
    #[ignore = "reads the installed display profile; run by hand"]
    fn it_matches_what_the_audit_measured() {
        let path = std::env::var("AG_ICC").expect("set AG_ICC to a display profile");
        let m = from_file(std::path::Path::new(&path)).expect("readable profile");

        println!("\nsRGB to display:");
        for row in &m {
            println!("  {:9.6} {:9.6} {:9.6}", row[0], row[1], row[2]);
        }

        // What the audit derived from the same file, independently.
        const AUDIT: [[f32; 3]; 3] = [
            [0.733831, 0.238423, 0.025512],
            [0.033035, 0.957447, 0.010285],
            [0.017213, 0.079285, 0.904307],
        ];
        let mut worst = 0.0f32;
        for i in 0..3 {
            for j in 0..3 {
                worst = worst.max((m[i][j] - AUDIT[i][j]).abs());
            }
        }
        println!("worst difference from the audit's matrix: {worst:.6}\n");
        assert!(
            worst < 0.01,
            "this does not agree with the audit's own derivation"
        );
    }
}

/// The uniform's shape: three rows of four, the fourth column carrying whether
/// there is anything to do.
pub type ShaderRows = [[f32; 4]; 3];

/// No conversion, in the shape the shader expects.
pub const SHADER_IDENTITY: ShaderRows = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// Pack a conversion for the shader, with the flag set.
pub fn shader_rows(m: &[[f32; 3]; 3]) -> ShaderRows {
    [
        [m[0][0], m[0][1], m[0][2], 1.0],
        [m[1][0], m[1][1], m[1][2], 1.0],
        [m[2][0], m[2][1], m[2][2], 1.0],
    ]
}

#[cfg(test)]
mod shader_tests {
    use super::*;

    /// The flag is what the shader reads to decide whether to do anything, and
    /// the two constants have to disagree about it or the whole thing is either
    /// always on or always off.
    #[test]
    fn the_flag_says_which_is_which() {
        assert_eq!(
            SHADER_IDENTITY[0][3], 0.0,
            "identity must be marked as nothing to do"
        );
        let rows = shader_rows(&IDENTITY);
        assert_eq!(
            rows[0][3], 1.0,
            "a real conversion must be marked as something to do"
        );
    }

    /// And the numbers have to arrive in the order the shader multiplies them.
    #[test]
    fn the_rows_keep_their_order() {
        let m = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]];
        let rows = shader_rows(&m);
        for i in 0..3 {
            for j in 0..3 {
                assert_eq!(rows[i][j], m[i][j], "row {i} column {j} moved");
            }
        }
    }
}
