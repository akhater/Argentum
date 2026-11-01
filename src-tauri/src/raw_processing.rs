use crate::as_shot::AsShot;
use crate::image_processing::apply_orientation;
use crate::white_balance::WhiteBalance;
use anyhow::{Result, anyhow};
use argentum_engine::colour::srgb_decode;
use image::{DynamicImage, ImageBuffer, Rgb};
use rawler::{
    decoders::{Decoder, Orientation, RawDecodeParams},
    imgop::{
        develop::{DemosaicAlgorithm, Intermediate, ProcessingStep, RawDevelop},
        xyz::Illuminant,
    },
    rawimage::{RawImage, RawPhotometricInterpretation},
    rawsource::RawSource,
};
use rayon::prelude::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub fn develop_raw_image(
    file_bytes: &[u8],
    fast_demosaic: bool,
    highlight_compression: f32,
    linear_mode: String,
    cancel_token: Option<(Arc<AtomicUsize>, usize)>,
    photo_path: Option<&str>, // Argentum: which photo, so its own profile can be read
) -> Result<DynamicImage> {
    let (developed_image, orientation) = develop_internal(
        file_bytes,
        fast_demosaic,
        highlight_compression,
        linear_mode,
        cancel_token,
        photo_path,
    )?;
    Ok(apply_orientation(developed_image, orientation))
}

fn metadata_orientation(decoder: &dyn Decoder, source: &RawSource) -> Result<Orientation> {
    let metadata = decoder.raw_metadata(source, &RawDecodeParams::default())?;
    Ok(metadata
        .exif
        .orientation
        .map(Orientation::from_u16)
        .unwrap_or(Orientation::Normal))
}

pub fn extract_embedded_preview(file_bytes: &[u8]) -> Option<DynamicImage> {
    let source = RawSource::new_from_slice(file_bytes);
    let decoder = rawler::get_decoder(&source).ok()?;
    let preview = decoder
        .full_image(&source, &RawDecodeParams::default())
        .ok()??;
    let orientation =
        metadata_orientation(decoder.as_ref(), &source).unwrap_or(Orientation::Normal);
    Some(apply_orientation(preview, orientation))
}

fn is_linear_raw_format(raw_image: &RawImage) -> bool {
    matches!(
        raw_image.photometric,
        RawPhotometricInterpretation::LinearRaw
    )
}

fn decode_clamp_limit(fast_demosaic: bool) -> f32 {
    if fast_demosaic { 1.0 } else { 1000.0 }
}

fn develop_internal(
    file_bytes: &[u8],
    fast_demosaic: bool,
    _highlight_compression: f32,
    linear_mode: String,
    cancel_token: Option<(Arc<AtomicUsize>, usize)>,
    photo_path: Option<&str>, // Argentum
) -> Result<(DynamicImage, Orientation)> {
    let check_cancel = || -> Result<()> {
        if let Some((tracker, generation)) = &cancel_token
            && tracker.load(Ordering::SeqCst) != *generation
        {
            return Err(anyhow!("Load cancelled"));
        }
        Ok(())
    };

    check_cancel()?;

    let source = RawSource::new_from_slice(file_bytes);
    let decoder = rawler::get_decoder(&source)?;

    check_cancel()?;
    // Argentum: rawler, or FlashRaw's samples in rawler's image (mods/raw_decoder.rs)
    let mut raw_image: RawImage =
        crate::mods::raw_decoder::raw_image(&source, decoder.as_ref(), file_bytes, photo_path)?;
    crate::mods::decode::on_raw_decoded(&mut raw_image, file_bytes, photo_path); // Argentum: the single decode anchor, see mods/decode.rs

    let orientation = metadata_orientation(decoder.as_ref(), &source)?;

    let is_linear_format = is_linear_raw_format(&raw_image);

    let (apply_ungamma, apply_calibration) = match linear_mode.as_str() {
        "gamma" => (true, true),
        "skip_calib" => (false, false),
        "gamma_skip_calib" => (true, false),
        _ => (false, true),
    };

    let original_white_level = raw_image
        .whitelevel
        .0
        .first()
        .cloned()
        .unwrap_or(u16::MAX as u32) as f32;
    let original_black_level = raw_image
        .blacklevel
        .levels
        .first()
        .map(|r| r.as_f32())
        .unwrap_or(0.0);

    for level in raw_image.whitelevel.0.iter_mut() {
        *level = u32::MAX;
    }

    let mut developer = RawDevelop::default();

    if is_linear_format {
        developer.steps.retain(|&step| {
            step != ProcessingStep::SRgb
                && step != ProcessingStep::Demosaic
                && (apply_calibration || step != ProcessingStep::Calibrate)
        });
    } else if fast_demosaic {
        developer.demosaic_algorithm = DemosaicAlgorithm::Speed;
        developer.steps.retain(|&step| step != ProcessingStep::SRgb);
    } else {
        developer.steps.retain(|&step| step != ProcessingStep::SRgb);
    }

    raw_image.wb_coeffs =
        crate::multi_exposure::neutralize_wb_if_multiexposure(raw_image.wb_coeffs, file_bytes);
    // Argentum: the white balance this decode applies, for an edit made the DNG way (mods/dng_colour.rs)
    crate::mods::profile_correction::remember_colour(
        photo_path,
        crate::mods::dng_colour::inputs_of(&raw_image, !is_linear_format || apply_calibration),
    );

    check_cancel()?;
    let mut developed_intermediate = developer.develop_intermediate(&raw_image)?;

    drop(raw_image);

    let denominator = (original_white_level - original_black_level).max(1.0);
    let rescale_factor = (u32::MAX as f32 - original_black_level) / denominator;

    // Keep old `raw_highlight_compression` preferences readable, but no longer
    // use them to compress decoded values. The editor handles scene-linear
    // headroom; fast/thumbnail decode retains its existing 1.0 ceiling.
    let clamp_limit = decode_clamp_limit(fast_demosaic);

    let (width, height) = {
        let dim = developed_intermediate.dim();
        (dim.w as u32, dim.h as u32)
    };

    check_cancel()?;

    match &mut developed_intermediate {
        Intermediate::Monochrome(pixels) => {
            pixels.data.par_iter_mut().for_each(|p| {
                let mut linear_val = *p * rescale_factor;
                if is_linear_format && apply_ungamma {
                    linear_val = srgb_decode(linear_val.max(0.0));
                }
                *p = linear_val.clamp(0.0, clamp_limit);
            });
        }
        Intermediate::ThreeColor(pixels) => {
            pixels.data.par_iter_mut().for_each(|p| {
                let mut r = (p[0] * rescale_factor).max(0.0);
                let mut g = (p[1] * rescale_factor).max(0.0);
                let mut b = (p[2] * rescale_factor).max(0.0);

                if is_linear_format && apply_ungamma {
                    r = srgb_decode(r.max(0.0));
                    g = srgb_decode(g.max(0.0));
                    b = srgb_decode(b.max(0.0));
                }

                // Keep scene-linear values above nominal white for the editor's
                // tone mapping. Argentum's pre-demosaic recovery ran earlier in
                // Argentum's earlier decode hook; do not stack RapidRAW's
                // post-demosaic recovery or the superseded compression pass here.
                p[0] = r.clamp(0.0, clamp_limit);
                p[1] = g.clamp(0.0, clamp_limit);
                p[2] = b.clamp(0.0, clamp_limit);
            });
        }
        Intermediate::FourColor(pixels) => {
            pixels.data.par_iter_mut().for_each(|p| {
                p.iter_mut().for_each(|c| {
                    let mut linear_val = *c * rescale_factor;
                    if is_linear_format && apply_ungamma {
                        linear_val = srgb_decode(linear_val.max(0.0));
                    }
                    *c = linear_val.clamp(0.0, clamp_limit);
                });
            });
        }
    }

    check_cancel()?;

    // RapidRAW 346a49ea: src-tauri/src/raw_processing.rs. RGB, not RGBA: a
    // quarter less memory per developed photo, and the RGB pixels are moved
    // into the image rather than copied.
    let dynamic_image = match developed_intermediate {
        Intermediate::ThreeColor(pixels) => {
            DynamicImage::ImageRgb32F(rgb_pixels_to_rgb(pixels.data, width, height)?)
        }
        Intermediate::Monochrome(pixels) => {
            DynamicImage::ImageRgb32F(mono_pixels_to_rgb(&pixels.data, width, height)?)
        }
        _ => {
            return Err(anyhow!("Unsupported intermediate format for conversion"));
        }
    };

    Ok((dynamic_image, orientation))
}

pub fn read_as_shot(file_bytes: &[u8]) -> Option<AsShot> {
    let source = RawSource::new_from_slice(file_bytes);
    let decoder = rawler::get_decoder(&source).ok()?;
    let raw_image = decoder
        .raw_image(&source, &RawDecodeParams::default(), true)
        .ok()?;
    let is_monochrome = raw_image.cpp == 1
        && !matches!(raw_image.photometric, RawPhotometricInterpretation::Cfa(_));

    let as_shot_white = (!is_monochrome)
        .then(|| as_shot_white_balance(&raw_image, file_bytes))
        .flatten();
    Some(AsShot {
        white_balance: as_shot_white
            .as_ref()
            .map_or_else(WhiteBalance::reference, |(white, _)| *white),
        xyz_to_camera: as_shot_white.and_then(|(_, matrix)| matrix.try_into().ok()),
        exposure: crate::baseline_exposure::read(decoder.as_ref(), file_bytes, is_monochrome),
        wb_coeffs: crate::multi_exposure::neutralize_wb_if_multiexposure(
            raw_image.wb_coeffs,
            file_bytes,
        ),
    })
}

/// The as-shot white and the matrix it was solved with.
fn as_shot_white_balance(
    raw_image: &RawImage,
    file_bytes: &[u8],
) -> Option<(WhiteBalance, Vec<f32>)> {
    let wb_coeffs =
        crate::multi_exposure::neutralize_wb_if_multiexposure(raw_image.wb_coeffs, file_bytes);
    let neutral = if wb_coeffs[0].is_nan() {
        [1.0; 4]
    } else {
        wb_coeffs.map(|c| 1.0 / c)
    };

    let matrices = &raw_image.color_matrix;
    if let (Some(matrix_a), Some(matrix_d65)) =
        (matrices.get(&Illuminant::A), matrices.get(&Illuminant::D65))
    {
        return WhiteBalance::dual_illuminant_camera_neutral(matrix_a, matrix_d65, &neutral);
    }

    let color_matrix = matrices
        .get(&Illuminant::D65)
        .or_else(|| matrices.values().next())?;
    let white = WhiteBalance::from_camera_neutral(color_matrix, &neutral)?;
    Some((white, color_matrix.clone()))
}

fn rgb_pixels_to_rgb(
    data: Vec<[f32; 3]>,
    width: u32,
    height: u32,
) -> Result<ImageBuffer<Rgb<f32>, Vec<f32>>> {
    ImageBuffer::from_raw(width, height, data.into_flattened())
        .ok_or_else(|| anyhow!("Developed image size does not match its dimensions"))
}

fn mono_pixels_to_rgb(
    data: &[f32],
    width: u32,
    height: u32,
) -> Result<ImageBuffer<Rgb<f32>, Vec<f32>>> {
    let mut rgb = vec![0.0f32; data.len() * 3];
    rgb.par_chunks_exact_mut(3)
        .zip(data.par_iter())
        .for_each(|(dst, &p)| dst.fill(p));
    ImageBuffer::from_raw(width, height, rgb)
        .ok_or_else(|| anyhow!("Developed image size does not match its dimensions"))
}

pub fn get_fast_demosaic_scale_factor(
    file_bytes: &[u8],
    decoded_width: u32,
    decoded_height: u32,
) -> f32 {
    let source = RawSource::new_from_slice(file_bytes);
    if let Ok(decoder) = rawler::get_decoder(&source)
        && let Ok(raw_img) = decoder.raw_image(&source, &RawDecodeParams::default(), true)
    {
        let max_orig = (raw_img.width as f32).max(raw_img.height as f32);
        let max_comp = (decoded_width as f32).max(decoded_height as f32);
        if max_orig > 0.0 {
            let ratio = max_comp / max_orig;
            if ratio > 0.1 && ratio < 0.35 {
                return 0.25;
            } else if (0.35..0.75).contains(&ratio) {
                return 0.5;
            }
        }
    }
    1.0
}

#[cfg(test)]
mod tests {
    use super::{decode_clamp_limit, mono_pixels_to_rgb, rgb_pixels_to_rgb};

    #[test]
    fn full_quality_decode_keeps_headroom_to_the_v164_limit() {
        assert_eq!(decode_clamp_limit(false), 1000.0);
    }

    #[test]
    fn fast_decode_keeps_its_existing_one_white_limit() {
        assert_eq!(decode_clamp_limit(true), 1.0);
    }

    /// The develop's RGB keeps every value the RGBA it replaced held in its
    /// first three channels, in the same order.
    #[test]
    fn the_rgb_develop_keeps_every_value() {
        let rgb = rgb_pixels_to_rgb(vec![[0.1, 0.2, 0.3], [4.0, -0.5, 1e-6]], 2, 1).unwrap();
        assert_eq!(rgb.as_raw(), &[0.1, 0.2, 0.3, 4.0, -0.5, 1e-6]);
        let mono = mono_pixels_to_rgb(&[0.25, 7.5], 1, 2).unwrap();
        assert_eq!(mono.as_raw(), &[0.25, 0.25, 0.25, 7.5, 7.5, 7.5]);
        assert!(
            rgb_pixels_to_rgb(vec![[0.; 3]], 2, 1).is_err(),
            "too few pixels"
        );
    }
}
