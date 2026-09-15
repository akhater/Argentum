//! Canon sRAW / mRAW black and white levels, set the way rawspeed sets them.
//!
//! THE BUG
//!
//! Canon's small-RAW formats do not store sensor data. They store the picture as
//! luma and chroma, already black-subtracted and already white-balanced by the
//! camera. rawler converts that back to RGB correctly — its arithmetic matches
//! dcraw and rawspeed line for line — and then treats the result like ordinary
//! sensor data: it subtracts the sensor black level (1023 on a 5D Mark II) and
//! takes a white level from its camera table (64948) rather than from the file.
//!
//! Neither is right for this format. rawspeed, which darktable uses, is explicit
//! about it (`Cr2Decoder::decodeMetaDataInternal`): for sRAW the black level is
//! zero and the white point is the file's specular white shifted up two bits,
//! because the conversion scales the data up by four.
//!
//! WHY IT LOOKED LIKE A COLOUR PROBLEM
//!
//! Before white balance, red sits at roughly half of green in this data — the
//! stored coefficients that undo the camera's balance are `[545, 1170, 721]`.
//! Subtracting the same 1023 from every channel therefore takes about 18% off
//! red and 8% off green. White balance then scales that gap up, and the camera
//! matrix, which has large off-diagonal terms for Canon, amplifies it again.
//! Measured on one of AK's files, linear, against darktable at the same stage:
//!
//! ```text
//!                    R/G      B/G
//! darktable         0.97     1.03
//! rawler            0.53     0.80
//! ```
//!
//! It is also the source of the crushed shadows: the darkest sRAW values sit at
//! 100–300 counts, so subtracting 1023 sends them below zero, where they clamp.
//! On a backlit frame that was 40% of the picture, pure black before any tool
//! could touch it. Every downstream fix that tried to compensate — the D50/D65
//! matrix correction, the exposure experiments, the tone curves — was fighting
//! this one subtraction.
//!
//! GENERAL, NOT PER CAMERA
//!
//! The trigger is the format, not the model: three channels straight out of the
//! decoder in a Canon file. The white level is read from the file itself, using
//! Canon's own ColorData layout — the same version table rawspeed and rawler
//! both carry, which covers every body that shoots sRAW. Nothing here names a
//! camera. If the layout is not understood the black level is still zeroed,
//! because that part does not depend on the layout at all, and rawler's white
//! level is kept.
//!
//! WHY HERE AND NOT IN RAWLER
//!
//! rawler is an upstream dependency pulled by git; patching it would mean
//! carrying a fork of a fork. Overriding two public fields on the decoded image
//! costs one line in `raw_processing.rs` and is trivially removable if the fix
//! lands upstream.
//!
//! A SECOND sRAW PROBLEM, WHICH IS NOT OURS
//!
//! rawler's CR2 decoder can be asked for a *dummy* image: allocated, never
//! filled, so the caller can read its dimensions without paying for a decode.
//! RapidRAW asks for one to size thumbnails. For sRAW the decoder hands that
//! uninitialised buffer to `convert_to_rgb` regardless, and `pixels_mut()`
//! asserts on exactly that — so every sRAW thumbnail died in a dev build, four
//! at a time, while release was fine because the assertion is compiled out.
//!
//! The fix is `debug-assertions = false` for dependencies in
//! `src-tauri/Cargo.toml`, beside the `opt-level = 3` that is already there:
//! dependencies in this project are built as release code, and asserting inside
//! them contradicts that. It silences a real bug in rawler rather than fixing
//! it, which is the honest description — the bug is theirs, the dimensions it
//! returns are right, and the alternative is patching their decoder.

use rawler::rawimage::{BlackLevel, RawImage, WhiteLevel};

/// Canon MakerNote tag 0x4001: ColorData, an array of u16.
const CANON_COLOR_DATA: u16 = 0x4001;

/// ColorData is hundreds of shorts; a tiny or absurd count is not it.
const PLAUSIBLE_LENGTH: std::ops::RangeInclusive<usize> = 64..=8192;

/// Set the levels an sRAW / mRAW actually has. A no-op for everything else.
pub fn fix(raw: &mut RawImage, file_bytes: &[u8]) {
    if !is_canon_small_raw(raw) {
        return;
    }

    // The conversion output is already black-subtracted. Always.
    let zero = [0u16; 3];
    raw.blacklevel = BlackLevel::new(&zero, 1, 1, raw.cpp);

    // The white point is in the file; the camera table's guess is close but not
    // equal, and equal is the point.
    if let Some(white) = specular_white(file_bytes) {
        let scaled = shift_up_two_bits(white);
        raw.whitelevel = WhiteLevel(vec![scaled; raw.cpp]);
    }
}

/// Three channels out of a Canon decoder is a small RAW; sensor data is one.
fn is_canon_small_raw(raw: &RawImage) -> bool {
    raw.cpp == 3 && raw.clean_make.eq_ignore_ascii_case("canon")
}

/// rawspeed's rule: the interpolated data is four times the stored range, so the
/// white point moves with it. A power-of-two-minus-one white stays one.
pub fn shift_up_two_bits(white: u16) -> u32 {
    let w = white as u32;
    if (w + 1).is_power_of_two() {
        ((w + 1) << 2) - 1
    } else {
        w << 2
    }
}

/// The index of the specular white level inside ColorData, by layout version.
///
/// Canon's ColorData layout changed with generations; the version word at index
/// 0 says which. This is the table exiftool documents and rawspeed and rawler
/// both use. Older layouts (versions 1–3, the 40D era) carry no white level.
pub fn specular_white_index(version: i16, len: usize) -> Option<usize> {
    Some(match version {
        4 | 5 => 0x2b9,
        6 | 7 => 0x2d0,
        9 => 0x2d4,
        10 => {
            if len == 1273 || len == 1275 {
                0x1e4
            } else {
                0x1fd
            }
        }
        11 => 0x2dd,
        12 | 13 | 15 => 0x30f,
        14 => 0x231,
        16..=19 => 0x31d,
        32 | 33 => 0x32b,
        34 => 0x281,
        48 => 0x282,
        64..=66 => 0x295,
        -4 => 0x56a,
        _ => return None,
    })
}

/// The specular white level the camera wrote into ColorData, if the file has
/// one and the layout is understood.
fn specular_white(file_bytes: &[u8]) -> Option<u16> {
    let data = color_data(file_bytes)?;
    let version = *data.first()? as i16;
    let index = specular_white_index(version, data.len())?;
    let white = *data.get(index)?;
    (white > 0).then_some(white)
}

/// Read ColorData out of the Canon MakerNote as a vector of u16.
fn color_data(file_bytes: &[u8]) -> Option<Vec<u16>> {
    super::canon_makernote::shorts(file_bytes, CANON_COLOR_DATA, PLAUSIBLE_LENGTH)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// rawspeed's own numbers for a 5D Mark II: specular white 15821 becomes
    /// 63284; a 14-bit full-scale 16383 becomes 65535, not 65532.
    #[test]
    fn white_point_scales_like_rawspeed() {
        assert_eq!(shift_up_two_bits(15821), 63284);
        assert_eq!(shift_up_two_bits(16383), 65535);
        assert_eq!(shift_up_two_bits(4095), 16383);
    }

    /// The layout AK's files use, and the ones that carry no white level.
    #[test]
    fn knows_the_layouts() {
        assert_eq!(specular_white_index(6, 1250), Some(0x2d0));
        assert_eq!(specular_white_index(10, 1273), Some(0x1e4));
        assert_eq!(specular_white_index(10, 1312), Some(0x1fd));
        assert_eq!(
            specular_white_index(1, 800),
            None,
            "40D era has no white level"
        );
        assert_eq!(
            specular_white_index(99, 1250),
            None,
            "unknown layouts are declined"
        );
    }
}
