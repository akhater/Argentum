//! JPEG XL export: how it is encoded, what the encoder costs in memory, and
//! where its XMP goes.
//!
//! - Lossless encodes without tree learning. With it the encoder learned a
//!   modular tree from half the pixels: about 300 B/px at its peak, so a batch
//!   running several at once ran out of memory. Measured on the five sheet
//!   RAWs, one at a time: peak 9.08 GB -> 2.71 GB, 46 min -> 1-3 min, files
//!   13% larger.
//! - The encoder's own memory joins a batch's per-photo budget, so fewer
//!   JPEG XL photos are encoded at once.
//! - Keep metadata writes EXIF (little_exif wraps the bare codestream in a
//!   container) and the XMP packet as an `xml ` box.

use jxl_encoder::LosslessConfig;

use crate::export_processing::{ResizeMode, ResizeOptions};

/// The lossless encoder's settings.
// RapidRAW 892bfa30: src-tauri/src/export_processing.rs
pub fn lossless() -> LosslessConfig {
    LosslessConfig::new().with_tree_learning(false)
}

/// The encoder's peak memory per output pixel: RapidRAW's figures (36 MP),
/// above what the five sheet RAWs measured (8-bit RGB, 32 MP and 5 MP,
/// 2026-11-21): lossless 84 B/px, lossy (distance 1) 127 B/px. With tree
/// learning, lossless was 281 B/px.
const LOSSLESS_BYTES_PER_PX: f64 = 90.0;
const LOSSY_BYTES_PER_PX: f64 = 170.0;

/// The photo size a batch budgets for when the export does not shrink it:
/// a 45 MP body, the largest common one.
const FULL_SIZE_PX: f64 = 45e6;

/// The pixels an export writes at most: the long edge squared when the
/// export resizes by its long edge, otherwise a full-size photo.
fn output_px(resize: Option<&ResizeOptions>) -> f64 {
    match resize {
        Some(ResizeOptions {
            mode: ResizeMode::LongEdge,
            value,
            ..
        }) => (*value as f64 * *value as f64).min(FULL_SIZE_PX),
        _ => FULL_SIZE_PX,
    }
}

/// The encoder's memory for one photo, added to a batch's per-photo budget:
/// nothing for formats other than JPEG XL. Quality 100 is lossless.
// RapidRAW 892bfa30: src-tauri/src/export_processing.rs (encoder_bytes_per_pixel)
pub fn encoder_bytes_per_photo(
    output_format: &str,
    quality: u8,
    resize: Option<&ResizeOptions>,
) -> f64 {
    if !output_format.eq_ignore_ascii_case("jxl") {
        return 0.0;
    }
    let bytes_per_px = if quality == 100 {
        LOSSLESS_BYTES_PER_PX
    } else {
        LOSSY_BYTES_PER_PX
    };
    bytes_per_px * output_px(resize)
}

/// Adds an XMP packet to an encoded JPEG XL: a bare codestream is wrapped in
/// a container first.
// RapidRAW 898e4775: src-tauri/src/metadata/export_metadata.rs (embed_jxl_xmp)
pub fn embed_xmp(image_bytes: &mut Vec<u8>, packet: &str) {
    if jxl_encoder::container::is_bare_codestream(image_bytes) {
        *image_bytes =
            jxl_encoder::container::wrap_in_container(image_bytes, None, Some(packet.as_bytes()));
        return;
    }

    if !jxl_encoder::container::is_container(image_bytes) {
        return;
    }

    let Ok(box_size) = u32::try_from(8 + packet.len()) else {
        log::warn!("Skipping XMP metadata: packet exceeds JXL box size");
        return;
    };

    image_bytes.extend_from_slice(&box_size.to_be_bytes());
    image_bytes.extend_from_slice(b"xml ");
    image_bytes.extend_from_slice(packet.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use jxl_encoder::PixelLayout;

    fn encoded() -> Vec<u8> {
        let rgb: Vec<u8> = (0..16 * 16 * 3).map(|i| (i * 7 % 256) as u8).collect();
        lossless()
            .encode(&rgb, 16, 16, PixelLayout::Rgb8)
            .expect("encodes")
    }

    /// The export's own metadata step on a JPEG XL: the source's EXIF and
    /// the photo's rating both arrive, and the picture still decodes.
    #[test]
    fn keep_metadata_writes_exif_and_xmp_into_jpeg_xl() {
        use little_exif::{exif_tag::ExifTag, filetype::FileExtension, metadata::Metadata};

        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.jpg");
        let mut jpeg = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(8, 8, image::Rgb([9, 9, 9])))
            .write_to(
                &mut std::io::Cursor::new(&mut jpeg),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        let mut exif = Metadata::new();
        exif.set_tag(ExifTag::Make("TestCam Industries".to_string()));
        exif.write_to_vec(&mut jpeg, FileExtension::JPEG).unwrap();
        std::fs::write(&source, &jpeg).unwrap();

        let rated = crate::image_processing::ImageMetadata {
            rating: 4,
            ..Default::default()
        };
        let mut bytes = encoded();
        crate::mods::export_metadata::write_export_metadata(
            &mut bytes,
            source.to_str().unwrap(),
            "jxl",
            true,
            false,
            &crate::exif_processing::ExportXmp::from_metadata(&rated),
            None,
        )
        .unwrap();

        let written = Metadata::new_from_vec(&bytes, FileExtension::JXL).unwrap();
        let make = written
            .get_tag(&ExifTag::Make(String::new()))
            .next()
            .cloned();
        assert_eq!(make, Some(ExifTag::Make("TestCam Industries".to_string())));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("xmp:Rating"), "no XMP rating in the JPEG XL");
        jxl_oxide::JxlImage::builder()
            .read(std::io::Cursor::new(&bytes))
            .expect("still decodes");
    }

    #[test]
    fn lossless_stays_lossless_without_tree_learning() {
        let rgb: Vec<u8> = (0..32 * 24 * 3).map(|i| (i * 13 % 251) as u8).collect();
        let bytes = lossless()
            .encode(&rgb, 32, 24, PixelLayout::Rgb8)
            .expect("encodes");
        let image = jxl_oxide::JxlImage::builder()
            .read(std::io::Cursor::new(&bytes))
            .expect("decodes");
        let render = image.render_frame(0).expect("renders");
        let decoded: Vec<u8> = render
            .image_all_channels()
            .buf()
            .iter()
            .map(|v| (v * 255.0).round() as u8)
            .collect();
        assert_eq!(decoded, rgb);
    }

    #[test]
    fn xmp_wraps_a_bare_codestream_and_follows_a_container() {
        let packet = "<x:xmpmeta>rating</x:xmpmeta>";
        let mut bytes = encoded();
        assert!(jxl_encoder::container::is_bare_codestream(&bytes));
        embed_xmp(&mut bytes, packet);
        assert!(jxl_encoder::container::is_container(&bytes));
        assert!(bytes.windows(packet.len()).any(|w| w == packet.as_bytes()));

        let before = bytes.len();
        embed_xmp(&mut bytes, packet);
        assert_eq!(bytes.len(), before + 8 + packet.len());
        assert_eq!(&bytes[before + 4..before + 8], b"xml ");
    }

    #[test]
    fn only_jpeg_xl_adds_encoder_memory() {
        assert_eq!(encoder_bytes_per_photo("jpeg", 100, None), 0.0);
        assert_eq!(encoder_bytes_per_photo("JXL", 100, None), 90.0 * 45e6);
        assert_eq!(encoder_bytes_per_photo("jxl", 90, None), 170.0 * 45e6);
        let small = ResizeOptions {
            mode: ResizeMode::LongEdge,
            value: 2048,
            dont_enlarge: true,
        };
        assert_eq!(
            encoder_bytes_per_photo("jxl", 100, Some(&small)),
            90.0 * 2048.0 * 2048.0
        );
    }
}
