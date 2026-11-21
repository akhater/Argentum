//! A generated image (HDR, panorama, focus stack, denoise, positive, restore
//! or enlargement) saved beside its source, carrying the source's EXIF and,
//! as a TIFF, the sRGB profile. Ours, after RapidRAW's `save_converted_image`.
//!
//! The EXIF goes in through the export's own writer (`export_metadata`), so a
//! generated file says what an export of the same photo says: camera, lens,
//! date, GPS. Orientation is written as 1, as for an export; a generated file
//! that is not upright already gets its rotation from its edits file, as
//! before. The name comes from `file_ops::write_unique_output`, so an earlier
//! result is never replaced.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use image::{DynamicImage, ImageFormat};

/// Saves `image` as `format` in `dir` under the first free `stem[_N].extension`,
/// with the EXIF of the photo at `source`. A source without readable EXIF
/// gives a file without it, never a failed save.
// RapidRAW 36bb789a: src-tauri/src/exif_processing.rs `save_converted_image`
// (metadata embedded in conversion outputs; float TIFFs stay float).
pub fn save(
    dir: &Path,
    stem: &str,
    extension: &str,
    image: &DynamicImage,
    format: ImageFormat,
    source: &Path,
) -> Result<PathBuf, String> {
    let bytes = encode(image, format, extension, source)?;
    super::file_ops::write_unique_output(dir, stem, extension, |file| file.write_all(&bytes))
        .map_err(|e| e.to_string())
}

/// `image` encoded as `format`, with the EXIF of `source` written in.
fn encode(
    image: &DynamicImage,
    format: ImageFormat,
    extension: &str,
    source: &Path,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut into = Cursor::new(&mut bytes);
    match (format, image) {
        (ImageFormat::Tiff, DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_)) => {
            super::export_precision::encode_float_tiff(image, &mut into)?
        }
        (ImageFormat::Tiff, _) => super::export_precision::encode_tiff(image, &mut into, None)?,
        _ => image
            .write_to(&mut into, format)
            .map_err(|e| e.to_string())?,
    }

    let no_xmp = crate::exif_processing::ExportXmp::from_metadata(
        &crate::image_processing::ImageMetadata::default(),
    );
    if let Err(e) = super::export_metadata::write_export_metadata(
        &mut bytes,
        &source.to_string_lossy(),
        extension,
        true,
        false,
        &no_xmp,
        None,
    ) {
        log::warn!(
            "Could not copy the EXIF of {} into its generated image: {e}",
            source.display()
        );
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};
    use little_exif::exif_tag::ExifTag;
    use little_exif::filetype::FileExtension;
    use little_exif::metadata::Metadata;

    /// A JPEG photo carrying the tags a camera writes.
    fn camera_jpeg(dir: &Path) -> PathBuf {
        let path = dir.join("IMG_1.jpg");
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(ImageBuffer::from_pixel(8, 8, Rgb([9_u8, 9, 9])))
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Jpeg)
            .unwrap();
        let mut metadata = Metadata::new();
        metadata.set_tag(ExifTag::Make("TestCam Industries".to_string()));
        metadata.set_tag(ExifTag::Model("TC-1".to_string()));
        metadata
            .write_to_vec(&mut bytes, FileExtension::JPEG)
            .unwrap();
        std::fs::write(&path, &bytes).unwrap();
        path
    }

    fn make(bytes: &[u8], file_type: FileExtension) -> Option<String> {
        let metadata = Metadata::new_from_vec(&bytes.to_vec(), file_type).ok()?;
        metadata.into_iter().find_map(|tag| match tag {
            ExifTag::Make(make) => Some(make.trim_end_matches('\0').to_string()),
            _ => None,
        })
    }

    fn icc_profile(bytes: &[u8]) -> Option<Vec<u8>> {
        let mut decoder = tiff::decoder::Decoder::new(Cursor::new(bytes)).unwrap();
        decoder.get_tag_u8_vec(tiff::tags::Tag::IccProfile).ok()
    }

    /// A 16-bit TIFF (stack, denoise, positive) keeps its pixels and gains the
    /// camera's EXIF and the sRGB profile.
    #[test]
    fn a_16_bit_tiff_gets_the_camera_exif_and_the_profile() {
        let dir = tempfile::tempdir().unwrap();
        let source = camera_jpeg(dir.path());
        let image = DynamicImage::ImageRgb16(ImageBuffer::from_fn(5, 4, |x, y| {
            Rgb([x as u16 * 9000, y as u16 * 11000, 1234])
        }));
        let path = save(
            dir.path(),
            "IMG_1_Stacked",
            "tiff",
            &image,
            ImageFormat::Tiff,
            &source,
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(path.file_name().unwrap(), "IMG_1_Stacked.tiff");
        assert_eq!(
            make(&bytes, FileExtension::TIFF).as_deref(),
            Some("TestCam Industries")
        );
        assert_eq!(
            icc_profile(&bytes).as_deref(),
            Some(crate::export_processing::SRGB_ICC_PROFILE)
        );
        assert_eq!(
            image::load_from_memory(&bytes).unwrap().to_rgb16(),
            image.to_rgb16()
        );
    }

    /// A merged HDR or panorama stays a float TIFF.
    #[test]
    fn a_float_tiff_stays_float_and_gets_the_camera_exif() {
        let dir = tempfile::tempdir().unwrap();
        let source = camera_jpeg(dir.path());
        let image = DynamicImage::ImageRgb32F(ImageBuffer::from_fn(5, 4, |x, y| {
            Rgb([x as f32 * 0.21, y as f32 * 0.3, 1.75])
        }));
        let path = save(
            dir.path(),
            "IMG_1_Hdr",
            "tiff",
            &image,
            ImageFormat::Tiff,
            &source,
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            make(&bytes, FileExtension::TIFF).as_deref(),
            Some("TestCam Industries")
        );
        assert_eq!(
            icc_profile(&bytes).as_deref(),
            Some(crate::export_processing::SRGB_ICC_PROFILE)
        );
        let read = image::load_from_memory(&bytes).unwrap();
        assert!(read.as_rgb32f().is_some(), "{:?}", read.color());
        assert_eq!(read.to_rgb32f(), image.to_rgb32f());
    }

    /// A PNG (a non-RAW denoised, an HDR with alpha) gets the EXIF too.
    #[test]
    fn a_png_gets_the_camera_exif() {
        let dir = tempfile::tempdir().unwrap();
        let source = camera_jpeg(dir.path());
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(5, 4, Rgb([10_u8, 20, 30])));
        let path = save(
            dir.path(),
            "IMG_1_Denoised",
            "png",
            &image,
            ImageFormat::Png,
            &source,
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            make(
                &bytes,
                FileExtension::PNG {
                    as_zTXt_chunk: true
                }
            )
            .as_deref(),
            Some("TestCam Industries")
        );
        assert_eq!(
            image::load_from_memory(&bytes).unwrap().to_rgb8(),
            image.to_rgb8()
        );
    }

    /// A source with nothing to read still saves the image, under a new name
    /// beside an earlier one.
    #[test]
    fn a_source_without_exif_still_saves_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("gone.CR2");
        let image = DynamicImage::ImageRgb16(ImageBuffer::from_pixel(3, 2, Rgb([1_u16, 2, 3])));
        let first = save(
            dir.path(),
            "IMG_Positive",
            "tiff",
            &image,
            ImageFormat::Tiff,
            &missing,
        )
        .unwrap();
        let second = save(
            dir.path(),
            "IMG_Positive",
            "tiff",
            &image,
            ImageFormat::Tiff,
            &missing,
        )
        .unwrap();
        assert_eq!(first.file_name().unwrap(), "IMG_Positive.tiff");
        assert_eq!(second.file_name().unwrap(), "IMG_Positive_1.tiff");
        assert_eq!(image::open(&second).unwrap().to_rgb16(), image.to_rgb16());
    }
}
