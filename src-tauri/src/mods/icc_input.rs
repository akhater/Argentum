//! A JPEG, TIFF, PNG or HEIC saved in a wider colour space than sRGB, shown
//! in its own colours.
//!
//! A photo saved in Adobe RGB, Display P3 or ProPhoto says so with an ICC
//! profile, or, from a camera set to Adobe RGB, with EXIF's colour-space tag
//! alone. Its numbers are that space's: read as sRGB, as they were, they look
//! washed out. Here they become what the rest of the app reads from any
//! non-RAW photo, sRGB-encoded floats.
//!
//! OUT OF sRGB
//!
//! A colour outside sRGB comes out of the conversion with a channel below
//! zero, above one, or both. Argentum's working RGB carries values above one
//! (a RAW's highlights, an EXR's), so those stay, for Highlights or Exposure
//! to bring back. It carries none below zero: the RAW decode clips there
//! (`raw_processing.rs`), and so does the CPU noise reduction. So a channel
//! below zero is clipped to zero, as a RAW's is. RapidRAW instead pulls the
//! pixel toward its brightest channel until none is below zero; measured on
//! every colour of a 17-step cube outside sRGB, that lands further from the
//! colour than clipping (mean CIEDE2000 4.7 against 4.3 in Adobe RGB, 3.0
//! against 2.7 in Display P3, 10.3 against 9.5 in ProPhoto; p99 12.1 against
//! 9.9, 8.6 against 7.2, 28.5 against 25.3: `icc_input_measure.rs`), so it
//! is not taken.
//!
//! WHAT IS LEFT ALONE
//!
//! A file with no profile, or whose profile is sRGB (or within 2/255 of it),
//! decodes bit for bit as before. Edits saved before the profile rule keep
//! the look they were made with (`highlight_rule.rs`).
//!
//! From RapidRAW `47e6274b` (`src-tauri/src/image_loader.rs`:
//! `is_exif_adobe_rgb`, `convert_profile_to_srgb`), cut to take any RGB
//! samples so the HEIC decode (`heic.rs`) shares it, and without its pull
//! of a colour outside sRGB toward its peak or its scaling of a pixel
//! brighter than white back to white.

use exif::{Exif, In, Tag};
use glam::{DMat3, Mat3, Vec3};
use moxcms::{
    CmsError, ColorProfile, DataColorSpace, Layout, RenderingIntent, ToneReprCurve,
    TransformOptions,
};
use rayon::prelude::*;

use crate::image_processing::linear_channel_to_srgb;

/// The colour space a decoded file's pixels are in, when it says one: its
/// embedded ICC profile, or Adobe RGB when only its EXIF says so. `None` for
/// a file that says neither (sRGB), or whose profile does not read.
pub fn source_profile(icc: Option<&[u8]>, exif: Option<&Exif>) -> Option<ColorProfile> {
    match icc {
        Some(icc) => ColorProfile::new_from_slice(icc).ok(),
        None => exif
            .is_some_and(is_exif_adobe_rgb)
            .then(ColorProfile::new_adobe_rgb),
    }
}

// RapidRAW 47e6274b: src-tauri/src/image_loader.rs
/// A camera set to Adobe RGB writes no profile: its EXIF says "uncalibrated"
/// and its interoperability index "R03" (DCF's option file).
fn is_exif_adobe_rgb(exif: &Exif) -> bool {
    let uncalibrated = exif
        .get_field(Tag::ColorSpace, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        == Some(0xFFFF);
    let interop_r03 = exif
        .get_field(Tag::InteroperabilityIndex, In::PRIMARY)
        .is_some_and(|f| f.display_value().to_string().contains("R03"));
    uncalibrated && interop_r03
}

// RapidRAW 47e6274b: src-tauri/src/image_loader.rs
fn with_linear_trc(mut profile: ColorProfile) -> ColorProfile {
    let identity = ToneReprCurve::Lut(Vec::new());
    profile.red_trc = Some(identity.clone());
    profile.green_trc = Some(identity.clone());
    profile.blue_trc = Some(identity);
    profile.cicp = None;
    profile
}

fn colorants(profile: &ColorProfile) -> DMat3 {
    DMat3::from_cols_array_2d(&profile.colorant_matrix().v).transpose()
}

/// Interleaved RGB samples, 0 to 1 in `source`'s encoding, converted in
/// place to sRGB-encoded floats (see the module's note for colours outside
/// sRGB). Samples in sRGB, or a profile within 2/255 of it, are left as they
/// are. An error leaves them as they are too: a profile that is not RGB, or
/// that moxcms cannot build a transform from.
pub fn convert_to_srgb(samples: &mut [f32], source: &ColorProfile) -> Result<(), CmsError> {
    if source.color_space != DataColorSpace::Rgb {
        return Err(CmsError::UnsupportedProfileConnection);
    }
    if samples.is_empty() {
        return Ok(());
    }
    // RapidRAW 47e6274b: src-tauri/src/image_loader.rs
    let srgb = ColorProfile::new_srgb();
    // A matrix profile is only linearised by moxcms and its matrix applied
    // here, so nothing outside sRGB is clipped; any other goes to linear sRGB.
    let (target, to_srgb) = if source.is_matrix_shaper() {
        let matrix = (colorants(&srgb).inverse() * colorants(source)).as_mat3();
        (with_linear_trc(source.clone()), matrix)
    } else {
        (with_linear_trc(srgb), Mat3::IDENTITY)
    };
    let options = TransformOptions {
        rendering_intent: RenderingIntent::RelativeColorimetric,
        ..TransformOptions::default()
    };
    let transform = source.create_transform_f32(Layout::Rgb, &target, Layout::Rgb, options)?;
    let to_linear_srgb = |src: &[f32], dst: &mut [f32]| {
        transform.transform(src, dst)?;
        for rgb in dst.as_chunks_mut::<3>().0 {
            *rgb = (to_srgb * Vec3::from_array(*rgb)).to_array();
        }
        Ok::<_, CmsError>(())
    };

    const STEPS: usize = 16;
    let probe: Vec<f32> = (0..(STEPS + 1).pow(3))
        .flat_map(|i| {
            [
                i % (STEPS + 1),
                i / (STEPS + 1) % (STEPS + 1),
                i / (STEPS + 1).pow(2),
            ]
            .map(|c| c as f32 / STEPS as f32)
        })
        .collect();
    let mut probe_out = vec![0.0f32; probe.len()];
    to_linear_srgb(&probe, &mut probe_out)?;
    let is_srgb = probe.iter().zip(&probe_out).all(|(input, linear)| {
        (linear_channel_to_srgb(*linear).min(1.0) - input).abs() < 2.0 / 255.0
    });
    if is_srgb {
        return Ok(());
    }

    // Argentum: chunks of whole pixels rather than rows, for any samples.
    const CHUNK: usize = 3 * 4096;
    let mut linear = vec![0.0f32; samples.len()];
    linear
        .par_chunks_mut(CHUNK)
        .zip(samples.par_chunks(CHUNK))
        .try_for_each(|(dst, src)| to_linear_srgb(src, dst))?;
    samples
        .par_chunks_exact_mut(3)
        .zip(linear.par_chunks_exact(3))
        .for_each(|(out, rgb)| {
            // Argentum: zero below zero, unbounded above one (see the note).
            for (o, c) in out.iter_mut().zip(rgb) {
                *o = linear_channel_to_srgb(*c);
            }
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use argentum_engine::colour::srgb_decode;

    fn converted(profile: &ColorProfile, rgb: [f32; 3]) -> [f32; 3] {
        let mut samples = rgb.to_vec();
        convert_to_srgb(&mut samples, profile).unwrap();
        [samples[0], samples[1], samples[2]]
    }

    /// The numbers moxcms gives going the other way: an sRGB colour written
    /// in `profile`'s encoding.
    fn encoded_in(profile: &ColorProfile, srgb: [f32; 3]) -> [f32; 3] {
        let transform = ColorProfile::new_srgb()
            .create_transform_f32(Layout::Rgb, profile, Layout::Rgb, Default::default())
            .unwrap();
        let mut out = [0.0; 3];
        transform.transform(&srgb, &mut out).unwrap();
        out
    }

    #[test]
    fn srgb_is_left_bit_for_bit() {
        let samples: Vec<f32> = (0..300).map(|i| (i % 256) as f32 / 255.0).collect();
        let mut out = samples.clone();
        convert_to_srgb(&mut out, &ColorProfile::new_srgb()).unwrap();
        assert_eq!(out, samples);
        // As is a profile that does not read, and a file that says nothing.
        assert!(source_profile(Some(b"not a profile"), None).is_none());
        assert!(source_profile(None, None).is_none());
    }

    #[test]
    fn wide_spaces_come_back_to_the_srgb_colour_they_were_made_from() {
        let colours = [
            [0.8, 0.2, 0.1],
            [0.1, 0.6, 0.3],
            [0.2, 0.3, 0.9],
            [0.5, 0.5, 0.5],
            [0.95, 0.85, 0.6],
        ];
        for profile in [
            ColorProfile::new_adobe_rgb(),
            ColorProfile::new_display_p3(),
            ColorProfile::new_pro_photo_rgb(),
        ] {
            for srgb in colours {
                let wide = encoded_in(&profile, srgb);
                let back = converted(&profile, wide);
                for (b, s) in back.iter().zip(srgb) {
                    assert!((b - s).abs() < 1.5e-3, "{srgb:?} came back {back:?}");
                }
            }
        }
    }

    /// Adobe RGB's pure green is outside sRGB: it is sRGB's green, nothing
    /// below zero.
    #[test]
    fn a_colour_outside_srgb_is_clipped_below_zero_and_kept_above_one() {
        let green = converted(&ColorProfile::new_adobe_rgb(), [0.0, 1.0, 0.0]);
        assert_eq!([green[0], green[2]], [0.0, 0.0], "{green:?}");
        assert!(green[1] > 0.99, "{green:?}");
        // ProPhoto's yellow is brighter than sRGB's white in red: kept.
        let yellow = converted(&ColorProfile::new_pro_photo_rgb(), [1.0, 1.0, 0.0]);
        assert!(yellow[0] > 1.0 && yellow[2] == 0.0, "{yellow:?}");
    }

    /// A camera set to Adobe RGB writes no profile, only EXIF's
    /// "uncalibrated" colour space and the R03 interoperability index.
    #[test]
    fn exif_alone_can_say_adobe_rgb() {
        use exif::{Field, Value};
        let exif_of = |space: u16, index: &[u8]| {
            let fields = [
                Field {
                    tag: Tag::ColorSpace,
                    ifd_num: In::PRIMARY,
                    value: Value::Short(vec![space]),
                },
                Field {
                    tag: Tag::InteroperabilityIndex,
                    ifd_num: In::PRIMARY,
                    value: Value::Ascii(vec![index.to_vec()]),
                },
            ];
            let mut writer = exif::experimental::Writer::new();
            fields.iter().for_each(|f| writer.push_field(f));
            let mut tiff = std::io::Cursor::new(Vec::new());
            writer.write(&mut tiff, false).unwrap();
            exif::Reader::new().read_raw(tiff.into_inner()).unwrap()
        };
        let adobe = source_profile(None, Some(&exif_of(0xFFFF, b"R03"))).expect("Adobe RGB");
        assert_eq!(
            adobe.red_colorant,
            ColorProfile::new_adobe_rgb().red_colorant
        );
        assert!(source_profile(None, Some(&exif_of(1, b"R98"))).is_none());
        assert!(source_profile(None, Some(&exif_of(0xFFFF, b"R98"))).is_none());
        // An embedded profile says more than EXIF.
        let srgb = ColorProfile::new_srgb().encode().unwrap();
        let tagged = source_profile(Some(&srgb), Some(&exif_of(0xFFFF, b"R03"))).unwrap();
        assert_eq!(tagged.red_colorant, ColorProfile::new_srgb().red_colorant);
    }

    #[test]
    fn a_profile_that_is_not_rgb_is_an_error_and_leaves_the_samples() {
        let mut gray = ColorProfile::new_srgb();
        gray.color_space = DataColorSpace::Gray;
        let mut samples = vec![0.25, 0.5, 0.75];
        assert!(convert_to_srgb(&mut samples, &gray).is_err());
        assert_eq!(samples, [0.25, 0.5, 0.75]);
    }

    /// Mid-grey stays mid-grey whatever the space (all three are D65 or
    /// adapted to it), down to the encoding's own rounding.
    #[test]
    fn grey_stays_grey() {
        for profile in [
            ColorProfile::new_adobe_rgb(),
            ColorProfile::new_pro_photo_rgb(),
        ] {
            let grey = encoded_in(&profile, [0.5; 3]);
            let back = converted(&profile, grey);
            let spread = back.iter().cloned().fold(f32::MIN, f32::max)
                - back.iter().cloned().fold(f32::MAX, f32::min);
            assert!(spread < 1e-3, "{back:?}");
            assert!((srgb_decode(back[1]) - srgb_decode(0.5)).abs() < 1e-3);
        }
    }
}
